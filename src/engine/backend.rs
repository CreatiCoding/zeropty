//! The engine's seams to the OS, as traits: `Poller` (where interests are registered and events waited
//! for), `Clock` (what time it is) and `Programs` (starting the programs sessions run, signalling and
//! reaping them, and the size file they read). `Backend` bundles one of each, and `Engine<B: Backend>`
//! is generic over it, defaulting to the real ones (`System`: a kqueue, the monotonic clock, fork/exec
//! and killpg). Its scheduling (coalesced reads, low-water re-arming, sweeps, kill timers, event
//! generations) and every command then run unchanged against fakes in unit tests (`fake`), with no
//! kqueue, no process and no waiting. All static dispatch: the default engine compiles to the same calls
//! as before the traits.
//!
//! Invariants a `Poller` keeps:
//! - `apply` and `wait` apply changes in the order given; a later change for the same (ident, interest
//!   kind) replaces an earlier one, as kqueue's EV_ADD does.
//! - `wait` never blocks longer than `timeout_ms`, and an interrupted wait returns `Ok(0)`.
//! - A change refused during a `wait` never comes back as an event. `wait` fails either because a
//!   change was refused with no room to say so (the changes after it are then not applied) or because
//!   the queue itself failed; the engine tells the two apart (Engine::try_poll).
//!
//! Invariants a `Programs` keeps:
//! - A program it starts leads a process group of its own, whose id is its pid: `signal_group(pid, ..)`
//!   reaches whatever it started.
//! - Signalling or reaping a process already gone is not an error (the engine does both for programs
//!   that may have ended on their own).

use super::spawn;
use crate::sys::kq::{Change, Event, Kqueue};
use crate::sys::process;
use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;

/// Where interests are registered and events come from: a kqueue for the real engine. Owned by the
/// engine ('static: its command table is a constant of the engine's type).
///
/// ```
/// use zeropty::engine::{Change, Event, Kqueue, Poller};
///
/// fn wait_once(p: &impl Poller, fd: i32) -> std::io::Result<usize> {
///     let mut events = [Event::EMPTY; 4];
///     p.wait(&[Change::read(fd, 1)], &mut events, 10)
/// }
/// let (_tx, rx) = std::os::unix::net::UnixStream::pair()?;
/// assert_eq!(wait_once(&Kqueue::new()?, std::os::fd::AsRawFd::as_raw_fd(&rx))?, 0); // nothing written
/// # Ok::<(), std::io::Error>(())
/// ```
pub trait Poller: 'static {
    /// Applies `changes` now; fails with the first one refused (an exit interest in a process already
    /// gone, a closed fd).
    fn apply(&self, changes: &[Change]) -> io::Result<()>;

    /// Applies `changes`, then waits up to `timeout_ms` for events into `events`; returns how many came.
    fn wait(&self, changes: &[Change], events: &mut [Event], timeout_ms: u64) -> io::Result<usize>;
}

impl Poller for Kqueue {
    #[inline]
    fn apply(&self, changes: &[Change]) -> io::Result<()> {
        Kqueue::apply(self, changes)
    }

    #[inline]
    fn wait(&self, changes: &[Change], events: &mut [Event], timeout_ms: u64) -> io::Result<usize> {
        Kqueue::wait(self, changes, events, timeout_ms)
    }
}

/// The engine's time: monotonic milliseconds, for its timers and quiet spells. Owned by the engine, as
/// a `Poller` is.
///
/// A clock a test moves by hand (an engine built on it with [`Engine::with_backend`](crate::engine::Engine::with_backend)
/// sees time pass only when the test says so):
///
/// ```
/// use std::cell::Cell;
/// use zeropty::engine::Clock;
///
/// struct Manual(Cell<u64>);
/// impl Clock for Manual {
///     fn now_ms(&self) -> u64 { self.0.get() }
/// }
/// let c = Manual(Cell::new(0));
/// c.0.set(300);
/// assert_eq!(c.now_ms(), 300);
/// ```
pub trait Clock: 'static {
    /// Milliseconds since some fixed start; never goes back.
    fn now_ms(&self) -> u64;
}

/// The process's monotonic clock (crate::sys::time::now_ms).
///
/// ```
/// use zeropty::engine::{Clock, SystemClock};
///
/// let a = SystemClock.now_ms();
/// assert!(SystemClock.now_ms() >= a);
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    #[inline]
    fn now_ms(&self) -> u64 {
        crate::sys::time::now_ms()
    }
}

