//! Accounts: where the money is. One opened by hand starts with its balance
//! as a transaction of its own; one a bank feeds is opened by its first
//! import ([`crate::plaid`]).

use ark::prelude::*;

use crate::budget::{ensure_setup, Plan};
use crate::cards;
use crate::schema::*;
use crate::types::*;

/// The sorts of account there are, by the key an account keeps
/// ([`AccountKind::key`]).
pub const KINDS: [&str; 6] = ["checking", "savings", "cash", "credit", "loan", "investment"];

/// A sort of account: what a form offers, in this order, and what an
/// account's `kind` names by its key. It crosses the wire and the log as
/// its key (`"checking"`), the text it always was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, ark::twin::Twin)]
#[ark(ffi_eq)]
pub enum AccountKind {
    #[default]
    Checking,
    Savings,
    Cash,
    Credit,
    Loan,
    Investment,
}

impl AccountKind {
    /// Every kind, in the order a form offers them.
    pub const ALL: [AccountKind; 6] = [
        AccountKind::Checking,
        AccountKind::Savings,
        AccountKind::Cash,
        AccountKind::Credit,
        AccountKind::Loan,
        AccountKind::Investment,
    ];

    /// The key an account keeps: `checking`, `credit`, ….
    pub fn key(self) -> &'static str {
        KINDS[self as usize]
    }

    /// The kind a key names; none for one that is no kind.
    pub fn of(key: &str) -> Option<AccountKind> {
        KINDS.iter().position(|k| *k == key).map(|i| AccountKind::ALL[i])
    }

    /// What a person calls it: "Credit Card", "Loan or Mortgage".
    pub fn title(self) -> &'static str {
        match self {
            AccountKind::Checking => "Checking",
            AccountKind::Savings => "Savings",
            AccountKind::Cash => "Cash",
            AccountKind::Credit => "Credit Card",
            AccountKind::Loan => "Loan or Mortgage",
            AccountKind::Investment => "Investment",
        }
    }

    /// Whether an account of this kind belongs in the budget unless
    /// somebody says otherwise: what is spent from it is spent from the
    /// budget.
    pub fn on_budget_by_default(self) -> bool {
        on_budget_by_default(self.key())
    }

    /// Whether its balance is what is owed on it.
    pub fn owes(self) -> bool {
        matches!(self, AccountKind::Credit | AccountKind::Loan)
    }
}

/// What a person calls an account's kind by its key: [`AccountKind::title`],
/// or the key itself, capitalized, for one that is no kind.
pub fn kind_title(key: &str) -> String {
    match AccountKind::of(key) {
        Some(k) => k.title().into(),
        None => capitalized(key),
    }
}

/// `s` with its first letter a capital.
pub fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

impl serde::Serialize for AccountKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.key())
    }
}

impl<'de> serde::Deserialize<'de> for AccountKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let k = String::deserialize(d)?;
        AccountKind::of(&k).ok_or_else(|| serde::de::Error::custom("checking, savings, cash, credit, loan or investment"))
    }
}

impl ark::row::Describe for AccountKind {
    fn describe() -> ark::row::Type {
        ark::row::Type::Text
    }
}

/// Whether an account of this sort belongs in the budget unless somebody
/// says otherwise: what is spent from it is spent from the budget.
pub fn on_budget_by_default(kind: &str) -> bool {
    matches!(kind, "checking" | "savings" | "cash" | "credit")
}

/// The sidebar's sections, as every client lists the accounts: open ones
/// on the budget, open ones tracked beside it, and the closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Describe)]
#[ark(ffi)]
pub enum Shelving {
    Budget,
    Tracking,
    Closed,
}

impl Shelving {
    pub const ALL: [Shelving; 3] = [Shelving::Budget, Shelving::Tracking, Shelving::Closed];

