//! The sheets any of money's screens raises over the app: a transaction
//! added, looked at and corrected (its amount on the keypad, its payee
//! with the payees offered, its category or the account a transfer goes
//! to, its account, day, cleared mark and memo; deleted, asked first);
//! transactions given a category, and approved with it; and an account
//! reconciled with the bank. One runtime of their own, so a register, the
//! Transactions tab, the shell's toolbar and a launch all raise the same
//! one ([`MoneyRequest::Sheet`]).
//!
//! The form is the core's [`Draft`] (its rules, [`Draft::problem`], and
//! the change it is, [`Draft::to_change`]) with what the form shows beside
//! it: the amount entered unsigned and its way apart, the way the iPhone's
//! keypad takes one. A sheet closes once the domain has taken what it said
//! and stays up with the domain's sentence when it refused.

use ark_app::modules::haptics::{perform, Haptic};
use ark_app::{App, Command, Data};
use ark_format::{amount, NumberStyle};
use money_core::transactions::{suggest, Draft};
use money_core::*;

use crate::accounts::Style;
use crate::budget::Figure;
use crate::routes::Route;
use crate::shell::inspect::transaction_named;
use crate::MoneyRequest;

pub mod view;

#[cfg(test)]
mod tests;

/// The sheets, as `ark_app` runs them.
pub struct Sheets;

/// The transaction form, as it is being filled.
#[derive(Clone, Debug, PartialEq)]
pub struct TxnForm {
    pub draft: Draft,
    /// The amount, never below nothing; `inflow` says which way.
    pub figure: Figure,
    pub inflow: bool,
    /// A transfer already: it stays one, only its other account chosen.
    pub transfer: bool,
    /// Delete was pressed: asked first.
    pub deleting: bool,
    /// The domain's sentence, when it refused what was saved.
    pub refused: Option<String>,
}

/// Transactions (hex ids) being given a category, approved with it when `approve`.
#[derive(Clone, Debug, PartialEq)]
pub struct Choosing {
    pub ids: Vec<String>,
    pub approve: bool,
    pub chosen: Option<String>,
}

/// An account (hex id) being agreed with the bank: asked first whether its
/// cleared balance is the bank's; when it is not, what the bank says.
#[derive(Clone, Debug, PartialEq)]
pub struct Reconciling {
    pub account: String,
    pub differs: bool,
    pub figure: Figure,
}

/// The sheet up, if any.
#[derive(Clone, Debug, PartialEq)]
pub enum Up {
    Txn(Box<TxnForm>),
    Categorize(Choosing),
    Reconcile(Reconciling),
}

/// What the sheets are in the middle of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub up: Option<Up>,
    /// The month the budget is on (`2026-11`): the categories offered and their figures.
    pub month: String,
    /// The session's day (`2026-11-11`): a new transaction's, and a reconciling's.
    pub today: String,
    pub style: Style,
}

/// Everything the sheets can be told. Ids are rows' `Id::hex`.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum SheetsMsg {
    /// The month the budget is on, and today, as the platform has them.
    Month {
        month: String,
        today: String,
    },
    /// A new transaction, in this account (none: the usual one).
    New {
        account: Option<String>,
    },
    /// The transaction a key names (`ark_app`'s launch keys: its id, `receipt`,
    /// `review`, `transfer`, a number, a payee), its form up.
    Open {
        key: String,
    },
    /// Its way: money coming in (`inflow`), or going out (`outflow`).
    Way {
        way: String,
    },
    Typed {
        typed: String,
    },
    Cents {
        cents: i64,
    },
    Payee {
        payee: String,
    },
    /// One of the payees offered: its last category comes with it.
    ChosePayee {
        name: String,
    },
    /// A category (`c<hex>`), Ready to Assign (`ready`) or a transfer's
    /// other account (`t<hex>`) chosen.
    Category {
        key: String,
    },
    Account {
        key: String,
    },
    Day {
        day: String,
    },
    Cleared {
        cleared: bool,
    },
    Memo {
        memo: String,
    },
    Save,
    Delete,
    ConfirmDelete,
    CancelDelete,
    /// Transactions given a category, approved with it when `approve`.
    Categorize {
        ids: Vec<String>,
        approve: bool,
    },
    ChoseCategory {
        key: String,
    },
    SaveCategory,
    /// An account reconciled with the bank.
    Reconcile {
        account: String,
    },
    /// Its cleared balance is what the bank says: what was cleared locked.
    Agree,
    /// It is not: what the bank says is asked.
    Differs,
    ReconcileTyped {
        typed: String,
    },
    ReconcileCents {
        cents: i64,
    },
    SaveReconcile,
    /// The sheet put away.
    Dismiss,
    /// What became of a change sent.
    Sent {
        refused: Option<String>,
    },
    /// What became of the sheet's own change: it closes when taken.
    Done {
        refused: Option<String>,
    },
}

