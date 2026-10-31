//! Credit cards, budgeted as YNAB budgets them, on a household's months:
//! what a category covers of spending on a card moves to the card's payment
//! category, what it cannot is debt that next month's money does not pay
//! for, a payment is spending from the payment category, a card's own debt
//! is the payment category's to cover, and a payment the banks tell of
//! from both ends is one transfer.

mod common;

use common::Book;
use money_domain::cards::{card_group, payment_category, CARD_GROUP};
use money_domain::*;

/// A household: $3,000 in checking, a card that owes nothing yet, and an
/// everyday group with groceries and dining in it.
struct House {
    b: Book,
    checking: Id<Account>,
    card: Id<Account>,
    groceries: Id<Category>,
    dining: Id<Category>,
}

fn house() -> House {
    let mut b = Book::new();
    let checking = b.account("Checking", "checking", 300_000);
    let card = b.account("Visa", "credit", 0);
    let everyday = b.group("Everyday");
    let groceries = b.category(everyday, "Groceries");
    let dining = b.category(everyday, "Dining");
    House {
        b,
        checking,
        card,
        groceries,
        dining,
    }
}

impl House {
    fn pay(&self) -> Id<Category> {
        payment_category(self.card)
    }

    fn transfer(&mut self, from: Id<Account>, to: Id<Account>, date: &str, amount: i64) {
        let t = AddTransaction {
            account_id: from,
            date: date.into(),
            amount: -amount,
            transfer_to: Some(to),
            ..AddTransaction::default()
        };
        self.b.run("add_transaction", &t).unwrap();
    }
}

fn figures(l: &BudgetLine) -> (i64, i64, i64, i64, i64) {
    (l.assigned, l.activity, l.available, l.debt, l.underfunded)
}

#[test]
fn a_card_on_the_budget_has_a_payment_category_in_the_cards_group() {
    let h = house();
    let groups: Vec<GroupLine> = h.b.list("groups", &());
    let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, [CARD_GROUP, "Everyday"], "the cards' group comes first");
    assert_eq!(groups[0].id, card_group());
    let pay = h.b.line("2026-09", payment_category(h.card));
    assert_eq!((pay.name.as_str(), pay.card, pay.group_id), ("Visa", Some(h.card), card_group()));
    assert_eq!(figures(&pay), (0, 0, 0, 0, 0));
    // Not a card: no payment category; and a card off the budget has none.
    let mut b = h.b;
    b.account("Mortgage", "loan", -1_000_000);
    b.run(
        "open_account",
        &OpenAccount {
            name: "Store Card".into(),
            kind: "credit".into(),
            on_budget: false,
            balance: -5_000,
            date: "2026-09-01".into(),
        },
    )
    .unwrap();
    let lines: Vec<BudgetLine> = b.list("month", &Month { month: "2026-09".into() });
    let cards: Vec<&str> = lines.iter().filter(|l| l.group_id == card_group()).map(|l| l.name.as_str()).collect();
    assert_eq!(cards, ["Visa"]);
}

#[test]
fn spending_on_a_card_from_a_funded_category_moves_the_money_to_its_payment() {
    let mut h = house();
    h.b.assign("2026-09", h.groceries, 40_000);
    h.b.spend(h.card, "2026-09-05", "Market", Some(h.groceries), -15_000);
    let g = h.b.line("2026-09", h.groceries);
    assert_eq!((g.activity, g.available, g.credit_overspent), (-15_000, 25_000, 0));
    let pay = h.pay();
    let p = h.b.line("2026-09", pay);
    assert_eq!(figures(&p), (0, 15_000, 15_000, 15_000, 0), "the debt is covered");
    // Nothing more was spent from the budget's money: ready to assign is
    // what was assigned less, and the month's totals net the move out.
    let s = h.b.summary("2026-09");
    assert_eq!((s.ready_to_assign, s.assigned, s.activity, s.available), (260_000, 40_000, 0, 40_000));
    assert_eq!(s.underfunded, 0);
    // And it carries: next month the payment category still has it.
    let oct = h.b.line("2026-10", pay);
    assert_eq!((oct.carried, oct.available, oct.debt, oct.underfunded), (15_000, 15_000, 15_000, 0));
}

