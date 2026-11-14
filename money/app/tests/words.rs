//! What the rows say, as every client says it (`money_core::words`,
//! `accounts`, `banks::follow`, `transactions::Draft`): the rules the
//! phones held in their own tests before (Android's CardsTests, BanksTests,
//! BookTests and RulesTests; the iPhone's BudgetTests, CardTests,
//! FormTests, BankTests, RuleTests and ReceiptTests), held here once.
// A test reads its fixtures and times itself: the kernel's and the core's
// purity (clippy.toml) is their library's, not their tests'.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use ark::row::Id;
use ark_format::NumberStyle;
use money_app::words::{BankWords, LineWords};
use money_core::accounts::{self, AccountKind, Shelving};
use money_core::banks::{follow, Follow};
use money_core::transactions::{suggest, Draft};
use money_core::words::{self, Tone};
use money_core::*;

fn us() -> NumberStyle {
    NumberStyle::us()
}

fn id<R>(n: u8) -> Id<R> {
    Id::new([n; 16])
}

fn line(available: i64, target: i64, assigned: i64) -> BudgetLine {
    BudgetLine {
        category_id: id(9),
        name: "Groceries".into(),
        available,
        target,
        assigned,
        ..BudgetLine::default()
    }
}

fn payment(available: i64, debt: i64, underfunded: i64) -> BudgetLine {
    BudgetLine {
        card: Some(id(7)),
        group_id: money_core::cards::card_group(),
        available,
        debt,
        underfunded,
        ..line(available, 0, 0)
    }
}

// * A budget line

/// Falsified by the iPhone's `assigned < target` without `target > 0`: a
/// category given back money (assigned below nothing) with no target read
/// short.
#[test]
fn a_lines_tone_is_decided_once() {
    assert_eq!(line(0, 0, 0).tone(), Tone::Neutral);
    assert_eq!(line(500, 0, 500).tone(), Tone::Positive);
    assert_eq!(line(500, 1_000, 500).tone(), Tone::Short);
    assert_eq!(line(1_000, 1_000, 1_000).tone(), Tone::Positive);
    assert_eq!(line(-300, 0, 0).tone(), Tone::Overspent);
    assert_eq!(line(200, 0, -100).tone(), Tone::Positive, "no target, nothing short");
    let on_card = BudgetLine {
        credit_overspent: 500,
        ..line(-500, 0, 0)
    };
    assert_eq!(on_card.tone(), Tone::OnCard);
    let both = BudgetLine {
        credit_overspent: 200,
        ..line(-500, 0, 0)
    };
    assert_eq!(both.tone(), Tone::Overspent, "some cash overspent is cash's red");
}

/// A card's payment reads by its debt, never by a target.
#[test]
fn a_cards_payment_reads_by_its_debt() {
    let short = payment(3_000, 10_000, 7_000);
    let covered = payment(10_000, 10_000, 0);
    let paid_off = payment(0, 0, 0);
    assert!(short.is_payment() && !line(0, 0, 0).is_payment());
    assert_eq!(
        (short.tone(), covered.tone(), paid_off.tone()),
        (Tone::Short, Tone::Positive, Tone::Neutral)
    );
    let with_target = BudgetLine {
        target: 50_000,
        ..covered.clone()
    };
    assert_eq!(with_target.tone(), Tone::Positive, "a payment's target never counts");
    assert_eq!(short.meter().map(|m| (m.thousandths, m.tone)), Some((300, Tone::Short)));
    assert_eq!(covered.meter().map(|m| (m.thousandths, m.tone)), Some((1000, Tone::Positive)));
    assert_eq!(paid_off.meter(), None);
    assert_eq!(line(0, 0, 0).meter(), None, "no target, no bar");
    assert_eq!(line(500, 1_000, 500).meter().map(|m| m.thousandths), Some(500));
    let short_assigned = BudgetLine {
        assigned: 2_000,
        ..short.clone()
    };
    assert_eq!(short_assigned.to_cover(), Some(9_000));
    assert_eq!(covered.to_cover(), None);
    assert_eq!(line(0, 0, 0).to_cover(), None);
    assert_eq!(words::uncovered(&[short.clone(), covered.clone(), line(-5, 0, 0)]), 7_000);
    let s = us();
    assert_eq!(short.story(&s), "$70.00 short of the $100.00 owed");
    assert_eq!(covered.story(&s), "Covered: $100.00 owed");
    assert_eq!(paid_off.story(&s), "Nothing owed on the card");
    assert_eq!(short.captions(&s).left, "$70.00 short");
    assert_eq!(short.captions(&s).right, "Owed $100.00");
    assert_eq!(paid_off.captions(&s).left, "Nothing owed");
    assert_eq!(short.progress(&s), "$70.00 short");
    assert_eq!(covered.progress(&s), "Covered");
}

