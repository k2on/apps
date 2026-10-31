//! Banks connected from the app, against the scripted Plaid's Hosted
//! Link: a connection asked for on the socket and its page's address
//! answered to the asker, the page finished, the login kept in the data
//! directory and the bank imported at once; a login given again; a bank
//! disconnected; a store written by hand, as an existing connection is
//! moved in; a backup of the data directory restored on a fresh server;
//! and the login never reaching a device, a view or a line said.

mod common;

use common::*;
use money_domain::*;
use money_server::importer::Outcome;

/// The asker's asks, as their device reads them.
fn asks(p: &Peer) -> Vec<AskLine> {
    p.query("banks.asks", &()).unwrap()
}

/// Pump `p` until its newest ask stands as `state`.
fn until_ask(p: &mut Peer, state: AskState) -> AskLine {
    pump_until(&mut [p], 15, |ps| asks(ps[0]).first().is_some_and(|a| a.state == state));
    asks(p).remove(0)
}

fn wells_rows(server: &money_server::Server) -> Vec<LinkedLine> {
    let all: Vec<LinkedLine> = hub_query(server, "banks.linked", ());
    all.into_iter().filter(|l| l.bank == "wells-fargo").collect()
}

/// Alice asks to connect a bank; the importer opens a Hosted Link page for
/// a new bank's transactions and answers her, and her alone, with its
/// address. Once the page is finished at Plaid the importer trades the
/// public token, keeps the login in `banks.json` (0600, nowhere else),
/// names the bank for its institution, answers done, and imports it at
/// once: its accounts wait for a choice. Alice opens the card as a credit
/// account and links checking to the one she kept by hand; the next import
/// asks from the start and brings the card's history and the checking
/// account's since the bank was connected. Falsified by answering before
/// the store is saved: a restart in between would lose the login, which
/// the store's check below catches by reading it the moment the ask is
/// done.
#[test]
fn a_bank_is_connected_from_the_app() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    let server = importing(&rt, data.path(), &mock, secrets.path());
    runs(&server, 1);
    let mut alice = caught_up(&server, "alice");
    let kept = OpenAccount {
        name: "Checking".into(),
        kind: "checking".into(),
        on_budget: true,
        balance: 50_000,
        date: "2026-09-01".into(),
    };
    alice.mutate("accounts.open_account", &kept);
    alice.mutate("banks.connect", &());
    let waiting = until_ask(&mut alice, AskState::Waiting);
    assert_eq!(
        (waiting.what.as_str(), waiting.url.as_str()),
        ("connect", "https://hosted.plaid.test/link-1")
    );
    assert!(waiting.expires_ms > 0);
    let made = &mock.asked("/link/token/create")[0];
    assert_eq!(made["products"], serde_json::json!(["transactions"]));
    assert_eq!(made["transactions"]["days_requested"], 90);
    assert!(made["hosted_link"].is_object() && made["access_token"].is_null());
    let mut sam = caught_up(&server, "sam");
    assert!(asks(&sam).is_empty(), "sam sees his own asks only");

    mock.finish();
    let done = until_ask(&mut alice, AskState::Done);
    assert_eq!(
        (done.bank.as_str(), done.institution.as_str(), done.url.as_str()),
        ("wells-fargo", "Wells Fargo", "")
    );
    let kept_now = store(data.path());
    let wf = &kept_now.banks["wells-fargo"];
    assert_eq!(
        (wf.access_token.as_str(), wf.item_id.as_str(), wf.institution.as_str()),
        (WF_TOKEN, "item-wf", "Wells Fargo")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(data.path().join("banks.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let runs_now = runs(&server, 2);
    assert_eq!(runs_now[1].banks["wells-fargo"], Outcome::Imported { upserts: 2, removed: 0 });
    assert_eq!(store(data.path()).banks["wells-fargo"].cursor.as_deref(), Some("w1"));
    let rows = wells_rows(&server);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|l| l.waiting));
    let card = rows.iter().find(|l| l.plaid_account_id == "wf-card").unwrap();
    assert_eq!(card.opens_as, "credit");

    // Alice says what each feeds.
    pump_until(&mut [&mut alice], 10, |ps| ps[0].cursor() == head(&server));
    let checking = hub_rows::<Account>(&server, "account")
        .into_iter()
        .find(|a| a.name == "Checking")
        .unwrap()
        .id;
    alice.mutate(
        "banks.open",
        &OpenLinked {
            plaid_account_id: "wf-card".into(),
            name: "Visa".into(),
        },
    );
    alice.mutate(
        "banks.link",
        &LinkAccount {
            plaid_account_id: "wf-chk".into(),
            account_id: checking,
        },
    );
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    assert!(eventually(20, || hub_rows::<Txn>(&server, "txn").iter().any(|t| t.payee == "WC1 STORE")));
    let accounts: Vec<AccountLine> = hub_query(&server, "accounts.accounts", ());
    let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
    assert_eq!((visa.kind.as_str(), visa.balance, visa.bank.as_str()), ("credit", -30_000, "wells-fargo"));
    let checking = accounts.iter().find(|a| a.name == "Checking").unwrap();
    assert_eq!(
        checking.balance, 50_000,
        "its history is from before the bank was connected: kept by hand already"
    );
    let banks: Vec<BankLine> = hub_query(&server, "banks.banks", ());
    assert_eq!((banks[0].institution.as_str(), banks[0].waiting), ("Wells Fargo", 0));

    // The login went to Plaid alone: in no row, no view, no line said.
    for p in [&alice, &sam] {
        assert!(!in_rows(&p.client.log.store, WF_TOKEN), "a device holds the token");
    }
    assert!(!leaked(&server, WF_TOKEN) && !leaked(&server, "public-"), "the log holds the token");
    let said = server.importer.as_ref().unwrap().said().join("\n");
    assert!(!said.contains(WF_TOKEN) && !said.contains("public-1"), "{said}");
    sam.pump();
    rt.block_on(server.stop());
}

