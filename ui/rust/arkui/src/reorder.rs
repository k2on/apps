//! A list whose rows are dragged into a new order: a row pressed and pulled
//! lifts out and follows the pointer, and the rows it passes slide out of
//! its way, a row's height up or down, so the gap it would land in is
//! always open under it. Let go, it settles into that gap.
//!
//! **The rows are all one height** ([`crate::panel::ENTRY`], in a panel), so
//! where a row would land is arithmetic on how far the pointer has moved
//! since the press, not a measurement of anything laid out — the module
//! doc of [`crate::panel`] says why. Nor does it need to know where the list
//! is on screen: only how far the pointer has moved.
//!
//! **A press that never moves is a click.** Under [`THRESHOLD`] pixels of
//! travel nothing lifts, and letting go is [`Drop::Click`], so a row still
//! does what a click on it did.
//!
//! **Nothing ticks at rest.** An app keeps a frame clock while
//! [`Reorder::moving`] or [`Settle::moving`] says so, as for any
//! [`crate::motion`].
//!
//! The app owns the order. A drop says from where to where
//! ([`Drop::Moved`]); the app moves its row, and draws the list in the new
//! order with the [`Settle`] it was given, which carries the dropped row
//! the rest of the way from where it was let go.
use iced::time::{Duration, Instant};
use iced::widget::{column, container, stack, Space};
use iced::{Element, Length, Shadow, Vector};

use crate::motion::{self, Slide, Spring};

/// How far, in pixels, a press travels before it is a drag.
pub const THRESHOLD: f32 = 4.0;

/// What a row slides out of the way on: the shared move
/// ([`motion::SMOOTH`]), critically damped, so the gap is open by the time
/// the eye is on it. Sent back on its way — the pointer turned — it turns
/// from where it is ([`crate::motion`]).
pub const SLIDING: Spring = motion::SMOOTH;

/// What a row let go at `speed` settles into its place on, setting off as
/// fast as the pointer was going: thrown — at least a flick's speed — a
/// little under critically damped ([`motion::THROWN`]), so it swings a hair
/// past and comes back, as a thing thrown does; put down, critically damped
/// ([`motion::SMOOTH`]). The phones' `Springs.settle` and `released`, by
/// [`motion::released`]. Either way it is at rest within [`motion::CAP`].
pub fn settling(speed: f32) -> Spring {
    motion::released(speed)
}

/// How long the pointer can be still before letting go and still hand on
/// its speed: held still longer, it was put down, not thrown.
const HELD: Duration = Duration::from_millis(60);

/// How much bigger a lifted row is drawn than its place.
pub const LIFT: f32 = 1.02;

/// A press on one row of a list `rows` long, each `row` tall, and the drag
/// it may become.
#[derive(Clone, Debug, PartialEq)]
pub struct Reorder {
    from: usize,
    rows: usize,
    row: f32,
    press: f32,
    y: f32,
    dragging: bool,
    to: usize,
    slides: Vec<Slide>,
    /// When the pointer last moved, and how fast it was going then, in
    /// pixels a second: what a row let go hands on to its settling.
    moved: Instant,
    speed: f32,
}

/// What letting go did: nothing moved far enough to be a drag, or the row
/// pressed goes from one place to another (perhaps the same).
#[derive(Clone, Debug, PartialEq)]
pub enum Drop {
    Click(usize),
    Moved { from: usize, to: usize, settle: Settle },
}

/// A row let go, on its way the rest of the way into its new place `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settle {
    pub to: usize,
    /// When it was let go: past [`motion::CAP`] it is in its place.
    since: Instant,
    slide: Slide,
}

