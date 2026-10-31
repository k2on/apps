//! Banks, through Plaid: what ArkOS's `money-sync` did for Actual Budget,
//! with the budget now the domain's own.
//!
//! Two halves, as there were:
//!
//! - **What Plaid says, as an import** ([`Pulled`], [`import_of`]): Plaid's
//!   `/accounts/get` and `/transactions/sync` answers, read as they come
//!   (dollars as floats, money leaving positive) and made one [`Import`] in
//!   cents and our sign — `sync.rs`'s `entity` and `bank_balance`. A server's
//!   importer holds the access tokens (secrets, never in the log), pulls from
//!   each bank's cursor and authors one import per bank; it holds the
//!   [`crate::BANK`] role, which nobody else does.
//! - **The import itself** ([`import`]): `actual-import`'s `runImport`, as a
//!   mutator — so it runs, on every peer, against the budget as it is when
//!   the entry lands. Plaid's transaction id travels as `imported_id`, which
//!   is what makes a run idempotent. A pending transaction that posts arrives
//!   as a new one naming the pending one (and a removal of it); it is moved
//!   to the posted id in place, so a category somebody gave it while it was
//!   pending survives. One somebody typed in by hand first is matched rather
//!   than doubled. A reconciled one is never touched. The first import that
//!   sees an account opens it here, under the bank's name for it or the
//!   one the server was told, with the balance before the imported history
//!   as its starting balance; one the server was told not to follow is
//!   known and opens nothing. Money leaving one account on the budget and
//!   arriving in another — a card's payment, told of by both banks — is
//!   joined into one transfer as its second half comes
//!   ([`crate::transfers::join_imported`]).
//! - **Two logins to one bank** (`fold`): a bank connected from the app
//!   for an institution already here folds, at its first import, into the
//!   login already here — each of its accounts takes its twin's choice,
//!   the ones with no twin are opened, and the login it replaces is
//!   retired. Where two connections feed one account (that fold, or a
//!   waiting account linked by hand to an account another login feeds),
//!   a transaction one imports that the other already did is matched to
//!   the other's copy (`seam`) instead of doubled.
//!
//! And **an ask** ([`sync`]): anybody in the household may ask the importer
//! to ask the banks now, as a row it sees (`money/server`'s `importer.rs`).

use std::collections::BTreeSet;

use ark::prelude::*;
use serde::Deserialize as De;

use crate::accounts::STARTING_BALANCE;
use crate::budget::ensure_setup;
use crate::dates::{add_days, day_of_ms, days_between};
use crate::schema::*;
use crate::transactions::remembered;
use crate::types::*;

// * What Plaid says

/// One of an item's accounts, as `/accounts/get` has it.
#[derive(De, Clone, Debug, PartialEq)]
pub struct PlaidAccount {
    pub account_id: String,
    pub name: String,
    pub mask: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub subtype: Option<String>,
    #[serde(default)]
    pub balances: Balances,
}

#[derive(De, Clone, Debug, Default, PartialEq)]
pub struct Balances {
    pub current: Option<f64>,
}

/// A transaction as `/transactions/sync` has it: `amount` in dollars,
/// positive for money leaving the account.
#[derive(De, Clone, Debug, PartialEq)]
pub struct PlaidTransaction {
    pub transaction_id: String,
    pub account_id: String,
    pub amount: f64,
    pub date: String,
    pub name: String,
    #[serde(default)]
    pub merchant_name: Option<String>,
    #[serde(default)]
    pub original_description: Option<String>,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub pending_transaction_id: Option<String>,
}

#[derive(De, Clone, Debug, PartialEq)]
pub struct PlaidRemoved {
    pub transaction_id: String,
    pub account_id: String,
}

/// Every page of one `/transactions/sync` run from a cursor, and the cursor
/// after it.
#[derive(De, Clone, Debug, Default, PartialEq)]
pub struct Pulled {
    pub added: Vec<PlaidTransaction>,
    pub modified: Vec<PlaidTransaction>,
    pub removed: Vec<PlaidRemoved>,
    #[serde(alias = "next_cursor")]
    pub cursor: String,
}

/// Dollars as cents.
pub fn cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

/// The bank's balance as the budget counts it: what you owe on a card or a
/// loan is negative.
pub fn bank_balance(account: &PlaidAccount, current: f64) -> i64 {
    let owed = account.kind == "credit" || account.kind == "loan";
    cents(if owed { -current } else { current })
}

