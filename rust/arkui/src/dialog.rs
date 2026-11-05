//! A dialog: a panel standing alone in the middle of the window, over the
//! page dimmed, for something done rather than chosen from — a search, a
//! form. A menu or a picker is about a thing on the page and opens at it;
//! this is about the page, so it takes the middle.
//!
//! **It moves on a spring** ([`SPRING`]): it fades in and grows from 95% of
//! its size, as shadcn/ui's does, the page dimming a little ahead of it,
//! and back the same way when it closes — from wherever it had got to, so
//! closed as it opens it turns back without a jump ([`crate::motion`]).
//! Where the system asks for less motion it only fades. An app holds a
//! [`Shown`] for it, keeps a clock running while that is
//! [`Shown::moving`], draws each frame from [`open`] and [`dimmed`], and
//! stops drawing it once it is [`Shown::gone`].
//!
//! A click on the dimmed page closes it and the wheel there is swallowed, as
//! [`crate::layer::dimmed`] does for a picker. A click on the dialog itself,
//! anywhere on it, is the dialog's: it is opaque to the pointer, so a click
//! between its controls does not fall through to the page under it.
use iced::time::Instant;
use iced::widget::{column, container, mouse_area, opaque, row, text, Space};
use iced::{Alignment, Background, Border, Element, Length, Vector};

use crate::motion::{self, Shown, Spring};
use crate::theme::of;

/// What it opens and closes on: the shared move ([`motion::SMOOTH`]),
/// critically damped, so it lands without a bounce — it was asked for, not
/// thrown.
pub const SPRING: Spring = motion::SMOOTH;

/// How far open it is when the page under it is all the way dimmed: the
/// dimming leads, as shadcn/ui's overlay's 150 ms does its dialog's 200.
pub const DIM_LEAD: f32 = 0.75;

/// How small it starts and ends: `zoom-in-95`, `zoom-out-95`.
pub const FROM: f32 = 0.95;

/// The page's dimming once it is open: [`crate::layer::dimmed`]'s.
const DIM: f32 = 0.72;

/// Its padding and its corner.
const PADDING: f32 = 20.0;
const RADIUS: f32 = 12.0;

/// A dialog coming into view at `now`.
pub fn shown(now: Instant) -> Shown {
    Shown::new(SPRING, now)
}

/// How far open it is at `now`, 0 to 1, on its spring.
pub fn open(shown: &Shown, now: Instant) -> f32 {
    shown.at(now)
}

/// How far the page under something [`Shown`] is dimmed at `now`, 0 to 1:
/// all the way once it is [`DIM_LEAD`] open. Worked out from how far open
/// it is rather than on a spring of its own, so it turns back with it.
pub fn dimmed(shown: &Shown, now: Instant) -> f32 {
    (shown.at(now) / DIM_LEAD).min(1.0)
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
    // The dialog takes every click over it: a container lets one through,
    // and under it is the page, whose click closes. Inside the move, so
    // "over it" is where it is drawn, not where it will be.
    let panel = opaque(panel);
    // With less motion it only fades: at its size from the start.
    let scale = match motion::reduced() {
        true => 1.0,
        false => FROM + (1.0 - FROM) * open,
    };
    let drawn: Element<'a, M> = if open < 1.0 {
        motion::transformed(panel, Vector::ZERO, scale, 1.0 - open, RADIUS)
    } else {
        panel
    };
    container(drawn).center_x(Length::Fill).center_y(Length::Fill).into()
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

    /// It opens, the page all the way dimmed before it is all the way open,
    /// and is still once open; closed, it goes back the same way and is
    /// then gone; closed as it opens, it turns back from where it got to.
    /// Falsified by the page dimmed at the dialog's own pace: when the page
    /// was all the way dimmed, the dialog was all the way open.
    #[test]
    fn it_opens_and_closes_and_turns_back() {
        let ms = std::time::Duration::from_millis;
        let at = Instant::now();
        let mut s = shown(at);
        assert_eq!(open(&s, at), 0.0);
        let dim = (0..200).map(|n| at + ms(n * 5)).find(|&t| dimmed(&s, t) >= 1.0).expect("dimmed");
        assert!(open(&s, dim) < 1.0, "the page dims ahead of the dialog");
        let rest = at + ms(800);
        assert_eq!(open(&s, rest), 1.0);
        assert!(!s.moving(rest + crate::motion::LAST_FRAME));
        s.hide(rest);
        assert!(s.moving(rest + ms(100)));
        assert!(s.gone(rest + ms(1000)));
        let mut s = shown(at);
        let part = open(&s, at + ms(60));
        s.hide(at + ms(60));
        assert!((open(&s, at + ms(60)) - part).abs() < 1e-3, "no jump");
        assert!(s.gone(at + ms(1000)));
    }
}