    /// Its heading: "Budget", "Tracking", "Closed".
    pub fn title(self) -> &'static str {
        match self {
            Shelving::Budget => "Budget",
            Shelving::Tracking => "Tracking",
            Shelving::Closed => "Closed",
        }
    }

    /// Whether `a` is on it.
    pub fn holds(self, a: &AccountLine) -> bool {
        match self {
            Shelving::Budget => !a.closed && a.on_budget,
            Shelving::Tracking => !a.closed && !a.on_budget,
            Shelving::Closed => a.closed,
        }
    }
}

/// One section of the accounts, its accounts in order and what they add
/// up to.
#[derive(Clone, Debug, PartialEq, Eq, Describe)]
#[ark(ffi)]
pub struct AccountShelf {
    pub shelving: Shelving,
    pub title: String,
    pub accounts: Vec<AccountLine>,
    pub total: i64,
}

/// The accounts in their sections — on the budget, tracking, closed —
/// each in the sidebar's order (`pos`, then name); a section with
/// nothing in it left out.
pub fn shelves(accounts: &[AccountLine]) -> Vec<AccountShelf> {
    let mut ordered: Vec<&AccountLine> = accounts.iter().collect();
    ordered.sort_by(|a, b| (a.pos, &a.name).cmp(&(b.pos, &b.name)));
    Shelving::ALL
        .into_iter()
        .map(|s| {
            let accounts: Vec<AccountLine> = ordered.iter().filter(|a| s.holds(a)).map(|a| (*a).clone()).collect();
            AccountShelf {
                shelving: s,
                title: s.title().into(),
                total: accounts.iter().map(|a| a.balance).sum(),
                accounts,
            }
        })
        .filter(|s| !s.accounts.is_empty())
        .collect()
}

/// Everything the household has less everything it owes: the open
/// accounts' balances.
pub fn net_worth(accounts: &[AccountLine]) -> i64 {
    accounts.iter().filter(|a| !a.closed).map(|a| a.balance).sum()
}

/// The account a new transaction goes to when none is chosen: the first
/// open one on the budget, in the sidebar's order.
pub fn usual(accounts: &[AccountLine]) -> Option<Id<Account>> {
    shelves(accounts)
        .into_iter()
        .find(|s| s.shelving == Shelving::Budget)?
        .accounts
        .first()
        .map(|a| a.id)
}

/// The accounts a bank's account may feed: open, and fed by no bank yet;
/// those of `kind` (the kind it would open as) first, then in the
/// sidebar's order.
pub fn linkable(accounts: &[AccountLine], kind: &str) -> Vec<AccountLine> {
    let mut out: Vec<AccountLine> = accounts.iter().filter(|a| !a.closed && a.bank.is_empty()).cloned().collect();
    out.sort_by(|a, b| (a.kind != kind, a.pos, &a.name).cmp(&(b.kind != kind, b.pos, &b.name)));
    out
}

impl AccountLine {
    /// What its line says under its name: its bank and last digits when a
    /// bank feeds it (`Chase ••4321`), or its kind.
    pub fn subtitle(&self) -> String {
        match (self.bank.is_empty(), self.mask.is_empty()) {
            (true, _) => kind_title(&self.kind),
            (false, true) => capitalized(&self.bank),
            (false, false) => format!("{} \u{2022}\u{2022}{}", capitalized(&self.bank), self.mask),
        }
    }

    /// Its kind, when its key names one.
    pub fn account_kind(&self) -> Option<AccountKind> {
        AccountKind::of(&self.kind)
    }
}

/// What the first transaction of an account is called.
pub const STARTING_BALANCE: &str = "Starting Balance";

