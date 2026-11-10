//! The Accounts tab, as one Ark app: what the household is worth, All
//! Accounts, the accounts on their shelves (on budget, tracking, closed),
//! Bank Connections and Add Account.
//!
//! The rows are the store's (`MoneyView::Accounts`, `MoneyView::Banks`);
//! [`State`] keeps only what the screen is in the middle of — a new
//! account's form, a rename. [`update`] decides everything a button does;
//! [`screen`] works out every figure and word the screen shows
//! ([`AccountsPage`]); [`view::accounts`] says how it is laid out in
//! ArkUI's parts, once, for every toolkit.

use ark::row::Id;
use ark_app::{App, Command, Data};
use ark_format::NumberStyle;
use ark_haptics::{perform, Haptic};
use money_core::accounts::{net_worth, shelves, AccountKind, Shelving, STARTING_BALANCE};
use money_core::dates::day_of_ms;
use money_core::routes::Route;
use money_core::*;

pub mod view;

/// The screen, as `ark_app` runs it.
pub struct Accounts;

/// What the screen is in the middle of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The new account being filled in, while Add Account's sheet is up.
    pub adding: Option<Draft>,
    /// The account being renamed and the name typed, while the alert is up.
    pub renaming: Option<(String, String)>,
    /// How amounts are typed and written here.
    pub style: Style,
}

/// How amounts are typed: the person's locale's marks.
#[derive(Clone, Debug, PartialEq)]
pub struct Style(pub NumberStyle);

impl Default for Style {
    fn default() -> Self {
        Style(NumberStyle::us())
    }
}

/// A new account as its form has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub name: String,
    pub kind: AccountKind,
    pub on_budget: bool,
    /// The balance as typed (a text field: Android, the desktop).
    pub typed: String,
    /// The balance as entered in cents, when it was (the iPhone's amount
    /// entry); what was typed counts otherwise.
    pub entered: Option<i64>,
}

impl Default for Draft {
    fn default() -> Self {
        Draft {
            name: String::new(),
            kind: AccountKind::Checking,
            on_budget: AccountKind::Checking.on_budget_by_default(),
            typed: String::new(),
            entered: None,
        }
    }
}

impl Draft {
    /// The balance typed, in cents — what is owed on a card or a loan is
    /// below nothing however it was typed; none when it is not an amount.
    pub fn balance(&self, style: &NumberStyle) -> Option<i64> {
        let cents = match (self.entered, self.typed.trim()) {
            (Some(c), _) => Some(c),
            (None, "") => Some(0),
            (None, t) => ark_format::parse_amount(t, style),
        }?;
        Some(if self.kind.owes() { -cents.abs() } else { cents })
    }

    /// The change it is, opened on `day`.
    pub fn change(&self, style: &NumberStyle, day: &str) -> MoneyChange {
        MoneyChange::OpenAccount(OpenAccount {
            name: self.name.trim().into(),
            kind: self.kind,
            on_budget: self.on_budget,
            balance: self.balance(style).unwrap_or(0),
            date: day.into(),
        })
    }
}

/// Everything the Accounts screen can be told. Ids are rows' `Id::hex`,
/// as routes carry them.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
pub enum AccountsMsg {
    /// Every account's register.
    OpenAll,
    /// One account's register.
    OpenAccount {
        id: String,
    },
    /// The banks Plaid feeds.
    OpenBanks,
    /// Ask the bank for anything new (the list pulled down).
    Refresh,
    /// Add Account's sheet raised.
    Add,
    DraftName {
        name: String,
    },
    DraftKind {
        kind: AccountKind,
    },
    DraftOnBudget {
        on_budget: bool,
    },
    DraftBalance {
        typed: String,
    },
    /// The balance entered in cents (the iPhone's amount entry).
    DraftCents {
        cents: i64,
    },
    /// A kind chosen by its key, as the form's chips send it.
    DraftKindKey {
        key: String,
    },
    /// The new account kept, if the form has no problem.
    SaveDraft,
    /// The sheet put away.
    DismissDraft,
    /// The rename alert raised for an account.
    Rename {
        id: String,
    },
    RenameTyped {
        name: String,
    },
    SaveRename,
    CancelRename,
    /// An account closed, or (false) reopened.
    SetClosed {
        id: String,
        closed: bool,
    },
    /// An open account closed, a closed one reopened: its menu's action.
    ToggleClosed {
        id: String,
    },
    /// An account's balance put on the clipboard.
    CopyBalance {
        id: String,
    },
    /// An account agreed with its bank: the shell's own sheet for it
    /// (`Command::Native("reconcile <id>")`) until reconciling moves here.
    Reconcile {
        id: String,
    },
    /// What became of the last change sent: none when it was taken.
    Sent {
        closed: Option<bool>,
        refused: Option<String>,
    },
}