/// A session's program to start (`Programs::spawn`).
///
/// ```
/// use zeropty::engine::{Programs, Spawn, SystemPrograms};
///
/// let size_file = std::env::temp_dir().join(format!("zeropty-doc-spawn-{}", std::process::id()));
/// let p = SystemPrograms.spawn(&Spawn {
///     command: "exit 0",
///     dir: "/",
///     name: "doc-spawn",
///     rows: 24,
///     cols: 80,
///     size_file: &size_file,
///     env: &[],
///     extra_env: vec!["GREETING=hi"],
/// })?;
/// assert!(p.pid > 0);
/// std::thread::sleep(std::time::Duration::from_millis(50));
/// SystemPrograms.reap(p.pid);
/// # Ok::<(), String>(())
/// ```
#[derive(Debug)]
pub struct Spawn<'a> {
    /// A shell command line, run by /bin/sh.
    pub command: &'a str,
    /// Its working directory.
    pub dir: &'a str,
    /// The session's name (ZEROPTY_SESSION).
    pub name: &'a str,
    /// Its terminal's height, as it is told it (LINES, ZEROPTY_ROWS).
    pub rows: u16,
    /// Its terminal's width (COLUMNS, ZEROPTY_COLS).
    pub cols: u16,
    /// Where it reads a new size (ZEROPTY_SIZE_FILE; written only once the session is resized).
    pub size_file: &'a Path,
    /// set-environment -g: set (Some) or removed (None), in order.
    pub env: &'a [(String, Option<String>)],
    /// new-session -e NAME=VALUE.
    pub extra_env: Vec<&'a str>,
}

/// A program just started: its pid (and process group), its input and its output, both non-blocking.
///
/// What [`SystemPrograms`]'s `spawn` returns; see [`Spawn`] for a whole call.
///
/// ```
/// use std::io::Read as _;
/// use zeropty::engine::{Programs, Spawn, SystemPrograms};
///
/// let size_file = std::env::temp_dir().join(format!("zeropty-doc-spawned-{}", std::process::id()));
/// let p = SystemPrograms.spawn(&Spawn { command: "echo out", dir: "/", name: "doc-spawned", rows: 24, cols: 80, size_file: &size_file, env: &[], extra_env: vec![] })?;
/// let mut out = std::fs::File::from(p.out);
/// let mut text = String::new();
/// for _ in 0..100 {
///     let mut b = [0u8; 64];
///     match out.read(&mut b) {
///         Ok(0) => break,
///         Ok(n) => text.push_str(&String::from_utf8_lossy(&b[..n])),
///         Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)), // non-blocking: not yet
///     }
///     if text.contains("out") { break }
/// }
/// assert!(text.starts_with("out"), "{text:?}");
/// SystemPrograms.reap(p.pid);
/// # Ok::<(), String>(())
/// ```
#[derive(Debug)]
pub struct Spawned {
    /// Its pid, which is also its process group's id.
    pub pid: i32,
    /// Where its input is written: zeropty's end of its terminal socket (a dup of `out`).
    pub stdin: OwnedFd,
    /// Where its output is read (stdout and stderr together): zeropty's end of its terminal socket.
    pub out: OwnedFd,
}

/// What the engine does to the programs its sessions run: start them (and pipe-pane's commands),
/// signal their process groups, reap them, describe them, and tell them their size. Owned by the
/// engine, as a `Poller` is. Errors from starting are a reply's text (`spawn: ...\n`).
///
/// ```
/// use zeropty::engine::{Programs, SystemPrograms};
///
/// // What list-sessions shows as a session's program and directory comes from here.
/// let me = std::process::id() as i32;
/// assert!(!SystemPrograms.name(me).is_empty());
/// assert!(!SystemPrograms.cwd(me).is_empty());
/// ```
pub trait Programs: 'static {
    /// Starts a session's program.
    fn spawn(&self, program: &Spawn<'_>) -> Result<Spawned, String>;

    /// Starts pipe-pane's `command` reading a socket; returns its pid and our end of the socket
    /// (non-blocking; writing to it after the command ended fails instead of raising SIGPIPE).
    fn spawn_pipe(&self, command: &str) -> Result<(i32, OwnedFd), String>;

    /// Sends signal `sig` (libc's numbering) to process group `pgid`; a group already gone is ignored.
    fn signal_group(&self, pgid: i32, sig: i32);

    /// Whether process group `pgid` is `root`'s or a descendant's (a job a shell in the session runs).
    /// Not known (the default): C-c and the like then go to the session's program, as before.
    fn group_under(&self, _pgid: i32, _root: i32) -> bool {
        false
    }

    /// Reaps `pid` if it exited, without waiting.
    fn reap(&self, pid: i32);

    /// The command name of process `pid` (as ps shows it); empty if unknown.
    fn name(&self, pid: i32) -> String;

    /// The working directory of process `pid`; empty if unknown.
    fn cwd(&self, pid: i32) -> String;

    /// Tells a program reading `size_file` its new size (before it gets SIGWINCH). The file is replaced
    /// whole, so the program never reads half of it. A failure leaves the old size there.
    fn write_size(&self, size_file: &Path, rows: u16, cols: u16);

    /// Removes a size file nobody reads any more.
    fn remove_size(&self, size_file: &Path);
}

