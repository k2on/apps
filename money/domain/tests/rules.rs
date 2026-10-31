//! Assignment by rule, on a household's months: a category given a share
//! of the paychecks that match its text in the accounts it names, from the
//! month the rule is set on — every month before keeps what was typed —
//! and that share counted everywhere a typed amount would be.

mod common;

use common::Book;
use money_domain::budget::ready_to_assign;
use money_domain::cards::payment_category;
use money_domain::plaid::{import_of, PlaidAccount, Pulled};
use money_domain::rules::FOLLOWS;
use money_domain::*;

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

impl House {
    /// Money in, for Ready to Assign.
    fn paid(&mut self, account: Id<Account>, date: &str, payee: &str, amount: i64) {
        self.b.spend(account, date, payee, Some(ready_to_assign()), amount);
    }

    fn rule(&mut self, from: Option<&str>, matching: &str, accounts: &[Id<Account>], share: &str) -> Result<usize, String> {
        let (times, per) = rules::parse_share(share).expect("a share");
        let r = SetRule {
            category_id: self.giving,
            from: from.map(str::to_string),
            matching: matching.into(),
            accounts: accounts.to_vec(),
            times,
            per,
        };
        self.b.run("set_rule", &r)
    }

    fn clear(&mut self, from: Option<&str>) -> Result<usize, String> {
        let c = ClearRule {
            category_id: self.giving,
            from: from.map(str::to_string),
        };
        self.b.run("clear_rule", &c)
    }

    fn giving(&self, month: &str) -> BudgetLine {
        self.b.line(month, self.giving)
    }

    fn preview(&self, month: &str, matching: &str, accounts: &[Id<Account>], share: &str) -> RulePreview {
        let (times, per) = rules::parse_share(share).unwrap();
        let p = PreviewRule {
            month: month.into(),
            matching: matching.into(),
            accounts: accounts.to_vec(),
            times,
            per,
        };
        self.b.list::<RulePreview>("preview_rule", &p).pop().expect("a preview")
    }
}

/// October's paychecks: ACME into checking and savings, ACME into cash
/// (not one of the rule's), another employer into checking, and ACME
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
fn a_rule_assigns_a_share_of_what_matched_in_its_accounts() {
    let mut h = house();
    october(&mut h);
    let (checking, savings) = (h.checking, h.savings);
    h.rule(Some("2026-10"), "acme", &[checking, savings], "10%").unwrap();
    // $2,000 + $500 matched; the cash account's $1,000, the side gig and
    // the outflow did not.
    let g = h.giving("2026-10");
    assert_eq!((g.assigned, g.available, g.ruled), (25_000, 25_000, true));
    assert_eq!(g.rule, "10% of income matching acme in Checking, Savings");
    let s = h.b.summary("2026-10");
    // Income in October is $3,800; ready to assign has the $3,000 too.
    assert_eq!(s.ready_to_assign, 300_000 + 380_000 - 25_000);
    assert_eq!(s.assigned, 25_000);
    // Every account on the budget: the cash account's counts too.
    h.rule(Some("2026-10"), "acme", &[], "10%").unwrap();
    let g = h.giving("2026-10");
    assert_eq!(g.assigned, 35_000);
    assert_eq!(g.rule, "10% of income matching acme in any account");
    // A rule over another month's paychecks gives this one nothing.
    assert_eq!(h.giving("2026-11").assigned, 0);
    assert!(h.giving("2026-11").ruled);
}

