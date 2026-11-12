//! The demo: the seeded household (`money_core::demo`) on a peer that is
//! its own authority, in memory — what the apps run with no server, their
//! previews draw and their screenshots are taken of. It is the demo
//! window's (`money/iced`) to the cent: Alex's, some of it Sam's, the
//! Chase card's last few days waiting for a look. Nothing it does leaves
//! the process, and opening it again starts from the seed again.

use std::sync::Arc;

use ark_ffi::{Peer, Refusal};
use money_core::dates::{days_from_civil, parse_day};
use money_seed as seed;

/// Noon of `day` (UTC), in ms: when a demo seeded for that day says it is.
fn noon(day: &str) -> Option<i64> {
    let (y, m, d) = parse_day(day)?;
    Some(days_from_civil(y, m, d) * 86_400_000 + 43_200_000)
}

/// The demo's household, seeded up to `today` (`2026-10-26`) — today when
/// none, and a fixed day is the same household every time, which is what
/// a screenshot wants. Alone, signed in as Alex, holding the bank
/// importer's role so that money's host (`money_app::host`) can run its
/// bank's import here as a server's importer would.
#[uniffi::export]
pub fn money_demo(today: Option<String>) -> Result<Arc<Peer>, Refusal> {
    let (today, now) = match today {
        Some(t) => {
            let now = noon(&t).ok_or_else(|| format!("a day, as 2026-10-26: {t}"))?;
            (t, now)
        }
        None => {
            let now = ark_peer::now_ms();
            (money_core::dates::day_of_ms(now), now)
        }
    };
    Peer::of(seed::open(seed::seeded_disk(&today, now)))
}