impl Settle {
    /// Whether the cap has gone by at `now`, however the spring stands.
    fn capped(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) >= motion::CAP
    }

    /// How far the row is from its place at `now`.
    pub fn offset(&self, now: Instant) -> f32 {
        match self.capped(now) {
            true => 0.0,
            false => self.slide.at(now),
        }
    }

    /// Whether it is still on its way — and for the frame after, so the
    /// one at rest is drawn ([`motion::LAST_FRAME`]).
    pub fn moving(&self, now: Instant) -> bool {
        self.slide.moving(now) && !self.capped(now.checked_sub(motion::LAST_FRAME).unwrap_or(now))
    }
}

impl Reorder {
    /// Row `from` of `rows`, each `row` tall, pressed with the pointer at
    /// `y`.
    pub fn press(from: usize, rows: usize, row: f32, y: f32, now: Instant) -> Self {
        Reorder {
            from,
            rows,
            row,
            press: y,
            y,
            dragging: false,
            to: from,
            slides: vec![Slide::still(0.0, now); rows],
            moved: now,
            speed: 0.0,
        }
    }

    /// The row pressed.
    pub fn pressed(&self) -> usize {
        self.from
    }

    /// Whether the press has become a drag.
    pub fn dragging(&self) -> bool {
        self.dragging
    }

    /// Where the row would land if let go now.
    pub fn to(&self) -> usize {
        self.to
    }

    /// How far the lifted row is drawn from its own place: with the
    /// pointer, but never off either end of the list.
    pub fn lift(&self) -> f32 {
        if !self.dragging {
            return 0.0;
        }
        let up = -(self.from as f32) * self.row;
        let down = (self.rows.saturating_sub(1) - self.from.min(self.rows.saturating_sub(1))) as f32 * self.row;
        (self.y - self.press).clamp(up, down)
    }

    /// The pointer moved to `y`: past the threshold it is a drag, and the
    /// rows between where the row was and where it would now land slide out
    /// of its way.
    pub fn drag(&mut self, y: f32, now: Instant) {
        // How fast it is going: this move's speed, weighed with the last
        // ones' unless the pointer had stopped, so one jittery event does
        // not throw the row.
        let dt = now.saturating_duration_since(self.moved);
        if !dt.is_zero() {
            let speed = (y - self.y) / dt.as_secs_f32();
            self.speed = match dt > HELD {
                true => speed,
                false => 0.6 * speed + 0.4 * self.speed,
            };
            self.moved = now;
        }
        self.y = y;
        if !self.dragging && (y - self.press).abs() < THRESHOLD {
            return;
        }
        self.dragging = true;
        let landing = (self.from as f32 * self.row + self.lift()) / self.row;
        self.to = (landing.round().max(0.0) as usize).min(self.rows.saturating_sub(1));
        let sliding = SLIDING.or_instant();
        for i in 0..self.rows {
            let target = self.gap(i);
            self.slides[i].toward(target, now, sliding);
        }
    }

    /// How fast the lifted row is going at `now`, in pixels a second:
    /// the pointer's, unless it has been held still, or the row is held at
    /// an end of the list and goes nowhere however the pointer does.
    fn lift_speed(&self, now: Instant) -> f32 {
        let held = now.saturating_duration_since(self.moved) > HELD;
        let pinned = self.lift() != self.y - self.press;
        match held || pinned {
            true => 0.0,
            false => self.speed,
        }
    }

    /// Where row `i` goes to leave the gap open: a row's height up if it is
    /// between the pressed row and below it where it lands, down if between
    /// where it lands and above it, and nowhere otherwise.
    fn gap(&self, i: usize) -> f32 {
        let (from, to) = (self.from, self.to);
        if from < to && from < i && i <= to {
            -self.row
        } else if to < from && to <= i && i < from {
            self.row
        } else {
            0.0
        }
    }

    /// How far row `i` is drawn from its place at `now`.
    pub fn offset(&self, i: usize, now: Instant) -> f32 {
        self.slides.get(i).map_or(0.0, |s| s.at(now))
    }

    /// Whether any row is still sliding at `now`.
    pub fn moving(&self, now: Instant) -> bool {
        self.slides.iter().any(|s| s.moving(now))
    }

