//! What the views' rows say, decided once for every client: a budget
//! line's tone and the words under it, the month's, a transaction's, a
//! bank's and an ask's. The phones read these through their records'
//! methods (`money/domain`'s `ffi` feature), the window calls them as they
//! are; no client works them out again. Amounts are written in the
//! client's own number style (`ark_format`).

use ark::prelude::*;
use ark_format::{ago, amount, NumberStyle};

use crate::accounts::capitalized;
use crate::schema::AskState;
use crate::types::*;

// * A budget line

/// What a category's figure says by its colour, as YNAB's do: decided
/// here for every client, each drawing it in its own tint (`Short` and
/// `OnCard` amber, `Overspent` red, `Positive` green).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi))]
pub enum Tone {
    /// Nothing, neither good nor bad.
    #[default]
    Neutral,
    /// Money with a job.
    Positive,
    /// Short of what its target asks this month, or — a card's payment —
    /// of what the card owes.
    Short,
    /// Below nothing in cash, which next month's Ready to Assign pays for.
    Overspent,
    /// Below nothing, all of it on a card: new debt rather than cash gone.
    OnCard,
}

impl Tone {
    /// An amount's own tone: money positive, below nothing overspent.
    pub fn of_amount(cents: i64) -> Tone {
        match cents {
            c if c > 0 => Tone::Positive,
            c if c < 0 => Tone::Overspent,
            _ => Tone::Neutral,
        }
    }

    /// The tone as a sentence's word: "funded", "overspent on a card".
    pub fn words(self) -> &'static str {
        match self {
            Tone::Neutral => "nothing available",
            Tone::Positive => "funded",
            Tone::Short => "underfunded",
            Tone::Overspent => "overspent",
            Tone::OnCard => "overspent on a card",
        }
    }
}

/// The two ends of the line under a category's bar: what it was given (of
/// its target, when it has one) and what it spent; for a card's payment,
/// whether the debt is covered and what the card owes. Blank where there
/// is nothing to say.
#[derive(Clone, Debug, PartialEq, Eq, Default, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi))]
pub struct Captions {
    pub left: String,
    pub right: String,
}

/// How far along a category is, as its bar draws it: a share from 0 to
/// 1000 (thousandths) and the tone it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi))]
pub struct Meter {
    pub thousandths: i64,
    pub tone: Tone,
}

impl Meter {
    /// The share as a fraction, for a toolkit's progress bar.
    pub fn fraction(&self) -> f64 {
        self.thousandths as f64 / 1000.0
    }
}

fn share(part: i64, whole: i64) -> i64 {
    match whole {
        w if w <= 0 => 0,
        w => (part.max(0).saturating_mul(1000) / w).min(1000),
    }
}

impl BudgetLine {
    /// Whether it is a card's payment category: one of the cards' own
    /// group, which the domain keeps as the cards change.
    pub fn is_payment(&self) -> bool {
        self.card.is_some() || self.group_id == crate::cards::card_group()
    }

    /// Whether it is the budget's own Ready to Assign.
    pub fn is_ready_to_assign(&self) -> bool {
        self.category_id == crate::budget::ready_to_assign()
    }

    /// Whether what the month gives it is anybody's to change by hand: not
    /// Ready to Assign's, and not a month its formula assigns.
    pub fn assignable(&self) -> bool {
        !self.ruled && !self.is_ready_to_assign()
    }

    /// Overspent, and all of it on a card: its shortfall is the card's
    /// debt now, not next month's cash.
    pub fn on_card(&self) -> bool {
        self.available < 0 && self.credit_overspent >= -self.available
    }

    /// What its figure says by its colour. Overspending on a card apart
    /// from cash; a card's payment by whether it covers the debt (its
    /// target never counts); any other by its target, once it has one.
    pub fn tone(&self) -> Tone {
        match () {
            _ if self.on_card() => Tone::OnCard,
            _ if self.available < 0 => Tone::Overspent,
            _ if self.is_payment() && self.underfunded > 0 => Tone::Short,
            _ if self.is_payment() => match self.available > 0 {
                true => Tone::Positive,
                false => Tone::Neutral,
            },
            _ if self.target > 0 && self.assigned < self.target => Tone::Short,
            _ if self.available > 0 => Tone::Positive,
            _ => Tone::Neutral,
        }
    }

