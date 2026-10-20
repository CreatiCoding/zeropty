//! The engine inside a host process. It has a thread of its own, where it reads sessions' output, parses
//! and freezes screens, and runs the commands that allocate for long (new-session, attach...): one
//! thread doing the allocating keeps one allocator heap (commands all run on the caller's thread left
//! 400 idle sessions at 1.2 MB instead of 0.7 MB). The rest (reading a screen with nothing pending,
//! sending keys, lookups) runs on the caller's thread holding the engine, so it does not wait for the
//! engine's thread to wake (~0.1 ms when it slept, a command's whole cost otherwise).
//!
//! Two ways to run it: `Embedded` (a thread of its own; commands from any thread) and `Polled` (no
//! thread: the host's event loop calls it when its fd turns readable or its timeout passes). Either can
//! also answer the socket protocol (`listen`, crate::server). Both run their turns through
//! crate::host::HostLoop, and both are what the `Frontend` trait asks of an embedded engine, which is
//! all the bindings (bindings/c, bindings/node) call.
//!
//! Either keeps a plugin registry (crate::plugin), run by its HostLoop: `add_plugin` starts one,
//! `command` and the socket route the plugins' commands, `command_start` hands one back in flight. With
//! no plugin, a command pays one atomic load for them, and the registry holds no fd and no thread.

use crate::engine::{Change, Engine, Event, HOST_TAG, Interest, Kqueue, Options, Reply};
use crate::host::{HostLoop, Source, Waiting, Yield};
use crate::plugin::{Call, Due, Plugin, PluginError, Plugins};
use crate::server::{Hooks, Server};
use std::os::fd::{AsFd, AsRawFd as _, BorrowedFd, RawFd};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// What a front end (the C ABI, the Node module) asks of an embedded engine, however it is run: the
/// calls that do not depend on who drives the loop.
///
/// Code written once for both ways of running the engine:
///
/// ```
/// use std::sync::Mutex;
/// use zeropty::embed::{Embedded, Frontend, Polled};
/// use zeropty::engine::Options;
///
/// fn sessions(zp: &impl Frontend) -> String {
///     zp.command(&["list-sessions".to_string(), "-F".into(), "#{session_name}".into()], "/").out
/// }
/// let threaded = Embedded::start(Options::default());
/// let polled = Mutex::new(Polled::start(Options::default()));
/// assert_eq!(sessions(&threaded), sessions(&polled)); // no sessions in either
/// ```
pub trait Frontend {
    /// Runs one tmux-style command (`argv[0]` is the command); see Engine::command.
    fn command(&self, argv: &[String], cwd: &str) -> Reply;
    /// Answers commands on a socket at `path` too (crate::server).
    fn listen(&self, path: &Path) -> std::io::Result<()>;
    /// `command` without waiting for a plugin: a plugin's command comes back in flight, anything else
    /// answered already. By default, everything answered at once.
    fn command_start(&self, argv: &[String], cwd: &str) -> Call {
        Call::ready(self.command(argv, cwd))
    }
    /// Starts a plugin (crate::plugin) on this engine. By default, `PluginError::Unsupported`: a front
    /// end written before plugins keeps compiling and simply has none.
    fn add_plugin_boxed(&self, _plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        Err(PluginError::Unsupported)
    }
}

impl Frontend for Embedded {
    fn command(&self, argv: &[String], cwd: &str) -> Reply {
        Embedded::command(self, argv, cwd)
    }
    fn listen(&self, path: &Path) -> std::io::Result<()> {
        Embedded::listen(self, path)
    }
    fn command_start(&self, argv: &[String], cwd: &str) -> Call {
        Embedded::command_start(self, argv, cwd)
    }
    fn add_plugin_boxed(&self, plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        Embedded::add_plugin_boxed(self, plugin)
    }
}

/// A polled engine shared between threads that take turns (the C ABI's): each call holds the lock.
impl Frontend for Mutex<Polled> {
    fn command(&self, argv: &[String], cwd: &str) -> Reply {
        self.lock().unwrap().command(argv, cwd)
    }
    fn listen(&self, path: &Path) -> std::io::Result<()> {
        self.lock().unwrap().listen(path)
    }
    fn command_start(&self, argv: &[String], cwd: &str) -> Call {
        self.lock().unwrap().command_start(argv, cwd)
    }
    fn add_plugin_boxed(&self, plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        self.lock().unwrap().add_plugin_boxed(plugin)
    }
}

/// The loop's own event: commands are waiting, or it is told to stop.
const WAKE: usize = HOST_TAG | 1;
/// The EVFILT_USER ident of WAKE.
const WAKE_IDENT: usize = 1;