    /// Let go at `now`.
    pub fn drop(&self, now: Instant) -> Drop {
        if !self.dragging {
            return Drop::Click(self.from);
        }
        // Where it was let go, from its new place: the rest of the way, from
        // as fast as the pointer was going, so there is no seam between the
        // drag and the settling.
        let left = self.from as f32 * self.row + self.lift() - self.to as f32 * self.row;
        Drop::Moved {
            from: self.from,
            to: self.to,
            settle: Settle {
                to: self.to,
                since: now,
                slide: Slide::new(left, 0.0, now, settling(self.lift_speed(now)).or_instant()).moving_at(self.lift_speed(now)),
            },
        }
    }
}

/// `items` with the one at `from` taken out and put back at `to`: the order
/// a [`Drop::Moved`] leaves.
pub fn moved<T: Clone>(items: &[T], from: usize, to: usize) -> Vec<T> {
    let mut out = items.to_vec();
    if from < out.len() && to < out.len() {
        let item = out.remove(from);
        out.insert(to, item);
    }
    out
}

/// The list: `rows`, each `row` tall, in order, with a press or a drag of
/// one (`drag`) or a row settling after one (`settle`) drawn over the rest
/// as it moves. A row lifted or settling is drawn above its neighbours with
/// a shadow, a little larger; its place under it is left empty.
pub fn list<'a, M: 'a>(rows: Vec<Element<'a, M>>, row: f32, drag: Option<&Reorder>, settle: Option<&Settle>, now: Instant) -> Element<'a, M> {
    // The row drawn over the rest, and how far from its own place.
    let lifted = match (drag, settle) {
        (Some(d), _) if d.dragging() => Some((d.pressed(), d.lift())),
        (_, Some(s)) if s.moving(now) => Some((s.to, s.offset(now))),
        _ => None,
    };
    let mut under = column![].spacing(0);
    let mut over = None;
    for (i, r) in rows.into_iter().enumerate() {
        match lifted {
            Some((at, by)) if at == i => {
                under = under.push(Space::new().height(Length::Fixed(row)));
                over = Some((at, by, r));
            }
            _ => {
                let by = drag.map_or(0.0, |d| d.offset(i, now));
                under = under.push(if by == 0.0 {
                    r
                } else {
                    motion::transformed(r, Vector::new(0.0, by), 1.0, 0.0, 0.0)
                });
            }
        }
    }
    let Some((at, by, r)) = over else {
        return under.into();
    };
    // Laid over the list at its place, then moved: `Space` cannot be
    // negative, so the move is the widget's, not the room above it.
    let shadowed = container(r).style(|_| container::Style {
        shadow: Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    });
    let top = column![
        Space::new().height(Length::Fixed(at as f32 * row)),
        motion::transformed(shadowed, Vector::new(0.0, by), LIFT, 0.0, 0.0),
    ];
    stack![under, top].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: f32 = 20.0;

    /// Long enough for any of its springs to have come to rest.
    const SETTLED: Duration = Duration::from_secs(1);

    /// Let go while the pointer is moving, the row sets off into its place
    /// at the pointer's speed; let go after holding it still, from a
    /// standstill. Falsified by a settle that always set off from a
    /// standstill: the one thrown was no further along a moment later.
    #[test]
    fn a_row_let_go_moving_carries_on_at_the_pointer_s_speed() {
        let ms = Duration::from_millis;
        let at = Instant::now();
        let mut d = Reorder::press(0, 5, ROW, 100.0, at);
        for n in 1..=10u32 {
            // Down at 400 pixels a second: 4 pixels every 10 ms.
            d.drag(100.0 + 4.0 * n as f32, at + ms(10 * n as u64));
        }
        let up = at + ms(100);
        let thrown = match d.drop(up) {
            Drop::Moved { settle, .. } => settle,
            other => panic!("{other:?}"),
        };
        let put = match d.drop(up + ms(200)) {
            Drop::Moved { settle, .. } => settle,
            other => panic!("{other:?}"),
        };
        assert!((thrown.slide.speed(up) - 400.0).abs() < 1.0, "{}", thrown.slide.speed(up));
        assert_eq!(put.slide.speed(up + ms(200)), 0.0, "held still, it was put down");
        assert!(thrown.offset(up + ms(5)) > put.offset(up + ms(205)));
        assert_eq!(thrown.offset(up + SETTLED), 0.0);
    }

    /// Let go at a flick's speed, a row settles with its momentum, a hair
    /// past and back; slower, it was put down and lands critically damped;
    /// and whatever it was handed, it is home by the cap. Falsified by one
    /// damping for every release (0.85 of old) or no cap.
    #[test]
    fn a_flick_settles_with_momentum_and_a_put_down_without() {
        use crate::motion_tokens as tokens;
        assert_eq!(settling(tokens::FLICK + 300.0).damping, tokens::MOMENTUM);
        assert_eq!(settling(-(tokens::FLICK + 300.0)).damping, tokens::MOMENTUM, "up as well as down");
        assert_eq!(settling(tokens::FLICK - 100.0).damping, tokens::MOVE);
        let at = Instant::now();
        let settle = Settle {
            to: 0,
            since: at,
            slide: Slide::new(-400.0, 0.0, at, settling(9000.0)).moving_at(9000.0),
        };
        assert!(settle.moving(at));
        assert_eq!(settle.offset(at + motion::CAP), 0.0, "home by the cap");
        assert!(!settle.moving(at + motion::CAP + motion::LAST_FRAME));
    }

    /// A press that moves less than the threshold is a click on that row,
    /// and nothing lifts. Falsified by a threshold of 0: a hand that shook
    /// a pixel dropped the row where it was instead of playing it.
    #[test]
    fn a_press_that_hardly_moves_is_a_click() {
        let at = Instant::now();
        let mut d = Reorder::press(2, 5, ROW, 100.0, at);
        d.drag(102.0, at);
        assert!(!d.dragging());
        assert_eq!(d.drop(at), Drop::Click(2));
    }

    /// Dragged down a row and a half, the row lands two places down, and
    /// the two it passed slide up a row each while the rest stay; the drop
    /// says from where to where. Falsified by truncating rather than
    /// rounding the landing: it landed one place down.
    #[test]
    fn the_rows_passed_slide_out_of_the_way() {
        let at = Instant::now();
        let mut d = Reorder::press(1, 5, ROW, 100.0, at);
        d.drag(100.0 + 1.6 * ROW, at);
        assert_eq!(d.to(), 3);
        let later = at + SETTLED;
        assert_eq!(d.offset(0, later), 0.0);
        assert_eq!(d.offset(2, later), -ROW);
        assert_eq!(d.offset(3, later), -ROW);
        assert_eq!(d.offset(4, later), 0.0);
        assert!(d.moving(at + SETTLED / 10));
        assert!(!d.moving(later + crate::motion::LAST_FRAME));
        match d.drop(later) {
            Drop::Moved { from: 1, to: 3, settle } => {
                // Let go 0.4 of a row above its new place, and on its way.
                assert!((settle.offset(later) + 0.4 * ROW).abs() < 1e-3);
                assert_eq!(settle.offset(later + SETTLED), 0.0);
            }
            other => panic!("{other:?}"),
        }
    }

    /// Dragged past the top, it stops at the top; the rows above slide down.
    #[test]
    fn it_does_not_leave_the_list() {
        let at = Instant::now();
        let mut d = Reorder::press(2, 5, ROW, 100.0, at);
        d.drag(-500.0, at);
        assert_eq!(d.to(), 0);
        assert_eq!(d.lift(), -2.0 * ROW);
        assert_eq!(d.offset(0, at + SETTLED), ROW);
        assert_eq!(d.offset(1, at + SETTLED), ROW);
        assert_eq!(moved(&["a", "b", "c", "d"], 2, 0), ["c", "a", "b", "d"]);
    }
}
