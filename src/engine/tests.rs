//! The event loop's scheduling, error handling and process control, against the fake backend (see
//! `testing`).

use super::options::{BURST_LOWAT, DRAIN_BATCH, SWEEP_EVERY_MS};
use super::table::SessKey;
use super::testing::{engine, readable, screen, session};
use super::{Change, Event, HOST_TAG, Interest, Kind};
use std::io::Write as _;
use std::os::fd::AsRawFd as _;
use std::os::unix::net::UnixStream;

#[test]
fn a_readable_session_is_read_at_the_coalescing_boundary() {
    let mut e = engine();
    let (key, mut prog) = session(&mut e, "a");
    prog.write_all(b"hello").unwrap();
    e.handle(&[readable(&e, key)], &mut Vec::new());
    let at = super::policy::coalesced_read_at(10_000, super::options::coalesce_ms());
    assert_eq!(e.due, [(at, key)]);
    assert!(e.slots.get(key.slot).unwrap().due);
    let out = e.slots.get(key.slot).unwrap().out_fd().unwrap();
    // Meanwhile only a filling pipe is reported.
    assert_eq!(e.changes, [Change::new(out, Interest::Read { lowat: BURST_LOWAT }, key.udata())]);
    assert_eq!(e.next_timer_ms(), (at - 10_000).min(SWEEP_EVERY_MS), "the read, or the sweep before it");
    e.changes.clear();
    // A second report while due does not schedule twice.
    e.tick();
    assert_eq!(screen(&e, key), "\n\n\n\n", "not yet");
    e.clock.advance(at - 10_000);
    e.tick();
    assert!(e.due.is_empty());
    assert_eq!(screen(&e, key), "hello\n\n\n\n");
    // The read took it out of `due`: its filter reports any output again.
    assert_eq!(e.changes, [Change::read(out, key.udata())]);
    assert!(!e.slots.get(key.slot).unwrap().due);
}

#[test]
fn a_full_pipe_while_due_is_read_at_once_and_rearmed() {
    let mut e = engine();
    let (key, mut prog) = session(&mut e, "a");
    prog.write_all(b"x").unwrap();
    e.handle(&[readable(&e, key)], &mut Vec::new());
    e.changes.clear();
    // The low-water mark was reached: the second report reads now, not at the boundary.
    prog.write_all(b"y").unwrap();
    e.handle(&[readable(&e, key)], &mut Vec::new());
    assert_eq!(screen(&e, key), "xy\n\n\n\n");
    let out = e.slots.get(key.slot).unwrap().out_fd().unwrap();
    assert_eq!(e.changes, [Change::read(out, key.udata())], "lowat back to 1 after the read");
    // The scheduled read finds the session not due any more and leaves its filter alone.
    e.changes.clear();
    e.clock.advance(10_000);
    e.tick();
    assert!(e.due.is_empty() && e.changes.is_empty());
}

#[test]
fn events_for_an_earlier_generation_are_dropped() {
    let mut e = engine();
    let (key, mut prog) = session(&mut e, "a");
    prog.write_all(b"x").unwrap();
    let stale = SessKey { slot: key.slot, generation: key.generation + 1 };
    e.handle(&[Event::new(Kind::Readable, 0, stale.udata()), Event::new(Kind::Writable, 0, stale.udata())], &mut Vec::new());
    assert!(e.due.is_empty() && e.changes.is_empty(), "nothing scheduled for the slot's next session");
    assert!(!e.slots.get(key.slot).unwrap().due);
    // An empty slot, likewise.
    e.handle(&[Event::new(Kind::Readable, 0, SessKey { slot: 31, generation: 1 }.udata())], &mut Vec::new());
    assert!(e.due.is_empty());
    assert_eq!(e.stats.events, 3);
}