#[test]
fn spending_more_than_a_category_has_on_a_card_is_new_debt() {
    let mut h = house();
    h.b.assign("2026-09", h.dining, 5_000);
    h.b.spend(h.card, "2026-09-12", "Bistro", Some(h.dining), -8_000);
    let d = h.b.line("2026-09", h.dining);
    assert_eq!((d.available, d.credit_overspent), (-3_000, 3_000));
    let pay = h.pay();
    let p = h.b.line("2026-09", pay);
    assert_eq!(figures(&p), (0, 5_000, 5_000, 8_000, 3_000), "$30 of it not covered");
    let s = h.b.summary("2026-09");
    assert_eq!((s.overspent, s.underfunded), (1, 1));
    // October: dining starts at nothing, and ready to assign is NOT taken
    // for it — no cash was spent; the card's payment still lacks the $30.
    let d = h.b.line("2026-10", h.dining);
    assert_eq!((d.carried, d.available, d.credit_overspent), (0, 0, 0));
    let s = h.b.summary("2026-10");
    assert_eq!((s.overspent_last_month, s.ready_to_assign), (0, 300_000 - 5_000));
    let p = h.b.line("2026-10", pay);
    assert_eq!((p.carried, p.available, p.debt, p.underfunded), (5_000, 5_000, 8_000, 3_000));
    // Cash overspending, beside it, is still next month's to pay.
    let rent = {
        let bills = h.b.group("Bills");
        h.b.category(bills, "Rent")
    };
    h.b.spend(h.checking, "2026-09-01", "Landlord", Some(rent), -2_000);
    let s = h.b.summary("2026-10");
    assert_eq!((s.overspent_last_month, s.ready_to_assign), (-2_000, 300_000 - 5_000 - 2_000));
}

#[test]
fn covering_credit_overspending_in_its_month_moves_the_money_after_all() {
    let mut h = house();
    h.b.spend(h.card, "2026-09-12", "Bistro", Some(h.dining), -8_000);
    let pay = h.pay();
    assert_eq!(figures(&h.b.line("2026-09", pay)), (0, 0, 0, 8_000, 8_000));
    h.b.assign("2026-09", h.dining, 8_000);
    assert_eq!(figures(&h.b.line("2026-09", pay)), (0, 8_000, 8_000, 8_000, 0));
    assert_eq!(h.b.line("2026-09", h.dining).credit_overspent, 0);
}

#[test]
fn cash_overspending_comes_first_where_both_are() {
    let mut h = house();
    h.b.assign("2026-09", h.groceries, 10_000);
    h.b.spend(h.checking, "2026-09-02", "Market", Some(h.groceries), -13_000);
    h.b.spend(h.card, "2026-09-03", "Market", Some(h.groceries), -2_000);
    let g = h.b.line("2026-09", h.groceries);
    assert_eq!((g.available, g.credit_overspent), (-5_000, 2_000));
    assert_eq!(h.b.line("2026-09", h.pay()).activity, 0, "nothing was left to cover the card");
    // Only the cash part comes out of October.
    assert_eq!(h.b.summary("2026-10").overspent_last_month, -3_000);
}

