//! Filters on a household: the register for any filter and its total, the
//! built-in ones the same as the registers and the inbox were, and saved
//! filters kept, renamed, changed, put in order and deleted.

mod common;

use common::Book;
use money_domain::budget::ready_to_assign;
use money_domain::plaid::{import_of, PlaidAccount, Pulled};
use money_domain::*;

/// A household with a bit of everything: paychecks, spending on and off
/// the budget, a transfer each way, something for no category, and the
/// bank's imports not yet looked at.
struct House {
    b: Book,
    checking: Id<Account>,
    savings: Id<Account>,
    loan: Id<Account>,
    groceries: Id<Category>,
    dining: Id<Category>,
}

fn house() -> House {
    let mut b = Book::new();
    let checking = b.account("Checking", "checking", 300_000);
    let savings = b.account("Savings", "savings", 50_000);
    let loan = b.account("Loan", "loan", -900_000);
    let everyday = b.group("Everyday");
    let groceries = b.category(everyday, "Groceries");
    let dining = b.category(everyday, "Dining");
    b.spend(checking, "2026-10-01", "ACME Payroll", Some(ready_to_assign()), 200_000);
    b.spend(checking, "2026-09-01", "ACME Payroll", Some(ready_to_assign()), 200_000);
    b.spend(checking, "2026-10-03", "Market", Some(groceries), -8_000);
    b.spend(savings, "2026-10-04", "Cafe", Some(dining), -1_500);
    b.spend(checking, "2026-10-06", "Mystery", None, -700);
    b.spend(loan, "2026-10-07", "Interest", None, -3_000);
    let to_savings = AddTransaction {
        account_id: checking,
        date: "2026-10-08".into(),
        amount: -20_000,
        transfer_to: Some(savings),
        ..AddTransaction::default()
    };
    b.run("add_transaction", &to_savings).unwrap();
    let to_loan = AddTransaction {
        account_id: checking,
        date: "2026-10-09".into(),
        amount: -10_000,
        category_id: Some(groceries),
        transfer_to: Some(loan),
        ..AddTransaction::default()
    };
    b.run("add_transaction", &to_loan).unwrap();
    let accounts: Vec<PlaidAccount> = serde_json::from_str(
        r#"[{"account_id":"a1","name":"Bank Card","mask":"4321","type":"credit","subtype":"credit card","balances":{"current":40.0}}]"#,
    )
    .unwrap();
    let pulled: Pulled = serde_json::from_str(
        r#"{"added":[{"transaction_id":"x1","account_id":"a1","amount":25.0,"date":"2026-10-10","name":"ACME STORE 12","merchant_name":"Acme Store"},
                     {"transaction_id":"x2","account_id":"a1","amount":15.0,"date":"2026-10-11","name":"MARKET 9","merchant_name":"Market"}],
            "modified":[],"removed":[],"cursor":"c1"}"#,
    )
    .unwrap();
    b.import(&import_of("bank", "Bank", &accounts, &pulled)).unwrap();
    House {
        b,
        checking,
        savings,
        loan,
        groceries,
        dining,
    }
}

impl House {
    fn filtered(&self, filter: Filter, month: Option<&str>) -> Vec<TxnLine> {
        let f = Filtered {
            saved: None,
            filter,
            month: month.map(str::to_string),
        };
        self.b.list("filtered", &f)
    }

    fn total(&self, input: &Filtered) -> FilterTotal {
        self.b.list::<FilterTotal>("filtered_total", input).pop().unwrap_or_default()
    }

    fn payees(&self, filter: Filter) -> Vec<String> {
        let mut p: Vec<String> = self.filtered(filter, None).into_iter().map(|t| t.payee).collect();
        p.sort();
        p
    }

    fn save(&mut self, name: &str, filter: Filter) -> Result<Id<SavedFilter>, String> {
        self.b.run(
            "save_filter",
            &SaveFilter {
                name: name.into(),
                filter,
            },
        )?;
        Ok(self.saved().into_iter().find(|f| f.name == name).unwrap().id)
    }