/// A Plaid transaction in our sign, with the merchant as its payee where
/// Plaid knows one and the bank's own line kept beside it.
pub fn bank_txn(t: &PlaidTransaction) -> BankTxn {
    let non_empty = |s: &Option<String>| s.clone().filter(|s| !s.trim().is_empty());
    BankTxn {
        transaction_id: t.transaction_id.clone(),
        plaid_account_id: t.account_id.clone(),
        pending_transaction_id: t.pending_transaction_id.clone(),
        date: t.date.clone(),
        amount: cents(-t.amount),
        payee: non_empty(&t.merchant_name).unwrap_or_else(|| t.name.clone()),
        imported_payee: non_empty(&t.original_description).unwrap_or_else(|| t.name.clone()),
        pending: t.pending,
    }
}

/// One bank's run as the import its importer authors.
pub fn import_of(bank: &str, institution: &str, accounts: &[PlaidAccount], pulled: &Pulled) -> Import {
    Import {
        bank: bank.into(),
        institution: institution.into(),
        cursor: pulled.cursor.clone(),
        accounts: accounts
            .iter()
            .map(|a| BankAccount {
                plaid_account_id: a.account_id.clone(),
                name: a.name.clone(),
                mask: a.mask.clone().unwrap_or_default(),
                kind: a.kind.clone(),
                subtype: a.subtype.clone().unwrap_or_default(),
                balance: a.balances.current.map(|c| bank_balance(a, c)),
                called: String::new(),
                ignored: false,
            })
            .collect(),
        upserts: pulled.added.iter().chain(&pulled.modified).map(bank_txn).collect(),
        removed: pulled
            .removed
            .iter()
            .map(|r| BankRemoved {
                transaction_id: r.transaction_id.clone(),
                plaid_account_id: r.account_id.clone(),
            })
            .collect(),
        choose: false,
        from_start: false,
        institution_id: String::new(),
    }
}

/// What sort of account a Plaid account is here.
pub fn kind_of(kind: &str, subtype: &str) -> &'static str {
    match (kind, subtype) {
        ("depository", "savings" | "money market" | "cd" | "hsa") => "savings",
        ("depository", _) => "checking",
        ("credit", _) => "credit",
        ("loan", _) => "loan",
        _ => "investment",
    }
}

// * The import

/// How far apart a typed-in transaction and the bank's may be dated and
/// still be one.
pub const MATCH_DAYS: i64 = 3;

/// What one import did, account by account.
#[derive(Default)]
struct Counts {
    added: i64,
    updated: i64,
    deleted: i64,
    locked: i64,
}

fn by_imported(db: &Db<Money>, id: &str) -> Option<Txn> {
    db.txn.by(Txn::IMPORTED_ID).from(Some(id.to_string())).first()
}

/// One bank's run of `/transactions/sync`, whole: its accounts opened or
/// brought up to date, its transactions applied, its cursor kept, and
/// whatever its last failure said cleared.
#[mutator("banks.import")]
pub fn import(ctx: &Ctx, db: &mut Db<Money>, input: Import) -> Result<(), Refusal> {
    if !ctx.has_role(crate::BANK) {
        return Err(Refusal::new("only the bank importer imports"));
    }
    ensure_setup(ctx, db)?;
    let previous = db.bank.get(&input.bank).unwrap_or_default();
    // A login another has replaced is on its way out: what it says, the
    // one that replaced it says now.
    if !previous.replaced_by.is_empty() {
        return Ok(());
    }
    db.bank.put(Bank {
        name: input.bank.clone(),
        institution: input.institution.clone(),
        institution_id: match input.institution_id.trim() {
            "" => previous.institution_id.clone(),
            i => i.to_string(),
        },
        cursor: input.cursor.clone(),
        synced_ms: ctx.now,
        user_id: ctx.user().into(),
        error_code: String::new(),
        error: String::new(),
        // A choice made while this was on its way waits for one that is.
        repull: previous.repull && !input.from_start,
        fresh: false,
        adding: false,
        ..previous.clone()
    })?;
    let mut new = vec![];
    for a in &input.accounts {
        if follow(ctx, db, &input, a)? {
            new.push(a.plaid_account_id.clone());
        }
    }
    if previous.fresh {
        fold(ctx, db, &input)?;
    }
    if previous.adding {
        // Picked in Plaid's account picker: opened, as `banks.open` would.
        for id in new {
            if let Some(l) = db.linked.get(&id).filter(waits) {
                adopt(ctx, db, &input, l)?;
            }
        }
    }

    let superseded: BTreeSet<&str> = input.upserts.iter().filter_map(|u| u.pending_transaction_id.as_deref()).collect();
    let mut counts = Counts::default();
    for u in &input.upserts {
        let Some(feed) = followed(db, &u.plaid_account_id) else {
            continue;
        };
        if u.date < feed.since {
            // Before the day it took over another connection's account: that
            // one's copy is there already. Only one still pending there is
            // this one's to post.
            if feed.seam {
                if !superseded.contains(u.transaction_id.as_str()) {
                    seam(db, &input.bank, &feed.account, u, true, &mut counts)?;
                }
            }
            continue;
        }
        upsert(ctx, db, &input.bank, &feed, u, &superseded, &mut counts)?;
    }
    for r in &input.removed {
        if superseded.contains(r.transaction_id.as_str()) || followed(db, &r.plaid_account_id).is_none() {
            continue;
        }
        let Some(prior) = by_imported(db, &r.transaction_id) else { continue };
        if prior.cleared == Cleared::Reconciled {
            counts.locked += 1;
            continue;
        }
        crate::transfers::unlink(db, &prior)?;
        db.txn.remove(&prior)?;
        counts.deleted += 1;
    }

    let bank = db.bank.get(&input.bank).expect("the bank just written");
    db.bank.update(Bank {
        added: counts.added,
        updated: counts.updated,
        deleted: counts.deleted,
        locked: counts.locked,
        ..bank
    })
}