/// Work for the engine's thread, with the loop's server slot (a job may start listening).
type Job = Box<dyn FnOnce(&mut Engine, &mut Option<Server>) + Send>;

/// The engine on a thread of its own.
///
/// ```
/// use zeropty::{embed::Embedded, engine::Options};
///
/// let zp = Embedded::start(Options::default());
/// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// let r = zp.command(&argv(&["new-session", "-d", "-s", "doc-embedded", "exec cat"]), "/");
/// assert_eq!(r.code, 0, "{}", r.err);
/// assert_eq!(zp.command(&argv(&["has-session", "-t", "=doc-embedded"]), "/").code, 0);
/// zp.command(&argv(&["kill-server"]), "/");
/// ```
pub struct Embedded {
    inner: Arc<Inner>,
    thread: Option<std::thread::JoinHandle<()>>,
}

struct Inner {
    engine: Mutex<Engine>,
    /// Threads waiting for `engine`: the loop lets go between reads while any are (a capture otherwise
    /// waited out a batch of 32 reads, ~0.1-0.3 ms under load).
    waiting: AtomicUsize,
    /// The engine's queue, a handle of its own, to wait on and wake without holding the engine.
    kq: Kqueue,
    jobs: Mutex<Jobs>,
    /// Run by the loop after each turn (nothing to do while none is added).
    plugins: Plugins,
}

#[derive(Default)]
struct Jobs {
    queue: Vec<Job>,
    stop: bool,
}

impl Inner {
    /// Wakes the loop. Fails only if the queue is gone, with the engine.
    fn wake(&self) {
        let _ = self.kq.trigger(WAKE_IDENT, WAKE);
    }
}

impl Embedded {
    /// Starts the engine and its thread.
    ///
    /// # Panics
    /// When no kqueue can be made, or no thread started (the process is out of either).
    ///
    /// Sessions live as long as this process; dropping the `Embedded` stops the thread, and the sessions'
    /// programs run on until they end (send `kill-server` first to end them).
    ///
    /// ```
    /// use zeropty::{embed::Embedded, engine::Options};
    ///
    /// let zp = Embedded::start(Options { history: 2000, ..Options::default() });
    /// assert_eq!(zp.command(&["list-sessions".to_string()], "/").code, 0);
    /// drop(zp); // the thread stops here
    /// ```
    pub fn start(opts: Options) -> Self {
        let engine = Engine::new(opts);
        let kq = engine.poller().try_clone().expect("zeropty: a handle on the engine's kqueue");
        kq.apply(&[Change::new(WAKE_IDENT as RawFd, Interest::User, WAKE)]).expect("zeropty: the engine's wake event");
        let inner =
            Arc::new(Inner { engine: Mutex::new(engine), waiting: AtomicUsize::new(0), kq, jobs: Mutex::new(Jobs::default()), plugins: Plugins::new() });
        let loop_inner = inner.clone();
        let thread = std::thread::Builder::new()
            .name("zeropty".into())
            // Parsing needs little stack; the default 2 MB would only be address space, but say so.
            .stack_size(256 << 10)
            .spawn(move || {
                // Woken on a performance core at once when work comes.
                crate::sys::process::set_interactive_qos();
                run(&loop_inner)
            })
            .expect("zeropty thread");
        Self { inner, thread: Some(thread) }
    }

