//! The Transactions tab: what needs a look first — the bank's imports not
//! yet approved, and anything on the budget with no category — and every
//! transaction in the household after, by day, as To Review / All switch
//! (Mail's inboxes). A row needing a category asks for one and is approved
//! with it; Approve All approves what has one; any row's menu approves,
//! categorizes, edits or deletes it. What is searched is the shell's
//! search field's (`TransactionsMsg::Search`).
//!
//! The rows are the store's (`MoneyView::Inbox`, `Register`, and the
//! month's `Lines` for the categories to choose from); [`State`] keeps
//! which list shows, what is searched, and a choice or a question up.

use ark_app::modules::haptics::{perform, Haptic};
use ark_app::{App, Command, Data};
use ark_format::{amount, NumberStyle};
use money_core::*;

use crate::accounts::Style;
use crate::routes::Route;
use crate::MoneyRequest;

pub mod view;

#[cfg(test)]
mod tests;

/// The screen, as `ark_app` runs it.
pub struct Transactions;

/// A category being chosen for these transactions (hex ids), and approved
/// with it when `approve`.
#[derive(Clone, Debug, PartialEq)]
pub struct Choosing {
    pub ids: Vec<String>,
    pub approve: bool,
    pub chosen: Option<String>,
}

/// What the tab is in the middle of.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// To Review (true) or All.
    pub review: bool,
    pub search: String,
    /// The month whose categories a choice offers (`2026-11`).
    pub month: String,
    pub choosing: Option<Choosing>,
    /// A transaction asked about before it is deleted.
    pub deleting: Option<String>,
    pub style: Style,
}

impl Default for State {
    fn default() -> State {
        State {
            review: true,
            search: String::new(),
            month: String::new(),
            choosing: None,
            deleting: None,
            style: Style::default(),
        }
    }
}

/// Everything the tab can be told. Ids are rows' `Id::hex`.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum TransactionsMsg {
    /// To Review, or All.
    Show {
        review: bool,
    },
    /// What the shell's search field says.
    Search {
        text: String,
    },
    /// The month the budget is on (the categories a choice offers).
    Month {
        month: String,
    },
    /// Ask the bank for anything new.
    Refresh,
    /// A row tapped: a category asked for when it needs one (and approved
    /// with it), its details otherwise.
    Open {
        id: String,
    },
    Approve {
        id: String,
    },
    /// Every one to review that has a category, approved.
    ApproveAll,
    Categorize {
        id: String,
    },
    Chose {
        key: String,
    },
    SaveCategory,
    Dismiss,
    Edit {
        id: String,
    },
    Delete {
        id: String,
    },
    ConfirmDelete,
    CancelDelete,
    /// What became of a change sent; `approved` when it approved some.
    Sent {
        refused: Option<String>,
    },
    Approved {
        refused: Option<String>,
    },
}

/// One transaction's row.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct TxnItem {
    pub id: String,
    pub payee: String,
    /// Its category (or "Needs a Category", "Transfer") and its account,
    /// "New" first while the bank's import waits to be approved.
    pub detail: String,
    pub amount: i64,
    pub said: String,
    pub can_approve: bool,
}

/// A day of All.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct TxnDay {
    /// "Monday, October 26".
    pub title: String,
    pub rows: Vec<TxnItem>,
}

/// A category to choose.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct CategoryChoice {
    pub key: String,
    pub title: String,
    pub glyph: String,
    /// Its group, and what it has available in its tone's tint.
    pub group: String,
    pub detail: String,
    pub tint: String,
}

/// The category's sheet.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct CategorizeForm {
    pub chosen: String,
    pub choices: Vec<CategoryChoice>,
    /// "Categorize and Approve", or "Categorize".
    pub confirm: String,
    pub can_save: bool,
}

/// The question before a transaction is deleted.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct DeleteQuestion {
    pub message: String,
}

/// Everything the tab shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct TransactionsPage {
    pub reviewing: bool,
    pub all_shown: bool,
    /// "To Review (3)".
    pub review_title: String,
    /// Nothing to review: "All Caught Up", or "No Results" while searching.
    pub caught_up: bool,
    pub empty_title: String,
    pub empty_text: String,
    pub has_review: bool,
    /// "3 to Review".
    pub review_count: String,
    pub review: Vec<TxnItem>,
    /// "Approve All (2)", offered while some to review have a category.
    pub approve_all: String,
    pub can_approve_all: bool,
    /// Under To Review: how a row is reviewed.
    pub review_note: Option<String>,
    pub days: Vec<TxnDay>,
    pub categorizing: Option<CategorizeForm>,
    pub deleting: Option<DeleteQuestion>,
}

