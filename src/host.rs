//! One turn of a host's event loop, the same for every host: the `zeropty serve` process, an engine on
//! a thread of its own (crate::embed::Embedded) and one run by its host's loop (crate::embed::Polled).
//! Each used to spell the order out itself; it lives here once:
//!
//! 1. the events: the engine's handled by the engine, the service's (the socket server's) by it, the
//!    rest handed to the host (`own`, e.g. its wake-up event and the jobs it signals);
//! 2. `tick`: the reads that are due (stopping early as `Yield` says), the sweep;
//! 3. `Service::after_tick`: output read for attached terminals, wait-panes that are over; then the
//!    plugins' (crate::plugin::Plugins, also a `Service`): the session events the engine queued handed
//!    to them, what they asked of the engine run;
//! 4. when the host waits on the queue without the engine (`Waiting::Apart`), the filter changes made
//!    during the turn, applied now, before the fd they name can be closed and reused.
//!
//! A command a host runs between turns (`after_command`) is followed by steps 3 and 4. A wait on the
//! queue that fails (the queue itself: a refused change is no failure, see Engine::try_poll) is a turn
//! with no events: steps 2 to 4 still run, and `turn` returns the error for the host to report.
//!
//! What the loop drives besides the engine is a `Service` (crate::server::Server for every host here)
//! and, once set, the plugins' registry, a `Service` too; the engine is any `Engine<B>`, so the order
//! above is unit-tested with fakes of both. With no plugins set, or none registered, they cost a branch
//! (and three atomic loads) a turn.
//!
//! # Example
//!
//! The loop of the `zeropty serve` process, cut down to a few turns: an engine, a socket server, and
//! [`HostLoop::turn`] doing the four steps.
//!
//! ```
//! use zeropty::engine::{Engine, Options};
//! use zeropty::host::{HostLoop, Source, Waiting, Yield};
//! use zeropty::server::{Hooks, Server};
//!
//! let path = std::env::temp_dir().join(format!("zeropty-doc-host-{}.sock", std::process::id()));
//! let mut e = Engine::new(Options::default());
//! let server = Server::bind(&e, &path, Hooks::default())?;
//! let mut host = HostLoop::new(Some(server), Waiting::WithEngine);
//! for _ in 0..3 {
//!     host.turn(&mut e, Source::Wait(Some(std::time::Duration::from_millis(10))), Yield::Never, |_, _, own| {
//!         assert!(own.is_empty()); // no fds of the host's own here
//!     })?;
//! }
//! # Ok::<(), std::io::Error>(())
//! ```

use crate::engine::{Backend, Engine, Event};
use crate::plugin::Plugins;
use crate::server::Server;
use std::time::{Duration, Instant};

/// How the host waits on the engine's queue.
///
/// ```
/// use zeropty::host::{HostLoop, Waiting};
///
/// // The zeropty server waits through the engine; embed::Polled's host waits on the fd itself.
/// let serve: HostLoop = HostLoop::new(None, Waiting::WithEngine);
/// let polled: HostLoop = HostLoop::new(None, Waiting::Apart);
/// # drop((serve, polled));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waiting {
    /// Through `Engine::try_poll`, holding the engine: pending changes go with that wait.
    WithEngine,
    /// On the queue's fd by itself (another thread may hold the engine meanwhile, or the host's own
    /// loop watches the fd): pending changes are applied at the end of every turn.
    Apart,
}

/// Where a turn's events come from.
///
/// ```
/// use zeropty::engine::{Engine, Event, Options};
/// use zeropty::host::{HostLoop, Source, Waiting, Yield};
///
/// let mut e = Engine::new(Options::default());
/// let mut host: HostLoop = HostLoop::new(None, Waiting::Apart);
/// // Events the host already waited for (none here), instead of a wait inside the turn.
/// let events: [Event; 0] = [];
/// host.turn(&mut e, Source::Events(&events), Yield::Never, |_, _, _| {})?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub enum Source<'a> {
    /// Wait on the engine for at most this long (and no longer than its next timer, nor the service's);
    /// `None`: as long as the engine and the service allow.
    Wait(Option<Duration>),
    /// Events the host already waited for itself.
    Events(&'a [Event]),
}

