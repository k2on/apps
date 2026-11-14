//! Where the app is, as an address bar would say it — for any app's routes,
//! kept in memory on the desktop ([`Memory`]) so that back and forward work
//! as a browser's do.
//!
//! An app's routes are its `Route` enum, declared once with `ark-router`
//! ([`Routable`]), and the bar says each one's URL — `/album/water-music/`,
//! `/account/<id>/checking/` — the same URL the browser's bar says for it
//! and the path of a phone's deep link (ui/docs/app.md, "Addresses"); this
//! file owns the bar.
//!
//! **Read after every update, not on a clock.** The app calls
//! [`Router::poll`] after every update — a string compare, which also
//! catches a deep link that was not yet resolvable when it arrived.
//!
//! **The bar is reconciled, not pushed.** There are several ways what is shown
//! can change — a click, a key, a menu's "Go to", the back button itself — and
//! pushing the route from the ones somebody remembered covers some of them.
//! [`Router::sync`] is called after *every* update with where the app now is,
//! and is the only thing that writes the bar; a rule every call site has to
//! remember is a rule that is already broken.
pub use ark_router::{Address, Routable};

/// The route a URL names. Never fails: a URL somebody typed wrong lands on
/// the app's home rather than nowhere.
pub fn parse<R: Routable>(url: &str) -> R {
    R::route_of(url)
}

/// How the next change to the bar should reach the history.
///
/// Everything is a place you went, except a scrub: walking a sidebar whose
/// cursor *is* the selection changes what is shown on every step, and forty
/// history entries is a back button that needs forty presses to undo one
/// scroll. The URL is still right at every one of the forty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Push,
    Replace,
}

/// Where fragments are read from and written to. [`Memory`] is the real one;
/// a test can hand a [`Router`] anything else.
pub trait AddressBar {
    /// The URL, or `None` when there is none (or no bar).
    fn read(&self) -> Option<String>;
    /// Put `url` in the bar, as a new history entry or in place of the
    /// current one.
    fn write(&mut self, url: &str, nav: Nav);
    /// Go `by` entries through the history — back for `-1`, forward for `1`
    /// — as the browser's buttons do. The bar moves; [`Router::poll`] reads
    /// where to, as it does for the buttons themselves. Past either end, it
    /// stays.
    fn go(&mut self, by: isize);
    /// Whether there is an entry `by` away to go to.
    fn can_go(&self, _by: isize) -> bool {
        true
    }
}

/// An address bar kept in memory — the entries written and which is
/// current — so that back and forward (`^O`, `^I`, a swipe) work as a
/// browser's do, and the one place an app records a change is written once.
#[derive(Debug, Default, Clone)]
pub struct Memory {
    entries: Vec<String>,
    at: usize,
}

impl AddressBar for Memory {
    fn read(&self) -> Option<String> {
        self.entries.get(self.at).cloned()
    }

    fn write(&mut self, url: &str, nav: Nav) {
        // A new place drops whatever was forward of here, as a browser does.
        match (nav, self.entries.is_empty()) {
            (Nav::Replace, false) => self.entries.truncate(self.at),
            _ => self.entries.truncate(self.at + 1),
        }
        self.entries.push(url.to_string());
        self.at = self.entries.len() - 1;
    }

    fn go(&mut self, by: isize) {
        let to = self.at as isize + by;
        if to >= 0 && (to as usize) < self.entries.len() {
            self.at = to as usize;
        }
    }

    fn can_go(&self, by: isize) -> bool {
        let to = self.at as isize + by;
        to >= 0 && (to as usize) < self.entries.len()
    }
}

/// Keeps the address bar and what an app is showing in agreement.
pub struct Router<R, B = Memory> {
    bar: B,
    /// The last route this app acted on or wrote. Without it every update
    /// would re-resolve the same URL — and a URL naming something
    /// this peer has not got resolves to home every time, so it would re-read
    /// the page on every update for as long as the URL said so.
    routed: Option<R>,
    /// Reset to [`Nav::Push`] by every [`Router::sync`], so only the step that
    /// meant otherwise gets it.
    nav: Nav,
}

