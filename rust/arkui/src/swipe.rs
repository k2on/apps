//! Two fingers sideways across a trackpad, as back and forward — the way a
//! browser has it: the page follows the fingers, tighter the further it is
//! pulled; an arrow at the edge it is pulled from says which way it would
//! go, and lights once the pull is far enough; and the page goes only when
//! the fingers let go past that point. Short of it, it springs back.
//!
//! A browser does this by itself — but only over a page that lets the wheel
//! through, and a canvas app takes every wheel event, so the gesture never
//! reaches the browser; on the desktop there is no browser at all. So the
//! app feeds the wheel here ([`Swipe::feed`]), asks for a frame clock while
//! [`Swipe::active`], and on each frame asks [`Swipe::tick`] whether the
//! fingers have let go — and where to.
//!
//! **Letting go is the wheel falling quiet.** A wheel event carries no
//! touch phase here, only a distance; so a gesture ends when no event has
//! come for [`LET_GO`]. A trackpad that coasts after the fingers lift keeps
//! the gesture going until it stops, which reads as the fingers letting go
//! a moment late. Whatever is still arriving after a gesture ends is its
//! coast, and is spent until the wheel has been quiet for [`QUIET`].
use iced::mouse::ScrollDelta;
use iced::time::{Duration, Instant};
use iced::widget::{container, stack};
use iced::{Alignment, Background, Element, Length, Padding};

use crate::{glyphs, icon, theme};

/// How far sideways, in pixels of the fingers' travel, a pull goes before
/// letting go would move.
pub const FAR: f32 = 160.0;

/// How long the wheel is still before the fingers count as lifted.
pub const LET_GO: Duration = Duration::from_millis(90);

/// How long the wheel is still before what arrives is a new gesture rather
/// than the last one coasting.
pub const QUIET: Duration = Duration::from_millis(300);

/// How long the page takes to spring back.
pub const BACK: Duration = Duration::from_millis(220);

/// The furthest the page is drawn moved, however far it is pulled: the
/// rubber band's length.
pub const STRETCH: f32 = 180.0;

/// A line of a wheel that turns in lines, in pixels, as a browser counts it.
const LINE: f32 = 40.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum State {
    #[default]
    Idle,
    /// The fingers are down: `x` is how far they have gone.
    Pulling { x: f32 },
    /// Let go at `x`, springing back since `at`; what still arrives is the
    /// gesture coasting.
    Settling { x: f32, at: Instant },
}

/// One gesture, from the first sideways motion to the page settling.
#[derive(Debug, Default)]
pub struct Swipe {
    state: State,
    last: Option<Instant>,
    /// How far it has gone down, to tell a sideways pull from a scroll.
    y: f32,
}

/// Pulled `x`, drawn: as far as the fingers at first, and ever less for
/// each pixel more, never past [`STRETCH`] — a rubber band.
pub fn band(x: f32) -> f32 {
    x.signum() * STRETCH * (1.0 - (-x.abs() / STRETCH).exp())
}

impl Swipe {
    /// One wheel event.
    pub fn feed(&mut self, delta: ScrollDelta, now: Instant) {
        let (dx, dy) = match delta {
            ScrollDelta::Pixels { x, y } => (x, y),
            ScrollDelta::Lines { x, y } => (x * LINE, y * LINE),
        };
        let quiet = self.last.is_none_or(|t| now.duration_since(t) > QUIET);
        self.last = Some(now);
        match self.state {
            Settling { .. } if !quiet => {}
            Pulling { x } => {
                self.y += dy.abs();
                self.state = Pulling { x: x + dx };
            }
            _ => {
                self.y = dy.abs();
                self.state = Pulling { x: dx };
            }
        }
    }

    /// A frame: `Some(-1)` back or `Some(1)` forward when the fingers have
    /// just let go far enough sideways — fingers to the right are back, as a
    /// browser has it.
    pub fn tick(&mut self, now: Instant) -> Option<isize> {
        match self.state {
            Pulling { x } if self.last.is_some_and(|t| now.duration_since(t) > LET_GO) => {
                self.state = Settling { x, at: now };
                (self.armed_at(x)).then_some(if x > 0.0 { -1 } else { 1 })
            }
            Settling { at, .. } if now.duration_since(at) > BACK && self.last.is_some_and(|t| now.duration_since(t) > QUIET) => {
                self.state = Idle;
                None
            }
            _ => None,
        }
    }

    fn armed_at(&self, x: f32) -> bool {
        x.abs() >= FAR && x.abs() >= 2.0 * self.y
    }

    /// Whether a gesture is under way, and so frames are wanted.
    pub fn active(&self) -> bool {
        self.state != Idle
    }

