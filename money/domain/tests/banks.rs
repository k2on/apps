//! Connecting banks from the app: the asks a household member makes, the
//! importer's answers, and the choice of what each of a new bank's
//! accounts feeds — an account here, one opened for it, or nothing.

mod common;

use common::Book;
use money_domain::plaid::{import_of, PlaidAccount, Pulled};
use money_domain::*;

/// A bank of a checking account and a credit card.
fn accounts() -> Vec<PlaidAccount> {
    serde_json::from_str(
        r#"[
        {"account_id":"chk","name":"Everyday Checking","mask":"1234","type":"depository","subtype":"checking","balances":{"current":500.0}},
        {"account_id":"card","name":"Active Cash Card","mask":"9876","type":"credit","subtype":"credit card","balances":{"current":120.0}}
    ]"#,
    )
    .unwrap()
}

fn txn(id: &str, account: &str, amount: f64, date: &str) -> String {
    format!(
        r#"{{"transaction_id":"{id}","account_id":"{account}","amount":{amount},"date":"{date}","name":"{id} SHOP","pending":false}}"#
    )
}

/// The bank's whole history: two on the card, one on checking long ago and
/// one after the bank was connected (the book's today is 2026-10-26).
fn history() -> Pulled {
    let t = [
        txn("C1", "card", 70.0, "2026-09-10"),
        txn("C2", "card", 50.0, "2026-10-01"),
        txn("K1", "chk", 20.0, "2026-09-15"),
        txn("K2", "chk", 30.0, "2026-10-27"),
    ];
    serde_json::from_str(&format!(r#"{{"added":[{}],"modified":[],"removed":[],"cursor":"c1"}}"#, t.join(","))).unwrap()
}

/// The import a server's importer authors: new accounts wait for a choice.
fn import(pulled: &Pulled, from_start: bool) -> Import {
    Import {
        choose: true,
        from_start,
        ..import_of("wells", "Wells Fargo", &accounts(), pulled)
    }
}

fn asks(b: &Book, user: &str) -> Vec<AskLine> {
    let ctx = ark::peer::Ctx {
        actor: user.into(),
        ..Default::default()
    };
    let d = common::domain();
    let bytes = d.ask(&b.store, &ctx, &common::full("asks"), &ark::format::Encode::to_bytes(&())).unwrap();
    ark::format::Encode::from_bytes(&bytes).unwrap()
}

fn answer(b: &mut Book, a: Answer) -> Result<usize, String> {
    let ctx = b.ctx("bank", &[BANK]);
    b.apply(&ctx, "answer", &a)
}

fn bank(b: &Book) -> BankLine {
    b.list::<BankLine>("banks", &()).into_iter().find(|l| l.name == "wells").expect("the bank")
}

fn linked(b: &Book, id: &str) -> LinkedLine {
    b.list::<LinkedLine>("linked", &()).into_iter().find(|l| l.plaid_account_id == id).unwrap()
}

/// Anybody asks to connect; each sees their own asks; only the importer
/// answers, first with Plaid's page and then with the bank it kept, which
/// is known here at once. Falsified by `asks` not filtering by the reader:
/// sam saw alice's page.
#[test]
fn a_connection_is_asked_for_and_answered() {
    let mut b = Book::new();
    b.as_("alice", "connect", &()).unwrap();
    let mine = asks(&b, "alice");
    assert_eq!(mine.len(), 1);
    assert_eq!((mine[0].what.as_str(), mine[0].state), ("connect", AskState::Asked));
    assert!(asks(&b, "sam").is_empty(), "sam sees only his own");

    let waiting = Answer {
        id: mine[0].id,
        state: AskState::Waiting,
        url: "https://hosted.plaid.com/link/abc".into(),
        expires_ms: 1,
        ..Answer::default()
    };
    assert_eq!(b.run("answer", &waiting).unwrap_err(), "only the bank importer answers");
    answer(&mut b, waiting).unwrap();
    let mine = asks(&b, "alice");
    assert_eq!((mine[0].state, mine[0].url.as_str()), (AskState::Waiting, "https://hosted.plaid.com/link/abc"));

    let done = Answer {
        id: mine[0].id,
        state: AskState::Done,
        bank: "wells".into(),
        institution: "Wells Fargo".into(),
        ..Answer::default()
    };
    answer(&mut b, done).unwrap();
    let mine = asks(&b, "alice");
    assert_eq!((mine[0].state, mine[0].bank.as_str(), mine[0].url.as_str()), (AskState::Done, "wells", ""));
    let line = bank(&b);
    assert_eq!((line.institution.as_str(), line.synced_ms, line.accounts), ("Wells Fargo", 0, 0));

    // Asking about a bank nobody connected is refused where it is made.
    let nope = BankName { bank: "chase".into() };
    assert_eq!(b.run("reconnect", &nope).unwrap_err(), "no bank of that name is connected");
    assert_eq!(b.run("disconnect", &nope).unwrap_err(), "no bank of that name is connected");
}

/// A new bank's accounts wait: nothing opens and nothing is imported until
/// somebody chooses. The card given an account of its own opens as a
/// credit account, takes the bank's whole history at the next import from
/// the start, and its starting balance before it; checking linked to the
/// account the household kept by hand takes only what came after the bank
/// was first seen. Falsified by `open` opening the bank's kind as it is:
/// the card's account was "investment".
#[test]
fn each_account_is_chosen_for() {
    let mut b = Book::new();
    let kept = b.account("Checking", "checking", 40_000);
    b.import(&import(&history(), true)).unwrap();
    assert_eq!(b.accounts().len(), 1, "nothing opened by itself");
    assert!(b.register(None).iter().all(|t| !t.imported), "nothing imported before a choice");
    assert_eq!(bank(&b).waiting, 2);
    let card = linked(&b, "card");
    assert!(card.waiting && card.account_id.is_none());
    assert_eq!((card.opens_as.as_str(), card.subtype.as_str()), ("credit", "credit card"));

    b.run(
        "open",
        &OpenLinked {
            plaid_account_id: "card".into(),
            name: "".into(),
        },
    )
    .unwrap();
    b.run(
        "link",
        &LinkAccount {
            plaid_account_id: "chk".into(),
            account_id: kept,
        },
    )
    .unwrap();
    let opened = b.accounts().into_iter().find(|a| a.name == "Active Cash Card").expect("opened for the card");
    assert_eq!((opened.kind.as_str(), opened.on_budget), ("credit", true));
    assert_eq!(bank(&b).waiting, 0);
    let wells: Vec<Bank> = b.rows("bank");
    assert!(wells[0].repull, "the next import asks from the start");
    assert!(!b.rows::<Asked>("asked").is_empty(), "and the importer is asked for it now");

    // An import that was on its way from the old cursor changes no balance
    // and asks again.
    let none: Pulled = serde_json::from_str(r#"{"added":[],"modified":[],"removed":[],"cursor":"c2"}"#).unwrap();
    b.import(&import(&none, false)).unwrap();
    assert!(b.register(Some(opened.id)).is_empty());
    assert!(b.rows::<Bank>("bank")[0].repull);

    // From the start: the card's history and its balance before it.
    b.import(&import(&history(), true)).unwrap();
    assert!(!b.rows::<Bank>("bank")[0].repull);
    let mut card: Vec<(String, i64)> = b.register(Some(opened.id)).into_iter().map(|t| (t.payee, t.amount)).collect();
    card.sort();
    assert_eq!(
        card,
        [("C1 SHOP".to_string(), -7_000), ("C2 SHOP".to_string(), -5_000)],
        "owed 120 at the bank, all of it the history: no starting balance"
    );
    assert_eq!(b.accounts().into_iter().find(|a| a.id == opened.id).unwrap().balance, -12_000);
    let checking: Vec<String> = b.register(Some(kept)).into_iter().filter(|t| t.imported).map(|t| t.payee).collect();
    assert_eq!(checking, ["K2 SHOP"], "only what came after the bank was first seen");

    // Again from the start: nothing doubles, and no second starting balance.
    b.run(
        "open",
        &OpenLinked {
            plaid_account_id: "card".into(),
            name: "Again".into(),
        },
    )
    .unwrap_err();
    b.import(&import(&history(), true)).unwrap();
    assert_eq!(b.register(Some(opened.id)).len(), 2);
}

/// A starting balance is what the bank holds less the history: a card
/// owing more than its history spent opens owing the rest.
#[test]
fn a_card_opened_owes_what_its_history_does_not_explain() {
    let mut b = Book::new();
    let one: Pulled =
        serde_json::from_str(&format!(r#"{{"added":[{}],"modified":[],"removed":[],"cursor":"c1"}}"#, txn("C1", "card", 20.0, "2026-09-10"))).unwrap();
    b.import(&import(&one, true)).unwrap();
    b.run(
        "open",
        &OpenLinked {
            plaid_account_id: "card".into(),
            name: "Visa".into(),
        },
    )
    .unwrap();
    b.import(&import(&one, true)).unwrap();
    let visa = b.accounts().into_iter().find(|a| a.name == "Visa").unwrap();
    assert_eq!(visa.balance, -12_000, "the bank says 120 owed");
    let start = b.register(Some(visa.id)).into_iter().find(|t| t.payee == "Starting Balance").unwrap();
    assert_eq!((start.amount, start.date.as_str()), (-10_000, "2026-09-09"));
}

/// Ignoring a waiting account is a choice too; what it has is dropped.
#[test]
fn a_waiting_account_may_be_ignored() {
    let mut b = Book::new();
    b.import(&import(&history(), true)).unwrap();
    b.run(
        "ignore",
        &IgnoreLinked {
            plaid_account_id: "chk".into(),
            ignored: true,
        },
    )
    .unwrap();
    let chk = linked(&b, "chk");
    assert!(chk.ignored && !chk.waiting);
    assert_eq!(bank(&b).waiting, 1);
}

/// What Plaid said when an import failed is the bank's until one works: a
/// login run out says a reconnect mends it, the reconnect answered clears
/// it. Falsified by `import` keeping `error_code`: it was still
/// ITEM_LOGIN_REQUIRED after the import that worked.
#[test]
fn a_failure_is_said_until_it_is_mended() {
    let mut b = Book::new();
    b.import(&import(&history(), true)).unwrap();
    let failed = BankFailed {
        bank: "wells".into(),
        institution: String::new(),
        error_code: "ITEM_LOGIN_REQUIRED".into(),
        error: "the login details of this item have changed".into(),
    };
    assert!(b.run("failed", &failed).is_err(), "only the importer says it");
    let ctx = b.ctx("bank", &[BANK]);
    b.apply(&ctx, "failed", &failed).unwrap();
    let line = bank(&b);
    assert!(line.needs_login && line.failed_ms > 0);
    assert_eq!(line.institution, "Wells Fargo", "kept");

    b.run("reconnect", &BankName { bank: "wells".into() }).unwrap();
    let ask = asks(&b, "alice").remove(0);
    assert_eq!((ask.what.as_str(), ask.bank.as_str(), ask.institution.as_str()), ("reconnect", "wells", "Wells Fargo"));
    answer(
        &mut b,
        Answer {
            id: ask.id,
            state: AskState::Done,
            ..Answer::default()
        },
    )
    .unwrap();
    let line = bank(&b);
    assert!(!line.needs_login && line.error_code.is_empty());

    // And an import that works clears one too.
    let ctx = b.ctx("bank", &[BANK]);
    b.apply(&ctx, "failed", &BankFailed { error_code: "INTERNAL_SERVER_ERROR".into(), ..failed }).unwrap();
    assert_eq!(bank(&b).error_code, "INTERNAL_SERVER_ERROR");
    b.import(&import(&history(), false)).unwrap();
    assert_eq!(bank(&b).error_code, "");
}

/// A disconnection answered forgets the bank and its accounts' links; the
/// accounts it fed and their transactions stay, kept by hand from then on.
#[test]
fn a_disconnected_bank_leaves_its_accounts() {
    let mut b = Book::new();
    b.import(&import(&history(), true)).unwrap();
    b.run(
        "open",
        &OpenLinked {
            plaid_account_id: "card".into(),
            name: "Visa".into(),
        },
    )
    .unwrap();
    b.import(&import(&history(), true)).unwrap();
    b.run("disconnect", &BankName { bank: "wells".into() }).unwrap();
    let ask = asks(&b, "alice").remove(0);
    assert_eq!(ask.what, "disconnect");
    answer(
        &mut b,
        Answer {
            id: ask.id,
            state: AskState::Done,
            ..Answer::default()
        },
    )
    .unwrap();
    assert!(b.list::<BankLine>("banks", &()).is_empty());
    assert!(b.list::<LinkedLine>("linked", &()).is_empty());
    let visa = b.accounts().into_iter().find(|a| a.name == "Visa").unwrap();
    assert_eq!((visa.bank.as_str(), visa.balance), ("", -12_000));
    assert_eq!(b.register(Some(visa.id)).len(), 2);
}