impl<R: Routable> Default for Router<R, Memory> {
    fn default() -> Self {
        Router::with_bar(Memory::default())
    }
}

impl<R: Routable> Router<R, Memory> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<R: Routable, B: AddressBar> Router<R, B> {
    pub fn with_bar(bar: B) -> Self {
        Router {
            bar,
            routed: None,
            nav: Nav::Push,
        }
    }

    /// Back (`-1`) or forward (`1`) through the history; the app arrives
    /// there by its next [`Router::poll`].
    pub fn go(&mut self, by: isize) {
        self.bar.go(by);
    }

    /// Whether there is anywhere `by` entries away: back (`-1`) at the first
    /// place, forward (`1`) at the last, there is not.
    pub fn can_go(&self, by: isize) -> bool {
        self.bar.can_go(by)
    }

    /// Put `url` in the bar, for the next [`Router::poll`] to find: what a
    /// [`crate::navigator::Navigator`] does.
    pub(crate) fn write(&mut self, url: &str, nav: Nav) {
        self.bar.write(url, nav);
    }

    /// Make the next [`Router::sync`] rewrite the current entry rather than add
    /// one — for a scrub, like a sidebar cursor walking through what it shows.
    pub fn replace_next(&mut self) {
        self.nav = Nav::Replace;
    }

    /// The route last acted on.
    pub fn routed(&self) -> Option<&R> {
        self.routed.as_ref()
    }

    pub fn bar(&self) -> &B {
        &self.bar
    }

    /// Make the bar say `want`, which is where the app now is. Call it after
    /// every update; nothing else should write the bar.
    ///
    /// The write is skipped when the bar already says it, which is exactly
    /// the case where the route *came* from the back button: writing there
    /// would make one place two history entries, and the back button need
    /// pressing twice.
    pub fn sync(&mut self, want: &R) {
        if self.routed.as_ref() == Some(want) {
            return;
        }
        let url = want.url();
        if self.bar.read().as_deref() != Some(url.as_str()) {
            self.bar.write(&url, self.nav);
        }
        self.routed = Some(want.clone());
        self.nav = Nav::Push;
    }

