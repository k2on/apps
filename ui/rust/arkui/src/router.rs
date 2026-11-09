//! Where the app is, as an address bar would say it — for any app's routes,
//! kept in memory on the desktop ([`Memory`]) so that back and forward work
//! as a browser's do.
//!
//! Where the app is goes in a fragment, and the fragment is read back. An app's routes are its domain's `Route` enum, spelled by
//! `ark-router` ([`Routable`]: a path such as `album/Water%20Music`); this
//! file owns the address bar, which says that path after a `#`.
//!
//! **The fragment, not the query.** The query is where a page is told things —
//! which server, a sign-in's `?code=` — and recording a click there would mean
//! rewriting those on every click. A fragment is also the one part of a URL a
//! static host never has to be told about.
//!
//! **A route carries names, not ids.** An id is not something a person types;
//! `#album/Water%20Music` is. So an app resolves a name against what it has —
//! which also decides what a stale link does: land somewhere that is a page,
//! rather than on a heading with nothing under it.
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
pub use ark_router::{join, split, Routable};

/// The fragment `route` is written as in the bar: its path, `#` in front.
pub fn fragment<R: Routable>(route: &R) -> String {
    format!("#{}", route.path())
}

/// The route a fragment names. Never fails: a URL somebody typed wrong
/// lands on the app's home rather than nowhere.
pub fn parse<R: Routable>(fragment: &str) -> R {
    R::from_path(fragment).unwrap_or_else(R::home)
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
    /// The fragment, `#` included, or `None` when there is none (or no bar).
    fn read(&self) -> Option<String>;
    /// Put `fragment` in the bar, as a new history entry or in place of the
    /// current one.
    fn write(&mut self, fragment: &str, nav: Nav);
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

    fn write(&mut self, fragment: &str, nav: Nav) {
        // A new place drops whatever was forward of here, as a browser does.
        match (nav, self.entries.is_empty()) {
            (Nav::Replace, false) => self.entries.truncate(self.at),
            _ => self.entries.truncate(self.at + 1),
        }
        self.entries.push(fragment.to_string());
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
    /// would re-resolve the same fragment — and a fragment naming something
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

    /// Put `fragment` in the bar, for the next [`Router::poll`] to find:
    /// what a [`crate::navigator::Navigator`] does.
    pub(crate) fn write(&mut self, fragment: &str, nav: Nav) {
        self.bar.write(fragment, nav);
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
        let fragment = fragment(want);
        if self.bar.read().as_deref() != Some(fragment.as_str()) {
            self.bar.write(&fragment, self.nav);
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
        let fragment = self.bar.read().map(|f| parse::<R>(&f));
        if !ready || fragment == self.routed {
            return None;
        }
        self.routed = fragment.clone();
        fragment
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for an app's routes, shaped like harken's.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Here {
        Library,
        Albums,
        Playlist(String),
        Album(String),
        Artist(String),
        Work(String),
    }

    impl Routable for Here {
        fn path(&self) -> String {
            match self {
                Here::Library => join("library", None),
                Here::Albums => join("albums", None),
                Here::Playlist(n) => join("playlist", Some(n)),
                Here::Album(n) => join("album", Some(n)),
                Here::Artist(n) => join("artist", Some(n)),
                Here::Work(n) => join("work", Some(n)),
            }
        }

        fn from_path(path: &str) -> Option<Self> {
            Some(match split(path) {
                ("library", None) => Here::Library,
                ("albums", None) => Here::Albums,
                ("playlist", Some(n)) => Here::Playlist(n),
                ("album", Some(n)) => Here::Album(n),
                ("artist", Some(n)) => Here::Artist(n),
                ("work", Some(n)) => Here::Work(n),
                _ => return None,
            })
        }

        fn home() -> Self {
            Here::Library
        }
    }

    /// Every route survives the round trip, including the characters a real
    /// library is full of: a slash would look like the separator, a `#` would
    /// end the fragment, and an accent is two bytes. Falsified by writing the
    /// name unencoded in `join`: the round trips still pass (a string does not
    /// end at a `#`; a browser's fragment does), and the spelling fails.
    #[test]
    fn a_route_survives_the_address_bar() {
        for route in [
            Here::Library,
            Here::Albums,
            Here::Playlist("Favorites".into()),
            Here::Album("Water Music".into()),
            Here::Artist("Johann Sebastian Bach".into()),
            Here::Album("Boléro / Pavane #1".into()),
            // A key with a slash of its own, which only survives because
            // `split` splits on the first separator.
            Here::Work("johann-sebastian-bach/bwv-988".into()),
        ] {
            let fragment = fragment(&route);
            assert_eq!(parse::<Here>(&fragment), route, "round trip of {fragment}");
        }
        // The round trip alone cannot see the encoding — a raw `#` or `/`
        // parses back fine from a string; it is the *browser* that ends the
        // fragment at a `#`. So the spelling itself, once.
        assert_eq!(
            fragment(&Here::Album("Boléro / Pavane #1".into())),
            "#album/Bol%C3%A9ro%20%2F%20Pavane%20%231"
        );
    }

    /// A fragment nobody wrote lands somewhere rather than nowhere.
    #[test]
    fn a_fragment_that_means_nothing_is_home() {
        assert_eq!(parse::<Here>(""), Here::Library);
        assert_eq!(parse::<Here>("#"), Here::Library);
        assert_eq!(parse::<Here>("#nonsense/Water Music"), Here::Library);
        // A truncated escape is not a reason to fail: what decoded, decoded.
        assert_eq!(parse::<Here>("#album/Bol%"), Here::Album("Bol%".into()));
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
        fn write(&mut self, fragment: &str, nav: Nav) {
            if nav == Nav::Replace {
                self.history.pop();
            }
            self.history.push(fragment.to_string());
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
        // A page opened on a link: the bar already says where the app is
        // going, so arriving there is not a second entry for the same place.
        let mut linked: Router<Here, Fake> = Router::with_bar(Fake {
            history: vec!["#library".into(), "#albums".into()],
        });
        linked.sync(&Here::Albums);
        assert_eq!(linked.bar().history.len(), 2, "the link was pushed a second time");

        let mut router: Router<Here, Fake> = Router::with_bar(Fake::default());
        router.sync(&Here::Library);
        router.sync(&Here::Albums);
        assert_eq!(router.bar().history, ["#library", "#albums"]);

        // Asked twice for where it already is: nothing written.
        router.sync(&Here::Albums);
        assert_eq!(router.bar().history.len(), 2);

        // A scrub through three playlists adds nothing: every step rewrites
        // the entry it started from, and the bar is right at every step.
        for name in ["A", "B", "C"] {
            router.replace_next();
            router.sync(&Here::Playlist(name.into()));
            assert_eq!(router.bar().read().unwrap(), format!("#playlist/{name}"));
        }
        assert_eq!(router.bar().history, ["#library", "#playlist/C"], "one scroll, one entry");
        // …and the reset is automatic: the next place is a place again.
        router.sync(&Here::Album("Messiah".into()));
        assert_eq!(router.bar().history.len(), 3);

        // The back button: the bar moves under the app, `poll` reports it
        // once, and the app arriving there writes nothing.
        router.bar.history.pop();
        assert_eq!(router.poll(false), None, "not before there is anything to resolve against");
        assert_eq!(router.poll(true), Some(Here::Playlist("C".into())));
        assert_eq!(router.poll(true), None, "reported once");
        router.sync(&Here::Playlist("C".into()));
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
        router.sync(&Here::Album("Messiah".into()));
        router.go(-1);
        assert_eq!(router.poll(true), Some(Here::Albums));
        router.go(-1);
        assert_eq!(router.poll(true), Some(Here::Library));
        router.go(-1);
        assert_eq!(router.poll(true), None, "nothing before the first");
        router.go(1);
        assert_eq!(router.poll(true), Some(Here::Albums));
        router.sync(&Here::Work("bwv-988".into()));
        router.go(1);
        assert_eq!(router.poll(true), None, "a new place dropped what was forward");
        router.go(-1);
        assert_eq!(router.poll(true), Some(Here::Albums));
    }
}
