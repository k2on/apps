//! Assignment by formula, on a household's months: a category given the
//! sum of its terms — a share of what a saved filter's transactions add up
//! to in the month — from the month the formula is set on (every month
//! before keeps what was typed), and that amount counted everywhere a
//! typed one would be. And the phones' old `set_rule`, kept as a formula.
// A test reads its fixtures and times itself: the kernel's and the core's
// purity (clippy.toml) is their library's, not their tests'.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

mod common;

use common::Book;
use money_core::budget::ready_to_assign;
use money_core::cards::payment_category;
use money_core::formulas::{parse_share, FOLLOWS};
use money_core::*;

/// A household: $3,000 in checking, savings and cash with nothing yet, a
/// card, and a giving category beside groceries.
struct House {
    b: Book,
    checking: Id<Account>,
    savings: Id<Account>,
    cash: Id<Account>,
    card: Id<Account>,
    giving: Id<Category>,
    groceries: Id<Category>,
}

fn house() -> House {
    let mut b = Book::new();
    let checking = b.account("Checking", "checking", 300_000);
    let savings = b.account("Savings", "savings", 0);
    let cash = b.account("Cash", "cash", 0);
    let card = b.account("Visa", "credit", 0);
    let everyday = b.group("Everyday");
    let giving = b.category(everyday, "Giving");
    let groceries = b.category(everyday, "Groceries");
    House {
        b,
        checking,
        savings,
        cash,
        card,
        giving,
        groceries,
    }
}

/// Money in that matches `text`, not a transfer, in these accounts.
fn income(text: &str, accounts: &[Id<Account>]) -> Filter {
    Filter {
        accounts: accounts.to_vec(),
        text: text.into(),
        direction: Direction::In,
        no_transfers: true,
        ..Filter::default()
    }
}

impl House {
    /// Money in, for Ready to Assign.
    fn paid(&mut self, account: Id<Account>, date: &str, payee: &str, amount: i64) {
        self.b.spend(account, date, payee, Some(ready_to_assign()), amount);
    }

    /// A filter saved under `name`, by its id.
    fn filter(&mut self, name: &str, filter: Filter) -> Id<SavedFilter> {
        self.b.run("save_filter", &SaveFilter { name: name.into(), filter }).unwrap();
        self.b
            .rows::<SavedFilter>("saved_filter")
            .into_iter()
            .find(|f| f.name == name)
            .unwrap()
            .id
    }

    fn terms(terms: &[(Id<SavedFilter>, &str)]) -> Vec<Term> {
        terms
            .iter()
            .map(|(f, share)| {
                let (times, per) = parse_share(share).expect("a share");
                Term { filter_id: *f, times, per }
            })
            .collect()
    }

    fn formula(&mut self, from: Option<&str>, terms: &[(Id<SavedFilter>, &str)]) -> Result<usize, String> {
        let f = SetFormula {
            category_id: self.giving,
            from: from.map(str::to_string),
            terms: House::terms(terms),
        };
        self.b.run("set_formula", &f)
    }

    fn clear(&mut self, from: Option<&str>) -> Result<usize, String> {
        let c = ClearFormula {
            category_id: self.giving,
            from: from.map(str::to_string),
        };
        self.b.run("clear_formula", &c)
    }

    fn giving(&self, month: &str) -> BudgetLine {
        self.b.line(month, self.giving)
    }

    fn preview(&self, month: &str, terms: &[(Id<SavedFilter>, &str)]) -> FormulaPreview {
        let p = PreviewFormula {
            month: month.into(),
            terms: House::terms(terms),
        };
        self.b.list::<FormulaPreview>("preview_formula", &p).pop().expect("a preview")
    }

    fn versions(&self) -> Vec<(String, bool)> {
        self.b.rows::<Formula>("formula").into_iter().map(|f| (f.from, f.manual)).collect()
    }
}

