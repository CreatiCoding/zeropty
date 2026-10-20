//! What happens to sessions, for whoever above the engine listens (crate::plugin): sessions made, ended
//! and renamed, their output and activity. The engine queues an event only for kinds a listener asked
//! for (`Engine::set_tap`): with none, every path pays one branch on a mask and builds nothing, and no
//! output byte is copied. The queue is bounded: past it, events are dropped and counted, so a host that
//! takes them rarely never makes the engine buffer without end.
//!
//! The event types are plain data defined here, at the engine's layer, so the engine depends on nothing
//! above it; crate::plugin re-exports them as its stable `Event` and `ExitReason`, and adds their wire form.

use super::session::Stats;
use super::{Backend, Engine};

/// Kinds of event, as bits: the engine's own masks stay plain integers (one test on the hot paths);
/// crate::plugin::Events is the typed set over the same bits.
pub(crate) mod kind {
    /// A session was made (new-session, or taken over from another server).
    pub const SESSION_CREATED: u32 = 1 << 0;
    /// A session ended: its program exited, it was killed, or it was handed to another server.
    pub const SESSION_EXITED: u32 = 1 << 1;
    /// A session got a new name.
    pub const SESSION_RENAMED: u32 = 1 << 2;
    /// What a session's program printed, as read.
    pub const OUTPUT: u32 = 1 << 3;
    /// A session's screen changed (one event per read, without the bytes).
    pub const ACTIVITY: u32 = 1 << 4;
    /// How many kinds there are.
    pub const COUNT: usize = 5;
    /// Every kind.
    pub const ALL: u32 = (1 << COUNT) - 1;
}

/// Why a session ended. Non-exhaustive: a later version may tell more reasons apart (match with a
/// `_` arm; `as_str` names any of them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExitReason {
    /// Its program exited.
    Exited,
    /// kill-session. (kill-server hangs every program up: they end as Exited.)
    Killed,
    /// Handed to another server (push-sessions, restart-server): its program goes on there.
    HandedOver,
}

impl ExitReason {
    /// Its name on the wire: "exited", "killed", "handed-over".
    pub fn as_str(self) -> &'static str {
        match self {
            ExitReason::Exited => "exited",
            ExitReason::Killed => "killed",
            ExitReason::HandedOver => "handed-over",
        }
    }
}

/// Something that happened to a session. Non-exhaustive: later versions may add kinds (match with a
/// `_` arm).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A session was made (new-session, or taken over from another server).
    SessionCreated {
        /// Its name.
        session: String,
    },
    /// A session ended.
    SessionExited {
        /// Its name.
        session: String,
        /// Why.
        reason: ExitReason,
    },
    /// A session got a new name.
    SessionRenamed {
        /// Its new name.
        session: String,
        /// Its name before.
        from: String,
    },
    /// What the program printed (bytes as read: a UTF-8 character may be split across two events).
    /// At most 64 KB each: a burst comes as several events, so no single event holds megabytes.
    Output {
        /// The session's name.
        session: String,
        /// The bytes.
        data: Vec<u8>,
    },
    /// A session's screen changed (one per read).
    Activity {
        /// The session's name.
        session: String,
    },
}

impl Event {
    /// Its bit (`kind`; the stable form is `Event::kind`, an `Events` of this one bit).
    pub(crate) fn bit(&self) -> u32 {
        match self {
            Event::SessionCreated { .. } => kind::SESSION_CREATED,
            Event::SessionExited { .. } => kind::SESSION_EXITED,
            Event::SessionRenamed { .. } => kind::SESSION_RENAMED,
            Event::Output { .. } => kind::OUTPUT,
            Event::Activity { .. } => kind::ACTIVITY,
        }
    }

    /// The session it is about (after a rename, the new name).
    pub fn session(&self) -> &str {
        match self {
            Event::SessionCreated { session }
            | Event::SessionExited { session, .. }
            | Event::SessionRenamed { session, .. }
            | Event::Output { session, .. }
            | Event::Activity { session } => session,
        }
    }
}

/// Events the engine holds for the host's next take, at most; more are dropped and counted (a host that
/// takes them rarely, such as a polled one whose `process` is seldom called, must not buffer without end).
const TAP_EVENTS: usize = 16 * 1024;
/// The same for the bytes of output events. Also the most one read copies for the tap (`copy`): a 200 MB
/// burst read at once (Sess::slurp) is copied up to this, the rest counted as one dropped event.
const TAP_BYTES: usize = 16 << 20;
/// The most bytes one Output event holds. A burst used to be one event as big as the read (a 200 MB
/// `yes` made one 268 MB allocation, held by a plugin's queue until the plugin failed); in pieces, the
/// queues' byte budgets (here and crate::plugin's per plugin) bound what is held, to the piece.
pub(crate) const CHUNK: usize = 64 * 1024;