/// When the reads of a turn stop early, the rest due at once for the next turn.
///
/// ```
/// use std::time::Duration;
/// use zeropty::engine::{Engine, Options};
/// use zeropty::host::{HostLoop, Source, Waiting, Yield};
///
/// let mut e = Engine::new(Options::default());
/// let mut host: HostLoop = HostLoop::new(None, Waiting::Apart);
/// // A slice of the host's own loop: reads stop after 2 ms, the rest is due at once next turn.
/// host.turn(&mut e, Source::Wait(Some(Duration::ZERO)), Yield::After(Duration::from_millis(2)), |_, _, _| {})?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub enum Yield<'a> {
    /// Read everything due (up to the engine's batch).
    Never,
    /// Once this long went to reading (a host loop's slice).
    After(Duration),
    /// Once this says so (another thread waits for the engine).
    When(&'a dyn Fn() -> bool),
}

/// What a host loop serves besides the engine, on the engine's queue: its events are among the host's
/// (HOST_TAG), told apart by `owns`.
///
/// A service that serves nothing (the loop with no socket); [`crate::server::Server`] is the real one.
///
/// ```
/// use zeropty::engine::{Backend, Engine, Event, Options};
/// use zeropty::host::{HostLoop, Service, Source, Waiting, Yield};
///
/// struct Nothing;
/// impl<B: Backend> Service<B> for Nothing {
///     fn owns(&self, _: &Event) -> bool { false }
///     fn event(&mut self, _: &mut Engine<B>, _: &Event) {}
///     fn after_tick(&mut self, _: &mut Engine<B>) {}
///     fn wait_timeout(&self, _: &Engine<B>) -> Option<u64> { None }
/// }
/// let mut e = Engine::new(Options::default());
/// let mut host = HostLoop::with_service(Some(Nothing), Waiting::WithEngine);
/// host.turn(&mut e, Source::Wait(Some(std::time::Duration::ZERO)), Yield::Never, |_, _, _| {})?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub trait Service<B: Backend> {
    /// Whether an event the engine handed back is this service's.
    fn owns(&self, ev: &Event) -> bool;
    /// Handles one of its events.
    fn event(&mut self, e: &mut Engine<B>, ev: &Event);
    /// After the engine's tick (or a command): passes on what the engine read.
    fn after_tick(&mut self, e: &mut Engine<B>);
    /// How long the loop may sleep before the service needs a turn of its own; None: no limit.
    fn wait_timeout(&self, e: &Engine<B>) -> Option<u64>;
}

impl<B: Backend> Service<B> for Server {
    #[inline]
    fn owns(&self, ev: &Event) -> bool {
        Server::owns(ev)
    }
    #[inline]
    fn event(&mut self, e: &mut Engine<B>, ev: &Event) {
        Server::event(self, e, ev)
    }
    #[inline]
    fn after_tick(&mut self, e: &mut Engine<B>) {
        Server::after_tick(self, e)
    }
    #[inline]
    fn wait_timeout(&self, e: &Engine<B>) -> Option<u64> {
        Server::wait_timeout(self, e)
    }
}

/// The plugins' registry as a service: its events are its wake-ups (nothing to do but turn),
/// `after_tick` hands the plugins the engine's events and runs what they asked, and `wait_timeout` is
/// the watchdog's cap. Statically dispatched like the server's.
impl<B: Backend> Service<B> for Plugins {
    #[inline]
    fn owns(&self, ev: &Event) -> bool {
        Plugins::owns(ev)
    }
    #[inline]
    fn event(&mut self, _: &mut Engine<B>, _: &Event) {}
    #[inline]
    fn after_tick(&mut self, e: &mut Engine<B>) {
        self.pump(e)
    }
    #[inline]
    fn wait_timeout(&self, _: &Engine<B>) -> Option<u64> {
        self.watchdog_ms()
    }
}

/// A host's loop state: its service (the socket server), if it listens, its plugins, if it has a
/// registry, and the events it handed out.
///
/// The module's docs have a whole loop; [`crate::embed::Polled`] is one built on it.
///
/// ```
/// use zeropty::host::{HostLoop, Waiting};
///
/// let host: HostLoop = HostLoop::new(None, Waiting::Apart);
/// assert!(host.server().is_none()); // not listening
/// ```
pub struct HostLoop<S = Server> {
    server: Option<S>,
    plugins: Option<Plugins>,
    waiting: Waiting,
    /// The host's events of this turn, split from the engine's.
    host: Vec<Event>,
    /// Those that were not the service's, for `own`.
    foreign: Vec<Event>,
}

