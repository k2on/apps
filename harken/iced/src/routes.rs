//! Where the window can be: what the page shows, which is harken's
//! [`Route`] (`harken_domain::routes`, spelled in the address bar by its
//! path), the drawing beside each ([`Glyph`]), which pane has the cursor
//! ([`Pane`]), and which grid has the keyboard ([`Focus`]).
use arkui::glyphs;
use arkui::vim;

pub use harken_domain::routes::Route;

/// The drawing that goes beside a route — in the sidebar, and on a menu's
/// "Go to" entry, which asks the route it goes to rather than naming a glyph
/// itself, so the two cannot come to disagree about what an album looks like.
pub trait Glyph {
    fn glyph(&self) -> &'static [u8];
}

impl Glyph for Route {
    fn glyph(&self) -> &'static [u8] {
        match self {
            Route::Library => glyphs::NOTE,
            Route::Albums | Route::Album { .. } => glyphs::ALBUM,
            Route::Artists | Route::Artist { .. } => glyphs::ARTIST,
            Route::Composers => glyphs::COMPOSER,
            Route::Work { .. } | Route::Recording { .. } | Route::Settings | Route::Diagnostics | Route::Join { .. } => glyphs::LIBRARY,
            Route::Sermons => glyphs::PULPIT,
            Route::Show { key, .. } if key.starts_with("sermon/") => glyphs::PULPIT,
            Route::Podcasts | Route::Show { .. } => glyphs::SHOW,
            Route::Playlist { .. } => glyphs::PLAYLIST,
            Route::Transcript => glyphs::TRANSCRIPT,
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
    Queue,
    /// The dialog a shows page's + opens.
    Adding,
}
