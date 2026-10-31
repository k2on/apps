//! The bank importer, inside the server, against a scripted Plaid on
//! loopback: the three phases ArkOS's end-to-end check walked Actual
//! through (`money/domain/tests/import.rs` walks the domain through the
//! same answers), a new bank's accounts waiting for the household to say
//! what each feeds, a household member's ask answered, the cursor kept
//! across a restart and only once the import is in, a run said twice
//! changing nothing, and no secret ever said.

mod common;

use common::*;
use money_domain::*;
use money_server::importer::Outcome;

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
    store(data).banks.get("chase").and_then(|c| c.cursor.clone())
}

/// Both of chase's accounts given accounts of their own, as somebody would
/// from the Banks page: the next run asks from the start.
fn open_both(alice: &mut Peer) {
    for id in ["acc1", "acc2"] {
        let open = OpenLinked {
            plaid_account_id: id.into(),
            name: String::new(),
        };
        alice.mutate("banks.open", &open);
    }
    pump_until(&mut [alice], 10, |ps| ps[0].pending() == 0);
}

/// The first run at start pulls both pages and opens nothing: both
/// accounts wait for somebody to choose. Alice opens acc1 as the household
/// had it in Actual and ignores acc2, on the socket; the importer asks for
/// the whole history again and acc1 opens with its starting balance. Her
/// sync is answered with phase two: the pending fill-up posted in place, the
/// coffee's new amount. A nudge with nothing new changes nothing, and every
/// cursor asked is the one the run before kept. Falsified by keeping the
/// cursor through a choice: the second run asked from c1, and acc1's
/// history never arrived.
#[test]
fn the_importer_walks_one_bank_through_three_phases() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    connect(data.path(), &[("chase", TOKEN, None)]);
    let server = importing(&rt, data.path(), &mock, secrets.path());

    let r = runs(&server, 1);
    assert_eq!(r[0].banks["chase"], Outcome::Imported { upserts: 4, removed: 0 });
    assert_eq!(mock.cursors(), [None, Some("p1a".to_string())]);
    assert_eq!(cursor(data.path()).as_deref(), Some("c1"));
    assert!(hub_rows::<Account>(&server, "account").is_empty(), "nothing opens by itself");
    assert!(hub_rows::<Txn>(&server, "txn").is_empty());
    let linked: Vec<LinkedLine> = hub_query(&server, "banks.linked", ());
    assert!(linked.iter().all(|l| l.waiting), "{linked:#?}");

    let mut alice = caught_up(&server, "alice");
    let open = OpenLinked {
        plaid_account_id: "acc1".into(),
        name: "Joint Checking".into(),
    };
    alice.mutate("banks.open", &open);
    let ignore = IgnoreLinked {
        plaid_account_id: "acc2".into(),
        ignored: true,
    };
    alice.mutate("banks.ignore", &ignore);
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    let r = runs(&server, 2);
    assert_eq!(r[1].banks["chase"], Outcome::Imported { upserts: 4, removed: 0 });
    assert_eq!(mock.cursors()[2..], [None, Some("p1a".to_string())], "from the start");
    let accounts: Vec<AccountLine> = hub_query(&server, "accounts.accounts", ());
    assert_eq!(accounts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Joint Checking"]);
    let checking = &accounts[0];
    assert_eq!((checking.bank.as_str(), checking.mask.as_str()), ("chase", "0000"));
    assert_eq!(
        checking.cleared, 92_350,
        "the bank's balance: the history and the starting balance before it"
    );
    let after1 = txns(&server);
    assert!(after1.iter().all(|t| t.1 != "INTEREST"), "acc2 is not followed: {after1:#?}");
    assert!(after1.iter().any(|t| t.1 == "GAS STATION PENDING" && t.3 == Cleared::Uncleared));
    let imported: Vec<Txn> = hub_rows(&server, "txn");
    assert!(
        imported.iter().filter(|t| t.imported_id.is_some()).all(|t| t.user_id == "bank"),
        "the bank put them there"
    );
    assert!(!hub_rows::<Bank>(&server, "bank")[0].repull);

    // Somebody asks, on the socket, as any client would.
    mock.phase(2);
    alice.mutate("banks.sync", &());
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    let r = runs(&server, 3);
    assert_eq!(r[2].banks["chase"], Outcome::Imported { upserts: 2, removed: 1 });
    assert_eq!(cursor(data.path()).as_deref(), Some("c2"));
    let after2 = txns(&server);
    let gas: Vec<_> = after2.iter().filter(|t| t.1.starts_with("GAS")).collect();
    assert_eq!(gas.len(), 1, "posted in place: {gas:?}");
    assert_eq!((gas[0].2, gas[0].3, gas[0].4.as_deref()), (-2_150, Cleared::Cleared, Some("T2")));
    assert!(after2.iter().any(|t| t.1 == "Coffee Shop" && t.2 == -5_500));
    // And the household sees it on its own device.
    pump_until(&mut [&mut alice], 10, |ps| ps[0].cursor() == head(&server));
    let banks: Vec<BankLine> = alice.query("banks.banks", &()).unwrap();
    assert_eq!((banks[0].name.as_str(), banks[0].updated, banks[0].waiting), ("chase", 2, 0));

    // Phase three: nothing new, nothing moves.
    mock.phase(3);
    server.importer.as_ref().unwrap().nudge();
    runs(&server, 4);
    assert_eq!(txns(&server), after2);
    assert_eq!(mock.cursors()[4..], [Some("c1".to_string()), Some("c2".to_string())]);
    rt.block_on(server.stop());
}

