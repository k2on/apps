//! The budget's rules, on a household's month: money in is ready to
//! assign, a category keeps what it was given until it is spent, and
//! overspending is paid for by the month after.

mod common;

use common::Book;
use money_domain::budget::ready_to_assign;
use money_domain::*;

/// A household: a checking account with $3,000 in it, and two categories.
fn household() -> (Book, Id<Account>, Id<Category>, Id<Category>) {
    let mut b = Book::new();
    let checking = b.account("Checking", "checking", 300_000);
    let bills = b.group("Bills");
    let rent = b.category(bills, "Rent");
    let groceries = b.category(bills, "Groceries");
    (b, checking, rent, groceries)
}

#[test]
fn a_starting_balance_is_ready_to_assign() {
    let (b, checking, ..) = household();
    let s = b.summary("2026-09");
    assert_eq!((s.ready_to_assign, s.income), (300_000, 300_000));
    let a = &b.accounts()[0];
    assert_eq!((a.id, a.balance, a.cleared, a.uncleared), (checking, 300_000, 300_000, 0));
    // A month before the money came has none to assign.
    assert_eq!(b.summary("2026-08").ready_to_assign, 0);
}

#[test]
fn assigning_takes_from_ready_to_assign_and_spending_from_the_category() {
    let (mut b, checking, rent, groceries) = household();
    b.assign("2026-09", rent, 150_000);
    b.assign("2026-09", groceries, 60_000);
    b.spend(checking, "2026-09-03", "Landlord", Some(rent), -150_000);
    b.spend(checking, "2026-09-10", "Market", Some(groceries), -12_345);
    let s = b.summary("2026-09");
    assert_eq!(s.ready_to_assign, 90_000);
    assert_eq!((s.assigned, s.activity, s.available), (210_000, -162_345, 47_655));
    let g = b.line("2026-09", groceries);
    assert_eq!((g.assigned, g.activity, g.available), (60_000, -12_345, 47_655));
    // Setting again is setting, not adding.
    b.assign("2026-09", groceries, 50_000);
    assert_eq!(b.summary("2026-09").ready_to_assign, 100_000);
}

#[test]
fn what_is_left_carries_and_what_is_short_comes_out_of_next_month() {
    let (mut b, checking, rent, groceries) = household();
    b.assign("2026-09", groceries, 40_000);
    b.spend(checking, "2026-09-10", "Market", Some(groceries), -30_000);
    // October: groceries carries $100 in and is overspent by $50.
    b.spend(checking, "2026-10-04", "Market", Some(groceries), -15_000);
    let oct = b.line("2026-10", groceries);
    assert_eq!((oct.carried, oct.available), (10_000, -5_000));
    // Rent was never given anything and is overspent too.
    b.spend(checking, "2026-10-01", "Landlord", Some(rent), -2_000);
    assert_eq!(b.summary("2026-10").overspent, 2);
    // November starts both at nothing, and its money is $70 less.
    let nov = b.line("2026-11", groceries);
    assert_eq!((nov.carried, nov.available), (0, 0));
    let s = b.summary("2026-11");
    assert_eq!((s.overspent_last_month, s.ready_to_assign), (-7_000, 300_000 - 40_000 - 7_000));
}

#[test]
fn money_moves_between_categories_and_back_to_be_assigned() {
    let (mut b, _, rent, groceries) = household();
    b.assign("2026-09", rent, 100_000);
    let m = MoveMoney {
        month: "2026-09".into(),
        from: Some(rent),
        to: Some(groceries),
        amount: 25_000,
    };
    b.run("move_money", &m).unwrap();
    assert_eq!((b.line("2026-09", rent).assigned, b.line("2026-09", groceries).assigned), (75_000, 25_000));
    let back = MoveMoney { to: None, ..m };
    b.run("move_money", &back).unwrap();
    assert_eq!(b.line("2026-09", rent).assigned, 50_000);
    assert_eq!(b.summary("2026-09").ready_to_assign, 300_000 - 75_000);
    // Ready to Assign is where money is assigned from, not a category to give it to.
    let wrong = Assign {
        month: "2026-09".into(),
        category_id: ready_to_assign(),
        amount: 1,
    };
    assert_eq!(b.run("assign", &wrong).unwrap_err(), "money is assigned from Ready to Assign, not to it");
}

