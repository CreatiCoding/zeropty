//! The socket in front of an engine: tmux-style commands as JSON lines, attached terminals, wait-pane and
//! the handing over of sessions. The `zeropty serve` process is this plus an engine; an embedded engine
//! (crate::embed) can take one too (`listen`), so `zeropty send-keys` and attach reach the host's sessions
//! with no server process of its own.
//!
//! It holds only the connections: every call is handed the engine, on the thread running its loop. Its
//! fds are on the engine's kqueue, tagged so the host can tell their events from its own (`owns`). The
//! bytes on the wire are crate::protocol's; this module is the I/O around them.
//!
//! Plugins (crate::plugin): given the host's registry (`set_plugins`), it answers `list-plugins`, routes
//! the plugins' commands (a client waiting for one holds only itself: its reply comes back through a
//! `Call` the loop polls), and lets a connection become a plugin (`plugin-attach`), served from then on
//! by the plugin's threads, off the loop. Without a registry, or with no plugin adding commands, a
//! command pays one branch for them.
//!
//! Invariants: a connection never blocks the loop (non-blocking, with its own input and output
//! buffers); replies go out in the order their commands came; a connection's tag carries its index and
//! generation, so events for a closed one never reach the next in its place.
//!
//! # Example
//!
//! An embedded engine answering on a socket of its own ([`crate::embed::Embedded::listen`]), and a client
//! speaking the protocol: one JSON [`Request`](crate::protocol::Request) per line, one JSON
//! [`Reply`] per line back, in order, on a connection that stays open.
//! `ZEROPTY_SOCK=<path> zeropty ...` reaches the same sessions.
//!
//! ```
//! use std::io::{BufRead as _, BufReader, Write as _};
//! use zeropty::{embed::Embedded, engine::{Options, Reply}};
//!
//! let path = std::env::temp_dir().join(format!("zeropty-doc-{}.sock", std::process::id()));
//! let zp = Embedded::start(Options::default());
//! zp.listen(&path).expect("listen");
//!
//! let mut sock = std::os::unix::net::UnixStream::connect(&path).unwrap();
//! let mut replies = BufReader::new(sock.try_clone().unwrap());
//! let mut ask = |line: &str| -> Reply {
//!     writeln!(sock, "{line}").unwrap();
//!     let mut answer = String::new();
//!     replies.read_line(&mut answer).unwrap();
//!     serde_json::from_str(&answer).unwrap()
//! };
//! assert_eq!(ask(r#"{"argv":["new-session","-d","-s","doc-server","exec cat"],"cwd":"/"}"#).code, 0);
//! let r = ask(r#"{"argv":["has-session","-t","=doc-server"],"cwd":"/"}"#);
//! assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "", ""));
//! let r = ask(r#"{"argv":["has-session","-t","=nope"],"cwd":"/"}"#);
//! assert_eq!((r.code, r.err.as_str()), (1, "can't find session: nope\n"));
//!
//! ask(r#"{"argv":["kill-server"],"cwd":"/"}"#); // ends the sessions; the host and its socket go on
//! drop(zp);
//! let _ = std::fs::remove_file(&path);
//! ```

use crate::engine::{Backend, Engine, Event, HOST_TAG, Interest, Kind, Reply, ViewerEvent, fail};
use crate::protocol::{self, ClientFrame, Frame as _, FrameError, Frames, ServerFrame, reply_line};
use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::os::fd::AsFd as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// One command line from a client.
#[deprecated(since = "0.2.0", note = "moved to zeropty::protocol::Request")]
pub type Request = protocol::Request;
/// Server to client: what the session prints.
#[deprecated(since = "0.2.0", note = "use zeropty::protocol::ServerFrame::Output")]
pub const FRAME_OUTPUT: u8 = protocol::legacy::FRAME_OUTPUT;
/// Server to client: the session ended.
#[deprecated(since = "0.2.0", note = "use zeropty::protocol::ServerFrame::Ended")]
pub const FRAME_ENDED: u8 = protocol::legacy::FRAME_ENDED;
/// Client to server: keys.
#[deprecated(since = "0.2.0", note = "use zeropty::protocol::ClientFrame::Input")]
pub const FRAME_INPUT: u8 = protocol::legacy::FRAME_INPUT;
/// Client to server: the terminal's new size.
#[deprecated(since = "0.2.0", note = "use zeropty::protocol::ClientFrame::Resize")]
pub const FRAME_RESIZE: u8 = protocol::legacy::FRAME_RESIZE;
/// Client to server: detach.
#[deprecated(since = "0.2.0", note = "use zeropty::protocol::ClientFrame::Detach")]
pub const FRAME_DETACH: u8 = protocol::legacy::FRAME_DETACH;

/// Appends a frame of `kind` holding `bytes` to `out`.
#[deprecated(since = "0.2.0", note = "use zeropty::protocol::{ServerFrame, ClientFrame}::encode")]
pub fn frame(out: &mut Vec<u8>, kind: u8, bytes: &[u8]) {
    protocol::legacy::frame(out, kind, bytes)
}

