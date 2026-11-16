//! A sheet: a panel the height of the window along its right-hand edge,
//! over the page dimmed, for a list kept beside the page rather than one
//! about a thing on it — a queue. A dialog takes the middle for something
//! done; this takes the side for something looked down and rearranged.
//!
//! **It slides in from beyond the window's edge on a spring** ([`SPRING`]),
//! and back out the way it came, as the page dims and clears with it as a
//! dialog's does ([`crate::dialog::dimmed`]). Closed as it slides in, it
//! slows, stops and goes back from where it was, not from all the way in
//! ([`crate::motion`]); where the system asks for less motion it only
//! fades, where it stands. Its width is shadcn/ui's sheet's: three
//! quarters of the window, and never more than [`MAX_WIDTH`]. An app holds
//! a [`Shown`] for it as for a dialog ([`crate::dialog`]). As a dialog is,
//! it is opaque to the pointer: a click anywhere on it is its own, and only
//! one on the dimmed page beside it closes it.
use crate::text::{self, text};
use iced::time::Instant;
use iced::widget::{column, container, opaque, row, Space};
use iced::{Alignment, Background, Border, Element, Length, Padding, Vector};

use crate::motion::{self, Shown, Spring};
use crate::theme::of;

/// What it slides on: the shared move ([`motion::SMOOTH`]), critically
/// damped, as a thing opened by a click and not flung is — Apple's drawer
/// has a little bounce, for one dragged. The same response as a dialog's
/// and the phones' sheets: a spring's speed grows with the distance, so
/// the further travel needs no slower spring.
pub const SPRING: Spring = motion::SMOOTH;

/// The widest it gets: `sm:max-w-sm`, 24 rem.
pub const MAX_WIDTH: f32 = 384.0;

/// Its padding, and the room between its header and its body: `p-4`,
/// `gap-4`. The padding is on the top and the sides only: the body runs
/// down to the window's foot, so a list in it scrolls all the way down
/// rather than stopping a padding short of the edge with its last row cut
/// off there. A body that scrolls carries the padding under its last row
/// itself, inside what scrolls, so that row can still be scrolled clear.
pub const PADDING: f32 = 16.0;
pub const GAP: f32 = 16.0;

/// How tall its header is with a line under the title, and without: the
/// title's line, and the description's.
pub const HEADER: f32 = 24.0 + 6.0 + 18.0;
pub const HEADER_ALONE: f32 = 24.0;

/// A sheet coming into view at `now`.
pub fn shown(now: Instant) -> Shown {
    Shown::new(SPRING, now)
}

/// How far in it is at `now`, 0 to 1, on its spring.
pub fn open(shown: &Shown, now: Instant) -> f32 {
    shown.at(now)
}

/// How wide it is in a window `window` wide: `w-3/4 sm:max-w-sm`.
pub fn width(window: f32) -> f32 {
    (window * 0.75).min(MAX_WIDTH)
}

/// How tall its body is in a window `window` tall, with a description under
/// its title or not: what a list in it is given to scroll in.
pub fn body_height(window: f32, described: bool) -> f32 {
    let header = if described { HEADER } else { HEADER_ALONE };
    (window - PADDING - header - GAP).max(0.0)
}

/// The sheet, `width` wide and the window's height at its right-hand edge:
/// `title`, and `description` under it in a quieter line, with `close`
/// beside them, over `body`; `open` of the way slid in.
pub fn view<'a, M: 'a>(
    title: String,
    description: Option<String>,
    close: Element<'a, M>,
    body: Element<'a, M>,
    width: f32,
    open: f32,
) -> Element<'a, M> {
    let mut words = column![text(title).size(16).font(crate::hero::BOLD).height(Length::Fixed(HEADER_ALONE))].spacing(6);
    if let Some(d) = description {
        words = words.push(text(d).size(13).height(Length::Fixed(18.0)).style(|theme| text::Style {
            color: Some(of(theme).background.strong.text),
            ..Default::default()
        }));
    }
    let head = row![words, Space::new().width(Length::Fill), close].align_y(Alignment::Start);
    let panel = container(column![head, body].spacing(GAP))
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .padding(Padding {
            bottom: 0.0,
            ..Padding::new(PADDING)
        })
        .style(|theme| {
            let palette = of(theme);
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                // `border-l`: a hairline down the side it slid in from; the
                // others are the window's edges.
                border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            }
        });
    // Slid in from beyond the edge: as far out as it is wide, at first.
    let open = open.clamp(0.0, 1.0);
    // Every click over it is its own: a container lets one through to the
    // page, whose click closes. Inside the slide, so "over it" is where it
    // is drawn: half in, a click where it will be but is not yet closes it.
    let panel = opaque(panel);
    // With less motion it stays where it ends and only fades, under a veil
    // of the ground thinning as it opens.
    let drawn: Element<'a, M> = if open < 1.0 {
        match motion::reduced() {
            true => motion::transformed(panel, Vector::ZERO, 1.0, 1.0 - open, 0.0),
            false => motion::transformed(panel, Vector::new(width * (1.0 - open), 0.0), 1.0, 0.0, 0.0),
        }
    } else {
        panel
    };
    container(drawn).align_right(Length::Fill).height(Length::Fill).into()
}
