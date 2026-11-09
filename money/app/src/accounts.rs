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
use ark_app::{App, Command, Data, ModuleCall};
use ark_format::NumberStyle;
use ark_modules::Haptic;
use money_domain::accounts::{net_worth, shelves, AccountKind, Shelving};
use money_domain::dates::day_of_ms;
use money_domain::routes::Route;
use money_domain::*;

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
    /// The balance as typed.
    pub typed: String,
}

impl Default for Draft {
    fn default() -> Self {
        Draft {
            name: String::new(),
            kind: AccountKind::Checking,
            on_budget: AccountKind::Checking.on_budget_by_default(),
            typed: String::new(),
        }
    }
}

impl Draft {
    /// The balance typed, in cents — what is owed on a card or a loan is
    /// below nothing however it was typed; none when it is not an amount.
    pub fn balance(&self, style: &NumberStyle) -> Option<i64> {
        let cents = match self.typed.trim() {
            "" => Some(0),
            t => ark_format::parse_amount(t, style),
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
}

/// A shelf of accounts under its heading.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Shelf {
    pub title: String,
    pub total: i64,
    /// Whether the heading shows the total: the closed shelf's adds up to nothing that matters.
    pub shows_total: bool,
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
    pub typed: String,
    pub balance_label: String,
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
    /// The open accounts on the budget: All Accounts' figure.
    pub all_total: i64,
    pub shelves: Vec<Shelf>,
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
        "cash" => "wallet",
        "credit" => "credit-card",
        "loan" => "landmark",
        "investment" => "trending-up",
        "savings" => "piggy-bank",
        _ => "building-2",
    }
}

fn find<'a>(accounts: &'a [AccountLine], id: &str) -> Option<&'a AccountLine> {
    accounts.iter().find(|a| a.id.hex() == id)
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
        vec![MoneyView::Accounts, MoneyView::Banks]
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
                    out.push(Command::ask(ModuleCall::Haptic { haptic: Haptic::Selection }));
                }
                out
            }
            M::DraftKindKey { key } => match AccountKind::of(&key) {
                Some(kind) => vec![Command::Then(M::DraftKind { kind })],
                None => vec![],
            },
            M::DraftOnBudget { on_budget } => edit(s, |d| d.on_budget = on_budget),
            M::DraftBalance { typed } => edit(s, |d| d.typed = typed),
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
            M::CopyBalance { id } => match find(accounts, &id) {
                Some(a) => vec![Command::ask(ModuleCall::Copy {
                    text: ark_format::amount(a.balance, &s.style.0),
                })],
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
                    vec![Command::ask(ModuleCall::Haptic { haptic })]
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
            .map(|sh| Shelf {
                title: sh.title,
                total: sh.total,
                shows_total: sh.shelving != Shelving::Closed,
                accounts: sh.accounts.iter().map(|a| item(a, style)).collect(),
            })
            .collect();
        let synced = banks
            .first()
            .filter(|b| b.synced_ms > 0)
            .map(|b| format!("Last synced with {} {}.", b.institution, ark_format::ago(b.synced_ms, data.now_ms)));
        AccountsPage {
            net_worth: net_worth(accounts),
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
            true => "What's spent from it comes out of categories",
            false => "Tracked only: its balance counts toward net worth",
        }
        .into(),
        typed: d.typed.clone(),
        balance_label: if d.kind.owes() { "What You Owe on It" } else { "What's in It Today" }.into(),
        balance_problem: balance.is_none().then(|| "That isn't an amount".into()),
        can_save: !d.name.trim().is_empty() && name_problem.is_none() && balance.is_some(),
        name_problem,
    }
}

#[cfg(test)]
mod tests;
