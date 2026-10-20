//! The registry a host keeps ([`Plugins`]) and the machinery behind the stable API: each plugin's
//! thread and queue, the routing of commands, the watchdog, the inbox of what plugins ask of the engine.
//! Nothing here is for plugin authors (they see `api`); hosts use [`Plugins`]' methods, all stable, and
//! run it from their loop as a `host::Service` (hosting.rs).
use super::api::{API_VERSION, Access, Event, Events, Host, Manifest, Plugin, PluginError, Reply, fail, ok};
use crate::engine::{Backend, Engine, HOST_TAG, Kqueue};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// What no plugin may run through `Host::command`, whatever its access: ending the host's sessions all
/// at once, and the handing over of sessions (it blocks the engine's thread for the handover).
const DENIED: &[&str] = &["kill-server", "pull-sessions", "push-sessions"];

/// Events waiting in a plugin's queue; more are dropped (and counted) until it catches up.
const QUEUE: usize = 1024;
/// Output bytes waiting in a plugin's queue; an Output event past them is dropped (and counted) as if
/// the queue were full. Counted alone, 1024 events could hold any number of bytes: a 200 MB burst and
/// one plugin that never read took the server from 141 MB to 656 MB. Output events are at most
/// engine::tap::CHUNK (64 KB), so a queue holds at most this plus one of them.
const QUEUE_BYTES: usize = 8 << 20;
/// A plugin whose queue filled up is "open" again (the engine copies output for it again) once its queue
/// is down to this, and its bytes to QUEUE_BYTES / 4.
const DRAINED: usize = QUEUE / 4;
/// Failed plugins kept for list-plugins (the oldest go first): programs that reconnect under new names
/// must not grow the registry without end.
const KEEP_FAILED: usize = 16;
/// How long a plugin has to answer a command, and to come back from a hook, unless told otherwise
/// (ZEROPTY_PLUGIN_TIMEOUT_MS, Plugins::set_timeout).
const TIMEOUT_MS: u64 = 10_000;

/// How long `shutdown` (and so dropping an Embedded or Polled engine) waits for the plugins' threads to
/// end, unless told otherwise (Plugins::set_shutdown_wait).
const SHUTDOWN_WAIT_MS: u64 = 1_000;

/// The longest `add` waits for a plugin's `manifest()` (less if the registry's deadline is shorter).
/// A manifest is built from constants in microseconds; this caps what a stuck one costs its caller, who
/// may be a host's loop (`Polled::add_plugin`).
const MANIFEST_MS: u64 = 1_000;

/// The kqueue user event plugins wake the engine's loop with (crate::embed's is ident 1).
pub(super) const WAKE_IDENT: usize = 0x7a70;
/// Its tag: HOST_TAG and a bit of its own, apart from crate::server's (1 << 62).
pub(crate) const PLUGIN_TAG: usize = HOST_TAG | 1 << 61;

/// The plugins of one engine. Cloning gives another handle to the same registry.
#[derive(Clone)]
pub struct Plugins {
    pub(super) shared: Arc<Shared>,
}

pub(super) struct Shared {
    entries: Mutex<Vec<Entry>>,
    /// Plugins running: none, and `pump` returns at once.
    pub(super) live: AtomicUsize,
    /// A running plugin answers commands: none, and callers do not look.
    pub(super) commands: AtomicBool,
    /// The engine must be told the events wanted (a plugin came or went, a queue filled or drained).
    pub(super) dirty: AtomicBool,
    inbox: Mutex<Vec<Request>>,
    pub(super) waiting: AtomicUsize,
    /// A handle on the engine's kqueue (a dup: it stays valid whatever the engine does), set by
    /// `attach`, taken by `shutdown`. Locked only to wake the loop, never by the engine's paths.
    pub(super) kq: RwLock<Option<Kqueue>>,
    pub(super) timeout_ms: AtomicU64,
    pub(super) shutdown_wait_ms: AtomicU64,
    pub(super) next_id: AtomicU64,
}

