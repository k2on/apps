//! money's colours: a deep indigo for what is lit, and the three colours a
//! budget is read by — green for money that has a job, amber for a
//! category short of its target, red for one overspent.
//!
//! Everything else asks [`arkui::theme::of`], as harken's window does, so
//! the system's light or dark is followed without a colour written down
//! anywhere but here.
use arkui::theme::Palette;
use iced::Color;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// Against a deep navy: the accent lifted so it reads as a light.
pub const DARK: iced::theme::Palette = iced::theme::Palette {
    background: rgb(0x16, 0x18, 0x28),
    text: rgb(0xE3, 0xE5, 0xF0),
    primary: rgb(0x8B, 0x97, 0xFF),
    success: rgb(0x6F, 0xD0, 0x8C),
    warning: rgb(0xF0, 0xC0, 0x5A),
    danger: rgb(0xFF, 0x7B, 0x7B),
};

/// Against white: the accent dark enough that white text on it reads.
pub const LIGHT: iced::theme::Palette = iced::theme::Palette {
    background: rgb(0xFF, 0xFF, 0xFF),
    text: rgb(0x22, 0x25, 0x3A),
    primary: rgb(0x3A, 0x4C, 0xC6),
    success: rgb(0x25, 0x8A, 0x45),
    warning: rgb(0xA8, 0x6A, 0x00),
    danger: rgb(0xD0, 0x36, 0x36),
};

pub const DARK_ART: [[Color; 2]; 3] = [
    [rgb(0x3D, 0x46, 0x9A), rgb(0x16, 0x18, 0x28)],
    [rgb(0x2E, 0x6B, 0x4A), rgb(0x12, 0x1E, 0x18)],
    [rgb(0x7A, 0x5A, 0x1E), rgb(0x20, 0x18, 0x09)],
];

pub const LIGHT_ART: [[Color; 2]; 3] = [
    [rgb(0xD9, 0xDD, 0xFA), rgb(0xA9, 0xB2, 0xF0)],
    [rgb(0xCF, 0xEF, 0xD9), rgb(0x9C, 0xD6, 0xAE)],
    [rgb(0xF6, 0xE4, 0xBF), rgb(0xE0, 0xC0, 0x7E)],
];

/// money's, for [`arkui::theme::install`] — once, before the first frame.
pub const MONEY: Palette = Palette {
    light: LIGHT,
    dark: DARK,
    light_art: &LIGHT_ART,
    dark_art: &DARK_ART,
};
