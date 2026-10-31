//! A second login to a bank already here: connected again from the app
//! (for an account the first login did not share), folded into the first
//! at its first import — each account takes its twin's choice, the new
//! one is opened, the first login is retired — and nothing imported twice
//! across the seam, whether the fold made it or somebody linked a waiting
//! account to an account another login feeds by hand. And accounts added
//! to a connected bank through Plaid's account picker.

mod common;

use common::Book;
use money_domain::dates::add_days;
use money_domain::*;

/// The book's today.
const TODAY: &str = "2026-10-26";
const INSTITUTION: &str = "Granite Bank";
const INSTITUTION_ID: &str = "ins_granite";

fn account(id: &str, name: &str, mask: &str, kind: &str, subtype: &str, balance: i64) -> BankAccount {
    BankAccount {
        plaid_account_id: id.into(),
        name: name.into(),
        mask: mask.into(),
        kind: kind.into(),
        subtype: subtype.into(),
        balance: Some(balance),
        called: String::new(),
        ignored: false,
    }
}

fn txn(id: &str, account: &str, date: &str, payee: &str, amount: i64, pending: bool) -> BankTxn {
    BankTxn {
        transaction_id: id.into(),
        plaid_account_id: account.into(),
        pending_transaction_id: None,
        date: date.into(),
        amount,
        payee: payee.into(),
        imported_payee: payee.to_uppercase(),
        pending,
    }
}

/// One login's accounts at Granite Bank, its account ids prefixed: checking
/// (1111), a card (2222) and a savings account (3333) — and, for the second
/// login, the savings account (4444) the first did not share.
fn accounts(login: &str, second: bool) -> Vec<BankAccount> {
    let mut a = vec![
        account(&format!("{login}-chk"), "Everyday Checking", "1111", "depository", "checking", 250_000),
        account(&format!("{login}-card"), "Cash Card", "2222", "credit", "credit card", 30_000),
        account(&format!("{login}-sav"), "Old Savings", "3333", "depository", "savings", 100),
    ];
    if second {
        a.push(account(&format!("{login}-way"), "Way Savings", "4444", "depository", "savings", 512_000));
    }
    a
}

/// Ninety days of checking at the bank, as `login` tells of them: one
/// every three days, today's among them, and a charge two days ago still
/// pending — or, for a login that sees it later, posted yesterday. The
/// card's spending, which nobody follows, beside it.
fn checking(login: &str, posted: bool) -> Vec<BankTxn> {
    let chk = format!("{login}-chk");
    let mut t: Vec<BankTxn> = (0..30)
        .map(|n| txn(&format!("{login}-k{n}"), &chk, &add_days(TODAY, -3 * n), &format!("Shop {n}"), -(1_000 + 41 * n), false))
        .collect();
    t.push(match posted {
        false => txn(&format!("{login}-pend"), &chk, &add_days(TODAY, -2), "Diner", -1_999, true),
        true => BankTxn {
            pending_transaction_id: Some(format!("{login}-pend")),
            ..txn(&format!("{login}-post"), &chk, &add_days(TODAY, -1), "Diner", -1_999, false)
        },
    });
    t.push(txn(&format!("{login}-c1"), &format!("{login}-card"), &add_days(TODAY, -5), "Fuel", -4_000, false));
    t
}

/// The savings account the second login shares: interest and a deposit.
fn savings(login: &str) -> Vec<BankTxn> {
    let way = format!("{login}-way");
    vec![
        txn(&format!("{login}-s1"), &way, &add_days(TODAY, -60), "Interest", 1_200, false),
        txn(&format!("{login}-s2"), &way, &add_days(TODAY, -40), "Deposit", 100_000, false),
        txn(&format!("{login}-s3"), &way, &add_days(TODAY, -30), "Interest", 1_300, false),
    ]
}