/// October's paychecks: ACME into checking and savings, ACME into cash
/// (not one of the filter's), another employer into checking, and ACME
/// taking money back.
fn october(h: &mut House) {
    let (checking, savings, cash) = (h.checking, h.savings, h.cash);
    h.paid(checking, "2026-10-01", "ACME Payroll", 200_000);
    h.paid(savings, "2026-10-01", "acme payroll", 50_000);
    h.paid(cash, "2026-10-02", "ACME PAYROLL", 100_000);
    h.paid(checking, "2026-10-15", "Side Gig", 30_000);
    h.b.spend(checking, "2026-10-20", "ACME Store", None, -5_000);
}

#[test]
fn a_formula_sums_its_filter_over_the_month() {
    let mut h = house();
    october(&mut h);
    let (checking, savings) = (h.checking, h.savings);
    let acme = h.filter("ACME paychecks", income("acme", &[checking, savings]));
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    // $2,000 + $500 matched; the cash account's $1,000, the side gig and
    // the outflow did not.
    let g = h.giving("2026-10");
    assert_eq!((g.assigned, g.available, g.ruled), (25_000, 25_000, true));
    assert_eq!(g.rule, "10% \u{d7} SUM(ACME paychecks)");
    let s = h.b.summary("2026-10");
    assert_eq!(s.ready_to_assign, 300_000 + 380_000 - 25_000);
    assert_eq!(s.assigned, 25_000);
    // The filter widened to every account: the formula follows it.
    let wide = ChangeFilter {
        id: acme,
        filter: income("acme", &[]),
    };
    h.b.run("change_filter", &wide).unwrap();
    assert_eq!(h.giving("2026-10").assigned, 35_000);
    // Another month's paychecks give this one nothing.
    assert_eq!(h.giving("2026-11").assigned, 0);
    assert!(h.giving("2026-11").ruled);
}

#[test]
fn terms_add_and_the_month_is_rounded_once() {
    let mut h = house();
    let (checking, savings) = (h.checking, h.savings);
    h.paid(checking, "2026-10-01", "ACME", 100_005);
    h.paid(checking, "2026-10-15", "ACME", 100_000);
    h.paid(savings, "2026-10-03", "Side Gig", 10_001);
    let acme = h.filter("ACME", income("acme", &[checking]));
    let side = h.filter("Side", income("side", &[]));
    for (share, want) in [
        ("10%", 20_001),
        ("0.1", 20_001),
        ("1.5", 300_008),
        ("1.5x", 300_008),
        ("12.5%", 25_001),
        ("0", 0),
    ] {
        h.formula(Some("2026-10"), &[(acme, share)]).unwrap();
        // $2,000.05 a month: 10% is $200.005, $200.01.
        assert_eq!(h.giving("2026-10").assigned, want, "{share}");
        assert_eq!(h.preview("2026-10", &[(acme, share)]).amount, want, "{share}");
    }
    let f = h.b.rows::<Formula>("formula").pop().unwrap();
    assert_eq!((f.terms[0].times, f.terms[0].per), (0, 1));
    // Two terms: 10% of $2,000.05 and 50% of $100.01 are $200.005 and
    // $50.005 — $250.01 together, where rounding each would say $250.02.
    h.formula(Some("2026-10"), &[(acme, "10%"), (side, "50%")]).unwrap();
    let g = h.giving("2026-10");
    assert_eq!(g.assigned, 25_001);
    assert_eq!(g.rule, "10% \u{d7} SUM(ACME) + 50% \u{d7} SUM(Side)");
    // A factor of one is the sum itself.
    h.formula(Some("2026-10"), &[(side, "100%")]).unwrap();
    assert_eq!((h.giving("2026-10").assigned, h.giving("2026-10").rule.as_str()), (10_001, "SUM(Side)"));
    // Money out counts as it is: a filter of spending gives less.
    let spent = h.filter(
        "Spent",
        Filter {
            direction: Direction::Out,
            ..Filter::default()
        },
    );
    let groceries = h.groceries;
    h.b.spend(checking, "2026-10-20", "Market", Some(groceries), -4_000);
    h.formula(Some("2026-10"), &[(side, "1"), (spent, "1")]).unwrap();
    assert_eq!(h.giving("2026-10").assigned, 10_001 - 4_000);
}