    fn saved(&self) -> Vec<FilterLine> {
        self.b.list("saved", &())
    }
}

/// Falsified by the built-in filters reading anything but what the
/// registers and the inbox always showed: every transaction, one
/// account's, and what was not approved or says "Needs a category".
#[test]
fn the_built_in_filters_are_the_registers_and_the_inbox() {
    let h = house();
    let every = h.filtered(Filter::all(), None);
    assert_eq!(every.len(), h.b.rows::<Txn>("txn").len());
    assert_eq!(every, h.b.register(None));
    for a in h.b.accounts() {
        let one = h.filtered(Filter::account(a.id), None);
        let was: Vec<TxnLine> = every.iter().filter(|t| t.account_id == a.id).cloned().collect();
        assert_eq!(one, was, "{}", a.name);
        assert_eq!(one, h.b.register(Some(a.id)), "{}", a.name);
        // Its total is the account's balance.
        let t = h.total(&Filtered {
            filter: Filter::account(a.id),
            ..Filtered::default()
        });
        assert_eq!(t.total, a.balance, "{}", a.name);
    }
    let look = h.filtered(Filter::needs_a_look(), None);
    let was: Vec<TxnLine> = every
        .iter()
        .filter(|t| !t.approved || t.category == "Needs a category")
        .cloned()
        .collect();
    assert_eq!(look, was);
    assert_eq!(look, h.b.list::<TxnLine>("inbox", &()));
    // The mystery spending, and the bank's two (a card is on the budget,
    // and the import filed nothing): not the loan's interest, which is
    // tracked, nor the transfer to savings.
    let mut payees: Vec<&str> = look.iter().map(|t| t.payee.as_str()).collect();
    payees.sort();
    assert_eq!(payees, ["Acme Store", "Market", "Mystery"]);
}

