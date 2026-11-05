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
use iced::time::Instant;
use iced::widget::{column, container, opaque, row, text, Space};
use iced::{Alignment, Background, Border, Element, Length, Vector};

use crate::motion::{self, Shown, Spring};
use crate::theme::of;

/// What it slides on: critically damped, as a thing opened by a click and
/// not flung is — Apple's drawer has a little bounce, for one dragged —
/// and a little slower than a dialog, for the further it travels.
pub const SPRING: Spring = Spring { damping: 1.0, response: 0.4 };

/// The widest it gets: `sm:max-w-sm`, 24 rem.
pub const MAX_WIDTH: f32 = 384.0;

/// Its padding, and the room between its header and its body: `p-4`,
/// `gap-4`.
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
    (window - 2.0 * PADDING - header - GAP).max(0.0)
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
        }));
    }
    let head = row![words, Space::new().width(Length::Fill), close].align_y(Alignment::Start);
    let panel = container(column![head, body].spacing(GAP))
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .padding(PADDING)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Three quarters of a narrow window, and no wider than 384 in a wide
    /// one. Falsified by a sheet the panel's own width: it overran a window
    /// of 400.
    /// A click on the sheet, below its rows, stays on it; a click on the
    /// dimmed page beside it closes it — and while it slides in, so does a
    /// click where it will be but is not yet. Falsified by the panel without
    /// `opaque` (the click on it fell through to the page and closed it),
    /// and by `opaque` outside the slide (the click where it would be, half
    /// in, was taken as one on it).
    #[test]
    fn a_click_on_it_does_not_close_it() {
        #[derive(Clone, Debug, PartialEq)]
        enum M {
            Close,
            Swallow,
        }
        let clicked_at = |at: iced::Point, open: f32| {
            let page = iced::widget::stack![
                crate::dialog::backdrop(M::Close, M::Swallow, 1.0),
                view("A sheet".into(), None, text("×").into(), text("a row").into(), width(800.0), open),
            ];
            let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), (800.0, 600.0), page);
            ui.point_at(at);
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages().collect::<Vec<_>>()
        };
        let clicked = |at| clicked_at(at, 1.0);
        assert_eq!(clicked(iced::Point::new(700.0, 500.0)), []);
        assert_eq!(clicked(iced::Point::new(100.0, 300.0)), [M::Close]);
        // Half way in, it spans 608 to 800 of a window 800 wide (384 wide,
        // 192 of it still beyond the edge): a click at 500, where it will
        // be but is not yet, is on the page; one at 700 is on it.
        assert_eq!(clicked_at(iced::Point::new(500.0, 300.0), 0.5), [M::Close]);
        assert_eq!(clicked_at(iced::Point::new(700.0, 300.0), 0.5), []);
    }

    #[test]
    fn it_is_three_quarters_of_the_window_and_no_wider_than_384() {
        assert_eq!(width(400.0), 300.0);
        assert_eq!(width(1600.0), MAX_WIDTH);
    }

    /// It slides in a little slower than a dialog opens, and is in; closed
    /// as it slides in, it turns back from where it was and is gone.
    /// Falsified by the dialog's spring: at 120 ms it was as far in as a
    /// dialog is open.
    #[test]
    fn it_slides_in_and_turns_back_from_where_it_is() {
        let ms = std::time::Duration::from_millis;
        let at = Instant::now();
        let mut s = shown(at);
        let dialog = crate::dialog::shown(at);
        assert!(open(&s, at + ms(120)) < crate::dialog::open(&dialog, at + ms(120)));
        assert_eq!(open(&s, at + ms(1000)), 1.0);
        let part = open(&s, at + ms(100));
        s.hide(at + ms(100));
        assert!((open(&s, at + ms(100)) - part).abs() < 1e-3, "no jump");
        assert!(!s.gone(at + ms(150)));
        assert!(s.gone(at + ms(1500)));
    }
}
