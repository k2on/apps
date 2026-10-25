//! A page about somebody, the way a record shop's catalogue opens one: their
//! picture across the whole page, their name set on it, and the page under it
//! in sections, each under a serif heading.
//!
//! **The picture is a backdrop, not a portrait.** It is drawn wide and
//! cropped to fill ([`ContentFit::Cover`]), at the shape a painting of
//! somebody is cut to for a banner (2400×933), so the name sits on the
//! picture's lower edge over a shade that keeps it legible on a white collar
//! or a white wig. Somebody with no wide picture has no banner: a portrait
//! stretched across a page is a close-up of a nose.
//!
//! **The serif is the catalogue's voice.** Names and section headings are set
//! in Source Serif; everything else stays in the interface's sans. It is
//! embedded ([`SERIF_BYTES`], loaded by the app), because a browser build has
//! no system fonts to ask.
use iced::font::{Family, Weight};
use iced::widget::{column, container, image, mouse_area, row, rule, stack, text, Space};
use iced::{Alignment, Background, Color, ContentFit, Element, Font, Length, Padding};

use crate::{style, theme};

/// Source Serif 4 Display, semibold: the file an app loads at start.
pub const SERIF_BYTES: &[u8] = include_bytes!("../fonts/SourceSerif4Display-Semibold.otf");

/// The headings' face, once [`SERIF_BYTES`] is loaded.
pub const SERIF: Font = Font {
    family: Family::Name("Source Serif 4 Display"),
    weight: Weight::Semibold,
    ..Font::DEFAULT
};

/// A banner's shape: width over height.
pub const ASPECT: f32 = 2400.0 / 933.0;

/// How tall a banner `width` wide is drawn: its shape, within reason — a
/// phone-width page would make a strip, a wide monitor a wall.
pub fn height(width: f32) -> f32 {
    (width / ASPECT).clamp(200.0, 380.0)
}

/// The banner: `picture` across `width`, `kind` in small capitals and `name`
/// in the serif on its lower edge. Until the picture arrives the ground is
/// drawn instead, so the name is already where it will be.
pub fn banner<'a, M: 'a>(picture: Option<&image::Handle>, kind: &str, name: &str, width: f32) -> Element<'a, M> {
    let h = height(width);
    let ground: Element<'a, M> = match picture {
        Some(p) => image(p.clone())
            .content_fit(ContentFit::Cover)
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .border_radius(8.0)
            .into(),
        None => container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .style(|t| container::Style {
                background: Some(Background::Color(theme::of(t).background.strong.color)),
                border: iced::border::rounded(8.0),
                ..container::Style::default()
            })
            .into(),
    };
    // Clear at the top, darkest at the foot, where the name is. A picture,
    // not a gradient fill: iced draws a layer's pictures over its fills, so a
    // fill laid on a picture is under it.
    let shade = image(shade())
        .content_fit(ContentFit::Fill)
        .width(Length::Fill)
        .height(Length::Fixed(h))
        .border_radius(8.0);
    let white = |a: f32| {
        move |_: &iced::Theme| text::Style {
            color: Some(Color::from_rgba(1.0, 1.0, 1.0, a)),
        }
    };
    let words = container(
        column![
            text(kind.to_uppercase()).size(11).style(white(0.85)),
            text(name.to_string()).size(34).font(SERIF).style(white(1.0)),
        ]
        .spacing(2),
    )
    .width(Length::Fill)
    .height(Length::Fixed(h))
    .padding(Padding {
        top: 0.0,
        right: 24.0,
        bottom: 20.0,
        left: 24.0,
    })
    .align_y(Alignment::End);
    stack![ground, shade, words].width(Length::Fill).into()
}