fn import(bank: &str, accounts: Vec<BankAccount>, upserts: Vec<BankTxn>, from_start: bool) -> Import {
    Import {
        bank: bank.into(),
        institution: INSTITUTION.into(),
        institution_id: INSTITUTION_ID.into(),
        cursor: format!("{bank}-c"),
        accounts,
        upserts,
        removed: vec![],
        choose: true,
        from_start,
    }
}

fn linked(b: &Book, id: &str) -> Linked {
    b.rows::<Linked>("linked").into_iter().find(|l| l.plaid_account_id == id).expect(id)
}

fn bank_row(b: &Book, name: &str) -> Option<Bank> {
    b.rows::<Bank>("bank").into_iter().find(|r| r.name == name)
}

/// The first login, `granite`, as a household had it: checking opened as
/// an account of its own with its whole history, the card and the old
/// savings ignored. The account checking feeds.
fn first_login(b: &mut Book) -> Id<Account> {
    b.import(&import("granite", accounts("granite", false), checking("granite", false), true)).unwrap();
    b.run(
        "open",
        &OpenLinked {
            plaid_account_id: "granite-chk".into(),
            name: "Checking".into(),
        },
    )
    .unwrap();
    for id in ["granite-card", "granite-sav"] {
        b.run(
            "ignore",
            &IgnoreLinked {
                plaid_account_id: id.into(),
                ignored: true,
            },
        )
        .unwrap();
    }
    b.import(&import("granite", accounts("granite", false), checking("granite", false), true)).unwrap();
    let id = b.accounts().into_iter().find(|a| a.name == "Checking").unwrap().id;
    assert_eq!(b.register(Some(id)).len(), 32, "its history, the pending charge, and a starting balance");
    assert_eq!(b.accounts().into_iter().find(|a| a.id == id).unwrap().balance, 248_001, "the bank's balance and the charge pending");
    id
}

/// `banks.connect` asked and answered, as the importer does once Plaid's
/// page is finished: the bank known here, its first import still to come.
fn connected(b: &mut Book, name: &str) {
    b.run("connect", &()).unwrap();
    let ask = b.rows::<BankAsk>("bank_ask").into_iter().max_by_key(|a| a.asked_ms).unwrap();
    let ctx = b.ctx("bank", &[BANK]);
    let done = Answer {
        id: ask.id,
        state: AskState::Done,
        bank: name.into(),
        institution: INSTITUTION.into(),
        ..Answer::default()
    };
    b.apply(&ctx, "answer", &done).unwrap();
}

/// What an account holds, as (payee, amount, date) sorted: a double shows.
fn held(b: &Book, account: Id<Account>) -> Vec<(String, i64, String)> {
    let mut t: Vec<_> = b.register(Some(account)).into_iter().map(|t| (t.payee, t.amount, t.date)).collect();
    t.sort();
    t
}

