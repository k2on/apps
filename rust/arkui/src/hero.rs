//! A page about somebody, the way a record shop's catalogue opens one: their
//! picture across the whole page, their name set on it, and the page under it
//! in sections, each under a heading.
//!
//! **The picture is a backdrop, not a portrait.** It is drawn wide and
//! cropped to fill ([`ContentFit::Cover`]), at the shape a painting of
//! somebody is cut to for a banner (2400×933), so the name sits on the
//! picture's lower edge over a shade that keeps it legible on a white collar
//! or a white wig. Somebody with no wide picture has no banner: a portrait
//! stretched across a page is a close-up of a nose.
//!
//! **One face, two weights.** Names and section headings are the interface's
//! own sans at [`BOLD`] and a size up; a second family beside it was one more
//! voice on a page that only needs the one.
use iced::font::Weight;
use iced::widget::{column, container, image, mouse_area, row, rule, stack, text, Space};
use iced::{Alignment, Background, Color, ContentFit, Element, Font, Length, Padding};

use crate::{style, theme};

/// A name or a heading: the interface's face, bold.
///
/// On the desktop that is the system's sans-serif, bold. In a browser there
/// are no system fonts and iced draws everything in the Fira Sans it embeds,
/// which is Regular alone — so [`FIRA_SANS_BOLD`] is loaded beside it, and
/// this names the family outright: a generic sans-serif there matches no
/// family at all.
pub const BOLD: Font = Font {
    #[cfg(target_arch = "wasm32")]
    family: iced::font::Family::Name("Fira Sans"),
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// Fira Sans Bold 4.203 (SIL OFL 1.1, `fonts/LICENSE.fira-sans.txt`), from
/// google/fonts — the same release as the Regular iced embeds. An app hands
/// it to `iced::application(...).font(...)` so [`BOLD`] has a face to draw
/// with in a browser; on the desktop it is a fallback.
pub const FIRA_SANS_BOLD: &[u8] = include_bytes!("../fonts/FiraSans-Bold.ttf");

/// A banner's shape: width over height.
pub const ASPECT: f32 = 2400.0 / 933.0;

/// How tall a banner `width` wide is drawn: its shape, within reason — a
/// phone-width page would make a strip, and past [`TALLEST`] a wide monitor
/// would make a wall, so it is cropped from there.
pub fn height(width: f32) -> f32 {
    (width / ASPECT).clamp(200.0, TALLEST)
}

/// The tallest a banner is drawn: its whole shape up to a page about 1250
/// wide.
pub const TALLEST: f32 = 486.0;

/// The banner: `picture` edge to edge across `width`, square-cornered as a
/// catalogue's is, `kind` in small capitals and `name` in bold on its
/// lower edge. Until the picture arrives the ground is
/// drawn instead, so the name is already where it will be.
pub fn banner<'a, M: 'a>(picture: Option<&image::Handle>, kind: &str, name: &str, width: f32) -> Element<'a, M> {
    let h = height(width);
    let ground: Element<'a, M> = match picture {
        Some(p) => image(p.clone())
            .content_fit(ContentFit::Cover)
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .into(),
        None => container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .style(|t| container::Style {
                background: Some(Background::Color(theme::of(t).background.strong.color)),
                ..container::Style::default()
            })
            .into(),
    };
    // Clear at the top, darkest at the foot, where the name is. A picture,
    // not a gradient fill: iced draws a layer's pictures over its fills, so a
    // fill laid on a picture is under it.
    let shade = image(shade()).content_fit(ContentFit::Fill).width(Length::Fill).height(Length::Fixed(h));
    let white = |a: f32| {
        move |_: &iced::Theme| text::Style {
            color: Some(Color::from_rgba(1.0, 1.0, 1.0, a)),
        }
    };
    let words = container(
        column![
            text(kind.to_uppercase()).size(12).style(white(0.85)),
            text(name.to_string()).size(40).font(BOLD).style(white(1.0)),
        ]
        .spacing(2),
    )
    .width(Length::Fill)
    .height(Length::Fixed(h))
    .padding(Padding {
        top: 0.0,
        right: 32.0,
        bottom: 28.0,
        left: 32.0,
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

/// A section's heading, in bold.
pub fn heading<'a, M: 'a>(title: &str) -> Element<'a, M> {
    container(text(title.to_string()).size(20).font(BOLD))
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
    let drawn = entries.iter().enumerate().map(|(at, e)| entry(None, e, Some(at) == cursor, on_hover(at)));
    grid(drawn.collect(), columns)
}

/// Entries already drawn, `columns` across and read across: a [`listing`]'s
/// layout, for entries that carry a picture ([`entry`]).
pub fn grid<'a, M: 'a>(entries: Vec<Element<'a, M>>, columns: usize) -> Element<'a, M> {
    let columns = columns.max(1);
    let mut page = column![].spacing(0);
    let mut entries = entries.into_iter().peekable();
    while entries.peek().is_some() {
        let chunk: Vec<_> = entries.by_ref().take(columns).collect();
        let n = chunk.len();
        let mut line = row![].spacing(24).align_y(Alignment::Start);
        for e in chunk {
            line = line.push(e);
        }
        // A short last row keeps its columns where the full ones have them.
        for _ in n..columns {
            line = line.push(Space::new().width(Length::Fill));
        }
        page = page.push(line);
    }
    page.width(Length::Fill).into()
}

/// One entry over its hairline, with `picture` (a cover, a face) at its
/// left when it has one.
pub fn entry<'a, M: Clone + 'a>(picture: Option<Element<'a, M>>, e: &Entry<M>, on_cursor: bool, hover: M) -> Element<'a, M> {
    let title = text(e.title.clone()).size(14).style(move |t| text::Style {
        color: Some(match on_cursor {
            true => theme::of(t).primary.base.color,
            false => theme::of(t).background.base.text,
        }),
    });
    let mut words = column![].spacing(2).width(Length::Fill);
    if !e.over.is_empty() {
        words = words.push(text(e.over.to_uppercase()).size(10).style(style::faint(0.5)));
    }
    // The line under is drawn even empty, so a row of entries lines up.
    words = words.push(title).push(text(e.under.clone()).size(11).style(style::faint(0.5)));
    let body = row![].spacing(12).align_y(Alignment::Center);
    let body = match picture {
        Some(p) => body.push(p),
        None => body,
    };
    let body = row![
        body.push(words).width(Length::Fill),
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
            rule::horizontal(1).style(style::hairline)
        ]
        .width(Length::Fill),
    )
    .on_enter(hover)
    .on_press(e.open.clone())
    .into()
}

/// The line under a section that opens the rest of it — "See All 40
/// Works" — in the accent, its chevron where an entry's is; `lit` under
/// the cursor, filled as the cursor's row is. Opening it is the caller's.
pub fn more<'a, M: 'a>(label: &str, lit: bool) -> Element<'a, M> {
    let line = row![
        text(label.to_string()).size(14).style(move |t| match lit {
            true => text::Style {
                color: Some(theme::of(t).primary.base.text),
            },
            false => style::accent(t),
        }),
        Space::new().width(Length::Fill),
        text("\u{203A}").size(18).style(style::faint(0.45)),
    ]
    .align_y(Alignment::Center);
    container(container(line).padding([4, 6]).style(move |t| match lit {
        true => style::row(t, true, true, false),
        false => container::Style::default(),
    }))
    .padding(Padding {
        top: 8.0,
        right: 0.0,
        bottom: 4.0,
        left: 0.0,
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded face is a bold one: its `OS/2` table says weight 700,
    /// the weight [`BOLD`] asks for. Falsified by embedding iced's
    /// `FiraSans-Regular.ttf` in its place: that says 400.
    #[test]
    fn the_embedded_face_is_bold() {
        let f = FIRA_SANS_BOLD;
        let u16_at = |at: usize| u16::from_be_bytes([f[at], f[at + 1]]) as usize;
        let u32_at = |at: usize| u32::from_be_bytes([f[at], f[at + 1], f[at + 2], f[at + 3]]) as usize;
        let os2 = (0..u16_at(4))
            .map(|i| 12 + 16 * i)
            .find(|&at| &f[at..at + 4] == b"OS/2")
            .map(|at| u32_at(at + 8))
            .expect("an OS/2 table");
        assert_eq!(u16_at(os2 + 4), 700);
        assert_eq!(BOLD.weight, Weight::Bold);
    }

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
        assert_eq!(height(5000.0), TALLEST);
    }
}