#[test]
fn a_formula_holds_from_its_month_and_earlier_months_keep_theirs() {
    let mut h = house();
    let (checking, giving) = (h.checking, h.giving);
    h.paid(checking, "2026-09-01", "ACME", 100_000);
    h.paid(checking, "2026-10-01", "ACME", 200_000);
    h.paid(checking, "2026-11-01", "ACME", 300_000);
    h.b.assign("2026-09", giving, 5_000);
    // Typed in October, before the formula: the formula, not this, holds.
    h.b.assign("2026-10", giving, 7_000);
    let acme = h.filter("ACME", income("acme", &[checking]));
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    let (sep, oct, nov) = (h.giving("2026-09"), h.giving("2026-10"), h.giving("2026-11"));
    assert_eq!((sep.assigned, sep.ruled, sep.rule.as_str()), (5_000, false, ""));
    assert_eq!((oct.assigned, oct.ruled), (20_000, true));
    assert_eq!((nov.assigned, nov.ruled), (30_000, true));
    assert_eq!((oct.carried, oct.available), (5_000, 25_000));
    assert_eq!((nov.carried, nov.available), (25_000, 55_000));
    let s = h.b.summary("2026-11");
    assert_eq!(s.ready_to_assign, 300_000 + 600_000 - 5_000 - 20_000 - 30_000);
    // None said is this month: the book's today is 2026-10-26.
    let mut h2 = house();
    let checking = h2.checking;
    let acme = h2.filter("ACME", income("acme", &[checking]));
    h2.formula(None, &[(acme, "10%")]).unwrap();
    assert_eq!(h2.b.rows::<Formula>("formula")[0].from, "2026-10");
}

#[test]
fn a_formula_changed_and_removed_mid_history() {
    let mut h = house();
    let (checking, savings, giving) = (h.checking, h.savings, h.giving);
    for m in ["08", "09", "10", "11", "12"] {
        h.paid(checking, &format!("2026-{m}-01"), "ACME", 100_000);
        h.paid(savings, &format!("2026-{m}-02"), "ACME", 10_000);
    }
    let chk = h.filter("Checking pay", income("acme", &[checking]));
    let sav = h.filter("Savings pay", income("acme", &[savings]));
    h.b.assign("2026-08", giving, 1_000);
    h.formula(Some("2026-09"), &[(chk, "10%")]).unwrap();
    // Changed in October: savings' too, and 20%.
    h.formula(Some("2026-10"), &[(chk, "20%"), (sav, "20%")]).unwrap();
    h.clear(Some("2026-12")).unwrap();
    let assigned: Vec<(i64, bool)> = ["2026-08", "2026-09", "2026-10", "2026-11", "2026-12"]
        .iter()
        .map(|m| {
            let l = h.giving(m);
            (l.assigned, l.ruled)
        })
        .collect();
    assert_eq!(assigned, vec![(1_000, false), (10_000, true), (22_000, true), (22_000, true), (0, false)]);
    h.b.assign("2026-12", giving, 3_000);
    assert_eq!(h.giving("2026-12").assigned, 3_000);
    assert_eq!(h.giving("2026-12").available, 1_000 + 10_000 + 22_000 + 22_000 + 3_000);
    assert_eq!(
        h.versions(),
        vec![("2026-09".into(), false), ("2026-10".into(), false), ("2026-12".into(), true)]
    );
    // Set again from September: what followed it is replaced.
    h.formula(Some("2026-09"), &[(chk, "5%")]).unwrap();
    assert_eq!(h.versions().len(), 1);
    assert_eq!((h.giving("2026-12").assigned, h.giving("2026-12").ruled), (5_000, true));
    // Removed from October: September keeps its share, December its typed.
    h.clear(Some("2026-10")).unwrap();
    assert_eq!(h.giving("2026-09").assigned, 5_000);
    assert_eq!((h.giving("2026-10").assigned, h.giving("2026-10").ruled), (0, false));
    assert_eq!(h.giving("2026-12").assigned, 3_000);
    // Removing what is not there says so; the same formula again is nothing.
    assert_eq!(h.clear(Some("2026-11")).unwrap_err(), "this category is assigned by hand already");
    h.formula(Some("2026-10"), &[(chk, "5%")]).unwrap();
    assert_eq!(h.versions().len(), 1, "the same formula as September's is not a version");
    h.clear(Some("2026-09")).unwrap();
    assert!(h.versions().is_empty());
    assert_eq!(h.giving("2026-09").assigned, 0);
}