/// Where a followed Plaid account's transactions go.
struct Feed {
    account: Account,
    /// The first day it takes transactions from.
    since: String,
    /// Another connection fed the account before it, or feeds it still:
    /// what that one imported is matched rather than doubled.
    seam: bool,
}

/// The account a Plaid account feeds, when it is followed.
fn followed(db: &Db<Money>, plaid_account_id: &str) -> Option<Feed> {
    let l = db.linked.get(plaid_account_id).filter(|l| !l.ignored)?;
    let account = db.account.get(l.account_id?)?;
    let seam = l.seam || shared(db, &l, account.id);
    Some(Feed {
        account,
        since: l.since,
        seam,
    })
}

/// Another connection's account feeds `account` too.
fn shared(db: &Db<Money>, l: &Linked, account: Id<Account>) -> bool {
    (db.linked.by(Linked::ACCOUNT_ID).from(Some(account)).all().into_iter()).any(|o| o.bank != l.bank && !o.ignored)
}

/// Neither feeding an account nor ignored: waiting for a choice.
fn waits(l: &Linked) -> bool {
    l.account_id.is_none() && !l.ignored
}

// * Two logins to one bank

/// Two banks are one institution: Plaid's id for it when both have one,
/// else its name.
fn same_bank(a: &Bank, b: &Bank) -> bool {
    match (a.institution_id.trim(), b.institution_id.trim()) {
        ("", _) | (_, "") => {
            let (x, y) = (a.institution.trim(), b.institution.trim());
            !x.is_empty() && x.eq_ignore_ascii_case(y)
        }
        (x, y) => x == y,
    }
}

/// Two logins' accounts are one account at the bank: of one type and
/// subtype, and the same last digits (or, with none, the same name).
fn same_account(a: &Linked, b: &Linked) -> bool {
    let named = match (a.mask.trim(), b.mask.trim()) {
        ("", "") => !a.name.trim().is_empty() && a.name.trim() == b.name.trim(),
        (x, y) => x == y,
    };
    named && a.kind == b.kind && a.subtype == b.subtype
}