/// The real programs: fork/exec through /bin/sh, killpg, waitpid, proc_pidinfo, and size files on disk.
///
/// ```
/// use zeropty::engine::{Programs, SystemPrograms};
///
/// let file = std::env::temp_dir().join(format!("zeropty-doc-size-{}", std::process::id()));
/// SystemPrograms.write_size(&file, 30, 100); // what a resized session's program reads
/// assert_eq!(std::fs::read_to_string(&file).unwrap(), "100 30\n");
/// SystemPrograms.remove_size(&file);
/// assert!(!file.exists());
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemPrograms;

impl Programs for SystemPrograms {
    fn spawn(&self, program: &Spawn<'_>) -> Result<Spawned, String> {
        spawn::session(program)
    }

    fn spawn_pipe(&self, command: &str) -> Result<(i32, OwnedFd), String> {
        spawn::pipe_command(command)
    }

    #[inline]
    fn signal_group(&self, pgid: i32, sig: i32) {
        process::signal_group(pgid, sig)
    }

    fn group_under(&self, pgid: i32, root: i32) -> bool {
        process::group_under(pgid, root)
    }

    #[inline]
    fn reap(&self, pid: i32) {
        process::reap(pid)
    }

    fn name(&self, pid: i32) -> String {
        process::name(pid)
    }

    fn cwd(&self, pid: i32) -> String {
        process::cwd(pid)
    }

    fn write_size(&self, size_file: &Path, rows: u16, cols: u16) {
        // Written whole then renamed, so the program never reads half of it.
        let tmp = size_file.with_extension("new");
        if std::fs::write(&tmp, format!("{cols} {rows}\n")).is_ok() {
            let _ = std::fs::rename(&tmp, size_file);
        }
    }

    fn remove_size(&self, size_file: &Path) {
        // Never written (the session was not resized): nothing to remove.
        let _ = std::fs::remove_file(size_file);
    }
}

/// One `Poller`, one `Clock` and one `Programs`: everything an engine needs from outside itself.
///
/// A function for any engine, real or fake:
///
/// ```
/// use zeropty::engine::{Backend, Engine};
///
/// fn count<B: Backend>(e: &Engine<B>) -> usize { e.session_names().len() }
/// assert_eq!(count(&Engine::new(Default::default())), 0);
/// ```
pub trait Backend: 'static {
    /// The event queue.
    type Poller: Poller;
    /// The time.
    type Clock: Clock;
    /// The programs sessions run.
    type Programs: Programs;
}

/// The real OS: a kqueue, the monotonic clock and real processes. The engine's default.
///
/// ```
/// use zeropty::engine::{Engine, System};
///
/// // `Engine` is `Engine<System>`: the two names are one type.
/// let e: Engine<System> = Engine::new(Default::default());
/// let _same: Engine = e;
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct System;

impl Backend for System {
    type Poller = Kqueue;
    type Clock = SystemClock;
    type Programs = SystemPrograms;
}

#[cfg(test)]
pub(crate) mod fake {
    //! A poller that records what it is asked and reports what a test queues, a clock a test moves, and
    //! programs that are socket pairs: nothing runs, every signal and size is recorded.