#[test]
fn a_month_that_follows_a_formula_is_not_assigned_by_hand() {
    let mut h = house();
    let (checking, giving, groceries) = (h.checking, h.giving, h.groceries);
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    h.b.assign("2026-09", giving, 2_000);
    h.b.assign("2026-09", groceries, 4_000);
    let acme = h.filter("ACME", income("acme", &[checking]));
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    let assign = Assign {
        month: "2026-10".into(),
        category_id: giving,
        amount: 1,
    };
    assert_eq!(h.b.run("assign", &assign).unwrap_err(), FOLLOWS);
    assert!(FOLLOWS.contains("follows its rule"), "the words the phones look for");
    h.b.run(
        "assign",
        &Assign {
            month: "2026-09".into(),
            ..assign.clone()
        },
    )
    .unwrap();
    let moved = |from, to| MoveMoney {
        month: "2026-10".into(),
        from,
        to,
        amount: 500,
    };
    assert_eq!(h.b.run("move_money", &moved(Some(giving), Some(groceries))).unwrap_err(), FOLLOWS);
    assert_eq!(h.b.run("move_money", &moved(Some(groceries), Some(giving))).unwrap_err(), FOLLOWS);
    assert_eq!(h.b.run("move_money", &moved(None, Some(giving))).unwrap_err(), FOLLOWS);
    assert_eq!(h.b.run("move_money", &moved(Some(giving), None)).unwrap_err(), FOLLOWS);
    let auto = |how| AutoAssign {
        month: "2026-10".into(),
        how,
        only: vec![],
    };
    h.b.run("auto_assign", &auto(Auto::LastMonth)).unwrap();
    assert_eq!((h.giving("2026-10").assigned, h.b.line("2026-10", groceries).assigned), (10_000, 4_000));
    h.b.run("auto_assign", &auto(Auto::Reset)).unwrap();
    assert_eq!((h.giving("2026-10").assigned, h.b.line("2026-10", groceries).assigned), (10_000, 0));
    // Last month's, read in November after the formula stops: its amount.
    h.clear(Some("2026-11")).unwrap();
    let nov = AutoAssign {
        month: "2026-11".into(),
        how: Auto::LastMonth,
        only: vec![giving],
    };
    h.b.run("auto_assign", &nov).unwrap();
    assert_eq!(h.giving("2026-11").assigned, 10_000);
}