#[test]
fn the_banks_line_is_matched_as_the_payee_is() {
    let mut h = house();
    let accounts: Vec<PlaidAccount> = serde_json::from_str(
        r#"[{"account_id":"a1","name":"Bank Checking","mask":"4321","type":"depository","subtype":"checking","balances":{"current":0.0}}]"#,
    )
    .unwrap();
    let pulled: Pulled = serde_json::from_str(
        r#"{"added":[{"transaction_id":"x1","account_id":"a1","amount":-1234.56,"date":"2026-10-01",
              "name":"ACME CORP DIR DEP","merchant_name":"Direct Deposit","original_description":"ACME CORP DIR DEP PPD"}],
            "modified":[],"removed":[],"cursor":"c1"}"#,
    )
    .unwrap();
    h.b.import(&import_of("bank", "Bank", &accounts, &pulled)).unwrap();
    let t = h.b.register(None).into_iter().find(|t| t.amount == 123_456).expect("the paycheck");
    assert_eq!((t.payee.as_str(), t.imported_payee.as_str()), ("Direct Deposit", "ACME CORP DIR DEP PPD"));
    h.rule(Some("2026-10"), "Acme Corp", &[], "0.1").unwrap();
    // 10% of $1,234.56 is $123.456: $123.46, half away from nothing.
    assert_eq!(h.giving("2026-10").assigned, 12_346);
    // The payee matches as well: by its text, not the bank's.
    h.rule(Some("2026-10"), "deposit", &[], "0.1").unwrap();
    assert_eq!(h.giving("2026-10").assigned, 12_346);
    h.rule(Some("2026-10"), "payroll", &[], "0.1").unwrap();
    assert_eq!(h.giving("2026-10").assigned, 0);
}

#[test]
fn transfers_and_outflows_are_not_income() {
    let mut h = house();
    let checking = h.checking;
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    // Money the bank calls ACME's, come in from checking: a transfer.
    let accounts: Vec<PlaidAccount> = serde_json::from_str(
        r#"[{"account_id":"a1","name":"Bank Savings","mask":"4321","type":"depository","subtype":"savings","balances":{"current":0.0}}]"#,
    )
    .unwrap();
    let pulled: Pulled = serde_json::from_str(
        r#"{"added":[{"transaction_id":"x1","account_id":"a1","amount":-400.0,"date":"2026-10-02",
              "name":"ACME XFER","original_description":"ACME XFER FROM CHK"}],
            "modified":[],"removed":[],"cursor":"c1"}"#,
    )
    .unwrap();
    h.b.import(&import_of("bank", "Bank", &accounts, &pulled)).unwrap();
    let saved = h.b.accounts().into_iter().find(|a| a.name == "Bank Savings").unwrap().id;
    let t = h.b.register(Some(saved)).into_iter().find(|t| t.amount == 40_000).unwrap();
    // Before it is a transfer, the bank's line counts.
    assert_eq!(h.preview("2026-10", "xfer", &[saved], "10%").amount, 4_000);
    let e = EditTransaction {
        id: t.id,
        date: t.date.clone(),
        payee: t.payee.clone(),
        amount: t.amount,
        transfer_to: Some(checking),
        ..EditTransaction::default()
    };
    h.b.run("edit_transaction", &e).unwrap();
    let after = h.b.register(Some(saved)).into_iter().find(|t| t.amount == 40_000).unwrap();
    assert!(after.transfer, "a transfer");
    assert_eq!(after.imported_payee, "ACME XFER FROM CHK");
    // Money out to ACME is not money in.
    h.b.spend(checking, "2026-10-03", "ACME", Some(h.groceries), -2_000);
    h.rule(Some("2026-10"), "acme", &[checking, saved], "10%").unwrap();
    assert_eq!(h.giving("2026-10").assigned, 10_000);
    assert_eq!(h.preview("2026-10", "xfer", &[saved], "10%").count, 0);
}

#[test]
fn a_share_is_kept_exactly_and_rounded_once_a_month() {
    let mut h = house();
    let checking = h.checking;
    h.paid(checking, "2026-10-01", "ACME", 100_005);
    h.paid(checking, "2026-10-15", "ACME", 100_000);
    for (share, want) in [
        ("10%", 20_001),
        ("0.1", 20_001),
        ("1.5", 300_008),
        ("1.5x", 300_008),
        ("12.5%", 25_001),
        ("0", 0),
    ] {
        h.rule(Some("2026-10"), "acme", &[checking], share).unwrap();
        // $2,000.05 a month: 10% is $200.005, $200.01 — the month's sum is
        // rounded, not each paycheck's.
        assert_eq!(h.giving("2026-10").assigned, want, "{share}");
        assert_eq!(h.preview("2026-10", "acme", &[checking], share).amount, want, "{share}");
    }
    let rule = h.b.rows::<Rule>("rule").pop().unwrap();
    assert_eq!((rule.times, rule.per), (0, 1));
    h.rule(Some("2026-10"), "acme", &[checking], "1.5").unwrap();
    let rule = h.b.rows::<Rule>("rule").pop().unwrap();
    assert_eq!((rule.times, rule.per), (3, 2));
    assert_eq!(h.giving("2026-10").rule, "150% of income matching acme in Checking");
}