    use super::{Backend, Clock, Poller, Programs, Spawn, Spawned};
    use crate::sys::kq::{Change, Event, Interest};
    use std::cell::{Cell, RefCell};
    use std::io;
    use std::os::fd::{OwnedFd, RawFd};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};

    /// The fakes together.
    pub(crate) struct Fake;

    impl Backend for Fake {
        type Poller = FakePoller;
        type Clock = FakeClock;
        type Programs = FakePrograms;
    }

    /// Records every change applied (now or with a wait); a wait hands out the events queued.
    #[derive(Default)]
    pub(crate) struct FakePoller {
        pub(crate) applied: RefCell<Vec<Change>>,
        pub(crate) queued: RefCell<Vec<Event>>,
        /// Idents whose exit interest is refused (a process already gone).
        pub(crate) gone: RefCell<Vec<i32>>,
        /// Fds whose changes are refused in a wait with no room to say so (closed meanwhile): the wait
        /// fails with EBADF, as kevent(2) does, and the changes after it are not applied.
        pub(crate) closed: RefCell<Vec<RawFd>>,
        /// The queue itself is broken: every wait fails.
        pub(crate) broken: Cell<bool>,
        /// The timeout of every wait, in order.
        pub(crate) timeouts: RefCell<Vec<u64>>,
    }

    impl FakePoller {
        /// How many waits there were.
        pub(crate) fn waits(&self) -> usize {
            self.timeouts.borrow().len()
        }
    }

    impl Poller for FakePoller {
        fn apply(&self, changes: &[Change]) -> io::Result<()> {
            for c in changes {
                if c.interest() == Interest::Exit && self.gone.borrow().contains(&c.ident()) {
                    return Err(io::Error::from_raw_os_error(libc::ESRCH));
                }
                if c.interest() != Interest::Exit && self.closed.borrow().contains(&c.ident()) {
                    return Err(io::Error::from_raw_os_error(libc::EBADF));
                }
                self.applied.borrow_mut().push(*c);
            }
            Ok(())
        }

        fn wait(&self, changes: &[Change], events: &mut [Event], timeout_ms: u64) -> io::Result<usize> {
            self.timeouts.borrow_mut().push(timeout_ms);
            if self.broken.get() {
                return Err(io::Error::from_raw_os_error(libc::EBADF));
            }
            self.apply(changes)?;
            let mut q = self.queued.borrow_mut();
            let n = q.len().min(events.len());
            for (slot, ev) in events.iter_mut().zip(q.drain(..n)) {
                *slot = ev;
            }
            Ok(n)
        }
    }

    /// A clock that stands still until a test moves it.
    #[derive(Default)]
    pub(crate) struct FakeClock(pub(crate) Cell<u64>);

    impl FakeClock {
        pub(crate) fn advance(&self, ms: u64) {
            self.0.set(self.0.get() + ms)
        }
    }

    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0.get()
        }
    }

    /// A program started by `FakePrograms::spawn`: the other ends of its session's pipes.
    pub(crate) struct FakeProgram {
        pub(crate) pid: i32,
        pub(crate) command: String,
        /// Reads what the session sends the program.
        pub(crate) input: UnixStream,
        /// What the program prints, for the session to read.
        pub(crate) output: UnixStream,
    }

    /// Programs that are socket pairs. Pids count down from i32::MAX, far from any real process.
    #[derive(Default)]
    pub(crate) struct FakePrograms {
        pub(crate) started: RefCell<Vec<FakeProgram>>,
        pub(crate) signals: RefCell<Vec<(i32, i32)>>,
        /// (job's group, session's program): the jobs that are under a session.
        pub(crate) groups: RefCell<Vec<(i32, i32)>>,
        pub(crate) reaped: RefCell<Vec<i32>>,
        pub(crate) sizes: RefCell<Vec<(PathBuf, u16, u16)>>,
        pub(crate) removed: RefCell<Vec<PathBuf>>,
        next: Cell<i32>,
    }

    impl FakePrograms {
        /// A pid no process has.
        pub(crate) fn pid(&self) -> i32 {
            self.next.set(self.next.get() + 1);
            i32::MAX - self.next.get()
        }

        /// The signals sent to `pgid`, in order.
        pub(crate) fn signals_to(&self, pgid: i32) -> Vec<i32> {
            self.signals.borrow().iter().filter(|s| s.0 == pgid).map(|s| s.1).collect()
        }
    }

    fn pair() -> (UnixStream, UnixStream) {
        let (a, b) = crate::sys::fd::socket_pair().unwrap();
        a.set_nonblocking(true).unwrap();
        b.set_nonblocking(true).unwrap();
        (a, b)
    }

    impl Programs for FakePrograms {
        fn spawn(&self, p: &Spawn<'_>) -> Result<Spawned, String> {
            if p.command == "fail" {
                return Err("spawn: refused\n".into());
            }
            let ((stdin, input), (out, output)) = (pair(), pair());
            let pid = self.pid();
            self.started.borrow_mut().push(FakeProgram { pid, command: p.command.into(), input, output });
            Ok(Spawned { pid, stdin: stdin.into(), out: out.into() })
        }

        fn spawn_pipe(&self, command: &str) -> Result<(i32, OwnedFd), String> {
            let (ours, theirs) = pair();
            let pid = self.pid();
            self.started.borrow_mut().push(FakeProgram { pid, command: command.into(), input: theirs, output: pair().0 });
            Ok((pid, ours.into()))
        }

        fn signal_group(&self, pgid: i32, sig: i32) {
            self.signals.borrow_mut().push((pgid, sig))
        }

        fn group_under(&self, pgid: i32, root: i32) -> bool {
            self.groups.borrow().contains(&(pgid, root))
        }

        fn reap(&self, pid: i32) {
            self.reaped.borrow_mut().push(pid)
        }

        fn name(&self, _pid: i32) -> String {
            "fake".into()
        }

        fn cwd(&self, _pid: i32) -> String {
            "/fake".into()
        }

        fn write_size(&self, size_file: &Path, rows: u16, cols: u16) {
            self.sizes.borrow_mut().push((size_file.into(), rows, cols))
        }

        fn remove_size(&self, size_file: &Path) {
            self.removed.borrow_mut().push(size_file.into())
        }
    }
}
