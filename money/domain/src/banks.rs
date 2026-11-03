//! Connecting banks from the app: asks a household member authors, which
//! the server's importer answers (`money/server`'s `importer.rs`).
//!
//! A bank's login — Plaid's access token for it — is the server's alone,
//! kept in its data directory and never in a row, so nobody's device can
//! connect a bank by itself. What a device can do is ask:
//!
//! - **`banks.connect`**: the importer opens a Plaid Hosted Link page for
//!   it and answers with the page's address ([`AskState::Waiting`], `url`),
//!   which the asker's window opens. Once the person has logged in to their
//!   bank there, the importer keeps the login, imports the bank at once and
//!   answers [`AskState::Done`] with the name it keeps the bank under.
//!   Every account the bank has then waits for somebody to say what it
//!   feeds: an account already here (`banks.link`), one opened for it
//!   (`banks.open`), or nothing (`banks.ignore`).
//!   Unless it is a second login to a bank already here (the same
//!   institution): then its first import folds it into the first
//!   (`crate::plaid`), each account taking its twin's choice, and the
//!   login it replaces is removed (`banks.retired`).
//! - **`banks.reconnect`**: the same page in Plaid's update mode, for a bank
//!   whose login ran out (`ITEM_LOGIN_REQUIRED`, [`crate::plaid::LOGIN_CODES`]).
//! - **`banks.add_accounts`**: the same page in update mode with Plaid's
//!   account picker, for sharing an account of a connected bank's that
//!   was not shared at first: each one picked opens here at the next
//!   import, as an account of its kind with its whole history.
//! - **`banks.disconnect`**: the importer tells Plaid to forget the login,
//!   drops it, and answers `Done`, which removes the bank and what it knew
//!   of its accounts here. The accounts and their transactions stay: they
//!   are the household's.
//!
//! An ask the importer cannot finish is answered [`AskState::Failed`] with
//! a sentence, or [`AskState::Expired`] when nobody finished Plaid's page in
//! time. Only the importer (the [`crate::BANK`] role) answers, and nothing
//! it answers carries a secret: the page's address is a one-time link, not
//! a login. Each person sees their own asks ([`asks`]).

use ark::prelude::*;

use crate::schema::*;
use crate::types::*;

fn ask(ctx: &Ctx, db: &mut Db<Money>, what: &str, bank: &Option<Bank>) -> Result<(), Refusal> {
    // After every ask before it, even within one millisecond: newest first
    // is the order a person's asks are read in.
    let last = db.bank_ask.scan().iter().map(|a| a.asked_ms).max().unwrap_or(0);
    db.bank_ask.insert(BankAsk {
        id: ctx.new_id(),
        what: what.into(),
        bank: bank.as_ref().map(|b| b.name.clone()).unwrap_or_default(),
        institution: bank.as_ref().map(|b| b.institution.clone()).unwrap_or_default(),
        state: AskState::Asked,
        asked_ms: ctx.now.max(last + 1),
        user_id: ctx.user().into(),
        ..BankAsk::default()
    })
}

fn bank_of(db: &Db<Money>, name: &str) -> Result<Bank, Refusal> {
    db.bank.get(name).ok_or_else(|| Refusal::new("no bank of that name is connected"))
}

/// Ask the server to connect a bank: anybody in the household may.
#[mutator("banks.connect")]
pub fn connect(ctx: &Ctx, db: &mut Db<Money>, _: ()) -> Result<(), Refusal> {
    ask(ctx, db, "connect", &None)
}

/// Ask the server for a page to log in to a connected bank again.
#[mutator("banks.reconnect")]
pub fn reconnect(ctx: &Ctx, db: &mut Db<Money>, input: BankName) -> Result<(), Refusal> {
    let bank = bank_of(db, &input.bank)?;
    ask(ctx, db, "reconnect", &Some(bank))
}

/// Ask the server for a page to share more of a connected bank's accounts
/// (Plaid's update mode, its account picker): every account picked there
/// that was not shared before is opened here at the next import, as an
/// account of its kind with its whole history (`banks.open`).
#[mutator("banks.add_accounts")]
pub fn add_accounts(ctx: &Ctx, db: &mut Db<Money>, input: BankName) -> Result<(), Refusal> {
    let bank = bank_of(db, &input.bank)?;
    ask(ctx, db, "add_accounts", &Some(bank))
}

/// Ask the server to disconnect a bank: Plaid forgets the login and the
/// bank stops importing. Its accounts here, and what is in them, stay.
#[mutator("banks.disconnect")]
pub fn disconnect(ctx: &Ctx, db: &mut Db<Money>, input: BankName) -> Result<(), Refusal> {
    let bank = bank_of(db, &input.bank)?;
    ask(ctx, db, "disconnect", &Some(bank))
}