/// Copies a read's bytes for the tap (Stats::tapped), in pieces of at most CHUNK and TAP_BYTES in all
/// until the next event; what does not fit is cut (Stats::tap_cut). Only when output is tapped.
#[cold]
#[inline(never)]
pub(super) fn copy(stats: &mut Stats, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        if stats.tapped.last().is_none_or(|last| last.len() >= CHUNK) {
            if stats.tapped.len() >= TAP_BYTES / CHUNK {
                stats.tap_cut = true;
                return;
            }
            stats.tapped.push(Vec::with_capacity(bytes.len().min(CHUNK)));
        }
        let last = stats.tapped.last_mut().expect("pushed above");
        let n = (CHUNK - last.len()).min(bytes.len());
        last.extend_from_slice(&bytes[..n]);
        bytes = &bytes[n..];
    }
}

/// The engine's side: the kinds wanted, and the events waiting for the host.
#[derive(Default)]
pub(crate) struct Tap {
    mask: u32,
    /// Kinds only counted as dropped, not built (every listener wanting them is full).
    muted: u32,
    /// mask | muted: the one load the engine's paths test.
    pub(super) any: u32,
    events: Vec<Event>,
    /// Output bytes in `events`.
    bytes: usize,
    /// Dropped since the last take, by kind (bit k at k).
    lost: [u64; kind::COUNT],
}

impl Tap {
    /// What the engine is to queue (`mask`) and only count (`muted`).
    fn set(&mut self, mask: u32, muted: u32) {
        let muted = muted & !mask;
        (self.mask, self.muted, self.any) = (mask, muted, mask | muted);
    }

    /// Queues the event `make` builds, if its kind is wanted (counts it when that kind is muted). One
    /// branch when none is.
    #[inline(always)]
    pub(super) fn emit(&mut self, kind: u32, make: impl FnOnce() -> Event) {
        if self.any & kind != 0 {
            self.push(kind, make);
        }
    }

    #[cold]
    #[inline(never)]
    fn push(&mut self, kind: u32, make: impl FnOnce() -> Event) {
        if self.mask & kind == 0 || self.events.len() >= TAP_EVENTS || self.bytes >= TAP_BYTES {
            self.lost[kind.trailing_zeros() as usize % kind::COUNT] += 1;
            return;
        }
        let ev = make();
        if let Event::Output { data, .. } = &ev {
            self.bytes += data.len();
        }
        self.events.push(ev);
    }

    fn has_events(&self) -> bool {
        !self.events.is_empty() || self.lost.iter().any(|&n| n != 0)
    }
}

impl<B: Backend> Engine<B> {
    /// The kinds of event to queue (`mask`) and those only to count as dropped (`muted`: no listener
    /// wanting them can take more now, so output is not even copied for them). 0 and 0, the default,
    /// queues none and frees the queue.
    pub(crate) fn set_tap(&mut self, mask: u32, muted: u32) {
        self.tap.set(mask, muted);
        self.stats.tap = mask & kind::OUTPUT != 0;
        if !self.stats.tap {
            (self.stats.tapped, self.stats.tap_cut) = (Vec::new(), false);
        }
        if mask | muted == 0 {
            self.tap = Tap::default();
        }
    }

    /// Whether events (or counts of dropped ones) wait for `take_tap`. One branch when nothing is wanted.
    #[inline]
    pub(crate) fn has_tap_events(&self) -> bool {
        self.tap.any != 0 && self.tap.has_events()
    }

    /// The events queued since the last call, and how many were dropped by kind (index k: bit 1 << k).
    pub(crate) fn take_tap(&mut self) -> (Vec<Event>, [u64; kind::COUNT]) {
        self.tap.bytes = 0;
        (std::mem::take(&mut self.tap.events), std::mem::take(&mut self.tap.lost))
    }
}

impl Tap {
    /// After a session (`session`) was read: its output and activity, when wanted and the read got
    /// anything (`before`: read_bytes before it). One branch on the mask when nothing is wanted.
    #[inline(always)]
    pub(super) fn after_read(&mut self, stats: &mut Stats, session: &str, before: u64) {
        if self.any & (kind::OUTPUT | kind::ACTIVITY) != 0 && stats.read_bytes != before {
            self.read_now(stats, session);
        }
    }