fn us(s: &State) -> &NumberStyle {
    &s.style.0
}

fn hex_ids(ids: &[String]) -> Vec<Id<Txn>> {
    ids.iter().filter_map(|i| crate::id_of(i)).collect()
}

fn found<'a>(data: &Data<'a, MoneyView>, id: &str) -> Option<&'a TxnLine> {
    let all: &[TxnLine] = data.rows(&MoneyView::Register { account: None });
    all.iter().find(|t| t.id.hex() == id)
}

/// Whether `t` is what `search` asks for: its payee, category, memo or amount.
fn matches(t: &TxnLine, search: &str, style: &NumberStyle) -> bool {
    let q = search.trim().to_lowercase();
    q.is_empty()
        || t.payee.to_lowercase().contains(&q)
        || t.category.to_lowercase().contains(&q)
        || t.memo.to_lowercase().contains(&q)
        || amount(t.amount, style).to_lowercase().contains(&q)
}

fn send(change: MoneyChange, then: ark_app::Sent<TransactionsMsg>) -> Vec<Command<Transactions>> {
    vec![Command::Send { change, then: Some(then) }]
}

fn sent(r: Result<(), String>) -> TransactionsMsg {
    TransactionsMsg::Sent { refused: r.err() }
}

fn approved(r: Result<(), String>) -> TransactionsMsg {
    TransactionsMsg::Approved { refused: r.err() }
}

impl App for Transactions {
    type Key = MoneyView;
    type State = State;
    type Msg = TransactionsMsg;
    type Change = MoneyChange;
    type Route = Route;
    type Request = MoneyRequest;
    type Screen = TransactionsPage;

    fn holds(s: &State) -> Vec<MoneyView> {
        let mut v = vec![MoneyView::Inbox, MoneyView::Register { account: None }];
        if !s.month.is_empty() {
            v.push(MoneyView::Lines { month: s.month.clone() });
        }
        v
    }

    fn update(s: &mut State, msg: TransactionsMsg, data: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        use TransactionsMsg as M;
        match msg {
            M::Show { review } => {
                s.review = review;
                vec![]
            }
            M::Search { text } => {
                s.search = text;
                vec![]
            }
            M::Month { month } => {
                s.month = month;
                vec![]
            }
            M::Refresh => vec![Command::Request(MoneyRequest::Refresh)],
            M::Open { id } => match found(data, &id) {
                Some(t) if t.needs_category() => {
                    s.choosing = Some(Choosing {
                        ids: vec![id],
                        approve: true,
                        chosen: None,
                    });
                    vec![]
                }
                Some(_) => vec![Command::Request(MoneyRequest::OpenTransaction { key: id })],
                None => vec![],
            },
            M::Edit { id } => vec![Command::Request(MoneyRequest::OpenTransaction { key: id })],
            M::Approve { id } => match found(data, &id).filter(|t| !t.approved && !t.needs_category()) {
                Some(t) => send(MoneyChange::Approve(Txns { ids: vec![t.id] }), approved),
                None => vec![],
            },
            M::ApproveAll => {
                let ids: Vec<Id<Txn>> = review_rows(s, data)
                    .iter()
                    .filter(|t| !t.approved && !t.needs_category())
                    .map(|t| t.id)
                    .collect();
                match ids.is_empty() {
                    true => vec![],
                    false => send(MoneyChange::Approve(Txns { ids }), approved),
                }
            }
            M::Categorize { id } => {
                if let Some(t) = found(data, &id) {
                    s.choosing = Some(Choosing {
                        ids: vec![id],
                        approve: !t.approved,
                        chosen: t.category_id.map(|c| c.hex()),
                    });
                }
                vec![]
            }
            M::Chose { key } => {
                if let Some(c) = &mut s.choosing {
                    c.chosen = Some(key);
                }
                vec![]
            }
            M::SaveCategory => {
                let Some(c) = s.choosing.clone() else { return vec![] };
                let Some(category_id) = c.chosen.as_deref().and_then(crate::id_of) else {
                    return vec![];
                };
                let ids = hex_ids(&c.ids);
                let mut out = send(
                    MoneyChange::Categorize(Categorize {
                        ids: ids.clone(),
                        category_id: Some(category_id),
                    }),
                    sent,
                );
                if c.approve {
                    out.extend(send(MoneyChange::Approve(Txns { ids }), approved));
                }
                s.choosing = None;
                out
            }
            M::Dismiss => {
                s.choosing = None;
                vec![]
            }
            M::Delete { id } => {
                s.deleting = found(data, &id).map(|t| t.id.hex());
                vec![]
            }
            M::CancelDelete => {
                s.deleting = None;
                vec![]
            }
            M::ConfirmDelete => match s.deleting.take() {
                Some(id) => send(MoneyChange::DeleteTransactions(Txns { ids: hex_ids(&[id]) }), sent),
                None => vec![],
            },
            M::Sent { .. } => vec![],
            M::Approved { refused } => match refused {
                None => vec![Command::ask(perform(Haptic::Success))],
                Some(_) => vec![],
            },
        }
    }

