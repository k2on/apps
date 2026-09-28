//! harken's colours: AppKit's greys and the gold.
//!
//! What was `branding/nix/palette.nix`, generated into the old client's
//! `palette.rs`, as the one value [`arkui::theme::install`] takes. Everything
//! else asks [`arkui::theme::of`] — nothing in this crate writes a colour
//! down, which is the whole reason dark mode works: iced picks Light or Dark
//! from the system and hands every style closure the one it picked, and `of`
//! answers with ours.
//!
//! The gold is not one colour. A dark sheet can take a bright leaf gold; a
//! white one cannot, because the text on a lit row has to be legible *on*
//! it. That asymmetry is the only one.
use arkui::theme::Palette;
use iced::Color;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// Black, white text, gold — `controlBackgroundColor`, `labelColor`
/// flattened over it, and the accent where `controlAccentColor` would be.
pub const DARK: iced::theme::Palette = iced::theme::Palette {
    background: rgb(0x1E, 0x1E, 0x1E),
    text: rgb(0xDD, 0xDD, 0xDD),
    primary: rgb(0xE9, 0xBB, 0x45),
    success: rgb(0x32, 0xD7, 0x4B),
    warning: rgb(0xE9, 0xBB, 0x45),
    danger: rgb(0xFF, 0x45, 0x3A),
};

/// …and the same against white. The gold is darker here because text has to
/// be legible on it.
pub const LIGHT: iced::theme::Palette = iced::theme::Palette {
    background: rgb(0xFF, 0xFF, 0xFF),
    text: rgb(0x26, 0x26, 0x26),
    primary: rgb(0x9A, 0x74, 0x1A),
    success: rgb(0x28, 0xCD, 0x41),
    warning: rgb(0x9A, 0x74, 0x1A),
    danger: rgb(0xFF, 0x3B, 0x30),
};

/// The derived square's gradients against the dark page, as its two stops.
/// Six, and the phone has the same six: which one a name picks is
/// `arkui::art::hash`, FNV-1a over UTF-16 so both clients agree.
pub const DARK_ART: [[Color; 2]; 6] = [
    [rgb(0x7A, 0x5C, 0x15), rgb(0x20, 0x19, 0x07)],
    [rgb(0x8A, 0x6B, 0x22), rgb(0x1B, 0x15, 0x09)],
    [rgb(0x6B, 0x5A, 0x2A), rgb(0x17, 0x14, 0x0B)],
    [rgb(0x8F, 0x73, 0x27), rgb(0x22, 0x1B, 0x09)],
    [rgb(0x5E, 0x4A, 0x18), rgb(0x14, 0x10, 0x07)],
    [rgb(0xA0, 0x81, 0x37), rgb(0x26, 0x1E, 0x08)],
];

/// …and against white, where the pair has to stay dark enough that the note
/// drawn on it reads.
pub const LIGHT_ART: [[Color; 2]; 6] = [
    [rgb(0xE8, 0xD1, 0x9A), rgb(0xC9, 0xA8, 0x5F)],
    [rgb(0xEF, 0xDC, 0xAE), rgb(0xD2, 0xB3, 0x70)],
    [rgb(0xE2, 0xD2, 0xAC), rgb(0xBF, 0xA6, 0x71)],
    [rgb(0xF0, 0xDF, 0xA8), rgb(0xCB, 0xAA, 0x5C)],
    [rgb(0xE6, 0xD0, 0xA0), rgb(0xC3, 0xA0, 0x5A)],
    [rgb(0xF3, 0xE6, 0xBE), rgb(0xD6, 0xB8, 0x77)],
];

/// harken's, for [`arkui::theme::install`] — once, before the first frame.
pub const HARKEN: Palette = Palette {
    light: LIGHT,
    dark: DARK,
    light_art: &LIGHT_ART,
    dark_art: &DARK_ART,
};