/// A second login to the same bank, connected from the app for the savings
/// account the first did not share, with ninety days of history that
/// overlap the first's: at its first import checking feeds the account the
/// first login's checking feeds, from the day it was first seen, with
/// today's transaction and the charge the first had pending (posted now)
/// matched rather than doubled; the card and the old savings are ignored
/// as the first login had them; the new savings account is opened as
/// savings with its whole history and a starting balance; and the first
/// login, every choice of which the second now carries, is retired — the
/// importer removes it and the budget forgets it, its account and what is
/// in it staying. Falsified by inserting a since-day import that another
/// login imported already: today's transaction was there twice.
#[test]
fn a_second_login_to_the_same_bank_folds_into_the_first() {
    let mut b = Book::new();
    let kept = first_login(&mut b);
    let before = held(&b, kept);

    connected(&mut b, "granite-2");
    assert!(bank_row(&b, "granite-2").unwrap().fresh);
    let mut history = checking("granite-2", true);
    history.extend(savings("granite-2"));
    b.import(&import("granite-2", accounts("granite-2", true), history.clone(), true)).unwrap();

    let chk = linked(&b, "granite-2-chk");
    assert_eq!((chk.account_id, chk.since.as_str(), chk.seam), (Some(kept), TODAY, true));
    assert!(linked(&b, "granite-2-card").ignored && linked(&b, "granite-2-sav").ignored);
    let way = linked(&b, "granite-2-way");
    let opened = b.accounts().into_iter().find(|a| Some(a.id) == way.account_id).expect("opened for it");
    assert_eq!((opened.name.as_str(), opened.kind.as_str(), opened.on_budget), ("Way Savings", "savings", true));
    assert_eq!(opened.balance, 512_000, "the bank's balance");
    let mut way_held = held(&b, opened.id);
    way_held.retain(|t| t.0 != "Starting Balance");
    assert_eq!(way_held.len(), 3, "its whole history");
    let start = b.register(Some(opened.id)).into_iter().find(|t| t.payee == "Starting Balance").unwrap();
    assert_eq!((start.amount, start.date.as_str()), (512_000 - 102_500, add_days(TODAY, -61).as_str()));

    // Checking: nothing twice. Today's is the second login's now, and the
    // pending charge posted through it, where it was.
    let after = held(&b, kept);
    let posted: Vec<(String, i64)> = before.iter().map(|t| (t.0.clone(), t.1)).collect();
    assert_eq!(after.iter().map(|t| (t.0.clone(), t.1)).collect::<Vec<_>>(), posted);
    let txns: Vec<Txn> = b.rows("txn");
    let diner = txns.iter().find(|t| t.payee == "Diner").unwrap();
    assert_eq!(
        (diner.imported_id.as_deref(), diner.imported_by.as_str(), diner.cleared, diner.date.as_str()),
        (Some("granite-2-post"), "granite-2", Cleared::Cleared, add_days(TODAY, -1).as_str())
    );
    let today = txns.iter().find(|t| t.payee == "Shop 0").unwrap();
    assert_eq!(today.imported_id.as_deref(), Some("granite-2-k0"));
    let older = txns.iter().find(|t| t.payee == "Shop 5").unwrap();
    assert_eq!(older.imported_id.as_deref(), Some("granite-k5"), "before the second login was seen: the first's");
    assert_eq!(b.accounts().into_iter().find(|a| a.id == kept).unwrap().balance, 248_001, "the bank's balance and the charge pending");

    // The first login is replaced: what it imports now changes nothing.
    assert_eq!(bank_row(&b, "granite").unwrap().replaced_by, "granite-2");
    assert!(!bank_row(&b, "granite-2").unwrap().fresh);
    let mut more = checking("granite", true);
    more.push(txn("granite-late", "granite-chk", TODAY, "Late", -777, false));
    b.import(&import("granite", accounts("granite", false), more, false)).unwrap();
    assert_eq!(held(&b, kept), after);

    // The importer removes it at Plaid and says so: forgotten here.
    assert_eq!(
        b.run("retired", &BankName { bank: "granite".into() }).unwrap_err(),
        "only the bank importer retires a login"
    );
    let ctx = b.ctx("bank", &[BANK]);
    b.apply(&ctx, "retired", &BankName { bank: "granite".into() }).unwrap();
    assert!(bank_row(&b, "granite").is_none());
    assert!(b.rows::<Linked>("linked").iter().all(|l| l.bank == "granite-2"));
    let line = b.accounts().into_iter().find(|a| a.id == kept).unwrap();
    assert_eq!((line.bank.as_str(), line.balance), ("granite-2", 248_001));

    // The second login's history again, and what is new: nothing doubles.
    b.import(&import("granite-2", accounts("granite-2", true), history.clone(), true)).unwrap();
    assert_eq!(held(&b, kept), after);
    let mut new = history;
    new.push(txn("granite-2-new", "granite-2-chk", TODAY, "Grocer", -5_500, false));
    b.import(&import("granite-2", accounts("granite-2", true), new, false)).unwrap();
    assert_eq!(held(&b, kept).len(), after.len() + 1);
}

