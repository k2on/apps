//! Numbers, days and lengths as people read them: `ark_format`, the crate
//! every client formats with (the phones through their libraries), and
//! what only a window can tell it — where the person is ([`utc_offset_minutes`]).

pub use ark_format::*;

/// The minutes this machine's clock is east of UTC at `now_ms`, which
/// [`published`] takes to find the day an instant fell on where the person
/// is: the system's time zone on the desktop, the browser's in a page.
pub fn utc_offset_minutes(now_ms: i64) -> i32 {
    {
        use chrono::{Local, Offset, TimeZone};
        match Local.timestamp_millis_opt(now_ms) {
            chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => t.offset().fix().local_minus_utc() / 60,
            chrono::LocalResult::None => 0,
        }
    }
}