#[test]
fn a_rule_holds_from_its_month_and_earlier_months_keep_theirs() {
    let mut h = house();
    let (checking, giving) = (h.checking, h.giving);
    h.paid(checking, "2026-09-01", "ACME", 100_000);
    h.paid(checking, "2026-10-01", "ACME", 200_000);
    h.paid(checking, "2026-11-01", "ACME", 300_000);
    h.b.assign("2026-09", giving, 5_000);
    // Typed in October, before the rule: the rule, not this, holds there.
    h.b.assign("2026-10", giving, 7_000);
    h.rule(Some("2026-10"), "acme", &[checking], "10%").unwrap();
    let (sep, oct, nov) = (h.giving("2026-09"), h.giving("2026-10"), h.giving("2026-11"));
    assert_eq!((sep.assigned, sep.ruled, sep.rule.as_str()), (5_000, false, ""));
    assert_eq!((oct.assigned, oct.ruled), (20_000, true));
    assert_eq!((nov.assigned, nov.ruled), (30_000, true));
    // What was left carries, as any assignment's does.
    assert_eq!((oct.carried, oct.available), (5_000, 25_000));
    assert_eq!((nov.carried, nov.available), (25_000, 55_000));
    let s = h.b.summary("2026-11");
    assert_eq!(s.ready_to_assign, 300_000 + 600_000 - 5_000 - 20_000 - 30_000);
    // None said is this month: the book's today is 2026-10-26.
    let mut h2 = house();
    let checking = h2.checking;
    h2.rule(None, "acme", &[checking], "10%").unwrap();
    assert_eq!(h2.b.rows::<Rule>("rule")[0].from, "2026-10");
}

#[test]
fn a_rule_changed_and_removed_mid_history() {
    let mut h = house();
    let (checking, savings, giving) = (h.checking, h.savings, h.giving);
    for m in ["08", "09", "10", "11", "12"] {
        h.paid(checking, &format!("2026-{m}-01"), "ACME", 100_000);
        h.paid(savings, &format!("2026-{m}-02"), "ACME", 10_000);
    }
    h.b.assign("2026-08", giving, 1_000);
    h.rule(Some("2026-09"), "acme", &[checking], "10%").unwrap();
    // Changed in October: savings' too, and 20%.
    h.rule(Some("2026-10"), "acme", &[checking, savings], "20%").unwrap();
    // Assigned by hand again from December.
    h.clear(Some("2026-12")).unwrap();
    let assigned: Vec<(i64, bool)> = ["2026-08", "2026-09", "2026-10", "2026-11", "2026-12"]
        .iter()
        .map(|m| {
            let l = h.giving(m);
            (l.assigned, l.ruled)
        })
        .collect();
    assert_eq!(assigned, vec![(1_000, false), (10_000, true), (22_000, true), (22_000, true), (0, false)]);
    // December is typed in again.
    h.b.assign("2026-12", giving, 3_000);
    assert_eq!(h.giving("2026-12").assigned, 3_000);
    // Carried, through every kind of month.
    assert_eq!(h.giving("2026-12").available, 1_000 + 10_000 + 22_000 + 22_000 + 3_000);
    let versions: Vec<(String, bool)> = h.b.rows::<Rule>("rule").into_iter().map(|r| (r.from, r.manual)).collect();
    assert_eq!(
        versions,
        vec![("2026-09".into(), false), ("2026-10".into(), false), ("2026-12".into(), true)]
    );
    // Set again from September: what followed it is replaced.
    h.rule(Some("2026-09"), "acme", &[checking], "5%").unwrap();
    assert_eq!(h.b.rows::<Rule>("rule").len(), 1);
    assert_eq!((h.giving("2026-12").assigned, h.giving("2026-12").ruled), (5_000, true));
    // Removed from October: September keeps its share.
    h.clear(Some("2026-10")).unwrap();
    assert_eq!(h.giving("2026-09").assigned, 5_000);
    assert_eq!((h.giving("2026-10").assigned, h.giving("2026-10").ruled), (0, false));
    // The typed December from before the rule is back.
    assert_eq!(h.giving("2026-12").assigned, 3_000);
    // Removing what is not there says so; the same rule again is nothing.
    assert_eq!(h.clear(Some("2026-11")).unwrap_err(), "this category is assigned by hand already");
    h.rule(Some("2026-10"), "acme", &[checking], "5%").unwrap();
    assert_eq!(h.b.rows::<Rule>("rule").len(), 1, "the same rule as September's is not a version");
    // Removed from its own first month: no rule at all.
    h.clear(Some("2026-09")).unwrap();
    assert!(h.b.rows::<Rule>("rule").is_empty());
    assert_eq!(h.giving("2026-09").assigned, 0);
}

