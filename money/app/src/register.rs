//! A register: one account's transactions, or every account's (All
//! Accounts), a day at a time, newest first, as a table — its payee, its
//! category (or that it needs one, in the caution's tint), the account
//! when it is every account's, its memo, its amount (an inflow in the
//! positive tint) and its cleared mark, with a dot while the bank's
//! import waits for a look. Over it the account's balances: the working
//! balance, the cleared and uncleared, and what the bank last said. A row
//! tapped opens its details (the inspector's, or its form); held, it is
//! edited, categorized, approved, cleared or uncleared, or deleted (asked
//! first). The account is reconciled, its waiting rows approved at once,
//! or it is closed or reopened. What is searched is the shell's.

use ark_app::modules::haptics::{perform, Haptic};
use ark_app::view::{Cells, TableCell};
use ark_app::{App, Command, Data};
use ark_format::{amount, amount_signed, NumberStyle};
use money_core::*;

use crate::accounts::Style;
use crate::routes::Route;
use crate::sheets::SheetsMsg;
use crate::MoneyRequest;

pub mod view;

#[cfg(test)]
mod tests;

/// A register, as `ark_app` runs it.
pub struct Register;

/// What the register is in the middle of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The account's hex id; none is every account's.
    pub account: Option<String>,
    pub search: String,
    /// The transaction the inspector is about, lit.
    pub selected: Option<String>,
    /// A transaction asked about before it is deleted.
    pub deleting: Option<String>,
    pub style: Style,
}

/// Everything a register can be told. Ids are rows' `Id::hex`.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum RegisterMsg {
    /// The account it is the register of (none: every account's).
    Show {
        account: Option<String>,
    },
    /// What the shell's search field says.
    Search {
        text: String,
    },
    /// The transaction the inspector is about.
    Select {
        id: Option<String>,
    },
    Refresh,
    /// A row tapped: its details.
    Open {
        id: String,
    },
    Edit {
        id: String,
    },
    Categorize {
        id: String,
    },
    Approve {
        id: String,
    },
    /// Every one in it waiting for a look approved.
    ApproveAll,
    /// Marked cleared, or uncleared.
    Clear {
        id: String,
        cleared: bool,
    },
    Delete {
        id: String,
    },
    ConfirmDelete,
    CancelDelete,
    Reconcile,
    /// The account closed, or reopened.
    Close {
        closed: bool,
    },
    /// What became of a change sent; `felt` when the hand should know.
    Sent {
        refused: Option<String>,
    },
    Felt {
        refused: Option<String>,
    },
}

/// A day of the register, its heading and its rows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct RegisterDay {
    /// "Today", "Yesterday", or "Monday, October 26".
    pub title: String,
    pub rows: Vec<RegisterRow>,
}

/// One transaction's row: a cell for each of the table's columns.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct RegisterRow {
    pub id: String,
    pub cells: Vec<String>,
    pub tints: Vec<String>,
    pub glyphs: Vec<String>,
    /// The bank's import, not yet looked at.
    pub waiting: bool,
    pub selected: bool,
    pub said: String,
    pub can_approve: bool,
    pub can_categorize: bool,
    pub can_mark_cleared: bool,
    pub can_mark_uncleared: bool,
}

/// Everything a register shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct RegisterPage {
    /// An account's (its balances over it), or every account's.
    pub one: bool,
    pub all: bool,
    pub owes: bool,
    pub owed: bool,
    pub balance: i64,
    pub cleared: i64,
    pub uncleared: i64,
    /// Its working balance at the end of each of its last days, oldest first.
    pub trend: Vec<i64>,
    /// "The bank says $1,204.00", while it does.
    pub bank: String,
    pub has_bank: bool,
    pub empty: bool,
    pub empty_title: String,
    pub empty_text: String,
    pub days: Vec<RegisterDay>,
    /// "Approve All (3)", while some wait.
    pub approve_all: String,
    pub can_approve_all: bool,
    pub can_reconcile: bool,
    /// "Close Account", or "Reopen Account".
    pub close_title: String,
    pub closed: bool,
    pub deleting: bool,
    pub delete_message: String,
}

fn us(s: &State) -> &NumberStyle {
    &s.style.0
}

fn key(s: &State) -> MoneyView {
    MoneyView::Register {
        account: s.account.as_deref().and_then(crate::id_of),
    }
}

fn rows<'a>(s: &State, data: &Data<'a, MoneyView>) -> &'a [TxnLine] {
    data.rows(&key(s))
}

fn found<'a>(s: &State, data: &Data<'a, MoneyView>, id: &str) -> Option<&'a TxnLine> {
    rows(s, data).iter().find(|t| t.id.hex() == id)
}

fn account<'a>(s: &State, data: &Data<'a, MoneyView>) -> Option<&'a AccountLine> {
    let id = s.account.as_deref()?;
    data.rows::<AccountLine>(&MoneyView::Accounts).iter().find(|a| a.id.hex() == id)
}

