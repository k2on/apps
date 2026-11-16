//! What money says of its routes beyond their paths: what each is called,
//! where one whose row is gone lands. The routes themselves are files, in
//! `src/app` (`crate::Route`, `ark_router::routes!`).
//!
//! **A row is named by its id.** An account, a saved filter and a category
//! carry their row's id (`Id::hex`) and, after it, a slug of their name only
//! to read (`/account/<id>/checking/`): renaming an account keeps its
//! register on every back stack, and two categories of one name in two
//! groups are two routes. The name rides along as the title. A bank is keyed
//! by its name, so that is its route. A route whose row is gone lands on its
//! [`Route::fallback`]: an account's or a filter's register on All Accounts,
//! as the desktop already did.
use crate::Route;

impl Route {
    /// What the page is called.
    pub fn title(&self) -> String {
        match self {
            Route::Budget => "Budget".into(),
            Route::Insights => "Insights".into(),
            Route::Accounts => "All Accounts".into(),
            Route::Inbox => "Needs a Look".into(),
            Route::Filtered => "Filtered".into(),
            Route::Banks => "Bank Connections".into(),
            Route::Settings => "Settings".into(),
            Route::Join { .. } => "Sign In".into(),
            Route::Welcome => "Welcome".into(),
            Route::Account { name, .. } | Route::Filter { name, .. } | Route::Category { name, .. } | Route::Bank { name } => name.clone(),
        }
    }

    /// The id of the row the route names (`Id::hex`), or `None` for a route
    /// that names none.
    pub fn row_id(&self) -> Option<String> {
        match self {
            Route::Account { id, .. } | Route::Filter { id, .. } | Route::Category { id, .. } if !id.is_empty() => Some(id.clone()),
            _ => None,
        }
    }

    /// Whether the route names the row of this id (`Id::hex`) — what every
    /// shell finds a route's row by. So a renamed account is still its
    /// register's, and of two categories of one name only the one meant.
    /// The name is the shells' to pass; it is not which row.
    pub fn names(&self, id: &str, _name: &str) -> bool {
        self.row_id().is_some_and(|mine| mine == id)
    }

    /// Whether the route is a root — the budget or where its money went,
    /// which every shell reaches without another page — rather than a page
    /// pushed onto a back stack.
    pub fn is_root(&self) -> bool {
        matches!(self, Route::Budget | Route::Insights)
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

    /// Every variant survives its URL, names with the characters a
    /// household's are full of among them, and a server with a port and a
    /// path of its own. Falsified by spelling `Filtered` as `/filter`: it
    /// reads back as nothing.
    #[test]
    fn every_route_survives_its_url() {
        assert_round_trips(&[
            Route::Budget,
            Route::Insights,
            Route::Accounts,
            Route::Inbox,
            account(CHECKING, "Checking"),
            account(CHECKING, "Joint / Savings #2"),
            Route::Filter {
                id: FOOD.into(),
                name: "Groceries & Dining 100%".into(),
            },
            Route::Filtered,
            category(FOOD, "Café"),
            Route::Banks,
            Route::Bank { name: "Chase".into() },
            Route::Settings,
            Route::Welcome,
            Route::Join {
                server: "https://money.example.com:8443/household/?a=1&b=2".into(),
                code: Some("XK-42 9".into()),
            },
            Route::Join {
                server: "localhost:3000".into(),
                code: None,
            },
        ]);
        // By id, the name after it to read, a slash at the end.
        assert_eq!(
            account(CHECKING, "Joint / Savings #2").url(),
            format!("/account/{CHECKING}/joint-savings-2/")
        );
        assert_eq!(category(FOOD, "Café").url(), format!("/category/{FOOD}/caf/"));
        assert_eq!(Route::Accounts.url(), "/accounts/");
        assert_eq!(
            Route::Join {
                server: "http://h:80".into(),
                code: Some("c".into())
            }
            .url(),
            "/join/?server=http%3A%2F%2Fh%3A80&code=c"
        );
        assert_eq!(Route::from_url("money://budget/"), Some(Route::Budget), "a phone's link");
    }

    /// Every route is at the address it always was: the folder spells each
    /// as the enum's patterns did. Falsified by moving a file into another
    /// folder that is not a group: its path grows a segment.
    #[test]
    fn every_address_is_where_it_was() {
        let id = CHECKING;
        let urls: Vec<(Route, String)> = vec![
            (Route::Budget, "/budget/".into()),
            (Route::Insights, "/insights/".into()),
            (Route::Accounts, "/accounts/".into()),
            (Route::Inbox, "/inbox/".into()),
            (account(id, "Checking"), format!("/account/{id}/checking/")),
            (
                Route::Filter {
                    id: id.into(),
                    name: "Food".into(),
                },
                format!("/filter/{id}/food/"),
            ),
            (Route::Filtered, "/filtered/".into()),
            (category(id, "Food"), format!("/category/{id}/food/")),
            (Route::Banks, "/banks/".into()),
            (Route::Bank { name: "Chase".into() }, "/bank/Chase/".into()),
            (Route::Settings, "/settings/".into()),
            (Route::Welcome, "/welcome/".into()),
            (
                Route::Join {
                    server: "h".into(),
                    code: Some("c".into()),
                },
                "/join/?server=h&code=c".into(),
            ),
        ];
        for (route, url) in &urls {
            assert_eq!(&route.url(), url);
            assert_eq!(Route::from_url(&format!("money:/{url}")).as_ref(), Some(route), "money:/{url}");
        }
        assert_eq!(Route::statics().len(), 8, "each route that carries nothing");
    }

    /// A renamed account is the same route, and its register the same row:
    /// a register on a back stack survives the rename. Falsified by finding
    /// the row by name: the old name names nothing.
    #[test]
    fn a_rename_keeps_the_route() {
        let before = account(CHECKING, "Checking");
        let after = account(CHECKING, "Everyday");
        assert_eq!(before, after);
        assert_eq!(Route::from_url(&before.url()), Some(after.clone()), "a link from before the rename");
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

    /// A URL nobody wrote names nothing — a row by its name among them —
    /// and a shell lands it on the budget.
    #[test]
    fn a_url_that_means_nothing_is_none() {
        for url in ["/nonsense/", "/account/", "/account/Checking/", "/budget/x/", "/join/", "/join/?code=1"] {
            assert_eq!(Route::from_url(url), None, "{url}");
        }
        assert_eq!(<Route as Routable>::home(), Route::Budget);
        assert_eq!(Route::route_of("/"), Route::Budget);
        assert_eq!(
            Route::from_url("/budget/?inspector=1"),
            Some(Route::Budget),
            "the UI's query is passed over"
        );
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
        assert_eq!(account(CHECKING, "Checking").row_id().as_deref(), Some(CHECKING));
        assert!(!Route::Accounts.names(CHECKING, "All Accounts"));
    }
}