/// One account's row.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct AccountItem {
    pub id: String,
    pub name: String,
    /// Its bank and last digits, or its kind.
    pub subtitle: String,
    pub kind: AccountKind,
    /// Its kind's key (`checking`): what the phones' marks are drawn by.
    pub kind_key: String,
    /// Its kind's mark, by ArkUI's glyph name.
    pub glyph: String,
    pub balance: i64,
    /// Transactions waiting for a look, while there are any.
    pub reviews: Option<i64>,
    pub closed: bool,
    /// Whether its menu's action closes it (not reopens it): destructive.
    pub is_open: bool,
    /// What its row says when read aloud.
    pub said: String,
    /// What its close action is called: "Close Account" or "Reopen".
    pub close_title: String,
    /// Its close action's glyph: closing's bin, reopening's arrow back.
    pub close_glyph: String,
}

/// A shelf of accounts under its heading.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct AccountsShelf {
    pub title: String,
    pub total: i64,
    /// Whether the heading shows the total: the closed shelf's adds up to nothing that matters.
    pub shows_total: bool,
    /// What the shelf is, said under it.
    pub note: Option<String>,
    pub accounts: Vec<AccountItem>,
}

/// One kind the form offers.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct KindChoice {
    pub kind: AccountKind,
    /// Its key (`credit`): what choosing it sends.
    pub key: String,
    pub title: String,
    pub glyph: String,
    pub selected: bool,
}

/// Add Account's form, as it stands.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct AddForm {
    pub name: String,
    /// The domain's sentence for what is wrong with the name, once one is typed.
    pub name_problem: Option<String>,
    pub kinds: Vec<KindChoice>,
    pub on_budget: bool,
    pub on_budget_note: String,
    /// The chosen kind's key: the picker's selection.
    pub kind_key: String,
    pub typed: String,
    /// The balance in cents, as the amount entry shows it (none typed is nothing).
    pub cents: i64,
    /// What a text field for the balance is labelled.
    pub balance_label: String,
    /// The heading over the balance in a form: "Balance", or "Balance Owed".
    pub balance_header: String,
    /// What the amount entry says under its figure.
    pub balance_prompt: String,
    pub balance_problem: Option<String>,
    pub can_save: bool,
}

/// The rename alert, as it stands.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct RenameForm {
    pub id: String,
    pub name: String,
    pub can_save: bool,
}

/// Everything the Accounts screen shows.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct AccountsPage {
    /// Every open account, less what's owed.
    pub net_worth: i64,
    /// What the open accounts hold, and what they owe (below nothing).
    pub assets: i64,
    pub liabilities: i64,
    /// Net worth at the end of each of the last weeks, oldest first, now last.
    pub trend: Vec<i64>,
    /// The open accounts on the budget: All Accounts' figure.
    pub all_total: i64,
    pub shelves: Vec<AccountsShelf>,
    /// "Bank Connections", or "Connect a Bank" while none is.
    pub banks_title: String,
    /// When the bank was last asked, said: "Last synced with Chase 5 min ago."
    pub synced: Option<String>,
    pub adding: Option<AddForm>,
    pub renaming: Option<RenameForm>,
}