    #[cold]
    #[inline(never)]
    fn read_now(&mut self, stats: &mut Stats, session: &str) {
        let pieces = std::mem::take(&mut stats.tapped);
        // Muted (every listener wanting output is full): nothing was copied, the read is counted as dropped.
        if pieces.is_empty() && self.muted & kind::OUTPUT != 0 {
            self.emit(kind::OUTPUT, || unreachable!("muted output is only counted"));
        }
        for data in pieces {
            self.emit(kind::OUTPUT, || Event::Output { session: session.to_owned(), data });
        }
        if std::mem::take(&mut stats.tap_cut) {
            self.lost[kind::OUTPUT.trailing_zeros() as usize] += 1;
        }
        self.emit(kind::ACTIVITY, || Event::Activity { session: session.to_owned() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::testing::{advance, coalesce_ms, engine, readable, session};
    use std::io::Write as _;

    const ALL: u32 = kind::ALL;

    /// The engine's queue holds at most TAP_EVENTS events and about TAP_BYTES bytes of output between
    /// takes; the rest is counted by kind, and the counts start over once taken.
    #[test]
    fn the_queue_is_bounded() {
        let mut t = Tap::default();
        t.set(ALL, 0);
        for _ in 0..TAP_EVENTS + 10 {
            t.emit(kind::ACTIVITY, || Event::Activity { session: "a".into() });
        }
        assert_eq!(t.events.len(), TAP_EVENTS);
        assert_eq!(t.lost[kind::ACTIVITY.trailing_zeros() as usize], 10);
        t.events.clear();
        assert!(t.has_events(), "the losses are still to be told");
        t.lost = [0; kind::COUNT];
        let chunk = 1 << 20;
        for _ in 0..TAP_BYTES / chunk + 5 {
            t.emit(kind::OUTPUT, || Event::Output { session: "a".into(), data: vec![0; chunk] });
        }
        assert_eq!(t.events.len(), TAP_BYTES / chunk);
        assert_eq!(t.lost[kind::OUTPUT.trailing_zeros() as usize], 5);
        // Kinds nobody wants cost nothing and are not counted.
        t.set(kind::OUTPUT, 0);
        t.emit(kind::SESSION_CREATED, || unreachable!());
        assert_eq!(t.lost[0], 0);
        // Muted kinds are counted, never built.
        (t.events, t.lost) = (Vec::new(), [0; kind::COUNT]);
        t.set(kind::SESSION_CREATED | kind::SESSION_EXITED | kind::SESSION_RENAMED, kind::OUTPUT | kind::ACTIVITY);
        t.emit(kind::OUTPUT, || unreachable!());
        t.emit(kind::ACTIVITY, || unreachable!());
        assert_eq!((t.lost[3], t.lost[4], t.events.len()), (1, 1, 0));
    }

    /// A read is copied in pieces of at most CHUNK bytes, TAP_BYTES in all until the next event; the rest
    /// is cut and counted as one dropped output event, and a burst comes as events of CHUNK bytes or less.
    #[test]
    fn bursts_come_in_bounded_pieces() {
        let mut stats = Stats { tap: true, ..Stats::default() };
        copy(&mut stats, &[1; 10]);
        copy(&mut stats, &vec![2; CHUNK]);
        assert_eq!(stats.tapped.iter().map(Vec::len).collect::<Vec<_>>(), [CHUNK, 10]);
        assert_eq!(stats.tapped[1], [2; 10]);
        assert!(!stats.tap_cut);
        copy(&mut stats, &vec![3; TAP_BYTES * 2]);
        assert_eq!(stats.tapped.len(), TAP_BYTES / CHUNK);
        assert!(stats.tapped.iter().all(|p| p.len() == CHUNK && p.capacity() == CHUNK), "no piece grows past CHUNK");
        assert!(stats.tap_cut);
        let mut t = Tap::default();
        t.set(ALL, 0);
        stats.read_bytes = 1;
        t.after_read(&mut stats, "a", 0);
        assert!(stats.tapped.is_empty() && !stats.tap_cut);
        let outputs = t.events.iter().filter(|e| matches!(e, Event::Output { data, .. } if data.len() <= CHUNK)).count();
        assert_eq!(outputs, TAP_BYTES / CHUNK, "every piece an event of CHUNK bytes or less");
        assert_eq!(t.lost[kind::OUTPUT.trailing_zeros() as usize], 1, "the cut is counted");
    }

    /// With nothing wanted the engine queues nothing and copies no output; once output is wanted, a
    /// read's bytes come as one event, with the session's lifecycle around them when asked for.
    #[test]
    fn reads_and_lifecycle_are_tapped_only_when_wanted() {
        let mut e = engine();
        let (key, mut prog) = session(&mut e, "a");
        prog.write_all(b"one").unwrap();
        e.handle(&[readable(&e, key)], &mut Vec::new());
        advance(&e, 2 * coalesce_ms());
        e.tick();
        assert!(!e.has_tap_events() && e.stats.tapped.capacity() == 0, "nothing wanted, nothing kept");
        e.set_tap(ALL, 0);
        prog.write_all(b"two").unwrap();
        e.handle(&[readable(&e, key)], &mut Vec::new());
        advance(&e, 2 * coalesce_ms());
        e.tick();
        assert_eq!(e.command(&["rename-session".into(), "-t".into(), "a".into(), "b".into()], "/").code, 0);
        assert_eq!(e.command(&["kill-session".into(), "-t".into(), "b".into()], "/").code, 0);
        let (evs, lost) = e.take_tap();
        assert_eq!(lost, [0; kind::COUNT]);
        assert_eq!(
            evs,
            [
                Event::Output { session: "a".into(), data: b"two".to_vec() },
                Event::Activity { session: "a".into() },
                Event::SessionRenamed { session: "b".into(), from: "a".into() },
                Event::SessionExited { session: "b".into(), reason: ExitReason::Killed },
            ]
        );
        e.set_tap(0, 0);
        assert!(!e.stats.tap && e.tap.any == 0);
    }
}