struct Entry {
    manifest: Manifest,
    /// None once it stopped or failed (it is still listed then).
    tx: Option<SyncSender<Msg>>,
    state: Arc<State>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Entry {
    /// Gets events and commands: not stopped, not removed, and not failed. Its thread records a failure
    /// (a panic) before the engine's next pump takes its queue away: in between, it is already out.
    fn live(&self) -> bool {
        self.tx.is_some() && !self.state.gone.load(Relaxed) && self.state.failed.lock().unwrap().is_none()
    }
}

pub(super) struct State {
    id: u64,
    pub(super) name: Box<str>,
    /// Manifest::access.
    access: Access,
    /// Why it stopped working (a panic, a hook past the deadline, a lost connection).
    failed: Mutex<Option<String>>,
    dropped: AtomicU64,
    /// When the hook running now started (ms, see now_ms), 0 between hooks; moved forward by the time
    /// spent waiting in Host::command, so that it measures the plugin's own time.
    busy_since: AtomicU64,
    /// Host::command calls of this plugin waiting for their reply: the watchdog leaves it alone meanwhile.
    on_host: AtomicUsize,
    /// The plugin whose command this one waits for (Host::command of a CALL), 0 if none: what the cycle
    /// check follows.
    awaiting: AtomicU64,
    /// Messages in its queue (events and calls).
    queued: AtomicUsize,
    /// Output bytes in its queue (QUEUE_BYTES).
    queued_bytes: AtomicUsize,
    /// Its queue was full: the engine stops copying output for it until it drained (to DRAINED).
    full: AtomicBool,
    /// Removed: what it asks fails at once.
    gone: AtomicBool,
}

impl State {
    /// One message taken off its queue (on its thread): once a full queue drained, the engine is told.
    fn took(&self, shared: &Shared, msg: &Msg) {
        let left = self.queued.fetch_sub(1, Relaxed).saturating_sub(1);
        let bytes = msg.bytes();
        let bytes_left = if bytes == 0 { self.queued_bytes.load(Relaxed) } else { self.queued_bytes.fetch_sub(bytes, Relaxed) - bytes };
        if left <= DRAINED && bytes_left <= QUEUE_BYTES / 4 && self.full.load(Relaxed) && self.full.swap(false, Relaxed) {
            shared.dirty.store(true, Relaxed);
            shared.wake();
        }
    }

    /// Queues `msg` for it, counting it: Full when its queue holds QUEUE messages or `msg` would take its
    /// output bytes past QUEUE_BYTES.
    fn send(&self, tx: &SyncSender<Msg>, msg: Msg) -> Result<(), TrySendError<Msg>> {
        let bytes = msg.bytes();
        if bytes != 0 && self.queued_bytes.fetch_add(bytes, Relaxed) + bytes > QUEUE_BYTES {
            self.queued_bytes.fetch_sub(bytes, Relaxed);
            return Err(TrySendError::Full(msg));
        }
        self.queued.fetch_add(1, Relaxed);
        tx.try_send(msg).inspect_err(|_| {
            self.queued.fetch_sub(1, Relaxed);
            self.queued_bytes.fetch_sub(bytes, Relaxed);
        })
    }
}

pub(super) enum Msg {
    Event(Arc<Event>),
    Call { argv: Vec<String>, cwd: String, reply: ReplyTo },
    Stop,
}

impl Msg {
    /// The output bytes it holds (State::queued_bytes).
    fn bytes(&self) -> usize {
        match self {
            Msg::Event(ev) => match &**ev {
                Event::Output { data, .. } => data.len(),
                _ => 0,
            },
            _ => 0,
        }
    }
}

/// A command's fate, decided once: started by whoever runs it, or cancelled by its caller giving up
/// first. What was cancelled is never run, so a caller told "timed out, not run" can rely on it.
pub(super) struct Ticket(AtomicU8);

const PENDING: u8 = 0;
const STARTED: u8 = 1;
const CANCELLED: u8 = 2;

impl Ticket {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self(AtomicU8::new(PENDING)))
    }
    /// Whoever runs the command: false if its caller gave up (then it is not run).
    pub(super) fn start(&self) -> bool {
        self.0.compare_exchange(PENDING, STARTED, Relaxed, Relaxed).is_ok()
    }
    /// The caller gives up: true if nobody started it, and nobody will.
    pub(super) fn cancel(&self) -> bool {
        self.0.compare_exchange(PENDING, CANCELLED, Relaxed, Relaxed).is_ok()
    }
    fn cancelled(&self) -> bool {
        self.0.load(Relaxed) == CANCELLED
    }
}

/// Where a command's reply goes, and whether the engine's loop is woken for it (a server answering a
/// client later; a caller blocked on the reply needs no waking).
pub(super) struct ReplyTo {
    pub(super) tx: Sender<Reply>,
    pub(super) wake: Option<Arc<Shared>>,
    pub(super) ticket: Arc<Ticket>,
}

impl ReplyTo {
    fn send(self, r: Reply) {
        let _ = self.tx.send(r);
        if let Some(s) = self.wake {
            s.wake();
        }
    }
}

enum Request {
    /// A plugin's Host::command.
    Command { from: u64, argv: Vec<String>, cwd: String, reply: Sender<Reply>, ticket: Arc<Ticket> },
    /// A plugin leaves (Host::detach: its connection closed).
    Detach(u64),
}

pub(crate) fn now_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    // +1: 0 means "not busy".
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64 + 1
}

impl Shared {
    pub(super) fn wake(&self) {
        if let Some(kq) = self.kq.read().unwrap().as_ref() {
            // Fails only if the queue is gone, with its engine: nobody left to wake.
            let _ = kq.trigger(WAKE_IDENT, PLUGIN_TAG);
        }
    }