    /// Runs `f` on the engine's thread and returns what it returns.
    ///
    /// # Example
    ///
    /// Calls beyond [`command`](Self::command), such as the engine's own methods:
    ///
    /// ```
    /// use zeropty::{embed::Embedded, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let zp = Embedded::start(Options::default());
    /// zp.command(&argv(&["new-session", "-d", "-s", "doc-with", "exec cat"]), "/");
    ///
    /// let names = zp.with(|engine| engine.session_names());
    /// assert_eq!(names, ["doc-with"]);
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn with<R: Send + 'static>(&self, f: impl FnOnce(&mut Engine) -> R + Send + 'static) -> R {
        self.with_server(move |e, _| f(e))
    }

    /// `with`, with the loop's server slot too.
    fn with_server<R: Send + 'static>(&self, f: impl FnOnce(&mut Engine, &mut Option<Server>) -> R + Send + 'static) -> R {
        let done = Arc::new((Mutex::new(None), Condvar::new()));
        let back = done.clone();
        self.inner.jobs.lock().unwrap().queue.push(Box::new(move |e: &mut Engine, s: &mut Option<Server>| {
            let r = f(e, s);
            *back.0.lock().unwrap() = Some(r);
            back.1.notify_one();
        }));
        self.inner.wake();
        let mut slot = done.0.lock().unwrap();
        while slot.is_none() {
            slot = done.1.wait(slot).unwrap();
        }
        slot.take().unwrap()
    }

    /// Answers commands on a socket at `path` too (crate::server): the zeropty CLI, attach and the socket
    /// clients reach this engine's sessions, with no server process. Its connections are served on the
    /// engine's thread.
    ///
    /// # Example
    ///
    /// A socket client (here one JSON line each way; `ZEROPTY_SOCK=<path> zeropty ...` works the same) sees
    /// the sessions this process made:
    ///
    /// ```
    /// use std::io::{BufRead as _, BufReader, Write as _};
    /// use zeropty::{embed::Embedded, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let path = std::env::temp_dir().join(format!("zeropty-doc-listen-{}.sock", std::process::id()));
    /// let zp = Embedded::start(Options::default());
    /// zp.listen(&path).expect("listen");
    /// zp.command(&argv(&["new-session", "-d", "-s", "doc-listen", "exec cat"]), "/");
    ///
    /// let mut sock = std::os::unix::net::UnixStream::connect(&path).unwrap();
    /// writeln!(sock, r##"{{"argv":["list-sessions","-F","#{{session_name}}"],"cwd":"/"}}"##).unwrap();
    /// let mut line = String::new();
    /// BufReader::new(&sock).read_line(&mut line).unwrap();
    /// assert_eq!(line, "{\"code\":0,\"out\":\"doc-listen\\n\",\"err\":\"\"}\n");
    ///
    /// zp.command(&argv(&["kill-server"]), "/");
    /// drop(zp);
    /// let _ = std::fs::remove_file(&path);
    /// ```
    pub fn listen(&self, path: &Path) -> std::io::Result<()> {
        let path = path.to_owned();
        // Programs may attach as plugins through it: they wake the loop like plugins added here.
        self.attach_plugins()?;
        let plugins = self.inner.plugins.clone();
        self.with_server(move |e, server| {
            let mut s = Server::bind(e, &path, Hooks::default())?;
            s.set_plugins(plugins);
            *server = Some(s);
            Ok(())
        })
    }

    /// Lets the plugins wake the loop (once: a handle on the engine's queue).
    fn attach_plugins(&self) -> std::io::Result<()> {
        if self.inner.plugins.is_attached() {
            return Ok(());
        }
        self.inner.plugins.attach_queue(&self.inner.kq)
    }

    /// Starts a plugin (crate::plugin) on a thread of its own: it hears this engine's sessions, and its
    /// commands are answered by `command` (and by the socket once `listen` was called). It hears every
    /// event from the moment this returns.
    pub fn add_plugin(&self, plugin: impl Plugin) -> Result<(), PluginError> {
        self.add_plugin_boxed(Box::new(plugin))
    }

    /// `add_plugin` for a plugin chosen at run time.
    pub fn add_plugin_boxed(&self, plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        self.attach_plugins().map_err(|e| PluginError::Thread(format!("the engine's queue: {e}")))?;
        self.inner.plugins.add_boxed(plugin)?;
        // Told the events it wants before this returns, so none made after is missed.
        let plugins = self.inner.plugins.clone();
        self.with(move |e| plugins.pump(e));
        Ok(())
    }

    /// This engine's plugins: remove one, list them, set their deadline.
    pub fn plugins(&self) -> &Plugins {
        &self.inner.plugins
    }

    /// Runs one tmux-style command (`argv[0]` is the command); see Engine::command. A plugin's command
    /// is answered by the plugin, this thread waiting for it up to the deadline (`command_start` does
    /// not wait).
    ///
    /// # Example
    ///
    /// The [`Reply`] is what the `zeropty` CLI would exit with and print:
    ///
    /// ```
    /// use zeropty::{embed::Embedded, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let zp = Embedded::start(Options::default());
    ///
    /// let r = zp.command(&argv(&["new-session", "-d", "-P", "-F", "#{session_name}", "-s", "doc-command", "exec cat"]), "/");
    /// assert_eq!((r.code, r.out.as_str()), (0, "doc-command\n"));
    ///
    /// // Failures carry tmux's exit code and message.
    /// let r = zp.command(&argv(&["send-keys", "-t", "=nope:", "Enter"]), "/");
    /// assert_eq!((r.code, r.err.as_str()), (1, "can't find session: nope\n"));
    ///
    /// // Threads share the engine: here two read the same screen at once.
    /// std::thread::scope(|s| {
    ///     for _ in 0..2 {
    ///         s.spawn(|| assert_eq!(zp.command(&argv(&["capture-pane", "-p", "-t", "=doc-command:"]), "/").code, 0));
    ///     }
    /// });
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn command(&self, argv: &[String], cwd: &str) -> Reply {
        // One atomic load while no plugin adds commands.
        if let Some(call) = self.inner.plugins.call(argv, cwd, false) {
            return call.wait();
        }
        self.inner.waiting.fetch_add(1, Ordering::SeqCst);
        let (fast, viewed) = {
            let mut e = self.inner.engine.lock().unwrap();
            let r = e.command_fast(argv, cwd);
            // Session events for plugins (a capture read output) are handed over by the loop as well.
            (r, e.has_viewer_events() || e.has_tap_events())
        };
        self.inner.waiting.fetch_sub(1, Ordering::SeqCst);
        if viewed {
            // It read output an attached terminal is waiting for: the loop passes it on.
            self.inner.wake();
        }
        if let Some(r) = fast {
            // Filter changes it made (a read filter re-armed) are applied when the loop next waits.
            return r;
        }
        // Not the engine's: the registry's list (the slow path, a thread hop anyway).
        if argv.first().is_some_and(|c| c == "list-plugins") {
            return self.inner.plugins.list();
        }
        let (argv, cwd) = (argv.to_vec(), cwd.to_owned());
        self.with(move |e| e.command(&argv, &cwd))
    }

    /// `command` without waiting for a plugin: a plugin's command comes back in flight (poll the Call,
    /// or `wait` on it from a thread that may block), anything else answered already.
    pub fn command_start(&self, argv: &[String], cwd: &str) -> Call {
        match self.inner.plugins.call(argv, cwd, false) {
            Some(call) => call,
            None => Call::ready(self.command(argv, cwd)),
        }
    }
}

