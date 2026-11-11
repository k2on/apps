//! Insights over the demo household's month: its spending by group, the
//! most first, the month's figures, and each group's plan.

use super::*;
use crate::testing::Demo;
use ark_app::Runtime;

/// The bars are the groups that spent, the most first, their shares and
/// the month adding up. Falsified by counting a card's payment as
/// spending: the cards' group had a bar.
#[test]
fn where_the_months_money_went() {
    let mut d = Demo::<Insights>::new();
    let mut rt = Runtime::<Insights>::new();
    d.run(&mut rt, [InsightsMsg::Month { month: "2026-10".into() }]);
    let p = rt.screen(&d);
    assert!(p.spent_any, "the demo spends in October");
    assert_eq!(p.labels.len(), p.values.len());
    assert!(p.values.windows(2).all(|w| w[0] >= w[1]), "the most first");
    assert!(!p.labels.iter().any(|l| l == "Credit Card Payments"));
    assert_eq!(p.spent, -p.values.iter().sum::<i64>());
    assert_eq!(p.left, p.income + p.spent);
    assert_eq!(p.month, "October 2026");
    assert!(p.has_plans && p.plans.iter().all(|g| g.title != "Credit Card Payments"));
    assert!(p.groups.iter().all(|g| g.cents < 0 && g.share.ends_with('%')));
}