/// A bank just connected from the app that is a second login to a bank
/// already here (`banks.connect` again for the same institution, as for an
/// account the first login did not share) folded into it, at its first
/// import. Each of its accounts whose twin at the other login has a choice
/// takes that choice: it feeds the same account, from the day it was first
/// seen (what came before is there already), or is ignored. Its accounts
/// with no twin were picked in Plaid's account picker for this
/// connection: each is opened as an account of its kind, its whole history
/// and a starting balance (as `banks.open`). A login every one of whose
/// choices the new one now carries is replaced by it: the importer removes
/// it at Plaid and the budget forgets it (`banks.retired`). Nothing is done
/// unless at least one account has a twin with a choice: another bank's
/// accounts are never touched, nor another institution's.
fn fold(ctx: &Ctx, db: &mut Db<Money>, input: &Import) -> Result<(), Refusal> {
    let Some(me) = db.bank.get(&input.bank) else { return Ok(()) };
    let others: Vec<Bank> = (db.bank.scan().into_iter())
        .filter(|b| b.name != me.name && b.replaced_by.is_empty() && same_bank(&me, b))
        .collect();
    if others.is_empty() {
        return Ok(());
    }
    let theirs: Vec<Linked> = (db.linked.scan().into_iter())
        .filter(|l| others.iter().any(|b| b.name == l.bank))
        .collect();
    let chose = |db: &Db<Money>, l: &Linked| l.ignored || l.account_id.is_some_and(|a| db.account.get(a).is_some());
    let mine: Vec<Linked> = (db.linked.scan().into_iter()).filter(|l| l.bank == me.name && waits(l)).collect();
    let mut carried: BTreeSet<String> = BTreeSet::new();
    let mut alone = vec![];
    for l in mine {
        let twin = (theirs.iter()).find(|t| !carried.contains(&t.plaid_account_id) && same_account(t, &l));
        let Some(t) = twin else {
            alone.push(l);
            continue;
        };
        if !chose(db, t) {
            // Its twin waits for a choice too: so does it.
            continue;
        }
        carried.insert(t.plaid_account_id.clone());
        match t.account_id.filter(|_| !t.ignored) {
            None => db.linked.update(Linked { ignored: true, ..l })?,
            Some(account) => take_over(ctx, db, input, l, account)?,
        }
    }
    if carried.is_empty() {
        return Ok(());
    }
    for l in alone {
        adopt(ctx, db, input, l)?;
    }
    for b in others {
        let chosen: Vec<&Linked> = theirs.iter().filter(|t| t.bank == b.name && chose(db, t)).collect();
        if !chosen.is_empty() && chosen.iter().all(|t| carried.contains(&t.plaid_account_id)) {
            db.bank.update(Bank {
                replaced_by: me.name.clone(),
                ..b
            })?;
        }
    }
    Ok(())
}

/// A waiting account made to feed `account`, which another connection's
/// account fed: from the day it was first seen, matched to what the other
/// imported. Its bank's history is asked again unless this import is it.
fn take_over(ctx: &Ctx, db: &mut Db<Money>, input: &Import, l: Linked, account: Id<Account>) -> Result<(), Refusal> {
    if !input.from_start {
        repull(ctx, db, &input.bank)?;
    }
    db.linked.update(Linked {
        account_id: Some(account),
        ignored: false,
        since: l.seen.clone(),
        seam: true,
        ..l
    })
}

/// A waiting account given an account of its own, as `banks.open` gives
/// one: its whole history, and a starting balance before it — at once
/// when this import is the bank's whole history, else at the next import,
/// asked from the start.
fn adopt(ctx: &Ctx, db: &mut Db<Money>, input: &Import, l: Linked) -> Result<(), Refusal> {
    let account = open_for(ctx, db, l.name.clone(), &l.kind, &l.subtype)?;
    let now = input.from_start;
    if now {
        if let Some(a) = input.accounts.iter().find(|a| a.plaid_account_id == l.plaid_account_id) {
            opening_balance(ctx, db, input, a, &account)?;
        }
    } else {
        repull(ctx, db, &input.bank)?;
    }
    db.linked.update(Linked {
        account_id: Some(account.id),
        ignored: false,
        since: String::new(),
        opening: !now,
        ..l
    })
}

/// A login replaced by another (`fold`), removed at Plaid and dropped by
/// the importer: forgotten here as a disconnected bank is. What it fed
/// stays, and the login that replaced it feeds it now.
#[mutator("banks.retired")]
pub fn retired(ctx: &Ctx, db: &mut Db<Money>, input: BankName) -> Result<(), Refusal> {
    if !ctx.has_role(crate::BANK) {
        return Err(Refusal::new("only the bank importer retires a login"));
    }
    match db.bank.get(&input.bank) {
        Some(b) if !b.replaced_by.is_empty() => crate::banks::forget(db, &input.bank),
        Some(_) => Err(Refusal::new("that bank has not been replaced")),
        None => Ok(()),
    }
}