/// The glyph a kind of account is marked with (ArkUI's names, Lucide's).
pub fn kind_glyph(kind: &str) -> &'static str {
    match kind {
        "cash" => "dollar-sign",
        "credit" => "credit-card",
        "loan" => "house",
        "investment" => "trending-up",
        "savings" => "banknote",
        _ => "landmark",
    }
}

/// How many weeks the net worth's trend looks back.
const TREND_WEEKS: i64 = 12;

/// Net worth at the end of each of the last [`TREND_WEEKS`] weeks, oldest
/// first, today last: today's, less what every open account's
/// transactions since each of those days did (a transfer between two of
/// them did nothing to it). An account's starting balance is not a change
/// in what the household is worth, only in what this app knows of it, so
/// it is not taken back.
fn trend(accounts: &[AccountLine], txns: &[TxnLine], now_ms: i64) -> Vec<i64> {
    let now = net_worth(accounts);
    let open: Vec<_> = accounts.iter().filter(|a| !a.closed).map(|a| a.id).collect();
    let today = now_ms.div_euclid(86_400_000);
    (0..=TREND_WEEKS)
        .rev()
        .map(|weeks| {
            let day = today - weeks * 7;
            let since: i64 = txns
                .iter()
                .filter(|t| t.payee != STARTING_BALANCE && open.contains(&t.account_id))
                .filter(|t| day_number(&t.date).is_some_and(|d| d > day && d <= today))
                .map(|t| t.amount)
                .sum();
            now - since
        })
        .collect()
}

/// A day (`2026-11-08`) as days since 1970.
fn day_number(day: &str) -> Option<i64> {
    let mut parts = day.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn find<'a>(accounts: &'a [AccountLine], id: &str) -> Option<&'a AccountLine> {
    accounts.iter().find(|a| a.id.hex() == id)
}

/// An account's id from its hex, as a message carries it.
pub fn account_id(hex: &str) -> Option<Id<Account>> {
    id_of(hex)
}

fn id_of(hex: &str) -> Option<Id<Account>> {
    let b: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect::<Option<_>>()?;
    Some(Id::new(b.try_into().ok()?))
}

impl App for Accounts {
    type Key = MoneyView;
    type State = State;
    type Msg = AccountsMsg;
    type Change = MoneyChange;
    type Route = Route;
    type Screen = AccountsPage;

    fn holds(_: &State) -> Vec<MoneyView> {
        vec![MoneyView::Accounts, MoneyView::Banks, MoneyView::Register { account: None }]
    }

