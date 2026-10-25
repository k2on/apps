//! Two fingers sideways across a trackpad, as back and forward.
//!
//! A browser goes back on that swipe by itself — but only over a page that
//! lets the wheel through, and a canvas app takes every wheel event, so the
//! gesture never reaches the browser. On the desktop there is no browser to
//! do it at all. So the app sees the wheel itself and says when the motion
//! was a swipe: far enough sideways, and mostly sideways, in one gesture.
//!
//! **One gesture, one step.** A trackpad keeps sending as the fingers coast
//! (the momentum the system adds), so once a swipe has stepped the rest of
//! that gesture is spent; a pause of [`QUIET`] ends it.
use iced::mouse::ScrollDelta;
use iced::time::{Duration, Instant};

/// How far sideways, in pixels, a gesture travels before it is a swipe.
pub const FAR: f32 = 140.0;

/// How long the wheel is still before the next motion is a new gesture.
pub const QUIET: Duration = Duration::from_millis(300);

/// A line of a wheel that turns in lines, in pixels, as a browser counts it.
const LINE: f32 = 40.0;

/// One gesture's travel so far.
#[derive(Debug, Default)]
pub struct Swipe {
    x: f32,
    y: f32,
    last: Option<Instant>,
    spent: bool,
}

impl Swipe {
    /// Feed one wheel event. `Some(-1)` is back and `Some(1)` forward, once
    /// per gesture: fingers to the right are back, as a browser has it.
    pub fn feed(&mut self, delta: ScrollDelta, now: Instant) -> Option<isize> {
        let (dx, dy) = match delta {
            ScrollDelta::Pixels { x, y } => (x, y),
            ScrollDelta::Lines { x, y } => (x * LINE, y * LINE),
        };
        if self.last.is_none_or(|t| now.duration_since(t) > QUIET) {
            *self = Swipe::default();
        }
        self.last = Some(now);
        self.x += dx;
        self.y += dy.abs();
        if self.spent || self.x.abs() < FAR || self.x.abs() < 2.0 * self.y {
            return None;
        }
        self.spent = true;
        Some(if self.x > 0.0 { -1 } else { 1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(x: f32, y: f32) -> ScrollDelta {
        ScrollDelta::Pixels { x, y }
    }

    /// A swipe steps once however long it coasts; a pause makes the next
    /// one new; a scroll that is mostly down is a scroll. Falsified by
    /// dropping `spent`: the coasting stepped a second time.
    #[test]
    fn one_gesture_is_one_step() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let mut s = Swipe::default();
        assert_eq!(s.feed(px(80.0, 0.0), ms(0)), None, "not far enough yet");
        assert_eq!(s.feed(px(80.0, 5.0), ms(16)), Some(-1), "fingers right: back");
        for i in 0..20 {
            assert_eq!(s.feed(px(60.0, 0.0), ms(32 + i * 16)), None, "coasting");
        }
        assert_eq!(s.feed(px(-200.0, 0.0), ms(2000)), Some(1), "a new gesture, the other way");

        let mut down = Swipe::default();
        assert_eq!(down.feed(px(150.0, 120.0), ms(0)), None, "mostly down is a scroll");
    }
}