/// A Plaid account seen: brought up to date, or — the first time — known.
/// What the first sight does is the import's to say: a server's importer
/// leaves it waiting for the household to choose (`choose`); otherwise it
/// opens here under what the import calls it (the bank's name unless told),
/// or is known and opens nothing when the import says not to follow it.
/// After that, which account a bank's feeds is the household's
/// (`banks.link`, `banks.open`, `banks.ignore`). Whether this was its
/// first sight.
fn follow(ctx: &Ctx, db: &mut Db<Money>, input: &Import, a: &BankAccount) -> Result<bool, Refusal> {
    if let Some(l) = db.linked.get(&a.plaid_account_id) {
        // An account opened for it is owed the balance before the history,
        // which only an import from the bank's start brings whole.
        let owed = l.opening && input.from_start && !l.ignored;
        let opened = match (owed, l.account_id.and_then(|id| db.account.get(id))) {
            (true, Some(account)) => {
                opening_balance(ctx, db, input, a, &account)?;
                true
            }
            _ => false,
        };
        db.linked.update(Linked {
            name: a.name.clone(),
            mask: a.mask.clone(),
            kind: a.kind.clone(),
            subtype: a.subtype.clone(),
            balance: a.balance,
            opening: l.opening && !opened,
            ..l
        })?;
        return Ok(false);
    }
    let known = Linked {
        plaid_account_id: a.plaid_account_id.clone(),
        bank: input.bank.clone(),
        account_id: None,
        name: a.name.clone(),
        mask: a.mask.clone(),
        kind: a.kind.clone(),
        subtype: a.subtype.clone(),
        balance: a.balance,
        ignored: false,
        seen: day_of_ms(ctx.now),
        since: String::new(),
        opening: false,
        seam: false,
    };
    if a.ignored {
        db.linked.insert(Linked { ignored: true, ..known })?;
        return Ok(true);
    }
    if input.choose && a.called.trim().is_empty() {
        db.linked.insert(known)?;
        return Ok(true);
    }
    let name = match a.called.trim() {
        "" => a.name.clone(),
        called => called.to_string(),
    };
    let account = open_for(ctx, db, name, &a.kind, &a.subtype)?;
    db.linked.insert(Linked {
        account_id: Some(account.id),
        ..known
    })?;
    opening_balance(ctx, db, input, a, &account)?;
    Ok(true)
}

/// An account opened here for one of a bank's: of the kind the bank's is
/// (a card is a credit account), on budget as that kind is by default.
pub(crate) fn open_for(ctx: &Ctx, db: &mut Db<Money>, name: String, kind: &str, subtype: &str) -> Result<Account, Refusal> {
    let kind = kind_of(kind, subtype);
    let account = Account {
        id: ctx.new_id(),
        name,
        kind: kind.into(),
        on_budget: crate::accounts::on_budget_by_default(kind),
        pos: db.account.by(Account::POS).whole().last().map_or(0, |a| a.pos) + 1,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Account::default()
    };
    db.account.insert(account.clone())?;
    crate::cards::keep(ctx, db, &account)?;
    Ok(account)
}

/// An account opened for a bank's given its starting balance: what the
/// bank holds less the history this import brings (posted only: the bank's
/// balance leaves pending out), the day before the oldest of it.
fn opening_balance(ctx: &Ctx, db: &mut Db<Money>, input: &Import, a: &BankAccount, account: &Account) -> Result<(), Refusal> {
    let Some(balance) = a.balance else { return Ok(()) };
    let mine = input.upserts.iter().filter(|u| u.plaid_account_id == a.plaid_account_id);
    let posted: i64 = mine.clone().filter(|u| !u.pending).map(|u| u.amount).sum();
    let opening = balance - posted;
    let date = match mine.map(|u| u.date.as_str()).min() {
        Some(oldest) => add_days(oldest, -1),
        None => day_of_ms(ctx.now),
    };
    if opening == 0 {
        return Ok(());
    }
    db.txn.insert(Txn {
        id: ctx.new_id(),
        account_id: account.id,
        date,
        payee: STARTING_BALANCE.into(),
        category_id: crate::cards::starting_category(account, opening),
        memo: "Balance before the first import".into(),
        amount: opening,
        cleared: Cleared::Cleared,
        approved: true,
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Txn::default()
    })
}

fn cleared(pending: bool) -> Cleared {
    match pending {
        true => Cleared::Uncleared,
        false => Cleared::Cleared,
    }
}