/// The same choices made by hand, for a second login that was waiting
/// already (connected before folding was): nothing folds by itself; the
/// household links its checking to the account the first login feeds
/// (allowed: another login's), opens the savings account as new, ignores
/// the rest, and disconnects the first login. While both feed checking,
/// neither doubles what the other imported; after, nothing is lost.
/// Falsified by matching only one way: the first login's import of a
/// transaction the second had imported was there twice.
#[test]
fn a_waiting_second_login_is_mended_by_hand() {
    let mut b = Book::new();
    let kept = first_login(&mut b);
    // Known by its import alone, as one connected before this was: no fold.
    let mut history = checking("granite-2", true);
    history.extend(savings("granite-2"));
    b.import(&import("granite-2", accounts("granite-2", true), history.clone(), true)).unwrap();
    assert!(b.rows::<Linked>("linked").iter().filter(|l| l.bank == "granite-2").all(|l| l.account_id.is_none() && !l.ignored));
    assert!(bank_row(&b, "granite").unwrap().replaced_by.is_empty());
    let before = held(&b, kept);

    b.run(
        "link",
        &LinkAccount {
            plaid_account_id: "granite-2-chk".into(),
            account_id: kept,
        },
    )
    .unwrap();
    assert!(linked(&b, "granite-2-chk").seam);
    b.run(
        "open",
        &OpenLinked {
            plaid_account_id: "granite-2-way".into(),
            name: "".into(),
        },
    )
    .unwrap();
    for id in ["granite-2-card", "granite-2-sav"] {
        b.run(
            "ignore",
            &IgnoreLinked {
                plaid_account_id: id.into(),
                ignored: true,
            },
        )
        .unwrap();
    }
    // Two of one login's accounts never feed one account.
    let other = b.accounts().into_iter().find(|a| a.name == "Way Savings").unwrap().id;
    assert_eq!(
        b.run(
            "link",
            &LinkAccount {
                plaid_account_id: "granite-2-sav".into(),
                account_id: other,
            },
        )
        .unwrap_err(),
        "that account is fed by another of this bank's accounts already"
    );

    // The history again, from the start, as the choice asked.
    b.import(&import("granite-2", accounts("granite-2", true), history.clone(), true)).unwrap();
    let after = held(&b, kept);
    let amounts = |h: &[(String, i64, String)]| h.iter().map(|t| (t.0.clone(), t.1)).collect::<Vec<_>>();
    assert_eq!(amounts(&after), amounts(&before), "nothing twice");
    let way = b.accounts().into_iter().find(|a| a.id == other).unwrap();
    assert_eq!((way.kind.as_str(), way.balance), ("savings", 512_000));
    assert_eq!(b.register(Some(other)).len(), 4, "its history and a starting balance");

    // Both feed checking for now: each one's copy of the other's is matched.
    let mut second = history.clone();
    second.push(txn("granite-2-new", "granite-2-chk", TODAY, "Grocer", -5_500, false));
    b.import(&import("granite-2", accounts("granite-2", true), second.clone(), false)).unwrap();
    let mut first = checking("granite", true);
    first.push(txn("granite-new", "granite-chk", TODAY, "Grocer", -5_500, false));
    b.import(&import("granite", accounts("granite", false), first, false)).unwrap();
    assert_eq!(held(&b, kept).len(), after.len() + 1, "the grocer once");

    // The first login disconnected: what it fed stays, the second feeds it.
    b.run("disconnect", &BankName { bank: "granite".into() }).unwrap();
    let ask = b.rows::<BankAsk>("bank_ask").into_iter().max_by_key(|a| a.asked_ms).unwrap();
    let ctx = b.ctx("bank", &[BANK]);
    b.apply(
        &ctx,
        "answer",
        &Answer {
            id: ask.id,
            state: AskState::Done,
            ..Answer::default()
        },
    )
    .unwrap();
    b.import(&import("granite-2", accounts("granite-2", true), second, true)).unwrap();
    assert_eq!(held(&b, kept).len(), after.len() + 1);
    assert_eq!(b.accounts().into_iter().find(|a| a.id == kept).unwrap().bank, "granite-2");
}