    /// What the bar says, when it says something new: the back button, or a
    /// link somebody was sent. `None` when nothing moved.
    ///
    /// Only consumed once `ready` — once the app has something to resolve a
    /// name against — so a deep link that arrives before the database has
    /// opened is still waiting when it does.
    pub fn poll(&mut self, ready: bool) -> Option<R> {
        let at = self.bar.read().map(|u| parse::<R>(&u));
        if !ready || at == self.routed {
            return None;
        }
        self.routed = at.clone();
        at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_router::Routes;

    /// A stand-in for an app's routes, shaped like harken's.
    #[derive(Debug, Clone, Routes)]
    enum Here {
        #[home]
        #[route("/library")]
        Library,
        #[route("/albums")]
        Albums,
        #[route("/playlist/:name")]
        Playlist { name: String },
        #[route("/album/:name")]
        Album { name: String },
        #[route("/work/:name")]
        Work { name: String },
    }

    fn playlist(n: &str) -> Here {
        Here::Playlist { name: n.into() }
    }

    fn album(n: &str) -> Here {
        Here::Album { name: n.into() }
    }

    /// Every route survives the bar, including the characters a real library
    /// is full of: a slash would read as two segments, a `#` would end the
    /// URL, an accent is two bytes. Falsified by writing the name
    /// unencoded: the spelling fails.
    #[test]
    fn a_route_survives_the_address_bar() {
        for route in [
            Here::Library,
            Here::Albums,
            playlist("Favorites"),
            album("Water Music"),
            album("Boléro / Pavane #1"),
            Here::Work {
                name: "johann-sebastian-bach/bwv-988".into(),
            },
        ] {
            assert_eq!(parse::<Here>(&route.url()), route, "round trip of {}", route.url());
        }
        assert_eq!(album("Boléro / Pavane #1").url(), "/album/Bol%C3%A9ro%20%2F%20Pavane%20%231/");
    }

    /// A URL that means nothing lands somewhere rather than nowhere.
    #[test]
    fn a_url_that_means_nothing_is_home() {
        assert_eq!(parse::<Here>(""), Here::Library);
        assert_eq!(parse::<Here>("/"), Here::Library);
        assert_eq!(parse::<Here>("/nonsense/Water Music/"), Here::Library);
        // A truncated escape is not a reason to fail: what decoded, decoded.
        assert_eq!(parse::<Here>("/album/Bol%/"), album("Bol%"));
    }

    /// A bar with a history, the way a browser has one.
    #[derive(Default)]
    struct Fake {
        history: Vec<String>,
    }

    impl AddressBar for Fake {
        fn read(&self) -> Option<String> {
            self.history.last().cloned()
        }
        fn write(&mut self, url: &str, nav: Nav) {
            if nav == Nav::Replace {
                self.history.pop();
            }
            self.history.push(url.to_string());
        }
        fn go(&mut self, _by: isize) {}
    }

    /// Routes visited are history entries; a scrub is one; and the back
    /// button's own answer is not written back.
    ///
    /// Falsified three ways: ignoring `nav` in `sync` leaves four entries
    /// where the scrub should have left two; dropping the "bar already says
    /// it" check pushes a page's own link a second time; and not recording
    /// the route in `poll` makes it report the same route twice.
    #[test]
    fn the_bar_is_reconciled_and_a_scrub_is_one_entry() {
        let mut linked: Router<Here, Fake> = Router::with_bar(Fake {
            history: vec!["/library/".into(), "/albums/".into()],
        });
        linked.sync(&Here::Albums);
        assert_eq!(linked.bar().history.len(), 2, "the link was pushed a second time");

        let mut router: Router<Here, Fake> = Router::with_bar(Fake::default());
        router.sync(&Here::Library);
        router.sync(&Here::Albums);
        assert_eq!(router.bar().history, ["/library/", "/albums/"]);
        router.sync(&Here::Albums);
        assert_eq!(router.bar().history.len(), 2);
        for name in ["A", "B", "C"] {
            router.replace_next();
            router.sync(&playlist(name));
            assert_eq!(router.bar().read().unwrap(), format!("/playlist/{name}/"));
        }
        assert_eq!(router.bar().history, ["/library/", "/playlist/C/"], "one scroll, one entry");
        router.sync(&album("Messiah"));
        assert_eq!(router.bar().history.len(), 3);
        router.bar.history.pop();
        assert_eq!(router.poll(false), None, "not before there is anything to resolve against");
        assert_eq!(router.poll(true), Some(playlist("C")));
        assert_eq!(router.poll(true), None, "reported once");
        router.sync(&playlist("C"));
        assert_eq!(router.bar().history.len(), 2, "arriving where the back button went is not a new entry");
    }

    /// The desktop's bar in memory goes back and forward as a browser's
    /// does, and a new place drops what was forward. Falsified by not
    /// truncating in `write`: the last `go(1)` still found the album.
    #[test]
    fn the_desktop_bar_goes_back_and_forward() {
        let mut router: Router<Here> = Router::new();
        router.sync(&Here::Library);
        router.sync(&Here::Albums);
        router.sync(&album("Messiah"));
        router.go(-1);
        assert_eq!(router.poll(true), Some(Here::Albums));
        router.go(-1);
        assert_eq!(router.poll(true), Some(Here::Library));
        router.go(-1);
        assert_eq!(router.poll(true), None, "nothing before the first");
        router.go(1);
        assert_eq!(router.poll(true), Some(Here::Albums));
        router.sync(&Here::Work { name: "bwv-988".into() });
        router.go(1);
        assert_eq!(router.poll(true), None, "a new place dropped what was forward");
        router.go(-1);
        assert_eq!(router.poll(true), Some(Here::Albums));
    }
}