/// One transaction the bank added or changed.
fn upsert(
    ctx: &Ctx,
    db: &mut Db<Money>,
    bank: &str,
    feed: &Feed,
    u: &BankTxn,
    superseded: &BTreeSet<&str>,
    counts: &mut Counts,
) -> Result<(), Refusal> {
    let account = &feed.account;
    let prior = by_imported(db, &u.transaction_id).or_else(|| u.pending_transaction_id.as_deref().and_then(|p| by_imported(db, p)));
    if let Some(prior) = prior {
        if prior.cleared == Cleared::Reconciled {
            counts.locked += 1;
            return Ok(());
        }
        // Known: the bank's facts brought up to date, and what somebody
        // chose — payee, category, memo — left alone. A transfer's other
        // half follows — unless it is another bank's, which says its own
        // date; one that no longer agrees on the amount is let go.
        let mut prior = prior;
        if let Some(pair) = prior.transfer_id.and_then(|p| db.txn.get(p)) {
            match pair.imported_id {
                None => db.txn.update(Txn {
                    amount: -u.amount,
                    date: u.date.clone(),
                    ..pair
                })?,
                Some(_) if pair.amount == -u.amount => {}
                Some(_) => {
                    crate::transfers::unlink(db, &prior)?;
                    prior = Txn {
                        transfer_account: None,
                        transfer_id: None,
                        approved: false,
                        ..prior
                    };
                }
            }
        }
        db.txn.update(Txn {
            imported_id: Some(u.transaction_id.clone()),
            imported_payee: u.imported_payee.clone(),
            amount: u.amount,
            date: u.date.clone(),
            cleared: cleared(u.pending),
            imported_by: bank.into(),
            ..prior
        })?;
        counts.updated += 1;
        return Ok(());
    }
    // Never a pending transaction whose posted form is in the same run.
    if superseded.contains(u.transaction_id.as_str()) {
        return Ok(());
    }
    // Typed in by hand before the bank told of it: the same amount, in the
    // same account, within a few days, not yet the bank's.
    let lo = (account.id, add_days(&u.date, -MATCH_DAYS));
    let hi = (account.id, add_days(&u.date, MATCH_DAYS + 1));
    let typed = db
        .txn
        .by(Txn::ACCOUNT_ID_DATE)
        .between(lo, hi)
        .all()
        .into_iter()
        .filter(|t| t.imported_id.is_none() && t.amount == u.amount && t.cleared != Cleared::Reconciled)
        .min_by_key(|t| days_between(&t.date, &u.date).abs());
    if let Some(t) = typed {
        db.txn.update(Txn {
            imported_id: Some(u.transaction_id.clone()),
            imported_payee: u.imported_payee.clone(),
            cleared: cleared(u.pending),
            imported_by: bank.into(),
            ..t
        })?;
        counts.updated += 1;
        return Ok(());
    }
    // Imported by another connection that feeds, or fed, this account: a
    // second login to the same bank tells of what the first did.
    if feed.seam && seam(db, bank, account, u, false, counts)? {
        return Ok(());
    }
    let category_id = match account.on_budget {
        true => remembered(db, &u.payee),
        false => None,
    };
    let t = Txn {
        id: ctx.new_id(),
        account_id: account.id,
        date: u.date.clone(),
        payee: u.payee.clone(),
        category_id,
        amount: u.amount,
        cleared: cleared(u.pending),
        approved: false,
        imported_id: Some(u.transaction_id.clone()),
        imported_payee: u.imported_payee.clone(),
        created_ms: ctx.now,
        user_id: ctx.user().into(),
        imported_by: bank.into(),
        ..Txn::default()
    };
    db.txn.insert(t.clone())?;
    // The other half of a transfer — a card's payment, most often — that
    // another account's bank already told of: one transfer now.
    crate::transfers::join_imported(db, &t)?;
    counts.added += 1;
    Ok(())
}