#[test]
fn a_lines_words() {
    let s = us();
    let l = BudgetLine {
        activity: -2_500,
        ..line(2_500, 10_000, 5_000)
    };
    assert_eq!(l.story(&s), "$50.00 more needed this month");
    assert_eq!(
        l.captions(&s),
        money_app::words::Captions {
            left: "$50.00 of $100.00".into(),
            right: "Spent $25.00".into()
        }
    );
    assert_eq!(l.progress(&s), "50% goal");
    let spent = BudgetLine {
        activity: -2_500,
        ..line(2_500, 0, 5_000)
    };
    assert_eq!(spent.story(&s), "Spent $25.00 of $50.00");
    assert_eq!(spent.captions(&s).left, "Assigned $50.00");
    assert_eq!(line(-300, 0, 0).story(&s), "Overspent by $3.00");
    let ruled = BudgetLine {
        ruled: true,
        rule: " 10% × ACME ".into(),
        ..line(0, 0, 0)
    };
    assert_eq!(ruled.rule_words().as_deref(), Some("Assigned by rule: 10% × ACME"));
    assert!(!ruled.assignable());
    assert_eq!(line(0, 0, 0).rule_words(), None);
    let rta = BudgetLine {
        category_id: money_core::budget::ready_to_assign(),
        ..line(0, 0, 0)
    };
    assert!(rta.is_ready_to_assign() && !rta.assignable());
    assert_eq!(
        line(500, 1_000, 500).spoken(&s),
        "$5.00 available, underfunded. Assigned $5.00 of a $10.00 target"
    );
}

#[test]
fn the_months_words() {
    let s = |r: i64| Summary {
        ready_to_assign: r,
        overspent: 1,
        underfunded: 2,
        ..Summary::default()
    };
    assert_eq!(s(-1).ready_words(), "You've assigned more than you have");
    assert_eq!(s(0).ready_words(), "Every dollar has a job");
    assert_eq!(s(5).ready_words(), "Give these dollars a job");
    assert_eq!((s(5).tone(), s(0).tone(), s(-5).tone()), (Tone::Positive, Tone::Neutral, Tone::Overspent));
    assert_eq!(s(0).attention(), 3);
}

// * Transactions and the form

#[test]
fn a_transaction_says_what_the_bank_did() {
    let mut t = TxnLine {
        payee: "Chipotle".into(),
        imported: true,
        imported_payee: "CHIPOTLE 1234 SAN FRANCISCO CA".into(),
        ..TxnLine::default()
    };
    assert_eq!(t.bank_said().as_deref(), Some("CHIPOTLE 1234 SAN FRANCISCO CA"));
    t.imported_payee = "chipotle ".into();
    assert_eq!(t.bank_said(), None, "named alike");
    t.imported_payee = "CHIPOTLE 1234".into();
    t.imported = false;
    assert_eq!(t.bank_said(), None, "typed in");
    t.category = money_core::transactions::NEEDS_A_CATEGORY.into();
    assert!(t.needs_category());
}

fn account(n: u8, name: &str, pos: i64, on_budget: bool, closed: bool) -> AccountLine {
    AccountLine {
        id: id(n),
        name: name.into(),
        kind: "checking".into(),
        pos,
        on_budget,
        closed,
        balance: 1_000 * n as i64,
        ..AccountLine::default()
    }
}

