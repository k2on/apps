//! Where money can be: every route, one path each (`ark-router`), on every
//! shell — the desktop's and the browser's address bar, each phone's back
//! stack, a deep link.
//!
//! **A route carries names, not ids.** An account, a saved filter, a
//! category and a bank are named; a shell finds the row by its name when it
//! navigates there, and a route whose row is gone lands on its
//! [`Route::fallback`]: an account's or a filter's register on All Accounts,
//! as the desktop already did.
use ark_router::Routes;

/// Somewhere money can be.
#[derive(Debug, Clone, Routes)]
#[cfg_attr(feature = "ffi", derive(uniffi::Enum), uniffi::export(Eq, Hash))]
pub enum Route {
    /// The month's budget.
    #[home]
    #[route("/budget")]
    Budget,
    /// Where the month's money went: the desktop's Reflect, the phones'
    /// Insights.
    #[route("/reflect")]
    Reflect,
    /// Every account's register: All Accounts.
    #[route("/accounts")]
    Accounts,
    /// What was imported and not yet approved, or has no category.
    #[route("/inbox")]
    Inbox,
    /// One account's register, by the account's name.
    #[route("/account/:name")]
    Account { name: String },
    /// A saved filter's register, by its name.
    #[route("/filter/:name")]
    Filter { name: String },
    /// A register for conditions not saved: the last ones this window had.
    #[route("/filtered")]
    Filtered,
    /// A category's month, by the category's name.
    #[route("/category/:name")]
    Category { name: String },
    /// The banks Plaid feeds.
    #[route("/banks")]
    Banks,
    /// One connected bank, by the name the server keeps it under.
    #[route("/bank/:name")]
    Bank { name: String },
    #[route("/settings")]
    Settings,
    /// Sign in to a server from a link or a QR code: `join?server=…&code=…`.
    /// Not a page: a shell hands it to the onboarding.
    #[route("/join?server=:server&code=:code")]
    Join { server: String, code: Option<String> },
}

impl Route {
    /// What the page is called.
    pub fn title(&self) -> String {
        match self {
            Route::Budget => "Budget".into(),
            Route::Reflect => "Reflect".into(),
            Route::Accounts => "All Accounts".into(),
            Route::Inbox => "Needs a Look".into(),
            Route::Filtered => "Filtered".into(),
            Route::Banks => "Bank Connections".into(),
            Route::Settings => "Settings".into(),
            Route::Join { .. } => "Sign In".into(),
            Route::Account { name } | Route::Filter { name } | Route::Category { name } | Route::Bank { name } => name.clone(),
        }
    }

    /// Whether the route is a root — the budget or where its money went,
    /// which every shell reaches without another page — rather than a page
    /// pushed onto a back stack.
    pub fn is_root(&self) -> bool {
        matches!(self, Route::Budget | Route::Reflect)
    }

    /// Where the route lands when what it names is gone: an account's or a
    /// filter's register on All Accounts, a category on the budget, a bank
    /// on the banks, and anything else on the budget.
    pub fn fallback(&self) -> Route {
        match self {
            Route::Account { .. } | Route::Filter { .. } => Route::Accounts,
            Route::Bank { .. } => Route::Banks,
            _ => Route::Budget,
        }
    }

    /// Whether the route is a register of transactions.
    pub fn is_register(&self) -> bool {
        matches!(
            self,
            Route::Accounts | Route::Inbox | Route::Account { .. } | Route::Filter { .. } | Route::Filtered
        )
    }
}

/// The route a path names, or none: `Route::from_path`, for the phones
/// (UniFFI writes no constructor on an enum).
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn route_from_path(path: String) -> Option<Route> {
    Route::from_path(&path)
}

#[cfg(feature = "ffi")]
#[uniffi::export]
impl Route {
    /// The path (`Route::path`): `account/Checking`.
    #[uniffi::method(name = "path")]
    pub fn ffi_path(&self) -> String {
        self.path()
    }

    #[uniffi::method(name = "title")]
    pub fn ffi_title(&self) -> String {
        self.title()
    }

    #[uniffi::method(name = "is_root")]
    pub fn ffi_is_root(&self) -> bool {
        self.is_root()
    }

    #[uniffi::method(name = "fallback")]
    pub fn ffi_fallback(&self) -> Route {
        self.fallback()
    }

    #[uniffi::method(name = "is_register")]
    pub fn ffi_is_register(&self) -> bool {
        self.is_register()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_router::{assert_round_trips, Routable};

    /// Every variant survives its path, names with the characters a
    /// household's are full of among them, and a server with a port and a
    /// path of its own. Falsified by spelling `Filtered` as `/filter`: it
    /// reads back as nothing.
    #[test]
    fn every_route_survives_its_path() {
        assert_round_trips(&[
            Route::Budget,
            Route::Reflect,
            Route::Accounts,
            Route::Inbox,
            Route::Account { name: "Checking".into() },
            Route::Account {
                name: "Joint / Savings #2".into(),
            },
            Route::Filter {
                name: "Groceries & Dining 100%".into(),
            },
            Route::Filtered,
            Route::Category { name: "Café".into() },
            Route::Banks,
            Route::Bank { name: "Chase".into() },
            Route::Settings,
            Route::Join {
                server: "https://money.example.com:8443/household/?a=1&b=2".into(),
                code: Some("XK-42 9".into()),
            },
            Route::Join {
                server: "localhost:3000".into(),
                code: None,
            },
        ]);
        // The spelling every desktop link already carries.
        assert_eq!(
            Route::Account {
                name: "Joint / Savings #2".into()
            }
            .path(),
            "account/Joint%20%2F%20Savings%20%232"
        );
        assert_eq!(Route::Accounts.path(), "accounts");
        assert_eq!(
            Route::Join {
                server: "http://h:80".into(),
                code: Some("c".into())
            }
            .path(),
            "join?server=http%3A%2F%2Fh%3A80&code=c"
        );
    }

    /// A path nobody wrote names nothing, and the desktop lands it on the
    /// budget.
    #[test]
    fn a_path_that_means_nothing_is_none() {
        for path in ["", "#", "nonsense", "account", "account/", "budget/x", "join", "join?code=1"] {
            assert_eq!(Route::from_path(path), None, "{path}");
        }
        assert_eq!(<Route as Routable>::home(), Route::Budget);
    }

    /// The desktop's rules, now every shell's.
    #[test]
    fn what_a_gone_route_falls_back_to() {
        assert_eq!(Route::Account { name: "x".into() }.fallback(), Route::Accounts);
        assert_eq!(Route::Filter { name: "x".into() }.fallback(), Route::Accounts);
        assert_eq!(Route::Bank { name: "x".into() }.fallback(), Route::Banks);
        assert_eq!(Route::Category { name: "x".into() }.fallback(), Route::Budget);
        assert_eq!(Route::Accounts.title(), "All Accounts");
        assert!(Route::Budget.is_root() && !Route::Accounts.is_root());
        assert!(Route::Filtered.is_register() && !Route::Budget.is_register());
    }
}