impl Drop for Embedded {
    /// Stops the thread. Sessions are left running (their pipes close with the engine, so programs see
    /// EOF); a host that wants them ended runs `kill-server` first.
    fn drop(&mut self) {
        self.inner.plugins.shutdown();
        self.inner.jobs.lock().unwrap().stop = true;
        self.inner.wake();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The engine's thread: waits on the queue without the engine, so commands on other threads are not
/// held up meanwhile, then takes a turn with it.
fn run(inner: &Inner) {
    let mut events = vec![Event::EMPTY; 64];
    let mut host = HostLoop::new(None, Waiting::Apart);
    host.set_plugins(inner.plugins.clone());
    loop {
        let wait = {
            let mut e = inner.engine.lock().unwrap();
            // Changes a command made on another thread since the last turn.
            e.apply_changes();
            host.timeout_ms(&e)
        };
        // No changes go with this wait (they were applied above), so a failure is the queue's own: then
        // a turn with no events, after a pause, that runs the jobs anyway (their wake-up may be lost).
        let (n, failed) = match inner.kq.wait(&[], &mut events, wait) {
            Ok(n) => (n, false),
            Err(err) => {
                report_queue_failure(&err);
                std::thread::sleep(QUEUE_RETRY);
                (0, true)
            }
        };
        let mut e = inner.engine.lock().unwrap();
        let mut stop = false;
        let waiting = || inner.waiting.load(Ordering::Relaxed) > 0;
        // Source::Events: nothing to wait for, so the turn cannot fail.
        let _ = host.turn(&mut e, Source::Events(&events[..n]), Yield::When(&waiting), |e, server, own| {
            // The wake event: jobs are queued, or the loop is to stop.
            if own.is_empty() && !failed {
                return;
            }
            let jobs = std::mem::take(&mut inner.jobs.lock().unwrap().queue);
            for job in jobs {
                job(e, server);
            }
            stop = inner.jobs.lock().unwrap().stop;
        });
        if stop {
            return;
        }
    }
}

/// How long a host pauses after its queue failed, so a queue that keeps failing is no busy loop.
const QUEUE_RETRY: Duration = Duration::from_millis(100);

/// Says on stderr that the engine's queue failed (a bug or a process out of resources: the kernel
/// refused the wait itself, not one of its changes).
fn report_queue_failure(err: &std::io::Error) {
    eprintln!("zeropty: the engine's event queue failed: {err}")
}

/// How much of a burst a polled engine reads ahead of parsing at once, and how long one `process` call
/// goes on reading: the host's loop turns between slices (unsliced, 5.4 MB held Node's loop 11-32 ms;
/// sliced, 3-6 ms, the 5.4 MB on screen as soon).
const POLLED_SLURP: usize = 256 << 10;
const POLLED_BUDGET: Duration = Duration::from_millis(2);

/// The engine run by its host's own event loop, with no thread of its own: the host watches `fd()`
/// (a kqueue: readable when the engine has events) and calls `process` when it is readable or
/// `timeout_ms()` passed. Saves the thread's allocator heap, stack and page tables (~0.2 MB). Not for
/// commands from several threads: the host's loop thread runs everything.
///
/// # Example
///
/// A host loop: wait until `fd()` is readable or `timeout_ms()` passed, then `process`. Here it is
/// `poll(2)` on the engine's fd alone; a real host adds its own fds to the same wait (or registers
/// `fd()` with its event loop, as the Node module does with libuv).
///
/// ```
/// use zeropty::{embed::Polled, engine::Options};
///
/// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// let mut zp = Polled::start(Options::default());
/// let r = zp.command(&argv(&["new-session", "-d", "-s", "doc-polled", "-x", "40", "-y", "4", "exec cat"]), "/");
/// assert_eq!(r.code, 0, "{}", r.err);
/// zp.command(&argv(&["send-keys", "-t", "=doc-polled:", "-l", "hi"]), "/");
/// zp.command(&argv(&["send-keys", "-t", "=doc-polled:", "Enter"]), "/");
///
/// // Turn the loop until cat has echoed the line back (the engine reads it in process).
/// let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
/// let mut screen = String::new();
/// while std::time::Instant::now() < deadline {
///     let mut p = libc::pollfd { fd: zp.fd(), events: libc::POLLIN, revents: 0 };
///     unsafe { libc::poll(&mut p, 1, zp.timeout_ms().min(50) as i32) };
///     zp.process();
///     screen = zp.command(&argv(&["capture-pane", "-p", "-t", "=doc-polled:"]), "/").out;
///     if screen.starts_with("hi\nhi\n") { break }
/// }
/// assert!(screen.starts_with("hi\nhi\n"), "{screen:?}");
/// zp.command(&argv(&["kill-server"]), "/");
/// ```
///
/// Waiting for text without polling the screen: [`Engine::wait_start`](crate::engine::Engine::wait_start).
pub struct Polled {
    engine: Engine,
    host: HostLoop,
    plugins: Plugins,
    /// Deadlines (crate::plugin::now_ms) of the calls `command_start` handed out: `timeout_ms` is due by
    /// the earliest still waiting, so a host polls a call that timed out on time. One answered or
    /// dropped counts no more (it would wake the loop for nothing), and goes with past ones in `process`.
    dues: Vec<Due>,
}

impl Polled {
    /// A burst is read POLLED_SLURP at a time (Options::slurp_max): parsing it takes ~1 ms, then the host's loop
    /// turns before the next slice.
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let mut zp = Polled::start(Options::default()); // no thread: nothing runs until `process`
    /// assert_eq!(zp.command(&["list-sessions".to_string()], "/").code, 0);
    /// ```
    pub fn start(opts: Options) -> Self {
        let opts = Options { slurp_max: opts.slurp_max.min(POLLED_SLURP), ..opts };
        let plugins = Plugins::new();
        let mut host = HostLoop::new(None, Waiting::Apart);
        host.set_plugins(plugins.clone());
        Self { engine: Engine::new(opts), host, plugins, dues: Vec::new() }
    }

    /// The fd to watch for reading (`as_fd` borrows it).
    ///
    /// ```
    /// let zp = zeropty::embed::Polled::start(Default::default());
    /// assert!(zp.fd() >= 0); // register it with the host's loop (kqueue, poll, libuv ...) for reading
    /// ```
    pub fn fd(&self) -> RawFd {
        self.engine.as_fd().as_raw_fd()
    }

    /// Milliseconds after which `process` is due even if `fd()` did not turn readable.
    /// A command started with `command_start` counts too: its deadline makes `process` due.
    ///
    /// # Example
    ///
    /// The wait of one turn of the host's loop, clamped to what `poll(2)` takes:
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// let wait_ms = zp.timeout_ms().min(i32::MAX as u64) as i32;
    /// let mut p = libc::pollfd { fd: zp.fd(), events: libc::POLLIN, revents: 0 };
    /// let ready = unsafe { libc::poll(&mut p, 1, wait_ms.min(20)) };
    /// assert_eq!(ready, 0); // no sessions: nothing to read, the wait times out
    /// zp.process(); // due either way: the engine's timers run here
    /// ```
    pub fn timeout_ms(&self) -> u64 {
        // The plugins' watchdog is the HostLoop's to count; a started call gives up at its deadline, due
        // then to be polled.
        let t = self.host.timeout_ms(&self.engine);
        self.next_due().map_or(t, |d| t.min(d.saturating_sub(crate::plugin::now_ms())))
    }

    /// The earliest deadline of a started call still waiting for its reply.
    fn next_due(&self) -> Option<u64> {
        if self.dues.is_empty() {
            return None;
        }
        self.dues.iter().filter(|d| d.open()).map(|d| d.at).min()
    }

    /// Answers commands on a socket at `path` too (crate::server), served by `process`: the zeropty CLI,
    /// attach and the socket clients reach this engine's sessions, with no server process.
    ///
    /// ```
    /// use std::io::{BufRead as _, BufReader, Write as _};
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let path = std::env::temp_dir().join(format!("zeropty-doc-polled-listen-{}.sock", std::process::id()));
    /// let mut zp = Polled::start(Options::default());
    /// zp.listen(&path)?;
    /// let mut sock = std::os::unix::net::UnixStream::connect(&path)?;
    /// writeln!(sock, r#"{{"argv":["has-session","-t","=none"],"cwd":"/"}}"#)?;
    /// sock.set_nonblocking(true)?;
    /// let (mut reader, mut line) = (BufReader::new(&sock), String::new());
    /// // The connection is served by `process`, in the host's loop.
    /// for _ in 0..200 {
    ///     zp.process();
    ///     if reader.read_line(&mut line).is_ok() && line.ends_with('\n') { break }
    ///     std::thread::sleep(std::time::Duration::from_millis(5));
    /// }
    /// assert!(line.starts_with(r#"{"code":1"#), "{line:?}");
    /// drop(zp);
    /// let _ = std::fs::remove_file(&path);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn listen(&mut self, path: &Path) -> std::io::Result<()> {
        // Programs may attach as plugins through it: they wake the host's loop through fd().
        self.attach_plugins()?;
        let mut s = Server::bind(&self.engine, path, Hooks::default())?;
        s.set_plugins(self.plugins.clone());
        self.host.set_server(s);
        Ok(())
    }

    /// Lets the plugins wake the host's loop (fd() turns readable), once.
    fn attach_plugins(&self) -> std::io::Result<()> {
        if self.plugins.is_attached() {
            return Ok(());
        }
        self.plugins.attach(&self.engine)
    }

    /// Starts a plugin (crate::plugin) on a thread of its own; `process` hands it this engine's events
    /// (a plugin's own wake-ups turn `fd()` readable). It hears every event from the moment this returns.
    /// Its `manifest()` runs on its thread; this waits for it 1 s at most (`Plugins::add_boxed`).
    pub fn add_plugin(&mut self, plugin: impl Plugin) -> Result<(), PluginError> {
        self.add_plugin_boxed(Box::new(plugin))
    }

    /// `add_plugin` for a plugin chosen at run time.
    pub fn add_plugin_boxed(&mut self, plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        self.attach_plugins().map_err(|e| PluginError::Thread(format!("the engine's queue: {e}")))?;
        self.plugins.add_boxed(plugin)?;
        self.plugins.pump(&mut self.engine);
        Ok(())
    }

    /// This engine's plugins: remove one, list them, set their deadline.
    pub fn plugins(&self) -> &Plugins {
        &self.plugins
    }

    /// Handles what is waiting, without blocking. Reads stop once they took POLLED_BUDGET; the rest are
    /// due at once, for the next call. A failure of the engine's queue is reported on stderr (the timers
    /// still ran); `try_process` returns it instead.
    ///
    /// # Example
    ///
    /// The loop of [examples/polled.rs](https://github.com/CreatiCoding/zeropty/blob/main/examples/polled.rs),
    /// cut down: wait, process, look at the screen.
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// zp.command(&argv(&["new-session", "-d", "-s", "doc-process", "sh -c 'sleep 0.1; echo build done; exec cat'"]), "/");
    ///
    /// let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    /// let mut screen = String::new();
    /// while !screen.contains("build done") && std::time::Instant::now() < deadline {
    ///     let mut p = libc::pollfd { fd: zp.fd(), events: libc::POLLIN, revents: 0 };
    ///     unsafe { libc::poll(&mut p, 1, zp.timeout_ms().min(100) as i32) };
    ///     zp.process();
    ///     screen = zp.command(&argv(&["capture-pane", "-p", "-t", "=doc-process:"]), "/").out;
    /// }
    /// assert!(screen.starts_with("build done\n"), "{screen:?}");
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn process(&mut self) {
        if let Err(e) = self.try_process() {
            report_queue_failure(&e);
        }
    }

    /// `process`, returning a failure of the engine's queue (the rest of the turn ran all the same).
    ///
    /// ```
    /// let mut zp = zeropty::embed::Polled::start(Default::default());
    /// zp.try_process()?; // nothing waiting: returns at once
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn try_process(&mut self) -> std::io::Result<()> {
        self.process_within(Duration::ZERO)
    }

    /// `try_process`, waiting up to `max` on the engine's queue first if nothing is there yet.
    fn process_within(&mut self, max: Duration) -> std::io::Result<()> {
        let r = self.host.turn(&mut self.engine, Source::Wait(Some(max)), Yield::After(POLLED_BUDGET), |_, _, _| {});
        if !self.dues.is_empty() {
            let now = crate::plugin::now_ms();
            self.dues.retain(|d| d.at > now && d.open());
        }
        r
    }

    /// Runs one tmux-style command without holding the host's thread: a plugin's command comes back in
    /// flight, anything else answered already (as `command` would). Poll the Call after `process`: the
    /// plugin's reply turns `fd()` readable, and `timeout_ms()` counts the call's deadline, so a loop
    /// waiting on both picks the reply (or the timeout) up at once and never spins.
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// let call = zp.command_start(&argv(&["list-sessions"]), "/");
    /// // in the host's loop, whenever fd() is readable or timeout_ms() passed:
    /// zp.process();
    /// let reply = call.poll().expect("not a plugin's command: answered at once");
    /// assert_eq!(reply.code, 0);
    /// ```
    pub fn command_start(&mut self, argv: &[String], cwd: &str) -> Call {
        // With `wake`: the reply triggers the engine's queue (fd() turns readable).
        if let Some(call) = self.plugins.call(argv, cwd, true) {
            // The ones answered or dropped since go first: a host that never calls `process` between
            // calls does not grow the list.
            self.dues.retain(Due::open);
            self.dues.push(call.due());
            return call;
        }
        Call::ready(self.command(argv, cwd))
    }

    /// Runs one tmux-style command; see Engine::command. A plugin's command is waited for here, up to
    /// the deadline (`command_start` does not wait).
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// let r = zp.command(&argv(&["new-session", "-d", "-P", "-F", "#{session_name}", "-s", "doc-polled-cmd", "exec cat"]), "/");
    /// assert_eq!((r.code, r.out.as_str()), (0, "doc-polled-cmd\n"));
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn command(&mut self, argv: &[String], cwd: &str) -> Reply {
        // One atomic load while no plugin adds commands.
        if let Some(call) = self.plugins.call(argv, cwd, true) {
            return self.wait_for(call);
        }
        let r = self.engine.command(argv, cwd);
        self.host.after_command(&mut self.engine);
        // Not the engine's but the registry's (the engine's failure path only, so no cost when it answers).
        if r.code != 0 && argv.first().is_some_and(|c| c == "list-plugins") {
            return self.plugins.list();
        }
        r
    }

    /// A plugin's command, waited for on the host's loop thread up to its deadline. Meanwhile this thread
    /// does what `process` does (sessions read, socket clients served, what plugins ask of the engine
    /// run), so a slow plugin delays only this caller, never the engine. It sleeps on the engine's queue
    /// between turns: the reply wakes it (the call is made with `wake`), as does any work, so the wait
    /// costs no CPU.
    fn wait_for(&mut self, call: Call) -> Reply {
        loop {
            if let Some(r) = call.poll() {
                let _ = self.try_process();
                return r;
            }
            let ms = self.timeout_ms().min(call.due_ms());
            if let Err(e) = self.process_within(Duration::from_millis(ms)) {
                report_queue_failure(&e);
                std::thread::sleep(QUEUE_RETRY.min(Duration::from_millis(call.due_ms().max(1))));
            }
        }
    }

    /// The engine itself, for calls beyond commands (attach, wait-pane).
    ///
    /// # Example
    ///
    /// Waiting for text is one of them: [`Engine::wait_start`](crate::engine::Engine::wait_start) and
    /// [`Engine::wait_check`](crate::engine::Engine::wait_check) have the loop. Lookups need no loop:
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// zp.command(&argv(&["new-session", "-d", "-s", "doc-engine", "exec cat"]), "/");
    /// assert_eq!(zp.engine().session_names(), ["doc-engine"]);
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn engine(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

impl Drop for Polled {
    /// Stops its plugins (each runs `stop`).
    fn drop(&mut self) {
        self.plugins.shutdown()
    }
}

impl AsFd for Polled {
    /// Readable when `process` has work.
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.engine.as_fd()
    }
}

#[cfg(test)]
mod tests {
    //! Both ways of running an embedded engine answer the same through `Frontend`, the bindings' view.

    use super::{Embedded, Frontend, Polled};
    use crate::engine::{Options, ok};
    use std::sync::Mutex;

    fn run(f: &impl Frontend, argv: &[&str]) -> crate::engine::Reply {
        f.command(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "/")
    }

    /// A session's life through `f`: made, typed into, captured, killed.
    fn session_through(f: &impl Frontend, settle: impl Fn()) {
        assert_eq!(run(f, &["new-session", "-d", "-s", "t", "-x", "20", "-y", "3", "cat"]), ok(""));
        assert_eq!(run(f, &["send-keys", "-t", "t", "-l", "hello"]), ok(""));
        let mut screen = String::new();
        for _ in 0..200 {
            settle();
            screen = run(f, &["capture-pane", "-p", "-t", "t"]).out;
            if screen.starts_with("hello") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(screen.starts_with("hello"), "{screen:?}");
        assert_eq!(run(f, &["kill-session", "-t", "t"]), ok(""));
        assert_eq!(run(f, &["has-session", "-t", "t"]).code, 1);
    }

    #[test]
    fn embedded_and_polled_answer_alike() {
        let opts = || Options { size_dir: std::env::temp_dir(), ..Options::default() };
        session_through(&Embedded::start(opts()), || {});
        let polled = Mutex::new(Polled::start(opts()));
        session_through(&polled, || polled.lock().unwrap().try_process().unwrap());
    }

    /// A front end written before plugins (only the methods `Frontend` had then) still compiles: it
    /// has no plugins, and says so instead of dropping the plugin silently; command_start answers at once.
    #[test]
    fn a_frontend_from_before_plugins_still_compiles() {
        struct Old;
        impl Frontend for Old {
            fn command(&self, _: &[String], _: &str) -> crate::engine::Reply {
                ok("old\n")
            }
            fn listen(&self, _: &std::path::Path) -> std::io::Result<()> {
                Ok(())
            }
        }
        struct Quiet;
        impl crate::plugin::Plugin for Quiet {
            fn manifest(&self) -> crate::plugin::Manifest {
                crate::plugin::Manifest::new("quiet")
            }
        }
        assert!(matches!(Old.add_plugin_boxed(Box::new(Quiet)), Err(crate::plugin::PluginError::Unsupported)));
        assert_eq!(Old.command_start(&["x".to_string()], "/").poll(), Some(ok("old\n")));
    }

    /// A started call's deadline bounds `timeout_ms` only while the call waits: answered or dropped, it
    /// no longer counts (it would wake the host's loop at a deadline nobody waits for), and the list
    /// does not grow with calls the host never waited out.
    #[test]
    fn answered_or_dropped_calls_are_not_due() {
        use crate::plugin::{Host, Manifest, Plugin};
        use std::time::{Duration, Instant};
        /// `now` answers at once; `never` holds its thread until the test ends.
        struct Answers(std::sync::mpsc::Receiver<()>);
        impl Plugin for Answers {
            fn manifest(&self) -> Manifest {
                Manifest::new("answers").command("now").command("never")
            }
            fn command(&mut self, argv: &[String], _: &str, _: &Host) -> crate::engine::Reply {
                if argv[0] == "never" {
                    let _ = self.0.recv();
                }
                ok("now\n")
            }
        }
        let (release, held) = std::sync::mpsc::channel();
        let mut p = Polled::start(Options { size_dir: std::env::temp_dir(), ..Options::default() });
        p.add_plugin(Answers(held)).unwrap();
        let start = |p: &mut Polled, cmd: &str| p.command_start(&[cmd.to_string()], "/");

        // Waiting: due by its deadline.
        let call = start(&mut p, "now");
        assert_eq!(p.next_due(), Some(call.due().at));
        let t0 = Instant::now();
        let r = loop {
            p.process();
            if let Some(r) = call.poll() {
                break r;
            }
            assert!(t0.elapsed() < Duration::from_secs(5), "no reply");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(r.out, "now\n");
        // Answered: no longer due, though its deadline is still ahead, and gone after a turn.
        assert_eq!(p.next_due(), None);
        p.process();
        assert!(p.dues.is_empty());

        // Dropped before its reply (`never` keeps the plugin busy, so the second call is still queued).
        let busy = start(&mut p, "never");
        drop(start(&mut p, "now"));
        assert_eq!(p.next_due(), Some(busy.due().at), "only the call still waiting counts");
        drop(busy);
        assert_eq!(p.next_due(), None);
        // Calls started and dropped without a turn between them do not pile up.
        for _ in 0..100 {
            drop(start(&mut p, "now"));
        }
        assert!(p.dues.len() <= 1, "{} dues kept", p.dues.len());
        let _ = release.send(());
    }
}