#[test]
fn a_refund_on_a_card_reverses_what_its_spending_did() {
    let mut h = house();
    let pay = h.pay();
    h.b.assign("2026-09", h.groceries, 40_000);
    h.b.spend(h.card, "2026-09-05", "Market", Some(h.groceries), -15_000);
    h.b.spend(h.card, "2026-09-08", "Market", Some(h.groceries), 4_000);
    assert_eq!(h.b.line("2026-09", h.groceries).available, 29_000);
    assert_eq!(figures(&h.b.line("2026-09", pay)), (0, 11_000, 11_000, 11_000, 0));
    // A refund into a category overspent on the card first undoes the debt
    // it made: the payment category is no different, and lacks less.
    h.b.spend(h.card, "2026-09-12", "Bistro", Some(h.dining), -8_000);
    assert_eq!(figures(&h.b.line("2026-09", pay)), (0, 11_000, 11_000, 19_000, 8_000));
    h.b.spend(h.card, "2026-09-14", "Bistro", Some(h.dining), 3_000);
    let d = h.b.line("2026-09", h.dining);
    assert_eq!((d.available, d.credit_overspent), (-5_000, 5_000));
    assert_eq!(figures(&h.b.line("2026-09", pay)), (0, 11_000, 11_000, 16_000, 5_000));
    // A refund in a later month, into a category with nothing: it moves
    // the money back out of the payment category.
    h.b.spend(h.card, "2026-10-02", "Market", Some(h.groceries), 1_000);
    let p = h.b.line("2026-10", pay);
    assert_eq!((p.activity, p.available, p.debt), (-1_000, 10_000, 15_000));
}

#[test]
fn paying_the_card_is_spending_from_its_payment_category() {
    let mut h = house();
    let pay = h.pay();
    h.b.assign("2026-09", h.groceries, 40_000);
    h.b.spend(h.card, "2026-09-05", "Market", Some(h.groceries), -15_000);
    let (checking, card) = (h.checking, h.card);
    h.transfer(checking, card, "2026-09-20", 10_000);
    let p = h.b.line("2026-09", pay);
    assert_eq!(figures(&p), (0, 5_000, 5_000, 5_000, 0));
    // The transfer is for no category on either side, and moved nothing
    // to assign.
    assert!(h.b.register(Some(card)).iter().any(|t| t.transfer && t.category_id.is_none()));
    assert_eq!(h.b.summary("2026-09").ready_to_assign, 260_000);
    let a = h.b.accounts();
    let bal = |id| a.iter().find(|x| x.id == id).unwrap().balance;
    assert_eq!((bal(checking), bal(card)), (290_000, -5_000));
    // Paying more than it has is overspending it — cash, so October pays.
    h.transfer(checking, card, "2026-09-25", 8_000);
    let p = h.b.line("2026-09", pay);
    assert_eq!((p.available, p.debt, p.credit_overspent), (-3_000, 0, 0));
    let s = h.b.summary("2026-10");
    assert_eq!((s.overspent_last_month, s.ready_to_assign), (-3_000, 300_000 - 40_000 - 3_000));
    // Paid from checking without a transfer, filed under the payment
    // category: the same.
    h.b.spend(h.card, "2026-10-03", "Market", Some(h.groceries), -2_000);
    h.b.spend(h.checking, "2026-10-04", "Visa payment", Some(pay), -1_000);
    assert_eq!(h.b.line("2026-10", pay).activity, 2_000 - 1_000);
}

#[test]
fn a_cards_debt_when_it_is_opened_is_its_payment_categorys_to_cover() {
    let mut b = Book::new();
    b.account("Checking", "checking", 300_000);
    let amex = b.account("Amex", "credit", -50_000);
    let pay = payment_category(amex);
    // Not off ready to assign: it is debt, not the budget's money.
    assert_eq!(b.summary("2026-09").ready_to_assign, 300_000);
    let start = b.register(Some(amex)).pop().unwrap();
    assert_eq!(
        (start.payee.as_str(), start.category.as_str(), start.amount),
        ("Starting Balance", "Amex", -50_000)
    );
    assert_eq!(figures(&b.line("2026-09", pay)), (0, 0, 0, 50_000, 50_000));
    assert_eq!(b.summary("2026-09").underfunded, 1);
    // Money assigned to it pays the debt down.
    b.assign("2026-09", pay, 20_000);
    assert_eq!(figures(&b.line("2026-09", pay)), (20_000, 0, 20_000, 50_000, 30_000));
    assert_eq!(b.summary("2026-09").ready_to_assign, 280_000);
    // Auto-assign's "underfunded" covers the rest.
    let auto = AutoAssign {
        month: "2026-09".into(),
        how: Auto::Underfunded,
        only: vec![],
    };
    b.run("auto_assign", &auto).unwrap();
    assert_eq!(figures(&b.line("2026-09", pay)), (50_000, 0, 50_000, 50_000, 0));
    assert_eq!(b.summary("2026-09").underfunded, 0);
    // A card the household is owed on starts with money to assign, as any
    // account does.
    b.account("Refunded", "credit", 2_500);
    assert_eq!(b.summary("2026-09").ready_to_assign, 300_000 - 50_000 + 2_500);
}