/// The server's tags: HOST_TAG and this bit, so a host's own (crate::embed's wake event) stay apart.
const SERVER_TAG: usize = HOST_TAG | 1 << 62;
const LISTENER: usize = usize::MAX;

/// An attached terminal this far behind (stalled or slow) gets the screen afresh instead of the backlog.
const VIEWER_BACKLOG_MAX: usize = 4 << 20;

/// More than this unanswered from one client and it is dropped (commands are lines of a few hundred bytes).
const CLIENT_INPUT_MAX: usize = 16 << 20;

/// A connection: one command per line, the replies in order on the same connection, which stays open.
/// The CLI sends one command and hangs up; client/zeropty.mjs keeps one connection for all its commands,
/// which saves a connect and an accept per command. Never blocks the loop: a slow or stuck client only
/// holds its own buffers (the one-shot blocking read it replaced could stall every session for 2 s).
struct Client {
    stream: UnixStream,
    generation: u32,
    input: Vec<u8>,
    output: Vec<u8>,
    /// The client hung up; closed once its replies are out.
    eof: bool,
    /// After `attach`: its viewer id and whether its keys reach the session (not with -r). The
    /// connection then carries frames instead of command lines.
    attached: Option<(u32, bool)>,
    /// A wait-pane not answered yet, and whether its reply is raw.
    wait: Option<(crate::engine::Wait, bool)>,
    /// A plugin's command not answered yet (crate::plugin), and whether its reply is raw.
    call: Option<(crate::plugin::Call, bool)>,
}

/// A host's own command: answers `argv`, or None to pass it on to the engine.
pub type CommandHook = fn(&[String]) -> Option<Reply>;

/// What the host adds: commands it answers itself, and whether kill-server ends the process.
///
/// ```
/// use zeropty::engine::{ok, Reply};
/// use zeropty::server::Hooks;
///
/// fn version(argv: &[String]) -> Option<Reply> {
///     (argv.first().map(String::as_str) == Some("my-version")).then(|| ok("1.0\n"))
/// }
/// let hooks = Hooks { command: Some(version), ..Hooks::default() };
/// assert!(!hooks.exit_on_kill); // kill-server ends the sessions, not the host
/// ```
#[derive(Default, Clone, Copy)]
pub struct Hooks {
    /// Answers a command before the engine does (the zeropty server's `mstats`); None passes it on.
    pub command: Option<CommandHook>,
    /// kill-server ends the process once its reply is out (the zeropty server). Otherwise the sessions end
    /// and the host goes on, still listening.
    pub exit_on_kill: bool,
}

/// The socket server: a listener and its connections, served on the engine's loop.
///
/// [`Server::bind`] has a host loop serving one; [`crate::host::HostLoop`] runs that loop for you.
///
/// ```
/// use zeropty::{engine::{Engine, Options}, server::{Hooks, Server}};
///
/// let path = std::env::temp_dir().join(format!("zeropty-doc-server-{}.sock", std::process::id()));
/// let e = Engine::new(Options::default());
/// let server = Server::bind(&e, &path, Hooks::default())?;
/// assert!(Server::bind(&e, &path, Hooks::default()).is_err()); // one server a socket
/// drop(server); // removes its socket
/// assert!(!path.exists());
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct Server {
    listener: UnixListener,
    path: PathBuf,
    /// (device, inode) of the socket at `path`, so dropping the server removes its own socket only.
    file: (u64, u64),
    hooks: Hooks,
    clients: Vec<Option<Client>>,
    free_clients: Vec<usize>,
    next_generation: u32,
    /// viewer id -> (client, its generation)
    viewers: HashMap<u32, (usize, u32)>,
    /// The host's plugins (set_plugins): their commands, list-plugins, and plugin-attach.
    plugins: Option<crate::plugin::Plugins>,
    /// Clients with a plugin's command not answered yet (at most; recounted by check_calls). 0, and the
    /// loop does not look at clients for them: no cost while no plugin command is under way.
    calls: usize,
}

