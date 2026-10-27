//! A dialog: a panel standing alone in the middle of the window, over the
//! page dimmed, for something done rather than chosen from — a search, a
//! form. A menu or a picker is about a thing on the page and opens at it;
//! this is about the page, so it takes the middle.
//!
//! **It opens as a sheet does**: rising the last [`RISE`] pixels into place
//! as the page dims, over [`OPENING`], eased out so it arrives softly. The
//! app keeps a clock running while [`opening`] says so, and draws each
//! frame from [`opened`]; at rest it is still, and nothing ticks.
//!
//! A click on the dimmed page closes it and the wheel there is swallowed, as
//! [`crate::layer::dimmed`] does for a picker.
use iced::time::{Duration, Instant};
use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use crate::theme::of;

/// How long it takes to open.
pub const OPENING: Duration = Duration::from_millis(220);

/// How far below its place it starts.
pub const RISE: f32 = 28.0;

/// The page's dimming once it is open: [`crate::layer::dimmed`]'s.
const DIM: f32 = 0.72;

/// Its padding and its corner.
const PADDING: f32 = 20.0;
const RADIUS: f32 = 12.0;

/// How far open it is at `now`, opened at `since`: 0 to 1, eased out
/// (cubic), so it moves quickest at first and settles.
pub fn opened(since: Instant, now: Instant) -> f32 {
    let t = (now.saturating_duration_since(since).as_secs_f32() / OPENING.as_secs_f32()).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Whether it is still opening at `now`: a frame to draw yet.
pub fn opening(since: Instant, now: Instant) -> bool {
    now.saturating_duration_since(since) < OPENING
}

/// The page under it, dimmed as far as it is open: a click closes, the
/// wheel is swallowed.
pub fn backdrop<'a, M: Clone + 'a>(close: M, swallow: M, open: f32) -> Element<'a, M> {
    mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |theme| container::Style {
                background: Some(Background::Color(of(theme).background.base.color.scale_alpha(DIM * open))),
                ..container::Style::default()
            }),
    )
    .on_press(close.clone())
    .on_right_press(close)
    .on_scroll(move |_| swallow.clone())
    .into()
}

/// The dialog, `width` wide and at most `height` tall: its title and its
/// close beside it over `body`, in the middle of the window and `open` of
/// the way risen into place.
pub fn view<'a, M: Clone + 'a>(title: String, close: Element<'a, M>, body: Element<'a, M>, width: f32, height: f32, open: f32) -> Element<'a, M> {
    let head = row![text(title).size(17).font(crate::hero::BOLD), Space::new().width(Length::Fill), close].align_y(Alignment::Center);
    let panel = container(column![head, body].spacing(14))
        .width(Length::Fixed(width))
        .max_height(height)
        .padding(PADDING)
        .style(move |theme| {
            let palette = of(theme);
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                border: Border {
                    color: palette.background.strong.color.scale_alpha(open),
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                ..container::Style::default()
            }
        });
    // Centred, then let down by what is left of the rise: padding above it
    // is twice the drop, as the centring halves it.
    container(container(panel).padding(Padding {
        top: 2.0 * RISE * (1.0 - open),
        ..Padding::ZERO
    }))
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// It opens from nothing to all the way, eased out — past halfway at
    /// half the time — and is still once open. Falsified by easing in: half
    /// the time was an eighth of the way.
    #[test]
    fn it_opens_eased_out_and_then_rests() {
        let at = Instant::now();
        assert_eq!(opened(at, at), 0.0);
        assert!(opened(at, at + OPENING / 2) > 0.8);
        assert_eq!(opened(at, at + OPENING), 1.0);
        assert!(opening(at, at + OPENING / 2));
        assert!(!opening(at, at + OPENING));
    }
}