#[test]
fn auto_assign_covers_credit_overspending_after_the_categories() {
    let mut h = house();
    let pay = h.pay();
    h.b.run("set_target", &SetTarget { id: h.dining, target: 5_000 }).unwrap();
    h.b.spend(h.card, "2026-09-12", "Bistro", Some(h.dining), -8_000);
    let auto = AutoAssign {
        month: "2026-09".into(),
        how: Auto::Underfunded,
        only: vec![],
    };
    h.b.run("auto_assign", &auto).unwrap();
    // Dining got its target, which covered $50 of the bistro; the payment
    // category was given the $30 the card still owed beyond it.
    assert_eq!(h.b.line("2026-09", h.dining).assigned, 5_000);
    assert_eq!(figures(&h.b.line("2026-09", pay)), (3_000, 5_000, 8_000, 8_000, 0));
}

#[test]
fn credit_overspending_rolls_month_after_month_on_two_cards() {
    let mut h = house();
    let mc = h.b.account("Mastercard", "credit", 0);
    let (visa, mcp) = (h.pay(), payment_category(mc));
    h.b.assign("2026-09", h.groceries, 10_000);
    h.b.spend(h.card, "2026-09-03", "Market", Some(h.groceries), -6_000);
    h.b.spend(mc, "2026-09-04", "Market", Some(h.groceries), -7_000);
    // $100 covers $130 of card spending: $30 is debt, laid on the cards in
    // their order, and each moves what was covered of it.
    let g = h.b.line("2026-09", h.groceries);
    assert_eq!((g.available, g.credit_overspent), (-3_000, 3_000));
    let (v, m) = (h.b.line("2026-09", visa), h.b.line("2026-09", mcp));
    assert_eq!(v.activity + m.activity, 10_000);
    assert_eq!(v.underfunded + m.underfunded, 3_000);
    // October, November: groceries at nothing, the budget's money whole,
    // the debt still not covered.
    for month in ["2026-10", "2026-11"] {
        assert_eq!(h.b.line(month, h.groceries).available, 0);
        let s = h.b.summary(month);
        assert_eq!((s.ready_to_assign, s.overspent_last_month), (290_000, 0), "{month}");
        let (v, m) = (h.b.line(month, visa), h.b.line(month, mcp));
        assert_eq!((v.underfunded + m.underfunded, v.available + m.available), (3_000, 10_000), "{month}");
    }
    // Overspent again in October on the card: still not November's money.
    h.b.spend(h.card, "2026-10-09", "Market", Some(h.groceries), -1_000);
    assert_eq!(h.b.summary("2026-11").ready_to_assign, 290_000);
    assert_eq!(h.b.line("2026-11", visa).debt + h.b.line("2026-11", mcp).debt, 14_000);
}