#[test]
fn a_ruled_month_is_not_assigned_by_hand() {
    let mut h = house();
    let (checking, giving, groceries) = (h.checking, h.giving, h.groceries);
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    h.b.assign("2026-09", giving, 2_000);
    h.b.assign("2026-09", groceries, 4_000);
    h.rule(Some("2026-10"), "acme", &[checking], "10%").unwrap();
    let assign = Assign {
        month: "2026-10".into(),
        category_id: giving,
        amount: 1,
    };
    assert_eq!(h.b.run("assign", &assign).unwrap_err(), FOLLOWS);
    // September is the hand's still.
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
    // The quick buttons pass it by, and do the rest.
    let auto = |how| AutoAssign {
        month: "2026-10".into(),
        how,
        only: vec![],
    };
    h.b.run("auto_assign", &auto(Auto::LastMonth)).unwrap();
    assert_eq!((h.giving("2026-10").assigned, h.b.line("2026-10", groceries).assigned), (10_000, 4_000));
    h.b.run("auto_assign", &auto(Auto::Reset)).unwrap();
    assert_eq!((h.giving("2026-10").assigned, h.b.line("2026-10", groceries).assigned), (10_000, 0));
    assert!(h
        .b
        .rows::<Assigned>("assigned")
        .iter()
        .all(|a| a.month != "2026-10" || a.category_id != giving));
    // Last month's, read in November after the rule stops: the rule's share.
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
fn overspending_a_ruled_category_is_paid_for_by_next_month() {
    let mut h = house();
    let (checking, giving) = (h.checking, h.giving);
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    h.rule(Some("2026-10"), "acme", &[checking], "10%").unwrap();
    h.b.spend(checking, "2026-10-10", "Charity", Some(giving), -15_000);
    let oct = h.giving("2026-10");
    assert_eq!((oct.assigned, oct.activity, oct.available), (10_000, -15_000, -5_000));
    assert_eq!(h.b.summary("2026-10").overspent, 1);
    let nov = h.b.summary("2026-11");
    assert_eq!(nov.overspent_last_month, -5_000);
    assert_eq!(nov.ready_to_assign, 300_000 + 100_000 - 10_000 - 5_000);
    assert_eq!(h.giving("2026-11").carried, 0);
}

#[test]
fn spending_on_a_card_from_a_ruled_category_moves_its_share() {
    let mut h = house();
    let (checking, card, giving) = (h.checking, h.card, h.giving);
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    h.rule(Some("2026-10"), "acme", &[checking], "10%").unwrap();
    // $150 on the card: $100 covered by the rule's share, $50 debt.
    h.b.spend(card, "2026-10-05", "Charity", Some(giving), -15_000);
    let pay = h.b.line("2026-10", payment_category(card));
    assert_eq!((pay.activity, pay.available, pay.debt, pay.underfunded), (10_000, 10_000, 15_000, 5_000));
    assert_eq!(h.giving("2026-10").credit_overspent, 5_000);
    // Credit overspending is the card's: November's money is not taken.
    assert_eq!(h.b.summary("2026-11").ready_to_assign, 300_000 + 100_000 - 10_000);
    // A card's payment category and Ready to Assign have no rules.
    let pay_rule = SetRule {
        category_id: payment_category(card),
        from: None,
        matching: "acme".into(),
        accounts: vec![],
        times: 1,
        per: 10,
    };
    assert_eq!(
        h.b.run("set_rule", &pay_rule).unwrap_err(),
        "a card's payment category is assigned by hand: its money is the card's"
    );
    let rta_rule = SetRule {
        category_id: ready_to_assign(),
        ..pay_rule.clone()
    };
    assert_eq!(
        h.b.run("set_rule", &rta_rule).unwrap_err(),
        "money is assigned from Ready to Assign, not to it"
    );
    // Nor reads a card, or an account off the budget.
    let loan = h.b.account("Loan", "loan", -100_000);
    let off = SetRule {
        category_id: giving,
        accounts: vec![loan],
        ..pay_rule
    };
    assert_eq!(
        h.b.run("set_rule", &off).unwrap_err(),
        "Loan is not on the budget: its money is not assigned"
    );
}

#[test]
fn a_rule_is_previewed_before_it_is_saved_and_listed_after() {
    let mut h = house();
    october(&mut h);
    let (checking, savings, giving) = (h.checking, h.savings, h.giving);
    let p = h.preview("2026-10", "ACME", &[checking, savings], "10%");
    assert_eq!((p.count, p.matched, p.amount), (2, 250_000, 25_000));
    assert_eq!(p.summary, "10% of income matching ACME in Checking, Savings");
    let p = h.preview("2026-10", "", &[], "10%");
    assert_eq!((p.count, p.amount), (0, 0));
    assert!(h.b.list::<RuleLine>("rules", &Month { month: "2026-10".into() }).is_empty());
    h.rule(Some("2026-10"), "ACME", &[checking, savings], "10%").unwrap();
    let rules = h.b.list::<RuleLine>("rules", &Month { month: "2026-10".into() });
    assert_eq!(rules.len(), 1);
    let r = &rules[0];
    assert_eq!((r.category_id, r.from.as_str(), r.matching.as_str()), (giving, "2026-10", "ACME"));
    assert_eq!((r.times, r.per, r.matched, r.amount), (1, 10, 250_000, 25_000));
    assert!(h.b.list::<RuleLine>("rules", &Month { month: "2026-09".into() }).is_empty());
    // A rule that matches nothing is refused; the text is trimmed.
    let blank = SetRule {
        category_id: giving,
        from: None,
        matching: "  ".into(),
        accounts: vec![],
        times: 1,
        per: 10,
    };
    assert!(h.b.run("set_rule", &blank).is_err());
}

#[test]
fn a_ruled_category_deleted_gives_what_it_was_given() {
    let mut h = house();
    let (checking, giving, groceries) = (h.checking, h.giving, h.groceries);
    h.paid(checking, "2026-10-01", "ACME", 100_000);
    h.b.assign("2026-09", giving, 2_000);
    h.rule(Some("2026-10"), "acme", &[checking], "10%").unwrap();
    let before = h.b.summary("2026-10").ready_to_assign;
    let d = DeleteCategory {
        id: giving,
        move_to: Some(groceries),
    };
    h.b.run("delete_category", &d).unwrap();
    assert!(h.b.rows::<Rule>("rule").is_empty());
    assert_eq!(h.b.line("2026-10", groceries).assigned, 10_000);
    assert_eq!(h.b.line("2026-09", groceries).assigned, 2_000);
    assert_eq!(h.b.summary("2026-10").ready_to_assign, before);
}
