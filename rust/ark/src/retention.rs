//! How much of the log is kept: one rule, for the server's log and — later
//! — a replica's own confirmed journal (`docs/plan-alone.md` §3,
//! `docs/plan-perf.md` R10).
//!
//! A log is permanent in what it *means* and not in what it *holds*: an
//! entry at or below the horizon is represented by the snapshot the log
//! stands on (§10.3), and a peer whose cursor falls below it is served that
//! snapshot and rebases its pending intents onto it (§12.2, §12.4). So
//! moving the horizon never loses anything; what it costs is a snapshot
//! sent where a page would have done. The rule decides when that trade is
//! worth making, and it is a pure function so that the server and a peer
//! alone make it the same way and a test can walk its edges without a
//! clock or a disk:
//!
//! - **keep everything above the lowest cursor of any session heard within
//!   the window** ([`RETAIN_DAYS`]): a laptop closed for a fortnight comes
//!   back to a page, not a snapshot;
//! - **and never fewer than [`RETAIN_ENTRIES`] below the head**: a peer a
//!   little behind — one that crashed before its disk caught up with what
//!   it was sent, one whose session was never recorded — is paged too;
//! - **move the horizon only when the log holds half as much again as the
//!   rule keeps**: a compaction rewrites the snapshot, and one per append
//!   would be a snapshot per append.
//!
//! A cursor already below the horizon pins nothing: that peer is served the
//! snapshot whatever is kept, and keeping entries for it would keep them
//! for nobody.
//!
//! Entry ids are not retention's to drop: a log keeps every id it ever
//! sequenced, below the horizon too, by design (§10.3), so a re-push is
//! still recognised.

use crate::log::Seq;

/// Entries kept below the head whoever has been heard from. Ten thousand
/// is weeks of a household's use of a music library — a scan of a large
/// directory is the burst, and it is a few thousand `add_song`s — so a peer
/// that is merely behind is paged; and at the few hundred bytes an entry
/// takes in memory and on disk (the fleet measures ~200 on disk), it is a
/// few megabytes, which is what a server's log should cost regardless of
/// how long it has run.
pub const RETAIN_ENTRIES: u64 = 10_000;

/// How long a session keeps the log above its cursor after it was last
/// heard from. Thirty days is longer than a holiday and shorter than a
/// device that is simply gone: a phone left in a drawer is served a
/// snapshot when it comes back, which is correct and costs one message,
/// and does not hold the log for ever in the meantime.
pub const RETAIN_DAYS: u64 = 30;

/// A day, in the milliseconds a last-heard time is kept in.
pub const DAY_MS: i64 = 86_400_000;

/// The two constants, as a deployment sets them (`HARKEN_RETAIN_ENTRIES`,
/// `HARKEN_RETAIN_DAYS`). `days: 0` holds the log only for a session heard
/// at this very moment — one with an open connection — which is what a
/// test that wants a returning peer served the snapshot asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    pub entries: u64,
    pub days: u64,
}

impl Default for Retention {
    fn default() -> Retention {
        Retention {
            entries: RETAIN_ENTRIES,
            days: RETAIN_DAYS,
        }
    }
}

impl Retention {
    /// The window, in milliseconds; saturating, so a deployment that says
    /// "for ever" in days gets for ever rather than an overflow.
    pub fn window_ms(&self) -> i64 {
        i64::try_from(self.days).unwrap_or(i64::MAX).saturating_mul(DAY_MS)
    }
}