/// One payee offered under the payee field.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct PayeeChoice {
    pub name: String,
    /// Its last category.
    pub category: String,
}

/// One category, account or Ready to Assign a picker offers.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct SheetChoice {
    pub key: String,
    pub title: String,
    pub glyph: String,
}

/// The transaction form as it shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct TxnSheet {
    /// A new one: "New Transaction"; else "Transaction".
    pub adding: bool,
    pub editing: bool,
    /// "outflow" or "inflow".
    pub way: String,
    pub ways: Vec<SheetChoice>,
    pub prompt: String,
    pub typed: String,
    pub cents: i64,
    pub amount_problem: Option<String>,
    pub payee: String,
    /// A transfer has no payee of its own: its other account names it.
    pub has_payee: bool,
    pub payees: Vec<PayeeChoice>,
    pub has_payees: bool,
    /// What the bank called it, while it says more than its payee.
    pub bank: String,
    pub has_bank: bool,
    /// The category's key, and the choices (the account a transfer goes to among them).
    pub category: String,
    pub categories: Vec<SheetChoice>,
    /// Under the category, for one already there: what choosing an account does.
    pub category_note: String,
    pub account: String,
    pub accounts: Vec<SheetChoice>,
    /// Its account's name, when it can no longer change (one corrected).
    pub account_name: String,
    pub day: String,
    pub cleared: bool,
    pub memo: String,
    /// The domain's refusal, or why it cannot be saved yet, or "Entered by sam".
    pub note: String,
    pub can_save: bool,
    /// Delete was pressed: the question, and what it says.
    pub deleting: bool,
    pub delete_message: String,
}

/// Transactions being given a category.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct CategorizeSheet {
    pub chosen: String,
    pub choices: Vec<SheetChoice>,
    pub can_save: bool,
}

/// An account being reconciled.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ReconcileSheet {
    /// "Is your cleared balance $1,204.00?"
    pub question: String,
    /// "The bank last said $1,204.00", while it has said.
    pub bank: String,
    pub has_bank: bool,
    pub asking: bool,
    pub differs: bool,
    pub prompt: String,
    pub typed: String,
    pub cents: i64,
    pub problem: Option<String>,
}

/// Every sheet's record; at most one is up.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct SheetsPage {
    pub transaction: Option<TxnSheet>,
    pub categorizing: Option<CategorizeSheet>,
    pub reconciling: Option<ReconcileSheet>,
}

fn us(s: &State) -> &NumberStyle {
    &s.style.0
}

fn all<'a>(data: &Data<'a, MoneyView>) -> &'a [TxnLine] {
    data.rows(&MoneyView::Register { account: None })
}

fn found<'a>(data: &Data<'a, MoneyView>, id: &str) -> Option<&'a TxnLine> {
    all(data).iter().find(|t| t.id.hex() == id)
}

fn accounts<'a>(data: &Data<'a, MoneyView>) -> &'a [AccountLine] {
    data.rows(&MoneyView::Accounts)
}