/// A login connected for another institution, with the same last digits,
/// folds into nothing; nor does a second login to the same bank none of
/// whose accounts has a twin. Their accounts wait, and the first login is
/// left as it was.
#[test]
fn nothing_folds_across_banks_or_without_a_twin() {
    let mut b = Book::new();
    let kept = first_login(&mut b);
    let before = held(&b, kept);

    connected(&mut b, "other");
    let elsewhere = Import {
        institution: "Other Bank".into(),
        institution_id: "ins_other".into(),
        ..import("other", accounts("other", true), checking("other", true), true)
    };
    b.import(&elsewhere).unwrap();

    connected(&mut b, "granite-2");
    let unlike = vec![account("granite-2-x", "Brokerage", "9999", "investment", "brokerage", 10)];
    b.import(&import("granite-2", unlike, vec![], true)).unwrap();

    let all: Vec<Linked> = b.rows("linked");
    assert!(all.iter().filter(|l| l.bank != "granite").all(|l| l.account_id.is_none() && !l.ignored));
    assert!(bank_row(&b, "granite").unwrap().replaced_by.is_empty());
    assert_eq!(held(&b, kept), before);
    assert_eq!(b.accounts().len(), 1);
}

/// Accounts added to a connected bank through Plaid's account picker
/// (`banks.add_accounts`, answered once the page is finished) are opened
/// at the next import, each as an account of its kind with its history and
/// a starting balance; an account appearing at an import nobody asked for
/// waits as before. Falsified by `adding` kept after the import: the
/// account that appeared later opened by itself.
#[test]
fn accounts_added_at_plaid_are_opened() {
    let mut b = Book::new();
    let kept = first_login(&mut b);
    b.run("add_accounts", &BankName { bank: "granite".into() }).unwrap();
    let ask = b.rows::<BankAsk>("bank_ask").into_iter().max_by_key(|a| a.asked_ms).unwrap();
    assert_eq!((ask.what.as_str(), ask.bank.as_str()), ("add_accounts", "granite"));
    let ctx = b.ctx("bank", &[BANK]);
    b.apply(
        &ctx,
        "answer",
        &Answer {
            id: ask.id,
            state: AskState::Done,
            ..Answer::default()
        },
    )
    .unwrap();
    assert!(bank_row(&b, "granite").unwrap().adding);

    // The import after: the savings account is new, its history comes from
    // the cursor; it opens and asks for the bank's history from the start.
    let mut new = accounts("granite", true);
    b.import(&import("granite", new.clone(), savings("granite"), false)).unwrap();
    let way = linked(&b, "granite-way");
    let id = way.account_id.expect("opened");
    assert!(way.opening && bank_row(&b, "granite").unwrap().repull);
    let mut history = checking("granite", false);
    history.extend(savings("granite"));
    b.import(&import("granite", new.clone(), history.clone(), true)).unwrap();
    let line = b.accounts().into_iter().find(|a| a.id == id).unwrap();
    assert_eq!((line.kind.as_str(), line.balance), ("savings", 512_000));
    assert_eq!(b.register(Some(id)).len(), 4);
    assert_eq!(b.register(Some(kept)).len(), 32, "checking as it was");

    // One more account, nobody having asked: it waits.
    new.push(account("granite-cd", "Certificate", "5555", "depository", "cd", 1_000));
    b.import(&import("granite", new, history, false)).unwrap();
    let cd = linked(&b, "granite-cd");
    assert!(cd.account_id.is_none() && !cd.ignored);

    assert_eq!(
        b.run("add_accounts", &BankName { bank: "nope".into() }).unwrap_err(),
        "no bank of that name is connected"
    );
}
