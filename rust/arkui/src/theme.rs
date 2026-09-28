//! What a colour *is*: the app's palette, answered for whichever theme iced
//! picked.
//!
//! iced picks Light or Dark from the system and hands every style closure the
//! theme it picked. [`of`] reads which of the two that was and answers with
//! the app's colours rather than iced's. So the rule that makes dark mode work
//! is one rule: **nothing writes a colour down, it asks.** Every style in this
//! crate asks here, and an app's own closures should too.
//!
//! **iced still picks the theme; the app only picks the colours.** A fully
//! custom `iced::Theme` is what you would reach for, and it cannot follow the
//! system: iced resolves the preference internally and surfaces it only by
//! handing the chosen theme to each style closure. The cost is that iced's
//! *own* widget defaults — a bare `slider`, `button::text` — keep iced's
//! colours, which is why [`crate::style`] styles every widget this crate draws.
//!
//! The palette is installed once, at startup, with [`install`], because a style
//! closure is `Fn(&Theme)` and has nowhere else to find it. Before anything is
//! installed — and in every test — the answer is [`Palette::NEUTRAL`], which is
//! the only colour this crate owns and is documented as a default rather than
//! a brand. harken's gold stays in harken.
use iced::theme::palette::Extended;
use iced::{Color, Theme};
use std::sync::OnceLock;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// An app's colours: one iced palette per theme, and the gradient pairs the
/// derived square ([`crate::art`]) picks from.
///
/// The same four things harken's generated `palette.rs` held — `LIGHT`,
/// `DARK`, `LIGHT_ART` and `DARK_ART` — as one value an app hands over once.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub light: iced::theme::Palette,
    pub dark: iced::theme::Palette,
    /// Stand-in artwork against white: the two stops of each gradient. It has
    /// to stay dark enough that the note drawn on the square reads.
    pub light_art: &'static [[Color; 2]],
    /// …and against the dark page.
    pub dark_art: &'static [[Color; 2]],
}

impl Palette {
    /// What is drawn when an app installs nothing: AppKit's neutral greys —
    /// `controlBackgroundColor` and `labelColor`, flattened over their plane —
    /// and its default `controlAccentColor`, the system blue. Deliberately
    /// nobody's brand: the accent is the one slot an app is expected to fill.
    pub const NEUTRAL: Palette = Palette {
        light: iced::theme::Palette {
            background: rgb(0xFF, 0xFF, 0xFF),
            text: rgb(0x26, 0x26, 0x26),
            primary: rgb(0x00, 0x6A, 0xDC),
            success: rgb(0x28, 0xCD, 0x41),
            warning: rgb(0xB2, 0x6B, 0x00),
            danger: rgb(0xFF, 0x3B, 0x30),
        },
        dark: iced::theme::Palette {
            background: rgb(0x1E, 0x1E, 0x1E),
            text: rgb(0xDD, 0xDD, 0xDD),
            primary: rgb(0x0A, 0x84, 0xFF),
            success: rgb(0x32, 0xD7, 0x4B),
            warning: rgb(0xFF, 0x9F, 0x0A),
            danger: rgb(0xFF, 0x45, 0x3A),
        },
        light_art: &[
            [rgb(0xE4, 0xE4, 0xE6), rgb(0xBC, 0xBC, 0xC0)],
            [rgb(0xDD, 0xE2, 0xEA), rgb(0xAE, 0xB8, 0xC6)],
            [rgb(0xE6, 0xE1, 0xDC), rgb(0xC0, 0xB6, 0xAC)],
        ],
        dark_art: &[
            [rgb(0x5A, 0x5A, 0x5E), rgb(0x18, 0x18, 0x1A)],
            [rgb(0x4E, 0x58, 0x66), rgb(0x14, 0x17, 0x1B)],
            [rgb(0x60, 0x58, 0x50), rgb(0x1A, 0x17, 0x14)],
        ],
    };

    /// The iced palette for one side.
    pub fn colors(&self, dark: bool) -> iced::theme::Palette {
        match dark {
            true => self.dark,
            false => self.light,
        }
    }

    /// The gradient pairs for one side.
    pub fn art(&self, dark: bool) -> &'static [[Color; 2]] {
        match dark {
            true => self.dark_art,
            false => self.light_art,
        }
    }
}

/// A palette with both sides already expanded.
///
/// Generated once each rather than per widget per frame: `Extended` is
/// forty-odd colours derived from six, and deriving them at sixty hertz would
/// be forty-odd divisions nobody asked for.
struct Expanded {
    palette: Palette,
    light: Extended,
    dark: Extended,
}

impl Expanded {
    fn new(palette: Palette) -> Expanded {
        Expanded {
            palette,
            light: Extended::generate(palette.light),
            dark: Extended::generate(palette.dark),
        }
    }
}

static INSTALLED: OnceLock<Expanded> = OnceLock::new();
static NEUTRAL: OnceLock<Expanded> = OnceLock::new();

fn current() -> &'static Expanded {
    INSTALLED.get().unwrap_or_else(|| NEUTRAL.get_or_init(|| Expanded::new(Palette::NEUTRAL)))
}

/// Make `palette` the answer for the rest of the process.
///
/// Once, before the first frame: a style closure has nowhere to be handed the
/// palette, so it is a process-wide fact. Returns `false` if one was already
/// installed, which is then the one that stays — two palettes in one window
/// would be two answers to "what colour is this".
pub fn install(palette: Palette) -> bool {
    INSTALLED.set(Expanded::new(palette)).is_ok()
}

/// The installed palette, or [`Palette::NEUTRAL`].
pub fn palette() -> &'static Palette {
    &current().palette
}

/// Whether iced picked the dark side. Asked of iced's own palette because that
/// is where iced records what the system said.
pub fn is_dark(theme: &Theme) -> bool {
    theme.extended_palette().is_dark
}

/// The app's colours for whichever of the two themes iced picked.
pub fn of(theme: &Theme) -> &'static Extended {
    let expanded = current();
    match is_dark(theme) {
        true => &expanded.dark,
        false => &expanded.light,
    }
}

/// The gradient pairs the derived square picks from, for this theme.
pub fn art(theme: &Theme) -> &'static [[Color; 2]] {
    current().palette.art(is_dark(theme))
}