#[test]
fn a_month_can_be_assigned_at_once() {
    let (mut b, _, rent, groceries) = household();
    b.assign("2026-09", rent, 150_000);
    b.run("set_target", &SetTarget { id: groceries, target: 60_000 }).unwrap();
    let auto = |how| AutoAssign {
        month: "2026-10".into(),
        how,
        only: vec![],
    };
    b.run("auto_assign", &auto(Auto::LastMonth)).unwrap();
    assert_eq!((b.line("2026-10", rent).assigned, b.line("2026-10", groceries).assigned), (150_000, 0));
    assert_eq!(b.summary("2026-10").underfunded, 1);
    b.run("auto_assign", &auto(Auto::Underfunded)).unwrap();
    assert_eq!(b.line("2026-10", groceries).assigned, 60_000);
    assert_eq!(b.summary("2026-10").underfunded, 0);
    b.run("auto_assign", &auto(Auto::Reset)).unwrap();
    assert_eq!(b.summary("2026-10").assigned, 0);
}

#[test]
fn a_transfer_is_two_transactions_kept_together() {
    let (mut b, checking, ..) = household();
    let savings = b.account("Savings", "savings", 0);
    let mortgage = b.account("Mortgage", "loan", -20_000_000);
    let t = AddTransaction {
        account_id: checking,
        date: "2026-09-15".into(),
        amount: -50_000,
        transfer_to: Some(savings),
        ..AddTransaction::default()
    };
    b.run("add_transaction", &t).unwrap();
    let mine = b.register(Some(savings));
    assert_eq!((mine[0].payee.as_str(), mine[0].amount, mine[0].transfer), ("Transfer : Checking", 50_000, true));
    // Between two accounts on budget no money was spent.
    assert_eq!(b.summary("2026-09").ready_to_assign, 300_000);
    // Editing one half moves the other.
    let edit = EditTransaction {
        id: b.register(Some(checking))[0].id,
        date: "2026-09-16".into(),
        amount: -60_000,
        ..EditTransaction::default()
    };
    b.run("edit_transaction", &edit).unwrap();
    assert_eq!(b.register(Some(savings))[0].amount, 60_000);
    // Out of the budget (a mortgage payment) it is spending, from a category.
    let bills = b.group("Housing");
    let mortgage_payment = b.category(bills, "Mortgage");
    let pay = AddTransaction {
        account_id: checking,
        date: "2026-09-20".into(),
        amount: -180_000,
        category_id: Some(mortgage_payment),
        transfer_to: Some(mortgage),
        ..AddTransaction::default()
    };
    b.run("add_transaction", &pay).unwrap();
    assert_eq!(b.line("2026-09", mortgage_payment).activity, -180_000);
    assert_eq!(b.accounts().iter().find(|a| a.id == mortgage).unwrap().balance, -20_000_000 + 180_000);
    // Deleting one half deletes both.
    let ids = vec![b.register(Some(savings))[0].id];
    b.run("delete_transactions", &Txns { ids }).unwrap();
    assert!(b.register(Some(savings)).iter().all(|t| !t.transfer));
    assert_eq!(b.register(Some(checking)).iter().filter(|t| t.transfer).count(), 1);
}

#[test]
fn an_account_off_the_budget_is_only_tracked() {
    let (mut b, _, rent, _) = household();
    let house = b.account("House", "investment", 40_000_000);
    assert_eq!(b.summary("2026-09").ready_to_assign, 300_000, "its balance is not the budget's");
    let t = AddTransaction {
        account_id: house,
        date: "2026-09-02".into(),
        amount: 100,
        category_id: Some(rent),
        ..AddTransaction::default()
    };
    assert!(b.run("add_transaction", &t).unwrap_err().contains("only tracked"));
}

