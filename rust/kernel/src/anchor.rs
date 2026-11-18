//! A position that moves on its own, as playback does: its owner sets it
//! whole when it starts, stops, seeks or changes speed, and every view
//! extrapolates it from there, so nothing streams while it plays.
//!
//! ```text
//! Anchor { at, wall, rate, playing }
//! ```
//!
//! `at` is the position in milliseconds at the owner's wall time `wall`,
//! and `rate` how fast it moves, in thousandths (1000 is real time). An
//! anchor is a session row like any other: the owner's player writes it,
//! its other nodes take it as a session entry, and a node's views read it
//! from its session space ([`crate::Node::take_changes`]).
//!
//! Times are the owner's clock: a host whose clock may differ from its
//! owner's corrects `now` by the offset it measures before asking.

use alloc::vec::Vec;

use crate::codec::{Decode, Encode, Error, Reader};

/// Where a position was at a wall time, and how it moves from there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Anchor {
    /// The position at `wall`, in milliseconds.
    pub at: u64,
    /// The owner's wall time of `at`, in milliseconds.
    pub wall: u64,
    /// How fast it moves while playing, in thousandths of real time.
    pub rate: u16,
    pub playing: bool,
}

impl Anchor {
    /// The position at wall time `now`: still while paused, and never
    /// before `at`. It moves for at most 49 days from `wall`.
    pub fn position(&self, now: u64) -> u64 {
        if !self.playing {
            return self.at;
        }
        // In 32 bits, which a Cortex-M3 divides: whole seconds, then the
        // rest.
        let e = now.saturating_sub(self.wall).min(u64::from(u32::MAX)) as u32;
        let rate = u32::from(self.rate);
        let moved = u64::from(e / 1000) * u64::from(rate) + u64::from(e % 1000 * rate / 1000);
        self.at.saturating_add(moved)
    }

    /// When the position next reaches a multiple of `every` milliseconds
    /// after `now`: when a view showing it in steps of `every` next
    /// changes, for the host to wake then. `None` while it is still, or
    /// past 49 days.
    pub fn next(&self, now: u64, every: u32) -> Option<u64> {
        if !self.playing || self.rate == 0 || every == 0 {
            return None;
        }
        let p = u32::try_from(self.position(now)).ok()?;
        let to = (p / every + 1).checked_mul(every)?;
        // Long enough, as the position moves at least this fast; then
        // back to the first millisecond it is there.
        let mut t = (to - p).saturating_mul(1000).div_ceil(u32::from(self.rate));
        while t > 1 && self.position(now + u64::from(t - 1)) >= u64::from(to) {
            t -= 1;
        }
        Some(now + u64::from(t))
    }
}

impl Encode for Anchor {
    fn encode(&self, out: &mut Vec<u8>) {
        self.at.encode(out);
        self.wall.encode(out);
        self.rate.encode(out);
        self.playing.encode(out);
    }
}

impl Decode for Anchor {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Anchor {
            at: u64::decode(r)?,
            wall: u64::decode(r)?,
            rate: u16::decode(r)?,
            playing: bool::decode(r)?,
        })
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::codec::{from_slice, to_vec};
    use crate::Rng;

    #[cfg_attr(test, test)]
    fn an_anchor_moves_on_its_own() {
        let a = Anchor {
            at: 30_000,
            wall: 1_000_000,
            rate: 1000,
            playing: true,
        };
        assert_eq!(a.position(1_000_000), 30_000);
        assert_eq!(a.position(1_002_500), 32_500);
        // A clock behind the anchor's never shows it before `at`.
        assert_eq!(a.position(999_000), 30_000);
        let fast = Anchor { rate: 1500, ..a };
        assert_eq!(fast.position(1_002_000), 33_000);
        let paused = Anchor { playing: false, ..a };
        assert_eq!((paused.position(9_999_999), paused.next(1_000_000, 1000)), (30_000, None));
        // The next whole second: at 32.5 s it is 33 s, half a second on;
        // at one and a half times, a third of a second on.
        assert_eq!(a.next(1_002_500, 1000), Some(1_003_000));
        assert_eq!(fast.next(1_002_000, 1000), Some(1_002_667));
        assert_eq!(fast.position(1_002_667), 34_000);
        assert_eq!(from_slice(&to_vec(&fast)), Ok(fast));
    }

    /// At random, the time `next` answers is the first at which the
    /// position, in steps of `every`, is past what it was at `now`.
    #[cfg_attr(test, test)]
    fn next_is_when_the_step_changes() {
        let mut rng = Rng::new(5);
        for _ in 0..500 {
            let a = Anchor {
                at: rng.below(1 << 20),
                wall: rng.below(1 << 20),
                rate: 1 + rng.below(3000) as u16,
                playing: true,
            };
            let every = 1 + rng.below(2000) as u32;
            let now = a.wall + rng.below(1 << 16);
            let t = a.next(now, every).unwrap();
            assert!(t > now);
            let step = |t| a.position(t) / u64::from(every);
            assert!(step(t) > step(now));
            assert_eq!(step(t - 1), step(now));
        }
    }

    crate::listed!(an_anchor_moves_on_its_own, next_is_when_the_step_changes);
}
