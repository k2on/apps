//! Where money can be: every route, one path each (`ark-router`), on every
//! shell — the desktop's and the browser's address bar, each phone's back
//! stack, a deep link.
//!
//! **A row is named by its id.** An account, a saved filter and a category
//! carry their row's id (`Id::hex`) and, after it, a slug of their name only
//! to read (`account/<id>/checking`): renaming an account keeps its register
//! on every back stack, and two categories of one name in two groups are two
//! routes. The name rides along as the title. A bank is keyed by its name,
//! so that is its route. An older link's name (`account/Checking`) still
//! reads, with no id: a shell resolves that one by name ([`Route::row_id`] is
//! `None`), and only that one. A route whose row is gone lands on its
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
    /// One account's register, by the account's id; its name is the title.
    #[route("/account/:id(hex)/~name")]
    #[route("/account/:name")]
    Account { id: String, name: String },
    /// A saved filter's register, by its id.
    #[route("/filter/:id(hex)/~name")]
    #[route("/filter/:name")]
    Filter { id: String, name: String },
    /// A register for conditions not saved: the last ones this window had.
    #[route("/filtered")]
    Filtered,
    /// A category's month, by the category's id.
    #[route("/category/:id(hex)/~name")]
    #[route("/category/:name")]
    Category { id: String, name: String },
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
            Route::Account { name, .. } | Route::Filter { name, .. } | Route::Category { name, .. } | Route::Bank { name } => name.clone(),
        }
    }

    /// The id of the row the route names (`Id::hex`), or `None` for a route
    /// that names none, and for an older link's that named its row by name
    /// (a shell finds that one by [`Route::title`]).
    pub fn row_id(&self) -> Option<String> {
        match self {
            Route::Account { id, .. } | Route::Filter { id, .. } | Route::Category { id, .. } if !id.is_empty() => Some(id.clone()),
            _ => None,
        }
    }

    /// Whether the route names the row of this id (`Id::hex`) and name —
    /// what every shell finds a route's row by. By the id; by the name only
    /// for an older link's, which has no id. So a renamed account is still
    /// its register's, and of two categories of one name only the one meant.
    pub fn names(&self, id: &str, name: &str) -> bool {
        match (self.row_id(), self) {
            (Some(mine), _) => mine == id,
            (None, Route::Account { name: mine, .. } | Route::Filter { name: mine, .. } | Route::Category { name: mine, .. }) => {
                !mine.is_empty() && mine == name
            }
            _ => false,
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

    /// The row's id, or nil for an older link's by name (`Route::row_id`).
    #[uniffi::method(name = "row_id")]
    pub fn ffi_row_id(&self) -> Option<String> {
        self.row_id()
    }

    /// Whether the route names this row (`Route::names`).
    #[uniffi::method(name = "names")]
    pub fn ffi_names(&self, id: String, name: String) -> bool {
        self.names(&id, &name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_router::{assert_round_trips, Routable};

    const CHECKING: &str = "0000000000000000000000000000000a";
    const FOOD: &str = "0000000000000000000000000000000b";
    const OTHER_FOOD: &str = "0000000000000000000000000000000c";

    fn account(id: &str, name: &str) -> Route {
        Route::Account {
            id: id.into(),
            name: name.into(),
        }
    }

    fn category(id: &str, name: &str) -> Route {
        Route::Category {
            id: id.into(),
            name: name.into(),
        }
    }

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
            account(CHECKING, "Checking"),
            account(CHECKING, "Joint / Savings #2"),
            account("", "Joint / Savings #2"),
            Route::Filter {
                id: FOOD.into(),
                name: "Groceries & Dining 100%".into(),
            },
            Route::Filter {
                id: String::new(),
                name: "Groceries & Dining 100%".into(),
            },
            Route::Filtered,
            category(FOOD, "Café"),
            category("", "Café"),
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
        // By id, the name after it to read.
        assert_eq!(
            account(CHECKING, "Joint / Savings #2").path(),
            format!("account/{CHECKING}/joint-savings-2")
        );
        assert_eq!(category(FOOD, "Café").path(), format!("category/{FOOD}/caf"));
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

    /// A renamed account is the same route, and its register the same row:
    /// a register on a back stack survives the rename. Falsified by finding
    /// the row by name: the old name names nothing.
    #[test]
    fn a_rename_keeps_the_route() {
        let before = account(CHECKING, "Checking");
        let after = account(CHECKING, "Everyday");
        assert_eq!(before, after);
        assert_eq!(Route::from_path(&before.path()), Some(after.clone()), "a link from before the rename");
        assert!(before.names(CHECKING, "Everyday"), "the renamed row is still the route's");
        assert!(!before.names(FOOD, "Checking"), "another row of the old name is not");
    }

    /// Two categories of one name, in two groups, are two routes, and each
    /// finds only its own. Falsified by identifying a category by its name.
    #[test]
    fn categories_of_one_name_stay_apart() {
        let food = category(FOOD, "Food");
        let other = category(OTHER_FOOD, "Food");
        assert_ne!(food, other);
        assert_ne!(food.key(), other.key());
        assert!(food.names(FOOD, "Food") && !food.names(OTHER_FOOD, "Food"));
        assert!(other.names(OTHER_FOOD, "Food") && !other.names(FOOD, "Food"));
    }

    /// The desktop's older links named their row: they still read, by name,
    /// and only those are found by name.
    #[test]
    fn an_older_link_by_name_still_reads() {
        let old = Route::from_path("#account/Joint%20%2F%20Savings%20%232").unwrap();
        assert_eq!(old, account("", "Joint / Savings #2"));
        assert_eq!(old.row_id(), None);
        assert!(old.names(CHECKING, "Joint / Savings #2"));
        assert!(!old.names(CHECKING, "Checking"));
        assert_eq!(old.path(), "account/Joint%20%2F%20Savings%20%232", "written as it was");
        assert_eq!(
            Route::from_path("filter/Paychecks"),
            Some(Route::Filter {
                id: String::new(),
                name: "Paychecks".into()
            })
        );
        assert_eq!(Route::from_path("category/Rent").unwrap().title(), "Rent");
        assert_eq!(account(CHECKING, "Checking").row_id().as_deref(), Some(CHECKING));
        assert!(!Route::Accounts.names(CHECKING, "All Accounts"));
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
        assert_eq!(account(CHECKING, "x").fallback(), Route::Accounts);
        assert_eq!(
            Route::Filter {
                id: FOOD.into(),
                name: "x".into()
            }
            .fallback(),
            Route::Accounts
        );
        assert_eq!(Route::Bank { name: "x".into() }.fallback(), Route::Banks);
        assert_eq!(category(FOOD, "x").fallback(), Route::Budget);
        assert_eq!(Route::Accounts.title(), "All Accounts");
        assert_eq!(account(CHECKING, "Checking").title(), "Checking");
        assert!(Route::Budget.is_root() && !Route::Accounts.is_root());
        assert!(Route::Filtered.is_register() && !Route::Budget.is_register());
    }
}