    /// Its bar, when it has one: for a card's payment that owes something,
    /// how much of the debt is set aside; for a category with a target,
    /// how much of it the month gave. None for any other.
    pub fn meter(&self) -> Option<Meter> {
        if self.is_payment() {
            if self.debt <= 0 {
                return None;
            }
            let t = share(self.debt - self.underfunded, self.debt);
            let tone = if t >= 1000 { Tone::Positive } else { Tone::Short };
            return Some(Meter { thousandths: t, tone });
        }
        if self.target <= 0 {
            return None;
        }
        let t = share(self.assigned, self.target);
        let tone = match () {
            _ if self.on_card() => Tone::OnCard,
            _ if self.available < 0 => Tone::Overspent,
            _ if t < 1000 => Tone::Short,
            _ => Tone::Positive,
        };
        Some(Meter { thousandths: t, tone })
    }

    /// The line under its name: what is happening to it, in words.
    pub fn story(&self, s: &NumberStyle) -> String {
        let a = |c: i64| amount(c, s);
        let had = self.carried + self.assigned;
        match () {
            _ if self.on_card() => format!("{} on a card, not covered", a(self.credit_overspent)),
            _ if self.available < 0 && self.credit_overspent > 0 => {
                format!("Overspent by {}, {} of it on a card", a(-self.available), a(self.credit_overspent))
            }
            _ if self.available < 0 => format!("Overspent by {}", a(-self.available)),
            _ if self.is_payment() && self.underfunded > 0 => format!("{} short of the {} owed", a(self.underfunded), a(self.debt)),
            _ if self.is_payment() && self.debt > 0 => format!("Covered: {} owed", a(self.debt)),
            _ if self.is_payment() => "Nothing owed on the card".into(),
            _ if self.target > 0 && self.assigned < self.target => format!("{} more needed this month", a(self.target - self.assigned)),
            _ if self.activity < 0 && had > 0 => format!("Spent {} of {}", a(-self.activity), a(had)),
            _ if self.activity < 0 => format!("Spent {}", a(-self.activity)),
            _ if self.target > 0 => "Funded".into(),
            _ if self.ruled => "Assigned by rule".into(),
            _ => String::new(),
        }
    }

    /// The two ends of the line under its bar ([`Captions`]).
    pub fn captions(&self, s: &NumberStyle) -> Captions {
        let a = |c: i64| amount(c, s);
        let spent = match self.activity {
            0 => String::new(),
            n => format!("Spent {}", a(-n)),
        };
        let (left, right) = match () {
            _ if self.is_payment() && self.debt <= 0 => ("Nothing owed".into(), String::new()),
            _ if self.is_payment() => (
                match self.underfunded {
                    u if u > 0 => format!("{} short", a(u)),
                    _ => "Covered".into(),
                },
                format!("Owed {}", a(self.debt)),
            ),
            _ if self.target > 0 => (format!("{} of {}", a(self.assigned), a(self.target)), spent),
            _ if self.activity != 0 || self.assigned != 0 || self.ruled => (format!("Assigned {}", a(self.assigned)), spent),
            _ => (String::new(), String::new()),
        };
        Captions { left, right }
    }

    /// How far along it is, in a word or two, as a table's narrow column
    /// says it: a card's shortfall or "Covered", what was spent on a card,
    /// the share of its target given, or of what it had that was spent.
    pub fn progress(&self, s: &NumberStyle) -> String {
        let had = self.carried + self.assigned;
        match () {
            _ if self.is_payment() && self.underfunded > 0 => format!("{} short", amount(self.underfunded, s)),
            _ if self.is_payment() && self.debt > 0 => "Covered".into(),
            _ if self.is_payment() => String::new(),
            _ if self.credit_overspent > 0 => format!("{} on card", amount(self.credit_overspent, s)),
            _ if self.target > 0 => format!("{}% goal", (self.assigned.max(0) * 100 / self.target).min(999)),
            _ if self.activity < 0 && had > 0 => format!("{}% used", (-self.activity * 100 / had).min(999)),
            _ => String::new(),
        }
    }

