//! The bank importer, inside the server, against a scripted Plaid on
//! loopback: the three phases ArkOS's end-to-end check walked Actual
//! through (`money/domain/tests/import.rs` walks the domain through the
//! same answers), the accounts named and followed as the server was told,
//! a household member's ask answered, the cursor kept across a restart and
//! only once the import is in, a run said twice changing nothing, and no
//! secret ever said.

mod common;

use common::*;
use money_domain::*;
use money_server::importer::{state_file, Outcome, State};

/// Every transaction the hub holds, as the check compares them: date,
/// payee, amount, whether it cleared, the bank's id.
fn txns(server: &money_server::Server) -> Vec<(String, String, i64, Cleared, Option<String>)> {
    let mut t: Vec<_> = hub_rows::<Txn>(server, "txn")
        .into_iter()
        .map(|t| (t.date, t.payee, t.amount, t.cleared, t.imported_id))
        .collect();
    t.sort();
    t
}

fn cursor(data: &std::path::Path) -> Option<String> {
    State::load(&state_file(data)).unwrap().cursors.get("chase").cloned()
}

/// The first run at start pulls both pages and names Plaid's acc1 as the
/// household had it in Actual; acc2, not named, is known and opens
/// nothing. A household member asks for a sync on the socket, and the
/// importer answers it with phase two: the pending fill-up posted in place,
/// the coffee's new amount. A nudge with nothing new changes nothing, and
/// every cursor asked is the one the run before kept. Falsified by keeping
/// the cursor before the hub confirmed: see `a_failed_run_keeps_the_cursor`.
#[test]
fn the_importer_walks_one_bank_through_three_phases() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    let banks = banks(&mock, secrets.path(), &[("Joint Checking", "chase", "0000")]);
    let server = serve(&rt, data.path(), |c| c.banks = Some(banks));

    let r = runs(&server, 1);
    assert_eq!(r[0].banks["chase"], Outcome::Imported { upserts: 4, removed: 0 });
    assert_eq!(mock.cursors(), [None, Some("p1a".to_string())]);
    assert_eq!(cursor(data.path()).as_deref(), Some("c1"));
    let accounts: Vec<AccountLine> = hub_query(&server, "accounts.accounts", ());
    assert_eq!(accounts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Joint Checking"]);
    let checking = &accounts[0];
    assert_eq!((checking.bank.as_str(), checking.mask.as_str()), ("chase", "0000"));
    let linked: Vec<Linked> = hub_rows(&server, "linked");
    let savings = linked.iter().find(|l| l.plaid_account_id == "acc2").unwrap();
    assert_eq!((savings.account_id, savings.ignored), (None, true));
    let after1 = txns(&server);
    assert!(after1.iter().all(|t| t.1 != "INTEREST"), "acc2 is not followed: {after1:#?}");
    assert!(after1.iter().any(|t| t.1 == "GAS STATION PENDING" && t.3 == Cleared::Uncleared));
    let imported: Vec<Txn> = hub_rows(&server, "txn");
    assert!(
        imported.iter().filter(|t| t.imported_id.is_some()).all(|t| t.user_id == "bank"),
        "the bank put them there"
    );

    // Somebody asks, on the socket, as any client would.
    mock.phase(2);
    let mut alice = signed_in(&server, "alice");
    pump_until(&mut [&mut alice], 10, |ps| ps[0].cursor() == head(&server));
    alice.mutate("banks.sync", &());
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    let r = runs(&server, 2);
    assert_eq!(r[1].banks["chase"], Outcome::Imported { upserts: 2, removed: 1 });
    assert_eq!(cursor(data.path()).as_deref(), Some("c2"));
    let after2 = txns(&server);
    let gas: Vec<_> = after2.iter().filter(|t| t.1.starts_with("GAS")).collect();
    assert_eq!(gas.len(), 1, "posted in place: {gas:?}");
    assert_eq!((gas[0].2, gas[0].3, gas[0].4.as_deref()), (-2_150, Cleared::Cleared, Some("T2")));
    assert!(after2.iter().any(|t| t.1 == "Coffee Shop" && t.2 == -5_500));
    // And the household sees it on its own device.
    pump_until(&mut [&mut alice], 10, |ps| ps[0].cursor() == head(&server));
    let banks: Vec<BankLine> = alice.query("banks.banks", &()).unwrap();
    assert_eq!((banks[0].name.as_str(), banks[0].updated), ("chase", 2));

    // Phase three: nothing new, nothing moves.
    mock.phase(3);
    server.importer.as_ref().unwrap().nudge();
    runs(&server, 3);
    assert_eq!(txns(&server), after2);
    assert_eq!(mock.cursors()[2..], [Some("c1".to_string()), Some("c2".to_string())]);
    rt.block_on(server.stop());
}