/// The month's categories a transaction can be for, by their groups:
/// never a card's payment category (money goes there by a transfer).
fn lines<'a>(s: &State, data: &Data<'a, MoneyView>) -> Vec<&'a BudgetLine> {
    let all: &[BudgetLine] = data.rows(&MoneyView::Lines { month: s.month.clone() });
    let mut ls: Vec<&BudgetLine> = all.iter().filter(|l| !l.hidden && !l.is_payment()).collect();
    ls.sort_by_key(|l| (l.group_pos, l.pos));
    ls
}

fn txn(s: &mut State) -> Option<&mut TxnForm> {
    match &mut s.up {
        Some(Up::Txn(f)) => Some(f),
        _ => None,
    }
}

fn edit(s: &mut State, f: impl FnOnce(&mut TxnForm)) -> Vec<Command<Sheets>> {
    if let Some(t) = txn(s) {
        f(t);
        t.refused = None;
    }
    vec![]
}

fn form_of(t: &TxnLine, style: &NumberStyle) -> TxnForm {
    TxnForm {
        draft: Draft::of(t),
        figure: Figure::of(t.amount.abs(), style),
        inflow: t.amount > 0,
        transfer: t.transfer,
        deleting: false,
        refused: None,
    }
}

/// The draft as it is sent: its amount in the domain's sign.
fn sent_draft(f: &TxnForm, style: &NumberStyle) -> Option<Draft> {
    let cents = f.figure.cents(style)?;
    Some(Draft {
        amount: if f.inflow { cents } else { -cents },
        ..f.draft.clone()
    })
}

fn send(change: MoneyChange, then: ark_app::Sent<SheetsMsg>) -> Vec<Command<Sheets>> {
    vec![Command::Send { change, then: Some(then) }]
}

fn done(r: Result<(), String>) -> SheetsMsg {
    SheetsMsg::Done { refused: r.err() }
}

fn sent(r: Result<(), String>) -> SheetsMsg {
    SheetsMsg::Sent { refused: r.err() }
}

/// A category's key in a picker: `ready`, `c<hex>`, a transfer's `t<hex>`.
const READY: &str = "ready";

impl App for Sheets {
    type Key = MoneyView;
    type State = State;
    type Msg = SheetsMsg;
    type Change = MoneyChange;
    type Route = Route;
    type Request = MoneyRequest;
    type Screen = SheetsPage;

    fn holds(s: &State) -> Vec<MoneyView> {
        let mut v = vec![MoneyView::Register { account: None }, MoneyView::Accounts];
        if matches!(s.up, Some(Up::Txn(_))) {
            v.push(MoneyView::Payees);
        }
        if !s.month.is_empty() {
            v.push(MoneyView::Lines { month: s.month.clone() });
            v.push(MoneyView::Summary { month: s.month.clone() });
        }
        v
    }