#[test]
fn every_condition_narrows_the_register() {
    let h = house();
    let all = Filter::all();
    // Accounts, and on the budget only.
    let two = Filter {
        accounts: vec![h.savings, h.loan],
        ..all.clone()
    };
    let lines = h.filtered(two.clone(), None);
    assert!(lines.iter().all(|t| t.account_id == h.savings || t.account_id == h.loan));
    assert!(lines.iter().any(|t| t.payee == "Interest") && lines.iter().any(|t| t.payee == "Cafe"));
    let on = Filter {
        on_budget: true,
        ..two.clone()
    };
    assert!(h.filtered(on, None).iter().all(|t| t.account_id == h.savings));
    // Text: the payee or the bank's line, any case.
    let acme = Filter {
        text: "acme".into(),
        ..all.clone()
    };
    assert_eq!(h.payees(acme), ["ACME Payroll", "ACME Payroll", "Acme Store"]);
    let line = Filter {
        text: "market 9".into(),
        ..all.clone()
    };
    assert_eq!(h.payees(line), ["Market"], "the bank's line");
    // Categories, and no category.
    let groceries = Filter {
        categories: vec![Some(h.groceries)],
        ..all.clone()
    };
    let g = h.filtered(groceries, None);
    // The bank's Market, filed as the last one was; the transfer to the
    // loan, its budget side; the market typed in.
    assert_eq!(g.iter().map(|t| t.amount).collect::<Vec<_>>(), [-1_500, -10_000, -8_000]);
    let none = Filter {
        categories: vec![None],
        on_budget: true,
        no_transfers: true,
        ..all.clone()
    };
    // The bank's Market took the category its payee had last time.
    assert_eq!(h.payees(none), ["Acme Store", "Mystery"]);
    let either = Filter {
        categories: vec![Some(h.dining), None],
        accounts: vec![h.savings],
        ..all.clone()
    };
    // Savings' cafe, its starting balance (Ready to Assign is a category,
    // so not that), and the transfer in from checking (for none).
    let e = h.filtered(either, None);
    assert_eq!(e.iter().map(|t| t.amount).collect::<Vec<_>>(), [20_000, -1_500]);
    // Which way.
    let out = Filter {
        direction: Direction::Out,
        accounts: vec![h.checking],
        ..all.clone()
    };
    assert!(h.filtered(out.clone(), None).iter().all(|t| t.amount < 0));
    let inn = Filter {
        direction: Direction::In,
        accounts: vec![h.checking],
        ..all.clone()
    };
    assert!(h.filtered(inn, None).iter().all(|t| t.amount > 0));
    // Transfers left out.
    let no = Filter {
        no_transfers: true,
        ..out.clone()
    };
    assert_eq!(h.filtered(out, None).len(), h.filtered(no.clone(), None).len() + 2);
    assert!(h.filtered(no, None).iter().all(|t| !t.transfer));
    // Dates, and a month.
    let oct = h.filtered(all.clone(), Some("2026-10"));
    assert!(oct.iter().all(|t| t.date.starts_with("2026-10")));
    let sep = h.filtered(all.clone(), Some("2026-09"));
    assert!(sep.iter().all(|t| t.date.starts_with("2026-09")) && !sep.is_empty());
    let days = Filter {
        from: Some("2026-10-03".into()),
        through: Some("2026-10-04".into()),
        ..all.clone()
    };
    assert_eq!(h.payees(days.clone()), ["Cafe", "Market"]);
    assert!(h.filtered(days, Some("2026-09")).is_empty(), "the month and its own days, both");
    // All of them at once: October's ACME paychecks into checking.
    let paychecks = Filter {
        accounts: vec![h.checking],
        on_budget: true,
        text: "acme".into(),
        categories: vec![Some(ready_to_assign())],
        direction: Direction::In,
        no_transfers: true,
        needs_look: false,
        from: Some("2026-10-01".into()),
        through: Some("2026-10-31".into()),
    };
    let p = h.filtered(paychecks.clone(), None);
    assert_eq!((p.len(), p[0].amount), (1, 200_000));
    // And the inbox's condition with them: it was looked at.
    assert!(h
        .filtered(
            Filter {
                needs_look: true,
                ..paychecks
            },
            None
        )
        .is_empty());
}

#[test]
fn a_registers_total_is_what_it_shows() {
    let h = house();
    for f in [
        Filter::all(),
        Filter::needs_a_look(),
        Filter::account(h.checking),
        Filter {
            direction: Direction::Out,
            ..Filter::all()
        },
    ] {
        let input = Filtered {
            filter: f.clone(),
            month: Some("2026-10".into()),
            ..Filtered::default()
        };
        let lines = h.filtered(f.clone(), Some("2026-10"));
        let t = h.total(&input);
        assert_eq!(t.count as usize, lines.len(), "{f:?}");
        assert_eq!(t.total, lines.iter().map(|l| l.amount).sum::<i64>());
        assert_eq!(t.inflow, lines.iter().map(|l| l.amount.max(0)).sum::<i64>());
        assert_eq!(t.outflow, lines.iter().map(|l| l.amount.min(0)).sum::<i64>());
        assert_eq!(t.total, t.inflow + t.outflow);
    }
    let nothing = Filtered {
        filter: Filter {
            text: "nobody".into(),
            ..Filter::all()
        },
        ..Filtered::default()
    };
    assert_eq!(h.total(&nothing), FilterTotal::default());
}