/// One rule set: an account, an amount, a day, a payee unless it is a
/// transfer; then the domain's own refinements.
#[test]
fn the_form_has_one_set_of_rules() {
    let accounts = [account(2, "Savings", 2, true, false), account(1, "Checking", 1, true, false)];
    let d = Draft::new(&accounts, None, "2026-10-26");
    assert_eq!(d.account, Some(id(1)), "the first on budget");
    assert_eq!(d.problem().as_deref(), Some("Enter an amount."));
    let d = Draft { amount: -650, ..d };
    assert_eq!(d.problem().as_deref(), Some("Enter a payee."));
    let d = Draft {
        payee: "  Blue Bottle ".into(),
        ..d
    };
    assert_eq!(d.problem(), None);
    let Some(MoneyChange::AddTransaction(a)) = d.to_change() else { panic!() };
    assert_eq!((a.payee.as_str(), a.amount, a.account_id), ("Blue Bottle", -650, id(1)));
    assert_eq!(
        Draft {
            date: "26 Oct".into(),
            ..d.clone()
        }
        .problem()
        .as_deref(),
        Some("Choose a day.")
    );
    assert_eq!(Draft { account: None, ..d.clone() }.problem().as_deref(), Some("Choose an account."));
    assert!(
        Draft {
            payee: "x".repeat(121),
            ..d.clone()
        }
        .problem()
        .is_some(),
        "the domain's own length"
    );
    let t = d.with_category(Some(id(5))).with_transfer(id(2));
    assert_eq!((t.category, t.transfer_to), (None, Some(id(2))));
    let t = Draft { payee: String::new(), ..t };
    assert_eq!(t.problem(), None, "a transfer's payee is its account");
    let Some(MoneyChange::AddTransaction(a)) = t.to_change() else { panic!() };
    assert_eq!((a.payee.as_str(), a.category_id, a.transfer_to), ("", None, Some(id(2))));
    let p = PayeeLine {
        name: "Blue Bottle".into(),
        count: 3,
        category_id: Some(id(4)),
    };
    assert_eq!(d.with_payee(&p).category, Some(id(4)), "its last category comes with it");
    assert_eq!(d.with_category(Some(id(5))).with_payee(&p).category, Some(id(5)), "unless one is chosen");
}

/// Falsified by the shells' "Transfer : <name>" parsing: two accounts of
/// one name, one closed, read the closed one.
#[test]
fn a_transfer_corrected_keeps_its_account() {
    let t = TxnLine {
        id: id(3),
        account_id: id(1),
        date: "2026-10-02".into(),
        payee: "Transfer : Savings".into(),
        transfer: true,
        transfer_account_id: Some(id(2)),
        amount: -10_000,
        ..TxnLine::default()
    };
    let d = Draft::of(&t);
    assert_eq!((d.payee.as_str(), d.transfer_to, d.editing), ("", Some(id(2)), Some(id(3))));
    let Some(MoneyChange::EditTransaction(e)) = d.to_change() else {
        panic!()
    };
    assert_eq!((e.transfer_to, e.payee.as_str(), e.category_id), (Some(id(2)), "", None));
}

#[test]
fn payees_are_offered_as_one_types() {
    let p = |n: &str, c: i64| PayeeLine {
        name: n.into(),
        count: c,
        category_id: None,
    };
    let all = [p("Blue Bottle", 2), p("Bluestone Lane", 5), p("True Blue", 9), p("Safeway", 1)];
    let names = |v: Vec<PayeeLine>| v.into_iter().map(|p| p.name).collect::<Vec<_>>();
    assert_eq!(names(suggest("blue", &all, 4)), ["Bluestone Lane", "Blue Bottle", "True Blue"]);
    assert_eq!(names(suggest("", &all, 2)), ["True Blue", "Bluestone Lane"]);
    assert!(suggest("safeway", &all, 4).is_empty(), "typed whole");
}