impl HostLoop {
    /// A loop waiting as `waiting` says, with `server` in front of the engine if it listens.
    ///
    /// ```
    /// use zeropty::{engine::{Engine, Options}, host::{HostLoop, Waiting}, server::{Hooks, Server}};
    ///
    /// let path = std::env::temp_dir().join(format!("zeropty-doc-host-new-{}.sock", std::process::id()));
    /// let e = Engine::new(Options::default());
    /// let host = HostLoop::new(Some(Server::bind(&e, &path, Hooks::default())?), Waiting::WithEngine);
    /// assert!(host.server().is_some());
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn new(server: Option<Server>, waiting: Waiting) -> Self {
        Self::with_service(server, waiting)
    }
}

impl<S> HostLoop<S> {
    /// A loop waiting as `waiting` says, serving `service` if there is one.
    ///
    /// ```
    /// use zeropty::host::{HostLoop, Waiting};
    ///
    /// let host: HostLoop<()> = HostLoop::with_service(None, Waiting::Apart); // any service type, here none
    /// assert!(host.server().is_none());
    /// ```
    pub fn with_service(service: Option<S>, waiting: Waiting) -> Self {
        Self { server: service, plugins: None, waiting, host: Vec::new(), foreign: Vec::new() }
    }

    /// Runs `plugins` with the loop from now on (crate::plugin): they hear the engine's events and their
    /// requests run after every turn and command. The host also lets them wake its loop
    /// (`Plugins::attach`) and routes their commands (the server's `set_plugins`, `Plugins::call`).
    pub fn set_plugins(&mut self, plugins: Plugins) {
        self.plugins = Some(plugins)
    }

    /// The plugins' registry, once set.
    pub fn plugins(&self) -> Option<&Plugins> {
        self.plugins.as_ref()
    }

    /// The socket server (the service), once listening.
    ///
    /// ```
    /// let host: zeropty::host::HostLoop = zeropty::host::HostLoop::new(None, zeropty::host::Waiting::Apart);
    /// assert!(host.server().is_none());
    /// ```
    pub fn server(&self) -> Option<&S> {
        self.server.as_ref()
    }

    /// The socket server (the service), once listening, to call directly.
    ///
    /// ```
    /// let mut host: zeropty::host::HostLoop = zeropty::host::HostLoop::new(None, zeropty::host::Waiting::Apart);
    /// assert!(host.server_mut().is_none());
    /// ```
    pub fn server_mut(&mut self) -> Option<&mut S> {
        self.server.as_mut()
    }

    /// Starts answering through `server` (replacing one before).
    ///
    /// ```
    /// use zeropty::{engine::{Engine, Options}, host::{HostLoop, Waiting}, server::{Hooks, Server}};
    ///
    /// let path = std::env::temp_dir().join(format!("zeropty-doc-set-server-{}.sock", std::process::id()));
    /// let e = Engine::new(Options::default());
    /// let mut host: HostLoop = HostLoop::new(None, Waiting::Apart);
    /// host.set_server(Server::bind(&e, &path, Hooks::default())?); // what Polled::listen does
    /// assert!(host.server().is_some());
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn set_server(&mut self, server: S) {
        self.server = Some(server)
    }

    /// How long the host may sleep before the engine or the service needs a turn.
    ///
    /// ```
    /// use zeropty::{engine::{Engine, Options}, host::{HostLoop, Waiting}};
    ///
    /// let e = Engine::new(Options::default());
    /// let host: HostLoop = HostLoop::new(None, Waiting::Apart);
    /// assert_eq!(host.timeout_ms(&e), e.next_timer_ms().min(host.timeout_ms(&e)));
    /// assert!(host.timeout_ms(&e) <= 1000);
    /// ```
    pub fn timeout_ms<B: Backend>(&self, e: &Engine<B>) -> u64
    where
        S: Service<B>,
    {
        let t = e.next_timer_ms();
        self.server_timeout_ms(e).map_or(t, |w| w.min(t))
    }

