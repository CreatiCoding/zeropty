//! The engine: sessions, their screens and every tmux-style command, on one event queue. A host runs it:
//! the `zeropty` server (src/main.rs) puts a socket in front of it; an embedding program calls it directly.
//!
//! The host owns the thread. It calls `try_poll` (waits on the queue and handles the engine's events),
//! handles the events tagged HOST_TAG it got back (its own fds, registered with `watch`), then calls
//! `tick` (reads that are due, freezing and compressing quiet screens). crate::host encodes that order
//! once for every host in this crate.
//!
//! The engine's seams to the OS are three traits (`backend`), bundled as `Engine<B: Backend>`: the
//! queue (`Poller`), the time (`Clock`) and the programs sessions run (`Programs`: starting, signalling,
//! reaping them, their size file); `System` (a kqueue, the monotonic clock, real processes) by default.
//! Everything the engine decides (when to read, what to re-arm, what to freeze, whom to signal and
//! when) goes through them, so its tests (`tests`) drive it with fakes (`testing`), on sockets, with no
//! process started.
//!
//! # Modules
//!
//! Pure (no I/O, unit-tested on their own):
//! - `args`: tmux-style flags and targets. `keys`: key names to bytes. `ldisc`: the tty line discipline.
//! - `format`: #{variables}. `render`: screens as text (capture-pane, redraw). `record`: a session's
//!   wire format for handing it over. `policy`: the timing decisions. `table`: the session table.
//!   `reply`: replies and command errors. `backend`: the `Poller`, `Clock` and `Programs` traits.
//!
//! With I/O, through crate::sys:
//! - `session`: one session's pipes and screen. `spawn`: starting programs (behind `SystemPrograms`).
//! - `commands`: the command table and handlers. `viewers`: attached terminals. `wait`: wait-pane.
//!   `transfer`: one session out of and into an engine. `handover`: sessions between engines in
//!   different processes, over a socket. `tap`: what happens to sessions, queued for crate::plugin
//!   only when asked for. This module: the event loop and what ties them together.
//!
//! # Invariants
//!
//! - A session's event tag is its `SessKey` (slot << 32 | generation); every event is checked against
//!   the slot's current generation, so events queued for an ended session are dropped.
//! - Pending filter changes (`changes`) never outlive their session (`remove_keeping` drops those with
//!   its tag), and never name a new session's fd under an older tag (`watch_session` drops those naming
//!   its fds: an fd closed at the end of output is reused by the next one opened).
//! - A session whose read is `due` has its read filter set to BURST_LOWAT; every read that takes it out
//!   of `due` sets it back to report any output.
//!
//! # Example
//!
//! Every command goes through [`Engine::command`], as tmux takes it on its command line:
//!
//! ```
//! use zeropty::engine::{Engine, Options};
//!
//! let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
//! let mut e = Engine::new(Options::default());
//! assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "doc-engine", "exec cat"]), "/").code, 0);
//! let r = e.command(&argv(&["list-sessions", "-F", "#{session_name}"]), "/");
//! assert_eq!(r.out, "doc-engine\n");
//! e.command(&argv(&["kill-server"]), "/");
//! ```
//!
//! [`Engine`] has the host's loop, [`Engine::wait_start`] waiting for text without polling, and
//! [`Engine::attach`] a terminal of the host's own.

mod args;
mod backend;
mod commands;
mod format;
pub mod handover;
mod keys;
mod ldisc;
mod options;
mod policy;
mod record;
mod render;
mod reply;
mod session;
mod spawn;
mod table;
pub(crate) mod tap;
mod transfer;
mod viewers;
mod wait;

pub use crate::sys::kq::{Change, Event, Interest, Kind, Kqueue};
pub use backend::{Backend, Clock, Poller, Programs, Spawn, Spawned, System, SystemClock, SystemPrograms};
pub use options::Options;
pub use reply::{CmdError, Reply, fail, ok};
pub use viewers::ViewerEvent;
pub use wait::Wait;

/// This engine's version (Semantic Versioning, docs/VERSIONING.md), as tmux's #{version} gives the server's.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use crate::term::TTY_ICANON;
use args::session_name;
use options::{BURST_LOWAT, DRAIN_BATCH, IDLE_TRIM_MS, SWEEP_EVERY_MS, coalesce_ms};
use policy::Sweep;
use session::{Screenful, Sess, Stats};
use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd as _, BorrowedFd, RawFd};
use std::path::PathBuf;
use table::{Names, SLOT_CHUNK, SessKey, Slots};