#[test]
fn reconciling_agrees_with_the_bank_and_locks_what_it_agreed_to() {
    let (mut b, checking, rent, _) = household();
    b.spend(checking, "2026-09-03", "Landlord", Some(rent), -150_000);
    let first = b.register(Some(checking))[0].id;
    b.run("set_cleared", &SetCleared { id: first, cleared: true }).unwrap();
    b.spend(checking, "2026-09-04", "Corner shop", Some(rent), -500);
    // The bank says $1,499.00: a dollar more than what cleared here.
    let r = Reconcile {
        account_id: checking,
        balance: 149_900,
        date: "2026-09-30".into(),
    };
    b.run("reconcile", &r).unwrap();
    let lines = b.register(Some(checking));
    let adjustment = lines.iter().find(|l| l.payee == money_domain::transactions::ADJUSTMENT).unwrap();
    assert_eq!((adjustment.amount, adjustment.cleared), (-100, Cleared::Reconciled));
    let landlord = lines.iter().find(|l| l.id == first).unwrap();
    assert_eq!(landlord.cleared, Cleared::Reconciled);
    assert_eq!(lines.iter().find(|l| l.payee == "Corner shop").unwrap().cleared, Cleared::Uncleared);
    let a = &b.accounts()[0];
    assert_eq!(a.cleared, 149_900);
    // The bank agreed to it: its amount stays.
    let edit = EditTransaction {
        id: first,
        date: "2026-09-03".into(),
        amount: -1,
        ..EditTransaction::default()
    };
    assert_eq!(b.run("edit_transaction", &edit).unwrap_err(), "reconciled: its amount is agreed with the bank");
}

#[test]
fn a_deleted_category_hands_on_its_money_and_its_spending() {
    let (mut b, checking, rent, groceries) = household();
    b.assign("2026-09", rent, 100_000);
    b.assign("2026-09", groceries, 20_000);
    b.spend(checking, "2026-09-03", "Landlord", Some(rent), -90_000);
    b.run(
        "delete_category",
        &DeleteCategory {
            id: rent,
            move_to: Some(groceries),
        },
    )
    .unwrap();
    let g = b.line("2026-09", groceries);
    assert_eq!((g.assigned, g.activity, g.available), (120_000, -90_000, 30_000));
    assert_eq!(b.summary("2026-09").ready_to_assign, 180_000);
}

#[test]
fn everybody_in_the_household_keeps_the_same_budget() {
    let (mut b, checking, rent, _) = household();
    b.as_("bob", "assign", &Assign {
        month: "2026-09".into(),
        category_id: rent,
        amount: 10_000,
    })
    .unwrap();
    b.as_(
        "carol",
        "add_transaction",
        &AddTransaction {
            account_id: checking,
            date: "2026-09-02".into(),
            payee: "Landlord".into(),
            category_id: Some(rent),
            amount: -10_000,
            ..AddTransaction::default()
        },
    )
    .unwrap();
    // alice, bob and carol see one budget: alice's account, bob's money, carol's spending.
    assert_eq!(b.line("2026-09", rent).available, 0);
    assert_eq!(b.register(None).iter().find(|t| t.payee == "Landlord").unwrap().user_id, "carol");
}

#[test]
fn a_form_says_what_is_wrong_with_it() {
    let (mut b, checking, ..) = household();
    let bad = AddTransaction {
        account_id: checking,
        date: "2026-13-01".into(),
        ..AddTransaction::default()
    };
    assert!(b.run("add_transaction", &bad).unwrap_err().contains("a day, as 2026-10-26"));
    assert!(b.run("add_group", &AddGroup { name: "  ".into() }).unwrap_err().contains("a group needs a name"));
}