    fn update(s: &mut State, msg: AccountsMsg, data: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        use AccountsMsg as M;
        let accounts: &[AccountLine] = data.rows(&MoneyView::Accounts);
        match msg {
            M::OpenAll => vec![Command::Open(Route::Accounts)],
            M::OpenAccount { id } => match find(accounts, &id) {
                Some(a) => vec![Command::Open(Route::Account { id, name: a.name.clone() })],
                // Gone since it was drawn: every account's, as a route to a gone row is.
                None => vec![Command::Open(Route::Accounts)],
            },
            M::OpenBanks => vec![Command::Open(Route::Banks)],
            M::Refresh => vec![Command::Native("refresh".into())],
            M::Add => {
                s.adding = Some(Draft::default());
                vec![]
            }
            M::DraftName { name } => edit(s, |d| d.name = name),
            M::DraftKind { kind } => {
                let stepped = s.adding.as_ref().is_some_and(|d| d.kind != kind);
                let mut out = edit(s, |d| {
                    d.kind = kind;
                    d.on_budget = kind.on_budget_by_default();
                });
                if stepped {
                    out.push(Command::ask(perform(Haptic::Selection)));
                }
                out
            }
            M::DraftKindKey { key } => match AccountKind::of(&key) {
                Some(kind) => vec![Command::Then(M::DraftKind { kind })],
                None => vec![],
            },
            M::DraftOnBudget { on_budget } => edit(s, |d| d.on_budget = on_budget),
            M::DraftBalance { typed } => edit(s, |d| {
                d.typed = typed;
                d.entered = None;
            }),
            M::DraftCents { cents } => edit(s, |d| d.entered = Some(cents.abs())),
            M::SaveDraft => {
                let Some(d) = &s.adding else { return vec![] };
                let change = d.change(&s.style.0, &day_of_ms(data.now_ms));
                if d.balance(&s.style.0).is_none() || change.problem().is_some() {
                    return vec![];
                }
                vec![Command::Send {
                    change,
                    then: Some(|r| M::Sent {
                        closed: None,
                        refused: r.err(),
                    }),
                }]
            }
            M::DismissDraft => {
                s.adding = None;
                vec![]
            }
            M::Rename { id } => {
                s.renaming = find(accounts, &id).map(|a| (id, a.name.clone()));
                vec![]
            }
            M::RenameTyped { name } => {
                if let Some(r) = &mut s.renaming {
                    r.1 = name
                }
                vec![]
            }
            M::SaveRename => {
                let Some((id, name)) = s.renaming.take() else {
                    return vec![];
                };
                let Some(id) = id_of(&id) else { return vec![] };
                let change = MoneyChange::RenameAccount(RenameAccount {
                    id,
                    name: name.trim().into(),
                });
                match change.problem() {
                    Some(_) => vec![],
                    None => vec![Command::send(change)],
                }
            }
            M::CancelRename => {
                s.renaming = None;
                vec![]
            }
            M::SetClosed { id, closed } => match id_of(&id) {
                Some(id) => vec![Command::Send {
                    change: MoneyChange::CloseAccount(CloseAccount { id, closed }),
                    then: Some(if closed { closed_sent } else { reopened_sent }),
                }],
                None => vec![],
            },
            M::ToggleClosed { id } => match find(accounts, &id) {
                Some(a) => vec![Command::Then(M::SetClosed { closed: !a.closed, id })],
                None => vec![],
            },
            M::Reconcile { id } => match find(accounts, &id) {
                Some(a) if !a.closed => vec![Command::Native(format!("reconcile {id}"))],
                _ => vec![],
            },
            M::CopyBalance { id } => match find(accounts, &id) {
                Some(a) => vec![Command::ask(ark_clipboard::copy(ark_format::amount(a.balance, &s.style.0)))],
                None => vec![],
            },
            M::Sent { closed, refused } => match refused {
                // The refusal is the store's to say (`Store::refusal`); the form stays up.
                Some(_) => vec![],
                None => {
                    let haptic = match closed {
                        Some(true) => Haptic::Warning,
                        Some(false) => Haptic::Success,
                        None => {
                            s.adding = None;
                            Haptic::Success
                        }
                    };
                    vec![Command::ask(perform(haptic))]
                }
            },
        }
    }

    fn screen(s: &State, data: &Data<'_, MoneyView>) -> AccountsPage {
        let accounts: &[AccountLine] = data.rows(&MoneyView::Accounts);
        let banks: &[BankLine] = data.rows(&MoneyView::Banks);
        let style = &s.style.0;
        let shelves = shelves(accounts)
            .into_iter()
            .map(|sh| AccountsShelf {
                title: format!("{} Accounts", sh.title),
                note: match sh.shelving {
                    Shelving::Budget => Some("What these hold is budgeted: ready to assign, then spent from categories.".into()),
                    Shelving::Tracking => Some("Counted toward net worth; none of it is budgeted.".into()),
                    Shelving::Closed => None,
                },
                total: sh.total,
                shows_total: sh.shelving != Shelving::Closed,
                accounts: sh.accounts.iter().map(|a| item(a, style)).collect(),
            })
            .collect();
        let synced = banks
            .first()
            .filter(|b| b.synced_ms > 0)
            .map(|b| format!("Last synced with {} {}.", b.institution, ark_format::ago(b.synced_ms, data.now_ms)));
        let open = || accounts.iter().filter(|a| !a.closed);
        let txns: &[TxnLine] = data.rows(&MoneyView::Register { account: None });
        AccountsPage {
            net_worth: net_worth(accounts),
            assets: open().map(|a| a.balance).filter(|b| *b > 0).sum(),
            liabilities: open().map(|a| a.balance).filter(|b| *b < 0).sum(),
            trend: trend(accounts, txns, data.now_ms),
            all_total: accounts.iter().filter(|a| !a.closed && a.on_budget).map(|a| a.balance).sum(),
            shelves,
            banks_title: if banks.is_empty() { "Connect a Bank" } else { "Bank Connections" }.into(),
            synced,
            adding: s.adding.as_ref().map(|d| form(d, style)),
            renaming: s.renaming.as_ref().map(|(id, name)| RenameForm {
                id: id.clone(),
                name: name.clone(),
                can_save: !name.trim().is_empty(),
            }),
        }
    }
}

