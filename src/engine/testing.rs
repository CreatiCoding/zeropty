//! An engine on the fake backend (backend::fake), and sessions whose program's output is a socket
//! pair: no process, no kqueue, no waiting. For the engine's tests (`tests` and each child module's)
//! and the hosts' (crate::host).

pub(crate) use super::backend::fake::{Fake, FakeClock, FakePoller, FakePrograms};
use super::session::{Sess, Started};
use super::table::SessKey;
use super::{Clock as _, Engine, Event, Kind, Options};
use crate::term::Term;
use std::os::unix::net::UnixStream;

pub(crate) type Test = Engine<Fake>;

/// An engine whose clock reads 10 s.
pub(crate) fn engine() -> Test {
    let clock = FakeClock::default();
    clock.advance(10_000);
    Engine::with_backend(Options { size_dir: std::env::temp_dir(), ..Options::default() }, FakePoller::default(), clock, FakePrograms::default())
}

/// A session named `name` whose program's output is the returned socket; its pid is made up.
pub(crate) fn session(e: &mut Test, name: &str) -> (SessKey, UnixStream) {
    let (out, theirs) = crate::sys::fd::socket_pair().unwrap();
    out.set_nonblocking(true).unwrap();
    let generation = e.next_generation();
    let now = e.clock.now_ms();
    let s = Sess::new(
        Started {
            generation,
            name: name.into(),
            pid: e.programs.pid(),
            stdin: None,
            out: Some(out.into()),
            size: (4, 20),
            base: (4, 20),
            created: 0,
            term: Term::new(4, 20, 10),
        },
        now,
    );
    let slot = e.insert(s);
    (SessKey { slot, generation }, theirs)
}

/// `session`, with its program's input too: the second socket returned reads what the session is sent.
pub(crate) fn session_with_input(e: &mut Test, name: &str) -> (SessKey, UnixStream, UnixStream) {
    let (key, out) = session(e, name);
    let (ours, theirs) = crate::sys::fd::socket_pair().unwrap();
    ours.set_nonblocking(true).unwrap();
    theirs.set_nonblocking(true).unwrap();
    e.slots.get_mut(key.slot).unwrap().stdin = Some(ours.into());
    (key, out, theirs)
}

/// Closes session `key`'s input for writing, as a program that went away does to a write: EPIPE. Done
/// on the engine's own end, not by dropping the program's: a process another test is starting holds a
/// copy of that end until it execs (crate::sys::fd::no_spawns), and writes then still succeed.
pub(crate) fn close_input(e: &Test, key: SessKey) {
    let fd = e.slots.get(key.slot).unwrap().stdin.as_ref().unwrap().try_clone().unwrap();
    // NotConnected: the other end is closed for good already, and the write fails as it should.
    let _ = UnixStream::from(fd).shutdown(std::net::Shutdown::Write);
}

/// The event for session `key`'s output turning readable.
pub(crate) fn readable(e: &Test, key: SessKey) -> Event {
    Event::new(Kind::Readable, e.slots.get(key.slot).unwrap().out_fd().unwrap(), key.udata())
}

/// Session `key`'s screen as capture-pane prints it.
pub(crate) fn screen(e: &Test, key: SessKey) -> String {
    e.slots.get(key.slot).unwrap().term.read(|t| super::render::capture(t.view(), &Default::default()).unwrap())
}

/// Moves the engine's clock `ms` on.
pub(crate) fn advance(e: &Test, ms: u64) {
    e.clock.advance(ms)
}

/// The fake programs the engine runs.
pub(crate) fn programs(e: &Test) -> &FakePrograms {
    &e.programs
}

/// How long a quiet session's read is put off (Readable to its read).
pub(crate) fn coalesce_ms() -> u64 {
    super::options::coalesce_ms()
}