    /// Host::command: queued for the engine's thread (pump), waited for up to half the deadline.
    pub(super) fn command_for(&self, st: &State, argv: Vec<String>, cwd: &str) -> Reply {
        if st.gone.load(Relaxed) {
            return fail("plugin removed\n");
        }
        if self.kq.read().unwrap().is_none() {
            return fail("plugin: no engine\n");
        }
        let (tx, rx) = mpsc::channel();
        let cmd = argv.first().cloned().unwrap_or_default();
        let started = now_ms();
        let ticket = Ticket::new();
        st.on_host.fetch_add(1, Relaxed);
        self.inbox.lock().unwrap().push(Request::Command { from: st.id, argv, cwd: cwd.to_owned(), reply: tx, ticket: ticket.clone() });
        self.waiting.fetch_add(1, Relaxed);
        self.wake();
        let limit = self.timeout() / 2;
        let r = rx.recv_timeout(limit);
        // The hook (if one started before this wait) gets the time waited back.
        let waited = now_ms() - started;
        // fetch_update is deprecated for try_update on newer Rust, which the MSRV (1.88) does not have.
        #[allow(deprecated)]
        let _ = st.busy_since.fetch_update(Relaxed, Relaxed, |b| (b != 0 && b <= started).then_some(b + waited));
        st.awaiting.store(0, Relaxed);
        st.on_host.fetch_sub(1, Relaxed);
        match r {
            Ok(r) => r,
            // Not started yet: it never will be. Started (the engine is slow, or another plugin is): its
            // effect may still come, and the caller is told so rather than "failed".
            Err(RecvTimeoutError::Timeout) if ticket.cancel() => {
                fail(format!("{cmd}: no answer within {} ms (half the plugin deadline); not run\n", limit.as_millis()))
            }
            Err(RecvTimeoutError::Timeout) => {
                fail(format!("{cmd}: no answer within {} ms (half the plugin deadline); started, may still take effect\n", limit.as_millis()))
            }
            Err(RecvTimeoutError::Disconnected) => fail("plugin: the engine stopped\n"),
        }
    }

    /// Host::detach: the plugin leaves at the next pump.
    pub(super) fn detach(&self, st: &State) {
        if st.gone.swap(true, Relaxed) {
            return;
        }
        self.inbox.lock().unwrap().push(Request::Detach(st.id));
        self.waiting.fetch_add(1, Relaxed);
        self.wake();
    }

    /// Host::fail: out of routing at once (Entry::live), out of the registry at the next pump.
    pub(super) fn fail(&self, st: &State, why: String) {
        st.failed.lock().unwrap().get_or_insert(why);
        self.dirty.store(true, Relaxed);
        self.wake();
    }

    pub(super) fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.load(Relaxed))
    }

    /// Flags and count from the running plugins; the engine is told the events wanted at the next pump.
    fn recount(&self, entries: &[Entry]) {
        let running = || entries.iter().filter(|e| e.live());
        self.commands.store(running().any(|e| !e.manifest.commands.is_empty()), Relaxed);
        self.live.store(running().count(), Relaxed);
        self.dirty.store(true, Relaxed);
    }
}

/// The events the engine is to queue, and those it is only to count as dropped: OUTPUT and ACTIVITY while
/// every running plugin that wants them has a full queue (copying output nobody can take would cost the
/// engine for nothing).
fn masks(entries: &[Entry]) -> (Events, Events) {
    let (mut wanted, mut open) = (Events::empty(), Events::empty());
    for e in entries.iter().filter(|e| e.live()) {
        wanted |= e.manifest.events;
        if !e.state.full.load(Relaxed) {
            open |= e.manifest.events;
        }
    }
    let muted = (wanted - open) & (Events::OUTPUT | Events::ACTIVITY);
    (wanted - muted, muted)
}

/// Forgets the oldest failed plugins past KEEP_FAILED.
fn prune(entries: &mut Vec<Entry>) {
    let mut extra = entries.iter().filter(|e| e.tx.is_none()).count().saturating_sub(KEEP_FAILED);
    entries.retain(|e| {
        let drop = extra > 0 && e.tx.is_none();
        extra -= drop as usize;
        !drop
    });
}

