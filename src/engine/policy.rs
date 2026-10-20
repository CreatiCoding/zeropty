//! The engine's timing decisions as pure functions of the clock: when a coalesced read is due, how long
//! a session stays a live screen once quiet, and when a screen is frozen or compressed. `Engine` feeds
//! them `now_ms()`; the tests feed them numbers.

use super::options::{DEEPEN_AFTER_MS, HOLD_CAP_MS, HOLD_MAX_MS};

/// When a read is due for output that arrived at `now`, coalescing every `interval` ms: the first multiple
/// of `interval` at least `interval` after `now`, so sessions that turn readable close together are read
/// in one wakeup (with hundreds of busy sessions, one wakeup each cost more than reading). `interval` 1:
/// the next millisecond.
pub(super) fn coalesced_read_at(now: u64, interval: u64) -> u64 {
    let c = interval.max(1);
    (now + c).div_ceil(c) * c
}

/// The hold of a frozen session that prints `gap` ms after its last output: its gap plus `freeze_after`,
/// up to HOLD_CAP_MS, so output coming every few hundred ms is not frozen and thawed between reads;
/// `freeze_after` alone after a long silence (HOLD_MAX_MS), which starts a new burst.
pub(super) fn hold_after(gap: u64, freeze_after: u64) -> u16 {
    if gap < HOLD_MAX_MS { (gap + freeze_after).min(HOLD_CAP_MS) as u16 } else { freeze_after as u16 }
}

/// What a sweep does with a screen last active (written or read) at `active`.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Sweep {
    /// Leave it as it is.
    Keep,
    /// A live screen quiet for its hold: freeze it.
    Freeze,
    /// A frozen screen quiet for DEEPEN_AFTER_MS: compress it.
    Deepen,
}

/// The sweep's decision at `now` for a screen last active at `active` that is `frozen` (and already
/// `deep`), with `hold` its quiet time before freezing.
pub(super) fn sweep(now: u64, active: u64, hold: u16, frozen: bool, deep: bool) -> Sweep {
    match (frozen, deep) {
        (true, false) if active + DEEPEN_AFTER_MS <= now => Sweep::Deepen,
        (true, _) => Sweep::Keep,
        (false, _) if active + u64::from(hold) <= now => Sweep::Freeze,
        (false, _) => Sweep::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::{Sweep, coalesced_read_at, hold_after, sweep};
    use crate::engine::options::{DEEPEN_AFTER_MS, HOLD_CAP_MS, HOLD_MAX_MS};

    #[test]
    fn reads_meet_at_boundaries() {
        assert_eq!(coalesced_read_at(1000, 300), 1500);
        assert_eq!(coalesced_read_at(1200, 300), 1500, "at least an interval later");
        assert_eq!(coalesced_read_at(1201, 300), 1800);
        assert_eq!(coalesced_read_at(1150, 300), coalesced_read_at(1010, 300), "close together: one wakeup");
        assert_eq!(coalesced_read_at(1000, 1), 1001, "watched: at once");
        assert_eq!(coalesced_read_at(1000, 0), 1001, "0 is taken as 1");
    }

    #[test]
    fn holds_grow_with_the_gap_up_to_the_cap() {
        assert_eq!(hold_after(100, 250), 350);
        assert_eq!(hold_after(HOLD_MAX_MS - 1, 250), HOLD_CAP_MS as u16);
        assert_eq!(hold_after(HOLD_MAX_MS, 250), 250, "a new burst");
    }

    #[test]
    fn sweeps_freeze_then_deepen() {
        assert_eq!(sweep(1000, 800, 250, false, false), Sweep::Keep);
        assert_eq!(sweep(1050, 800, 250, false, false), Sweep::Freeze);
        assert_eq!(sweep(800 + DEEPEN_AFTER_MS - 1, 800, 250, true, false), Sweep::Keep);
        assert_eq!(sweep(800 + DEEPEN_AFTER_MS, 800, 250, true, false), Sweep::Deepen);
        assert_eq!(sweep(800 + DEEPEN_AFTER_MS, 800, 250, true, true), Sweep::Keep, "already deep");
    }
}