#[test]
fn saved_filters_are_kept_renamed_changed_ordered_and_deleted() {
    let mut h = house();
    let acme = h
        .save(
            "ACME",
            Filter {
                text: "  acme ".into(),
                ..Filter::all()
            },
        )
        .unwrap();
    let food = h
        .save(
            "Food",
            Filter {
                categories: vec![Some(h.dining), Some(h.groceries), Some(h.dining)],
                ..Filter::all()
            },
        )
        .unwrap();
    let lines = h.saved();
    assert_eq!(lines.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["ACME", "Food"]);
    // Kept tidy: the text trimmed, the categories once each, in order.
    assert_eq!(lines[0].filter.text, "acme");
    assert_eq!(lines[1].filter.categories.len(), 2);
    // Its total, ever: two paychecks and the store.
    assert_eq!((lines[0].count, lines[0].total), (3, 400_000 - 2_500));
    assert_eq!(lines[0].said, "\u{201c}acme\u{201d}");
    assert_eq!(lines[1].total, -8_000 - 1_500 - 10_000 - 1_500);
    // Its register, by the saved filter, follows a change to it.
    let by_id = Filtered {
        saved: Some(acme),
        ..Filtered::default()
    };
    assert_eq!(h.b.list::<TxnLine>("filtered", &by_id).len(), 3);
    let change = ChangeFilter {
        id: acme,
        filter: Filter {
            text: "acme".into(),
            direction: Direction::In,
            ..Filter::all()
        },
    };
    h.b.run("change_filter", &change).unwrap();
    assert_eq!(h.b.list::<TxnLine>("filtered", &by_id).len(), 2);
    assert_eq!(h.total(&by_id).total, 400_000);
    // A name is one filter's, any case.
    assert_eq!(h.save("acme", Filter::all()).unwrap_err(), "a filter is already called acme");
    let rename = RenameFilter {
        id: food,
        name: "ACME".into(),
    };
    assert!(h.b.run("rename_filter", &rename).is_err());
    h.b.run(
        "rename_filter",
        &RenameFilter {
            id: food,
            name: "Eating".into(),
        },
    )
    .unwrap();
    h.b.run(
        "rename_filter",
        &RenameFilter {
            id: acme,
            name: "Acme".into(),
        },
    )
    .unwrap();
    // Conditions on something not here are refused.
    let bad = ChangeFilter {
        id: food,
        filter: Filter {
            from: Some("October".into()),
            ..Filter::all()
        },
    };
    assert_eq!(h.b.run("change_filter", &bad).unwrap_err(), "a day, as 2026-10-26");
    let gone = SaveFilter {
        name: "Gone".into(),
        filter: Filter::account(Id::new([3; 16])),
    };
    assert_eq!(h.b.run("save_filter", &gone).unwrap_err(), "no such account");
    // Put in order: the third in front of the first, then the first last.
    let card = h.save("Card", Filter::needs_a_look()).unwrap();
    h.b.run(
        "move_filter",
        &MoveFilter {
            id: card,
            before: Some(acme),
        },
    )
    .unwrap();
    let names = |h: &House| h.saved().into_iter().map(|l| l.name).collect::<Vec<_>>();
    assert_eq!(names(&h), ["Card", "Acme", "Eating"]);
    h.b.run("move_filter", &MoveFilter { id: card, before: None }).unwrap();
    assert_eq!(names(&h), ["Acme", "Eating", "Card"]);
    // Deleted; its register is nothing.
    h.b.run("delete_filter", &FilterId { id: acme }).unwrap();
    assert_eq!(names(&h), ["Eating", "Card"]);
    assert!(h.b.list::<TxnLine>("filtered", &by_id).is_empty());
    assert_eq!(h.b.run("delete_filter", &FilterId { id: acme }).unwrap_err(), "no such filter");
}

/// A category deleted: a saved filter that asked for it asks for where
/// its transactions went.
#[test]
fn a_filters_category_deleted_follows_its_transactions() {
    let mut h = house();
    let food = h
        .save(
            "Food",
            Filter {
                categories: vec![Some(h.dining)],
                ..Filter::all()
            },
        )
        .unwrap();
    let before = h.saved()[0].total;
    let d = DeleteCategory {
        id: h.dining,
        move_to: Some(h.groceries),
    };
    h.b.run("delete_category", &d).unwrap();
    let after = h.saved().into_iter().find(|l| l.id == food).unwrap();
    assert_eq!(after.filter.categories, vec![Some(h.groceries)]);
    assert_eq!(after.total, before - 8_000 - 10_000 - 1_500);
}