    fn update(s: &mut State, msg: SheetsMsg, data: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        use SheetsMsg as M;
        match msg {
            M::Month { month, today } => {
                s.month = month;
                s.today = today;
                vec![]
            }
            M::New { account } => {
                let account = account.as_deref().and_then(crate::id_of);
                s.up = Some(Up::Txn(Box::new(TxnForm {
                    draft: Draft::new(accounts(data), account, &s.today),
                    figure: Figure::default(),
                    inflow: false,
                    transfer: false,
                    deleting: false,
                    refused: None,
                })));
                vec![]
            }
            M::Open { key } => {
                if let Some(t) = transaction_named(all(data), &key) {
                    s.up = Some(Up::Txn(Box::new(form_of(t, us(s)))));
                }
                vec![]
            }
            M::Way { way } => edit(s, |f| f.inflow = way == "inflow"),
            M::Typed { typed } => edit(s, |f| {
                f.figure.typed = typed;
                f.figure.entered = None;
            }),
            M::Cents { cents } => edit(s, |f| f.figure.entered = Some(cents.abs())),
            M::Payee { payee } => edit(s, |f| f.draft.payee = payee),
            M::ChosePayee { name } => {
                let payees: &[PayeeLine] = data.rows(&MoneyView::Payees);
                match payees.iter().find(|p| p.name == name) {
                    Some(p) => edit(s, |f| f.draft = f.draft.with_payee(p)),
                    None => vec![],
                }
            }
            M::Category { key } => {
                if key == READY {
                    return edit(s, |f| f.draft = f.draft.with_category(Some(money_core::budget::ready_to_assign())));
                }
                match (key.split_at_checked(1), key.len() > 1) {
                    (Some(("c", hex)), true) => match crate::id_of(hex) {
                        Some(c) => edit(s, |f| f.draft = f.draft.with_category(Some(c))),
                        None => vec![],
                    },
                    (Some(("t", hex)), true) => match crate::id_of(hex) {
                        Some(a) => edit(s, |f| f.draft = f.draft.with_transfer(a)),
                        None => vec![],
                    },
                    _ => vec![],
                }
            }
            M::Account { key } => match crate::id_of(&key) {
                Some(a) => edit(s, |f| {
                    if f.draft.editing.is_none() {
                        f.draft.account = Some(a)
                    }
                }),
                None => vec![],
            },
            M::Day { day } => edit(s, |f| f.draft.date = day),
            M::Cleared { cleared } => edit(s, |f| f.draft.cleared = cleared),
            M::Memo { memo } => edit(s, |f| f.draft.memo = memo),
            M::Save => {
                let style = us(s).clone();
                let Some(f) = txn(s) else { return vec![] };
                match sent_draft(f, &style).and_then(|d| d.to_change()) {
                    Some(change) => send(change, done),
                    None => vec![],
                }
            }
            M::Delete => edit(s, |f| f.deleting = f.draft.editing.is_some()),
            M::CancelDelete => edit(s, |f| f.deleting = false),
            M::ConfirmDelete => match txn(s).and_then(|f| f.draft.editing) {
                Some(id) => {
                    s.up = None;
                    send(MoneyChange::DeleteTransactions(Txns { ids: vec![id] }), sent)
                }
                None => vec![],
            },
            M::Categorize { ids, approve } => {
                let chosen = match ids.as_slice() {
                    [one] => found(data, one).and_then(|t| t.category_id).map(|c| format!("c{}", c.hex())),
                    _ => None,
                };
                s.up = Some(Up::Categorize(Choosing { ids, approve, chosen }));
                vec![]
            }
            M::ChoseCategory { key } => {
                if let Some(Up::Categorize(c)) = &mut s.up {
                    c.chosen = Some(key);
                }
                vec![]
            }
            M::SaveCategory => {
                let Some(Up::Categorize(c)) = &s.up else { return vec![] };
                let category_id = match c.chosen.as_deref() {
                    Some(READY) => Some(money_core::budget::ready_to_assign()),
                    Some(k) => k.strip_prefix('c').and_then(crate::id_of),
                    None => return vec![],
                };
                let ids: Vec<Id<Txn>> = c.ids.iter().filter_map(|i| crate::id_of(i)).collect();
                let mut out = send(
                    MoneyChange::Categorize(Categorize {
                        ids: ids.clone(),
                        category_id,
                    }),
                    done,
                );
                if c.approve {
                    out.extend(send(MoneyChange::Approve(Txns { ids }), sent));
                }
                out
            }
            M::Reconcile { account } => {
                s.up = accounts(data).iter().find(|a| a.id.hex() == account && !a.closed).map(|a| {
                    Up::Reconcile(Reconciling {
                        account,
                        differs: false,
                        figure: Figure::of(a.bank_balance.unwrap_or(a.cleared).abs(), us(s)),
                    })
                });
                vec![]
            }
            M::Differs => {
                if let Some(Up::Reconcile(r)) = &mut s.up {
                    r.differs = true;
                }
                vec![]
            }
            M::ReconcileTyped { typed } => {
                if let Some(Up::Reconcile(r)) = &mut s.up {
                    r.figure.typed = typed;
                    r.figure.entered = None;
                }
                vec![]
            }
            M::ReconcileCents { cents } => {
                if let Some(Up::Reconcile(r)) = &mut s.up {
                    r.figure.entered = Some(cents.abs());
                }
                vec![]
            }
            M::Agree => reconcile(s, data, true),
            M::SaveReconcile => reconcile(s, data, false),
            M::Dismiss => {
                s.up = None;
                vec![]
            }
            M::Sent { .. } => vec![],
            M::Done { refused } => match refused {
                None => {
                    s.up = None;
                    vec![Command::ask(perform(Haptic::Success))]
                }
                Some(why) => {
                    if let Some(f) = txn(s) {
                        f.refused = Some(why);
                    }
                    vec![]
                }
            },
        }
    }