/// Events whose tag has this bit are the host's: `poll` hands them back. Session tags never have it
/// (slot << 32 stays far below). `watch` sets it.
pub const HOST_TAG: usize = 1 << 63;

/// Sets O_NONBLOCK on `fd`.
#[deprecated(since = "0.2.0", note = "a host sets its fds non-blocking itself (std's set_nonblocking)")]
pub fn set_nonblocking(fd: RawFd) {
    let _ = crate::sys::fd::set_nonblocking(fd);
}

/// Sessions and their commands, on one event queue, one clock and the programs it runs, all from
/// `B` (the real OS by default). See the module's docs for how a host runs it.
///
/// # Example
///
/// The whole host in a few lines: an engine on this thread, one session, and the loop that reads its
/// output ([`crate::embed`] and [`crate::host::HostLoop`] run this loop for you).
///
/// ```
/// use zeropty::engine::{Engine, Options};
///
/// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// let mut e = Engine::new(Options::default());
/// let r = e.command(&argv(&["new-session", "-d", "-s", "doc-engine-loop", "echo ready; exec cat"]), "/");
/// assert_eq!(r.code, 0, "{}", r.err);
///
/// let screen = |e: &mut Engine| e.command(&argv(&["capture-pane", "-p", "-t", "=doc-engine-loop:"]), "/").out;
/// // The host's loop: wait on the queue, read what is due, until the line is on screen.
/// let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
/// while !screen(&mut e).starts_with("ready\n") && std::time::Instant::now() < deadline {
///     e.try_poll(Some(50), &mut Vec::new()).unwrap();
///     e.tick();
/// }
/// assert!(screen(&mut e).starts_with("ready\n"));
/// e.command(&argv(&["kill-server"]), "/");
/// ```
pub struct Engine<B: Backend = System> {
    poller: B::Poller,
    clock: B::Clock,
    programs: B::Programs,
    /// Filter changes sent with the next wait: a session's read filter is off while its read is scheduled,
    /// so its further output during the coalescing window does not wake the loop again.
    changes: Vec<Change>,
    slots: Slots<Sess>,
    free: Vec<usize>,
    names: Names,
    next_generation: u32,
    /// Paste buffers by name (load-buffer, capture-pane without -p, paste-buffer).
    buffers: HashMap<String, Vec<u8>>,
    /// (when, session) of coalesced reads
    due: Vec<(u64, SessKey)>,
    /// (when, process group) of SIGKILL follow-ups
    kills: Vec<(u64, i32)>,
    last_sweep: u64,
    /// Scratch released and memory collected since the last activity (see tick).
    trimmed: bool,
    opts: Options,
    stats: Stats,
    events: Vec<Event>,
    /// Attached terminals: id -> their session.
    viewers: HashMap<u32, SessKey>,
    next_viewer: u32,
    /// What the host has to pass on to attached terminals (see viewer_events).
    viewer_events: Vec<(u32, ViewerEvent)>,
    /// Tells this engine's size files from those of an earlier process that had the same pid, whose
    /// sessions may have been handed to this one.
    instance: u64,
    next_wait: u32,
    /// set-environment -g: set (Some) or removed (None) in the sessions made after.
    env: Vec<(String, Option<String>)>,
    /// Session events for the layer above (crate::plugin), queued only for kinds it asked for.
    tap: tap::Tap,
}

/// What tmux looks a command's -t up as, which only words the error when nothing matches.
#[derive(Clone, Copy)]
enum Lookup {
    Session,
    Window,
    Pane,
}

/// The error tmux 3.7c gives when `target` names nothing, for a command that looks it up as `kind`.
/// With a ':' it is the session part that failed. Without one, tmux reads it as an id ($session,
/// @window, %pane) whatever the command, as a window when it has a '.' (window.pane), and otherwise
/// as what the command looks for: a pane is named as given, a window and a session without the '='.
fn missing(target: Option<&str>, kind: Lookup) -> String {
    let t = target.unwrap_or("");
    let name = session_name(t);
    if target.is_none() || t.contains(':') {
        return format!("can't find session: {name}\n");
    }
    match (t.chars().next(), t.split_once('.')) {
        (Some('$'), _) => format!("can't find session: {t}\n"),
        (Some('@'), _) => format!("can't find window: {t}\n"),
        (Some('%'), _) => format!("can't find pane: {t}\n"),
        (_, Some((window, _))) => format!("can't find window: {}\n", window.strip_prefix('=').unwrap_or(window)),
        _ => match kind {
            Lookup::Pane => format!("can't find pane: {t}\n"),
            Lookup::Window => format!("can't find window: {name}\n"),
            Lookup::Session => format!("can't find session: {name}\n"),
        },
    }
}