    /// How far the page is drawn moved now: right is positive.
    pub fn offset(&self, now: Instant) -> f32 {
        match self.state {
            Idle => 0.0,
            Pulling { x } if x.abs() < 2.0 * self.y => 0.0,
            Pulling { x } => band(x),
            Settling { x, at } => {
                let t = (now.duration_since(at).as_secs_f32() / BACK.as_secs_f32()).min(1.0);
                // Out of the way fast and then easing in, as a spring lets go.
                band(x) * (1.0 - t).powi(3)
            }
        }
    }

    /// Which way the arrow points and how far towards going it is, 0 to 1,
    /// and whether letting go now would go: `None` while there is no pull.
    pub fn arrow(&self) -> Option<(isize, f32, bool)> {
        match self.state {
            Pulling { x } if x.abs() >= 2.0 * self.y && x != 0.0 => {
                let by = if x > 0.0 { -1 } else { 1 };
                Some((by, (x.abs() / FAR).min(1.0), self.armed_at(x)))
            }
            _ => None,
        }
    }
}

use State::{Idle, Pulling, Settling};

/// `page` drawn moved by the swipe, squeezed from the side it is pulled
/// from rather than slid off the window, with the arrow at that edge: a
/// circle growing in as the pull does, filled with the accent once letting
/// go would go.
pub fn view<'a, M: 'a>(page: Element<'a, M>, swipe: &Swipe, now: Instant) -> Element<'a, M> {
    let off = swipe.offset(now);
    if off == 0.0 && swipe.arrow().is_none() {
        return page;
    }
    let moved = container(page).width(Length::Fill).height(Length::Fill).padding(Padding {
        left: off.max(0.0),
        right: (-off).max(0.0),
        ..Padding::ZERO
    });
    let Some((by, progress, armed)) = swipe.arrow() else {
        return moved.into();
    };
    let side = 24.0 + 20.0 * progress;
    let glyph = if by < 0 { glyphs::BACK } else { glyphs::CHEVRON };
    let mark = icon::tinted(glyph, side * 0.5, move |p| match armed {
        true => p.primary.base.text,
        false => p.background.base.text.scale_alpha(0.4 + 0.5 * progress),
    });
    let disc = container(mark)
        .width(Length::Fixed(side))
        .height(Length::Fixed(side))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |t| {
            let p = theme::of(t);
            container::Style {
                background: Some(Background::Color(match armed {
                    true => p.primary.base.color,
                    false => p.background.strong.color.scale_alpha(0.6 + 0.4 * progress),
                })),
                border: iced::border::rounded(side / 2.0),
                ..container::Style::default()
            }
        });
    // In the gap the pull opens, halfway across it.
    let inset = (off.abs() - side).max(0.0) / 2.0;
    let place = container(disc)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .align_x(if by < 0 { Alignment::Start } else { Alignment::End })
        .padding(Padding {
            left: if by < 0 { inset } else { 0.0 },
            right: if by > 0 { inset } else { 0.0 },
            ..Padding::ZERO
        });
    stack![moved, place].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(x: f32, y: f32) -> ScrollDelta {
        ScrollDelta::Pixels { x, y }
    }

    /// A pull goes only when it is let go past [`FAR`], once; short of it,
    /// it springs back and goes nowhere; the coast after letting go is the
    /// same gesture; and a scroll that is mostly down never pulls. Falsified
    /// by stepping in `feed` as the old swipe did: the first pull went
    /// before it was let go.
    #[test]
    fn a_pull_goes_when_it_is_let_go_far_enough() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let mut s = Swipe::default();
        s.feed(px(100.0, 0.0), ms(0));
        s.feed(px(100.0, 0.0), ms(16));
        assert_eq!(s.tick(ms(32)), None, "still down: nothing yet");
        assert_eq!(s.arrow(), Some((-1, 1.0, true)), "an armed arrow, pointing back");
        assert!(s.offset(ms(32)) > 0.0 && s.offset(ms(32)) < 200.0, "moved, and less than pulled");
        assert_eq!(s.tick(ms(200)), Some(-1), "let go: back");
        s.feed(px(40.0, 0.0), ms(210));
        assert_eq!(s.tick(ms(400)), None, "the coast is spent");
        assert_eq!(s.offset(ms(600)), 0.0, "and the page is back");

        let mut short = Swipe::default();
        short.feed(px(-80.0, 0.0), ms(2000));
        assert_eq!(short.arrow(), Some((1, 0.5, false)), "forward, half way, not armed");
        assert_eq!(short.tick(ms(2200)), None, "let go short: nowhere");

        let mut down = Swipe::default();
        down.feed(px(200.0, 150.0), ms(5000));
        assert_eq!(down.arrow(), None);
        assert_eq!(down.tick(ms(5200)), None, "mostly down is a scroll");
    }

    /// The band gives a pixel for a pixel at first and never more than its
    /// length.
    #[test]
    fn the_band_stretches_less_the_further_it_goes() {
        assert!((band(1.0) - 1.0).abs() < 0.01);
        assert!(band(1000.0) < STRETCH);
        assert!(band(-1000.0) > -STRETCH);
        assert!(band(200.0) - band(100.0) < band(100.0));
    }
}
