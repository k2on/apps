//! The square that stands in for a picture, derived from a name.
//!
//! Most things have no picture, and this is what is drawn for them: a square
//! derived from the name, which is most of what a cover is doing in a list and
//! the part that survives having no picture. It is the fallback —
//! [`picture`] draws the real one when [`crate::images::Images`] has it.
//!
//! **Deterministic, and the same on every client.** A phone that draws the same
//! square has to agree about which of the gradients a name picks, or one album
//! is two squares depending on which screen you look at. So the hash is
//! specified down to the width of what it walks, and the gradients are the
//! app's ([`crate::theme::Palette`]'s `light_art` and `dark_art`), generated
//! for every client from one description.
use iced::widget::{image, svg};
use iced::{Color, ContentFit, Element, Length, Theme};

use crate::theme::{self, Palette};

/// FNV-1a, over UTF-16 code units.
///
/// The width is the load-bearing part. JavaScript's `charCodeAt` yields UTF-16
/// code units, so a name outside the Basic Multilingual Plane is two units
/// there and would be one `char` here — `encode_utf16` keeps them the same.
/// `wrapping_mul` because `Math.imul` wraps at 32 bits where Rust would panic
/// in debug.
pub fn hash(text: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for unit in text.encode_utf16() {
        h ^= u32::from(unit);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The two stops `seed` draws as, in whichever theme iced picked.
pub fn pair(theme: &Theme, seed: &str) -> [Color; 2] {
    let mut set = theme::art(theme);
    if set.is_empty() {
        set = Palette::NEUTRAL.art(theme::is_dark(theme));
    }
    set[hash(seed) as usize % set.len()]
}

/// The square itself: a rounded rectangle in the name's own colour, `size`
/// across with corners of `corner` (half of `size` is a circle — a person).
///
/// **An SVG tinted by the style closure, not a `container` with a gradient
/// background.** The container was harken's first version and it drew nothing
/// at all: it laid out at the right size and the background simply never
/// painted. An image widget positions itself from its own bounds and nothing
/// else.
///
/// The consequence is that the *colour* cannot be in the file: `view` never
/// sees the theme, and `svg`'s filter replaces every colour in the drawing
/// with one. So the depth is in *alpha* — the gradient runs from the tint at
/// full strength to the tint at a third, which survives the filter because
/// the filter replaces colour and leaves opacity alone.
pub fn square<'a, M: 'a>(seed: &str, size: f32, corner: f32) -> Element<'a, M> {
    let radius = (corner / size * 100.0).clamp(0.0, 50.0);
    let seed = seed.to_string();
    svg(svg::Handle::from_memory(drawing(radius).into_bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |theme: &Theme, _| svg::Style {
            // The first stop of the pair: the end the square should read as.
            color: Some(pair(theme, &seed)[0]),
        })
        .into()
}

/// The picture if one has arrived, and the derived square until then.
///
/// One function, because every caller wants the same fallback and a caller
/// that forgot it would draw a hole. The square is not a placeholder to be
/// replaced by a spinner: it is right on its own, so a picture that never
/// comes costs nothing and one that does simply appears. Cropped to the square
/// rather than letterboxed — a photograph of a person is not square, and a
/// grid with grey bars down the sides of half its cards looks broken.
pub fn picture<'a, M: 'a>(seed: &str, handle: Option<&image::Handle>, side: f32, corner: f32) -> Element<'a, M> {
    match handle {
        Some(handle) => image(handle.clone())
            .width(Length::Fixed(side))
            .height(Length::Fixed(side))
            .content_fit(ContentFit::Cover)
            .border_radius(corner)
            .into(),
        None => square(seed, side, corner),
    }
}

/// The SVG source for a square of one corner radius, in a 100-unit box the
/// widget scales. Every colour is a placeholder the style filter replaces —
/// white, because white is what says "I was replaced" most loudly if the
/// filter ever stops being applied.
///
/// Mind the raw string: every colour in an SVG is written `fill="#…"`, and
/// `"#` ends an `r#"…"#` literal — which fails pointing inside the string and
/// reads like a `format!` problem. So `r##"…"##`.
fn drawing(radius: f32) -> String {
    format!(
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">"##,
            r##"<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">"##,
            r##"<stop offset="0" stop-color="#FFFFFF" stop-opacity="1"/>"##,
            r##"<stop offset="1" stop-color="#FFFFFF" stop-opacity="0.33"/>"##,
            r##"</linearGradient></defs>"##,
            r##"<rect width="100" height="100" rx="{radius}" ry="{radius}" fill="url(#g)"/>"##,
            // A note, knocked back rather than drawn in a second colour. It is
            // drawn, not typed: U+266A is not in the font, and U+00B7 is a
            // four-pixel dot at any size.
            r##"<g fill="#FFFFFF" fill-opacity="0.35">"##,
            r##"<circle cx="44" cy="62" r="8"/><rect x="50" y="26" width="4" height="36"/>"##,
            r##"<path d="M50 26 L68 32 L68 40 L50 34 Z"/>"##,
            "</g></svg>"
        ),
        radius = radius,
    )
}

#[cfg(test)]
mod tests {
    use super::hash;

    /// What harken's phone answers, with `Math.imul` over `charCodeAt`. If
    /// either side's hash moves, one album becomes two different squares
    /// depending on which screen draws it — invisibly. Falsified by starting
    /// from a different offset basis: every line fails.
    #[test]
    fn matches_the_phone() {
        assert_eq!(hash(""), 0x811c_9dc5);
        assert_eq!(hash("a"), 0xe40c_292c);
        assert_eq!(hash("Water Music"), 0x7441_7273);
        assert_eq!(hash("F\u{fc}r Elise"), 0x939e_323b);
        assert_eq!(hash("Messiah"), 0x4602_085d);
    }

    /// The one case where a `char` loop and a UTF-16 loop disagree. Everything
    /// above is in the Basic Multilingual Plane, where one `char` is one code
    /// unit and the two walks are the same function — so none of it would
    /// catch iterating `chars()`. U+1F3B5 is one `char` and the surrogate pair
    /// `D83C DFB5` to JavaScript, and the two walks give `0x442e75ca` and
    /// `0xb154da50`. Falsified by walking `chars()`: this fails, the rest pass.
    #[test]
    fn walks_utf16_code_units() {
        assert_eq!(hash("\u{1F3B5}"), 0x442e_75ca);
    }
}