/// The copy of `u` another connection imported into `account` — the same
/// amount, within [`MATCH_DAYS`], not yet matched by one of this bank's —
/// made this bank's: it takes this bank's id and the bank's facts, and
/// what somebody chose for it stays. Only one still pending there, when
/// `pending` (what is dated before this bank took the account over).
/// Whether there was one.
fn seam(db: &mut Db<Money>, bank: &str, account: &Account, u: &BankTxn, pending: bool, counts: &mut Counts) -> Result<bool, Refusal> {
    let lo = (account.id, add_days(&u.date, -MATCH_DAYS));
    let hi = (account.id, add_days(&u.date, MATCH_DAYS + 1));
    let theirs = (db.txn.by(Txn::ACCOUNT_ID_DATE).between(lo, hi).all().into_iter())
        .filter(|t| t.imported_id.is_some() && t.imported_by != bank && t.amount == u.amount)
        .filter(|t| t.cleared != Cleared::Reconciled && (!pending || t.cleared == Cleared::Uncleared))
        .min_by_key(|t| days_between(&t.date, &u.date).abs());
    let Some(t) = theirs else { return Ok(false) };
    db.txn.update(Txn {
        imported_id: Some(u.transaction_id.clone()),
        imported_by: bank.into(),
        imported_payee: u.imported_payee.clone(),
        date: u.date.clone(),
        cleared: cleared(u.pending),
        ..t
    })?;
    counts.updated += 1;
    Ok(true)
}

/// A bank's account made to feed `account_id`: every import after this
/// writes there. What the account it fed before already holds stays.
///
/// One that was waiting for a choice (a bank just connected) takes what
/// the bank has had since the day it was first seen: the next import asks
/// for the bank's history again (`repull`), and the account, kept by hand
/// until now, has what came before.
///
/// The account may be one another connection feeds, or fed: a second
/// login to the same bank. What this one imports into it is then matched
/// to what the other imported, rather than doubled (`seam`).
#[mutator("banks.link")]
pub fn link(ctx: &Ctx, db: &mut Db<Money>, input: LinkAccount) -> Result<(), Refusal> {
    let l = db
        .linked
        .get(&input.plaid_account_id)
        .ok_or_else(|| Refusal::new("no such bank account"))?;
    crate::accounts::account_of(db, input.account_id)?;
    let fed = db.linked.by(Linked::ACCOUNT_ID).from(Some(input.account_id)).all();
    if fed.iter().any(|o| o.bank == l.bank && o.plaid_account_id != l.plaid_account_id && !o.ignored) {
        return Err(Refusal::new("that account is fed by another of this bank's accounts already"));
    }
    let others = fed.iter().any(|o| o.bank != l.bank && !o.ignored)
        || (db.txn.by(Txn::ACCOUNT_ID_DATE).from(input.account_id).all().iter())
            .any(|t| t.imported_id.is_some() && t.imported_by != l.bank);
    let waiting = l.account_id.is_none() && !l.ignored;
    if waiting {
        repull(ctx, db, &l.bank)?;
    }
    db.linked.update(Linked {
        account_id: Some(input.account_id),
        ignored: false,
        since: if waiting { l.seen.clone() } else { l.since.clone() },
        seam: others,
        ..l
    })
}

/// A bank's account given an account of its own, opened for it now — a
/// card a credit account, a savings account savings — under the bank's
/// name for it unless another is given. It takes the bank's whole history
/// and a starting balance before it, which the next import brings (asked
/// of Plaid from the start).
#[mutator("banks.open")]
pub fn open(ctx: &Ctx, db: &mut Db<Money>, input: OpenLinked) -> Result<(), Refusal> {
    let l = db
        .linked
        .get(&input.plaid_account_id)
        .ok_or_else(|| Refusal::new("no such bank account"))?;
    if l.account_id.is_some_and(|a| db.account.get(a).is_some()) {
        return Err(Refusal::new(
            "that bank account has an account here already: follow it again, or link it to another",
        ));
    }
    let name = match input.name.trim() {
        "" => l.name.clone(),
        n => n.to_string(),
    };
    let account = open_for(ctx, db, name, &l.kind, &l.subtype)?;
    repull(ctx, db, &l.bank)?;
    db.linked.update(Linked {
        account_id: Some(account.id),
        ignored: false,
        since: String::new(),
        opening: true,
        seam: false,
        ..l
    })
}

/// The bank's next import asked from its start, and asked for now.
fn repull(ctx: &Ctx, db: &mut Db<Money>, bank: &str) -> Result<(), Refusal> {
    if let Some(b) = db.bank.get(bank) {
        db.bank.update(Bank { repull: true, ..b })?;
    }
    sync(ctx, db, ())
}

/// A bank's account followed again, or not: an ignored one's transactions
/// are dropped from every import after.
#[mutator("banks.ignore")]
pub fn ignore(_: &Ctx, db: &mut Db<Money>, input: IgnoreLinked) -> Result<(), Refusal> {
    let l = db
        .linked
        .get(&input.plaid_account_id)
        .ok_or_else(|| Refusal::new("no such bank account"))?;
    db.linked.update(Linked { ignored: input.ignored, ..l })
}