#[test]
fn the_cards_group_and_its_categories_are_the_cards() {
    let mut h = house();
    let pay = h.pay();
    let rename = RenameCategory {
        id: pay,
        name: "Mine".into(),
    };
    assert!(h.b.run("rename_category", &rename).unwrap_err().contains("the card's"));
    let delete = DeleteCategory { id: pay, move_to: None };
    assert!(h.b.run("delete_category", &delete).unwrap_err().contains("the card's"));
    let rename = RenameGroup {
        id: card_group(),
        name: "Debts".into(),
    };
    assert!(h.b.run("rename_group", &rename).unwrap_err().contains("the cards'"));
    let delete = DeleteGroup {
        id: card_group(),
        move_to: None,
    };
    assert!(h.b.run("delete_group", &delete).unwrap_err().contains("the cards'"));
    let add = AddCategory {
        group_id: card_group(),
        name: "Loan".into(),
    };
    assert!(h.b.run("add_category", &add).unwrap_err().contains("the cards'"));
    let into = MoveCategory {
        id: h.groceries,
        group_id: card_group(),
        before: None,
    };
    assert!(h.b.run("move_category", &into).unwrap_err().contains("stays with the cards"));
    let out = MoveCategory {
        id: pay,
        group_id: h.b.line("2026-09", h.groceries).group_id,
        before: None,
    };
    assert!(h.b.run("move_category", &out).unwrap_err().contains("stays with the cards"));
    // Hidden, given a target, money assigned: like any category.
    h.b.run("hide_category", &HideCategory { id: pay, hidden: true }).unwrap();
    assert!(h.b.line("2026-09", pay).hidden);
    // The card renamed renames it; closed, it is hidden; opened, it is back.
    let card = h.card;
    h.b.run(
        "rename_account",
        &RenameAccount {
            id: card,
            name: "Sapphire".into(),
        },
    )
    .unwrap();
    let p = h.b.line("2026-09", pay);
    assert_eq!((p.name.as_str(), p.hidden), ("Sapphire", false));
    h.b.run("close_account", &CloseAccount { id: card, closed: true }).unwrap();
    assert!(h.b.line("2026-09", pay).hidden);
    h.b.run("close_account", &CloseAccount { id: card, closed: false }).unwrap();
    assert!(!h.b.line("2026-09", pay).hidden);
    // Paying a card is not spending, in the month's spending by group.
    h.b.assign("2026-09", h.groceries, 10_000);
    h.b.spend(card, "2026-09-05", "Market", Some(h.groceries), -5_000);
    h.b.spend(h.checking, "2026-09-06", "Sapphire payment", Some(pay), -5_000);
    let spending: Vec<SpendingLine> = h.b.list("spending", &Month { month: "2026-09".into() });
    let names: Vec<(&str, i64)> = spending.iter().map(|s| (s.group.as_str(), s.spent)).collect();
    assert_eq!(names, [("Everyday", 5_000)]);
}

// * Card payments from both banks

fn bank_account(id: &str, name: &str, kind: &str, subtype: &str) -> BankAccount {
    BankAccount {
        plaid_account_id: id.into(),
        name: name.into(),
        mask: "0000".into(),
        kind: kind.into(),
        subtype: subtype.into(),
        balance: None,
        called: String::new(),
        ignored: false,
    }
}

fn bank_txn(id: &str, account: &str, date: &str, payee: &str, amount: i64) -> BankTxn {
    BankTxn {
        transaction_id: id.into(),
        plaid_account_id: account.into(),
        pending_transaction_id: None,
        date: date.into(),
        amount,
        payee: payee.into(),
        imported_payee: payee.to_uppercase(),
        pending: false,
    }
}

fn import(bank: &str, accounts: Vec<BankAccount>, upserts: Vec<BankTxn>) -> Import {
    Import {
        bank: bank.into(),
        institution: bank.into(),
        cursor: format!("{bank}-1"),
        accounts,
        upserts,
        removed: vec![],
        choose: false,
        from_start: false,
    }
}

/// Two banks: checking at one, the card at the other, each opened by its
/// first import.
fn banks(b: &mut Book) -> (Id<Account>, Id<Account>) {
    let checking = bank_account("chk", "Everyday Checking", "depository", "checking");
    let card = bank_account("card", "Chase Freedom", "credit", "credit card");
    b.import(&import("ally", vec![checking], vec![])).unwrap();
    b.import(&import("chase", vec![card], vec![])).unwrap();
    let a = b.accounts();
    let id = |n: &str| a.iter().find(|x| x.name == n).unwrap().id;
    (id("Everyday Checking"), id("Chase Freedom"))
}

