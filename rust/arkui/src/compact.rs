//! A page's header, small, at the top of the window once the header itself
//! has scrolled away: iOS's large title, which a list scrolled up hands to
//! the bar it scrolls under. Here the bar is the scroll edge's band
//! ([`crate::edge`]), where the account's face floats; once a page's own
//! header — an album's cover and name, an artist's face, Settings — is out
//! of sight, the band says whose page this is with a small picture of the
//! same art and the name on one line, cut to fit. Pressed, it takes the
//! page back to its top, as a tap on iOS's status bar does.
//!
//! **It comes from where the header went.** The header leaves upward with
//! the page, so the small one rises into the band — [`RISE`], a few pixels
//! and no more — as it fades in, and sinks back the way it came as it
//! goes, when the header comes back down into view. The picture and the
//! name are one thing and move as one: no stagger between them, one
//! entrance for the whole. It moves by a transform and fades under a veil
//! of the page's ground (iced has no opacity for a group; the band is that
//! ground once the page has scrolled this far), so nothing is laid out
//! again as it comes.
//!
//! **On a spring, from the scroll.** A window hands [`Compact::heard`] how
//! far its page is scrolled and how far down its header ends, after every
//! update; past it, the small header comes, and back above it, goes — on
//! a critically damped spring ([`SPRING`]), so scrolled back while it is
//! still coming it turns where it is, as fast as it was going, rather than
//! starting again. It is seen tens of times a day, so it is quick and
//! slight: under a quarter of a second, a few pixels.
//!
//! **Not for the keys, and not for less motion.** A page scrolled by the
//! keyboard (`j`, `G`, a page down) is someone moving fast through a list
//! many times a day: there it is simply there or not, as it is where the
//! system asks for less motion ([`crate::motion::reduced`]).
//!
//! **At rest there is nothing.** A page at its top has its header in view,
//! so nothing is drawn, and nothing ticks.
use iced::time::Instant;
use iced::widget::{button, row, Space};
use iced::{Alignment, Element, Length, Vector};

use crate::motion::{self, Slide, Spring};
use crate::{fit, line, style};

/// What it comes and goes on: critically damped and quick, so it is in
/// place in under a quarter of a second and never overshoots the band.
pub const SPRING: Spring = Spring {
    damping: 1.0,
    response: 0.25,
};

/// How far below its place it starts as it comes in: a few pixels.
pub const RISE: f32 = 4.0;

/// The picture beside the name.
pub const ART: f32 = 28.0;

/// The name's size: a heading's, not the page's title.
pub const SIZE: f32 = 15.0;

/// How near all the way in or out is at rest: a thousandth.
const REST: f32 = 1e-3;

/// Whether the small header is up, and how far: 0 gone to 1 in place.
#[derive(Clone, Copy, Debug)]
pub struct Compact {
    slide: Slide,
}

impl Compact {
    /// Hidden, as a page at its top has it.
    pub fn new(now: Instant) -> Compact {
        Compact {
            slide: Slide::still(0.0, now).resting_within(REST),
        }
    }

    /// A page scrolled `scrolled` down whose header ends `after` down (no
    /// header: `None`): in once the header is out of sight, out once it is
    /// back, from wherever it is. `keyed`: the scroll came from the
    /// keyboard, and it is there or not at once.
    pub fn heard(&mut self, scrolled: f32, after: Option<f32>, keyed: bool, now: Instant) {
        let to = match after.is_some_and(|after| scrolled > after) {
            true => 1.0,
            false => 0.0,
        };
        if self.slide.target() == to {
            return;
        }
        match keyed || motion::reduced() {
            true => self.slide = Slide::still(to, now).resting_within(REST),
            false => self.slide.toward(to, now, SPRING),
        }
    }

    /// At once hidden, without going: a page left for another, whose small
    /// header is not the new page's.
    pub fn reset(&mut self, now: Instant) {
        *self = Compact::new(now);
    }

    /// How far in it is at `now`, 0 to 1.
    pub fn at(&self, now: Instant) -> f32 {
        self.slide.at(now).clamp(0.0, 1.0)
    }

    /// Whether a frame is still to be drawn for it: the app's frame clock
    /// runs while this says so.
    pub fn moving(&self, now: Instant) -> bool {
        self.slide.moving(now)
    }

    /// Whether there is anything to draw.
    pub fn showing(&self, now: Instant) -> bool {
        self.at(now) > 0.0
    }
}

/// The small header, `open` of the way in: `art` (a picture [`ART`] tall,
/// the header's own) and `name` bold on one line, cut at its end to fit,
/// as a button that sends `top` — back to the page's top. Risen [`RISE`]
/// short of its place and veiled by what it has still to come.
pub fn view<'a, M: Clone + 'a>(art: Option<Element<'a, M>>, name: &str, open: f32, top: M) -> Element<'a, M> {
    let name = line::Line::new(name.to_string(), SIZE, fit::Cut::End, |t| crate::theme::of(t).background.base.text)
        .font(crate::hero::BOLD)
        .tracking(line::tracking(SIZE))
        .gives_way();
    let mut body = row![].spacing(8).align_y(Alignment::Center).height(Length::Fixed(ART));
    if let Some(art) = art {
        body = body.push(art);
    }
    let body = body.push(name).push(Space::new().width(Length::Fixed(2.0)));
    let pressed = button(body).padding([0, 6]).style(style::bare).on_press(top);
    let open = open.clamp(0.0, 1.0);
    motion::transformed(pressed, Vector::new(0.0, RISE * (1.0 - open)), 1.0, 1.0 - open, 6.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Hidden at the top, coming once the header is past, in place in a
    /// moment, and going again once it is back; at once from the keys;
    /// with no header, never. Falsified by showing on any scroll (a page
    /// moved a pixel had it), and by animating a keyed scroll (`G` drew it
    /// part of the way in).
    #[test]
    fn it_comes_once_the_header_is_gone_and_goes_when_it_is_back() {
        let t0 = Instant::now();
        let mut c = Compact::new(t0);
        assert!(!c.showing(t0), "at rest, nothing");
        assert!(!c.moving(t0), "and nothing ticks");
        c.heard(40.0, Some(120.0), false, t0);
        assert!(!c.showing(t0), "the header still in sight");
        c.heard(160.0, Some(120.0), false, t0);
        let mid = c.at(t0 + Duration::from_millis(60));
        assert!(mid > 0.0 && mid < 1.0, "on its way: {mid}");
        let later = t0 + Duration::from_secs(1);
        assert_eq!(c.at(later), 1.0, "all the way in");
        c.heard(10.0, Some(120.0), false, later);
        assert_eq!(c.at(later + Duration::from_secs(1)), 0.0, "gone again");
        let keyed = later + Duration::from_secs(2);
        c.heard(900.0, Some(120.0), true, keyed);
        assert_eq!(c.at(keyed), 1.0, "from the keys: there at once");
        c.heard(0.0, Some(120.0), true, keyed);
        assert_eq!(c.at(keyed), 0.0, "and gone at once");
        c.heard(500.0, None, false, keyed);
        assert!(!c.showing(keyed + Duration::from_secs(1)), "no header, nothing to stand for it");
    }
}
