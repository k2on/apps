//! The ui every app in this repository shares: iced components, the vim
//! keyboard, theming, the glyph table, routing.
//!
//! **Knows nothing about any domain.** No song, album or playlist appears in a
//! type here: an app hands over its palette ([`theme::install`]), its places
//! ([`router::Routable`]), its messages (every view is generic over `M`) and its
//! screens, and gets the parts of a window that are the same in every app —
//! the keyboard grammar, the table, the menus and pickers and the rules for who
//! has the keyboard, the card grid, the derived square, a picture cache, the
//! address bar, and what wakes a window that runs no loop ([`wake`]).
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

pub mod account;
pub mod art;
pub mod button;
pub mod cards;
mod clipboard;
pub mod command;
pub mod compact;
pub mod context;
pub mod dialog;
pub mod edge;
pub mod files;
pub mod fit;
pub mod fonts;
pub mod format;
pub mod glyphs;
pub mod hero;
pub mod history;
pub mod icon;
pub mod images;
pub mod later;
pub mod layer;
pub mod line;
pub mod menu;
pub mod motion;
pub mod motion_tokens;
pub mod navigator;
pub mod outline;
pub mod panel;
pub mod parts;
pub mod picker;
pub mod qr;
pub mod renderer;
pub mod reorder;
pub mod router;
pub mod scroll;
pub mod sheet;
pub mod sidebar;
pub mod style;
pub mod swipe;
pub mod table;
pub mod theme;
pub mod toast;
pub mod tokens;
pub mod touchpad;
/// Percent-encoding, which is `ark-router`'s: a domain spells its routes with it.
pub use ark_router::url;
pub mod velocity;
pub mod vim;
pub mod wake;
pub mod which;
pub mod window;

/// iced's default scrollbar width, which it takes out of the content's.
///
/// It has to be in every piece of arithmetic about what fits inside a
/// `scrollable` — a row of cards, a menu hung off a list's right-hand column —
/// because what the content gets is this much narrower than the pane.
pub const SCROLLBAR: f32 = 10.0;
