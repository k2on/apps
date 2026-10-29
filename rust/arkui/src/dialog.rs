//! A dialog: a panel standing alone in the middle of the window, over the
//! page dimmed, for something done rather than chosen from — a search, a
//! form. A menu or a picker is about a thing on the page and opens at it;
//! this is about the page, so it takes the middle.
//!
//! **It moves as shadcn/ui's does**, without the blur: it fades in and
//! grows from 95% of its size over [`TIMING`]'s 200 ms along CSS's `ease`,
//! as the page dims over [`DIMMING`]'s 150 ms, and back the same way when
//! it closes. An app holds a [`Shown`] for it, keeps a clock running while
//! that is [`Shown::moving`], draws each frame from [`open`] and
//! [`dimmed`], and stops drawing it once it is [`Shown::gone`].
//!
//! A click on the dimmed page closes it and the wheel there is swallowed, as
//! [`crate::layer::dimmed`] does for a picker. A click on the dialog itself,
//! anywhere on it, is the dialog's: it is opaque to the pointer, so a click
//! between its controls does not fall through to the page under it.
use iced::time::{Duration, Instant};
use iced::widget::{column, container, mouse_area, opaque, row, text, Space};
use iced::{Alignment, Background, Border, Element, Length, Vector};

use crate::motion::{self, Shown, Timing, EASE};
use crate::theme::of;

/// How long it takes to open and to close, and its curve: shadcn/ui's
/// `duration-200`, on `animate-in`'s and `animate-out`'s `ease`.
pub const TIMING: Timing = Timing {
    opening: Duration::from_millis(200),
    closing: Duration::from_millis(200),
    ease: EASE,
};

/// The page's dimming under it: its overlay's, which has no duration of
/// its own and so takes Tailwind's 150 ms.
pub const DIMMING: Timing = Timing {
    opening: Duration::from_millis(150),
    closing: Duration::from_millis(150),
    ease: EASE,
};

/// How small it starts and ends: `zoom-in-95`, `zoom-out-95`.
pub const FROM: f32 = 0.95;

/// The page's dimming once it is open: [`crate::layer::dimmed`]'s.
const DIM: f32 = 0.72;

/// Its padding and its corner.
const PADDING: f32 = 20.0;
const RADIUS: f32 = 12.0;

/// A dialog coming into view at `now`.
pub fn shown(now: Instant) -> Shown {
    Shown::new(TIMING, now)
}

/// How far open it is at `now`, 0 to 1, along its curve.
pub fn open(shown: &Shown, now: Instant) -> f32 {
    shown.at(now)
}

/// How far the page under it is dimmed at `now`, 0 to 1.
pub fn dimmed(shown: &Shown, now: Instant) -> f32 {
    shown.at_pace(DIMMING, now)
}

/// The page under it, dimmed `dim` of the way: a click closes, the wheel
/// is swallowed.
pub fn backdrop<'a, M: Clone + 'a>(close: M, swallow: M, dim: f32) -> Element<'a, M> {
    mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |theme| container::Style {
                background: Some(Background::Color(of(theme).background.base.color.scale_alpha(DIM * dim))),
                ..container::Style::default()
            }),
    )
    .on_press(close.clone())
    .on_right_press(close)
    .on_scroll(move |_| swallow.clone())
    .into()
}

/// The dialog, `width` wide and at most `height` tall: its title and its
/// close beside it over `body`, in the middle of the window, `open` of the
/// way grown to its size and faded in.
pub fn view<'a, M: Clone + 'a>(title: String, close: Element<'a, M>, body: Element<'a, M>, width: f32, height: f32, open: f32) -> Element<'a, M> {
    let head = row![text(title).size(17).font(crate::hero::BOLD), Space::new().width(Length::Fill), close].align_y(Alignment::Center);
    let panel = container(column![head, body].spacing(14))
        .width(Length::Fixed(width))
        .max_height(height)
        .padding(PADDING)
        .style(style);
    present(panel, open)
}

/// A dialog's ground and edge: what a panel in its place is drawn on.
pub fn style(theme: &iced::Theme) -> container::Style {
    let palette = of(theme);
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: RADIUS.into(),
        },
        ..container::Style::default()
    }
}

/// `panel` where a dialog goes, as a dialog moves, `open` of the way in:
/// in the middle of the window, grown from 95% and faded in, taking every
/// click over it. What a panel that is not titled as [`view`] is, but
/// stands where a dialog does — [`crate::command`]'s — is drawn by.
pub fn present<'a, M: 'a>(panel: impl Into<Element<'a, M>>, open: f32) -> Element<'a, M> {
    let panel = panel.into();
    // Faded in under a veil of the page's ground, thinning as it opens
    // (`motion`: iced has no opacity for a group of widgets).
    let open = open.clamp(0.0, 1.0);
    let drawn: Element<'a, M> = if open < 1.0 {
        motion::transformed(panel, Vector::ZERO, FROM + (1.0 - FROM) * open, 1.0 - open, RADIUS)
    } else {
        panel
    };
    // The dialog takes every click over it: a container lets one through,
    // and under it is the page, whose click closes.
    container(opaque(drawn)).center_x(Length::Fill).center_y(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click on the dialog, between its controls, stays on it; a click on
    /// the dimmed page beside it closes it. Falsified by the panel without
    /// `opaque`: the click in its middle fell through to the page and closed
    /// it.
    #[test]
    fn a_click_on_it_does_not_close_it() {
        #[derive(Clone, Debug, PartialEq)]
        enum M {
            Close,
            Swallow,
        }
        let clicked = |at: iced::Point| {
            let close = text("×");
            let body = text("words, and nothing to press").into();
            let page = iced::widget::stack![
                backdrop(M::Close, M::Swallow, 1.0),
                view("A dialog".into(), close.into(), body, 400.0, 300.0, 1.0)
            ];
            let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), (800.0, 600.0), page);
            ui.point_at(at);
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages().collect::<Vec<_>>()
        };
        assert_eq!(clicked(iced::Point::new(400.0, 300.0)), []);
        assert_eq!(clicked(iced::Point::new(20.0, 20.0)), [M::Close]);
    }

    /// It fades in and grows from 95% over 200 ms, the page dimming over
    /// 150 ms, and is still once open; closed, it goes back the same way and
    /// is then gone. Falsified by the page dimming at the dialog's pace: at
    /// 150 ms it was not yet all the way dimmed.
    #[test]
    fn it_opens_and_closes_as_shadcn_s_does() {
        let at = Instant::now();
        let mut s = shown(at);
        assert_eq!(open(&s, at), 0.0);
        assert_eq!(dimmed(&s, at + DIMMING.opening), 1.0);
        assert!(open(&s, at + DIMMING.opening) < 1.0);
        assert_eq!(open(&s, at + TIMING.opening), 1.0);
        assert!(!s.moving(at + TIMING.opening));
        let closing = at + TIMING.opening;
        s.hide(closing);
        assert!(s.moving(closing + TIMING.closing / 2));
        assert!(s.gone(closing + TIMING.closing));
    }
}