    /// What the formula its month follows assigns, said where it would be
    /// assigned by hand ("Assigned by rule: 10% × …"); none when it is
    /// assigned by hand.
    pub fn rule_words(&self) -> Option<String> {
        if !self.ruled {
            return None;
        }
        Some(match self.rule.trim() {
            "" => "Assigned by rule".into(),
            r => format!("Assigned by rule: {r}"),
        })
    }

    /// What assigning it takes to cover its card's debt: what it was given
    /// and what it lacks; none when nothing is uncovered or it is no card's.
    pub fn to_cover(&self) -> Option<i64> {
        (self.is_payment() && self.underfunded > 0).then_some(self.assigned + self.underfunded)
    }

    /// What it still lacks of its target this month; nothing for a card's.
    pub fn to_target(&self) -> i64 {
        match self.is_payment() {
            true => 0,
            false => (self.target - self.assigned).max(0),
        }
    }

    /// What a screen reader says of it, after its name.
    pub fn spoken(&self, s: &NumberStyle) -> String {
        let a = |c: i64| amount(c, s);
        if self.is_payment() {
            let mut w = format!("{} set aside for the card, which owes {}", a(self.available), a(self.debt));
            if self.underfunded > 0 {
                w += &format!(". {} not covered", a(self.underfunded));
            }
            return w;
        }
        let mut w = format!("{} available, {}. Assigned {}", a(self.available), self.tone().words(), a(self.assigned));
        if self.target > 0 {
            w += &format!(" of a {} target", a(self.target));
        }
        if self.ruled {
            w += ", by its rule";
        }
        if self.credit_overspent > 0 {
            w += &format!(". {} spent on a card beyond what it had", a(self.credit_overspent));
        }
        w
    }
}

// * The month

impl Summary {
    /// What Ready to Assign says under itself.
    pub fn ready_words(&self) -> String {
        match self.ready_to_assign {
            r if r < 0 => "You've assigned more than you have".into(),
            0 => "Every dollar has a job".into(),
            _ => "Give these dollars a job".into(),
        }
    }

    /// Its figure's tone: positive while there is money to assign,
    /// overspent once more was assigned than there is.
    pub fn tone(&self) -> Tone {
        match self.ready_to_assign {
            r if r > 0 => Tone::Positive,
            r if r < 0 => Tone::Overspent,
            _ => Tone::Neutral,
        }
    }

    /// How many categories ask for attention this month, as a badge says it:
    /// those overspent and those underfunded.
    pub fn attention(&self) -> i64 {
        self.overspent + self.underfunded
    }
}

/// What the cards owe beyond what their payment categories hold: the
/// month's lines', hidden ones left out.
pub fn uncovered(lines: &[BudgetLine]) -> i64 {
    lines.iter().filter(|l| l.is_payment() && !l.hidden).map(|l| l.underfunded).sum()
}

// * A transaction

impl TxnLine {
    /// Whether it has no category it could be approved with.
    pub fn needs_category(&self) -> bool {
        self.category == crate::transactions::NEEDS_A_CATEGORY
    }

    /// What the bank called it, when it was imported and the bank's words
    /// say more than the payee does (`CHIPOTLE 1234 SAN FRANCISCO CA` for
    /// Chipotle); none otherwise.
    pub fn bank_said(&self) -> Option<String> {
        let said = self.imported_payee.trim();
        (self.imported && !said.is_empty() && !said.eq_ignore_ascii_case(self.payee.trim())).then(|| said.to_string())
    }
}

// * Banks

