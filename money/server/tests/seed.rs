//! The demo's household seeded into a data directory (`money-seed`, in
//! `money/seed`), the server started on it, and a phone signed in as the
//! household's owner: what the app screenshots sign in to
//! (.github/screens/apps.md). The phone gets every account the demo window
//! has.

mod common;

use common::*;
use money_domain::*;
use money_seed as seed;

fn accounts(p: &Peer) -> Vec<AccountLine> {
    p.query("accounts", &()).unwrap_or_default()
}

/// Falsified by seeding a store without its snapshot, or with rows the
/// server's schema does not grow from: the server would start empty, or
/// not at all, and the phone would open on a budget with nothing in it.
#[test]
fn a_seeded_server_opens_on_the_demo_household() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let mut disk = ark_peer::Files::new(dir.path().join("log")).unwrap();
    let now = ark_peer::now_ms();
    seed::log(&mut disk, &dates::day_of_ms(now), now).unwrap();
    drop(disk);
    let server = serve(&rt, dir.path(), |_| {});

    let mut alex = signed_in(&server, seed::ME);
    pump_until(&mut [&mut alex], 15, |ps| accounts(ps[0]).len() == 6);
    let names: Vec<String> = accounts(&alex).into_iter().map(|a| a.name).collect();
    for n in ["Checking", "Savings", "Cash", "Mortgage", "Retirement", "Chase Sapphire"] {
        assert!(names.iter().any(|m| m == n), "{n} is among {names:?}");
    }
    assert!(alex.refused.is_empty(), "{:?}", alex.refused);
}
