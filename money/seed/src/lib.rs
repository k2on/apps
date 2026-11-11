//! The demo's household (money-core's `demo`) on a peer and a disk: what
//! the demo window, the phones' demo, the browser's `?demo` and a seeded
//! server open on (docs/seed.md). The household itself, and the rules it
//! is made by, are the core's and pure; writing it to a disk and opening a
//! peer over it is this adapter's.

use ark_peer::{Config, Disk, Memory, Peer};
use money_core::BANK;

pub use money_core::demo::*;

/// The household, three months of it up to `today`: the demo's records,
/// its own authority's, as the demo window and the phones' demo open them.
pub fn seeded_disk(today: &str, now: i64) -> Memory {
    let mut disk = Memory::default();
    log(&mut disk, today, now).expect("memory takes it");
    ark_peer::local::preset(&mut disk, |r| (r.actor, r.session) = (Some(ME.into()), LOCAL.into())).expect("memory takes it");
    disk
}

/// The household written onto `disk` as a log starting from its snapshot:
/// what a server's data directory (`<data>/log`) is seeded with before it
/// starts, so a phone signed in as [`ME`] opens on it (`money-seed`, what
/// the app screenshots run: .github/screens/apps.md).
pub fn log(disk: &mut dyn Disk, today: &str, now: i64) -> std::io::Result<()> {
    let (record, schema) = snapshot(today, now);
    disk.record(&record)?;
    ark_peer::migrate::put_schema(disk, &schema)
}

/// The demo's peer: the seeded household, its own authority, holding the
/// bank importer's role so that "Sync" can run the import here as a
/// server's importer would.
pub fn open(disk: Memory) -> Peer {
    let mut peer = Peer::open_as(Config::of(money_core::shared()), Box::new(disk), None);
    peer.set_roles([BANK]);
    peer.alone();
    peer
}

/// The demo, seeded for today.
pub fn seeded() -> Peer {
    let now = ark_peer::now_ms();
    open(seeded_disk(&money_core::dates::day_of_ms(now), now))
}
