//! What money's rows say with a figure in it, decided once for every
//! client: a budget line's story, captions, progress and what a screen
//! reader says of it; a bank's status; an item's quantity. Amounts are
//! written in the client's own number style and times as a while ago
//! (`ark_format`): display text, the app's. The rules beneath it (a line's
//! tone, its meter, whether it is a card's payment) are the domain's
//! (`money_core::words`).

use ark::prelude::*;
use ark_format::{ago, amount, NumberStyle};
use money_core::{BankLine, BudgetLine, ItemLine};

/// The two ends of the line under a category's bar: what it was given (of
/// its target, when it has one) and what it spent; for a card's payment,
/// whether the debt is covered and what the card owes. Blank where there
/// is nothing to say.
#[derive(Clone, Debug, PartialEq, Eq, Default, Describe)]
#[ark(ffi)]
pub struct Captions {
    pub left: String,
    pub right: String,
}

/// How a connected bank is doing, in a sentence; `alarm` when it wants
/// somebody.
#[derive(Clone, Debug, PartialEq, Eq, Default, Describe)]
#[ark(ffi)]
pub struct BankStatus {
    pub words: String,
    pub alarm: bool,
}

/// A budget line's words with its figures in them.
pub trait LineWords {
    /// The line under its name: what is happening to it, in words.
    fn story(&self, s: &NumberStyle) -> String;

    /// The two ends of the line under its bar ([`Captions`]).
    fn captions(&self, s: &NumberStyle) -> Captions;

    /// How far along it is, in a word or two, as a table's narrow column
    /// says it: a card's shortfall or "Covered", what was spent on a card,
    /// the share of its target given, or of what it had that was spent.
    fn progress(&self, s: &NumberStyle) -> String;

    /// What a screen reader says of it, after its name.
    fn spoken(&self, s: &NumberStyle) -> String;
}

impl LineWords for BudgetLine {
    /// The line under its name: what is happening to it, in words.
    fn story(&self, s: &NumberStyle) -> String {
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
    fn captions(&self, s: &NumberStyle) -> Captions {
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
    fn progress(&self, s: &NumberStyle) -> String {
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

    /// What a screen reader says of it, after its name.
    fn spoken(&self, s: &NumberStyle) -> String {
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

/// A connected bank's status, in words.
pub trait BankWords {
    /// How it is doing at `now`.
    fn status(&self, now: i64) -> BankStatus;
}

impl BankWords for BankLine {
    fn status(&self, now: i64) -> BankStatus {
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

/// How many of an item, as its line says it: `×2`, `11.482 gal`.
pub fn quantity(thousandths: i64, unit: &str) -> String {
    let n = ark_format::quantity(thousandths);
    match unit {
        "" | "ea" => format!("\u{00d7}{n}"),
        u => format!("{n} {u}"),
    }
}

/// An item's quantity, said.
pub trait ItemWords {
    /// How many of it, said ([`quantity`]).
    fn quantity_words(&self) -> String;
}

impl ItemWords for ItemLine {
    fn quantity_words(&self) -> String {
        quantity(self.quantity, &self.unit)
    }
}
