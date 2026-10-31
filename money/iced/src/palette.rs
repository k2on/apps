//! money's colours: harken's — AppKit's greys — with a dark green where
//! harken has its gold, and the three colours a budget is read by: green
//! for money that has a job, amber for a category short of its target, red
//! for one overspent.
//!
//! Everything else asks [`arkui::theme::of`], as harken's window does, so
//! the system's light or dark is followed without a colour written down
//! anywhere but here.
//!
//! The accent is not one colour, for harken's reason: a dark page can take
//! a lifted green, a white one needs it deep enough that the white text on
//! a lit row reads.
use arkui::theme::Palette;
use iced::Color;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// Black, white text, green: harken's `DARK` with the gold turned green.
/// harken's amber is its gold; money's is AppKit's, so "short" is not
/// the accent.
pub const DARK: iced::theme::Palette = iced::theme::Palette {
    background: rgb(0x1E, 0x1E, 0x1E),
    text: rgb(0xDD, 0xDD, 0xDD),
    primary: rgb(0x2F, 0x7D, 0x4F),
    success: rgb(0x32, 0xD7, 0x4B),
    warning: rgb(0xFF, 0x9F, 0x0A),
    danger: rgb(0xFF, 0x45, 0x3A),
};

/// …and the same against white, the green deeper.
pub const LIGHT: iced::theme::Palette = iced::theme::Palette {
    background: rgb(0xFF, 0xFF, 0xFF),
    text: rgb(0x26, 0x26, 0x26),
    primary: rgb(0x1B, 0x5E, 0x3B),
    success: rgb(0x28, 0xCD, 0x41),
    warning: rgb(0xB2, 0x6B, 0x00),
    danger: rgb(0xFF, 0x3B, 0x30),
};

pub const DARK_ART: [[Color; 2]; 3] = [
    [rgb(0x2F, 0x6B, 0x47), rgb(0x10, 0x1C, 0x15)],
    [rgb(0x3E, 0x5E, 0x4A), rgb(0x14, 0x1A, 0x16)],
    [rgb(0x5A, 0x5A, 0x5E), rgb(0x18, 0x18, 0x1A)],
];

pub const LIGHT_ART: [[Color; 2]; 3] = [
    [rgb(0xCF, 0xE6, 0xD6), rgb(0x9C, 0xC7, 0xAC)],
    [rgb(0xDC, 0xE6, 0xDF), rgb(0xB2, 0xC4, 0xB8)],
    [rgb(0xE4, 0xE4, 0xE6), rgb(0xBC, 0xBC, 0xC0)],
];

/// money's, for [`arkui::theme::install`] — once, before the first frame.
pub const MONEY: Palette = Palette {
    light: LIGHT,
    dark: DARK,
    light_art: &LIGHT_ART,
    dark_art: &DARK_ART,
};

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