    fn screen(s: &State, data: &Data<'_, MoneyView>) -> SheetsPage {
        let style = us(s);
        SheetsPage {
            transaction: match &s.up {
                Some(Up::Txn(f)) => Some(txn_sheet(s, f, data)),
                _ => None,
            },
            categorizing: match &s.up {
                Some(Up::Categorize(c)) => {
                    let mut choices = vec![SheetChoice {
                        key: READY.into(),
                        title: "Ready to Assign".into(),
                        glyph: "dollar-sign".into(),
                    }];
                    choices.extend(lines(s, data).iter().map(|l| category_choice(l)));
                    Some(CategorizeSheet {
                        chosen: c.chosen.clone().unwrap_or_default(),
                        choices,
                        can_save: c.chosen.is_some(),
                    })
                }
                _ => None,
            },
            reconciling: match &s.up {
                Some(Up::Reconcile(r)) => accounts(data).iter().find(|a| a.id.hex() == r.account).map(|a| {
                    let owes = a.account_kind().is_some_and(|k| k.owes());
                    let cents = r.figure.cents(style);
                    ReconcileSheet {
                        question: format!("Is {}'s cleared balance {}?", a.name, amount(a.cleared, style)),
                        bank: a
                            .bank_balance
                            .map(|b| format!("The bank last said {}", amount(b, style)))
                            .unwrap_or_default(),
                        has_bank: a.bank_balance.is_some(),
                        asking: !r.differs,
                        differs: r.differs,
                        prompt: match owes {
                            true => "What the bank says is owed".into(),
                            false => "What the bank says it is".into(),
                        },
                        typed: r.figure.typed.clone(),
                        cents: cents.unwrap_or(0),
                        problem: match cents {
                            None => Some("That isn't an amount.".into()),
                            Some(_) => None,
                        },
                    }
                }),
                _ => None,
            },
        }
    }
}

/// The reconciling account agreed with the bank: at its cleared balance
/// (`agree`), or at what the bank says.
fn reconcile(s: &State, data: &Data<'_, MoneyView>, agree: bool) -> Vec<Command<Sheets>> {
    let Some(Up::Reconcile(r)) = &s.up else { return vec![] };
    let Some(a) = accounts(data).iter().find(|a| a.id.hex() == r.account) else {
        return vec![];
    };
    let balance = match agree {
        true => a.cleared,
        false => {
            let Some(c) = r.figure.cents(us(s)) else { return vec![] };
            match a.account_kind().is_some_and(|k| k.owes()) {
                true => -c,
                false => c,
            }
        }
    };
    send(
        MoneyChange::Reconcile(Reconcile {
            account_id: a.id,
            balance,
            date: s.today.clone(),
        }),
        done,
    )
}

fn category_choice(l: &BudgetLine) -> SheetChoice {
    SheetChoice {
        key: format!("c{}", l.category_id.hex()),
        title: l.name.clone(),
        glyph: crate::look::category_glyph(&l.name).into(),
    }
}