// * Accounts

#[test]
fn accounts_are_shelved_once() {
    assert_eq!(
        AccountKind::ALL.map(|k| k.title()),
        ["Checking", "Savings", "Cash", "Credit Card", "Loan or Mortgage", "Investment"]
    );
    assert!(AccountKind::Credit.on_budget_by_default() && !AccountKind::Loan.on_budget_by_default());
    assert!(AccountKind::Loan.owes() && !AccountKind::Cash.owes());
    assert_eq!(accounts::kind_title("boat"), "Boat");
    let all = [
        account(3, "Old", 0, true, true),
        account(2, "Brokerage", 2, false, false),
        account(1, "Checking", 1, true, false),
    ];
    let shelves = accounts::shelves(&all);
    assert_eq!(
        shelves.iter().map(|s| s.shelving).collect::<Vec<_>>(),
        [Shelving::Budget, Shelving::Tracking, Shelving::Closed]
    );
    assert_eq!(shelves[0].title, "Budget");
    assert_eq!(accounts::net_worth(&all), 3_000, "the closed left out");
    assert_eq!(accounts::usual(&all), Some(id(1)));
    let fed = AccountLine {
        bank: "chase".into(),
        mask: "4321".into(),
        ..all[2].clone()
    };
    assert_eq!(fed.subtitle(), "Chase \u{2022}\u{2022}4321");
    assert_eq!(all[2].subtitle(), "Checking");
    let card = AccountLine {
        kind: "credit".into(),
        name: "Visa".into(),
        pos: 9,
        ..account(4, "Visa", 9, true, false)
    };
    let linkable = accounts::linkable(&[all[2].clone(), card, fed], "credit");
    assert_eq!(
        linkable.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        ["Visa", "Checking"],
        "the fed one left out"
    );
}

// * Banks

fn ask(n: u8, what: &str, state: AskState) -> AskLine {
    AskLine {
        id: id(n),
        what: what.into(),
        state,
        asked_ms: 1_000_000,
        ..AskLine::default()
    }
}

/// By the ask's id, not the clock: falsified by the iPhone's second of
/// grace, which took an ask made a minute before as this one.
#[test]
fn an_ask_is_followed_until_its_page_arrives() {
    let page = |n: u8| AskLine {
        url: "https://plaid.example/p".into(),
        ..ask(n, "connect", AskState::Waiting)
    };
    assert_eq!(follow(&[], None), Follow::Wait);
    assert_eq!(follow(&[page(1)], Some(id(1))), Follow::Wait, "the one there was before");
    assert_eq!(follow(&[ask(2, "connect", AskState::Asked), page(1)], Some(id(1))), Follow::Wait);
    assert_eq!(
        follow(&[ask(2, "connect", AskState::Waiting)], Some(id(1))),
        Follow::Wait,
        "no address yet"
    );
    assert_eq!(
        follow(&[page(2), page(1)], Some(id(1))),
        Follow::Page {
            url: "https://plaid.example/p".into()
        }
    );
    for s in [AskState::Done, AskState::Failed, AskState::Expired] {
        assert_eq!(follow(&[ask(2, "connect", s)], None), Follow::Over);
    }
}

