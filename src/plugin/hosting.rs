//! How hosts run a registry: what `host::HostLoop` calls (its wake-up event on the engine's queue,
//! `pump` after every turn, the watchdog bounding the loop's sleep; crate::host makes the registry a
//! `Service`), the routing of the plugins' commands the hosts call, and [`Call`], a plugin command in
//! flight that a host polls instead of blocking on it.
use super::api::{Reply, fail};
use super::registry::{PLUGIN_TAG, Plugins, ReplyTo, Ticket, WAKE_IDENT, now_ms, send_call};
use crate::engine::{Backend, Change, Engine, Event, Interest, Kqueue};
use std::os::fd::RawFd;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Weak};
use std::time::Duration;

impl Plugins {
    /// Lets the plugins wake the host's loop through engine `e`'s queue: their replies and requests then
    /// reach the engine at once, not at the loop's next timer. zeropty's hosts (`Embedded`, `Polled`,
    /// `zeropty serve`) do this themselves; a host running its own loop over `host::HostLoop` calls it
    /// once, after `HostLoop::set_plugins`. It takes the engine, not its queue, so the stable surface
    /// does not name the poller type (a kqueue today).
    pub fn attach(&self, e: &Engine) -> std::io::Result<()> {
        self.attach_queue(e.poller())
    }

    /// `attach` from a handle on the engine's queue: the hosts whose engine lives on another thread
    /// (`Embedded`) hold one.
    pub(crate) fn attach_queue(&self, kq: &Kqueue) -> std::io::Result<()> {
        kq.apply(&[Change::new(WAKE_IDENT as RawFd, Interest::User, PLUGIN_TAG)])?;
        *self.shared.kq.write().unwrap() = Some(kq.try_clone()?);
        Ok(())
    }

    /// Whether `attach` was called (and `shutdown` not since).
    pub(crate) fn is_attached(&self) -> bool {
        self.shared.kq.read().unwrap().is_some()
    }

    /// Whether an event the engine handed back (HOST_TAG) is the plugins' wake-up.
    #[inline]
    pub(crate) fn owns(ev: &Event) -> bool {
        ev.tag() == PLUGIN_TAG
    }

    /// The host's part, on the engine's thread, once per turn of its loop: the events the engine queued go
    /// to the plugins that want them, and what plugins asked of the engine is run. With no plugin it
    /// returns at once.
    #[inline]
    pub(crate) fn pump<B: Backend>(&self, e: &mut Engine<B>) {
        let s = &*self.shared;
        if s.live.load(Relaxed) == 0 && !s.dirty.load(Relaxed) && s.waiting.load(Relaxed) == 0 {
            return;
        }
        self.pump_now(e);
    }

    /// Hands `argv` to the plugin answering it, None if none does (one atomic load when no plugin adds
    /// commands). The reply comes through the returned Call; with `wake`, the engine's loop is woken when
    /// it does (a server answering its client then).
    #[inline]
    pub(crate) fn call(&self, argv: &[String], cwd: &str, wake: bool) -> Option<Call> {
        // Inlined into every host's command path: the load is all it costs there with no plugin command.
        if !self.shared.commands.load(Relaxed) {
            return None;
        }
        self.call_plugin(argv, cwd, wake)
    }

    /// `call` once some plugin adds commands: finds the one answering `argv`, if any.
    #[cold]
    #[inline(never)]
    fn call_plugin(&self, argv: &[String], cwd: &str, wake: bool) -> Option<Call> {
        let cmd = argv.first()?;
        let (to, tx) = self.target(cmd)?;
        let (rtx, rx) = mpsc::channel();
        let ticket = Ticket::new();
        send_call(&to, &tx, cmd, argv.to_vec(), cwd.to_owned(), ReplyTo { tx: rtx, wake: wake.then(|| self.shared.clone()), ticket: ticket.clone() });
        Some(Call::new(rx, self.shared.timeout_ms.load(Relaxed), cmd, ticket))
    }

    /// The longest the host's loop may sleep while plugins run: the stuck-hook watchdog runs in each
    /// turn, so the loop turns at least this often even when nothing happens. A quarter of the deadline
    /// (10 ms to 1 s); None with no plugin (no cap: an idle host sleeps as long as the engine allows).
    #[inline]
    pub fn watchdog_ms(&self) -> Option<u64> {
        if self.shared.live.load(Relaxed) == 0 {
            return None;
        }
        // A hook stuck past the deadline is cut off within 1.25 deadlines, and an idle server with
        // plugins wakes at most once a second for it.
        Some((self.shared.timeout_ms.load(Relaxed) / 4).clamp(10, 1000))
    }
}

