//! The Plaid import, walked through the three phases ArkOS's end-to-end
//! check walked Actual through (`sync/actual-import/src/test/`): an initial
//! pull with a pending transaction and one typed in by hand first, that
//! transaction posting with another amount while another is modified, and
//! nothing new. The Plaid answers are the mock's, as JSON.

mod common;

use common::Book;
use money_domain::plaid::{import_of, PlaidAccount, Pulled};
use money_domain::*;

fn accounts() -> Vec<PlaidAccount> {
    serde_json::from_str(
        r#"[
        {"account_id":"acc1","name":"Plaid Checking","mask":"0000","type":"depository","subtype":"checking","balances":{"current":923.5}},
        {"account_id":"acc2","name":"Plaid Savings","mask":"1111","type":"depository","subtype":"savings","balances":{"current":923.5}}
    ]"#,
    )
    .unwrap()
}

fn txn(id: &str, amount: f64, name: &str, extra: &str) -> String {
    let mut t: serde_json::Value = serde_json::from_str(&format!(
        r#"{{"transaction_id":"{id}","account_id":"acc1","amount":{amount},"date":"2026-09-20","name":"{name}","merchant_name":null,
            "original_description":"{name}","pending":false,"pending_transaction_id":null}}"#
    ))
    .unwrap();
    let extra: serde_json::Value = serde_json::from_str(&format!("{{{}}}", extra.trim_start_matches(','))).unwrap();
    for (k, v) in extra.as_object().unwrap() {
        t[k] = v.clone();
    }
    t.to_string()
}

