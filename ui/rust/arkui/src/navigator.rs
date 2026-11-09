//! Navigating, by the same names on every platform: `push`, `replace`,
//! `back`, `reset` and `handle` — the iPhone's `Navigator` over a
//! `NavigationStack`'s path (`ui/swift`), Android's over Compose's back
//! stack (`ui/kotlin`), and this one over the address bar's history
//! ([`crate::router::Router`], and so [`crate::history::History`]).
//!
//! The desktop's and the browser's bar is the back stack: a route pushed is
//! written into it, and the app arrives there by its next `poll`, the road
//! the back button and a link somebody typed already take. So navigating
//! never sets the app's page behind the bar's back.
use crate::router::{fragment, AddressBar, Nav, Routable, Router};

/// The route a link names: `scheme://album/Water%20Music` (an app's own
/// URL scheme), `#album/…` (a page's fragment) or the bare path. `None` for
/// a link of another scheme, or one that names nothing here.
pub fn route_of_link<R: Routable>(link: &str, scheme: &str) -> Option<R> {
    let path = match link.strip_prefix(scheme).and_then(|rest| rest.strip_prefix("://")) {
        Some(path) => path,
        None if link.contains("://") => return None,
        None => link,
    };
    R::from_path(path)
}

/// The link a desktop app was opened with — `scheme://…` among its
/// arguments, which is how a desktop's URL handler passes one. A browser's
/// link is its address, which the router already reads.
pub fn link_arg(scheme: &str) -> Option<String> {
    let prefix = format!("{scheme}://");
    std::env::args().skip(1).find(|a| a.starts_with(&prefix))
}

/// Where an app is, moved.
pub trait Navigator<R> {
    /// Navigate to `route`, a new entry on the back stack.
    fn push(&mut self, route: &R);
    /// Navigate to `route` in place of where the app is.
    fn replace(&mut self, route: &R);
    /// Back one entry; at the first, nowhere.
    fn back(&mut self);
    /// Start again at `route`. A browser's history cannot be emptied, so
    /// there it is `replace`.
    fn reset(&mut self, route: &R);
    /// A link the app was opened with or sent (`harken://album/…`): what it
    /// names is offered to `take` first — a sign-in link is the onboarding's,
    /// not a page — and navigated to if `take` does not take it. `false` for
    /// a link that names nothing here.
    fn handle(&mut self, link: &str, scheme: &str, take: &mut dyn FnMut(&R) -> bool) -> bool;
}

impl<R: Routable, B: AddressBar> Navigator<R> for Router<R, B> {
    fn push(&mut self, route: &R) {
        self.write(&fragment(route), Nav::Push);
    }

    fn replace(&mut self, route: &R) {
        self.write(&fragment(route), Nav::Replace);
    }

    fn back(&mut self) {
        self.go(-1);
    }

    fn reset(&mut self, route: &R) {
        self.replace(route);
    }

    fn handle(&mut self, link: &str, scheme: &str, take: &mut dyn FnMut(&R) -> bool) -> bool {
        let Some(route) = route_of_link::<R>(link, scheme) else {
            return false;
        };
        if !take(&route) {
            self.push(&route);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{join, split};

    #[derive(Debug, Clone, PartialEq)]
    enum Here {
        Home,
        Album(String),
        Join(String),
    }

    impl Routable for Here {
        fn path(&self) -> String {
            match self {
                Here::Home => join("home", None),
                Here::Album(n) => join("album", Some(n)),
                Here::Join(s) => format!("join?server={}", crate::url::encoded(s)),
            }
        }
        fn from_path(path: &str) -> Option<Self> {
            if let Some(q) = path.strip_prefix("join?server=") {
                return Some(Here::Join(crate::url::decode(q)));
            }
            match split(path) {
                ("home", None) => Some(Here::Home),
                ("album", Some(n)) => Some(Here::Album(n)),
                _ => None,
            }
        }
        fn home() -> Self {
            Here::Home
        }
    }

    #[derive(Default)]
    struct Fake {
        history: Vec<String>,
    }

    impl AddressBar for Fake {
        fn read(&self) -> Option<String> {
            self.history.last().cloned()
        }
        fn write(&mut self, fragment: &str, nav: Nav) {
            if nav == Nav::Replace {
                self.history.pop();
            }
            self.history.push(fragment.to_string());
        }
        fn go(&mut self, by: isize) {
            if by < 0 {
                self.history.pop();
            }
        }
    }

    /// Navigating writes the bar and the app arrives by `poll`; a link is
    /// pushed, a sign-in link is handed over and not navigated to, and a link
    /// of another scheme names nothing. Falsified by `handle` pushing what
    /// `take` took: the bar would say `#join?…`.
    #[test]
    fn navigating_goes_through_the_bar() {
        let mut router: Router<Here, Fake> = Router::with_bar(Fake::default());
        router.sync(&Here::Home);
        router.push(&Here::Album("Water Music".into()));
        assert_eq!(router.poll(true), Some(Here::Album("Water Music".into())));
        router.replace(&Here::Album("Messiah".into()));
        assert_eq!(router.bar().history, ["#home", "#album/Messiah"]);
        router.back();
        assert_eq!(router.poll(true), Some(Here::Home));

        let mut joined = vec![];
        let mut take = |r: &Here| match r {
            Here::Join(s) => {
                joined.push(s.clone());
                true
            }
            _ => false,
        };
        assert!(router.handle("harken://album/Bol%C3%A9ro", "harken", &mut take));
        assert_eq!(router.poll(true), Some(Here::Album("Boléro".into())));
        assert!(router.handle("harken://join?server=https%3A%2F%2Fh%3A8443", "harken", &mut take));
        assert!(!router.handle("money://album/x", "harken", &mut take), "another app's link");
        assert!(!router.handle("harken://auth?code=1", "harken", &mut take), "the sign-in's own");
        assert!(router.handle("#album/x", "harken", &mut take), "a page's fragment");
        assert_eq!(joined, ["https://h:8443"]);
        assert_eq!(router.bar().history.last().map(String::as_str), Some("#album/x"));
    }
}
