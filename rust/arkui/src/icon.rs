//! A glyph from [`crate::glyphs`], drawn at a size and in a colour the theme
//! decides.
//!
//! **Anything outside Latin-1 is a drawing, not a character.** iced's bundled
//! Fira Sans is a text face: it has no U+25B6 play, U+275A pause, U+2665 heart
//! or U+2582 block. A missing glyph lays out fine and draws a `?` or nothing at
//! all, so the button looks *broken* rather than unfontable — which is how a
//! transport once shipped with a `?` on its play button. `«`, `»` and `·` are
//! in the font; `▶`, `❚`, `♥` and `▂` are not.
//!
//! **And not a `canvas` either, once there is one per row.** A canvas inside a
//! `scrollable` is not translated to the row it belongs to under the WebGL
//! renderer: every one in a list draws at very nearly the same place, twenty of
//! them stack into what looks like one stray shape, and scrolling moves the
//! pile rather than the rows. An SVG is positioned by the widget that holds it
//! rather than by geometry in a shared layer, which is exactly the part that
//! was broken.
//!
//! The colour is applied through the `svg` style's colour filter rather than
//! written into the file, so one drawing is right on a plain row, a zebra row
//! and the accent-filled row under the cursor.
use iced::theme::palette::Extended;
use iced::widget::{svg, Svg};
use iced::{Color, Theme};

use crate::glyphs;
use crate::theme::of;

/// How big a glyph is drawn beside a line of 13pt text — a transport button, a
/// panel entry's glyph column, a tick.
pub const SIZE: f32 = 15.0;

/// A glyph at `size`, in whatever `tint` answers from the app's palette.
///
/// The general form; everything below is one of these with a rule for the
/// colour that has already been decided once.
pub fn tinted<'a>(shape: &'static [u8], size: f32, tint: impl Fn(&Extended) -> Color + 'a) -> Svg<'a> {
    svg(svg::Handle::from_memory(shape))
        .width(size)
        .height(size)
        .style(move |theme: &Theme, _| svg::Style {
            color: Some(tint(of(theme))),
        })
}

/// In the text colour, or dimmed: a transport button, where the pair either
/// side of play/pause do the same kind of thing and should not compete with it
/// for the eye.
pub fn plain<'a>(shape: &'static [u8], dim: bool) -> Svg<'a> {
    tinted(shape, SIZE, move |p| match dim {
        true => p.background.base.text.scale_alpha(0.6),
        false => p.background.base.text,
    })
}

/// In the accent — "this one": what is playing, a list this row is on. On the
/// cursor's own row, which is filled with the accent, it takes the one colour
/// that background was paired with.
pub fn accent<'a>(shape: &'static [u8], on_cursor: bool) -> Svg<'a> {
    tinted(shape, SIZE, move |p| match on_cursor {
        true => p.primary.base.text,
        false => p.primary.base.color,
    })
}

/// The tick beside a chosen row. The accent, the same as the title of the row
/// that is playing: both mean "this one", and a second colour for a second
/// kind of yes would be a colour nobody chose.
pub fn tick<'a>(on_cursor: bool) -> Svg<'a> {
    accent(glyphs::TICK, on_cursor)
}

/// A glyph beside a line of text — a sidebar row, a menu entry.
///
/// Dimmer than the words beside it, on both sides of the highlight: the icon is
/// *which kind of thing this row is*, and a shape as loud as the name would
/// compete with every name for the same glance.
pub fn line<'a>(shape: &'static [u8], lit: bool) -> Svg<'a> {
    tinted(shape, SIZE, move |p| match lit {
        true => p.primary.base.text.scale_alpha(0.9),
        false => p.background.base.text.scale_alpha(0.6),
    })
}

/// Something drawn on every row, faintly: on every row, a shape as loud as the
/// title is a shape competing with a hundred titles.
pub fn faint<'a>(shape: &'static [u8], on_cursor: bool) -> Svg<'a> {
    tinted(shape, SIZE, move |p| match on_cursor {
        true => p.primary.base.text.scale_alpha(0.8),
        false => p.background.base.text.scale_alpha(0.45),
    })
}

/// The three dots that open a row's menu.
pub fn more<'a>(on_cursor: bool) -> Svg<'a> {
    faint(glyphs::MORE, on_cursor)
}

/// The mark on a menu entry that has more behind it.
///
/// What a submenu looks like everywhere, and the thing an ellipsis could not
/// say: `Add to playlist…` and `Rename…` are the same three dots, and one opens
/// a panel *beside* the entry while the other replaces what is under it. Dimmer
/// than the label: it says *how* the entry behaves, not what it does.
pub fn chevron<'a>(lit: bool) -> Svg<'a> {
    tinted(glyphs::CHEVRON, SIZE, move |p| match lit {
        true => p.primary.base.text.scale_alpha(0.8),
        false => p.background.base.text.scale_alpha(0.55),
    })
}