/// Ask the server's importer to ask the banks now: anybody in the
/// household may. The importer is a peer like any other and sees the ask
/// as a row; what it imports arrives as any import does, and a bank's
/// `synced_ms` moving is the answer.
#[mutator("banks.sync")]
pub fn sync(ctx: &Ctx, db: &mut Db<Money>, _: ()) -> Result<(), Refusal> {
    db.asked.put(Asked {
        user_id: ctx.user().into(),
        asked_ms: ctx.now,
    })
}

/// The latest ask of anybody's: when the importer was last asked to sync.
pub fn last_asked(db: &Db<Money>) -> Option<Asked> {
    db.asked.scan().into_iter().max_by_key(|a| a.asked_ms)
}

/// Plaid's codes for a bank that wants its login again: a reconnect mends
/// it, and nothing else does.
pub const LOGIN_CODES: &[&str] = &["ITEM_LOGIN_REQUIRED", "PENDING_EXPIRATION", "PENDING_DISCONNECT", "ACCESS_NOT_GRANTED"];

/// Every connected bank, what its last import did, and what is wrong with
/// it when something is.
#[view("banks.banks")]
pub fn banks(_: &Ctx, _: &Db<Money>, _: ()) -> Result<crate::budget::Plan<BankLine>, Refusal> {
    Ok(scan::<Money, Bank>().map_with(|db, b| {
        let accounts: Vec<Linked> = db.linked.scan().into_iter().filter(|l| l.bank == b.name).collect();
        BankLine {
            name: b.name.clone(),
            institution: b.institution.clone(),
            synced_ms: b.synced_ms,
            added: b.added,
            updated: b.updated,
            deleted: b.deleted,
            locked: b.locked,
            accounts: accounts.len() as i64,
            error_code: b.error_code.clone(),
            error: b.error.clone(),
            failed_ms: b.failed_ms,
            needs_login: LOGIN_CODES.contains(&b.error_code.as_str()),
            waiting: accounts.iter().filter(|l| l.account_id.is_none() && !l.ignored).count() as i64,
        }
    }))
}

/// Every account a bank has, whether it feeds one here and which, is
/// ignored, or waits for somebody to say: what a household links
/// (`banks.link`), opens an account for (`banks.open`) and stops following
/// (`banks.ignore`) from.
#[view("banks.linked")]
pub fn linked(_: &Ctx, _: &Db<Money>, _: ()) -> Result<crate::budget::Plan<LinkedLine>, Refusal> {
    Ok(scan::<Money, Linked>().map_with(|db, l| {
        let account = l.account_id.and_then(|a| db.account.get(a));
        LinkedLine {
            plaid_account_id: l.plaid_account_id.clone(),
            bank: l.bank.clone(),
            account_id: account.as_ref().map(|a| a.id),
            account: account.map(|a| a.name).unwrap_or_default(),
            name: l.name.clone(),
            mask: l.mask.clone(),
            kind: l.kind.clone(),
            balance: l.balance,
            ignored: l.ignored,
            subtype: l.subtype.clone(),
            waiting: l.account_id.is_none() && !l.ignored,
            opens_as: kind_of(&l.kind, &l.subtype).into(),
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plaid's sign is the other way, and an owed balance is negative here.
    #[test]
    fn plaid_reads_in_our_sign() {
        let t: PlaidTransaction = serde_json::from_str(
            r#"{"transaction_id":"t1","account_id":"a1","amount":12.34,"date":"2026-10-01","name":"SQ *BLUE BOTTLE",
                "merchant_name":"Blue Bottle","original_description":"SQ *BLUE BOTTLE 1234","pending":true}"#,
        )
        .unwrap();
        let b = bank_txn(&t);
        assert_eq!(
            (b.amount, b.payee.as_str(), b.imported_payee.as_str(), b.pending),
            (-1234, "Blue Bottle", "SQ *BLUE BOTTLE 1234", true)
        );
        let card: PlaidAccount =
            serde_json::from_str(r#"{"account_id":"a2","name":"Card","mask":"9999","type":"credit","balances":{"current":410.5}}"#).unwrap();
        assert_eq!(bank_balance(&card, 410.5), -41050);
        assert_eq!(kind_of("depository", "savings"), "savings");
        assert_eq!(kind_of("credit", "credit card"), "credit");
    }
}