#[test]
fn host_events_are_handed_back() {
    let mut e = engine();
    let mut host = Vec::new();
    e.handle(&[Event::new(Kind::Readable, 5, HOST_TAG | 7)], &mut host);
    assert_eq!(host.len(), 1);
    assert_eq!(host[0].tag(), HOST_TAG | 7);
    // `watch` sets the bit itself.
    let (a, _b) = crate::sys::fd::socket_pair().unwrap();
    e.watch(std::os::fd::AsFd::as_fd(&a), Interest::WriteOnce, 9).unwrap();
    assert_eq!(e.poller.applied.borrow().last(), Some(&Change::new(a.as_raw_fd(), Interest::WriteOnce, HOST_TAG | 9)));
}

#[test]
fn removing_a_session_purges_its_pending_changes() {
    let mut e = engine();
    let (key, _prog) = session(&mut e, "a");
    let (other, _p2) = session(&mut e, "b");
    e.handle(&[readable(&e, key), readable(&e, other)], &mut Vec::new());
    assert_eq!(e.changes.len(), 2);
    e.remove(key.slot);
    let left: Vec<usize> = e.changes.iter().map(Change::tag).collect();
    assert_eq!(left, [other.udata()], "only the other session's change is left");
    assert!(e.names.find("a", &e.slots).is_none());
    // Its due read finds no session and does nothing.
    e.clock.advance(10_000);
    e.tick();
    assert!(e.due.is_empty());
}

#[test]
fn reads_go_a_batch_at_a_time() {
    let mut e = engine();
    let keys: Vec<(SessKey, UnixStream)> = (0..DRAIN_BATCH + 3).map(|i| session(&mut e, &i.to_string())).collect();
    for (k, p) in &keys {
        (&*p).write_all(b"z").unwrap();
        let ev = readable(&e, *k);
        e.handle(&[ev], &mut Vec::new());
    }
    e.clock.advance(10_000);
    e.tick();
    assert_eq!(e.due.len(), 3, "the rest wait for the next pass");
    assert_eq!(e.next_timer_ms(), 0, "and are due at once");
    e.tick();
    assert!(e.due.is_empty());
    // A host waiting for the engine stops the pass after one read.
    for (k, p) in &keys[..3] {
        (&*p).write_all(b"z").unwrap();
        let ev = readable(&e, *k);
        e.handle(&[ev], &mut Vec::new());
    }
    e.clock.advance(10_000);
    e.tick_until(&|| true);
    assert_eq!(e.due.len(), 2);
}

#[test]
fn quiet_screens_freeze_then_deepen() {
    let mut e = engine();
    let (key, mut prog) = session(&mut e, "a");
    prog.write_all(b"text").unwrap();
    e.drain_now(key.slot);
    assert!(!e.slots.get(key.slot).unwrap().frozen());
    e.clock.advance(SWEEP_EVERY_MS - 1);
    e.tick();
    assert!(!e.slots.get(key.slot).unwrap().frozen(), "no sweep yet");
    e.clock.advance(super::options::freeze_after_ms().max(SWEEP_EVERY_MS));
    e.tick();
    assert!(e.slots.get(key.slot).unwrap().frozen(), "frozen once quiet for its hold");
    // Output thaws it; a quick return grows its hold, so the next sweep keeps it live.
    prog.write_all(b"!").unwrap();
    e.drain_now(key.slot);
    assert!(!e.slots.get(key.slot).unwrap().frozen());
    assert!(u64::from(e.slots.get(key.slot).unwrap().hold) > super::options::freeze_after_ms(), "hold grew");
    e.clock.advance(SWEEP_EVERY_MS);
    e.tick();
    assert!(!e.slots.get(key.slot).unwrap().frozen(), "kept live within its longer hold");
    e.clock.advance(super::options::HOLD_CAP_MS);
    e.tick();
    assert!(e.slots.get(key.slot).unwrap().frozen());
    assert_eq!(screen(&e, key), "text!\n\n\n\n");
    e.clock.advance(super::options::DEEPEN_AFTER_MS);
    e.tick();
    let deep = matches!(&e.slots.get(key.slot).unwrap().term, super::Screenful::Frozen(f) if f.is_deep());
    assert!(deep, "compressed once quiet for long");
    assert_eq!(screen(&e, key), "text!\n\n\n\n");
}