/// The shade a banner's name sits on: one column of pixels, clear down to
/// 45% and then darkening to 60% black at the foot, stretched across.
fn shade() -> image::Handle {
    static SHADE: std::sync::OnceLock<image::Handle> = std::sync::OnceLock::new();
    SHADE
        .get_or_init(|| {
            const ROWS: u32 = 128;
            let pixels = (0..ROWS)
                .flat_map(|y| {
                    let at = y as f32 / (ROWS - 1) as f32;
                    let alpha = ((at - 0.45) / 0.55).clamp(0.0, 1.0) * 0.6;
                    [0, 0, 0, (alpha * 255.0) as u8]
                })
                .collect::<Vec<u8>>();
            image::Handle::from_rgba(1, ROWS, pixels)
        })
        .clone()
}

/// A section's heading, in the serif.
pub fn heading<'a, M: 'a>(title: &str) -> Element<'a, M> {
    container(text(title.to_string()).size(20).font(SERIF))
        .padding(Padding {
            top: 28.0,
            right: 0.0,
            bottom: 10.0,
            left: 0.0,
        })
        .into()
}

/// One entry of a [`listing`]: a line of small capitals over it (who it is
/// by), its title, a line under, a count at its right, and what opening it
/// does.
#[derive(Debug, Clone)]
pub struct Entry<M> {
    pub over: String,
    pub title: String,
    pub under: String,
    pub count: String,
    pub open: M,
}

/// The narrowest a [`listing`] column is drawn.
pub const COLUMN: f32 = 280.0;

/// How many columns a listing `room` wide has: as many as fit, and no more
/// than three, past which a line is too short to hold a title.
pub fn columns(room: f32) -> usize {
    ((room / COLUMN) as usize).clamp(1, 3)
}

/// Entries in `columns` columns, read across, each over a hairline — a
/// catalogue's list of works. The cursor's entry has its title in the accent,
/// as a card's does.
pub fn listing<'a, M: Clone + 'a>(entries: &[Entry<M>], columns: usize, cursor: Option<usize>, on_hover: impl Fn(usize) -> M) -> Element<'a, M> {
    let columns = columns.max(1);
    let mut page = column![].spacing(0);
    for (r, chunk) in entries.chunks(columns).enumerate() {
        let mut line = row![].spacing(24).align_y(Alignment::Start);
        for (c, e) in chunk.iter().enumerate() {
            let at = r * columns + c;
            line = line.push(entry(e, Some(at) == cursor, on_hover(at)));
        }
        // A short last row keeps its columns where the full ones have them.
        for _ in chunk.len()..columns {
            line = line.push(Space::new().width(Length::Fill));
        }
        page = page.push(line);
    }
    page.width(Length::Fill).into()
}

fn entry<'a, M: Clone + 'a>(e: &Entry<M>, on_cursor: bool, hover: M) -> Element<'a, M> {
    let title = text(e.title.clone()).size(14).style(move |t| text::Style {
        color: Some(match on_cursor {
            true => theme::of(t).primary.base.color,
            false => theme::of(t).background.base.text,
        }),
    });
    let words = column![
        text(e.over.to_uppercase()).size(10).style(style::faint(0.5)),
        title,
        text(e.under.clone()).size(11).style(style::faint(0.5)),
    ]
    .spacing(2)
    .width(Length::Fill);
    let body = row![
        words,
        text(e.count.clone()).size(12).style(style::faint(0.45)),
        text("\u{203A}").size(18).style(style::faint(0.45)),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    mouse_area(
        column![
            container(body).padding(Padding {
                top: 10.0,
                right: 4.0,
                bottom: 10.0,
                left: 0.0
            }),
            rule::horizontal(1)
        ]
        .width(Length::Fill),
    )
    .on_enter(hover)
    .on_press(e.open.clone())
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A listing is never so narrow a title has no room, nor wider than three.
    #[test]
    fn a_listing_is_one_to_three_columns() {
        assert_eq!(columns(0.0), 1);
        assert_eq!(columns(COLUMN * 2.0), 2);
        assert_eq!(columns(5000.0), 3);
    }

    /// A banner keeps its shape where it can and is never a strip or a wall.
    #[test]
    fn a_banner_keeps_its_shape_within_reason() {
        assert_eq!(height(ASPECT * 300.0), 300.0);
        assert_eq!(height(100.0), 200.0);
        assert_eq!(height(5000.0), 380.0);
    }
}
