//! Where the window can be: what the page shows ([`Source`]), how the address
//! bar spells it ([`Place`]), which pane has the cursor ([`Pane`]), and which
//! grid has the keyboard ([`Focus`]).
use arkui::glyphs;
use arkui::route::{join, split, Route};
use arkui::vim;

use ark_client::Id;

/// What the page is showing, which is what the sidebar picks.
///
/// Each variant carries the name it was selected by rather than an id to look
/// up: the header draws it, and a library that changed underneath the
/// selection should not make the header go blank.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Everything, in library order: the library view itself, which is open
    /// for as long as the window is. Every other source opens views of its
    /// own — see `Peer::open_page`.
    Library,
    /// Every album, and everyone who made something: index pages of cards.
    ///
    /// These were once *headings* over a sidebar row per album and per
    /// artist, which is fine for the twenty a demo has and wrong for a
    /// library — a sidebar that grows with the collection is a sidebar you
    /// scroll to find the thing you scroll.
    Albums,
    Artists,
    /// Everyone this library has a work *by*, which is not `Artists`: that is
    /// every `media.creator` of every kind, and this is exactly the people
    /// some `work` is by. A library of pop has none, and the sidebar then
    /// draws no such line.
    Composers,
    Playlist(Id, String),
    Album(String),
    Artist(String),
    /// One composer's works. Reached from a card on `Composers`.
    Works(String),
    /// One work and the performances of it this library holds: the derived
    /// key, and the title to draw — together, because a page needs both and
    /// the key is not a name.
    Work(String, String),
    /// One performance, in the order the work goes. The label is who played
    /// it, the only thing that tells two of them apart.
    Recording(String, String),
}

impl Source {
    /// How the address bar spells it. Names rather than ids.
    pub fn place(&self) -> Place {
        match self {
            Source::Library => Place::Library,
            Source::Albums => Place::Albums,
            Source::Artists => Place::Artists,
            Source::Composers => Place::Composers,
            Source::Playlist(_, name) => Place::Playlist(name.clone()),
            Source::Album(name) => Place::Album(name.clone()),
            Source::Artist(name) => Place::Artist(name.clone()),
            Source::Works(name) => Place::Composer(name.clone()),
            // The key, not the title — and still a name in the sense the
            // route means: `#work/johann-sebastian-bach/bwv-988` is built out
            // of the composer and the catalogue number, so somebody can read
            // it and type it. What that rule forbids is a uuid.
            Source::Work(id, _) => Place::Work(id.clone()),
            Source::Recording(id, _) => Place::Recording(id.clone()),
        }
    }

    /// The heading this source belongs under in the sidebar, or `None` for a
    /// page reached from another page. Headings are *derived* rather than
    /// interleaved, so a cursor can address line N without counting past
    /// decoration.
    pub fn heading(&self) -> Option<&'static str> {
        match self {
            Source::Library | Source::Albums | Source::Artists | Source::Composers => Some("Music"),
            Source::Playlist(..) => Some("Playlists"),
            Source::Album(_) | Source::Artist(_) | Source::Works(_) | Source::Work(..) | Source::Recording(..) => None,
        }
    }

    /// The drawing that goes beside it — in the sidebar, and on a menu's
    /// "Go to" entry, which asks the source it goes to rather than naming a
    /// glyph itself, so the two cannot come to disagree about what an album
    /// looks like.
    pub fn glyph(&self) -> &'static [u8] {
        match self {
            Source::Library => glyphs::NOTE,
            Source::Albums | Source::Album(_) => glyphs::ALBUM,
            Source::Artists | Source::Artist(_) => glyphs::ARTIST,
            Source::Composers | Source::Works(_) => glyphs::COMPOSER,
            Source::Work(..) | Source::Recording(..) => glyphs::LIBRARY,
            Source::Playlist(..) => glyphs::PLAYLIST,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Source::Library => "Songs",
            Source::Albums => "Albums",
            Source::Artists => "Artists",
            Source::Composers => "Composers",
            Source::Playlist(_, name)
            | Source::Album(name)
            | Source::Artist(name)
            | Source::Works(name)
            | Source::Work(_, name)
            | Source::Recording(_, name) => name,
        }
    }

    /// What a page of this kind is called, above its title: "Goldberg
    /// Variations" is a work and a record, and neither is obvious from the
    /// name alone.
    pub fn kind(&self) -> &'static str {
        match self {
            Source::Album(_) => "Album",
            Source::Artist(_) => "Artist",
            Source::Works(_) => "Composer",
            Source::Work(..) => "Work",
            Source::Recording(..) => "Recording",
            _ => "",
        }
    }
}

/// Somewhere the window can be, as the URL spells it.
///
/// **A route carries names, not ids.** A playlist is identified by id and an
/// id is not something a person types, so the URL says the name and the app
/// resolves it against the playlists it has — which also decides what a stale
/// link does: a playlist since renamed lands on the library, which is a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Library,
    /// The index pages: they carry no name because they *are* the whole list.
    Albums,
    Artists,
    Composers,
    Playlist(String),
    Album(String),
    Artist(String),
    /// One composer's works: `#composer/…`, because the name after the slash
    /// is the composer's and a fragment should read as what it names.
    Composer(String),
    /// A work and a recording, by the keys the domain derives. The slash
    /// inside one is why a fragment splits on the *first* separator only.
    Work(String),
    Recording(String),
}