/// The sequence to move the horizon to, or `None` to leave it (the module
/// docs). `heard` is every session's `(cursor, last heard, in ms)`; a
/// session with an open connection is heard `now_ms`. The answer, when
/// there is one, is above `horizon` and at most `head`.
///
/// **The hysteresis is relative to what is kept.** The log is compacted
/// when it holds more than half as much again as the rule keeps — with
/// every peer caught up that is [`RETAIN_ENTRIES`] × 1.5, the threshold
/// `docs/plan-alone.md` §3 states. Measured against the fixed constant
/// instead, a session holding the log low would make every step it
/// advanced a compaction, each rewriting everything it holds; measured
/// against what is kept, each compaction drops at least a third of the
/// log, so the snapshot bytes written stay within twice the entries
/// dropped however far behind the slowest peer is.
pub fn compact_to(head: Seq, horizon: Seq, retain: Retention, now_ms: i64, heard: impl IntoIterator<Item = (Seq, i64)>) -> Option<Seq> {
    let floor = head.saturating_sub(i64::try_from(retain.entries).unwrap_or(i64::MAX));
    let window = retain.window_ms();
    let pinned = heard
        .into_iter()
        .filter(|(cursor, at)| *cursor >= horizon && now_ms.saturating_sub(*at) <= window)
        .map(|(cursor, _)| cursor)
        .min();
    let target = pinned.map_or(floor, |p| p.min(floor)).min(head);
    if target <= horizon {
        return None;
    }
    let (kept, retained) = (head - target, head - horizon);
    (retained > kept + kept / 2).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000 * DAY_MS;

    fn rule(entries: u64, days: u64) -> Retention {
        Retention { entries, days }
    }

    /// Nobody heard from: the floor alone, and only past half again.
    /// Falsified by compacting whenever the log holds more than it keeps
    /// (`retained > kept`): 15,000 compacts to 5,000.
    #[test]
    fn the_floor_and_half_again() {
        let r = Retention::default();
        assert_eq!(compact_to(10_000, 0, r, NOW, []), None, "exactly the floor");
        assert_eq!(compact_to(15_000, 0, r, NOW, []), None, "half again, not more");
        assert_eq!(compact_to(15_001, 0, r, NOW, []), Some(5_001));
        assert_eq!(compact_to(30_000, 0, r, NOW, []), Some(20_000));
        // Just compacted: nothing until the log grows by half again.
        assert_eq!(compact_to(24_999, 10_000, r, NOW, []), None);
        assert_eq!(compact_to(25_001, 10_000, r, NOW, []), Some(15_001));
        assert_eq!(compact_to(50, 0, rule(0, 30), NOW, []), Some(50), "keep none: compact to the head");
    }

    /// A session heard inside the window keeps the log above its cursor
    /// however far below the head it is; one outside the window does not,
    /// and the window's edge is inside it. Falsified by leaving the window
    /// out (every session pins): the session heard thirty-one days ago
    /// holds the log at 100.
    #[test]
    fn a_session_inside_the_window_keeps_the_log() {
        let r = Retention::default();
        let yesterday = NOW - DAY_MS;
        assert_eq!(compact_to(30_000, 0, r, NOW, [(100, yesterday)]), None, "held: everything above 100");
        assert_eq!(compact_to(30_000, 0, r, NOW, [(100, NOW - 31 * DAY_MS)]), Some(20_000));
        assert_eq!(compact_to(30_000, 0, r, NOW, [(100, NOW - 30 * DAY_MS)]), None, "the edge is in");
        assert_eq!(compact_to(30_000, 0, r, NOW, [(100, NOW - 30 * DAY_MS - 1)]), Some(20_000));
        // Far enough behind the head to be worth a compaction: to the
        // session's cursor, not past it.
        assert_eq!(compact_to(100_000, 0, r, NOW, [(60_000, yesterday)]), Some(60_000));
        // The lowest recent cursor is the one; a caught-up one changes
        // nothing, and one outside the window counts for nothing.
        let many = [(100_000, NOW), (60_000, yesterday), (2_000, NOW - 40 * DAY_MS)];
        assert_eq!(compact_to(100_000, 0, r, NOW, many), Some(60_000));
        // `days: 0` holds the log for an open connection only.
        assert_eq!(compact_to(30_000, 0, rule(10_000, 0), NOW, [(100, NOW - 1)]), Some(20_000));
        assert_eq!(compact_to(30_000, 0, rule(10_000, 0), NOW, [(100, NOW)]), None);
    }

    /// A recent session never takes the horizon below the floor's reach
    /// the other way: one at the head keeps `RETAIN_ENTRIES` all the same.
    /// Falsified by taking the session's cursor over the floor
    /// (`p.max(floor)`): 30,000.
    #[test]
    fn never_fewer_than_the_floor() {
        let r = Retention::default();
        assert_eq!(compact_to(30_000, 0, r, NOW, [(30_000, NOW)]), Some(20_000));
        assert_eq!(compact_to(30_000, 0, r, NOW, [(40_000, NOW)]), Some(20_000), "a cursor past the head");
        assert_eq!(compact_to(9_000, 0, r, NOW, [(9_000, NOW)]), None, "under the floor, nothing");
    }

    /// The hysteresis is relative to what is kept (the function's docs): a
    /// session holding the log low does not make each step it advances a
    /// compaction. Falsified by measuring against `retain.entries`: the
    /// step from 100 to 101 compacts.
    #[test]
    fn a_slow_session_does_not_compact_every_step() {
        let r = Retention::default();
        let yesterday = NOW - DAY_MS;
        assert_eq!(compact_to(30_000, 100, r, NOW, [(101, yesterday)]), None);
        assert_eq!(compact_to(30_000, 100, r, NOW, [(10_000, yesterday)]), None, "a third of it would go");
        assert_eq!(compact_to(30_000, 100, r, NOW, [(10_200, yesterday)]), Some(10_200));
    }

    /// A cursor below the horizon pins nothing: that peer is served the
    /// snapshot whatever is kept. Falsified by counting it: the log is held
    /// at the horizon for ever.
    #[test]
    fn a_cursor_below_the_horizon_pins_nothing() {
        let r = Retention::default();
        assert_eq!(compact_to(30_000, 5_000, r, NOW, [(10, NOW)]), Some(20_000));
        assert_eq!(compact_to(30_000, 5_000, r, NOW, [(5_000, NOW)]), None, "at the horizon it pins");
    }
}