/// A command in flight: handed to the plugin answering it, its reply to be picked up later, so the
/// caller's thread is never held while a plugin works. `Polled::command_start`, `Embedded::command_start`
/// and the C and Node bindings hand these out (an engine command comes back already answered), and the
/// socket server keeps one per waiting client.
///
/// A polled host polls it after each `process`: the plugin's reply turns the engine's fd readable, and
/// `Polled::timeout_ms` counts its deadline, so a host that waits on both never spins. Dropped before
/// its reply, a call the plugin did not start yet is never run.
pub struct Call {
    rx: Receiver<Reply>,
    deadline: u64,
    plugin: Box<str>,
    ticket: Arc<Ticket>,
    /// True while the call is waiting for its reply: false once `poll` or `wait` gave it. Dropped with
    /// the call, so a host keeping the deadline (`Due`) sees an answered or dropped call alike.
    open: Arc<AtomicBool>,
}

/// A started call's deadline as a host keeps it (crate::embed::Polled, for `timeout_ms`): due only while
/// the call is still waiting, so an answered or dropped call no longer wakes the host's loop.
pub(crate) struct Due {
    pub(crate) at: u64,
    open: Weak<AtomicBool>,
}

impl Due {
    /// Whether the call still waits for its reply (not answered, not dropped).
    pub(crate) fn open(&self) -> bool {
        self.open.upgrade().is_some_and(|o| o.load(Relaxed))
    }
}

impl Call {
    pub(super) fn new(rx: Receiver<Reply>, deadline_ms: u64, plugin: &str, ticket: Arc<Ticket>) -> Self {
        Self { rx, deadline: now_ms() + deadline_ms, plugin: plugin.into(), ticket, open: Arc::new(AtomicBool::new(true)) }
    }

    /// A call answered already (an engine command run at once).
    pub fn ready(r: Reply) -> Self {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(r);
        let ticket = Ticket::new();
        ticket.start();
        Self::new(rx, 0, "", ticket)
    }

    /// The reply if it came, or a failure once the plugin stopped or the deadline passed; None meanwhile.
    /// Gives the reply once: after that, `poll` reports the misuse instead.
    pub fn poll(&self) -> Option<Reply> {
        if self.answered() {
            return Some(fail("call: answered already\n"));
        }
        let r = match self.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Disconnected) => self.stopped(),
            Err(TryRecvError::Empty) if now_ms() >= self.deadline => self.timed_out(),
            Err(TryRecvError::Empty) => return None,
        };
        self.open.store(false, Relaxed);
        Some(r)
    }

    /// Blocks for the reply, up to the deadline (for a caller with nothing else to do meanwhile).
    pub fn wait(self) -> Reply {
        if let Some(r) = self.poll() {
            return r;
        }
        let r = match self.rx.recv_timeout(Duration::from_millis(self.due_ms())) {
            Ok(r) => r,
            Err(RecvTimeoutError::Disconnected) => self.stopped(),
            Err(RecvTimeoutError::Timeout) => self.timed_out(),
        };
        self.open.store(false, Relaxed);
        r
    }

    /// Milliseconds until the deadline, when `poll` gives up on the plugin.
    pub fn due_ms(&self) -> u64 {
        self.deadline.saturating_sub(now_ms())
    }

    /// Whether `poll` gave its reply already.
    pub fn answered(&self) -> bool {
        !self.open.load(Relaxed)
    }

    /// The deadline on the registry's clock, valid while the call waits (crate::embed::Polled keeps
    /// them for `timeout_ms`).
    pub(crate) fn due(&self) -> Due {
        Due { at: self.deadline, open: Arc::downgrade(&self.open) }
    }

    fn stopped(&self) -> Reply {
        fail(format!("{}: plugin stopped without replying\n", self.plugin))
    }
    /// Past the deadline: dropped from the plugin's queue if it did not start on it yet.
    fn timed_out(&self) -> Reply {
        if self.ticket.cancel() {
            fail(format!("{}: plugin timed out (busy; not run)\n", self.plugin))
        } else {
            fail(format!("{}: plugin timed out\n", self.plugin))
        }
    }
}

impl Drop for Call {
    /// Given up before its reply: not run if the plugin did not start on it.
    fn drop(&mut self) {
        if !self.answered() {
            self.ticket.cancel();
        }
    }
}