/// Falsified by the phones' wording: "add accounts" done read
/// "Disconnected".
#[test]
fn what_an_ask_says() {
    let now = 1_000_000;
    let a = ask(1, "connect", AskState::Asked);
    assert_eq!(a.said(now), "Asking the server for Plaid's page\u{2026}");
    assert_eq!(a.said(now + 60_000), "The server hasn't answered yet. Does it have Plaid's keys?");
    let done = |what: &str| AskLine {
        institution: "Chase".into(),
        ..ask(1, what, AskState::Done)
    };
    assert_eq!(done("connect").said(now), "Connected Chase. Choose what each of its accounts feeds.");
    assert_eq!(done("reconnect").said(now), "Logged in to Chase again.");
    assert!(
        done("add_accounts").said(now).starts_with("Shared Chase's accounts again"),
        "{}",
        done("add_accounts").said(now)
    );
    assert_eq!(
        done("disconnect").said(now),
        "Disconnected Chase. Its accounts stay, kept by hand from now on."
    );
    let failed = AskLine {
        message: "Plaid said no".into(),
        ..ask(1, "add_accounts", AskState::Failed)
    };
    assert_eq!(failed.said(now), "Couldn't add accounts the bank: Plaid said no");
    assert_eq!(
        ask(1, "connect", AskState::Expired).said(now),
        "Plaid's page closed before it was finished."
    );
    let waiting = AskLine {
        expires_ms: now + 10 * 60_000,
        ..ask(1, "connect", AskState::Waiting)
    };
    assert_eq!(waiting.closes(now).as_deref(), Some("It closes in 10 min"));
    assert_eq!(waiting.closes(now + 10 * 60_000), None);
    assert_eq!(a.closes(now), None);
    let answered = AskLine {
        answered_ms: now,
        ..done("connect")
    };
    assert_eq!(
        words::ask_shown(std::slice::from_ref(&answered), None, now + 60_000).map(|a| a.id),
        Some(id(1))
    );
    assert_eq!(words::ask_shown(std::slice::from_ref(&answered), Some(id(1)), now), None, "put away");
    assert_eq!(words::ask_shown(&[answered], None, now + words::ASK_SHOWN_MS), None, "long ago");
}

#[test]
fn a_bank_says_how_it_is() {
    let b = |f: fn(&mut BankLine)| {
        let mut b = BankLine {
            name: "chase".into(),
            ..BankLine::default()
        };
        f(&mut b);
        b
    };
    assert!(b(|b| b.needs_login = true).status(0).alarm);
    let failed = b(|b| {
        b.error = "the bank is down".into();
        b.failed_ms = 1;
    });
    assert!(failed.status(1).alarm && failed.status(1).words.ends_with(": the bank is down"));
    assert_eq!(b(|_| {}).status(0).words, "Waiting for its first import");
    assert_eq!(b(|_| {}).title(), "Chase");
    assert_eq!(b(|b| b.institution = "Wells Fargo".into()).title(), "Wells Fargo");
    let wanting = [b(|b| b.waiting = 2), b(|b| b.needs_login = true), b(|_| {})];
    assert_eq!(words::banks_wanting(&wanting), 3);
    let l = LinkedLine {
        name: "Signature Visa".into(),
        mask: "4321".into(),
        kind: "credit".into(),
        subtype: "credit card".into(),
        opens_as: "credit".into(),
        ..LinkedLine::default()
    };
    assert_eq!(
        (l.title(), l.kind_words(), l.opens_words()),
        (
            "Signature Visa \u{2022}\u{2022}4321".into(),
            "Credit card".into(),
            "New Credit Card Account".into()
        )
    );
}

// * Receipts

#[test]
fn receipts_offered_for_a_transaction() {
    let r = |n: &str, total: i64, date: &str, txn: Option<Id<Txn>>| ReceiptLine {
        id: n.into(),
        store: "paper".into(),
        total,
        date: date.into(),
        txn_id: txn,
        ..ReceiptLine::default()
    };
    let all: Vec<ReceiptLine> = (0..12)
        .map(|i| r(&format!("r{i:02}"), -1_000 - i, "2026-10-01", None))
        .chain([r("near", -650, "2026-10-02", None), r("paid", -650, "2026-10-02", Some(id(1)))])
        .collect();
    let offered = words::unmatched_for(&all, -650, "2026-10-02");
    assert_eq!(offered.len(), words::UNMATCHED_SHOWN);
    assert_eq!(offered[0].id, "near");
    assert!(offered.iter().all(|r| r.id != "paid"));
    assert_eq!(words::store_name("amazon"), "Amazon");
    assert_eq!(money_app::words::quantity(2_000, "ea"), "\u{00d7}2");
    assert_eq!(money_app::words::quantity(11_482, "gal"), "11.482 gal");
}