/// How long an answered ask is still told of.
pub const ASK_SHOWN_MS: i64 = 15 * 60 * 1000;

/// How long an ask goes unanswered before a client wonders whether the
/// server can answer at all.
pub const ASK_QUIET_MS: i64 = 30_000;

impl AskLine {
    /// Whether it is still the server's or Plaid's to finish.
    pub fn is_open(&self) -> bool {
        matches!(self.state, AskState::Asked | AskState::Waiting)
    }

    /// The bank it is about, as a sentence names it.
    pub fn bank_name(&self) -> String {
        match (self.institution.as_str(), self.bank.as_str()) {
            ("", "") => "the bank".into(),
            ("", b) => capitalized(b),
            (i, _) => i.into(),
        }
    }

    /// What has come of it, in a sentence, at `now`.
    pub fn said(&self, now: i64) -> String {
        let name = self.bank_name();
        match (self.state, self.what.as_str()) {
            (AskState::Asked, _) if now - self.asked_ms > ASK_QUIET_MS => "The server hasn't answered yet. Does it have Plaid's keys?".into(),
            (AskState::Asked, "disconnect") => format!("Asking the server to disconnect {name}\u{2026}"),
            (AskState::Asked, _) => "Asking the server for Plaid's page\u{2026}".into(),
            (AskState::Waiting, _) => "Log in to your bank on Plaid's page. This finishes by itself once you have.".into(),
            (AskState::Done, "connect") => format!("Connected {name}. Choose what each of its accounts feeds."),
            (AskState::Done, "reconnect") => format!("Logged in to {name} again."),
            (AskState::Done, "add_accounts") => {
                format!("Shared {name}'s accounts again. One shared for the first time opens with the next import.")
            }
            (AskState::Done, _) => format!("Disconnected {name}. Its accounts stay, kept by hand from now on."),
            (AskState::Failed, what) => match self.message.trim() {
                "" => format!("Couldn't {} {name}.", what.replace('_', " ")),
                m => format!("Couldn't {} {name}: {m}", what.replace('_', " ")),
            },
            (AskState::Expired, _) => match self.message.trim() {
                "" => "Plaid's page closed before it was finished.".into(),
                m => m.into(),
            },
        }
    }

    /// How long Plaid's page stays open, said ("It closes in 12 min");
    /// none unless it waits on it.
    pub fn closes(&self, now: i64) -> Option<String> {
        if self.state != AskState::Waiting || self.expires_ms <= now {
            return None;
        }
        Some(match (self.expires_ms - now) / 60_000 {
            0 => "It closes in under a minute".into(),
            m => format!("It closes in {m} min"),
        })
    }
}

/// The ask a client tells of: the newest of this person's, while it is
/// open or lately answered, unless it was put away (`dismissed`).
pub fn ask_shown(asks: &[AskLine], dismissed: Option<Id<crate::schema::BankAsk>>, now: i64) -> Option<AskLine> {
    let a = asks.first()?;
    (Some(a.id) != dismissed && (a.is_open() || now - a.answered_ms < ASK_SHOWN_MS)).then(|| a.clone())
}

/// How a connected bank is doing, in a sentence; `alarm` when it wants
/// somebody.
#[derive(Clone, Debug, PartialEq, Eq, Default, Describe)]
#[cfg_attr(feature = "ffi", ark(ffi))]
pub struct BankStatus {
    pub words: String,
    pub alarm: bool,
}

impl BankLine {
    /// What it is called: the institution's name, or the name the server
    /// keeps it under.
    pub fn title(&self) -> String {
        match self.institution.is_empty() {
            true => capitalized(&self.name),
            false => self.institution.clone(),
        }
    }