impl Server {
    /// Listens at `path` for `engine`'s commands: the socket itself is private (0600), and a directory
    /// made for it too (0700; an existing one is left as it is). Fails if a server already answers there,
    /// or if something other than a socket is there (it is never deleted).
    ///
    /// # Example
    ///
    /// A host loop serving one client, with a command of the host's own ([`Hooks::command`]).
    /// [`crate::embed`] runs this same loop for you.
    ///
    /// ```
    /// use std::io::{BufRead as _, BufReader, Write as _};
    /// use zeropty::engine::{ok, Engine, Options, Reply};
    /// use zeropty::server::{Hooks, Server};
    ///
    /// fn version(argv: &[String]) -> Option<Reply> {
    ///     (argv.first().map(String::as_str) == Some("my-version")).then(|| ok("1.0\n"))
    /// }
    ///
    /// let path = std::env::temp_dir().join(format!("zeropty-doc-bind-{}.sock", std::process::id()));
    /// let mut e = Engine::new(Options::default());
    /// let mut server = Server::bind(&e, &path, Hooks { command: Some(version), ..Hooks::default() }).unwrap();
    ///
    /// let client = std::thread::spawn({
    ///     let path = path.clone();
    ///     move || {
    ///         let sock = std::os::unix::net::UnixStream::connect(&path).unwrap();
    ///         writeln!(&sock, r#"{{"argv":["my-version"],"cwd":"/"}}"#).unwrap();
    ///         let mut line = String::new();
    ///         BufReader::new(&sock).read_line(&mut line).unwrap();
    ///         line
    ///     }
    /// });
    /// let mut host = Vec::new();
    /// while !client.is_finished() {
    ///     e.try_poll(Some(server.wait_timeout(&e).unwrap_or(50).min(50)), &mut host).unwrap();
    ///     for ev in host.drain(..) {
    ///         if Server::owns(&ev) {
    ///             server.event(&mut e, &ev);
    ///         }
    ///     }
    ///     e.tick();
    ///     server.after_tick(&mut e);
    /// }
    /// assert_eq!(client.join().unwrap(), "{\"code\":0,\"out\":\"1.0\\n\",\"err\":\"\"}\n");
    /// ```
    pub fn bind<B: Backend>(engine: &Engine<B>, path: &Path, hooks: Hooks) -> std::io::Result<Self> {
        // sockaddr_un.sun_path is 104 bytes on macOS; a longer path fails to bind with no useful error.
        if path.as_os_str().len() >= 104 {
            return Err(std::io::Error::other(format!("socket path too long (max 103 bytes): {}", path.display())));
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty() && !d.exists()) {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
        }
        if crate::sys::fd::connect(path).is_ok() {
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, format!("a server already answers on {}", path.display())));
        }
        match std::fs::symlink_metadata(path) {
            // A socket no server answers on any more (one that exited without removing it).
            Ok(m) if std::os::unix::fs::FileTypeExt::is_socket(&m.file_type()) => std::fs::remove_file(path)?,
            Ok(_) => return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("not a socket, left as it is: {}", path.display()))),
            Err(_) => {}
        }
        let listener = crate::sys::fd::bind(path)?;
        // Others cannot connect even where the directory lets them in (accept checks who it is as well).
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let file = file_id(path).ok_or_else(|| std::io::Error::other(format!("socket gone: {}", path.display())))?;
        engine.watch(listener.as_fd(), Interest::Read { lowat: 1 }, LISTENER)?;
        Ok(Self {
            listener,
            path: path.to_owned(),
            file,
            hooks,
            clients: Vec::new(),
            free_clients: Vec::new(),
            next_generation: 0,
            viewers: HashMap::new(),
            plugins: None,
            calls: 0,
        })
    }

    /// Answers the plugins' commands and `list-plugins`, and lets programs attach as plugins
    /// (`plugin-attach`, crate::plugin). The host runs `plugins` with its loop (`HostLoop::set_plugins`)
    /// and lets them wake it (`Plugins::attach`).
    pub fn set_plugins(&mut self, plugins: crate::plugin::Plugins) {
        self.plugins = Some(plugins)
    }

    /// Whether an event the engine handed back (HOST_TAG) is this server's.
    ///
    /// ```
    /// use zeropty::{engine::{Event, Kind, HOST_TAG}, server::Server};
    ///
    /// // An event of the host's own fd, watched with tag 7 (Engine::watch adds HOST_TAG).
    /// let mine = Event::new(Kind::Readable, 5, 7 | HOST_TAG);
    /// assert!(!Server::owns(&mine));
    /// ```
    pub fn owns(ev: &Event) -> bool {
        ev.tag() & SERVER_TAG == SERVER_TAG
    }

    /// Handles one of this server's events.
    ///
    /// ```
    /// use std::io::{BufRead as _, BufReader, Write as _};
    /// use zeropty::{engine::{Engine, Options}, server::{Hooks, Server}};
    ///
    /// let path = std::env::temp_dir().join(format!("zeropty-doc-event-{}.sock", std::process::id()));
    /// let mut e = Engine::new(Options::default());
    /// let mut server = Server::bind(&e, &path, Hooks::default())?;
    /// let mut sock = std::os::unix::net::UnixStream::connect(&path)?;
    /// writeln!(sock, r#"{{"argv":["has-session","-t","=none"],"cwd":"/"}}"#)?;
    /// sock.set_nonblocking(true)?;
    /// let (mut lines, mut reply) = (BufReader::new(&sock), String::new());
    /// let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    /// while !reply.ends_with('\n') && std::time::Instant::now() < deadline {
    ///     let mut host = Vec::new();
    ///     e.try_poll(Some(20), &mut host)?;
    ///     for ev in host.iter().filter(|ev| Server::owns(ev)) {
    ///         server.event(&mut e, ev); // accepts the client, then answers its line
    ///     }
    ///     let _ = lines.read_line(&mut reply);
    /// }
    /// assert!(reply.starts_with(r#"{"code":1"#), "{reply}");
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn event<B: Backend>(&mut self, e: &mut Engine<B>, ev: &Event) {
        let tag = ev.tag();
        if tag == LISTENER {
            while let Ok(stream) = crate::sys::fd::accept(&self.listener) {
                self.accept(e, stream);
            }
            return;
        }
        let (i, generation) = ((tag & !SERVER_TAG) >> 32, tag as u32);
        if self.clients.get(i).and_then(Option::as_ref).is_some_and(|c| c.generation == generation) {
            self.serve_client(e, i, ev.kind() == Kind::Readable);
        }
    }

    /// After the engine's tick: output for attached terminals, and the wait-panes that are over.
    ///
    /// ```
    /// use zeropty::{engine::{Engine, Options}, server::{Hooks, Server}};
    ///
    /// let path = std::env::temp_dir().join(format!("zeropty-doc-after-tick-{}.sock", std::process::id()));
    /// let mut e = Engine::new(Options::default());
    /// let mut server = Server::bind(&e, &path, Hooks::default())?;
    /// e.try_poll(Some(0), &mut Vec::new())?;
    /// e.tick();
    /// server.after_tick(&mut e); // every turn of the loop, after the engine's tick
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn after_tick<B: Backend>(&mut self, e: &mut Engine<B>) {
        self.pass_on(e);
        self.check_waits(e);
        if self.calls != 0 {
            self.check_calls(e);
        }
    }

    /// How long the loop may sleep before a wait-pane may be over on its own (quiet long enough, timed
    /// out), or a plugin's command times out.
    ///
    /// ```
    /// use zeropty::{engine::{Engine, Options}, server::{Hooks, Server}};
    ///
    /// let path = std::env::temp_dir().join(format!("zeropty-doc-wait-timeout-{}.sock", std::process::id()));
    /// let e = Engine::new(Options::default());
    /// let server = Server::bind(&e, &path, Hooks::default())?;
    /// assert_eq!(server.wait_timeout(&e), None); // no client is in a wait-pane or a plugin command
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn wait_timeout<B: Backend>(&self, e: &Engine<B>) -> Option<u64> {
        let waits = self.clients.iter().flatten().filter_map(|c| c.wait.as_ref()).filter_map(|(w, _)| e.wait_due_ms(w));
        if self.calls == 0 {
            return waits.min();
        }
        waits.chain(self.clients.iter().flatten().filter_map(|c| c.call.as_ref()).map(|(c, _)| c.due_ms())).min()
    }

    fn accept<B: Backend>(&mut self, e: &Engine<B>, stream: UnixStream) {
        // Only this user (or root), as tmux does: a connection runs commands as this user, so the
        // socket's permissions are not the only guard (a socket put in a shared directory, the moment
        // between bind and chmod).
        if !crate::sys::process::peer_is_same_user(&stream) || stream.set_nonblocking(true).is_err() {
            return;
        }
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        let i = self.free_clients.pop().unwrap_or_else(|| {
            self.clients.push(None);
            self.clients.len() - 1
        });
        // Edge-triggered like the sessions' pipes; a command already sent is reported at once.
        if e.watch(stream.as_fd(), Interest::Read { lowat: 1 }, client_tag(i, generation)).is_err() {
            // Never heard from: dropped (closed) at once.
            self.free_clients.push(i);
            return;
        }
        self.clients[i] = Some(Client { stream, generation, input: Vec::new(), output: Vec::new(), eof: false, attached: None, wait: None, call: None });
    }

    /// Reads what client `i` sent (when `readable`), answers every complete line, and sends what it can.
    fn serve_client<B: Backend>(&mut self, e: &mut Engine<B>, i: usize, readable: bool) {
        let Some(mut c) = self.clients[i].take() else { return };
        if readable {
            let mut buf = [0u8; 4096];
            loop {
                match (&c.stream).read(&mut buf) {
                    Ok(0) => {
                        c.eof = true;
                        break;
                    }
                    Ok(n) => c.input.extend_from_slice(&buf[..n]),
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        c.eof = true;
                        break;
                    }
                }
            }
            if c.input.len() > CLIENT_INPUT_MAX {
                c.input.clear();
                c.eof = true;
            }
        }
        let mut exit = false;
        let mut at = 0;
        loop {
            if let Some((id, writable)) = c.attached {
                at += self.frames(e, &mut c, id, writable, at);
                break;
            }
            // Commands after a wait-pane (or a plugin's command) wait for its reply: replies go out in order.
            if c.wait.is_some() || c.call.is_some() {
                break;
            }
            let Some(n) = c.input[at..].iter().position(|&b| b == b'\n') else { break };
            let line = &c.input[at..at + n];
            at += n + 1;
            // Parsed once for every check below.
            let parsed = serde_json::from_slice::<protocol::Request>(line);
            if let Ok(req) = &parsed {
                let cmd = req.argv.first().map(String::as_str);
                if cmd == Some("plugin-attach") && self.plugins.is_some() {
                    match self.plugin_attach(e, c, i, &req.argv[1..], at) {
                        // The connection is the plugin's now.
                        Ok(()) => return,
                        Err(back) => {
                            c = back.0;
                            c.output.extend(reply_line(&back.1, req.raw));
                            continue;
                        }
                    }
                }
                // One atomic load while no plugin adds commands.
                if let Some(call) = self.plugins.as_ref().and_then(|p| p.call(&req.argv, &req.cwd, true)) {
                    // Answered by check_calls.
                    c.call = Some((call, req.raw));
                    self.calls += 1;
                    continue;
                }
                if matches!(cmd, Some("export-sessions" | "import-sessions")) {
                    // Sessions handed over on this connection, then it closes.
                    self.handover(e, &c, &req.argv);
                    at = c.input.len();
                    c.output.clear();
                    c.eof = true;
                    break;
                }
            }
            let mark = c.output.len();
            let mut raw = false;
            let reply = match parsed {
                Ok(req) if matches!(req.argv.first().map(String::as_str), Some("attach-session" | "attach" | "a")) => self.attach(e, &mut c, i, &req.argv[1..]),
                Ok(req) if req.argv.first().map(String::as_str) == Some("wait-pane") => {
                    raw = req.raw;
                    match e.wait_start(&req.argv[1..]) {
                        Err(r) => r,
                        Ok(w) => match e.wait_check(&w) {
                            Some(r) => {
                                e.wait_end(&w);
                                r
                            }
                            None => {
                                // Answered by check_waits.
                                c.wait = Some((w, raw));
                                continue;
                            }
                        },
                    }
                }
                Ok(req) => {
                    raw = req.raw;
                    self.dispatch(e, req, &mut exit)
                }
                Err(err) => fail(format!("bad request: {err}\n")),
            };
            // An attach's screen is already in `output`, framed: its reply line goes before it.
            c.output.splice(mark..mark, reply_line(&reply, raw));
            if exit {
                break;
            }
        }
        c.input.drain(..at);
        if c.eof
            && let Some((id, _)) = c.attached.take()
        {
            e.detach(id);
            self.viewers.remove(&id);
        }
        if exit {
            // kill-server: the reply goes out before the process ends.
            let _ = c.stream.set_nonblocking(false);
            let _ = (&c.stream).write_all(&c.output);
            std::process::exit(0);
        }
        self.send(e, &mut c, i);
        if c.eof && c.output.is_empty() {
            if let Some((w, _)) = c.wait.take() {
                e.wait_end(&w);
            }
            // Dropping the stream closes it, which also removes its filters.
            self.free_clients.push(i);
        } else {
            self.clients[i] = Some(c);
        }
    }

    /// Answers the plugins' commands that are (or timed out), then the commands their clients sent after them.
    fn check_calls<B: Backend>(&mut self, e: &mut Engine<B>) {
        for i in 0..self.clients.len() {
            let Some(c) = self.clients[i].as_mut() else { continue };
            let Some(r) = c.call.as_ref().and_then(|(call, _)| call.poll()) else { continue };
            let raw = c.call.take().is_some_and(|(_, raw)| raw);
            c.output.extend(reply_line(&r, raw));
            self.serve_client(e, i, false);
        }
        // Those still waiting (serve_client may have started more; closed clients took theirs along).
        self.calls = self.clients.iter().flatten().filter(|c| c.call.is_some()).count();
    }

    /// `plugin-attach -n name [-e events] [-c commands] [-a access] [-v api]`: the connection becomes
    /// plugin `name`'s (crate::plugin, the socket protocol), served on threads of its own from here on.
    /// `at` is where the bytes after the command start. On failure the client comes back with the reply
    /// to send it.
    ///
    /// The plugin is registered first; the replies the connection is owed and the attach's own go out
    /// from the plugin's thread after that, so a program told it attached finds itself in list-plugins
    /// and its commands routed, and the engine's thread never blocks on its socket.
    fn plugin_attach<B: Backend>(&mut self, e: &mut Engine<B>, c: Client, i: usize, argv: &[String], at: usize) -> Result<(), Box<(Client, Reply)>> {
        use crate::plugin::internal::remote;
        let plugins = self.plugins.clone().expect("checked by the caller");
        let manifest = match remote::parse_attach(argv).and_then(|m| plugins.check(&m).map(|()| m).map_err(String::from)) {
            Ok(m) => m,
            Err(err) => return Err(Box::new((c, fail(err)))),
        };
        if c.attached.is_some() {
            return Err(Box::new((c, fail("plugin-attach: connection attached to a session\n"))));
        }
        // Replies to what it sent before, then this one, before any event.
        let mut owed = c.output.clone();
        owed.extend(reply_line(&crate::engine::ok(format!("zeropty-plugin {}\n", crate::plugin::API_VERSION)), false));
        let pending = c.input[at..].to_vec();
        // A second descriptor for the plugin: if it cannot be registered, the client is still whole.
        let added = c
            .stream
            .try_clone()
            .and_then(|s| remote::Remote::new(manifest, s, pending, owed))
            .map_err(|err| format!("plugin-attach: {err}\n"))
            .and_then(|r| plugins.add(r).map_err(String::from));
        if let Err(err) = added {
            // Taken since the check (another thread added one of that name): the client hears why.
            return Err(Box::new((c, fail(err))));
        }
        // Closing this descriptor takes its filters off the engine's queue (kqueue drops an fd's
        // filters as it closes); the plugin's descriptor, never registered, stays open, read and written
        // by the plugin's threads, blocking.
        drop(c);
        // It hears every event from here on.
        plugins.pump(e);
        self.free_clients.push(i);
        Ok(())
    }

    /// Answers the wait-panes that are over, then the commands their clients sent after them.
    fn check_waits<B: Backend>(&mut self, e: &mut Engine<B>) {
        for i in 0..self.clients.len() {
            let Some(c) = self.clients[i].as_mut() else { continue };
            let Some((w, raw)) = c.wait.as_mut() else { continue };
            if let Some(r) = e.wait_check(w) {
                let raw = *raw;
                e.wait_end(w);
                c.wait = None;
                c.output.extend(reply_line(&r, raw));
                self.serve_client(e, i, false);
            }
        }
    }

    /// `attach-session [-r] -t target [-x cols -y rows]`: the connection switches to frames, starting with
    /// the screen as it is.
    fn attach<B: Backend>(&mut self, e: &mut Engine<B>, c: &mut Client, i: usize, argv: &[String]) -> Reply {
        let (mut target, mut size, mut writable) = (None, (None, None), true);
        let mut k = 0;
        while k < argv.len() {
            match argv[k].as_str() {
                "-t" => {
                    k += 1;
                    target = argv.get(k).cloned()
                }
                "-x" => {
                    k += 1;
                    size.1 = argv.get(k).and_then(|v| v.parse::<u16>().ok())
                }
                "-y" => {
                    k += 1;
                    size.0 = argv.get(k).and_then(|v| v.parse::<u16>().ok())
                }
                "-r" => writable = false,
                _ => {}
            }
            k += 1;
        }
        let size = match size {
            (Some(r), Some(c)) => Some((r, c)),
            _ => None,
        };
        match e.attach(target.as_deref().unwrap_or(""), size) {
            Ok((id, screen)) => {
                c.attached = Some((id, writable));
                self.viewers.insert(id, (i, c.generation));
                ServerFrame::Output(&screen).encode(&mut c.output);
                crate::engine::ok("")
            }
            Err(r) => r,
        }
    }

    /// export-sessions / import-sessions (crate::engine::handover), on a connection set to block for it: the
    /// sessions go over in one go, the loop doing nothing else meanwhile (a few ms).
    fn handover<B: Backend>(&mut self, e: &mut Engine<B>, c: &Client, argv: &[String]) {
        let s = &c.stream;
        let r = crate::engine::handover::prepare(s).and_then(|()| (&c.stream).write_all(&c.output)).and_then(|()| match argv[0].as_str() {
            "export-sessions" => crate::engine::handover::serve_export(e, s, &argv[1..]),
            _ => crate::engine::handover::serve_import(e, s),
        });
        if let Err(err) = r {
            eprintln!("zeropty: handover: {err}");
        }
        // Sessions that left may have had terminals attached here.
        self.pass_on(e);
    }

    /// Handles the complete frames in `c.input[at..]`; returns the bytes used. A frame too long to be
    /// one ends the connection; one of a kind this server does not know is skipped (a newer client's).
    fn frames<B: Backend>(&mut self, e: &mut Engine<B>, c: &mut Client, id: u32, writable: bool, at: usize) -> usize {
        let mut frames = Frames::<ClientFrame>::new(&c.input[at..]);
        let mut detach = false;
        for f in frames.by_ref() {
            match f {
                Ok(ClientFrame::Input(keys)) if writable => {
                    // Keys for a session whose input is closed go nowhere, as on a terminal.
                    let _ = e.viewer_input(id, keys);
                }
                Ok(ClientFrame::Input(_)) | Err(FrameError::UnknownKind(_) | FrameError::BadBody(_)) => {}
                Ok(ClientFrame::Resize { rows, cols }) => e.viewer_resize(id, rows, cols),
                Ok(ClientFrame::Detach) | Err(FrameError::TooLong(_)) => {
                    detach = true;
                    break;
                }
            }
        }
        let used = frames.used();
        if detach {
            e.detach(id);
            self.viewers.remove(&id);
            c.attached = None;
            c.eof = true;
        }
        used
    }

    /// Passes what the engine has for attached terminals on to their connections.
    fn pass_on<B: Backend>(&mut self, e: &mut Engine<B>) {
        for (id, ev) in e.viewer_events() {
            let Some(&(i, generation)) = self.viewers.get(&id) else { continue };
            let Some(mut c) = self.clients.get_mut(i).and_then(Option::take) else { continue };
            if c.generation != generation {
                self.clients[i] = Some(c);
                continue;
            }
            match ev {
                ViewerEvent::Output(bytes) => {
                    if c.output.len() > VIEWER_BACKLOG_MAX {
                        c.output.clear();
                        if let Some(screen) = e.viewer_redraw(id) {
                            ServerFrame::Output(&screen).encode(&mut c.output);
                        }
                    } else {
                        ServerFrame::Output(&bytes).encode(&mut c.output);
                    }
                }
                ViewerEvent::Ended => {
                    ServerFrame::Ended.encode(&mut c.output);
                    c.attached = None;
                    self.viewers.remove(&id);
                }
            }
            self.send(e, &mut c, i);
            self.clients[i] = Some(c);
        }
    }

    /// Writes what the socket takes now; the rest goes on EVFILT_WRITE.
    fn send<B: Backend>(&mut self, e: &Engine<B>, c: &mut Client, i: usize) {
        while !c.output.is_empty() {
            match (&c.stream).write(&c.output) {
                Ok(n) => {
                    c.output.drain(..n);
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if e.watch(c.stream.as_fd(), Interest::WriteOnce, client_tag(i, c.generation)).is_err() {
                        // Never told when it drains: a connection that cannot be served.
                        c.output.clear();
                        c.eof = true;
                    }
                    return;
                }
                Err(_) => {
                    c.output.clear();
                    c.eof = true;
                }
            }
        }
    }

    /// Commands the host answers itself, kill-server's exit; the rest go to the engine.
    fn dispatch<B: Backend>(&mut self, e: &mut Engine<B>, req: protocol::Request, exit: &mut bool) -> Reply {
        if let Some(r) = self.hooks.command.and_then(|f| f(&req.argv)) {
            return r;
        }
        match req.argv.first().map(String::as_str) {
            // It would wait on itself until the handover timed out.
            Some("pull-sessions" | "push-sessions") if req.argv.iter().any(|a| same_file(a, &self.path)) => fail("that is this server's socket\n"),
            Some("list-plugins") if self.plugins.is_some() => self.plugins.as_ref().map(|p| p.list()).expect("checked"),
            Some("kill-server") => {
                *exit = self.hooks.exit_on_kill;
                e.command(&req.argv, &req.cwd)
            }
            _ => e.command(&req.argv, &req.cwd),
        }
    }
}