/// A restart asks from the cursor the last run kept and answers no ask it
/// answered before; a run said again from an older cursor — the import
/// in, the cursor never kept — changes nothing. Falsified by loading no
/// store at start: the restart had no bank to import.
#[test]
fn a_restart_resumes_from_the_cursor_and_a_run_said_twice_changes_nothing() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    connect(data.path(), &[("chase", TOKEN, None)]);
    let server = importing(&rt, data.path(), &mock, secrets.path());
    runs(&server, 1);
    let mut alice = caught_up(&server, "alice");
    open_both(&mut alice);
    runs(&server, 2);
    let accounts: Vec<AccountLine> = hub_query(&server, "accounts.accounts", ());
    assert_eq!(
        accounts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        ["Plaid Checking", "Plaid Savings"]
    );
    mock.phase(2);
    alice.mutate("banks.sync", &());
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    runs(&server, 3);
    let after2 = txns(&server);
    rt.block_on(server.stop());

    let server = importing(&rt, data.path(), &mock, secrets.path());
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
    let mut s = store(data.path());
    s.banks.get_mut("chase").unwrap().cursor = Some("c1".into());
    s.save(data.path()).unwrap();
    let server = importing(&rt, data.path(), &mock, secrets.path());
    let r = runs(&server, 1);
    assert_eq!(r[0].banks["chase"], Outcome::Imported { upserts: 2, removed: 1 });
    assert_eq!(txns(&server), after2, "the same run twice is the same budget");
    assert_eq!(cursor(data.path()).as_deref(), Some("c2"));
    rt.block_on(server.stop());
}

/// A run that fails partway — Plaid's second page an error — imports
/// nothing and keeps no cursor, says so beside the bank, and the next run
/// asks again from the start. Falsified by keeping `next_cursor` page by
/// page: the cursor was `p1a`.
#[test]
fn a_failed_run_keeps_the_cursor() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    mock.phase(4);
    connect(data.path(), &[("chase", TOKEN, None)]);
    let server = importing(&rt, data.path(), &mock, secrets.path());
    let r = runs(&server, 1);
    assert!(
        matches!(&r[0].banks["chase"], Outcome::Failed(why) if why.contains("INTERNAL_SERVER_ERROR")),
        "{r:?}"
    );
    assert_eq!(cursor(data.path()), None);
    let bank: Vec<BankLine> = hub_query(&server, "banks.banks", ());
    assert_eq!(
        (bank[0].error_code.as_str(), bank[0].synced_ms, bank[0].needs_login),
        ("INTERNAL_SERVER_ERROR", 0, false)
    );

    mock.phase(1);
    server.importer.as_ref().unwrap().nudge();
    let r = runs(&server, 2);
    assert!(matches!(r[1].banks["chase"], Outcome::Imported { .. }));
    assert_eq!(mock.cursors(), [None, Some("p1a".into()), None, Some("p1a".into())]);
    assert_eq!(cursor(data.path()).as_deref(), Some("c1"));
    let bank: Vec<BankLine> = hub_query(&server, "banks.banks", ());
    assert_eq!(bank[0].error_code, "", "an import that works clears it");
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
    connect(data.path(), &[("chase", TOKEN, None), ("amex", "access-wrong-31e8", None)]);
    let server = importing(&rt, data.path(), &mock, secrets.path());
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
    assert!(mock.asked("/transactions/sync").iter().all(|b| b["access_token"] == TOKEN));
    // Nor is one in the log.
    assert!(!leaked(&server, "access-"), "a token is in the log");
    rt.block_on(server.stop());
}
