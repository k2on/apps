//! What the address bar says, in a browser — for any app's idea of a place.
//!
//! A page that shows a list, then a record, then a person and answers the back
//! button by leaving the site has broken the one control every browser has.
//! So where the app is goes in the URL's fragment, and the fragment is read
//! back. An app says what its places are by implementing [`Route`]; this file
//! owns the address bar.
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
//! **Read by polling, not by listening.** Hearing `popstate` means a closure
//! kept alive for the life of the page publishing into a channel iced can
//! subscribe to. An app with a tick already has a loop; reading
//! `location.hash` on it is a string compare that gives the same answer a
//! frame later. See [`Router::poll`].
//!
//! **The history API, not `location.hash = …`.** Both change the URL; the
//! second also fires a `hashchange` the poll would read back as somebody
//! pressing the back button.
//!
//! **The bar is reconciled, not pushed.** There are several ways what is shown
//! can change — a click, a key, a menu's "Go to", the back button itself — and
//! pushing the route from the ones somebody remembered covers some of them.
//! [`Router::sync`] is called after *every* update with where the app now is,
//! and is the only thing that writes the bar; a rule every call site has to
//! remember is a rule that is already broken.
use crate::url;

/// Somewhere an app can be, as the address bar spells it.
///
/// `parse` must accept anything — a URL somebody typed wrong should land
/// somewhere rather than nowhere — and `parse(&r.fragment()) == r` for every
/// route the app makes. [`join`] and [`split`] do the spelling.
pub trait Route: Clone + PartialEq {
    /// The fragment this route is written as, `#` included.
    fn fragment(&self) -> String;
    /// Read one back. Never fails: an unknown fragment is the app's home.
    fn parse(fragment: &str) -> Self;
}

/// `#kind`, or `#kind/name` with the name percent-encoded — so a `/` in a name
/// cannot read as the separator, a `#` cannot end the fragment, and an accent
/// survives as the two bytes it is.
pub fn join(kind: &str, name: Option<&str>) -> String {
    let mut out = format!("#{kind}");
    if let Some(name) = name {
        out.push('/');
        url::encode(name, &mut out);
    }
    out
}

