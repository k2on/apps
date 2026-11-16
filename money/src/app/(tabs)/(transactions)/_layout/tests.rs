//! The Transactions tab over the demo household: what waits to be looked
//! at, approving it (one, all), a category chosen and approved with it, a
//! delete asked about first, All by day, and the search.

use super::*;
use crate::testing::Demo;
use ark_app::{Host, Runtime};

fn open() -> (Demo<Transactions>, Runtime<Transactions>) {
    let mut d = Demo::<Transactions>::new();
    let mut rt = Runtime::<Transactions>::new();
    d.run(&mut rt, [TransactionsMsg::Month { month: "2026-10".into() }]);
    (d, rt)
}

/// To Review is the inbox's rows; Approve All approves those with a
/// category and leaves the rest, with one Success. Falsified by approving
/// one that needs a category.
#[test]
fn approve_all_approves_what_has_a_category() {
    let (mut d, mut rt) = open();
    let p = rt.screen(&d);
    assert!(p.reviewing && p.has_review, "the demo has imports waiting");
    let waiting = p.review.len();
    let ready = p.review.iter().filter(|t| t.can_approve).count();
    assert!(ready > 0);
    assert_eq!(
        (p.approve_all.as_str(), p.can_approve_all),
        (format!("Approve All ({ready})").as_str(), true)
    );
    assert!(p
        .review
        .iter()
        .all(|t| t.detail.starts_with("New") || t.detail.contains("Needs a Category")));
    d.run(&mut rt, [TransactionsMsg::ApproveAll]);
    let p = rt.screen(&d);
    assert_eq!(p.review.len(), waiting - ready, "those with a category approved");
    assert!(!p.can_approve_all);
    assert_eq!(d.haptics.performed(), vec![Haptic::Success]);
}

/// One needing a category asks for it when tapped, and is approved with
/// it. Falsified by approving it before a category is chosen.
#[test]
fn a_category_is_chosen_and_approved_with() {
    let (mut d, mut rt) = open();
    // Make one need a category.
    let id = rt.screen(&d).review[0].id.clone();
    let t = found(&ark_app::Data { store: &d.store, now_ms: 0 }, &id).unwrap().clone();
    Host::<Transactions>::send(
        &mut d,
        MoneyChange::Categorize(Categorize {
            ids: vec![t.id],
            category_id: None,
        }),
    )
    .unwrap();
    d.run(&mut rt, [TransactionsMsg::Open { id: id.clone() }]);
    let form = rt.screen(&d).categorizing.expect("asked for a category");
    assert_eq!(form.confirm, "Categorize and Approve");
    assert!(!form.can_save && !form.choices.is_empty());
    let key = form.choices[0].key.clone();
    d.run(&mut rt, [TransactionsMsg::Chose { key }, TransactionsMsg::SaveCategory]);
    assert!(rt.screen(&d).categorizing.is_none());
    assert!(!rt.screen(&d).review.iter().any(|r| r.id == id), "categorized and approved");
}

/// A delete is asked about, then made; Cancel makes none.
#[test]
fn a_delete_is_asked_about_first() {
    let (mut d, mut rt) = open();
    d.run(&mut rt, [TransactionsMsg::Show { review: false }]);
    let p = rt.screen(&d);
    assert!(p.all_shown && !p.days.is_empty());
    let id = p.days[0].rows[0].id.clone();
    let count: usize = p.days.iter().map(|d| d.rows.len()).sum();
    d.run(&mut rt, [TransactionsMsg::Delete { id: id.clone() }]);
    assert!(rt.screen(&d).deleting.unwrap().message.ends_with("This can't be undone."));
    d.run(&mut rt, [TransactionsMsg::CancelDelete]);
    assert_eq!(rt.screen(&d).days.iter().map(|d| d.rows.len()).sum::<usize>(), count);
    d.run(&mut rt, [TransactionsMsg::Delete { id }, TransactionsMsg::ConfirmDelete]);
    assert_eq!(rt.screen(&d).days.iter().map(|d| d.rows.len()).sum::<usize>(), count - 1);
}

/// All is by day, newest first; the search finds by payee, and To Review
/// says when nothing it holds matches.
#[test]
fn all_by_day_and_the_search() {
    let (mut d, mut rt) = open();
    d.run(&mut rt, [TransactionsMsg::Show { review: false }]);
    let p = rt.screen(&d);
    let titles: Vec<&str> = p.days.iter().map(|d| d.title.as_str()).collect();
    let mut unique = titles.clone();
    unique.dedup();
    assert_eq!(titles, unique, "one section a day");
    let payee = p.days[0].rows[0].payee.clone();
    d.run(&mut rt, [TransactionsMsg::Search { text: payee.to_uppercase() }]);
    assert!(rt
        .screen(&d)
        .days
        .iter()
        .flat_map(|d| &d.rows)
        .all(|t| t.payee == payee || t.detail.to_lowercase().contains(&payee.to_lowercase())));
    d.run(
        &mut rt,
        [
            TransactionsMsg::Show { review: true },
            TransactionsMsg::Search { text: "zzz-nothing".into() },
        ],
    );
    let p = rt.screen(&d);
    assert!(p.caught_up && p.empty_title == "No Results");
    assert!(rt.is_idle(&d.store));
}

/// Editing, and tapping one with a category, is the host's form.
#[test]
fn a_row_with_a_category_opens_its_details() {
    let (mut d, mut rt) = open();
    let t = rt.screen(&d).review.into_iter().find(|t| t.can_approve).unwrap();
    d.run(
        &mut rt,
        [TransactionsMsg::Open { id: t.id.clone() }, TransactionsMsg::Edit { id: t.id.clone() }],
    );
    assert_eq!(
        d.requests,
        vec![
            MoneyRequest::OpenTransaction { key: t.id.clone() },
            MoneyRequest::OpenTransaction { key: t.id }
        ]
    );
}
