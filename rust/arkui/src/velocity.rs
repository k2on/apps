//! How fast a drag was going when it was let go, read from its last moves
//! rather than its last one: a trackpad's deltas come unevenly, and two
//! that land in one frame make any one of them a lie.
//!
//! The moves are kept with when they came; the speed is the slope of a
//! straight line fitted through the newest of them by least squares, the
//! newer weighed more. Only the last [`HORIZON`] counts, and a pause of
//! [`GAP`] between two moves cuts off everything before it: the drag had
//! stopped there, and only what came after it is how it was let go.
//!
//! Ported from fluid-scroll's `VelocityTracker` (Copyright 2023 ktiays,
//! Apache License 2.0, `LICENSE.fluid-scroll` beside this file), itself
//! after UIScrollView's and Android's: its history of twenty, its horizon
//! of a tenth of a second and its 40 ms "assume the pointer stopped" —
//! written here as the `velocity-ms` and `velocity-gap-ms` tokens. Changed
//! from it: the gap is read between the moves' times (fluid-scroll's
//! `Lsq2` compares it with the distance between them), and the fit is a
//! weighted line rather than a parabola, whose slope at the newest move
//! swings with the timing noise of events read as they are handled.
use iced::time::{Duration, Instant};

use crate::motion_tokens as tokens;

/// How many moves are kept.
const HISTORY: usize = 20;

/// How far back the speed is read ([`tokens::VELOCITY_MS`]).
pub const HORIZON: Duration = Duration::from_millis(tokens::VELOCITY_MS);

/// A pause between two moves this long means the drag had stopped
/// ([`tokens::VELOCITY_GAP_MS`]).
pub const GAP: Duration = Duration::from_millis(tokens::VELOCITY_GAP_MS);

/// A drag's last moves: where it had got to, all told, and when.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Velocity {
    samples: [Option<(Instant, f32)>; HISTORY],
    /// Where the newest is.
    newest: usize,
}

impl Velocity {
    /// Had got to `at` (its moves added up) by `now`.
    pub fn add(&mut self, now: Instant, at: f32) {
        self.newest = (self.newest + 1) % HISTORY;
        self.samples[self.newest] = Some((now, at));
    }

    /// Nothing kept: a new drag.
    pub fn clear(&mut self) {
        *self = Velocity::default();
    }

    /// When the newest move came.
    pub fn last(&self) -> Option<Instant> {
        self.samples[self.newest].map(|(t, _)| t)
    }

    /// How fast it was going at its newest move, a second: the weighted
    /// slope through the moves within [`HORIZON`] of it and after the last
    /// [`GAP`]. None, or all at one moment, is still.
    pub fn speed(&self) -> f32 {
        let Some((newest, _)) = self.samples[self.newest] else {
            return 0.0;
        };
        // Newest first, back to the horizon or a pause.
        let mut points: Vec<(f64, f64)> = Vec::with_capacity(HISTORY);
        let mut later = newest;
        for n in 0..HISTORY {
            let Some((t, at)) = self.samples[(self.newest + HISTORY - n) % HISTORY] else {
                break;
            };
            if newest.saturating_duration_since(t) > HORIZON || later.saturating_duration_since(t) > GAP {
                break;
            }
            later = t;
            points.push((-(newest.saturating_duration_since(t).as_secs_f64()), at as f64));
        }
        if points.len() < 2 {
            return 0.0;
        }
        // Weighed from 1 at the newest down to a quarter at the horizon.
        let horizon = HORIZON.as_secs_f64();
        let (mut sw, mut st, mut sx) = (0.0, 0.0, 0.0);
        let weighed: Vec<(f64, f64, f64)> = points.iter().map(|&(t, x)| (1.0 + 0.75 * t / horizon, t, x)).collect();
        for &(w, t, x) in &weighed {
            sw += w;
            st += w * t;
            sx += w * x;
        }
        let (mt, mx) = (st / sw, sx / sw);
        let (mut stt, mut stx) = (0.0, 0.0);
        for &(w, t, x) in &weighed {
            stt += w * (t - mt) * (t - mt);
            stx += w * (t - mt) * (x - mx);
        }
        // All at one moment: nothing to read a speed from.
        if stt < 1e-9 {
            return 0.0;
        }
        (stx / stt) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A steady drag reads its speed exactly, from any cadence; one that
    /// slows reads nearer its end than its start; moves before a pause, or
    /// before the horizon, do not count; and moves all at one moment read
    /// as still. Falsified by the last move alone (two events in one frame
    /// read as a thousand times the speed), or by a gap read between
    /// positions as fluid-scroll's `Lsq2` has it (the pause was kept).
    #[test]
    fn the_speed_is_read_from_the_last_moves() {
        let t = Instant::now();
        let mut steady = Velocity::default();
        for n in 0..12 {
            steady.add(t + ms(8 * n), 8.0 * n as f32 * 1.5);
        }
        assert!((steady.speed() - 1500.0).abs() < 1.0, "{}", steady.speed());

        // Uneven: two moves in one frame, then a long one.
        let mut uneven = Velocity::default();
        let mut at = 0.0;
        for (n, dt) in [8u64, 8, 0, 16, 8, 1, 15, 8].iter().enumerate() {
            let when = t + ms([8u64, 8, 0, 16, 8, 1, 15, 8][..=n].iter().sum::<u64>());
            at += *dt as f32 * 2.0;
            uneven.add(when, at);
        }
        assert!((uneven.speed() - 2000.0).abs() < 150.0, "{}", uneven.speed());

        // Slowing from 3000 to 1000 a second over 80 ms: nearer the end.
        let mut slowing = Velocity::default();
        let mut at = 0.0;
        for n in 0..=10u64 {
            let v = 3000.0 - 200.0 * n as f32;
            at += v * 0.008;
            slowing.add(t + ms(8 * n), at);
        }
        let read = slowing.speed();
        assert!(read > 1000.0 && read < 2000.0, "{read}");

        // A pause: what came before it does not count.
        let mut paused = Velocity::default();
        for n in 0..5 {
            paused.add(t + ms(8 * n), 40.0 * n as f32);
        }
        paused.add(t + ms(32 + 60), 160.0);
        assert_eq!(paused.speed(), 0.0, "one move after a pause is no speed");
        paused.add(t + ms(32 + 68), 168.0);
        assert!((paused.speed() - 1000.0).abs() < 1.0, "{}", paused.speed());

        let mut together = Velocity::default();
        together.add(t, 0.0);
        together.add(t, 30.0);
        assert_eq!(together.speed(), 0.0);
        assert_eq!(Velocity::default().speed(), 0.0);
    }
}