#[test]
fn a_card_payment_the_banks_tell_of_from_both_ends_is_one_transfer() {
    let mut b = Book::new();
    let (checking, card) = banks(&mut b);
    let ally = |b: &mut Book, t| {
        b.import(&import(
            "ally",
            vec![bank_account("chk", "Everyday Checking", "depository", "checking")],
            vec![t],
        ))
    };
    let chase = |b: &mut Book, t| {
        b.import(&import(
            "chase",
            vec![bank_account("card", "Chase Freedom", "credit", "credit card")],
            vec![t],
        ))
    };
    chase(&mut b, bank_txn("c1", "card", "2026-10-01", "Market", -12_000)).unwrap();
    ally(&mut b, bank_txn("a1", "chk", "2026-10-05", "Payment to Chase", -12_000)).unwrap();
    // One bank first: nothing to join yet.
    assert!(b.register(Some(checking)).iter().all(|t| !t.transfer));
    chase(&mut b, bank_txn("c2", "card", "2026-10-07", "Payment Thank You", 12_000)).unwrap();
    let out = b.register(Some(checking)).into_iter().find(|t| t.amount == -12_000).unwrap();
    let back = b.register(Some(card)).into_iter().find(|t| t.amount == 12_000).unwrap();
    assert_eq!(
        (out.payee.as_str(), out.transfer, out.category.as_str()),
        ("Transfer : Chase Freedom", true, "")
    );
    assert_eq!((back.payee.as_str(), back.transfer), ("Transfer : Everyday Checking", true));
    assert!(!out.approved && !back.approved, "both still wait to be looked at");
    assert!(out.imported && back.imported);
    let rows: Vec<Txn> = b.rows("txn");
    let row = |id| rows.iter().find(|t| t.id == id).unwrap();
    assert_eq!((row(out.id).transfer_id, row(back.id).transfer_id), (Some(back.id), Some(out.id)));
    // The payment is the payment category's spending.
    assert_eq!(b.line("2026-10", payment_category(card)).activity, -12_000);
    // Neither needs a category now.
    let inbox: Vec<TxnLine> = b.list("inbox", &());
    assert!(inbox.iter().filter(|t| t.transfer).all(|t| t.category != "Needs a category"));

    // The bank taking one half back leaves the other as the bank said it.
    let mut gone = import("ally", vec![], vec![]);
    gone.removed = vec![BankRemoved {
        transaction_id: "a1".into(),
        plaid_account_id: "chk".into(),
    }];
    b.import(&gone).unwrap();
    let back = b.register(Some(card)).into_iter().find(|t| t.amount == 12_000).unwrap();
    assert_eq!(
        (back.payee.as_str(), back.transfer, back.category.as_str()),
        ("Payment Thank You", false, "Needs a category")
    );
}

#[test]
fn both_halves_in_one_import_are_one_transfer() {
    let mut b = Book::new();
    let accounts = vec![
        bank_account("chk", "Chase Checking", "depository", "checking"),
        bank_account("card", "Chase Sapphire", "credit", "credit card"),
    ];
    let pay = vec![
        bank_txn("p1", "chk", "2026-10-05", "Payment to Chase card", -30_000),
        bank_txn("p2", "card", "2026-10-05", "Payment Thank You", 30_000),
    ];
    b.import(&import("chase", accounts, pay)).unwrap();
    let all = b.register(None);
    assert_eq!(all.iter().filter(|t| t.transfer).count(), 2);
}