/// The importer's answer to an ask. A connection done is the bank known
/// here at once, before its first import lands; a reconnection done
/// clears what its last failure said; a disconnection done forgets the
/// bank and what it knew of its accounts.
#[mutator("banks.answer")]
pub fn answer(ctx: &Ctx, db: &mut Db<Money>, input: Answer) -> Result<(), Refusal> {
    if !ctx.has_role(crate::BANK) {
        return Err(Refusal::new("only the bank importer answers"));
    }
    let asked = db.bank_ask.get(input.id).ok_or_else(|| Refusal::new("no such ask"))?;
    if input.state == AskState::Done {
        match asked.what.as_str() {
            "connect" if !input.bank.trim().is_empty() && db.bank.get(&input.bank).is_none() => db.bank.insert(Bank {
                name: input.bank.clone(),
                institution: input.institution.clone(),
                user_id: ctx.user().into(),
                // Its first import folds it into another login to the same
                // bank, if there is one (`crate::plaid`).
                fresh: true,
                ..Bank::default()
            })?,
            "reconnect" => {
                if let Some(b) = db.bank.get(&asked.bank) {
                    db.bank.update(Bank {
                        error_code: String::new(),
                        error: String::new(),
                        ..b
                    })?;
                }
            }
            "add_accounts" => {
                if let Some(b) = db.bank.get(&asked.bank) {
                    db.bank.update(Bank {
                        error_code: String::new(),
                        error: String::new(),
                        adding: true,
                        ..b
                    })?;
                }
            }
            "disconnect" => forget(db, &asked.bank)?,
            _ => {}
        }
    }
    let bank = match input.bank.trim() {
        "" => asked.bank.clone(),
        b => b.to_string(),
    };
    let institution = match input.institution.trim() {
        "" => asked.institution.clone(),
        i => i.to_string(),
    };
    db.bank_ask.update(BankAsk {
        state: input.state,
        bank,
        institution,
        url: input.url,
        message: input.message,
        expires_ms: input.expires_ms,
        answered_ms: ctx.now,
        ..asked
    })
}

/// A bank gone: its row and its accounts' links. The accounts it fed stay,
/// kept by hand from now on.
pub(crate) fn forget(db: &mut Db<Money>, bank: &str) -> Result<(), Refusal> {
    for l in db.linked.scan().into_iter().filter(|l| l.bank == bank) {
        db.linked.remove(&l)?;
    }
    if db.bank.get(bank).is_some() {
        db.bank.delete(bank.to_string())?;
    }
    Ok(())
}

/// The importer could not import a bank: what Plaid said, for the
/// household to see beside the bank (a login run out wants a reconnect).
#[mutator("banks.failed")]
pub fn failed(ctx: &Ctx, db: &mut Db<Money>, input: BankFailed) -> Result<(), Refusal> {
    if !ctx.has_role(crate::BANK) {
        return Err(Refusal::new("only the bank importer says what a bank said"));
    }
    let previous = db.bank.get(&input.bank);
    let institution = match (input.institution.trim(), &previous) {
        ("", Some(b)) => b.institution.clone(),
        (i, _) => i.to_string(),
    };
    db.bank.put(Bank {
        name: input.bank.clone(),
        institution,
        error_code: input.error_code,
        error: input.error,
        failed_ms: ctx.now,
        user_id: previous.as_ref().map_or_else(|| ctx.user().to_string(), |b| b.user_id.clone()),
        ..previous.unwrap_or_default()
    })
}

/// The reader's own asks about bank connections, newest first: where the
/// page they asked for is, and what came of it.
#[view("banks.asks")]
pub fn asks(ctx: &Ctx, _: &Db<Money>, _: ()) -> Result<crate::budget::Plan<AskLine>, Refusal> {
    let me = ctx.user().to_string();
    Ok(scan::<Money, BankAsk>()
        .filter(move |a| a.user_id == me)
        .sort(|a| -a.asked_ms)
        .map(|a| AskLine {
            id: a.id,
            what: a.what.clone(),
            bank: a.bank.clone(),
            institution: a.institution.clone(),
            state: a.state,
            url: a.url.clone(),
            message: a.message.clone(),
            expires_ms: a.expires_ms,
            asked_ms: a.asked_ms,
            answered_ms: a.answered_ms,
        }))
}

/// Every ask nobody has answered, and every page still open, oldest
/// first: what the importer looks at.
pub fn open_asks(db: &Db<Money>) -> Vec<BankAsk> {
    let mut a: Vec<BankAsk> = (db.bank_ask.scan().into_iter())
        .filter(|a| matches!(a.state, AskState::Asked | AskState::Waiting))
        .collect();
    a.sort_by_key(|a| a.asked_ms);
    a
}