fn closed_sent(r: Result<(), String>) -> AccountsMsg {
    AccountsMsg::Sent {
        closed: Some(true),
        refused: r.err(),
    }
}

fn reopened_sent(r: Result<(), String>) -> AccountsMsg {
    AccountsMsg::Sent {
        closed: Some(false),
        refused: r.err(),
    }
}

/// The draft changed by `f`, while there is one.
fn edit(s: &mut State, f: impl FnOnce(&mut Draft)) -> Vec<Command<Accounts>> {
    if let Some(d) = &mut s.adding {
        f(d)
    }
    vec![]
}

fn item(a: &AccountLine, style: &NumberStyle) -> AccountItem {
    let mut said = format!("{}, {}, {}", a.name, a.subtitle(), ark_format::amount(a.balance, style));
    if a.unapproved > 0 {
        said += &format!(", {} to review", a.unapproved);
    }
    AccountItem {
        id: a.id.hex(),
        name: a.name.clone(),
        subtitle: a.subtitle(),
        kind: a.account_kind().unwrap_or_default(),
        kind_key: a.kind.clone(),
        glyph: kind_glyph(&a.kind).into(),
        balance: a.balance,
        reviews: (a.unapproved > 0).then_some(a.unapproved),
        closed: a.closed,
        is_open: !a.closed,
        said,
        close_title: if a.closed { "Reopen" } else { "Close Account" }.into(),
        close_glyph: if a.closed { "undo-2" } else { "archive" }.into(),
    }
}

fn form(d: &Draft, style: &NumberStyle) -> AddForm {
    let balance = d.balance(style);
    // The domain's own sentence for the name, asked as it is typed (the day is any day).
    let name_problem = match d.name.is_empty() {
        true => None,
        false => d.change(style, "2000-01-01").problem(),
    };
    AddForm {
        name: d.name.clone(),
        kinds: AccountKind::ALL
            .iter()
            .map(|k| KindChoice {
                kind: *k,
                key: k.key().into(),
                title: k.title().into(),
                glyph: kind_glyph(k.key()).into(),
                selected: *k == d.kind,
            })
            .collect(),
        on_budget: d.on_budget,
        on_budget_note: match d.on_budget {
            true => "What's spent from it comes out of the budget, and its balance is ready to assign.",
            false => "Tracking only: its balance counts toward net worth, and none of it is budgeted.",
        }
        .into(),
        kind_key: d.kind.key().into(),
        typed: d.typed.clone(),
        cents: balance.map_or(0, i64::abs),
        balance_label: if d.kind.owes() { "What You Owe on It" } else { "What's in It Today" }.into(),
        balance_header: if d.kind.owes() { "Balance Owed" } else { "Balance" }.into(),
        balance_prompt: if d.kind.owes() { "Owed on it today" } else { "In it today" }.into(),
        balance_problem: balance.is_none().then(|| "That isn't an amount".into()),
        can_save: !d.name.trim().is_empty() && name_problem.is_none() && balance.is_some(),
        name_problem,
    }
}

#[cfg(test)]
mod tests;