#[test]
fn overspending_and_cards_count_the_formulas_amount() {
    let mut h = house();
    let (checking, card, giving) = (h.checking, h.card, h.giving);
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    let acme = h.filter("ACME", income("acme", &[checking]));
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    // $150 on the card: $100 covered by the formula's amount, $50 debt.
    h.b.spend(card, "2026-10-05", "Charity", Some(giving), -15_000);
    let pay = h.b.line("2026-10", payment_category(card));
    assert_eq!((pay.activity, pay.available, pay.debt, pay.underfunded), (10_000, 10_000, 15_000, 5_000));
    assert_eq!(h.giving("2026-10").credit_overspent, 5_000);
    assert_eq!(h.b.summary("2026-11").ready_to_assign, 300_000 + 100_000 - 10_000);
    // A card's payment category and Ready to Assign have no formula.
    let pay_formula = SetFormula {
        category_id: payment_category(card),
        from: None,
        terms: House::terms(&[(acme, "10%")]),
    };
    assert_eq!(
        h.b.run("set_formula", &pay_formula).unwrap_err(),
        "a card's payment category is assigned by hand: its money is the card's"
    );
    let rta = SetFormula {
        category_id: ready_to_assign(),
        ..pay_formula.clone()
    };
    assert_eq!(
        h.b.run("set_formula", &rta).unwrap_err(),
        "money is assigned from Ready to Assign, not to it"
    );
    // Nothing summed, a filter not here, a factor below nothing.
    let none = SetFormula {
        category_id: giving,
        terms: vec![],
        ..pay_formula.clone()
    };
    assert_eq!(h.b.run("set_formula", &none).unwrap_err(), "a formula sums at least one filter");
    let gone = SetFormula {
        category_id: giving,
        terms: vec![Term {
            filter_id: Id::new([9; 16]),
            times: 1,
            per: 10,
        }],
        ..pay_formula.clone()
    };
    assert_eq!(h.b.run("set_formula", &gone).unwrap_err(), "no such filter");
    let below = SetFormula {
        category_id: giving,
        terms: vec![Term {
            filter_id: acme,
            times: -1,
            per: 10,
        }],
        ..pay_formula
    };
    assert!(h.b.run("set_formula", &below).is_err());
}

#[test]
fn overspending_a_category_its_formula_assigns_is_paid_for_by_next_month() {
    let mut h = house();
    let (checking, giving) = (h.checking, h.giving);
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    let acme = h.filter("ACME", income("acme", &[checking]));
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    h.b.spend(checking, "2026-10-10", "Charity", Some(giving), -15_000);
    let oct = h.giving("2026-10");
    assert_eq!((oct.assigned, oct.activity, oct.available), (10_000, -15_000, -5_000));
    let nov = h.b.summary("2026-11");
    assert_eq!(nov.overspent_last_month, -5_000);
    assert_eq!(nov.ready_to_assign, 300_000 + 100_000 - 10_000 - 5_000);
}

/// A filter a formula sums is not deleted: every month the formula covers
/// is worked out from it. Once nothing sums it, it goes.
#[test]
fn a_filter_a_formula_sums_is_kept() {
    let mut h = house();
    let checking = h.checking;
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    let acme = h.filter("ACME", income("acme", &[checking]));
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    let used = h.b.list::<FilterLine>("saved", &()).pop().unwrap();
    assert_eq!(used.used_by, vec!["Giving".to_string()]);
    assert_eq!(
        h.b.run("delete_filter", &FilterId { id: acme }).unwrap_err(),
        "ACME is summed by the formula of Giving"
    );
    // Assigned by hand again from November: October's version still sums
    // it, so it still stays.
    h.clear(Some("2026-11")).unwrap();
    assert!(h.b.run("delete_filter", &FilterId { id: acme }).is_err());
    h.clear(Some("2026-10")).unwrap();
    h.b.run("delete_filter", &FilterId { id: acme }).unwrap();
    assert!(h.b.rows::<SavedFilter>("saved_filter").is_empty());
    // A term whose filter is gone anyway (never by these functions) counts
    // nothing, and says so.
    let p = h.preview("2026-10", &[(acme, "10%")]);
    assert_eq!((p.amount, p.text.as_str()), (0, "10% \u{d7} SUM(a filter gone)"));
}