    /// How it is doing at `now`.
    pub fn status(&self, now: i64) -> BankStatus {
        let alarm = |words: String| BankStatus { words, alarm: true };
        if self.needs_login {
            return alarm("The bank wants its login again. Reconnect it.".into());
        }
        if !self.error.is_empty() {
            return alarm(format!("The last import failed {}: {}", ago(self.failed_ms, now), self.error));
        }
        let mut words = match self.synced_ms {
            0 => "Waiting for its first import".to_string(),
            t => format!("Synced {} \u{00b7} {} new, {} changed", ago(t, now), self.added, self.updated),
        };
        if self.locked > 0 {
            words += &format!(", {} left alone (reconciled)", self.locked);
        }
        match self.waiting {
            0 => {}
            1 => words += " \u{00b7} 1 account to choose for",
            n => words += &format!(" \u{00b7} {n} accounts to choose for"),
        }
        BankStatus { words, alarm: false }
    }
}

/// What wants somebody in Settings: logins run out, and accounts waiting
/// for a choice.
pub fn banks_wanting(banks: &[BankLine]) -> i64 {
    banks.iter().map(|b| b.waiting + b.needs_login as i64).sum()
}

impl LinkedLine {
    /// A bank's account as its row names it: `Sapphire ••4321`.
    pub fn title(&self) -> String {
        match self.mask.is_empty() {
            true => self.name.clone(),
            false => format!("{} \u{2022}\u{2022}{}", self.name, self.mask),
        }
    }

    /// What kind of account the bank says it is: its subtype, or its kind.
    pub fn kind_words(&self) -> String {
        capitalized(if self.subtype.is_empty() { &self.kind } else { &self.subtype })
    }

    /// What the choice to open an account for it says: "New Credit Card
    /// Account".
    pub fn opens_words(&self) -> String {
        format!("New {} Account", crate::accounts::kind_title(&self.opens_as))
    }
}

/// A bank's accounts, those waiting for a choice first.
pub fn bank_accounts(bank: &str, linked: &[LinkedLine]) -> Vec<LinkedLine> {
    let mine = linked.iter().filter(|l| l.bank == bank);
    mine.clone().filter(|l| l.waiting).chain(mine.filter(|l| !l.waiting)).cloned().collect()
}

// * Receipts

/// A store as a receipt names it: `Amazon` for `amazon`.
pub fn store_name(store: &str) -> String {
    match store {
        "amazon" => "Amazon".into(),
        "target" => "Target".into(),
        "costco" => "Costco".into(),
        crate::stores::PAPER => "Receipt".into(),
        s => s.into(),
    }
}

/// How many of an item, as its line says it: `×2`, `11.482 gal`.
pub fn quantity(thousandths: i64, unit: &str) -> String {
    let n = ark_format::quantity(thousandths);
    match unit {
        "" | "ea" => format!("\u{00d7}{n}"),
        u => format!("{n} {u}"),
    }
}

impl ItemLine {
    /// How many of it, said ([`quantity`]).
    pub fn quantity_words(&self) -> String {
        quantity(self.quantity, &self.unit)
    }
}

impl ReceiptLine {
    /// The store, as it is named.
    pub fn store_name(&self) -> String {
        store_name(&self.store)
    }

    /// Whether nothing pays for it yet: no transaction, whole or by a charge.
    pub fn unmatched(&self) -> bool {
        self.txn_id.is_none() && self.charges == 0
    }
}

/// How many unmatched receipts a transaction's page offers to attach.
pub const UNMATCHED_SHOWN: usize = 8;

/// The receipts nothing pays for yet that a transaction of `amount` on
/// `day` is offered to attach: the nearest in amount first, then in days;
/// at most [`UNMATCHED_SHOWN`].
pub fn unmatched_for(receipts: &[ReceiptLine], amount: i64, day: &str) -> Vec<ReceiptLine> {
    let far = |d: &str| match crate::dates::is_day(d) && crate::dates::is_day(day) {
        true => crate::dates::days_between(d, day).abs(),
        false => i64::MAX,
    };
    let mut out: Vec<&ReceiptLine> = receipts.iter().filter(|r| r.unmatched()).collect();
    out.sort_by_key(|r| ((r.total - amount).abs(), far(&r.date), r.id.clone()));
    out.into_iter().take(UNMATCHED_SHOWN).cloned().collect()
}
