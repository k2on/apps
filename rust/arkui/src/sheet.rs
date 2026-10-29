//! A sheet: a panel the height of the window along its right-hand edge,
//! over the page dimmed, for a list kept beside the page rather than one
//! about a thing on it — a queue. A dialog takes the middle for something
//! done; this takes the side for something looked down and rearranged.
//!
//! **It moves as shadcn/ui's sheet does**, without the blur: it slides in
//! from beyond the window's edge over [`TIMING`]'s 500 ms and back out over
//! its 300 ms, along Tailwind's `ease-in-out`, as the page dims and clears
//! as a dialog's does ([`crate::dialog::DIMMING`]). Its width is theirs: three
//! quarters of the window, and never more than [`MAX_WIDTH`]. An app holds
//! a [`Shown`] for it as for a dialog ([`crate::dialog`]). As a dialog is,
//! it is opaque to the pointer: a click anywhere on it is its own, and only
//! one on the dimmed page beside it closes it.
use iced::time::{Duration, Instant};
use iced::widget::{column, container, opaque, row, text, Space};
use iced::{Alignment, Background, Border, Element, Length, Vector};

use crate::motion::{self, Shown, Timing, EASE_IN_OUT};
use crate::theme::of;

/// How long it takes to slide in and out, and its curve: shadcn/ui's
/// `data-[state=open]:duration-500`, `data-[state=closed]:duration-300`,
/// on `ease-in-out`.
pub const TIMING: Timing = Timing {
    opening: Duration::from_millis(500),
    closing: Duration::from_millis(300),
    ease: EASE_IN_OUT,
};

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
    Shown::new(TIMING, now)
}

/// How far in it is at `now`, 0 to 1, along its curve.
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
    let drawn: Element<'a, M> = if open < 1.0 {
        motion::transformed(panel, Vector::new(width * (1.0 - open), 0.0), 1.0, 0.0, 0.0)
    } else {
        panel.into()
    };
    // Every click over it is its own: a container lets one through to the
    // page, whose click closes.
    container(opaque(drawn)).align_right(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three quarters of a narrow window, and no wider than 384 in a wide
    /// one. Falsified by a sheet the panel's own width: it overran a window
    /// of 400.
    /// A click on the sheet, below its rows, stays on it; a click on the
    /// dimmed page beside it closes it. Falsified by the panel without
    /// `opaque`: the click fell through to the page and closed it.
    #[test]
    fn a_click_on_it_does_not_close_it() {
        #[derive(Clone, Debug, PartialEq)]
        enum M {
            Close,
            Swallow,
        }
        let clicked = |at: iced::Point| {
            let page = iced::widget::stack![
                crate::dialog::backdrop(M::Close, M::Swallow, 1.0),
                view("A sheet".into(), None, text("×").into(), text("a row").into(), width(800.0), 1.0),
            ];
            let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), (800.0, 600.0), page);
            ui.point_at(at);
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages().collect::<Vec<_>>()
        };
        assert_eq!(clicked(iced::Point::new(700.0, 500.0)), []);
        assert_eq!(clicked(iced::Point::new(100.0, 300.0)), [M::Close]);
    }

    #[test]
    fn it_is_three_quarters_of_the_window_and_no_wider_than_384() {
        assert_eq!(width(400.0), 300.0);
        assert_eq!(width(1600.0), MAX_WIDTH);
    }

    /// In over half a second, out over less than a third; half way through
    /// sliding in it is some 78% of the way, `ease-in-out` landing softly.
    /// Falsified by the dialog's timing: at 300 ms it was all the way in.
    #[test]
    fn it_slides_in_slower_than_it_slides_out() {
        let at = Instant::now();
        let mut s = shown(at);
        assert!((open(&s, at + TIMING.opening / 2) - 0.776).abs() < 0.01);
        assert!(open(&s, at + Duration::from_millis(300)) < 1.0);
        assert_eq!(open(&s, at + TIMING.opening), 1.0);
        let out = at + TIMING.opening;
        s.hide(out);
        assert!(s.gone(out + TIMING.closing));
    }
}