/// A page closed without a bank connected is a failed ask, with Plaid's
/// sentence; one nobody finishes in time expires.
#[test]
fn a_page_given_up_on_fails_and_one_left_open_expires() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    let b = money_server::importer::Banks {
        lifetime_s: 2,
        ..banks(&mock, secrets.path())
    };
    let server = serve(&rt, data.path(), |c| c.banks = Some(b));
    let mut alice = caught_up(&server, "alice");
    alice.mutate("banks.connect", &());
    until_ask(&mut alice, AskState::Waiting);
    mock.give_up();
    let failed = until_ask(&mut alice, AskState::Failed);
    assert!(failed.message.contains("Wells Fargo is not answering"), "{}", failed.message);
    assert!(store(data.path()).banks.is_empty());

    alice.mutate("banks.connect", &());
    pump_until(&mut [&mut alice], 10, |ps| asks(ps[0]).len() == 2);
    let expired = until_ask(&mut alice, AskState::Expired);
    assert!(expired.message.contains("in time"), "{}", expired.message);
    rt.block_on(server.stop());
}

/// A bank whose login ran out says so beside the bank; Reconnect opens
/// Plaid's update mode for its login (its token, no products), and once
/// the page is finished the bank imports again at once and the failure is
/// gone. Falsified by opening a new bank's page for a reconnect: Plaid's
/// script refused products beside an access token.
#[test]
fn a_bank_is_logged_in_to_again() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    connect(data.path(), &[("wells", WF_TOKEN, Some("w1"))]);
    mock.login_required(WF_TOKEN);
    let server = importing(&rt, data.path(), &mock, secrets.path());
    let r = runs(&server, 1);
    assert!(matches!(&r[0].banks["wells"], Outcome::Failed(why) if why.contains("ITEM_LOGIN_REQUIRED")));
    let mut alice = caught_up(&server, "alice");
    pump_until(&mut [&mut alice], 10, |ps| {
        ps[0]
            .query::<Vec<BankLine>>("banks.banks", &())
            .unwrap()
            .first()
            .is_some_and(|b| b.needs_login)
    });
    let said = server.importer.as_ref().unwrap().said().join("\n");
    assert!(said.contains("Reconnect it from the app"), "{said}");

    alice.mutate("banks.reconnect", &BankName { bank: "wells".into() });
    let waiting = until_ask(&mut alice, AskState::Waiting);
    assert_eq!((waiting.what.as_str(), waiting.bank.as_str()), ("reconnect", "wells"));
    let made = &mock.asked("/link/token/create")[0];
    assert_eq!(made["access_token"], WF_TOKEN);
    assert!(made["products"].is_null());
    mock.finish();
    until_ask(&mut alice, AskState::Done);
    let r = runs(&server, 2);
    assert_eq!(r[1].banks["wells"], Outcome::Imported { upserts: 0, removed: 0 });
    pump_until(&mut [&mut alice], 10, |ps| {
        ps[0]
            .query::<Vec<BankLine>>("banks.banks", &())
            .unwrap()
            .first()
            .is_some_and(|b| !b.needs_login && b.error.is_empty())
    });
    assert_eq!(mock.cursors(), [Some("w1".to_string())], "from the cursor it had");
    rt.block_on(server.stop());
}

/// Disconnecting tells Plaid to forget the login, drops it from the store
/// and forgets the bank here; the account it fed stays with what is in it,
/// and the importer has nothing more to ask.
#[test]
fn a_bank_is_disconnected() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    connect(data.path(), &[("wells", WF_TOKEN, None)]);
    let server = importing(&rt, data.path(), &mock, secrets.path());
    runs(&server, 1);
    let mut alice = caught_up(&server, "alice");
    let open = OpenLinked {
        plaid_account_id: "wf-card".into(),
        name: "Visa".into(),
    };
    alice.mutate("banks.open", &open);
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    runs(&server, 2);

    alice.mutate("banks.disconnect", &BankName { bank: "wells".into() });
    until_ask(&mut alice, AskState::Done);
    assert_eq!(mock.asked("/item/remove")[0]["access_token"], WF_TOKEN);
    assert!(store(data.path()).banks.is_empty());
    assert!(hub_rows::<Bank>(&server, "bank").is_empty() && hub_rows::<Linked>(&server, "linked").is_empty());
    let accounts: Vec<AccountLine> = hub_query(&server, "accounts.accounts", ());
    let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
    assert_eq!((visa.balance, visa.bank.as_str()), (-30_000, ""));
    server.importer.as_ref().unwrap().nudge();
    let r = runs(&server, 3);
    assert!(r[2].banks.is_empty(), "nothing left to import");
    rt.block_on(server.stop());
}