fn txn_sheet(s: &State, f: &TxnForm, data: &Data<'_, MoneyView>) -> TxnSheet {
    let style = us(s);
    let d = &f.draft;
    let open: Vec<&AccountLine> = accounts(data).iter().filter(|a| !a.closed).collect();
    let editing = found(data, &d.editing.map(|i| i.hex()).unwrap_or_default());
    let cents = f.figure.cents(style);
    let sent = sent_draft(f, style);
    let payees: &[PayeeLine] = data.rows(&MoneyView::Payees);
    let lines = lines(s, data);
    let offered: Vec<PayeeChoice> = match d.transfer_to.is_some() || d.payee.trim().is_empty() {
        true => vec![],
        false => suggest(&d.payee, payees, 4)
            .into_iter()
            .map(|p| PayeeChoice {
                category: p
                    .category_id
                    .and_then(|c| lines.iter().find(|l| l.category_id == c))
                    .map(|l| l.name.clone())
                    .unwrap_or_default(),
                name: p.name,
            })
            .collect(),
    };
    // A transfer stays one: only its other account is chosen again.
    let mut categories = vec![];
    if !f.transfer {
        if f.inflow || d.category == Some(money_core::budget::ready_to_assign()) {
            categories.push(SheetChoice {
                key: READY.into(),
                title: "Ready to Assign".into(),
                glyph: "dollar-sign".into(),
            });
        }
        categories.extend(lines.iter().map(|l| category_choice(l)));
    }
    categories.extend(open.iter().filter(|a| Some(a.id) != d.account).map(|a| SheetChoice {
        key: format!("t{}", a.id.hex()),
        title: format!("Transfer: {}", a.name),
        glyph: "arrow-left-right".into(),
    }));
    let category = match (d.transfer_to, d.category) {
        (Some(a), _) => format!("t{}", a.hex()),
        (None, Some(c)) if c == money_core::budget::ready_to_assign() => READY.into(),
        (None, Some(c)) => format!("c{}", c.hex()),
        (None, None) => String::new(),
    };
    let problem = match &sent {
        None => Some("That isn't an amount.".to_string()),
        Some(d) => d.problem(),
    };
    let by = editing
        .map(|t| t.user_id.clone())
        .filter(|u| !u.is_empty())
        .map(|u| format!("Entered by {u}"));
    TxnSheet {
        adding: d.editing.is_none(),
        editing: d.editing.is_some(),
        way: if f.inflow { "inflow" } else { "outflow" }.into(),
        ways: vec![
            SheetChoice {
                key: "outflow".into(),
                title: "Outflow".into(),
                glyph: "arrow-up".into(),
            },
            SheetChoice {
                key: "inflow".into(),
                title: "Inflow".into(),
                glyph: "arrow-down".into(),
            },
        ],
        prompt: if f.inflow { "Coming in" } else { "Going out" }.into(),
        typed: f.figure.typed.clone(),
        cents: cents.unwrap_or(0),
        amount_problem: cents.is_none().then(|| "That isn't an amount.".into()),
        payee: d.payee.clone(),
        has_payee: d.transfer_to.is_none(),
        has_payees: !offered.is_empty(),
        payees: offered,
        bank: editing.and_then(|t| t.bank_said()).unwrap_or_default(),
        has_bank: editing.and_then(|t| t.bank_said()).is_some(),
        category,
        categories,
        category_note: match d.editing.is_some() && !f.transfer {
            true => "Made a transfer, it pairs with the matching transaction in that account, or one is added there.".into(),
            false => String::new(),
        },
        account: d.account.map(|a| a.hex()).unwrap_or_default(),
        accounts: open
            .iter()
            .map(|a| SheetChoice {
                key: a.id.hex(),
                title: a.name.clone(),
                glyph: crate::accounts::kind_glyph(&a.kind).into(),
            })
            .collect(),
        account_name: d
            .account
            .and_then(|id| accounts(data).iter().find(|a| a.id == id))
            .map(|a| a.name.clone())
            .unwrap_or_default(),
        day: d.date.clone(),
        cleared: d.cleared,
        memo: d.memo.clone(),
        note: f.refused.clone().or(problem.clone()).or(by).unwrap_or_default(),
        can_save: problem.is_none(),
        deleting: f.deleting && editing.is_some(),
        delete_message: editing
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