impl Default for Plugins {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugins {
    /// An empty registry; its deadline is ZEROPTY_PLUGIN_TIMEOUT_MS (ms), 10 s by default.
    pub fn new() -> Self {
        let timeout = std::env::var("ZEROPTY_PLUGIN_TIMEOUT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(TIMEOUT_MS);
        Self {
            shared: Arc::new(Shared {
                entries: Mutex::new(Vec::new()),
                live: AtomicUsize::new(0),
                commands: AtomicBool::new(false),
                dirty: AtomicBool::new(false),
                inbox: Mutex::new(Vec::new()),
                waiting: AtomicUsize::new(0),
                kq: RwLock::new(None),
                timeout_ms: AtomicU64::new(timeout),
                shutdown_wait_ms: AtomicU64::new(SHUTDOWN_WAIT_MS),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    /// How long a plugin has to answer a command or come back from a hook.
    pub fn set_timeout(&self, t: Duration) {
        self.shared.timeout_ms.store(t.as_millis().max(1) as u64, Relaxed)
    }

    /// The deadline (`set_timeout`).
    pub fn timeout(&self) -> Duration {
        self.shared.timeout()
    }

    /// Checks a manifest as `add` would (the name and commands free), without adding anything.
    pub fn check(&self, m: &Manifest) -> Result<(), PluginError> {
        validate(m).and_then(|()| conflicts(&self.shared.entries.lock().unwrap(), m))
    }

    /// Starts `plugin` on a thread of its own. Fails if its manifest is invalid, asks for a newer API,
    /// or takes a name or command already taken.
    pub fn add(&self, plugin: impl Plugin) -> Result<(), PluginError> {
        self.add_boxed(Box::new(plugin))
    }

    /// `add` for a plugin already boxed (chosen at run time).
    ///
    /// `manifest()` runs first, on the plugin's new thread: `add` waits for it at most the manifest
    /// deadline (the registry's deadline, at most 1 s) and fails with `ManifestTimeout` past it, so a
    /// plugin stuck there holds its caller (for `Polled`, the host's loop) that long at most, never the
    /// engine. A plugin not registered is dropped on its own thread, once `manifest()` returned.
    pub fn add_boxed(&self, plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        let (said, heard) = mpsc::sync_channel::<Result<Manifest, String>>(1);
        let (go, gone) = mpsc::sync_channel::<(Receiver<Msg>, Host)>(1);
        let thread = std::thread::Builder::new()
            .name("zeropty-plugin".into())
            .spawn(move || {
                let m = catch_unwind(AssertUnwindSafe(|| plugin.manifest())).map_err(|p| panic_text(&*p));
                if said.send(m).is_err() {
                    return; // `add` gave up on it (the deadline): dropped here, off the caller's thread.
                }
                // Registered: its queue and handle come. Refused: the sender is dropped, and so is it.
                if let Ok((rx, host)) = gone.recv() {
                    crate::sys::process::name_thread(&format!("zeropty-plugin-{}", host.state.name));
                    run(plugin, rx, host);
                }
            })
            .map_err(|e| PluginError::Thread(e.to_string()))?;
        let wait = self.shared.timeout().min(Duration::from_millis(MANIFEST_MS));
        let manifest = match heard.recv_timeout(wait) {
            Ok(Ok(m)) => m,
            Ok(Err(why)) => return Err(PluginError::ManifestPanicked(why)),
            Err(_) => return Err(PluginError::ManifestTimeout(wait.as_millis() as u64)),
        };
        validate(&manifest)?;
        let mut entries = self.shared.entries.lock().unwrap();
        conflicts(&entries, &manifest)?;
        // A plugin of that name that failed or stopped earlier makes room (a program reconnecting).
        let (old, kept): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut *entries).into_iter().partition(|e| !e.live() && e.manifest.name == manifest.name);
        *entries = kept;
        old.into_iter().for_each(stop);
        let state = Arc::new(State {
            id: self.shared.next_id.fetch_add(1, Relaxed),
            name: manifest.name.as_str().into(),
            access: manifest.access,
            failed: Mutex::new(None),
            dropped: AtomicU64::new(0),
            busy_since: AtomicU64::new(0),
            on_host: AtomicUsize::new(0),
            awaiting: AtomicU64::new(0),
            queued: AtomicUsize::new(0),
            queued_bytes: AtomicUsize::new(0),
            full: AtomicBool::new(false),
            gone: AtomicBool::new(false),
        });
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let host = Host { shared: self.shared.clone(), state: state.clone() };
        // Cannot fail: its thread waits on `gone` (a channel of one) until this is sent or dropped.
        let _ = go.send((rx, host));
        entries.push(Entry { manifest, tx: Some(tx), state, thread: Some(thread) });
        prune(&mut entries);
        self.shared.recount(&entries);
        drop(entries);
        self.shared.wake();
        Ok(())
    }

    /// Stops plugin `name` (its `stop` hook runs on its thread) and forgets it. False if there is none.
    pub fn remove(&self, name: &str) -> bool {
        let mut entries = self.shared.entries.lock().unwrap();
        let Some(i) = entries.iter().position(|e| e.manifest.name == name) else { return false };
        let e = entries.remove(i);
        stop(e);
        self.shared.recount(&entries);
        drop(entries);
        self.shared.wake();
        true
    }

    /// The running plugins' names.
    pub fn names(&self) -> Vec<String> {
        self.shared.entries.lock().unwrap().iter().filter(|e| e.live()).map(|e| e.manifest.name.clone()).collect()
    }

    /// How long `shutdown` waits for the plugins' threads (1 s unless set; zero waits for none).
    pub fn set_shutdown_wait(&self, t: Duration) {
        self.shared.shutdown_wait_ms.store(t.as_millis() as u64, Relaxed)
    }

    /// Stops every plugin: each runs `stop` on its thread after the hook it is in. Waits up to the
    /// shutdown wait (`set_shutdown_wait`, 1 s) for their threads; a thread still in a hook then is left
    /// running, detached (Rust cannot end a thread from outside): it ends when its hook returns, or with
    /// the process. Dropping an Embedded or Polled engine calls this.
    pub fn shutdown(&self) {
        let entries = std::mem::take(&mut *self.shared.entries.lock().unwrap());
        *self.shared.kq.write().unwrap() = None;
        self.shared.recount(&[]);
        let threads: Vec<_> = entries
            .into_iter()
            .filter_map(|mut e| {
                let t = e.thread.take();
                stop(e);
                t
            })
            .collect();
        let end = std::time::Instant::now() + Duration::from_millis(self.shared.shutdown_wait_ms.load(Relaxed));
        for t in threads {
            while !t.is_finished() && std::time::Instant::now() < end {
                std::thread::sleep(Duration::from_millis(1));
            }
            if t.is_finished() {
                let _ = t.join();
            }
        }
    }

    /// Tells the engine which events to queue and which only to count (masks).
    fn tell<B: Backend>(&self, e: &mut Engine<B>) {
        let (wanted, muted) = masks(&self.shared.entries.lock().unwrap());
        // The engine keeps plain masks: its hot paths test one integer.
        e.set_tap(wanted.bits(), muted.bits());
    }

    #[inline(never)]
    pub(super) fn pump_now<B: Backend>(&self, e: &mut Engine<B>) {
        let s = &*self.shared;
        if s.dirty.swap(false, Relaxed) {
            self.tell(e);
        }
        if e.has_tap_events() {
            let (evs, lost) = e.take_tap();
            let entries = s.entries.lock().unwrap();
            if lost.iter().any(|&n| n != 0) {
                // Dropped by the engine's queue (the host pumped too rarely, or every plugin wanting them
                // was full): counted for those wanting them.
                for en in entries.iter().filter(|en| en.tx.is_some()) {
                    let n: u64 = lost.iter().enumerate().filter(|&(k, _)| en.manifest.events.bits() & 1 << k != 0).map(|(_, n)| n).sum();
                    en.state.dropped.fetch_add(n, Relaxed);
                }
            }
            for ev in evs {
                let (kind, ev) = (ev.kind(), Arc::new(ev));
                for en in entries.iter().filter(|en| en.manifest.events.contains(kind)) {
                    if let Some(tx) = &en.tx
                        && let Err(err) = en.state.send(tx, Msg::Event(ev.clone()))
                    {
                        en.state.dropped.fetch_add(1, Relaxed);
                        // Full: the engine is told at the end of this pump (and stops copying output if
                        // nobody else wants it).
                        if matches!(err, TrySendError::Full(_)) && !en.state.full.swap(true, Relaxed) {
                            s.dirty.store(true, Relaxed);
                        }
                    }
                }
            }
        }
        if s.waiting.load(Relaxed) > 0 {
            let requests = std::mem::take(&mut *s.inbox.lock().unwrap());
            s.waiting.fetch_sub(requests.len(), Relaxed);
            for r in requests {
                match r {
                    // Its plugin gave up waiting (Host::command): dropped, as it was told.
                    Request::Command { ticket, .. } if ticket.cancelled() => {}
                    Request::Command { from, argv, cwd, reply, ticket } => self.run_for_plugin(e, from, argv, cwd, reply, ticket),
                    Request::Detach(id) => {
                        let mut entries = s.entries.lock().unwrap();
                        if let Some(i) = entries.iter().position(|en| en.state.id == id) {
                            stop(entries.remove(i));
                            s.recount(&entries);
                        }
                    }
                }
            }
        }
        // Plugins that panicked (their thread said so) or are stuck in a hook past the deadline: cut off.
        let (now, limit) = (now_ms(), s.timeout_ms.load(Relaxed));
        let mut entries = s.entries.lock().unwrap();
        let mut changed = false;
        for en in entries.iter_mut().filter(|en| en.tx.is_some()) {
            // Its own time only: not while it waits for the engine or another plugin (Host::command).
            let busy = en.state.busy_since.load(Relaxed);
            if busy != 0 && en.state.on_host.load(Relaxed) == 0 && now.saturating_sub(busy) > limit {
                en.state.failed.lock().unwrap().get_or_insert_with(|| format!("stuck in a hook for more than {limit} ms"));
            }
            if en.state.failed.lock().unwrap().is_some() {
                // Its thread ends once it comes back and finds its queue closed.
                en.tx = None;
                en.state.gone.store(true, Relaxed);
                changed = true;
            }
        }
        if changed {
            prune(&mut entries);
            s.recount(&entries);
        }
        drop(entries);
        if s.dirty.swap(false, Relaxed) {
            self.tell(e);
        }
    }

    /// A plugin's Host::command, on the engine's thread.
    fn run_for_plugin<B: Backend>(&self, e: &mut Engine<B>, from: u64, argv: Vec<String>, cwd: String, reply: Sender<Reply>, ticket: Arc<Ticket>) {
        let cmd = argv.first().cloned().unwrap_or_default();
        let (name, has) = {
            let entries = self.shared.entries.lock().unwrap();
            match entries.iter().find(|en| en.state.id == from) {
                Some(en) => (en.manifest.name.clone(), en.state.access),
                // Removed meanwhile: what it asked is not run.
                None => {
                    let _ = reply.send(fail("plugin removed\n"));
                    return;
                }
            }
        };
        let lacks = |need: Access| fail(format!("{cmd}: plugin {name} lacks access {need} (Manifest::access, plugin-attach -a)\n"));
        if let Some((to, tx)) = self.target(&cmd) {
            // Another plugin's command: handed to it, its reply going straight to the asking plugin.
            if to.id == from {
                let _ = reply.send(fail(format!("{cmd}: a plugin cannot call its own command\n")));
            } else if !has.contains(Access::CALL) {
                let _ = reply.send(lacks(Access::CALL));
            } else if let Some(path) = self.cycle(from, &to) {
                // `to` waits (through `path`) for the asking plugin: handing it the call would leave both
                // waiting for each other until their deadlines.
                let _ = reply.send(fail(format!("{cmd}: call cycle ({name} -> {path}): plugin {} is waiting for {name}\n", to.name)));
            } else {
                // Whom `from` waits for now, for the cycle check of calls made meanwhile; Host::command clears it.
                if let Some(en) = self.shared.entries.lock().unwrap().iter().find(|en| en.state.id == from) {
                    en.state.awaiting.store(to.id, Relaxed);
                }
                // Still pending: the plugin answering it starts it, unless the caller gave up by then.
                send_call(&to, &tx, &cmd, argv, cwd, ReplyTo { tx: reply, wake: None, ticket });
            }
            return;
        }
        let need = Access::needed(&cmd);
        let r = match cmd.as_str() {
            c if DENIED.contains(&c) => fail(format!("{cmd}: not for plugins\n")),
            // Only the socket's clients can: these hold a connection.
            "attach-session" | "attach" | "a" | "plugin-attach" => fail(format!("{cmd}: needs a connection of its own\n")),
            _ if !has.contains(need) => lacks(need),
            "list-plugins" => self.list(),
            _ if !ticket.start() => return,
            _ => e.command(&argv, &cwd),
        };
        let _ = reply.send(r);
    }

    /// The running plugin answering `cmd`: its state and queue.
    pub(super) fn target(&self, cmd: &str) -> Option<(Arc<State>, SyncSender<Msg>)> {
        if !self.shared.commands.load(Relaxed) {
            return None;
        }
        let entries = self.shared.entries.lock().unwrap();
        let en = entries.iter().find(|en| en.live() && en.manifest.commands.iter().any(|c| c == cmd))?;
        Some((en.state.clone(), en.tx.clone()?))
    }

    /// Whether plugin `to` waits, directly or through others, for plugin `from` (Host::command calls of
    /// other plugins' commands): the chain ("b -> a"), None if it does not.
    fn cycle(&self, from: u64, to: &State) -> Option<String> {
        let entries = self.shared.entries.lock().unwrap();
        let mut path = vec![to.name.to_string()];
        let mut next = to.awaiting.load(Relaxed);
        // Each plugin waits for one other at most: a chain longer than the registry is a cycle elsewhere.
        for _ in 0..entries.len() {
            if next == 0 {
                return None;
            }
            let en = entries.iter().find(|en| en.state.id == next)?;
            path.push(en.manifest.name.clone());
            if next == from {
                return Some(path.join(" -> "));
            }
            next = en.state.awaiting.load(Relaxed);
        }
        None
    }

    /// `list-plugins`: one line each, "name: state (api N) events=... commands=... access=... dropped=N",
    /// the state being running, stopped (removed, or its connection closed, and not gone from the list
    /// yet) or failed (why).
    pub fn list(&self) -> Reply {
        let entries = self.shared.entries.lock().unwrap();
        let mut out = String::new();
        for e in entries.iter() {
            // live() takes `failed`'s lock: first, not under it.
            let live = e.live();
            let state = match (&*e.state.failed.lock().unwrap(), live) {
                (Some(why), _) => format!("failed ({why})"),
                (None, true) => "running".into(),
                (None, false) => "stopped".into(),
            };
            out.push_str(&format!(
                "{}: {state} (api {}) events={} commands={} access={} dropped={}\n",
                e.manifest.name,
                e.manifest.api,
                e.manifest.events,
                e.manifest.commands.join(","),
                e.manifest.access,
                e.state.dropped.load(Relaxed)
            ));
        }
        ok(out)
    }
}

pub(super) fn send_call(to: &State, tx: &SyncSender<Msg>, cmd: &str, argv: Vec<String>, cwd: String, reply: ReplyTo) {
    match to.send(tx, Msg::Call { argv, cwd, reply }) {
        Ok(()) => {}
        Err(TrySendError::Full(Msg::Call { reply, .. })) => reply.send(fail(format!("{cmd}: plugin busy\n"))),
        Err(TrySendError::Disconnected(Msg::Call { reply, .. })) => reply.send(fail(format!("{cmd}: plugin stopped\n"))),
        Err(_) => {}
    }
}

/// Ends a plugin taken out of the registry: Stop goes ahead of what is queued if there is room, and the
/// queue closes, so its thread runs `stop` and ends.
fn stop(mut e: Entry) {
    e.state.gone.store(true, Relaxed);
    if let Some(tx) = e.tx.take() {
        let _ = e.state.send(&tx, Msg::Stop);
    }
}

/// A plugin's thread: its hooks, one at a time, each caught if it panics.
fn run(mut p: Box<dyn Plugin>, rx: Receiver<Msg>, host: Host) {
    let st = host.state.clone();
    let hook = |what: &str, f: &mut dyn FnMut()| -> bool {
        st.busy_since.store(now_ms(), Relaxed);
        let r = catch_unwind(AssertUnwindSafe(f));
        st.busy_since.store(0, Relaxed);
        match r {
            Ok(()) => true,
            Err(payload) => {
                host.fail(format!("panicked in {what}: {}", panic_text(&*payload)));
                false
            }
        }
    };
    if !hook("start", &mut || p.start(&host)) {
        return;
    }
    while let Ok(msg) = rx.recv() {
        st.took(&host.shared, &msg);
        match msg {
            Msg::Event(ev) => {
                if !hook("event", &mut || p.event(&ev, &host)) {
                    return;
                }
            }
            Msg::Call { argv, cwd, reply } => {
                // Its caller gave up while it waited in the queue: not run.
                if !reply.ticket.start() {
                    continue;
                }
                let mut r = None;
                let fine = hook("command", &mut || r = Some(p.command(&argv, &cwd, &host)));
                reply.send(r.unwrap_or_else(|| fail(format!("{}: plugin panicked\n", argv.first().map_or("", String::as_str)))));
                if !fine {
                    return;
                }
            }
            Msg::Stop => break,
        }
    }
    hook("stop", &mut || p.stop(&host));
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "?".into())
}

fn valid_word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b)) && !s.starts_with('-')
}

fn validate(m: &Manifest) -> Result<(), PluginError> {
    let plugin = || m.name.clone();
    if !valid_word(&m.name) {
        return Err(PluginError::BadName(m.name.clone()));
    }
    if m.api == 0 || m.api > API_VERSION {
        return Err(PluginError::Api { plugin: plugin(), api: m.api, supported: API_VERSION });
    }
    // Events and Access hold only known bits: nothing to check there.
    for (i, c) in m.commands.iter().enumerate() {
        if !valid_word(c) {
            return Err(PluginError::BadCommand { plugin: plugin(), command: c.clone() });
        }
        if super::api::is_reserved(c) {
            return Err(PluginError::BuiltinCommand { plugin: plugin(), command: c.clone() });
        }
        if m.commands[..i].contains(c) {
            return Err(PluginError::DuplicateCommand { plugin: plugin(), command: c.clone() });
        }
    }
    Ok(())
}

fn conflicts(entries: &[Entry], m: &Manifest) -> Result<(), PluginError> {
    for e in entries.iter().filter(|e| e.live()) {
        if e.manifest.name == m.name {
            return Err(PluginError::NameTaken(m.name.clone()));
        }
        if let Some(c) = m.commands.iter().find(|c| e.manifest.commands.contains(c)) {
            return Err(PluginError::CommandTaken { plugin: m.name.clone(), command: c.clone(), owner: e.manifest.name.clone() });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin that panicked in `start`, for the registry tests.
    struct Dud(String, Events);
    impl Plugin for Dud {
        fn manifest(&self) -> Manifest {
            Manifest::new(self.0.clone()).events(self.1)
        }
        fn start(&mut self, _: &Host) {
            panic!("dud")
        }
    }

    /// Failed plugins stay listed, but only the last KEEP_FAILED: programs reconnecting under new names
    /// (pid-suffixed, say) do not grow the registry without end.
    #[test]
    fn failed_plugins_are_pruned() {
        let mut e = Engine::new(crate::engine::Options::default());
        let p = Plugins::new();
        p.attach(&e).unwrap();
        for k in 0..KEEP_FAILED * 3 {
            p.add(Dud(format!("dud-{k}"), Events::empty())).unwrap();
            // Its thread says it failed; the pump takes it out.
            let st = p.shared.entries.lock().unwrap().last().unwrap().state.clone();
            while st.failed.lock().unwrap().is_none() {
                std::thread::sleep(Duration::from_millis(1));
            }
            p.pump(&mut e);
            assert!(p.shared.entries.lock().unwrap().len() <= KEEP_FAILED, "{k}");
        }
        let list = p.list().out;
        assert_eq!(list.lines().count(), KEEP_FAILED);
        // The newest are kept.
        assert!(list.contains(&format!("dud-{}: failed (panicked in start: dud)", KEEP_FAILED * 3 - 1)), "{list}");
        assert!(!list.contains("dud-0:"), "{list}");
        p.shutdown();
    }

    /// OUTPUT and ACTIVITY are muted while every plugin wanting them is full, and only then.
    #[test]
    fn full_queues_mute_output() {
        let p = Plugins::new();
        let quiet = |name: &str, ev: Events| {
            struct Q(Manifest);
            impl Plugin for Q {
                fn manifest(&self) -> Manifest {
                    self.0.clone()
                }
            }
            p.add(Q(Manifest::new(name).events(ev))).unwrap();
        };
        quiet("a", Events::ALL);
        quiet("b", Events::OUTPUT);
        let full = |name: &str, f: bool| p.shared.entries.lock().unwrap().iter().find(|e| e.manifest.name == name).unwrap().state.full.store(f, Relaxed);
        let m = || masks(&p.shared.entries.lock().unwrap());
        assert_eq!(m(), (Events::ALL, Events::empty()));
        full("a", true);
        // b still takes output; nobody else wants activity.
        assert_eq!(m(), (Events::SESSIONS | Events::OUTPUT, Events::ACTIVITY));
        full("b", true);
        // Lifecycle events are never muted (a few, and a full queue counts them itself).
        assert_eq!(m(), (Events::SESSIONS, Events::OUTPUT | Events::ACTIVITY));
        full("a", false);
        assert_eq!(m(), (Events::ALL, Events::empty()));
        p.shutdown();
    }

    /// A queue is full at QUEUE_BYTES of output as well as at QUEUE events: the event past it is refused
    /// (and counted by the pump as dropped), while events without bytes still fit; taking reopens it.
    #[test]
    fn a_queue_holds_a_byte_budget() {
        let p = Plugins::new();
        struct Q;
        impl Plugin for Q {
            fn manifest(&self) -> Manifest {
                Manifest::new("q").events(Events::OUTPUT)
            }
        }
        p.add(Q).unwrap();
        let st = p.shared.entries.lock().unwrap()[0].state.clone();
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let chunk = crate::engine::tap::CHUNK;
        let out = || Msg::Event(Arc::new(Event::Output { session: "a".into(), data: vec![0; chunk] }));
        let mut sent = 0;
        while st.send(&tx, out()).is_ok() {
            sent += 1;
        }
        assert_eq!(sent, QUEUE_BYTES / chunk);
        assert!(sent < QUEUE, "the bytes bound it before the count");
        assert_eq!(st.queued_bytes.load(Relaxed), QUEUE_BYTES);
        assert!(matches!(st.send(&tx, out()), Err(TrySendError::Full(_))));
        assert!(st.send(&tx, Msg::Event(Arc::new(Event::Activity { session: "a".into() }))).is_ok(), "no bytes, still room");
        st.full.store(true, Relaxed);
        for _ in 0..sent * 3 / 4 {
            st.took(&p.shared, &rx.recv().unwrap());
        }
        assert!(!st.full.load(Relaxed), "drained to a quarter of its bytes: open again");
        assert_eq!(st.queued_bytes.load(Relaxed), QUEUE_BYTES / 4);
        while let Ok(m) = rx.try_recv() {
            st.took(&p.shared, &m);
        }
        assert_eq!((st.queued.load(Relaxed), st.queued_bytes.load(Relaxed)), (0, 0));
        p.shutdown();
    }

    /// A drained queue tells the engine to queue again.
    #[test]
    fn a_drained_queue_reopens() {
        let p = Plugins::new();
        struct Q;
        impl Plugin for Q {
            fn manifest(&self) -> Manifest {
                Manifest::new("q").events(Events::OUTPUT)
            }
        }
        p.add(Q).unwrap();
        let st = p.shared.entries.lock().unwrap()[0].state.clone();
        st.full.store(true, Relaxed);
        p.shared.dirty.store(false, Relaxed);
        st.queued.store(DRAINED + 2, Relaxed);
        st.took(&p.shared, &Msg::Stop);
        assert!(st.full.load(Relaxed) && !p.shared.dirty.load(Relaxed), "not drained yet");
        st.took(&p.shared, &Msg::Stop);
        assert!(!st.full.load(Relaxed) && p.shared.dirty.load(Relaxed));
        st.queued.store(0, Relaxed);
        p.shutdown();
    }
}