/// A restart asks from the cursor the last run kept and answers no ask it
/// answered before; a run said again from an older cursor — the import
/// in, the cursor never kept — changes nothing. Falsified by loading no
/// state at start: the restart asked from no cursor, which the script
/// refuses in phase two.
#[test]
fn a_restart_resumes_from_the_cursor_and_a_run_said_twice_changes_nothing() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    let server = serve(&rt, data.path(), |c| c.banks = Some(banks(&mock, secrets.path(), &[])));
    runs(&server, 1);
    // No account named: every account followed, under the bank's name.
    let accounts: Vec<AccountLine> = hub_query(&server, "accounts.accounts", ());
    assert_eq!(
        accounts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        ["Plaid Checking", "Plaid Savings"]
    );
    mock.phase(2);
    let mut alice = signed_in(&server, "alice");
    alice.mutate("banks.sync", &());
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    runs(&server, 2);
    let after2 = txns(&server);
    rt.block_on(server.stop());

    let server = serve(&rt, data.path(), |c| c.banks = Some(banks(&mock, secrets.path(), &[])));
    let r = runs(&server, 1);
    assert_eq!(r[0].banks["chase"], Outcome::Imported { upserts: 0, removed: 0 });
    assert_eq!(mock.cursors().last().unwrap().as_deref(), Some("c2"));
    assert_eq!(txns(&server), after2);
    // The ask was answered before the restart: one run, the start's.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(server.importer.as_ref().unwrap().runs().len(), 1);
    rt.block_on(server.stop());

    // As if the server had stopped between the import and keeping its
    // cursor: phase two again, from c1.
    let mut state = State::load(&state_file(data.path())).unwrap();
    state.cursors.insert("chase".into(), "c1".into());
    state.save(&state_file(data.path())).unwrap();
    let server = serve(&rt, data.path(), |c| c.banks = Some(banks(&mock, secrets.path(), &[])));
    let r = runs(&server, 1);
    assert_eq!(r[0].banks["chase"], Outcome::Imported { upserts: 2, removed: 1 });
    assert_eq!(txns(&server), after2, "the same run twice is the same budget");
    assert_eq!(cursor(data.path()).as_deref(), Some("c2"));
    rt.block_on(server.stop());
}

/// A run that fails partway — Plaid's second page an error — imports
/// nothing and keeps no cursor, and the next run asks again from the
/// start. Falsified by keeping `next_cursor` page by page: the cursor was
/// `p1a`.
#[test]
fn a_failed_run_keeps_the_cursor() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    mock.phase(4);
    let server = serve(&rt, data.path(), |c| c.banks = Some(banks(&mock, secrets.path(), &[])));
    let r = runs(&server, 1);
    assert!(
        matches!(&r[0].banks["chase"], Outcome::Failed(why) if why.contains("INTERNAL_SERVER_ERROR")),
        "{r:?}"
    );
    assert_eq!(cursor(data.path()), None);
    assert!(hub_rows::<Bank>(&server, "bank").is_empty());

    mock.phase(1);
    server.importer.as_ref().unwrap().nudge();
    let r = runs(&server, 2);
    assert!(matches!(r[1].banks["chase"], Outcome::Imported { .. }));
    assert_eq!(mock.cursors(), [None, Some("p1a".into()), None, Some("p1a".into())]);
    assert_eq!(cursor(data.path()).as_deref(), Some("c1"));
    rt.block_on(server.stop());
}

/// Neither the keys nor a token is ever in a line the importer says —
/// a run that works, and one whose token Plaid turns away — while every
/// request carried the token to Plaid. Falsified by saying the request
/// body in a failure: the token was there.
#[test]
fn no_secret_is_ever_said() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    let mut b = banks(&mock, secrets.path(), &[]);
    let wrong = secrets.path().join("plaid-amex");
    std::fs::write(&wrong, "access-wrong-31e8\n").unwrap();
    b.items.insert("amex".into(), wrong);
    let server = serve(&rt, data.path(), |c| c.banks = Some(b));
    let r = runs(&server, 1);
    assert!(
        matches!(&r[0].banks["amex"], Outcome::Failed(why) if why.contains("INVALID_ACCESS_TOKEN")),
        "{r:?}"
    );
    assert!(matches!(r[0].banks["chase"], Outcome::Imported { .. }));
    let said = server.importer.as_ref().unwrap().said().join("\n");
    assert!(said.contains("amex: failed"), "{said}");
    for secret in [CLIENT_ID, SECRET, TOKEN, "access-wrong-31e8"] {
        assert!(!said.contains(secret), "{secret} was said:\n{said}");
    }
    assert!(mock.script.lock().unwrap().bodies.iter().all(|b| b.contains("access-")));
    // Nor is one in the log.
    let leaked = server
        .running
        .hub
        .with(|h| {
            let tables = ["account", "txn", "bank", "linked", "asked"];
            tables.iter().any(|t| h.log.store.rows(t).any(|r| r.windows(7).any(|w| w == b"access-")))
        })
        .unwrap();
    assert!(!leaked, "a token is in the log");
    rt.block_on(server.stop());
}
