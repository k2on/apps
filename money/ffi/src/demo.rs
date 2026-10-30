//! The demo: the seeded household (`money_domain::seed`) on a peer that is
//! its own authority, in memory — what the apps run with no server, their
//! previews draw and their screenshots are taken of. It is the demo
//! window's (`money/iced`) to the cent: Alex's, some of it Sam's, the
//! Chase card's last few days waiting for a look. Nothing it does leaves
//! the process, and opening it again starts from the seed again.

use std::sync::{Arc, Mutex};

use ark::format::Encode;
use ark_ffi::{Peer, Refusal};
use money_domain::dates::{days_from_civil, parse_day};
use money_domain::seed;
use money_domain::AccountLine;

/// Noon of `day` (UTC), in ms: when a demo seeded for that day says it is.
fn noon(day: &str) -> Option<i64> {
    let (y, m, d) = parse_day(day)?;
    Some(days_from_civil(y, m, d) * 86_400_000 + 43_200_000)
}

/// The demo's household, seeded up to `today` (`2026-10-26`) — today when
/// none, and a fixed day is the same household every time, which is what
/// a screenshot wants. Alone, signed in as Alex, holding the bank
/// importer's role so that [`DemoBank::sync`] can run the import here as a
/// server's importer would.
#[uniffi::export]
pub fn money_demo(today: Option<String>) -> Result<Arc<Peer>, Refusal> {
    let (today, now) = match today {
        Some(t) => {
            let now = noon(&t).ok_or_else(|| format!("a day, as 2026-10-26: {t}"))?;
            (t, now)
        }
        None => {
            let now = ark_peer::now_ms();
            (money_domain::dates::day_of_ms(now), now)
        }
    };
    Peer::of(seed::open(seed::seeded_disk(&today, now)))
}

/// What one press of the demo's Sync did: the import's intent, and what
/// the demo says of it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DemoSync {
    pub intent: String,
    pub note: String,
}

/// The demo's bank, standing in for Plaid behind Sync (`seed::Bank`): the
/// first sync brings a coffee still pending and a fill-up, the second
/// posts the coffee with its tip and brings an order, and after that the
/// bank has nothing new.
#[derive(uniffi::Object)]
pub struct DemoBank {
    bank: Mutex<seed::Bank>,
}

#[uniffi::export]
impl DemoBank {
    #[uniffi::constructor]
    pub fn new() -> Arc<DemoBank> {
        Arc::new(DemoBank { bank: Mutex::default() })
    }

    /// The bank's next run, on `today` (`2026-10-26`; today when none),
    /// imported on the demo's `peer` as its importer authors it.
    pub fn sync(&self, peer: Arc<Peer>, today: Option<String>) -> Result<DemoSync, Refusal> {
        let today = today.unwrap_or_else(crate::money_today);
        let mut bank = self.bank.lock().unwrap_or_else(|e| e.into_inner());
        let (intent, n, u) = peer.with(|p| -> Result<_, String> {
            let lines = Vec::<AccountLine>::from_bytes(&p.ask("accounts", &().to_bytes())?).map_err(|e| e.to_string())?;
            let owed = lines.iter().find(|a| a.bank == seed::BANK_NAME).map_or(0, |a| a.cleared);
            let import = bank.next(&today, owed);
            let (n, u) = (import.upserts.len(), import.removed.len());
            Ok((p.mutate("import", &import)?, n, u))
        })?;
        let note = match (n, bank.syncs) {
            (0, _) => "Chase: nothing new at the bank".into(),
            (_, 1) => format!("Chase: {n} new from the bank \u{2014} one still pending"),
            _ => format!("Chase: {n} from the bank, {u} pending one posted"),
        };
        Ok(DemoSync {
            intent: ark::hash::hex(&intent.0),
            note,
        })
    }
}