/// Whether `t` is what `search` asks for: its payee, what the bank called
/// it, its memo, category, account or amount.
fn matches(t: &TxnLine, search: &str, style: &NumberStyle) -> bool {
    let q = search.trim().to_lowercase();
    q.is_empty()
        || t.payee.to_lowercase().contains(&q)
        || t.imported_payee.to_lowercase().contains(&q)
        || t.memo.to_lowercase().contains(&q)
        || t.category.to_lowercase().contains(&q)
        || t.account.to_lowercase().contains(&q)
        || amount(t.amount, style).to_lowercase().contains(&q)
}

fn send(change: MoneyChange, felt: bool) -> Vec<Command<Register>> {
    vec![Command::Send {
        change,
        then: Some(match felt {
            true => |r: Result<(), String>| RegisterMsg::Felt { refused: r.err() },
            false => |r: Result<(), String>| RegisterMsg::Sent { refused: r.err() },
        }),
    }]
}

fn sheet(msg: SheetsMsg) -> Vec<Command<Register>> {
    vec![Command::Request(MoneyRequest::Sheet { msg })]
}

impl App for Register {
    type Key = MoneyView;
    type State = State;
    type Msg = RegisterMsg;
    type Change = MoneyChange;
    type Route = Route;
    type Request = MoneyRequest;
    type Screen = RegisterPage;

    fn holds(s: &State) -> Vec<MoneyView> {
        vec![key(s), MoneyView::Accounts]
    }

    fn update(s: &mut State, msg: RegisterMsg, data: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        use RegisterMsg as M;
        match msg {
            M::Show { account } => {
                if s.account != account {
                    s.account = account;
                    s.deleting = None;
                }
                vec![]
            }
            M::Search { text } => {
                s.search = text;
                vec![]
            }
            M::Select { id } => {
                s.selected = id;
                vec![]
            }
            M::Refresh => vec![Command::Request(MoneyRequest::Refresh)],
            M::Open { id } => vec![Command::Request(MoneyRequest::OpenTransaction { key: id })],
            M::Edit { id } => sheet(SheetsMsg::Open { key: id }),
            M::Categorize { id } => sheet(SheetsMsg::Categorize {
                ids: vec![id],
                approve: false,
            }),
            M::Approve { id } => match found(s, data, &id).filter(|t| !t.approved) {
                Some(t) => send(MoneyChange::Approve(Txns { ids: vec![t.id] }), true),
                None => vec![],
            },
            M::ApproveAll => {
                let ids: Vec<Id<Txn>> = rows(s, data).iter().filter(|t| !t.approved).map(|t| t.id).collect();
                match ids.is_empty() {
                    true => vec![],
                    false => send(MoneyChange::Approve(Txns { ids }), true),
                }
            }
            M::Clear { id, cleared } => match found(s, data, &id).filter(|t| t.cleared != Cleared::Reconciled) {
                Some(t) => send(MoneyChange::SetCleared(SetCleared { id: t.id, cleared }), false),
                None => vec![],
            },
            M::Delete { id } => {
                s.deleting = found(s, data, &id).map(|t| t.id.hex());
                vec![]
            }
            M::CancelDelete => {
                s.deleting = None;
                vec![]
            }
            M::ConfirmDelete => match s.deleting.take().as_deref().and_then(crate::id_of) {
                Some(id) => send(MoneyChange::DeleteTransactions(Txns { ids: vec![id] }), false),
                None => vec![],
            },
            M::Reconcile => match account(s, data).filter(|a| !a.closed) {
                Some(a) => sheet(SheetsMsg::Reconcile { account: a.id.hex() }),
                None => vec![],
            },
            M::Close { closed } => match account(s, data) {
                Some(a) => send(MoneyChange::CloseAccount(CloseAccount { id: a.id, closed }), false),
                None => vec![],
            },
            M::Sent { .. } => vec![],
            M::Felt { refused } => match refused {
                None => vec![Command::ask(perform(Haptic::Success))],
                Some(_) => vec![],
            },
        }
    }