#[test]
fn a_pair_is_not_joined_off_the_budget_or_for_another_amount_or_too_far_apart() {
    let mut b = Book::new();
    let (checking, card) = banks(&mut b);
    let loan = bank_account("loan", "Car Loan", "loan", "auto");
    let chk = || bank_account("chk", "Everyday Checking", "depository", "checking");
    let crd = || bank_account("card", "Chase Freedom", "credit", "credit card");
    b.import(&import("ally", vec![chk(), loan.clone()], vec![])).unwrap();
    let ups = vec![
        // A loan payment: the loan is off the budget.
        bank_txn("a1", "chk", "2026-10-05", "Auto loan", -25_000),
        bank_txn("l1", "loan", "2026-10-05", "Payment", 25_000),
    ];
    b.import(&import("ally", vec![chk(), loan], ups)).unwrap();
    // Amounts that do not agree, and days too far apart.
    b.import(&import(
        "ally",
        vec![chk()],
        vec![bank_txn("a2", "chk", "2026-10-10", "Payment to Chase", -10_000)],
    ))
    .unwrap();
    b.import(&import(
        "chase",
        vec![crd()],
        vec![
            bank_txn("c1", "card", "2026-10-11", "Payment Thank You", 9_999),
            bank_txn("c2", "card", "2026-10-20", "Payment Thank You", 10_000),
        ],
    ))
    .unwrap();
    assert!(b.register(None).iter().all(|t| !t.transfer), "{:#?}", b.register(None));
    // One somebody already looked at is theirs, not the import's to join.
    b.import(&import(
        "ally",
        vec![chk()],
        vec![bank_txn("a3", "chk", "2026-10-21", "Payment to Chase", -4_000)],
    ))
    .unwrap();
    let a3 = b.register(Some(checking)).into_iter().find(|t| t.amount == -4_000).unwrap();
    b.run("approve", &Txns { ids: vec![a3.id] }).unwrap();
    b.import(&import(
        "chase",
        vec![crd()],
        vec![bank_txn("c3", "card", "2026-10-22", "Payment Thank You", 4_000)],
    ))
    .unwrap();
    assert!(b.register(Some(card)).iter().all(|t| !t.transfer));
}

#[test]
fn a_transaction_is_made_a_transfer_from_its_edit_form() {
    let mut h = house();
    let (checking, card) = (h.checking, h.card);
    let savings = h.b.account("Savings", "savings", 0);
    // Typed in as spending, then the card's own payment arrives: the edit
    // pairs the two rather than making a third.
    h.b.spend(checking, "2026-10-05", "Chase", Some(h.groceries), -20_000);
    h.b.spend(card, "2026-10-06", "Payment Thank You", None, 20_000);
    let typed = h.b.register(Some(checking)).into_iter().find(|t| t.amount == -20_000).unwrap();
    let edit = |id, transfer_to| EditTransaction {
        id,
        date: "2026-10-05".into(),
        payee: "Chase".into(),
        category_id: None,
        memo: "October".into(),
        amount: -20_000,
        transfer_to,
    };
    h.b.run("edit_transaction", &edit(typed.id, Some(card))).unwrap();
    let mine = h.b.register(Some(card));
    assert_eq!(mine.len(), 2, "the starting balance, and the payment: no third");
    let paid = mine.iter().find(|t| t.amount == 20_000).unwrap();
    assert_eq!(
        (paid.payee.as_str(), paid.transfer, paid.date.as_str()),
        ("Transfer : Checking", true, "2026-10-06")
    );
    let out = h.b.register(Some(checking)).into_iter().find(|t| t.id == typed.id).unwrap();
    assert_eq!((out.transfer, out.category_id, out.memo.as_str()), (true, None, "October"));
    assert_eq!(h.b.line("2026-10", payment_category(card)).activity, -20_000);
    assert_eq!(h.b.line("2026-10", h.groceries).activity, 0, "no longer groceries");
    // Editing it again, the transfer kept.
    let mut again = edit(typed.id, None);
    again.memo = "Paid".into();
    h.b.run("edit_transaction", &again).unwrap();
    assert!(h
        .b
        .register(Some(checking))
        .iter()
        .any(|t| t.id == typed.id && t.transfer && t.memo == "Paid"));
    // Nothing matching in savings: its other half is made there, and the
    // card's half goes — typed by hand, it was nobody's but the transfer's.
    // (One the bank told of would stay, waiting to be looked at.)
    h.b.run("edit_transaction", &edit(typed.id, Some(savings))).unwrap();
    let s = h.b.register(Some(savings));
    assert!(s.iter().any(|t| t.amount == 20_000 && t.transfer && t.payee == "Transfer : Checking"));
    let left = h.b.register(Some(card));
    assert_eq!(left.len(), 1, "the card's half was made for the transfer: gone");
    // To itself is refused.
    let e = h.b.run("edit_transaction", &edit(typed.id, Some(checking))).unwrap_err();
    assert_eq!(e, "a transfer is between two accounts");
}