    fn screen(s: &State, data: &Data<'_, MoneyView>) -> TransactionsPage {
        let style = us(s);
        let review: Vec<&TxnLine> = review_rows(s, data);
        let waiting: &[TxnLine] = data.rows(&MoneyView::Inbox);
        let all: &[TxnLine] = data.rows(&MoneyView::Register { account: None });
        let mut days: Vec<TxnDay> = vec![];
        for t in all.iter().filter(|t| matches(t, &s.search, style)) {
            let title = ark_format::day_long(&t.date);
            match days.last_mut() {
                Some(d) if d.title == title => d.rows.push(item(t, style)),
                _ => days.push(TxnDay {
                    title,
                    rows: vec![item(t, style)],
                }),
            }
        }
        let ready = review.iter().filter(|t| !t.approved && !t.needs_category()).count();
        let searching = !s.search.trim().is_empty();
        let lines: &[BudgetLine] = data.rows(&MoneyView::Lines { month: s.month.clone() });
        TransactionsPage {
            reviewing: s.review,
            all_shown: !s.review,
            review_title: match waiting.len() {
                0 => "To Review".into(),
                n => format!("To Review ({n})"),
            },
            caught_up: review.is_empty(),
            empty_title: if searching { "No Results" } else { "All Caught Up" }.into(),
            empty_text: match searching {
                true => format!("Nothing waiting matches \u{201c}{}\u{201d}.", s.search.trim()),
                false => {
                    "Everything from the bank has been looked at, and everything has a category. Pull down to ask the bank for anything new.".into()
                }
            },
            has_review: !review.is_empty(),
            review_count: format!("{} to Review", review.len()),
            review: review.iter().map(|t| item(t, style)).collect(),
            approve_all: format!("Approve All ({ready})"),
            can_approve_all: ready > 0,
            review_note: Some("Tap one that needs a category to choose it; hold one for what else can be done.".into()),
            days,
            categorizing: s.choosing.as_ref().map(|c| {
                let mut ls: Vec<&BudgetLine> = lines.iter().filter(|l| !l.hidden && !l.is_payment()).collect();
                ls.sort_by_key(|l| (l.group_pos, l.pos));
                CategorizeForm {
                    chosen: c.chosen.clone().unwrap_or_default(),
                    choices: ls
                        .iter()
                        .map(|l| {
                            let o = crate::look::category_offered(l, style);
                            CategoryChoice {
                                key: l.category_id.hex(),
                                title: l.name.clone(),
                                glyph: crate::look::category_glyph(&l.name).into(),
                                group: o.group,
                                detail: o.detail,
                                tint: o.tint.into(),
                            }
                        })
                        .collect(),
                    confirm: if c.approve { "Categorize and Approve" } else { "Categorize" }.into(),
                    can_save: c.chosen.is_some(),
                }
            }),
            deleting: s.deleting.as_deref().and_then(|id| found(data, id)).map(|t| DeleteQuestion {
                message: format!(
                    "{}, {}, on {}. This can't be undone.",
                    t.payee,
                    amount(t.amount, style),
                    ark_format::day_long(&t.date)
                ),
            }),
        }
    }
}

/// What To Review lists: what needs a look, found by the search.
fn review_rows<'a>(s: &State, data: &Data<'a, MoneyView>) -> Vec<&'a TxnLine> {
    let inbox: &[TxnLine] = data.rows(&MoneyView::Inbox);
    inbox.iter().filter(|t| matches(t, &s.search, us(s))).collect()
}

fn item(t: &TxnLine, style: &NumberStyle) -> TxnItem {
    let category = match (t.transfer, t.needs_category()) {
        (true, _) => "Transfer".to_string(),
        (_, true) => "Needs a Category".to_string(),
        _ => t.category.clone(),
    };
    let detail = match t.approved {
        true => format!("{category} \u{b7} {}", t.account),
        false => format!("New \u{b7} {category} \u{b7} {}", t.account),
    };
    TxnItem {
        id: t.id.hex(),
        payee: t.payee.clone(),
        said: format!("{}, {}, {}", t.payee, amount(t.amount, style), detail.replace(" \u{b7} ", ", ")),
        detail,
        amount: t.amount,
        can_approve: !t.approved && !t.needs_category(),
    }
}