    fn screen(s: &State, data: &Data<'_, MoneyView>) -> RegisterPage {
        let style = us(s);
        let all = rows(s, data);
        let a = account(s, data);
        let shown: Vec<&TxnLine> = all.iter().filter(|t| matches(t, &s.search, style)).collect();
        let every = a.is_none();
        let today = money_core::dates::day_of_ms(data.now_ms);
        let mut days: Vec<RegisterDay> = vec![];
        for t in &shown {
            let title = ark_format::day_heading(&t.date, &today);
            let row = row(t, every, s.selected.as_deref() == Some(&t.id.hex()), style);
            match days.last_mut() {
                Some(d) if d.title == title => d.rows.push(row),
                _ => days.push(RegisterDay { title, rows: vec![row] }),
            }
        }
        let searching = !s.search.trim().is_empty();
        let waiting = all.iter().filter(|t| !t.approved).count();
        let owes = a.and_then(|a| a.account_kind()).is_some_and(|k| k.owes());
        RegisterPage {
            one: !every,
            all: every,
            owes: !every && owes,
            owed: !every && !owes,
            balance: a.map(|a| a.balance).unwrap_or(0),
            cleared: a.map(|a| a.cleared).unwrap_or(0),
            uncleared: a.map(|a| a.uncleared).unwrap_or(0),
            trend: trend(all, a.map(|a| a.balance).unwrap_or(0)),
            bank: a
                .and_then(|a| a.bank_balance)
                .map(|b| format!("The bank says {}", amount(b, style)))
                .unwrap_or_default(),
            has_bank: a.is_some_and(|a| a.bank_balance.is_some()),
            empty: shown.is_empty(),
            empty_title: if searching { "No Results" } else { "No Transactions" }.into(),
            empty_text: match searching {
                true => format!("Nothing matches \u{201c}{}\u{201d}.", s.search.trim()),
                false => "Add one with the + button.".into(),
            },
            days,
            approve_all: format!("Approve All ({waiting})"),
            can_approve_all: waiting > 0,
            can_reconcile: a.is_some_and(|a| !a.closed),
            close_title: match a.is_some_and(|a| a.closed) {
                true => "Reopen Account".into(),
                false => "Close Account".into(),
            },
            closed: a.is_some_and(|a| a.closed),
            deleting: s.deleting.is_some(),
            delete_message: s
                .deleting
                .as_deref()
                .and_then(|id| found(s, data, id))
                .map(|t| {
                    format!(
                        "{}, {}, on {}. This can't be undone.",
                        t.payee,
                        amount(t.amount, style),
                        ark_format::day_long(&t.date)
                    )
                })
                .unwrap_or_default(),
        }
    }
}

/// The working balance at the end of each of the last days it moved,
/// oldest first: `balance` now, less what came after.
fn trend(rows: &[TxnLine], balance: i64) -> Vec<i64> {
    let mut out = vec![balance];
    let mut b = balance;
    let mut day: Option<&str> = None;
    for t in rows {
        if day.is_some_and(|d| d != t.date) {
            out.push(b);
            if out.len() >= 30 {
                break;
            }
        }
        day = Some(&t.date);
        b -= t.amount;
    }
    out.reverse();
    out
}

/// The table's columns' cells for `t`: payee, category, the account when
/// every account's, memo, amount and its cleared mark.
fn row(t: &TxnLine, every: bool, selected: bool, style: &NumberStyle) -> RegisterRow {
    let needs = t.needs_category();
    let category = match (t.transfer, needs) {
        (true, _) => TableCell {
            text: t.category.clone(),
            tint: "quiet".into(),
            glyph: "arrow-left-right".into(),
        },
        (_, true) => TableCell::tinted("Needs a Category", "caution"),
        _ => TableCell::plain(t.category.clone()),
    };
    let marked = !t.receipt.is_empty() || t.photos > 0;
    let mut cells = vec![
        TableCell::plain(if t.payee.is_empty() { "No Payee".to_string() } else { t.payee.clone() }),
        category,
    ];
    if every {
        cells.push(TableCell::tinted(t.account.clone(), "quiet"));
    }
    cells.push(TableCell {
        text: t.memo.clone(),
        tint: "quiet".into(),
        glyph: if marked { "receipt".into() } else { String::new() },
    });
    cells.push(TableCell::tinted(
        match t.amount > 0 {
            true => amount_signed(t.amount, style),
            false => amount(t.amount, style),
        },
        if t.amount > 0 { "positive" } else { "" },
    ));
    let (glyph, tint, words) = match t.cleared {
        Cleared::Uncleared => ("circle", "quiet", "uncleared"),
        Cleared::Cleared => ("circle-check", "accent", "cleared"),
        Cleared::Reconciled => ("lock", "quiet", "reconciled"),
    };
    cells.push(TableCell::glyph(glyph, tint));
    let c = Cells::of(cells);
    RegisterRow {
        id: t.id.hex(),
        cells: c.text,
        tints: c.tint,
        glyphs: c.glyph,
        waiting: !t.approved,
        selected,
        said: format!(
            "{}, {}, {}, {words}{}{}",
            if t.payee.is_empty() { "No payee" } else { &t.payee },
            amount(t.amount, style),
            if needs { "needs a category" } else { &t.category },
            if t.approved { "" } else { ", waiting for review" },
            if marked { ", has a receipt" } else { "" },
        ),
        can_approve: !t.approved,
        can_categorize: !t.transfer,
        can_mark_cleared: t.cleared == Cleared::Uncleared,
        can_mark_uncleared: t.cleared == Cleared::Cleared,
    }
}