impl Engine {
    /// An engine with no sessions, on a kqueue of its own.
    ///
    /// # Panics
    /// When no kqueue can be made (the process is out of file descriptors); `try_new` returns that.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let e = Engine::new(Options::default());
    /// assert!(e.session_names().is_empty());
    /// ```
    pub fn new(opts: Options) -> Self {
        Self::try_new(opts).expect("zeropty: a kqueue for the engine")
    }

    /// An engine with no sessions, on a kqueue of its own.
    ///
    /// ```
    /// let e = zeropty::engine::Engine::try_new(Default::default())?;
    /// assert!(e.session_names().is_empty());
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn try_new(opts: Options) -> std::io::Result<Self> {
        Ok(Self::with_backend(opts, Kqueue::new()?, SystemClock, SystemPrograms))
    }

    /// The kqueue's fd.
    #[deprecated(since = "0.2.0", note = "register fds with `watch`, wait on `as_fd()`")]
    pub fn kq(&self) -> RawFd {
        self.poller.as_raw_fd()
    }
}

/// The engine's queue, for a host that waits on it itself (crate::embed): readable when the engine has
/// events.
impl<B: Backend> AsFd for Engine<B>
where
    B::Poller: AsFd,
{
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.poller.as_fd()
    }
}

impl<B: Backend> Engine<B> {
    /// An engine with no sessions on `poller`, timed by `clock`, running its sessions' programs through
    /// `programs`.
    ///
    /// The real OS spelled out, which is what [`Engine::new`] does; a test passes fakes of its own instead.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Kqueue, Options, System, SystemClock, SystemPrograms};
    ///
    /// let e: Engine<System> = Engine::with_backend(Options::default(), Kqueue::new()?, SystemClock, SystemPrograms);
    /// assert!(e.session_names().is_empty());
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn with_backend(opts: Options, poller: B::Poller, clock: B::Clock, programs: B::Programs) -> Self {
        let now = clock.now_ms();
        Self {
            poller,
            clock,
            programs,
            changes: Vec::new(),
            slots: Slots::default(),
            free: vec![],
            names: Names::default(),
            next_generation: 1,
            buffers: HashMap::new(),
            due: vec![],
            kills: vec![],
            last_sweep: now,
            trimmed: true,
            stats: Stats { slurp_max: opts.slurp_max, ..Stats::default() },
            opts,
            // 64 a wait: 2 KB, where 256 (8 KB) sat on a page of its own size class.
            events: vec![Event::EMPTY; 64],
            viewers: HashMap::new(),
            next_viewer: 1,
            viewer_events: Vec::new(),
            next_wait: 0,
            env: Vec::new(),
            tap: tap::Tap::default(),
            instance: crate::sys::time::unix_nanos(),
        }
    }