impl Route for Place {
    fn fragment(&self) -> String {
        let (kind, name) = match self {
            Place::Library => ("library", None),
            Place::Albums => ("albums", None),
            Place::Artists => ("artists", None),
            Place::Composers => ("composers", None),
            Place::Playlist(n) => ("playlist", Some(n)),
            Place::Album(n) => ("album", Some(n)),
            Place::Artist(n) => ("artist", Some(n)),
            Place::Composer(n) => ("composer", Some(n)),
            Place::Work(n) => ("work", Some(n)),
            Place::Recording(n) => ("recording", Some(n)),
        };
        join(kind, name.map(String::as_str))
    }

    /// Anything unrecognised is the library: a URL somebody typed wrong
    /// should land somewhere rather than nowhere.
    fn parse(fragment: &str) -> Place {
        match split(fragment) {
            ("albums", None) => Place::Albums,
            ("artists", None) => Place::Artists,
            ("composers", None) => Place::Composers,
            ("playlist", Some(n)) => Place::Playlist(n),
            ("album", Some(n)) => Place::Album(n),
            ("artist", Some(n)) => Place::Artist(n),
            ("composer", Some(n)) => Place::Composer(n),
            ("work", Some(n)) => Place::Work(n),
            ("recording", Some(n)) => Place::Recording(n),
            _ => Place::Library,
        }
    }
}

/// Which part of the window the cursor is in.
///
/// The panes and what leaving one means are the app's business, not the
/// grammar's: `vim` knows a motion was refused, and this decides that a
/// refused `l` in the sidebar means the page.
///
/// The now-playing bar is deliberately not one of them: everything it does
/// has a key of its own — `<Space>`, `{`, `}`, `d` — so making it a third
/// place the cursor can be would only add a stop to `<Tab>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sidebar,
    Tracks,
}

impl Pane {
    /// Where a motion this pane refused should take the cursor, if anywhere.
    pub fn beyond(self, motion: vim::Motion) -> Option<Pane> {
        use vim::Motion::{Left, Right};
        match (self, motion) {
            (Pane::Sidebar, Right(_)) => Some(Pane::Tracks),
            (Pane::Tracks, Left(_)) => Some(Pane::Sidebar),
            _ => None,
        }
    }

    /// `<Tab>` order.
    pub fn next(self) -> Pane {
        match self {
            Pane::Sidebar => Pane::Tracks,
            Pane::Tracks => Pane::Sidebar,
        }
    }
}

/// Which grid the keyboard is in: a pane while the page is bare, and
/// whichever overlay is on top while one is up — topmost first, the order the
/// view stacks them in. arkui's `Context::focus` answers for the menu and the
/// picker; the device picker is harken's own and sits above both.
///
/// It exists because three places once each decided this for themselves, and
/// a menu over the track list left two accent-filled rows on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Pane(Pane),
    Menu,
    Picker,
    Devices,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every place survives the round trip, including the characters a real
    /// library is full of — a slash would look like the separator, a `#`
    /// would end the fragment, an accent is two bytes — and a work's key,
    /// which has a slash of its own.
    ///
    /// Falsified by spelling `Composer` as `works` in `fragment`: it parses
    /// back as the library.
    #[test]
    fn a_place_survives_the_address_bar() {
        for place in [
            Place::Library,
            Place::Albums,
            Place::Artists,
            Place::Composers,
            Place::Playlist("Favorites".into()),
            Place::Album("Water Music".into()),
            Place::Artist("Johann Sebastian Bach".into()),
            Place::Album("Boléro / Pavane #1".into()),
            Place::Composer("Antonio Vivaldi".into()),
            Place::Work("johann-sebastian-bach/bwv-988".into()),
            Place::Recording("johann-sebastian-bach/bwv-988@kimiko-ishizaka".into()),
        ] {
            let fragment = place.fragment();
            assert_eq!(Place::parse(&fragment), place, "round trip of {fragment}");
        }
        assert_eq!(Place::Composer("x".into()).fragment(), "#composer/x");
    }

    /// A fragment nobody wrote lands somewhere rather than nowhere.
    /// Falsified by answering `Albums` for an unknown kind.
    #[test]
    fn a_fragment_that_means_nothing_is_the_library() {
        assert_eq!(Place::parse(""), Place::Library);
        assert_eq!(Place::parse("#"), Place::Library);
        assert_eq!(Place::parse("#library"), Place::Library);
        assert_eq!(Place::parse("#nonsense/Water Music"), Place::Library);
        assert_eq!(Place::parse("#album"), Place::Library, "a kind that needs a name, without one");
        assert_eq!(Place::parse("#album/Bol%"), Place::Album("Bol%".into()));
    }
}
