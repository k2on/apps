//! The ui every app in this repository shares: iced components, the vim
//! keyboard, theming, the glyph table, routing.
//!
//! **Knows nothing about any domain.** No song, album or playlist appears in a
//! type here: an app hands over its palette ([`theme::install`]), its places
//! ([`route::Route`]), its messages (every view is generic over `M`) and its
//! screens, and gets the parts of a window that are the same in every app —
//! the keyboard grammar, the table, the menus and pickers and the rules for who
//! has the keyboard, the card grid, the derived square, a picture cache and
//! the address bar.
//!
//! Most of it is lifted from harken's iced client, where each rule was paid
//! for; the doc comments keep the reasons, because a rule whose reason has
//! been lost is the one that gets "simplified" back into the bug.
//!
//! **Arithmetic, not measurement, is the recurring shape.** iced lays out after
//! `view`, and a panel has to be placed, a card grid walked and a submenu
//! lapped before then — so heights are declared constants given to their
//! containers, widths are estimates that err in the harmless direction, and
//! the keyboard and the view divide the same numbers.

pub mod art;
pub mod cards;
pub mod context;
pub mod fit;
pub mod format;
pub mod glyphs;
pub mod icon;
pub mod images;
pub mod layer;
pub mod menu;
pub mod panel;
pub mod picker;
pub mod route;
pub mod scroll;
pub mod style;
pub mod table;
pub mod theme;
pub mod url;
pub mod vim;

/// iced's default scrollbar width, which it takes out of the content's.
///
/// It has to be in every piece of arithmetic about what fits inside a
/// `scrollable` — a row of cards, a menu hung off a list's right-hand column —
/// because what the content gets is this much narrower than the pane.
pub const SCROLLBAR: f32 = 10.0;