    /// The queue the engine waits on.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Event, Options};
    ///
    /// let e = Engine::new(Options::default());
    /// let mut events = vec![Event::EMPTY; 4];
    /// // Nothing registered yet: a short wait on the engine's kqueue times out.
    /// assert_eq!(e.poller().wait(&[], &mut events, 1)?, 0);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn poller(&self) -> &B::Poller {
        &self.poller
    }

    /// Registers the host's own `fd` on the engine's queue: its events come back from `poll` (or
    /// `handle`) with `tag | HOST_TAG` as their tag.
    ///
    /// # Example
    ///
    /// A host's own fd on the engine's queue: its events come back in `host`, the engine's are handled
    /// inside.
    ///
    /// ```
    /// use std::io::Write as _;
    /// use std::os::fd::AsFd as _;
    /// use zeropty::engine::{Engine, Interest, Options, HOST_TAG};
    ///
    /// let mut e = Engine::new(Options::default());
    /// let (mut tx, rx) = std::os::unix::net::UnixStream::pair()?;
    /// e.watch(rx.as_fd(), Interest::Read { lowat: 1 }, 7)?;
    /// tx.write_all(b"x")?;
    ///
    /// let mut host = Vec::new();
    /// e.try_poll(Some(1000), &mut host)?;
    /// assert_eq!(host.len(), 1);
    /// assert_eq!(host[0].tag(), 7 | HOST_TAG);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn watch(&self, fd: BorrowedFd<'_>, interest: Interest, tag: usize) -> std::io::Result<()> {
        self.poller.apply(&[Change::new(fd.as_raw_fd(), interest, tag | HOST_TAG)])
    }

    /// Waits for events, at most `max_ms` (and no longer than the next timer), handles the engine's, and
    /// appends the host's to `host`. A failure of the queue itself is printed to stderr and the call
    /// sleeps out its wait instead (`try_poll` returns it).
    #[deprecated(since = "0.2.0", note = "use try_poll, which returns the queue's failures")]
    pub fn poll(&mut self, max_ms: Option<u64>, host: &mut Vec<Event>) {
        if let Err(e) = self.try_poll(max_ms, host) {
            eprintln!("zeropty: event queue: {e}");
            // Not a busy loop on a broken queue.
            std::thread::sleep(std::time::Duration::from_millis(self.next_timer_ms().clamp(1, 1000)));
        }
    }

    /// Waits for events, at most `max_ms` (and no longer than the next timer), handles the engine's, and
    /// appends the host's to `host`.
    ///
    /// The pending filter changes go with the wait. One of them refused (its fd closed: its session is
    /// going away) is no error: the changes are applied one by one, the refused ones dropped, and the
    /// wait made again without them. An error is the queue itself failing; nothing was handled.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let mut e = Engine::new(Options::default());
    /// let mut host = Vec::new();
    /// let start = std::time::Instant::now();
    /// e.try_poll(Some(20), &mut host)?; // no sessions, no fds of the host's: it waits the 20 ms out
    /// assert!(host.is_empty() && start.elapsed() >= std::time::Duration::from_millis(15));
    /// e.tick();
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn try_poll(&mut self, max_ms: Option<u64>, host: &mut Vec<Event>) -> std::io::Result<()> {
        let mut wait = self.next_timer_ms();
        if let Some(m) = max_ms {
            wait = wait.min(m);
        }
        let changes = std::mem::take(&mut self.changes);
        let mut events = std::mem::take(&mut self.events);
        let r = match self.poller.wait(&changes, &mut events, wait) {
            Err(_) if !changes.is_empty() => {
                for c in &changes {
                    // Refused only for an fd already closed, whose session is gone with it.
                    let _ = self.poller.apply(std::slice::from_ref(c));
                }
                self.poller.wait(&[], &mut events, wait)
            }
            r => r,
        };
        let r = r.map(|n| self.handle(&events[..n], host));
        self.events = events;
        r
    }

    /// Milliseconds until the engine has something to do on its own (a due read, a sweep, a kill).
    ///
    /// ```
    /// let e = zeropty::engine::Engine::new(Default::default());
    /// assert!(e.next_timer_ms() <= 1000); // the next sweep of quiet screens
    /// ```
    pub fn next_timer_ms(&self) -> u64 {
        let mut next = self.last_sweep + SWEEP_EVERY_MS;
        for &(at, _) in &self.due {
            next = next.min(at)
        }
        for &(at, _) in &self.kills {
            next = next.min(at)
        }
        next.saturating_sub(self.clock.now_ms())
    }

    /// Applies the filter changes made since the last wait now, for a host that waits on the queue
    /// without holding the engine (crate::embed): a change taken along into such a wait could land after
    /// its fd was closed and reused by a new session.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Event, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-changes", "exec cat"]), "/");
    /// e.apply_changes(); // in the queue now, before the host waits on it by itself
    /// assert!(!e.has_pending_changes());
    ///
    /// // A wait of the host's own, on a handle of the queue (embed::Embedded waits so, unlocked).
    /// let queue = e.poller().try_clone().unwrap();
    /// let mut events = vec![Event::EMPTY; 8];
    /// let n = queue.wait(&[], &mut events, 10).unwrap();
    /// e.handle(&events[..n], &mut Vec::new());
    /// e.tick();
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn apply_changes(&mut self) {
        let changes = std::mem::take(&mut self.changes);
        // Refused only for an fd already closed, whose session is gone with it.
        let _ = self.poller.apply(&changes);
    }

    /// Whether filter changes wait for the next wait (or `apply_changes`).
    ///
    /// ```
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = zeropty::engine::Engine::new(Default::default());
    /// assert!(!e.has_pending_changes());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-pending", "exec cat"]), "/");
    /// e.apply_changes();
    /// assert!(!e.has_pending_changes());
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn has_pending_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Handles events from a wait on the queue: the engine's, and the host's appended to `host`.
    ///
    /// # Example
    ///
    /// A host that waits on the queue by itself (another thread may hold the engine meanwhile, as in
    /// [`crate::embed::Embedded`]) hands the events over here:
    ///
    /// ```
    /// use zeropty::engine::{Engine, Event, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-handle", "echo hi; exec cat"]), "/");
    /// let queue = e.poller().try_clone().unwrap();
    /// let mut events = vec![Event::EMPTY; 64];
    /// let screen = |e: &mut Engine| e.command(&argv(&["capture-pane", "-p", "-t", "=doc-handle:"]), "/").out;
    ///
    /// let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    /// while !screen(&mut e).starts_with("hi\n") && std::time::Instant::now() < deadline {
    ///     e.apply_changes(); // before a wait that does not take them along
    ///     let n = queue.wait(&[], &mut events, e.next_timer_ms().min(50)).unwrap();
    ///     let mut host = Vec::new();
    ///     e.handle(&events[..n], &mut host);
    ///     e.tick();
    /// }
    /// assert!(screen(&mut e).starts_with("hi\n"));
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn handle(&mut self, events: &[Event], host: &mut Vec<Event>) {
        self.stats.loops += 1;
        self.stats.events += events.len() as u64;
        for ev in events {
            if ev.tag() & HOST_TAG != 0 {
                host.push(*ev);
            } else {
                self.event(ev);
            }
        }
    }

    fn event(&mut self, ev: &Event) {
        let key = SessKey::from_udata(ev.tag());
        let (kind, ident) = (ev.kind(), ev.ident());
        if kind == Kind::Exited {
            // Reap whether or not the session is still listed: kill-session forgets it before it exits.
            self.programs.reap(ident);
        }
        let now = self.clock.now_ms();
        let Some(s) = self.slots.find_mut(key) else { return };
        match kind {
            Kind::Readable => {
                if !s.due {
                    s.due = true;
                    // Someone watching sees it as it comes, not 300 ms at a time.
                    let interval = if s.burst || s.viewed() || s.waited() { 1 } else { coalesce_ms() };
                    self.due.push((policy::coalesced_read_at(now, interval), key));
                    // Meanwhile only a pipe filling up is reported: a program writing a lot would otherwise
                    // block on a full pipe until the boundary (cat of 5 MB: 0.6 s at 300 ms, tmux 0.16 s).
                    if let Some(out) = s.out_fd() {
                        self.changes.push(Change::new(out, Interest::Read { lowat: BURST_LOWAT }, key.udata()));
                    }
                } else {
                    // BURST_LOWAT bytes are waiting: read them now.
                    self.drain_now(key.slot);
                }
            }
            Kind::Writable if s.is_stdin(ident) => s.flush(&self.poller, key.udata()),
            Kind::Writable => {
                if let Some(x) = s.extra.as_mut().filter(|x| x.pipe.as_ref().is_some_and(|p| p.fd.as_raw_fd() == ident))
                    && !x.pipe.as_mut().unwrap().flush(&self.poller)
                {
                    x.pipe = None;
                }
            }
            // The session ends with its program, like a tmux pane without remain-on-exit.
            Kind::Exited if ident == s.pid => self.remove(key.slot),
            // A pipe-pane command ended (reaped above).
            Kind::Exited => {
                if let Some(x) = s.extra.as_mut().filter(|x| x.pipe.as_ref().is_some_and(|p| p.pid == ident)) {
                    x.pipe = None;
                }
                s.trim_extra();
            }
            Kind::Other => {}
        }
    }

    /// Reads that are due, and every SWEEP_EVERY_MS the freezing and compressing of quiet screens.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-tick", "exec cat"]), "/");
    /// for _ in 0..3 {
    ///     e.try_poll(Some(10), &mut Vec::new()).unwrap();
    ///     e.tick();
    /// }
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn tick(&mut self) {
        self.tick_until(&|| false)
    }

    /// `tick`, stopping between reads once `stop` says so (a host thread waits for the engine); what is
    /// left is due at once, for the next call.
    ///
    /// ```
    /// use std::sync::atomic::{AtomicBool, Ordering};
    ///
    /// let mut e = zeropty::engine::Engine::new(Default::default());
    /// let wanted = AtomicBool::new(false); // set by a host thread that needs the engine now
    /// e.try_poll(Some(0), &mut Vec::new()).unwrap();
    /// e.tick_until(&|| wanted.load(Ordering::Relaxed));
    /// ```
    pub fn tick_until(&mut self, stop: &dyn Fn() -> bool) {
        let now = self.clock.now_ms();
        let mut due = std::mem::take(&mut self.due);
        let mut drained = 0;
        let mut watched = Vec::new();
        due.retain(|&(at, key)| {
            // A few at a time: the rest are due at once, so the loop polls again without waiting, and a
            // command that came meanwhile is answered first (400 sessions read in one go held one ~10 ms).
            if at > now || drained >= DRAIN_BATCH || (drained > 0 && stop()) {
                return true;
            }
            drained += 1;
            // Not due any more: read early (pipe filling up, or a capture).
            if let Some(s) = self.slots.find_mut(key).filter(|s| s.due) {
                let before = self.stats.read_bytes;
                s.drain(&mut self.stats, now, &self.poller);
                self.tap.after_read(&mut self.stats, &s.name, before);
                if let Some(out) = s.out_fd() {
                    self.changes.push(Change::read(out, key.udata()));
                }
                if s.extra.as_ref().is_some_and(|x| !x.forward.is_empty() || !x.line.text.is_empty()) {
                    watched.push(key.slot);
                }
            }
            false
        });
        self.due.extend(due);
        for slot in watched {
            self.after_read(slot);
        }
        self.kills.retain(|&(at, pgid)| {
            if at <= now {
                self.programs.signal_group(pgid, libc::SIGKILL);
            }
            at > now
        });
        if now >= self.last_sweep + SWEEP_EVERY_MS {
            self.last_sweep = now;
            self.sweep(now);
        }
    }

    /// Freezes screens quiet for their hold, compresses those quiet for long, and once everything is
    /// frozen and quiet, gives scratch memory back.
    fn sweep(&mut self, now: u64) {
        let (mut deepened, mut live, mut latest) = (false, false, 0);
        for s in self.slots.iter_mut() {
            latest = latest.max(s.last_output_ms);
            // Not while it is being read either: a screen captured over and over stays a Term.
            let active = s.last_output_ms.max(s.last_read_ms);
            let deep = matches!(&s.term, Screenful::Frozen(f) if f.is_deep());
            match policy::sweep(now, active, s.hold, s.frozen(), deep) {
                Sweep::Deepen => {
                    if let Screenful::Frozen(f) = &mut s.term {
                        f.deepen();
                        deepened = true;
                    }
                }
                Sweep::Freeze => s.freeze(),
                Sweep::Keep => {
                    if let Screenful::Live(t) = &mut s.term {
                        // Busy: its rows are packed after each read already; a capture may have unpacked some.
                        t.settle();
                        live = true;
                    }
                }
            }
        }
        if live || deepened {
            self.trimmed = false;
        }
        if deepened {
            // Screens just compressed freed most of their bytes; without this mimalloc v2 kept the
            // pages (400 idle sessions: 2.1 MB, against 1.8 MB with v3).
            if let Some(f) = self.opts.after_compress {
                f();
            }
        } else if !live && !self.trimmed && now >= latest + IDLE_TRIM_MS {
            // Every screen frozen and quiet a while: the scratch buffers would only pin their pages,
            // and what the last parsing freed is given back (400 idle sessions: ~40 KB more).
            crate::term::release_scratch();
            session::release_read_buffer();
            if let Some(f) = self.opts.after_compress {
                f();
            }
            self.trimmed = true;
        }
    }

    /// Puts session `s` in a free slot, under its name; returns the slot.
    fn insert(&mut self, s: Sess) -> usize {
        let slot = self.free_slot();
        self.names.insert(&s.name, slot);
        self.tap.emit(tap::kind::SESSION_CREATED, || tap::Event::SessionCreated { session: s.name.to_string() });
        self.slots.put(slot, s);
        slot
    }

    /// Registers session `slot`'s output and its program's exit; false when the program is gone already
    /// (its exit event would never come), for the caller to remove it.
    fn watch_session(&mut self, slot: usize) -> bool {
        let key = self.key(slot);
        let s = self.slots.get(slot).expect("a session in this slot");
        // Its fds may have been another session's, closed since with a change for them still pending:
        // sent with the next wait, that change would register the fd again under the old tag.
        let (out, stdin) = (s.out_fd(), s.stdin.as_ref().map(|f| f.as_raw_fd()));
        self.changes.retain(|c| Some(c.ident()) != out && Some(c.ident()) != stdin);
        let exit = Change::new(s.pid, Interest::Exit, key.udata());
        let r = match out {
            Some(out) => self.poller.apply(&[Change::read(out, key.udata()), exit]),
            None => self.poller.apply(&[exit]),
        };
        if r.is_err() {
            // ESRCH: already gone. Reaped here, as no event will.
            self.programs.reap(s.pid);
            return false;
        }
        true
    }

    fn remove(&mut self, slot: usize) {
        self.remove_as(slot, tap::ExitReason::Exited)
    }

    /// Forgets session `slot`, closing its pipes; with `handed_over`, its program goes on elsewhere and
    /// keeps its size file.
    fn remove_keeping(&mut self, slot: usize, handed_over: bool) {
        self.remove_as(slot, if handed_over { tap::ExitReason::HandedOver } else { tap::ExitReason::Exited })
    }

    /// Forgets session `slot` (see remove_keeping), which ended for `why`.
    fn remove_as(&mut self, slot: usize, why: tap::ExitReason) {
        let handed_over = why == tap::ExitReason::HandedOver;
        let size_file = self.slots.get(slot).map(|_| self.size_file_of(slot));
        let key = self.slots.get(slot).map(|s| SessKey { slot, generation: s.generation });
        if let Some(mut s) = self.slots.take(slot) {
            // Pending filter changes name its fds, which a new session may get next.
            let tag = key.expect("taken above").udata();
            self.changes.retain(|c| c.tag() != tag);
            s.close();
            self.names.remove(&s.name, slot, &self.slots);
            self.free.push(slot);
            if !handed_over {
                self.programs.remove_size(&size_file.expect("taken above"));
            }
            for v in s.extra.map(|x| x.viewers).unwrap_or_default() {
                self.viewers.remove(&v);
                self.viewer_events.push((v, ViewerEvent::Ended));
            }
            self.tap.emit(tap::kind::SESSION_EXITED, || tap::Event::SessionExited { session: s.name.into(), reason: why });
        }
    }

    /// The slot of the session `target` names, or the error saying there is none.
    fn find(&self, target: Option<&str>) -> Result<usize, CmdError> {
        self.find_as(target, Lookup::Session)
    }

    /// As [`Engine::find`], for a command whose -t tmux looks up as `kind`, with tmux's error
    /// ([`missing`]).
    fn find_as(&self, target: Option<&str>, kind: Lookup) -> Result<usize, CmdError> {
        let name = session_name(target.unwrap_or(""));
        self.names.find(name, &self.slots).ok_or_else(|| CmdError::new(missing(target, kind)))
    }

    /// Session `slot`, which must be there.
    fn sess(&mut self, slot: usize) -> &mut Sess {
        self.slots.get_mut(slot).expect("a session in this slot")
    }

    fn key(&self, slot: usize) -> SessKey {
        SessKey { slot, generation: self.slots.get(slot).expect("a session in this slot").generation }
    }

    fn free_slot(&mut self) -> usize {
        self.free.pop().unwrap_or_else(|| {
            let first = self.slots.grow();
            self.free.extend((first + 1..first + SLOT_CHUNK).rev());
            first
        })
    }

    fn next_generation(&mut self) -> u32 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        generation
    }

    /// Reads a session's pending output now, outside its coalesced read. A read that was due had the
    /// pipe's filter waiting for BURST_LOWAT bytes; it is put back to report any output, else the
    /// skipped timer left it there and small output went unread until the next capture.
    fn drain_now(&mut self, slot: usize) {
        let now = self.clock.now_ms();
        let s = self.slots.get_mut(slot).expect("a session in this slot");
        let was_due = s.due;
        let before = self.stats.read_bytes;
        s.drain(&mut self.stats, now, &self.poller);
        self.tap.after_read(&mut self.stats, &s.name, before);
        if was_due && let Some(out) = s.out_fd() {
            let change = Change::read(out, SessKey { slot, generation: s.generation }.udata());
            self.changes.push(change);
        }
        self.after_read(slot);
    }

    /// After a read of session `slot`: the line typed while canonical goes over if the program is raw
    /// now, and output read for attached terminals is handed to them.
    fn after_read(&mut self, slot: usize) {
        self.hand_line_over(slot);
        self.pass_on(slot);
    }

    /// After a read of session `slot`: a line typed while its tty was canonical goes to the program once
    /// the read shows it is not any more (raw mode), as a tty hands its line over when the mode changes.
    fn hand_line_over(&mut self, slot: usize) {
        let Some(s) = self.slots.get_mut(slot) else { return };
        if s.tty & TTY_ICANON != 0 {
            return;
        }
        let tag = SessKey { slot, generation: s.generation }.udata();
        // A program whose input is closed has nothing to hand the line to.
        let _ = s.hand_line_over(&self.poller, tag);
        s.trim_extra();
    }

    /// Echo waiting to be drawn: a read now, as for output that just came (its own read draws it first).
    /// An embedded engine's thread is not woken for it (that cost the next command its lock, p95 4x): it
    /// draws it at its next timer, and a capture or grep-panes draws it first anyway.
    fn echo_soon(&mut self, slot: usize) {
        let key = self.key(slot);
        let now = self.clock.now_ms();
        let s = self.slots.get_mut(slot).expect("a session in this slot");
        if !s.due {
            s.due = true;
            self.due.push((now, key));
        }
    }

    /// Before keys for a session whose tty is not raw: output not read yet may say the program has made it
    /// raw since (the shim's OSC zeropty-tty), which the keys must go through. A raw one (claude) is left to
    /// its coalesced read: it turns cooked again only as its program ends.
    fn modes_now(&mut self, slot: usize) {
        if self.slots.get(slot).is_some_and(|s| s.tty != 0) {
            self.drain_now(slot);
        }
    }

    /// Keys for session `slot`'s program, through its line discipline; echo is drawn soon.
    fn type_keys(&mut self, slot: usize, keys: &[u8]) -> Result<(), CmdError> {
        let tag = self.key(slot).udata();
        let s = self.slots.get_mut(slot).expect("a session in this slot");
        if s.input(&self.poller, &self.programs, tag, keys)? {
            self.echo_soon(slot);
        }
        Ok(())
    }

    /// Marks session `slot` as being read: a frozen screen is thawed, and it is not frozen again while
    /// reads keep coming (each capture of a frozen screen decoded a copy of it, deep ones from the cold
    /// store: ~40% of the capture).
    fn read_now(&mut self, slot: usize) {
        let now = self.clock.now_ms();
        let s = self.sess(slot);
        s.last_read_ms = now;
        s.live();
    }

    fn size_file(&self, generation: u32) -> PathBuf {
        self.opts.size_dir.join(format!("zeropty-size-{}-{:x}-{generation}", std::process::id(), self.instance))
    }

    /// Where session `slot`'s program reads its size: a file of this engine's, or the one it was started
    /// with elsewhere.
    fn size_file_of(&self, slot: usize) -> PathBuf {
        let s = self.slots.get(slot).expect("a session");
        match s.extra.as_ref().and_then(|x| x.size_file.as_ref()) {
            Some(p) => p.to_path_buf(),
            None => self.size_file(s.generation),
        }
    }

    /// The session at a new size: its screen, the size its program reads (the size file, then SIGWINCH).
    fn resize_session(&mut self, slot: usize, rows: u16, cols: u16) {
        let (rows, cols) = (rows.clamp(1, crate::term::MAX_SIZE), cols.clamp(1, crate::term::MAX_SIZE));
        let file = self.size_file_of(slot);
        let s = self.sess(slot);
        if (s.rows, s.cols) == (rows, cols) {
            return;
        }
        (s.rows, s.cols) = (rows, cols);
        let t = s.live();
        t.resize(rows, cols);
        t.pack_live();
        let pid = s.pid;
        self.programs.write_size(&file, rows, cols);
        self.programs.signal_group(pid, libc::SIGWINCH);
    }

    /// Like tmux's kill-server: every session's process group gets SIGHUP. Without this, sessions outlive
    /// the server and pile up against the per-user process limit.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-hangup", "exec cat"]), "/");
    /// e.hangup_all(); // a host shutting down: cat gets SIGHUP and exits
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn hangup_all(&self) {
        for s in self.slots.iter() {
            self.programs.signal_group(s.pid, libc::SIGHUP);
        }
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