/// The other half of [`join`]: the kind, and the decoded name after the
/// *first* `/` if there is one — a name may itself be a key with a slash in
/// it (`johann-sebastian-bach/bwv-988`), which is why only the first splits.
pub fn split(fragment: &str) -> (&str, Option<String>) {
    let body = fragment.trim_start_matches('#');
    match body.split_once('/') {
        Some((kind, name)) => (kind, Some(url::decode(name))),
        None => (body, None),
    }
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

/// Where fragments are read from and written to. [`Browser`] is the real one;
/// a test can hand a [`Router`] anything else.
pub trait AddressBar {
    /// The fragment, `#` included, or `None` when there is none (or no bar).
    fn read(&self) -> Option<String>;
    /// Put `fragment` in the bar, as a new history entry or in place of the
    /// current one.
    fn write(&mut self, fragment: &str, nav: Nav);
}

/// The browser's address bar. On the desktop there is none, so it reads
/// nothing and writes nowhere — the type is shared so that the one place an app
/// records a change is written once and reads the same in both builds.
#[derive(Debug, Default, Clone, Copy)]
pub struct Browser;

#[cfg(target_arch = "wasm32")]
impl AddressBar for Browser {
    fn read(&self) -> Option<String> {
        let hash = web_sys::window()?.location().hash().ok()?;
        (!hash.is_empty()).then_some(hash)
    }

    fn write(&mut self, fragment: &str, nav: Nav) {
        let Some(history) = web_sys::window().and_then(|w| w.history().ok()) else {
            return;
        };
        let null = wasm_bindgen::JsValue::NULL;
        let _ = match nav {
            Nav::Push => history.push_state_with_url(&null, "", Some(fragment)),
            Nav::Replace => history.replace_state_with_url(&null, "", Some(fragment)),
        };
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl AddressBar for Browser {
    fn read(&self) -> Option<String> {
        None
    }

    fn write(&mut self, _fragment: &str, _nav: Nav) {}
}

/// Keeps the address bar and what an app is showing in agreement.
pub struct Router<R, B = Browser> {
    bar: B,
    /// The last route this app acted on or wrote. Without it the tick would
    /// re-resolve the same fragment twenty times a second — and a fragment
    /// naming something this peer has not got resolves to home every time, so
    /// it would re-read the page twenty times a second for as long as the URL
    /// said so.
    routed: Option<R>,
    /// Reset to [`Nav::Push`] by every [`Router::sync`], so only the step that
    /// meant otherwise gets it.
    nav: Nav,
}

impl<R: Route> Default for Router<R, Browser> {
    fn default() -> Self {
        Router::with_bar(Browser)
    }
}

impl<R: Route> Router<R, Browser> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<R: Route, B: AddressBar> Router<R, B> {
    pub fn with_bar(bar: B) -> Self {
        Router {
            bar,
            routed: None,
            nav: Nav::Push,
        }
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
        let fragment = want.fragment();
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
        let fragment = self.bar.read().map(|f| R::parse(&f));
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

    /// A stand-in for an app's places, shaped like harken's.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Place {
        Library,
        Albums,
        Playlist(String),
        Album(String),
        Artist(String),
        Work(String),
    }

    impl Route for Place {
        fn fragment(&self) -> String {
            match self {
                Place::Library => join("library", None),
                Place::Albums => join("albums", None),
                Place::Playlist(n) => join("playlist", Some(n)),
                Place::Album(n) => join("album", Some(n)),
                Place::Artist(n) => join("artist", Some(n)),
                Place::Work(n) => join("work", Some(n)),
            }
        }

        fn parse(fragment: &str) -> Self {
            match split(fragment) {
                ("albums", None) => Place::Albums,
                ("playlist", Some(n)) => Place::Playlist(n),
                ("album", Some(n)) => Place::Album(n),
                ("artist", Some(n)) => Place::Artist(n),
                ("work", Some(n)) => Place::Work(n),
                _ => Place::Library,
            }
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
            Place::Library,
            Place::Albums,
            Place::Playlist("Favorites".into()),
            Place::Album("Water Music".into()),
            Place::Artist("Johann Sebastian Bach".into()),
            Place::Album("Boléro / Pavane #1".into()),
            // A key with a slash of its own, which only survives because
            // `split` splits on the first separator.
            Place::Work("johann-sebastian-bach/bwv-988".into()),
        ] {
            let fragment = route.fragment();
            assert_eq!(Place::parse(&fragment), route, "round trip of {fragment}");
        }
        // The round trip alone cannot see the encoding — a raw `#` or `/`
        // parses back fine from a string; it is the *browser* that ends the
        // fragment at a `#`. So the spelling itself, once.
        assert_eq!(
            Place::Album("Boléro / Pavane #1".into()).fragment(),
            "#album/Bol%C3%A9ro%20%2F%20Pavane%20%231"
        );
    }

    /// A fragment nobody wrote lands somewhere rather than nowhere.
    #[test]
    fn a_fragment_that_means_nothing_is_home() {
        assert_eq!(Place::parse(""), Place::Library);
        assert_eq!(Place::parse("#"), Place::Library);
        assert_eq!(Place::parse("#nonsense/Water Music"), Place::Library);
        // A truncated escape is not a reason to fail: what decoded, decoded.
        assert_eq!(Place::parse("#album/Bol%"), Place::Album("Bol%".into()));
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
    }

    /// Places visited are history entries; a scrub is one; and the back
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
        let mut linked: Router<Place, Fake> = Router::with_bar(Fake {
            history: vec!["#library".into(), "#albums".into()],
        });
        linked.sync(&Place::Albums);
        assert_eq!(linked.bar().history.len(), 2, "the link was pushed a second time");

        let mut router: Router<Place, Fake> = Router::with_bar(Fake::default());
        router.sync(&Place::Library);
        router.sync(&Place::Albums);
        assert_eq!(router.bar().history, ["#library", "#albums"]);

        // Asked twice for where it already is: nothing written.
        router.sync(&Place::Albums);
        assert_eq!(router.bar().history.len(), 2);

        // A scrub through three playlists adds nothing: every step rewrites
        // the entry it started from, and the bar is right at every step.
        for name in ["A", "B", "C"] {
            router.replace_next();
            router.sync(&Place::Playlist(name.into()));
            assert_eq!(router.bar().read().unwrap(), format!("#playlist/{name}"));
        }
        assert_eq!(router.bar().history, ["#library", "#playlist/C"], "one scroll, one entry");
        // …and the reset is automatic: the next place is a place again.
        router.sync(&Place::Album("Messiah".into()));
        assert_eq!(router.bar().history.len(), 3);

        // The back button: the bar moves under the app, `poll` reports it
        // once, and the app arriving there writes nothing.
        router.bar.history.pop();
        assert_eq!(router.poll(false), None, "not before there is anything to resolve against");
        assert_eq!(router.poll(true), Some(Place::Playlist("C".into())));
        assert_eq!(router.poll(true), None, "reported once");
        router.sync(&Place::Playlist("C".into()));
        assert_eq!(router.bar().history.len(), 2, "arriving where the back button went is not a new entry");
    }
}