fn phase(n: u8) -> Pulled {
    let t1 = txn("T1", 50.25, "COFFEE SHOP #12", r#","merchant_name":"Coffee Shop""#);
    let p1 = txn("P1", 20.0, "GAS STATION PENDING", r#","pending":true,"date":"2026-09-21""#);
    let t2 = txn("T2", 21.5, "GAS STATION", r#","pending_transaction_id":"P1","date":"2026-09-22""#);
    let t3 = txn("T3", -1000.0, "PAYROLL", r#","date":"2026-09-23""#);
    let s1 = txn("S1", -10.0, "INTEREST", r#","account_id":"acc2""#);
    let t1m = txn("T1", 55.0, "COFFEE SHOP #12", r#","merchant_name":"Coffee Shop""#);
    let json = match n {
        1 => format!(r#"{{"added":[{t1},{s1},{p1},{t3}],"modified":[],"removed":[],"cursor":"c1"}}"#),
        2 => format!(r#"{{"added":[{t2}],"modified":[{t1m}],"removed":[{{"transaction_id":"P1","account_id":"acc1"}}],"cursor":"c2"}}"#),
        _ => r#"{"added":[],"modified":[],"removed":[],"cursor":"c2"}"#.to_string(),
    };
    serde_json::from_str(&json).unwrap()
}

/// A Checking account opened by hand, with the payroll typed in before the
/// bank saw it, and linked to Plaid's acc1 the way the first import of a
/// known account would be: its link row says so.
fn budget() -> (Book, Id<Account>) {
    let mut b = Book::new();
    let checking = b.account("Checking", "checking", 0);
    let typed = AddTransaction {
        account_id: checking,
        date: "2026-09-23".into(),
        payee: "Employer".into(),
        memo: "manual".into(),
        amount: 100_000,
        ..AddTransaction::default()
    };
    b.run("add_transaction", &typed).unwrap();
    (b, checking)
}

/// A row of a register as the check compares it.
fn row(t: &TxnLine) -> (String, String, i64, Cleared, String, String, String) {
    (
        t.date.clone(),
        t.payee.clone(),
        t.amount,
        t.cleared,
        t.imported_payee.clone(),
        t.category.clone(),
        t.memo.clone(),
    )
}

fn rows(b: &Book, account: Id<Account>) -> Vec<(String, String, i64, Cleared, String, String, String)> {
    let mut r: Vec<_> = b.register(Some(account)).iter().map(row).collect();
    r.sort();
    r
}

fn s(x: &str) -> String {
    x.to_string()
}

#[test]
fn three_phases_of_one_bank() {
    let (mut b, _) = budget();
    let i1 = import_of("chase", "Chase", &accounts(), &phase(1));
    // Only the importer imports.
    assert_eq!(b.run("import", &i1).unwrap_err(), "only the bank importer imports");
    b.import(&i1).unwrap();

    // Both Plaid accounts were new here: each opened under the bank's name,
    // on budget, its starting balance the bank's less what was imported,
    // the day before the oldest of it.
    let lines = b.accounts();
    let names: Vec<&str> = lines.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Checking", "Plaid Checking", "Plaid Savings"]);
    let savings = lines.iter().find(|a| a.name == "Plaid Savings").unwrap();
    assert!(savings.on_budget && savings.bank == "chase" && savings.mask == "1111");
    assert_eq!(
        rows(&b, savings.id),
        [
            (
                s("2026-09-19"),
                s("Starting Balance"),
                91_350,
                Cleared::Cleared,
                s(""),
                s("Ready to Assign"),
                s("Balance before the first import")
            ),
            (
                s("2026-09-20"),
                s("INTEREST"),
                1_000,
                Cleared::Cleared,
                s("INTEREST"),
                s("Needs a category"),
                s("")
            ),
        ]
    );
    assert_eq!((savings.balance, savings.bank_balance), (92_350, Some(92_350)));
    assert_eq!(savings.unapproved, 1);

    let plaid_checking = lines.iter().find(|a| a.name == "Plaid Checking").unwrap().id;
    let after1 = [
        (
            s("2026-09-19"),
            s("Starting Balance"),
            92_350 + 5_025 - 100_000,
            Cleared::Cleared,
            s(""),
            s("Ready to Assign"),
            s("Balance before the first import"),
        ),
        (
            s("2026-09-20"),
            s("Coffee Shop"),
            -5_025,
            Cleared::Cleared,
            s("COFFEE SHOP #12"),
            s("Needs a category"),
            s(""),
        ),
        (
            s("2026-09-21"),
            s("GAS STATION PENDING"),
            -2_000,
            Cleared::Uncleared,
            s("GAS STATION PENDING"),
            s("Needs a category"),
            s(""),
        ),
        (
            s("2026-09-23"),
            s("PAYROLL"),
            100_000,
            Cleared::Cleared,
            s("PAYROLL"),
            s("Needs a category"),
            s(""),
        ),
    ];
    assert_eq!(rows(&b, plaid_checking), after1);

    // Somebody files the pending gas under Gas.
    let gas = {
        let car = b.group("Car");
        b.category(car, "Gas")
    };
    let pending = b.register(Some(plaid_checking)).into_iter().find(|t| t.amount == -2_000).unwrap();
    b.run(
        "categorize",
        &Categorize {
            ids: vec![pending.id],
            category_id: Some(gas),
        },
    )
    .unwrap();

    // Phase 2: P1 posts as T2 with a new amount and date, keeping its
    // category; T1 is modified.
    b.import(&import_of("chase", "Chase", &accounts(), &phase(2))).unwrap();
    let after2 = [
        after1[0].clone(),
        (
            s("2026-09-20"),
            s("Coffee Shop"),
            -5_500,
            Cleared::Cleared,
            s("COFFEE SHOP #12"),
            s("Needs a category"),
            s(""),
        ),
        (
            s("2026-09-22"),
            s("GAS STATION PENDING"),
            -2_150,
            Cleared::Cleared,
            s("GAS STATION"),
            s("Gas"),
            s(""),
        ),
        after1[3].clone(),
    ];
    assert_eq!(rows(&b, plaid_checking), after2);
    let bank: Vec<BankLine> = b.list("banks", &());
    assert_eq!((bank[0].added, bank[0].updated, bank[0].deleted), (0, 2, 0));

    // Phase 3: nothing new, nothing moves; the same import again neither.
    let before = b.store.state_hash();
    b.import(&import_of("chase", "Chase", &accounts(), &phase(3))).unwrap();
    b.import(&import_of("chase", "Chase", &accounts(), &phase(2))).unwrap();
    assert_eq!(rows(&b, plaid_checking), after2);
    assert_ne!(before, b.store.state_hash(), "the bank's row says when it last looked");
}

#[test]
fn a_transaction_typed_in_first_is_matched_not_doubled() {
    let (mut b, checking) = budget();
    // Plaid's acc1 is this Checking: the bank seen once with no history,
    // then its account pointed at the one here.
    let mut i = import_of("chase", "Chase", &accounts()[..1], &Pulled::default());
    i.accounts[0].balance = None;
    b.import(&i).unwrap();
    // The link made a new account; this household would rather it fed the
    // one they already keep.
    let link = LinkAccount {
        plaid_account_id: "acc1".into(),
        account_id: checking,
    };
    b.run("link", &link).unwrap();
    b.import(&import_of("chase", "Chase", &accounts()[..1], &phase(1))).unwrap();
    let mine = b.register(Some(checking));
    let payroll: Vec<&TxnLine> = mine.iter().filter(|t| t.amount == 100_000).collect();
    assert_eq!(payroll.len(), 1, "matched, not doubled");
    let p = payroll[0];
    assert_eq!(
        (p.payee.as_str(), p.memo.as_str(), p.imported_payee.as_str(), p.imported),
        ("Employer", "manual", "PAYROLL", true)
    );
}

#[test]
fn reconciled_is_left_alone_and_ignored_is_dropped() {
    let mut b = Book::new();
    b.import(&import_of("chase", "Chase", &accounts(), &phase(1))).unwrap();
    let checking = b.accounts().into_iter().find(|a| a.name == "Plaid Checking").unwrap();
    let r = Reconcile {
        account_id: checking.id,
        balance: checking.cleared,
        date: "2026-09-30".into(),
    };
    b.run("reconcile", &r).unwrap();
    b.run(
        "ignore",
        &IgnoreLinked {
            plaid_account_id: "acc2".into(),
            ignored: true,
        },
    )
    .unwrap();
    let savings_before = b.register(None).iter().filter(|t| t.account != "Plaid Checking").count();
    b.import(&import_of("chase", "Chase", &accounts(), &phase(2))).unwrap();
    let coffee = b.register(Some(checking.id)).into_iter().find(|t| t.payee == "Coffee Shop").unwrap();
    assert_eq!(coffee.amount, -5_025, "reconciled: the bank's change is not taken");
    let bank: Vec<BankLine> = b.list("banks", &());
    assert_eq!(bank[0].locked, 1);
    assert_eq!(b.register(None).iter().filter(|t| t.account != "Plaid Checking").count(), savings_before);
    // What the household links and ignores from: each of the bank's
    // accounts, the one it feeds, and the one it was told not to follow.
    let linked: Vec<LinkedLine> = b.list("linked", &());
    let of = |id: &str| linked.iter().find(|l| l.plaid_account_id == id).unwrap().clone();
    assert_eq!(
        (of("acc1").account, of("acc1").account_id, of("acc1").ignored),
        ("Plaid Checking".to_string(), Some(checking.id), false)
    );
    assert!(of("acc2").ignored);
}

#[test]
fn a_payee_is_filed_where_it_was_last_time() {
    let mut b = Book::new();
    b.import(&import_of("chase", "Chase", &accounts(), &phase(1))).unwrap();
    let checking = b.accounts().into_iter().find(|a| a.name == "Plaid Checking").unwrap().id;
    let food = b.group("Food");
    let coffee = b.category(food, "Coffee");
    let t = b.register(Some(checking)).into_iter().find(|t| t.payee == "Coffee Shop").unwrap();
    b.run(
        "categorize",
        &Categorize {
            ids: vec![t.id],
            category_id: Some(coffee),
        },
    )
    .unwrap();
    let next: Pulled = serde_json::from_str(&format!(
        r#"{{"added":[{}],"modified":[],"removed":[],"cursor":"c9"}}"#,
        txn("T9", 4.5, "COFFEE SHOP #12", r#","merchant_name":"Coffee Shop","date":"2026-10-02""#)
    ))
    .unwrap();
    b.import(&import_of("chase", "Chase", &accounts(), &next)).unwrap();
    let new = b.register(Some(checking)).into_iter().find(|t| t.date == "2026-10-02").unwrap();
    assert_eq!((new.category.as_str(), new.approved), ("Coffee", false));
    b.run("approve", &Txns { ids: vec![new.id] }).unwrap();
    let inbox: Vec<TxnLine> = b.list("inbox", &());
    assert!(inbox.iter().all(|t| t.id != new.id));
}

/// The server was told what the household called Plaid's acc1 ("Joint
/// Checking", as Actual had it) and nothing of acc2: the first import
/// opens acc1 under that name, knows acc2 and opens nothing for it, and
/// drops its interest. Linking acc2 to an account later follows it from
/// the next import. Falsified by `follow` opening an account whatever
/// `ignored` says: a third account, "Plaid Savings", appeared.
#[test]
fn the_server_names_what_it_follows_and_knows_what_it_does_not() {
    let mut b = Book::new();
    let mut i = import_of("chase", "Chase", &accounts(), &phase(1));
    i.accounts[0].called = "Joint Checking".into();
    i.accounts[1].ignored = true;
    b.import(&i).unwrap();
    let names: Vec<String> = b.accounts().into_iter().map(|a| a.name).collect();
    assert_eq!(names, ["Joint Checking"]);
    let linked: Vec<Linked> = b.rows("linked");
    let savings = linked.iter().find(|l| l.plaid_account_id == "acc2").unwrap();
    assert_eq!((savings.account_id, savings.ignored), (None, true));
    assert!(b.register(None).iter().all(|t| t.payee != "INTEREST"));

    // What the server says is for first sight: a later import that says
    // otherwise changes nothing here.
    let mut again = import_of("chase", "Chase", &accounts(), &phase(2));
    again.accounts[0].called = "Something Else".into();
    b.import(&again).unwrap();
    let names: Vec<String> = b.accounts().into_iter().map(|a| a.name).collect();
    assert_eq!(names, ["Joint Checking"]);

    let jar = b.account("Savings", "savings", 0);
    b.run(
        "link",
        &LinkAccount {
            plaid_account_id: "acc2".into(),
            account_id: jar,
        },
    )
    .unwrap();
    let next: Pulled = serde_json::from_str(&format!(
        r#"{{"added":[{}],"modified":[],"removed":[],"cursor":"c3"}}"#,
        txn("S2", -3.0, "INTEREST", r#","account_id":"acc2","date":"2026-10-01""#)
    ))
    .unwrap();
    b.import(&import_of("chase", "Chase", &accounts(), &next)).unwrap();
    let interest: Vec<TxnLine> = b.register(Some(jar));
    let from_bank: Vec<i64> = interest.iter().filter(|t| t.imported).map(|t| t.amount).collect();
    assert_eq!(from_bank, [300]);
}

/// Anybody may ask the importer to sync; each person's latest ask is one
/// row, and the newest of them is what the importer answers.
#[test]
fn anybody_may_ask_for_a_sync() {
    let mut b = Book::new();
    b.as_("alice", "sync", &()).unwrap();
    b.as_("sam", "sync", &()).unwrap();
    b.as_("alice", "sync", &()).unwrap();
    let asked: Vec<Asked> = b.rows("asked");
    assert_eq!(asked.len(), 2);
    let newest = asked.iter().max_by_key(|a| a.asked_ms).unwrap();
    assert_eq!(newest.user_id, "alice");
}