/// Client `i`'s tag: SERVER_TAG, its index and its generation.
fn client_tag(i: usize, generation: u32) -> usize {
    SERVER_TAG | i << 32 | generation as usize
}

/// The file at `path` as (device, inode), not following a symbolic link at its end.
fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Whether paths `a` and `b` name the same file.
fn same_file(a: &str, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => (x.dev(), x.ino()) == (y.dev(), y.ino()),
        _ => false,
    }
}

impl Drop for Server {
    /// Removes its socket, unless another server has taken the path since: a client then finds no
    /// server there instead of connecting to a socket nobody serves (a child process started while it
    /// was being made can hold a copy of the listening fd).
    fn drop(&mut self) {
        if file_id(&self.path) == Some(self.file) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    //! The server on a real engine, its connections made with socket pairs handed to `accept` (the
    //! listener is bound but not used), driven a turn at a time through crate::host.

    use super::{Hooks, Server};
    use crate::engine::{Engine, Options, ok};
    use crate::host::{HostLoop, Source, Waiting, Yield};
    use crate::protocol::{ClientFrame, Frame as _, Frames, ServerFrame};
    use std::io::{Read as _, Write as _};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    struct Fixture {
        e: Engine,
        host: HostLoop,
        dir: PathBuf,
    }

    impl Fixture {
        fn new(name: &str, hooks: Hooks) -> Self {
            let dir = std::env::temp_dir().join(format!("zeropty-srv-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let e = Engine::new(Options { size_dir: dir.clone(), ..Options::default() });
            let srv = Server::bind(&e, &dir.join("s"), hooks).unwrap();
            Self { e, host: HostLoop::new(Some(srv), Waiting::WithEngine), dir }
        }

        /// A connection to the server: our end, the server's accepted as from its listener.
        fn connect(&mut self) -> UnixStream {
            let (ours, theirs) = crate::sys::fd::socket_pair().unwrap();
            theirs.set_nonblocking(true).unwrap();
            self.host.server_mut().unwrap().accept(&self.e, theirs);
            ours.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
            ours
        }

        /// Turns the loop until what `c` received satisfies `done`; returns it all.
        fn until(&mut self, c: &mut UnixStream, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
            let (mut got, start) = (Vec::new(), Instant::now());
            let mut b = [0u8; 4096];
            while !done(&got) {
                assert!(start.elapsed() < Duration::from_secs(5), "{:?}", String::from_utf8_lossy(&got));
                self.host.turn(&mut self.e, Source::Wait(Some(Duration::from_millis(5))), Yield::Never, |_, _, _| {}).unwrap();
                match c.read(&mut b) {
                    Ok(0) => break,
                    Ok(n) => got.extend_from_slice(&b[..n]),
                    Err(_) => {}
                }
            }
            got
        }

        fn lines(&mut self, c: &mut UnixStream, n: usize) -> String {
            String::from_utf8(self.until(c, |b| b.iter().filter(|&&x| x == b'\n').count() >= n)).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.e.command(&["kill-server".to_string()], "/");
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn replies_go_in_order_raw_or_json() {
        let mut f = Fixture::new("order", Hooks::default());
        let mut c = f.connect();
        // Three commands in one write, the last asking for a raw reply.
        c.write_all(b"{\"argv\":[\"has-session\",\"-t\",\"x\"],\"cwd\":\"/\"}\nnot json\n{\"argv\":[\"list-sessions\"],\"cwd\":\"/\",\"raw\":true}\n").unwrap();
        let got = f.lines(&mut c, 3);
        let mut lines = got.lines();
        assert_eq!(lines.next(), Some("{\"code\":1,\"out\":\"\",\"err\":\"can't find session: x\\n\"}"));
        assert!(lines.next().unwrap().contains("bad request"), "{got}");
        assert_eq!(lines.next(), Some("0 0 0"));
    }

    #[test]
    fn hooks_answer_first_and_own_socket_is_refused() {
        let hooks = Hooks { command: Some(|argv| (argv.first().map(String::as_str) == Some("ping")).then(|| ok("pong\n"))), exit_on_kill: false };
        let mut f = Fixture::new("hooks", hooks);
        let mut c = f.connect();
        let me = f.dir.join("s").display().to_string();
        c.write_all(format!("{{\"argv\":[\"ping\"],\"cwd\":\"/\"}}\n{{\"argv\":[\"pull-sessions\",\"-s\",\"{me}\"],\"cwd\":\"/\"}}\n").as_bytes()).unwrap();
        let got = f.lines(&mut c, 2);
        assert!(got.starts_with("{\"code\":0,\"out\":\"pong\\n\",\"err\":\"\"}\n"), "{got}");
        assert!(got.contains("that is this server's socket"), "{got}");
    }

    #[test]
    fn attached_terminals_get_frames_and_bad_frames_end_them() {
        let mut f = Fixture::new("attach", Hooks::default());
        assert_eq!(f.e.command(&["new-session", "-d", "-s", "a", "exec cat"].map(String::from), "/"), ok(""));
        let mut c = f.connect();
        c.write_all(b"{\"argv\":[\"attach\",\"-t\",\"nope\"],\"cwd\":\"/\"}\n").unwrap();
        assert!(f.lines(&mut c, 1).contains("can't find session: nope"), "still a command connection");
        c.write_all(b"{\"argv\":[\"attach\",\"-t\",\"a\"],\"cwd\":\"/\"}\n").unwrap();
        // The reply line, then frames: the screen first.
        let got = f.until(&mut c, |b| b.contains(&b'\n'));
        let nl = got.iter().position(|&b| b == b'\n').unwrap();
        assert!(got.starts_with(b"{\"code\":0"), "{:?}", String::from_utf8_lossy(&got));
        // Keys go to cat, whose output comes back framed; a frame of an unknown kind is skipped.
        let mut keys = Vec::new();
        crate::protocol::legacy::frame(&mut keys, 9, b"from the future");
        ClientFrame::Input(b"echoed\r").encode(&mut keys);
        c.write_all(&keys).unwrap();
        let rest = got[nl + 1..].to_vec();
        let out = f.until(&mut c, |b| {
            let all = [&rest[..], b].concat();
            // The echo of the keys, then cat printing them back.
            let outputs = Frames::<ServerFrame>::new(&all).filter_map(Result::ok).map(|fr| match fr {
                ServerFrame::Output(o) => o.windows(6).filter(|w| w == b"echoed").count(),
                ServerFrame::Ended => 0,
            });
            outputs.sum::<usize>() >= 2
        });
        assert!(!out.is_empty());
        // A length no frame has: the server lets the terminal go and closes the connection.
        let mut bad = vec![0u8];
        bad.extend_from_slice(&u32::MAX.to_le_bytes());
        c.write_all(&bad).unwrap();
        let start = Instant::now();
        loop {
            f.host.turn(&mut f.e, Source::Wait(Some(Duration::from_millis(5))), Yield::Never, |_, _, _| {}).unwrap();
            let mut b = [0u8; 4096];
            if let Ok(0) = c.read(&mut b) {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "connection not closed");
        }
        assert!(!f.e.command(&["list-sessions".to_string()], "/").out.contains("attached"), "detached");
    }
}
