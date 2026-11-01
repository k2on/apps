//! Every view kept up to date is the query: after each change of a
//! household's month — assignments, spending, a transfer, an import, a
//! category gone — a maintained view's rows are a fresh run's, and the
//! patches it handed out splice the old rows into them.

mod common;

use ark::format::Encode;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::plan::splice;
use common::{domain, full, Book};
use money_domain::plaid::{import_of, PlaidAccount, Pulled};
use money_domain::*;

type Step = Box<dyn Fn(&mut Book)>;

/// A saved filter's id, by its name.
fn saved(b: &Book, name: &str) -> Id<SavedFilter> {
    b.rows::<SavedFilter>("saved_filter").into_iter().find(|f| f.name == name).unwrap().id
}

/// A month of a household, one change at a time.
fn script(b: &mut Book) -> Vec<Step> {
    let checking = b.account("Checking", "checking", 300_000);
    let card = b.account("Card", "credit", 0);
    let bills = b.group("Bills");
    let rent = b.category(bills, "Rent");
    let food = b.category(bills, "Food");
    vec![
        Box::new(move |b| b.assign("2026-10", rent, 150_000)),
        Box::new(move |b| b.assign("2026-10", food, 40_000)),
        Box::new(move |b| b.spend(checking, "2026-10-01", "Landlord", Some(rent), -150_000)),
        Box::new(move |b| b.spend(card, "2026-10-03", "Market", Some(food), -52_000)),
        Box::new(move |b| b.spend(checking, "2026-09-28", "Market", Some(food), -1_000)),
        Box::new(move |b| {
            let t = AddTransaction {
                account_id: checking,
                date: "2026-10-05".into(),
                amount: -52_000,
                transfer_to: Some(card),
                ..AddTransaction::default()
            };
            b.run("add_transaction", &t).unwrap();
        }),
        Box::new(move |b| {
            let m = MoveMoney {
                month: "2026-10".into(),
                from: Some(rent),
                to: Some(food),
                amount: 5_000,
            };
            b.run("move_money", &m).unwrap();
        }),
        // Rent assigned by formula from October: a share of ACME's pay into
        // checking, which comes before and after the formula does; then
        // half of what food spent with it, and the pay filter widened.
        Box::new(move |b| b.spend(checking, "2026-10-02", "ACME Payroll", Some(budget::ready_to_assign()), 200_000)),
        Box::new(move |b| {
            let f = SaveFilter {
                name: "Pay".into(),
                filter: Filter {
                    accounts: vec![checking],
                    text: "acme".into(),
                    direction: Direction::In,
                    ..Filter::default()
                },
            };
            b.run("save_filter", &f).unwrap();
        }),
        Box::new(move |b| {
            let f = SetFormula {
                category_id: rent,
                from: Some("2026-10".into()),
                terms: vec![Term {
                    filter_id: saved(b, "Pay"),
                    times: 3,
                    per: 4,
                }],
            };
            b.run("set_formula", &f).unwrap();
        }),
        Box::new(move |b| b.spend(checking, "2026-11-02", "ACME Payroll", Some(budget::ready_to_assign()), 210_000)),
        Box::new(move |b| {
            let f = SaveFilter {
                name: "Food".into(),
                filter: Filter {
                    categories: vec![Some(food)],
                    ..Filter::default()
                },
            };
            b.run("save_filter", &f).unwrap();
        }),
        Box::new(move |b| {
            let terms = vec![
                Term {
                    filter_id: saved(b, "Pay"),
                    times: 3,
                    per: 4,
                },
                Term {
                    filter_id: saved(b, "Food"),
                    times: 1,
                    per: 2,
                },
            ];
            let f = SetFormula {
                category_id: rent,
                from: Some("2026-11".into()),
                terms,
            };
            b.run("set_formula", &f).unwrap();
        }),
        Box::new(move |b| {
            let f = ChangeFilter {
                id: saved(b, "Pay"),
                filter: Filter {
                    text: "acme".into(),
                    direction: Direction::In,
                    ..Filter::default()
                },
            };
            b.run("change_filter", &f).unwrap();
        }),
        Box::new(move |b| {
            let c = ClearFormula {
                category_id: rent,
                from: Some("2026-12".into()),
            };
            b.run("clear_formula", &c).unwrap();
        }),
        // One change a step: what a view is pushed is the last change.
        Box::new(move |b| {
            let id = saved(b, "Food");
            b.run(
                "move_filter",
                &MoveFilter {
                    id,
                    before: Some(saved(b, "Pay")),
                },
            )
            .unwrap();
        }),
        Box::new(move |b| {
            let id = saved(b, "Food");
            b.run("rename_filter", &RenameFilter { id, name: "Eating".into() }).unwrap();
        }),
        Box::new(|b| {
            let accounts: Vec<PlaidAccount> = serde_json::from_str(
                r#"[{"account_id":"a1","name":"Bank Checking","mask":"4321","type":"depository","subtype":"checking","balances":{"current":120.0}}]"#,
            )
            .unwrap();
            let pulled: Pulled = serde_json::from_str(
                r#"{"added":[{"transaction_id":"x1","account_id":"a1","amount":30.0,"date":"2026-10-06","name":"MARKET","merchant_name":"Market"}],
                    "modified":[],"removed":[],"cursor":"c1"}"#,
            )
            .unwrap();
            b.import(&import_of("bank", "Bank", &accounts, &pulled)).unwrap();
        }),
        Box::new(|b| {
            let ids = b.register(None).iter().filter(|t| !t.approved).map(|t| t.id).collect();
            b.run("approve", &Txns { ids }).unwrap();
        }),
        Box::new(move |b| {
            b.run("set_target", &SetTarget { id: food, target: 60_000 }).unwrap();
        }),
        Box::new(move |b| {
            b.run(
                "rename_group",
                &RenameGroup {
                    id: bills,
                    name: "Monthly".into(),
                },
            )
            .unwrap();
        }),
        Box::new(move |b| {
            b.run(
                "auto_assign",
                &AutoAssign {
                    month: "2026-11".into(),
                    how: Auto::LastMonth,
                    only: vec![],
                },
            )
            .unwrap();
        }),
        Box::new(move |b| {
            let first = b.register(Some(checking)).last().unwrap().id;
            b.run("set_cleared", &SetCleared { id: first, cleared: false }).ok();
        }),
        Box::new(move |b| {
            b.run(
                "delete_category",
                &DeleteCategory {
                    id: rent,
                    move_to: Some(food),
                },
            )
            .unwrap();
        }),
        // A card that comes with debt, and a payment typed as spending
        // made a transfer to the card.
        Box::new(|b| {
            b.account("Amex", "credit", -20_000);
        }),
        Box::new(move |b| {
            let t = b.register(Some(checking)).into_iter().find(|t| t.amount == -1_000).unwrap();
            let e = EditTransaction {
                id: t.id,
                date: t.date,
                amount: t.amount,
                transfer_to: Some(card),
                ..EditTransaction::default()
            };
            b.run("edit_transaction", &e).unwrap();
        }),
        Box::new(move |b| {
            let ids = b.register(Some(card)).iter().filter(|t| t.transfer).map(|t| t.id).collect();
            b.run("delete_transactions", &Txns { ids }).unwrap();
        }),
    ]
}