/// An account, after every other, and its first transaction: `balance` on
/// `date`. On budget, that money is ready to assign — but a card's debt,
/// which is its payment category's to cover ([`cards::starting_category`]).
#[allow(clippy::too_many_arguments)]
pub fn open(
    ctx: &Ctx,
    db: &mut Db<Money>,
    id: Id<Account>,
    name: &str,
    kind: &str,
    on_budget: bool,
    balance: i64,
    date: &str,
) -> Result<(), Refusal> {
    ensure_setup(ctx, db)?;
    let pos = db.account.by(Account::POS).whole().last().map_or(0, |a| a.pos) + 1;
    let account = Account {
        id,
        name: name.into(),
        kind: kind.into(),
        on_budget,
        closed: false,
        pos,
        note: String::new(),
        created_ms: ctx.now,
        user_id: ctx.user().into(),
    };
    db.account.insert(account.clone())?;
    cards::keep(ctx, db, &account)?;
    db.txn.insert(Txn {
        id: ctx.new_id(),
        account_id: id,
        date: date.into(),
        payee: STARTING_BALANCE.into(),
        category_id: cards::starting_category(&account, balance),
        amount: balance,
        cleared: Cleared::Cleared,
        approved: true,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Txn::default()
    })
}

#[mutator("accounts.open_account")]
pub fn open_account(ctx: &Ctx, db: &mut Db<Money>, input: OpenAccount) -> Result<(), Refusal> {
    open(
        ctx,
        db,
        ctx.new_id(),
        &input.name,
        input.kind.key(),
        input.on_budget,
        input.balance,
        &input.date,
    )
}

pub fn account_of(db: &Db<Money>, id: Id<Account>) -> Result<Account, Refusal> {
    db.account.get(id).ok_or_else(|| Refusal::new("no such account"))
}

/// An account named again — a card's payment category with it.
#[mutator("accounts.rename_account")]
pub fn rename_account(ctx: &Ctx, db: &mut Db<Money>, input: RenameAccount) -> Result<(), Refusal> {
    let a = Account {
        name: input.name,
        ..account_of(db, input.id)?
    };
    db.account.update(a.clone())?;
    cards::keep(ctx, db, &a)
}

/// An account closed, or open again. One with money in it stays open: the
/// money has to go somewhere first. A closed card's payment category is
/// hidden, and back when it opens again.
#[mutator("accounts.close_account")]
pub fn close_account(ctx: &Ctx, db: &mut Db<Money>, input: CloseAccount) -> Result<(), Refusal> {
    let a = account_of(db, input.id)?;
    if input.closed && balances(db, a.id).0 != 0 {
        return Err(Refusal::new("an account is closed with nothing in it"));
    }
    let a = Account { closed: input.closed, ..a };
    db.account.update(a.clone())?;
    cards::keep(ctx, db, &a)
}

/// An account's balance, what of it has cleared, and how many of its
/// transactions are waiting to be looked at.
pub fn balances(db: &Db<Money>, id: Id<Account>) -> (i64, i64, i64) {
    let (mut all, mut cleared, mut unapproved) = (0, 0, 0);
    for t in db.txn.by(Txn::ACCOUNT_ID_DATE).from(id).all() {
        all += t.amount;
        if t.cleared != Cleared::Uncleared {
            cleared += t.amount;
        }
        unapproved += (!t.approved) as i64;
    }
    (all, cleared, unapproved)
}

/// Every account, in order, with its balances and the bank that feeds it.
#[view("accounts.accounts")]
pub fn accounts(_: &Ctx, _: &Db<Money>, _: ()) -> Result<Plan<AccountLine>, Refusal> {
    Ok(scan::<Money, Account>()
        .map_with(|db, a| {
            let (balance, cleared, unapproved) = balances(db, a.id);
            let linked = db.linked.by(Linked::ACCOUNT_ID).from(Some(a.id)).first();
            AccountLine {
                id: a.id,
                name: a.name.clone(),
                kind: a.kind.clone(),
                on_budget: a.on_budget,
                closed: a.closed,
                pos: a.pos,
                balance,
                cleared,
                uncleared: balance - cleared,
                bank: linked.as_ref().map(|l| l.bank.clone()).unwrap_or_default(),
                mask: linked.as_ref().map(|l| l.mask.clone()).unwrap_or_default(),
                bank_balance: linked.and_then(|l| l.balance),
                unapproved,
            }
        })
        .sort(|a| (a.pos, a.id)))
}