    /// How long the services (the server, the plugins' watchdog) let the loop sleep.
    fn server_timeout_ms<B: Backend>(&self, e: &Engine<B>) -> Option<u64>
    where
        S: Service<B>,
    {
        let server = self.server.as_ref().and_then(|s| s.wait_timeout(e));
        match self.plugins.as_ref().and_then(|p| Service::<B>::wait_timeout(p, e)) {
            Some(p) => Some(server.map_or(p, |s| s.min(p))),
            None => server,
        }
    }

    /// One turn (see the module's docs). `own` gets the host's events that were not the service's, and
    /// the service slot (a job may start listening), between the events and the reads. Fails only when
    /// the wait on the queue failed; the rest of the turn ran all the same.
    ///
    /// ```
    /// use std::time::Duration;
    /// use zeropty::engine::{Engine, Options};
    /// use zeropty::host::{HostLoop, Source, Waiting, Yield};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// let mut host: HostLoop = HostLoop::new(None, Waiting::WithEngine);
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-turn", "echo turned; exec cat"]), "/");
    /// let screen = |e: &mut Engine| e.command(&argv(&["capture-pane", "-p", "-t", "=doc-turn:"]), "/").out;
    /// let deadline = std::time::Instant::now() + Duration::from_secs(5);
    /// while !screen(&mut e).starts_with("turned") && std::time::Instant::now() < deadline {
    ///     host.turn(&mut e, Source::Wait(Some(Duration::from_millis(50))), Yield::Never, |_, _, _| {})?;
    /// }
    /// assert!(screen(&mut e).starts_with("turned\n"));
    /// e.command(&argv(&["kill-server"]), "/");
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn turn<B: Backend>(
        &mut self,
        e: &mut Engine<B>,
        source: Source<'_>,
        until: Yield<'_>,
        own: impl FnOnce(&mut Engine<B>, &mut Option<S>, &[Event]),
    ) -> std::io::Result<()>
    where
        S: Service<B>,
    {
        let waited = match source {
            Source::Wait(max) => {
                let max = max.map(|m| m.as_millis() as u64);
                let max = match (max, self.server_timeout_ms(e)) {
                    (Some(m), Some(w)) => Some(m.min(w)),
                    (m, w) => m.or(w),
                };
                e.try_poll(max, &mut self.host)
            }
            Source::Events(events) => {
                e.handle(events, &mut self.host);
                Ok(())
            }
        };
        for ev in self.host.drain(..) {
            match self.server.as_mut() {
                Some(s) if s.owns(&ev) => s.event(e, &ev),
                // The plugins' wake-up: what it is for runs in after_command.
                _ if self.plugins.is_some() && Plugins::owns(&ev) => {}
                _ => self.foreign.push(ev),
            }
        }
        own(e, &mut self.server, &self.foreign);
        self.foreign.clear();
        match until {
            Yield::Never => e.tick(),
            Yield::After(d) => {
                let start = Instant::now();
                e.tick_until(&|| start.elapsed() >= d);
            }
            Yield::When(f) => e.tick_until(f),
        }
        self.after_command(e);
        waited
    }

    /// After the engine ran a command outside a turn: output it read for attached terminals goes out, the
    /// plugins get the events it made and what they asked is run, and (`Waiting::Apart`) the filters
    /// changed are in the queue before the host waits on it.
    ///
    /// ```
    /// use zeropty::{engine::{Engine, Options}, host::{HostLoop, Waiting}};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// let mut host: HostLoop = HostLoop::new(None, Waiting::Apart);
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-after", "exec cat"]), "/");
    /// host.after_command(&mut e); // Waiting::Apart: the new session's filters are in the queue now
    /// assert!(!e.has_pending_changes());
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn after_command<B: Backend>(&mut self, e: &mut Engine<B>)
    where
        S: Service<B>,
    {
        if let Some(s) = self.server.as_mut() {
            s.after_tick(e);
        }
        if let Some(p) = self.plugins.as_mut() {
            p.after_tick(e);
        }
        if self.waiting == Waiting::Apart {
            e.apply_changes();
        }
    }
}

#[cfg(test)]
mod tests {
    //! The turn's order and timeouts, with the engine on the fake backend and a service that records
    //! what it is called for.