#[test]
fn a_formula_is_previewed_before_it_is_saved_and_listed_after() {
    let mut h = house();
    october(&mut h);
    let (checking, savings, giving) = (h.checking, h.savings, h.giving);
    let acme = h.filter("ACME", income("ACME", &[checking, savings]));
    let side = h.filter("Side", income("side", &[]));
    let p = h.preview("2026-10", &[(acme, "10%"), (side, "50%")]);
    assert_eq!(p.amount, 25_000 + 15_000);
    assert_eq!(p.text, "10% \u{d7} SUM(ACME) + 50% \u{d7} SUM(Side)");
    assert_eq!((p.terms[0].count, p.terms[0].sum, p.terms[0].name.as_str()), (2, 250_000, "ACME"));
    assert_eq!((p.terms[1].count, p.terms[1].sum), (1, 30_000));
    assert!(h.b.list::<FormulaLine>("formulas", &Month { month: "2026-10".into() }).is_empty());
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    let lines = h.b.list::<FormulaLine>("formulas", &Month { month: "2026-10".into() });
    assert_eq!(lines.len(), 1);
    let l = &lines[0];
    assert_eq!((l.category_id, l.from.as_str(), l.amount), (giving, "2026-10", 25_000));
    assert_eq!((l.terms[0].filter_id, l.terms[0].times, l.terms[0].per), (acme, 1, 10));
    assert!(h.b.list::<FormulaLine>("formulas", &Month { month: "2026-09".into() }).is_empty());
}

#[test]
fn a_category_deleted_gives_what_its_formula_gave() {
    let mut h = house();
    let (checking, giving, groceries) = (h.checking, h.giving, h.groceries);
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    h.b.assign("2026-09", giving, 2_000);
    let acme = h.filter("ACME", income("acme", &[checking]));
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    let before = h.b.summary("2026-10").ready_to_assign;
    let d = DeleteCategory {
        id: giving,
        move_to: Some(groceries),
    };
    h.b.run("delete_category", &d).unwrap();
    assert!(h.b.rows::<Formula>("formula").is_empty());
    assert_eq!(h.b.line("2026-10", groceries).assigned, 10_000);
    assert_eq!(h.b.line("2026-09", groceries).assigned, 2_000);
    assert_eq!(h.b.summary("2026-10").ready_to_assign, before);
    // Nothing sums the filter now.
    h.b.run("delete_filter", &FilterId { id: acme }).unwrap();
}

/// The phones' `set_rule`, as a phone built before formulas sends it: a
/// formula of one term over a saved filter of the rule's conditions, said
/// as the rule was, the filter reused when the same rule is set again.
#[test]
fn the_phones_rule_is_a_formula() {
    let mut h = house();
    october(&mut h);
    let (checking, savings, giving) = (h.checking, h.savings, h.giving);
    let rule = SetRule {
        category_id: giving,
        from: Some("2026-10".into()),
        matching: "acme".into(),
        accounts: vec![savings, checking],
        times: 1,
        per: 10,
    };
    h.b.run("set_rule", &rule).unwrap();
    let g = h.giving("2026-10");
    assert_eq!((g.assigned, g.ruled), (25_000, true));
    assert_eq!(g.rule, "10% of income matching acme in Checking, Savings");
    let saved = h.b.rows::<SavedFilter>("saved_filter");
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "Income matching acme in Checking, Savings");
    let mut accounts = vec![checking, savings];
    accounts.sort();
    assert_eq!(
        saved[0].filter,
        Filter {
            accounts,
            on_budget: true,
            ..income("acme", &[])
        }
    );
    // Every account on the budget: the cash account's counts too.
    let any = SetRule {
        accounts: vec![],
        ..rule.clone()
    };
    h.b.run("set_rule", &any).unwrap();
    let g = h.giving("2026-10");
    assert_eq!((g.assigned, g.rule.as_str()), (35_000, "10% of income matching acme in any account"));
    h.b.run("set_rule", &any).unwrap();
    assert_eq!(h.b.rows::<SavedFilter>("saved_filter").len(), 2, "the same rule's filter again");
    // An account off the budget is refused, as the rule was.
    let loan = h.b.account("Loan", "loan", -100_000);
    let off = SetRule {
        accounts: vec![loan],
        ..rule.clone()
    };
    assert_eq!(
        h.b.run("set_rule", &off).unwrap_err(),
        "Loan is not on the budget: its money is not assigned"
    );
    // Set as a formula on the desktop afterwards, it is said by its terms.
    let f =
        h.b.rows::<SavedFilter>("saved_filter")
            .into_iter()
            .find(|f| f.name.ends_with("Savings"))
            .unwrap()
            .id;
    h.formula(Some("2026-10"), &[(f, "10%")]).unwrap();
    assert_eq!(h.giving("2026-10").rule, "10% \u{d7} SUM(Income matching acme in Checking, Savings)");
}