/// An existing connection moved in by hand: `banks.json` with the token
/// and the cursor alone, under the name the budget knows the bank by. A
/// fresh server imports only what is new since that cursor, fills in what
/// Plaid says of the connection, and doubles nothing. The cursor may stay
/// in `importer.json`, where the importer kept it before, too. Falsified
/// by ignoring the cursor of a connection written by hand: the import
/// asked from the start.
#[test]
fn a_connection_written_by_hand_imports_only_what_is_new() {
    let rt = runtime();
    let (data, secrets) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    // The budget as it stands: the bank imported once, its card followed.
    connect(data.path(), &[("wellsfargo", WF_TOKEN, None)]);
    let server = importing(&rt, data.path(), &mock, secrets.path());
    runs(&server, 1);
    let mut alice = caught_up(&server, "alice");
    alice.mutate(
        "banks.open",
        &OpenLinked {
            plaid_account_id: "wf-card".into(),
            name: "Visa".into(),
        },
    );
    pump_until(&mut [&mut alice], 10, |ps| ps[0].pending() == 0);
    runs(&server, 2);
    let before = hub_rows::<Txn>(&server, "txn").len();
    rt.block_on(server.stop());

    // Moved in by hand: a token and a cursor, nothing else.
    std::fs::write(
        data.path().join("banks.json"),
        format!(r#"{{"banks": {{"wellsfargo": {{"access_token": "{WF_TOKEN}", "cursor": "w1"}}}}}}"#),
    )
    .unwrap();
    mock.script.lock().unwrap().wf_more = true;
    let asked_before = mock.cursors().len();
    let server = importing(&rt, data.path(), &mock, secrets.path());
    let r = runs(&server, 1);
    assert_eq!(r[0].banks["wellsfargo"], Outcome::Imported { upserts: 1, removed: 0 });
    assert_eq!(mock.cursors()[asked_before..], [Some("w1".to_string())], "only what is new");
    let txns: Vec<Txn> = hub_rows(&server, "txn");
    assert_eq!(txns.len(), before + 1);
    assert_eq!(txns.iter().filter(|t| t.imported_id.as_deref() == Some("WC1")).count(), 1);
    let kept = store(data.path());
    let wf = &kept.banks["wellsfargo"];
    assert_eq!(
        (wf.cursor.as_deref(), wf.item_id.as_str(), wf.institution.as_str()),
        (Some("w2"), "item-wf", "Wells Fargo")
    );
    rt.block_on(server.stop());

    // The cursor where the importer kept it before: importer.json.
    std::fs::write(
        data.path().join("banks.json"),
        format!(r#"{{"banks": {{"wellsfargo": {{"access_token": "{WF_TOKEN}"}}}}}}"#),
    )
    .unwrap();
    std::fs::write(
        data.path().join("importer.json"),
        r#"{"cursors": {"wellsfargo": "w2"}, "answered_ms": 0}"#,
    )
    .unwrap();
    let asked_before = mock.cursors().len();
    let server = importing(&rt, data.path(), &mock, secrets.path());
    runs(&server, 1);
    assert_eq!(mock.cursors()[asked_before..], [Some("w2".to_string())]);
    assert_eq!(hub_rows::<Txn>(&server, "txn").len(), before + 1);
    rt.block_on(server.stop());
}

/// A backup restored: the data directory copied whole to another machine
/// (another directory) and a fresh server started on it, told nothing but
/// Plaid's keys. Every bank is still connected and imports from where it
/// was. Falsified by keeping the connections anywhere but the data
/// directory.
#[test]
fn a_restored_backup_keeps_every_connection() {
    let rt = runtime();
    let (data, secrets, elsewhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = plaid(&rt);
    let server = importing(&rt, data.path(), &mock, secrets.path());
    let mut alice = caught_up(&server, "alice");
    alice.mutate("banks.connect", &());
    until_ask(&mut alice, AskState::Waiting);
    mock.finish();
    until_ask(&mut alice, AskState::Done);
    runs(&server, 2);
    rt.block_on(server.stop());

    let restored = elsewhere.path().join("money");
    copy_dir(data.path(), &restored);
    mock.script.lock().unwrap().wf_more = true;
    let asked_before = mock.cursors().len();
    let server = importing(&rt, &restored, &mock, secrets.path());
    let r = runs(&server, 1);
    assert_eq!(r[0].banks["wells-fargo"], Outcome::Imported { upserts: 1, removed: 0 });
    assert_eq!(mock.cursors()[asked_before..], [Some("w1".to_string())]);
    let banks: Vec<BankLine> = hub_query(&server, "banks.banks", ());
    assert_eq!((banks.len(), banks[0].institution.as_str()), (1, "Wells Fargo"));
    rt.block_on(server.stop());
}