    use super::{HostLoop, Service, Source, Waiting, Yield};
    use crate::engine::testing::{Fake, Test, advance, coalesce_ms, engine, readable, screen, session};
    use crate::engine::{Change, Engine, Event, HOST_TAG, Kind};
    use std::cell::RefCell;
    use std::io::Write as _;
    use std::rc::Rc;
    use std::time::Duration;

    const MINE: usize = HOST_TAG | 2;
    const THEIRS: usize = HOST_TAG | 1;

    /// Records its calls in `log`, with what the engine showed at the time (`probe`).
    struct Recorder {
        log: Rc<RefCell<Vec<String>>>,
        probe: Box<dyn Fn(&Test) -> String>,
        timeout: Option<u64>,
    }

    impl Service<Fake> for Recorder {
        fn owns(&self, ev: &Event) -> bool {
            ev.tag() == MINE
        }
        fn event(&mut self, e: &mut Engine<Fake>, ev: &Event) {
            self.log.borrow_mut().push(format!("event {} {}", ev.ident(), (self.probe)(e)))
        }
        fn after_tick(&mut self, e: &mut Engine<Fake>) {
            self.log.borrow_mut().push(format!("after_tick {}", (self.probe)(e)))
        }
        fn wait_timeout(&self, _: &Engine<Fake>) -> Option<u64> {
            self.timeout
        }
    }

    fn recorder(probe: impl Fn(&Test) -> String + 'static) -> (Recorder, Rc<RefCell<Vec<String>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        (Recorder { log: log.clone(), probe: Box::new(probe), timeout: None }, log)
    }

    #[test]
    fn a_turn_goes_events_own_tick_after_tick_then_changes() {
        for waiting in [Waiting::Apart, Waiting::WithEngine] {
            let mut e = engine();
            let (key, mut prog) = session(&mut e, "a");
            prog.write_all(b"hi").unwrap();
            let (svc, log) = recorder(move |e| screen(e, key).trim_end().to_owned());
            let mut host = HostLoop::with_service(Some(svc), waiting);
            let events = [Event::new(Kind::Readable, 5, MINE), readable(&e, key), Event::new(Kind::Readable, 6, THEIRS)];
            let own_log = log.clone();
            host.turn(&mut e, Source::Events(&events), Yield::Never, |e, svc, own| {
                assert!(svc.is_some());
                let tags: Vec<usize> = own.iter().map(Event::tag).collect();
                assert_eq!(tags, [THEIRS], "only the events neither the engine's nor the service's");
                own_log.borrow_mut().push(format!("own {}", screen(e, key).trim_end()));
                // The read is due by the tick that follows (its boundary is at most two intervals on).
                advance(e, 2 * coalesce_ms());
            })
            .unwrap();
            assert_eq!(*log.borrow(), ["event 5 ", "own ", "after_tick hi"], "the read comes between own and after_tick");
            // The read re-armed the session's filter: applied at the end of the turn when the host waits
            // apart, left for the next wait otherwise.
            let rearm = e.poller().applied.borrow().iter().any(|c| {
                let ev = readable(&e, key);
                *c == Change::read(ev.ident(), ev.tag())
            });
            match waiting {
                Waiting::Apart => assert!(rearm && !e.has_pending_changes()),
                Waiting::WithEngine => assert!(!rearm && e.has_pending_changes()),
            }
        }
    }

    #[test]
    fn a_wait_lasts_no_longer_than_the_engine_or_the_service_allow() {
        let mut e = engine();
        let timer = e.next_timer_ms();
        let (mut svc, _log) = recorder(|_| String::new());
        svc.timeout = Some(7);
        let mut host = HostLoop::with_service(Some(svc), Waiting::WithEngine);
        assert_eq!(host.timeout_ms(&e), 7);
        let mut turn = |host: &mut HostLoop<Recorder>, max| host.turn(&mut e, Source::Wait(max), Yield::Never, |_, _, _| {}).unwrap();
        turn(&mut host, Some(Duration::from_millis(50)));
        turn(&mut host, None);
        turn(&mut host, Some(Duration::from_millis(3)));
        host.server_mut().unwrap().timeout = None;
        turn(&mut host, Some(Duration::from_millis(50)));
        turn(&mut host, None);
        assert_eq!(*e.poller().timeouts.borrow(), [7, 7, 3, 50.min(timer), timer]);
        assert_eq!(host.timeout_ms(&e), timer);
    }