#[test]
fn a_session_whose_program_is_gone_is_not_kept() {
    let mut e = engine();
    let (key, _prog) = session(&mut e, "a");
    let pid = e.slots.get(key.slot).unwrap().pid;
    e.poller.gone.borrow_mut().push(pid);
    assert!(!e.watch_session(key.slot));
    assert_eq!(*e.programs.reaped.borrow(), [pid], "reaped at once: no exit event will come");
    let (key, _prog) = session(&mut e, "b");
    assert!(e.watch_session(key.slot));
    let s = e.slots.get(key.slot).unwrap();
    let applied = e.poller.applied.borrow();
    assert_eq!(applied[applied.len() - 2..], [Change::read(s.out_fd().unwrap(), key.udata()), Change::new(s.pid, Interest::Exit, key.udata())]);
}

#[test]
fn exits_end_their_session() {
    let mut e = engine();
    let (key, _prog) = session(&mut e, "a");
    let pid = e.slots.get(key.slot).unwrap().pid;
    let size_file = e.size_file_of(key.slot);
    e.handle(&[Event::new(Kind::Exited, pid, key.udata())], &mut Vec::new());
    assert!(e.slots.get(key.slot).is_none());
    assert_eq!(e.free.last(), Some(&key.slot), "its slot is free again");
    assert_eq!(*e.programs.reaped.borrow(), [pid]);
    assert_eq!(*e.programs.removed.borrow(), [size_file], "its size file goes with it");
    // An exit for a session already forgotten (kill-session) is still reaped.
    e.handle(&[Event::new(Kind::Exited, 77, key.udata())], &mut Vec::new());
    assert_eq!(*e.programs.reaped.borrow(), [pid, 77]);
}

#[test]
fn a_refused_change_is_dropped_and_the_wait_made_again() {
    let mut e = engine();
    let (a, _pa) = session(&mut e, "a");
    let (b, _pb) = session(&mut e, "b");
    e.handle(&[readable(&e, a), readable(&e, b)], &mut Vec::new());
    // a's fd was closed meanwhile (with no room to report it, kevent fails the whole wait).
    let a_out = e.slots.get(a.slot).unwrap().out_fd().unwrap();
    e.poller.closed.borrow_mut().push(a_out);
    e.poller.queued.borrow_mut().push(Event::new(Kind::Readable, 3, HOST_TAG | 4));
    let mut host = Vec::new();
    e.try_poll(Some(5), &mut host).unwrap();
    assert_eq!(host.len(), 1, "the retried wait's events were handled");
    assert_eq!(e.poller.waits(), 2);
    let b_out = e.slots.get(b.slot).unwrap().out_fd().unwrap();
    let applied = e.poller.applied.borrow();
    assert!(applied.iter().any(|c| c.ident() == b_out), "the change after the refused one applied");
    assert!(!applied.iter().any(|c| c.ident() == a_out));
    assert!(!e.has_pending_changes());
}

#[test]
fn a_failed_queue_is_an_error_not_a_quiet_turn() {
    let mut e = engine();
    e.poller.broken.set(true);
    let err = e.try_poll(None, &mut Vec::new()).unwrap_err();
    assert_eq!(err.raw_os_error(), Some(libc::EBADF));
    assert_eq!(e.stats.loops, 0, "nothing handled");
}

#[test]
fn a_new_session_drops_stale_changes_for_its_fds() {
    let mut e = engine();
    let (key, _p) = session(&mut e, "a");
    let out = e.slots.get(key.slot).unwrap().out_fd().unwrap();
    // Left by an earlier session whose output had this fd number, closed since.
    e.changes.push(Change::new(out, Interest::Read { lowat: BURST_LOWAT }, SessKey { slot: 9, generation: 9 }.udata()));
    assert!(e.watch_session(key.slot));
    assert!(e.changes.is_empty(), "it would have registered the new fd under the old tag");
}
