//! money's colours: the shared neutrals and the green, arkui/theme.toml's
//! `accent.green`, and the three colours a budget is read by, the shared
//! signals: positive for money that has a job, caution for a category short
//! of its target, error for one overspent.
//!
//! Everything else asks [`arkui::theme::of`], as harken's window does, so
//! the system's light or dark is followed without a colour written down
//! anywhere but there.
use arkui::theme::Palette;
use iced::Color;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// money's, for [`arkui::theme::install`] — once, before the first frame.
pub const MONEY: Palette = arkui::tokens::GREEN;

/// The tiles a category's glyph sits on, one picked by its name
/// (`arkui::art::hash`) so it is the same tile everywhere: saturated
/// enough to read on the light page and the dark one alike.
pub const TILES: [Color; 8] = [
    rgb(0x2A, 0xA7, 0xC0),
    rgb(0xF0, 0x56, 0x7A),
    rgb(0xA8, 0x86, 0x5A),
    rgb(0x34, 0xA8, 0x53),
    rgb(0xF0, 0x8A, 0x24),
    rgb(0x8E, 0x6C, 0xEF),
    rgb(0x3B, 0x82, 0xF6),
    rgb(0xD4, 0xA2, 0x1A),
];

/// The glyph on a tile: white, on every one of them.
pub const TILE_GLYPH: Color = Color::WHITE;

/// A card's payment category's tile: every card's the same slate, so the
/// cards' group reads as the cards and not as what money is for.
pub const CARD_TILE: Color = rgb(0x47, 0x55, 0x69);