    #[test]
    fn reads_yield_after_their_slice() {
        let mut e = engine();
        let mut progs = Vec::new();
        let mut events = Vec::new();
        for name in ["a", "b", "c"] {
            let (key, mut prog) = session(&mut e, name);
            prog.write_all(b"x").unwrap();
            events.push(readable(&e, key));
            progs.push(prog);
        }
        let mut host: HostLoop<Recorder> = HostLoop::with_service(None, Waiting::Apart);
        host.turn(&mut e, Source::Events(&events), Yield::Never, |e, _, _| advance(e, 2 * coalesce_ms())).unwrap();
        assert!(e.next_timer_ms() > 0, "every read done");
        for prog in &mut progs {
            prog.write_all(b"y").unwrap();
        }
        // A slice of nothing: one read, then the rest are left due at once for the next turn.
        host.turn(&mut e, Source::Events(&events), Yield::After(Duration::ZERO), |e, _, _| advance(e, 2 * coalesce_ms())).unwrap();
        assert_eq!(e.next_timer_ms(), 0, "reads left for the next turn");
        host.turn(&mut e, Source::Events(&[]), Yield::Never, |_, _, _| {}).unwrap();
        assert!(e.next_timer_ms() > 0);
    }

    /// The plugins ride the loop as a service: their wake-ups are neither the service's nor the host's,
    /// they hear what the engine did in the turn (after the service's after_tick), and with plugins
    /// running the loop sleeps no longer than their watchdog; with none, the plugins change nothing.
    #[test]
    fn plugins_are_pumped_after_the_service_and_cap_the_wait() {
        use crate::plugin::{Event as PEvent, Events, Host, Manifest, PLUGIN_TAG, Plugin, Plugins};
        struct Ear(std::sync::mpsc::Sender<PEvent>);
        impl Plugin for Ear {
            fn manifest(&self) -> Manifest {
                Manifest::new("ear").events(Events::SESSIONS)
            }
            fn event(&mut self, ev: &PEvent, _: &Host) {
                let _ = self.0.send(ev.clone());
            }
        }
        let mut e = engine();
        let (svc, _log) = recorder(|_| String::new());
        let mut host = HostLoop::with_service(Some(svc), Waiting::WithEngine);
        let plugins = Plugins::new();
        host.set_plugins(plugins.clone());
        let timer = e.next_timer_ms();
        assert_eq!(host.timeout_ms(&e), timer, "no plugin running: no cap");
        let (tx, rx) = std::sync::mpsc::channel();
        plugins.add(Ear(tx)).unwrap();
        plugins.set_timeout(Duration::from_millis(400));
        assert_eq!(host.timeout_ms(&e), 100.min(timer), "a quarter of the deadline");
        let wake = [Event::new(Kind::Other, 0, PLUGIN_TAG)];
        // The first turn tells the engine what the plugin wants; what happens after reaches it.
        host.turn(&mut e, Source::Events(&wake), Yield::Never, |_, _, own| assert!(own.is_empty(), "a plugin wake-up is not the host's")).unwrap();
        let _s = session(&mut e, "a");
        host.turn(&mut e, Source::Events(&[]), Yield::Never, |_, _, _| {}).unwrap();
        let ev = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(ev, PEvent::SessionCreated { session: "a".into() });
        plugins.shutdown();
        assert_eq!(host.timeout_ms(&e), e.next_timer_ms(), "shut down: no cap");
    }

    #[test]
    fn a_failed_wait_is_a_turn_without_events_and_its_error() {
        let mut e = engine();
        let (key, mut prog) = session(&mut e, "a");
        prog.write_all(b"z").unwrap();
        e.handle(&[readable(&e, key)], &mut Vec::new());
        advance(&e, 2 * coalesce_ms());
        e.poller().broken.set(true);
        let mut host: HostLoop<Recorder> = HostLoop::with_service(None, Waiting::WithEngine);
        let mut ran = false;
        let r = host.turn(&mut e, Source::Wait(None), Yield::Never, |_, _, own| ran = own.is_empty());
        assert_eq!(r.unwrap_err().raw_os_error(), Some(libc::EBADF));
        assert!(ran, "the host's part ran");
        assert_eq!(screen(&e, key).trim_end(), "z", "and the reads that were due");
    }
}