/// The view `name` on `input`, held over the whole script.
fn hold(name: &str, input: &impl Encode) {
    let d = domain();
    let (f, input) = (full(name), input.to_bytes());
    let ctx = Ctx {
        id: EntryId::of("q", 0),
        actor: "bob".into(),
        session: "q".into(),
        ..Default::default()
    };
    let mut b = Book::new();
    let steps = script(&mut b);
    let mut v = d.watch(&b.store, &ctx, &f, &input).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut shown = v.rows();
    let mut moved = 0;
    for (n, step) in steps.iter().enumerate() {
        step(&mut b);
        let ps = v.push(&b.last, &b.store).unwrap_or_else(|e| panic!("{name} step {n}: {e}"));
        moved += !ps.is_empty() as usize;
        shown = splice(&ps, &shown);
        let fresh = d.watch(&b.store, &ctx, &f, &input).unwrap().rows();
        assert_eq!(v.rows(), fresh, "{name} step {n}: the view is the query");
        assert_eq!(shown, fresh, "{name} step {n}: the patches splice");
    }
    assert!(moved > 0, "{name}: the script moves it");
}

fn month(m: &str) -> Month {
    Month { month: m.into() }
}

#[test]
fn the_budget() {
    hold("month", &month("2026-10"));
    hold("month", &month("2026-11"));
    hold("summary", &month("2026-10"));
    hold("summary", &month("2026-11"));
    hold("groups", &());
    hold("spending", &month("2026-10"));
    hold("formulas", &month("2026-10"));
    hold("formulas", &month("2026-11"));
    // A preview's filter is whichever the script saves first.
    let mut b = Book::new();
    for step in script(&mut b) {
        step(&mut b);
    }
    let pay = b.rows::<SavedFilter>("saved_filter").into_iter().find(|f| f.name == "Pay").unwrap().id;
    let preview = PreviewFormula {
        month: "2026-11".into(),
        terms: vec![Term {
            filter_id: pay,
            times: 1,
            per: 10,
        }],
    };
    hold("preview_formula", &preview);
}

/// The saved filters, and the register for one, by its id and by its
/// conditions, in a month and not.
#[test]
fn the_filters() {
    hold("saved", &());
    let mut b = Book::new();
    for step in script(&mut b) {
        step(&mut b);
    }
    let pay = b.rows::<SavedFilter>("saved_filter").into_iter().find(|f| f.name == "Pay").unwrap().id;
    let by_id = Filtered {
        saved: Some(pay),
        ..Filtered::default()
    };
    hold("filtered", &by_id);
    hold("filtered_total", &by_id);
    let october = Filtered {
        month: Some("2026-10".into()),
        ..by_id
    };
    hold("filtered", &october);
    hold("filtered_total", &october);
    let spent = Filtered {
        filter: Filter {
            direction: Direction::Out,
            no_transfers: true,
            on_budget: true,
            ..Filter::default()
        },
        ..Filtered::default()
    };
    hold("filtered", &spent);
    hold("filtered_total", &spent);
    let look = Filtered {
        filter: Filter::needs_a_look(),
        ..Filtered::default()
    };
    hold("filtered", &look);
    hold("filtered_total", &look);
}

#[test]
fn the_accounts_and_their_transactions() {
    hold("accounts", &());
    hold("register", &Register { account_id: None });
    hold("inbox", &());
    hold("payees", &());
    hold("banks", &());
    hold("linked", &());
}