/// The `Rule` table is still in the schema, and read by nothing.
#[test]
fn the_rule_table_stays_and_assigns_nothing() {
    let h = house();
    let names: Vec<&str> = common::domain().schema.tables.iter().map(|t| t.name.as_str()).collect();
    assert!(names.windows(3).any(|w| w == ["rule", "saved_filter", "formula"]), "{names:?}");
    assert!(h.b.rows::<Rule>("rule").is_empty());
    assert_eq!(h.giving("2026-10").assigned, 0);
}

/// A saved filter and a formula over it read the payee and the bank's line
/// alone, whatever a search box reads: an account, a memo and a category
/// naming the text add nothing to its total nor to the formula's month,
/// while a search for the same text finds them. Falsified by the saved
/// filter reading as wide as a search: the total and the giving grew.
#[test]
fn a_saved_filter_and_its_formula_read_the_payee_alone() {
    let mut h = house();
    october(&mut h);
    let acme = h.filter("ACME paychecks", income("acme", &[]));
    h.formula(Some("2026-10"), &[(acme, "10%")]).unwrap();
    let saved = Filtered {
        saved: Some(acme),
        ..Filtered::default()
    };
    let total = |h: &House, f: &Filtered| h.b.list::<FilterTotal>("filtered_total", f).pop().unwrap_or_default();
    let (before, giving) = (total(&h, &saved), h.giving("2026-10").assigned);
    assert_eq!(giving, 35_000);
    // Money in that names ACME only where a register shows it beside the payee.
    let named = h.b.account("ACME Savings", "savings", 0);
    h.paid(named, "2026-10-05", "Transfer In", 40_000);
    let side = h.b.group("Side");
    let refunds = h.b.category(side, "ACME Refunds");
    let checking = h.checking;
    h.b.spend(checking, "2026-10-06", "Refund", Some(refunds), 7_000);
    let memo = AddTransaction {
        account_id: checking,
        date: "2026-10-07".into(),
        payee: "Bonus".into(),
        category_id: Some(ready_to_assign()),
        memo: "from acme".into(),
        amount: 9_000,
        ..AddTransaction::default()
    };
    h.b.run("add_transaction", &memo).unwrap();
    assert_eq!(total(&h, &saved), before, "the saved filter's total");
    assert_eq!(h.giving("2026-10").assigned, giving, "the formula's month");
    // A search box's register for the same text finds every one of them.
    let search = Filtered {
        filter: income("acme", &[]),
        search: true,
        ..Filtered::default()
    };
    let found: Vec<String> = h.b.list::<TxnLine>("filtered", &search).into_iter().map(|t| t.payee).collect();
    for p in ["Transfer In", "Refund", "Bonus"] {
        assert!(found.iter().any(|f| f == p), "{p}: {found:?}");
    }
    // A search of a saved filter is the saved filter: read as one.
    let as_saved = Filtered {
        saved: Some(acme),
        search: true,
        ..Filtered::default()
    };
    assert_eq!(total(&h, &as_saved), before);
}
