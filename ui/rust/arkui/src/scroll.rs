//! Keeping a keyboard cursor on screen, as vim keeps its cursor line: the
//! list does not move while the cursor stays in view, and when the cursor
//! would leave it the list moves only as far as keeps it in, a margin of
//! [`SCROLLOFF`] rows short of the edge (vim's `scrolloff`). `zz`, `zt` and
//! `zb` put the cursor's row in the middle, at the top or at the foot
//! ([`Place`]).
//!
//! Where a row is is worked out from the scrollable's own geometry, read by
//! an operation when the task runs ([`follow`]): its height, how tall what
//! it scrolls is, and how far it is scrolled. Rows are taken to be one
//! height, so row `n` of `rows` starts `n / rows` of the way down: close
//! enough to keep it in view without the widgets reporting where each row
//! was drawn.
//!
//! **A trackpad scrolls as Firefox scrolls it** ([`elastic`]). Let go
//! while moving, a list coasts on and slows as a scroll does — its speed
//! read from the fingers' last moves ([`crate::velocity`]), kept
//! [`crate::motion_tokens::DECELERATION`] a millisecond, which is Gecko's
//! desktop fling — and stops of itself; fingers landing on the trackpad
//! (Wayland's hold gesture, [`crate::touchpad`]), any word from the wheel,
//! a click or a key stop it at once. Pulled past its top or its foot it
//! goes on with Gecko's resistance ([`band`]): each pixel more of pull is
//! drawn less far than the last, so it is harder and harder to pull and
//! never stops. It springs back when the fingers lift, on Firefox's
//! overscroll spring ([`BACK`]), to the edge and no further; held still
//! with the fingers down it stays where it is. A coast that reaches an end
//! carries on into the band and springs back the same way. The coast
//! scrolls the scrollable as more of the wheel would; the band is only
//! drawn, and the scroll is never moved past its end: [`follow`], the
//! scroll edge and the small header read the offset they always did.
//!
//! The band, the spring and the fling re-implement Gecko's (Mozilla's
//! `gfx/layers/apz`, MPL-2.0: `Axis::ApplyResistance`,
//! `Axis::StartOverscrollAnimation`, `AxisPhysicsMSDModel`,
//! `DesktopFlingPhysics`), with its constants in `ui/theme.toml`;
//! none of its code is copied.
use std::sync::atomic::{AtomicBool, Ordering};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::operation::{Operation, Scrollable};
use iced::advanced::widget::{self, tree, Id, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::mouse::ScrollDelta;
use iced::time::{Duration, Instant};
use iced::widget::operation::{snap_to, RelativeOffset};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{keyboard, window, Element, Event, Length, Point, Rectangle, Size, Task, Theme, Transformation, Vector};

use crate::motion::{self, Slide, Spring};
use crate::motion_tokens as tokens;
use crate::velocity::Velocity;
use crate::vim::Grid;

/// How many rows short of a scrollable's edge the cursor's row is kept.
pub const SCROLLOFF: f32 = 2.0;

/// Where the cursor's row is to be in its scrollable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Wherever it is, while it is in view: the list moves only when it
    /// would leave, and only as far as keeps it [`SCROLLOFF`] rows in.
    Near,
    /// `zz`: in the middle.
    Middle,
    /// `zt`: at the top, [`SCROLLOFF`] rows down.
    Top,
    /// `zb`: at the foot, [`SCROLLOFF`] rows up.
    Bottom,
}

/// Where a scrollable `viewport` tall over `content` scrolled to `offset`
/// is to be scrolled so row `row` of `rows` is where `place` says, or
/// `None` when it is to stay where it is.
pub fn offset_for(row: usize, rows: usize, viewport: f32, content: f32, offset: f32, place: Place) -> Option<f32> {
    if rows == 0 || content <= viewport {
        return None;
    }
    let height = content / rows as f32;
    let top = row.min(rows - 1) as f32 * height;
    let bottom = top + height;
    // The margin, but never so much that a row cannot be kept inside it.
    let margin = (SCROLLOFF * height).min(((viewport - height) / 2.0).max(0.0));
    let wanted = match place {
        Place::Near if top - margin < offset => top - margin,
        Place::Near if bottom + margin > offset + viewport => bottom + margin - viewport,
        Place::Near => return None,
        Place::Middle => top + height / 2.0 - viewport / 2.0,
        Place::Top => top - margin,
        Place::Bottom => bottom + margin - viewport,
    };
    let wanted = wanted.clamp(0.0, content - viewport);
    ((wanted - offset).abs() >= 0.5).then_some(wanted)
}

/// Scroll the scrollable `id` so row `row` of its `rows` is where `place`
/// says ([`offset_for`]), from where it is scrolled when the task runs.
pub fn follow<M: Send + 'static>(id: &'static str, row: usize, rows: usize, place: Place) -> Task<M> {
    struct Follow {
        target: Id,
        row: usize,
        rows: usize,
        place: Place,
    }
    impl<T> Operation<T> for Follow {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }
        fn scrollable(&mut self, id: Option<&Id>, bounds: Rectangle, content: Rectangle, translation: Vector, state: &mut dyn Scrollable) {
            if id != Some(&self.target) {
                return;
            }
            if let Some(y) = offset_for(self.row, self.rows, bounds.height, content.height, translation.y, self.place) {
                state.scroll_to(AbsoluteOffset { x: None, y: Some(y) });
            }
        }
    }
    iced::advanced::widget::operate(Follow {
        target: Id::new(id),
        row,
        rows,
        place,
    })
}

/// Scroll the scrollable `id` so cell `at` of `grid` is where `place` says:
/// its row of the grid's rows ([`follow`]).
pub fn reveal<M: Send + 'static>(id: &'static str, grid: Grid, at: usize, place: Place) -> Task<M> {
    let columns = grid.columns.max(1);
    follow(id, at / columns, grid.cells.div_ceil(columns), place)
}

/// Scroll the scrollable `id` to `y` of the way down, 0 to 1: where a
/// [`crate::vim::Page`] says its cursor sits.
pub fn reveal_to<M: Send + 'static>(id: &'static str, y: f32) -> Task<M> {
    snap_to(id, RelativeOffset { x: Some(0.0), y: Some(y) })
}

/// Scroll the scrollable `id` so row `at` of `rows` is in the middle of
/// it, where `seen` rows fit: what a list that moves by itself — a
/// transcript's line being said — keeps its row at, with as much to come
/// in sight as gone.
///
/// Uniform rows, as [`reveal`] has them: the offset that puts row `at`'s
/// middle at the middle of the view is its distance from there over how
/// far the list scrolls, and a list that fits does not scroll.
pub fn centre<M: Send + 'static>(id: &'static str, at: usize, rows: usize, seen: f32) -> Task<M> {
    reveal_to(id, centred(at, rows, seen))
}

/// Where [`centre`] scrolls to, 0 to 1.
pub fn centred(at: usize, rows: usize, seen: f32) -> f32 {
    let travel = rows as f32 - seen;
    match travel > 0.0 {
        true => ((at as f32 + 0.5 - seen / 2.0) / travel).clamp(0.0, 1.0),
        false => 0.0,
    }
}

/// What a band past an end springs back on: Firefox's overscroll spring
/// (Gecko's `apz.overscroll.spring_stiffness` 200 and `.damping` 1.1 on a
/// unit mass, [`tokens::BAND_STIFFNESS`], [`tokens::BAND_DAMPING`]), a
/// touch over critically damped, so it comes to the edge without passing
/// it.
pub const BACK: Spring = Spring {
    damping: tokens::BAND_DAMPING,
    response: tokens::BAND_RESPONSE,
};

/// A coast slower than this, a second, has stopped ([`tokens::COAST_STOP`],
/// Gecko's `apz.fling_stopped_threshold`).
pub const STOP: f32 = tokens::COAST_STOP;

/// A release slower than this, a second, does not coast, and a band let go
/// slower springs back from a standstill ([`tokens::COAST_MIN`], Gecko's
/// `apz.fling_min_velocity_threshold`).
pub const START: f32 = tokens::COAST_MIN;

/// A lift seen only by its stillness, slower than this share of the
/// gesture's fastest, ran down by itself first — a browser's own coast
/// arriving as more of the wheel (Chrome on Windows or Wayland, Safari) —
/// and coasts no further ([`tokens::COAST_SPENT`]).
pub const SPENT: f32 = tokens::COAST_SPENT;

/// Where the system never says the fingers lifted (X11, a browser), a
/// trackpad still this long has been let go ([`tokens::LIFT_MS`]).
pub const LIFT: Duration = Duration::from_millis(tokens::LIFT_MS);

/// The same while it is pulled past an end ([`tokens::BAND_LIFT_MS`]):
/// longer, so a band held still under the fingers is not let go, at the
/// cost of a band let go waiting this long to spring back. Where the
/// system says when fingers lift (Wayland, macOS) neither is waited for.
pub const BAND_LIFT: Duration = Duration::from_millis(tokens::BAND_LIFT_MS);

/// The shortest the scrollbar's thumb is drawn while a band squeezes it:
/// about as long as it is wide (iced's scrollbar is 10 px wide).
pub const THUMB_MIN: f32 = 10.0;

/// Whether the system coasts a trackpad itself once the fingers lift,
/// sending its coast on as more of the wheel: macOS does (its momentum
/// phase), and so does a browser on a Mac, which hands a page the
/// system's coast as more `wheel` events. There the coast is followed,
/// never made, so it is not doubled — and it stops when the fingers land
/// because the system's stops. Everywhere else — Wayland, which only says
/// the fingers lifted, X11 and a browser elsewhere, which say nothing — it
/// is made here.
pub fn system_coasts() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// The system has said when fingers lift from a trackpad (a wheel event
/// that moved nowhere: Wayland's `axis_stop`, macOS's end of a phase), so
/// on the desktop only that lets go. Learned the first time it happens, for
/// every scrollable: it is the system's, not one list's. Never in a
/// browser, where a wheel that moves nowhere is a lift when it comes but
/// may not come every time.
static TOLD: AtomicBool = AtomicBool::new(false);

/// Whether only the system's word lets the fingers go, never stillness:
/// on macOS, on Wayland once [`crate::touchpad::listen`] has seen the
/// window's display, or once the desktop has said so ([`TOLD`]).
fn told() -> bool {
    crate::touchpad::says_lift() || TOLD.load(Ordering::Relaxed)
}

/// How many pixels a band is drawn for each pixel of pull at the edge:
/// Gecko's resistance, `1 / band-resistance` of each of its pixels, and
/// `band-gain` of them in each of a trackpad's ([`tokens::BAND_GAIN`]).
fn give_at_edge() -> f32 {
    tokens::BAND_GAIN / tokens::BAND_RESISTANCE
}

/// How far a band pulled `pull` pixels past an end from the edge, in a
/// view `view` tall, is drawn past it: Firefox's resistance (Gecko's
/// `Axis::ApplyResistance`, MPL-2.0, re-implemented), in which each pixel
/// more of pull is drawn `(1 − |band| / view) · k` further, `k`
/// [`give_at_edge`]. Integrated, that is `view · (1 − e^(−k · pull / view))`:
/// a quarter of a pixel for each pixel at first, ever less after, and
/// always some — harder and harder, never a wall. No more than the view,
/// which no pull reaches.
pub fn band(pull: f32, view: f32) -> f32 {
    stretch(0.0, pull, view, f32::INFINITY)
}

/// A band drawn `over` past an end, pulled `pull` pixels further out (the
/// same way, or from the edge): where it is drawn now. Gecko's resistance
/// integrated over the move rather than stepped, so a pull drawn in many
/// small moves lands where one move would, and a move draws at most `most`
/// more (Gecko holds each to 8 px, [`tokens::BAND_STEP`]).
fn stretch(over: f32, pull: f32, view: f32, most: f32) -> f32 {
    if view <= 0.0 || pull == 0.0 {
        return over;
    }
    let side = pull.signum();
    let at = (over * side).max(0.0);
    let room = (view - at).max(0.0);
    let more = -room * (-give_at_edge() * pull.abs() / view).exp_m1();
    side * (at + more.min(most))
}

/// The speed a band springs back set off at, for a release or a coast at
/// `speed`: [`tokens::BAND_THROW`] of it in Gecko's pixels
/// ([`tokens::BAND_GAIN`] of a trackpad's), never faster than
/// [`tokens::BAND_FLING_MAX`] (Gecko's `Axis::StartOverscrollAnimation`).
fn thrown(speed: f32) -> f32 {
    let most = tokens::BAND_FLING_MAX;
    (speed * tokens::BAND_GAIN * tokens::BAND_THROW).clamp(-most, most)
}

/// The scrollbar's thumb, as `(top, length)`, on a rail from `rail_top`
/// `rail` long beside a view `view` tall over `content`, scrolled to
/// `scrolled` (the true offset, never past an end) and drawn `overshoot`
/// past an end (positive past the top). At rest, iced's own thumb: as long
/// as the share of the content seen, as far down as the scroll. Banded it
/// stays at that end and is drawn shorter, as if the band were more
/// content — `length · view / (view + |overshoot|)`, so a band the size of
/// the view, which no pull reaches, would halve it — but never shorter than
/// [`THUMB_MIN`], as macOS and the iPhone draw it. `None` for content that
/// fits.
pub fn thumb(rail_top: f32, rail: f32, view: f32, content: f32, scrolled: f32, overshoot: f32) -> Option<(f32, f32)> {
    if content <= view || rail <= 0.0 {
        return None;
    }
    let length = (rail * view / content).max(2.0);
    let top = rail_top + scrolled * rail / content;
    if overshoot == 0.0 {
        return Some((top, length));
    }
    let short = (length * view / (view + overshoot.abs())).max(length.min(THUMB_MIN));
    Some(match overshoot > 0.0 {
        true => (rail_top, short),
        false => (rail_top + rail - short, short),
    })
}

/// `content` — a `scrollable`, or something holding one — scrolled by a
/// trackpad as Firefox scrolls one: let go while moving, it coasts on and
/// slows as a scroll does ([`motion::coast`], Gecko's desktop fling), and
/// pulled past its top or its foot it goes on with ever more resistance
/// ([`band`]) and springs back when the fingers lift, on [`BACK`], and
/// stops at the edge. A coast that reaches an end carries on into the band,
/// and springs back the same way. While it bands, the scrollbar's thumb
/// stays at that end and is drawn shorter ([`thumb`]).
///
/// Fingers that land stop it: two fingers resting on a Linux trackpad
/// stop a coast or a band springing back where it is, as Firefox's do
/// (Wayland's hold gesture, [`crate::touchpad`]); so does any word from the
/// wheel, even one that moved nowhere, a click, a touch or a key.
///
/// Only a trackpad's pixels coast or band ([`pulls`]): a wheel that turns
/// in lines steps and stops at the end as it always did. Where the system
/// asks for less motion ([`motion::reduced`]) nothing coasts or bands at
/// all. A key pressed puts a band back at once (keyboard motion is never
/// animated).
///
/// The coast scrolls the scrollable itself — as more of the wheel, so it
/// says where it is as ever — and the band is only drawn: the scroll stays
/// at its end while it shows, so [`follow`], `zz`/`zt`/`zb`, the scroll
/// edge ([`crate::edge`]) and the small header ([`crate::compact`]) read
/// the true offset. What is drawn moved is clipped to where the scrollable
/// is, so what it pulls away from shows what is behind it and nothing below
/// it is drawn over. The scrollbar is drawn here, unmoved, while it bands,
/// when the scrollable is styled with [`crate::style::bars`] (every one in
/// these apps is); another style's bar moves with the content.
///
/// It runs on the display's frames — it asks for the next one while it
/// moves, as [`motion::Clock::Frames`] does, and for none at rest.
///
/// The wheel belongs to the innermost scrollable under the pointer: with a
/// scrollable inside, the wheel over it is left to that one.
pub fn elastic<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Elastic { content: content.into() })
}

/// The wheel's motion, as a trackpad's pixels, if it coasts and bands at
/// all: never a wheel that turns in lines, and never where motion is
/// reduced.
pub fn pulls(delta: ScrollDelta, reduced: bool) -> Option<(f32, f32)> {
    match delta {
        ScrollDelta::Pixels { x, y } if !reduced => Some((x, y)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Phase {
    #[default]
    Idle,
    /// The fingers are on the trackpad, the band drawn [`Kinetic::over`]
    /// past an end (none while it scrolls).
    Down,
    /// Let go while moving: `speed` at `since`, and how much of its way has
    /// been handed on; `told` whether the system said the fingers lifted.
    /// A band it was let go from, inward, is given back first.
    Coasting { since: Instant, speed: f32, gone: f32, told: bool },
    /// The system's own coast ([`system_coasts`]): its wheel scrolls as it
    /// comes, and only an end is this one's.
    Following,
    /// Springing back to the edge, past the top (`side` 1) or the foot (−1).
    Settling { side: f32, back: Slide },
}

/// One scrollable's coast and band: whether the fingers are down, how far
/// past an end it is drawn, how it is coasting or springing back, and the
/// wheel's last moves, which say how fast it was let go. Pure, so it is
/// tested without a window; [`elastic`] feeds it.
///
/// The wheel is in iced's sense throughout: `y` positive moves the content
/// down, toward the top.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Kinetic {
    phase: Phase,
    /// How far past an end the band is drawn while the fingers are down or
    /// a coast gives it back: positive past the top (the content drawn
    /// lower), negative past the foot. Gecko keeps the band itself, not the
    /// pull, so pulled back it is given back pixel for pixel.
    over: f32,
    /// Fingers that came to rest stopped it ([`Kinetic::touch`]) and have
    /// not moved since: their lifting lets a band go.
    held: bool,
    /// How many times fingers have come to rest, as last felt
    /// ([`crate::touchpad::Fingers`]).
    holds: u64,
    /// The fingers' moves, added up, with when.
    moves: Velocity,
    travelled: f32,
    /// When the wheel last said anything, moved or not.
    heard: Option<Instant>,
    /// Which way it last moved.
    going: f32,
    /// The fastest the fingers went in this drag, a second.
    fastest: f32,
    /// The system's coast is still coming after a band it pulled sprang
    /// back, and is spent rather than pulling it out again.
    spending: bool,
    /// The frame last drawn, or the last event: when it is drawn at.
    now: Option<Instant>,
    /// Where the pointer was over it when the wheel last moved: where a
    /// coast's wheel is handed to the scrollable from.
    aim: Option<Point>,
    /// Where its scrollable and its scrollbar were last seen, from its own
    /// top left: what the thumb is drawn from while it bands.
    seen: Option<Seen>,
}

/// A scrollable as last seen: its bounds, from the [`elastic`]'s top
/// left, how tall what it scrolls is, and how far it is scrolled.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Seen {
    bounds: Rectangle,
    content: f32,
    scrolled: f32,
}

impl Kinetic {
    /// How far the content is drawn moved at `now`: down past the top, up
    /// past the foot, never across the edge.
    pub fn offset(&self, now: Instant) -> f32 {
        match self.phase {
            Phase::Settling { side, back } => {
                let at = back.at(now);
                match at * side > 0.0 {
                    true => at,
                    false => 0.0,
                }
            }
            _ => self.over,
        }
    }

    /// Whether anything moves or is drawn moved, or the fingers are down.
    pub fn active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Whether it is coasting by itself.
    pub fn coasting(&self) -> bool {
        matches!(self.phase, Phase::Coasting { .. })
    }

    /// Whether a band is springing back.
    pub fn settling(&self) -> bool {
        matches!(self.phase, Phase::Settling { .. })
    }

    /// Whether what comes now, `y`, is the system's coast going on
    /// ([`system_coasts`]): after a lift, the same way, unbroken.
    fn system_coast(&self, y: f32, now: Instant) -> bool {
        system_coasts()
            && y.signum() == self.going
            && self.heard.is_some_and(|t| now.saturating_duration_since(t) <= LIFT)
            && match self.phase {
                Phase::Following => true,
                Phase::Settling { .. } => self.spending,
                _ => false,
            }
    }

    /// `y` of the wheel's motion, or a coast's, given to the band first:
    /// further out it stretches ([`stretch`]); back in it is given back
    /// pixel for pixel, as Gecko's `Axis::AdjustDisplacement` gives an
    /// overscroll back before it scrolls. What is left is the scroll's.
    fn give(&mut self, y: f32, view: f32) -> f32 {
        if self.over == 0.0 || y == 0.0 {
            return y;
        }
        if y * self.over > 0.0 {
            self.over = stretch(self.over, y, view, tokens::BAND_STEP);
            return 0.0;
        }
        let to = self.over + y;
        match to * self.over > 0.0 {
            true => {
                self.over = to;
                0.0
            }
            false => {
                self.over = 0.0;
                to
            }
        }
    }

    /// The wheel moved `y` (a trackpad's pixels) at `now`, over a scrollable
    /// in a view `view` tall: how much of it is for the scrollable to
    /// scroll. The fingers are down: a coast stops where it is, and a band
    /// springing back is grabbed from where it is drawn. While it is past an
    /// end the wheel goes to the band — further out, or back in, and once
    /// all the way in what is left scrolls.
    pub fn wheel(&mut self, y: f32, now: Instant, view: f32) -> f32 {
        let system = self.system_coast(y, now);
        self.heard = Some(now);
        self.now = Some(now);
        if system {
            return match self.phase {
                Phase::Following => {
                    self.travelled += y;
                    self.moves.add(now, self.travelled);
                    y
                }
                // Spent: the band it pulled is springing back.
                _ => 0.0,
            };
        }
        if let Phase::Settling { .. } = self.phase {
            // Grabbed: the band stays where it is drawn (Gecko's
            // `CancelAnimation` with `ExcludeOverscroll`).
            self.over = self.offset(now);
        }
        if !matches!(self.phase, Phase::Down) {
            // New fingers: a new drag, read afresh.
            self.moves.clear();
            self.travelled = 0.0;
            self.spending = false;
            self.fastest = 0.0;
        }
        self.held = false;
        self.travelled += y;
        self.moves.add(now, self.travelled);
        self.fastest = self.fastest.max(self.moves.speed().abs());
        if y != 0.0 {
            self.going = y.signum();
        }
        self.phase = Phase::Down;
        self.give(y, view)
    }

    /// The scrollable reached its end with `y` of the wheel's motion left
    /// over: that much more pull past it, if the fingers are down; and if it
    /// was coasting, the coast carries on into the band.
    pub fn overshoot(&mut self, y: f32, now: Instant, view: f32) {
        self.now = Some(now);
        match self.phase {
            Phase::Down => self.over = stretch(self.over, y, view, tokens::BAND_STEP),
            Phase::Coasting { since, speed, .. } => {
                let (_, speed) = motion::coast(speed, ms(now, since));
                self.hit(speed, now);
            }
            Phase::Following => {
                let speed = self.moves.speed();
                self.hit(speed, now);
                self.spending = true;
            }
            _ => {}
        }
    }

    /// A coast reached an end going at `speed`: on into the band from the
    /// edge, set off [`thrown`] on [`BACK`], and back, never past the edge
    /// (Gecko's fling handing its speed to `HandleFlingOverscroll`).
    fn hit(&mut self, speed: f32, now: Instant) {
        let side = match speed > 0.0 {
            true => 1.0,
            false => -1.0,
        };
        self.over = 0.0;
        self.phase = Phase::Settling {
            side,
            back: Slide::new(0.0, 0.0, now, BACK).moving_at(thrown(speed)).resting_within(tokens::BAND_REST),
        };
    }

    /// The band springs back from where it is drawn, set off at `speed`.
    fn settle(&mut self, speed: f32, now: Instant) {
        let from = self.over;
        self.over = 0.0;
        self.held = false;
        self.phase = match from == 0.0 {
            true => Phase::Idle,
            false => Phase::Settling {
                side: from.signum(),
                back: Slide::new(from, 0.0, now, BACK).moving_at(speed).resting_within(tokens::BAND_REST),
            },
        };
    }

    /// The fingers lifted at `now` (`told`: the system said so, rather than
    /// stillness): a band springs back from where it is drawn — set off as
    /// fast as it was let go going out, as Gecko's fling into an overscroll
    /// does, or from a standstill — or, let go inward fast enough, is given
    /// back by a coast that goes on into the list; and a scroll coasts on at
    /// the speed it was let go, or, where the system coasts, its coast is
    /// followed. Any word from the system while it coasts stops the coast:
    /// fingers landing.
    pub fn lift(&mut self, now: Instant, told: bool) {
        self.now = Some(now);
        self.heard = Some(now);
        match self.phase {
            Phase::Down => {
                self.held = false;
                // Held still before the lift: let go at no speed.
                let still = self.moves.last().is_none_or(|t| now.saturating_duration_since(t) > crate::velocity::GAP);
                let speed = match still && told {
                    true => 0.0,
                    false => self.moves.speed(),
                };
                // Seen only by its stillness, and run down by itself first:
                // a coast the browser or the system already made.
                let spent = !told && speed.abs() < self.fastest * SPENT;
                // Read at the last move, so a lift only seen later has
                // slowed as the coast would have meanwhile.
                let late = self.moves.last().map_or(0.0, |t| ms(now, t));
                let (_, coasting) = motion::coast(speed, late);
                if self.over != 0.0 {
                    let inward = speed * self.over < 0.0;
                    let fast = speed.abs() >= START && !spent;
                    match () {
                        _ if inward && fast && !system_coasts() => {
                            self.phase = Phase::Coasting {
                                since: now,
                                speed: coasting,
                                gone: 0.0,
                                told,
                            };
                        }
                        _ if fast && !inward => self.settle(thrown(speed), now),
                        _ => self.settle(0.0, now),
                    }
                    self.spending = system_coasts();
                    return;
                }
                self.phase = match () {
                    _ if system_coasts() => Phase::Following,
                    _ if spent => Phase::Idle,
                    _ if speed.abs() >= START && coasting.abs() >= STOP => Phase::Coasting {
                        since: now,
                        speed: coasting,
                        gone: 0.0,
                        told,
                    },
                    _ => Phase::Idle,
                };
            }
            // The system's coast is over, or new fingers stopped it.
            Phase::Following => self.phase = Phase::Idle,
            Phase::Settling { .. } => self.spending = false,
            // A word from the system while coasting is fingers on the
            // trackpad — unless it came before the coast's first frame: the
            // same lift said twice, as some Wayland compositors do (Gecko's
            // `OnPanEnd` ignores a second pan-end the same way).
            Phase::Coasting { gone, .. } if told && gone != 0.0 => self.stop(),
            _ => {}
        }
    }

    /// Fingers came to rest on the trackpad at `now` (Wayland's hold
    /// gesture beginning; Gecko's `PANGESTURE_MAYSTART`, which cancels
    /// every animation but keeps the overscroll): a coast stops where it
    /// is, and a band springing back stops where it is drawn and is held
    /// there until they lift or move.
    pub fn touch(&mut self, now: Instant) {
        self.now = Some(now);
        match self.phase {
            Phase::Coasting { .. } | Phase::Following | Phase::Settling { .. } => {
                self.over = self.offset(now);
                self.moves.clear();
                self.travelled = 0.0;
                self.fastest = 0.0;
                self.spending = false;
                self.held = self.over != 0.0;
                self.phase = match self.held {
                    true => Phase::Down,
                    false => Phase::Idle,
                };
            }
            _ => {}
        }
    }

    /// The fingers that came to rest lifted, or moved on into a scroll, at
    /// `now` (the hold gesture ending, cancelled or not; Gecko's
    /// `PANGESTURE_CANCELLED`, which snaps an overscroll back): a band they
    /// held springs back from a standstill. Nothing else: a scroll that
    /// follows is the wheel's.
    pub fn untouch(&mut self, now: Instant) {
        if self.held && self.phase == Phase::Down {
            self.lift(now, true);
        }
        self.held = false;
    }

    /// What the fingers on the trackpad have done since last felt
    /// ([`crate::touchpad::fingers`]): come to rest ([`Kinetic::touch`]),
    /// and lifted again ([`Kinetic::untouch`]). Whether anything changed.
    pub fn feel(&mut self, fingers: crate::touchpad::Fingers, now: Instant) -> bool {
        let before = (self.phase, self.over);
        if fingers.holds != self.holds {
            self.holds = fingers.holds;
            self.touch(now);
        }
        if !fingers.resting {
            self.untouch(now);
        }
        before != (self.phase, self.over)
    }

    /// A click, a touch or a wheel elsewhere: a coast stops where it is,
    /// and a band it was giving back springs back. A band springing back
    /// goes on.
    pub fn stop(&mut self) {
        if matches!(self.phase, Phase::Coasting { .. } | Phase::Following) {
            let now = self.now.unwrap_or_else(Instant::now);
            self.settle(0.0, now);
        }
        self.spending = false;
    }

    /// Put back at once: a key pressed.
    pub fn reset(&mut self) {
        *self = Kinetic {
            heard: self.heard,
            seen: self.seen,
            holds: self.holds,
            ..Kinetic::default()
        };
    }

    /// A frame at `now`, `told` whether only the system's word lets the
    /// fingers go: how far a coast has gone since the last frame, for the
    /// scrollable to scroll, and what it wants next — a frame, a wake at a
    /// time, or nothing. Fingers held still are let go once still for
    /// [`LIFT`] ([`BAND_LIFT`] past an end) where the system never says,
    /// and never where it does; a coast gives back a band first; a band
    /// comes to rest at the edge.
    pub fn tick(&mut self, now: Instant, view: f32, told: bool) -> (f32, Wake) {
        self.now = Some(now);
        match self.phase {
            Phase::Idle => (0.0, Wake::Never),
            Phase::Down if told || self.held => (0.0, Wake::Never),
            Phase::Down | Phase::Following => {
                let since = self.heard.unwrap_or(now);
                let wait = match self.over != 0.0 {
                    true => BAND_LIFT,
                    false => LIFT,
                };
                match now.saturating_duration_since(since) >= wait {
                    true if self.phase == Phase::Following => {
                        self.phase = Phase::Idle;
                        (0.0, Wake::Never)
                    }
                    true => {
                        self.lift(now, false);
                        (0.0, Wake::Frame)
                    }
                    false => (0.0, Wake::At(since + wait)),
                }
            }
            Phase::Coasting { since, speed, gone, told } => {
                let (at, now_speed) = motion::coast(speed, ms(now, since));
                let step = self.give(at - gone, view);
                self.phase = Phase::Coasting {
                    since,
                    speed,
                    gone: at,
                    told,
                };
                match now_speed.abs() < STOP {
                    true => {
                        self.settle(0.0, now);
                        (step, Wake::Never)
                    }
                    false => (step, Wake::Frame),
                }
            }
            Phase::Settling { side, back } => match back.at(now) * side >= 0.0 && back.moving(now) {
                true => (0.0, Wake::Frame),
                // At rest at the edge, or across it: there it stops
                // (Gecko's `Axis::SampleOverscrollAnimation`).
                false => {
                    self.phase = Phase::Idle;
                    (0.0, Wake::Never)
                }
            },
        }
    }
}

/// Milliseconds from `then` to `now`.
fn ms(now: Instant, then: Instant) -> f32 {
    now.saturating_duration_since(then).as_secs_f32() * 1000.0
}

/// What a scrollable wants after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// Nothing: at rest.
    Never,
    /// The next frame: coasting or springing back.
    Frame,
    /// A frame at this time: to see whether the fingers have let go.
    At(Instant),
}

thread_local! {
    /// The next scrollbar [`crate::style::bars`] draws is not drawn: an
    /// [`elastic`] banding draws it itself, unmoved ([`thumb`]).
    static HIDE_BAR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the scrollbar now being styled is one an [`elastic`] draws
/// itself while it bands: asked once, by [`crate::style::bars`].
pub(crate) fn bar_hidden() -> bool {
    HIDE_BAR.with(|h| h.replace(false))
}

struct Elastic<'a, M> {
    content: Element<'a, M>,
}

/// The outermost scrollable in what an [`Elastic`] holds, as an operation
/// finds it — its bounds, how tall what it scrolls is, how far it is
/// scrolled — and the bounds of every scrollable inside it that scrolls
/// up and down, in its content's coordinates.
#[derive(Default)]
struct Look {
    outer: Option<(Rectangle, Rectangle, f32)>,
    inner: Vec<Rectangle>,
}

impl Operation for Look {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn scrollable(&mut self, _id: Option<&Id>, bounds: Rectangle, content: Rectangle, translation: Vector, _state: &mut dyn Scrollable) {
        match self.outer {
            None => self.outer = Some((bounds, content, translation.y)),
            Some(_) if content.height > bounds.height + 0.5 => self.inner.push(bounds),
            Some(_) => {}
        }
    }
}

impl<M> Elastic<'_, M> {
    fn look(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer) -> Look {
        let mut look = Look::default();
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, &mut look);
        let origin = layout.bounds().position();
        tree.state.downcast_mut::<Kinetic>().seen = look.outer.map(|(bounds, content, scrolled)| Seen {
            bounds: bounds - Vector::new(origin.x, origin.y),
            content: content.height,
            scrolled,
        });
        look
    }
}

/// How far the content in `tree` is drawn moved, and the move itself.
fn moved(tree: &Tree) -> (f32, Transformation) {
    let kinetic = tree.state.downcast_ref::<Kinetic>();
    let offset = kinetic.offset(kinetic.now.unwrap_or_else(Instant::now));
    (offset, Transformation::translate(0.0, offset))
}

/// What a scrollable wants, asked of the shell.
fn ask<M>(shell: &mut Shell<'_, M>, wake: Wake) {
    match wake {
        Wake::Never => {}
        Wake::Frame => shell.request_redraw(),
        Wake::At(at) => shell.request_redraw_at(window::RedrawRequest::At(at)),
    }
}

impl<M> Widget<M, Theme, iced::Renderer> for Elastic<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Kinetic>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Kinetic::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        let node = self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        layout::Node::with_children(node.size(), vec![node])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let inner = layout.children().next().expect("the content's layout");
        match event {
            Event::Window(window::Event::RedrawRequested(now)) => self.frame(tree, *now, inner, renderer, clipboard, shell, viewport),
            Event::Keyboard(keyboard::Event::KeyPressed { .. }) => {
                let kinetic = tree.state.downcast_mut::<Kinetic>();
                if kinetic.active() {
                    kinetic.reset();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(_) => tree.state.downcast_mut::<Kinetic>().stop(),
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => match pulls(*delta, motion::reduced()) {
                Some((x, y)) => {
                    if self.wheel(tree, x, y, inner, cursor, renderer, clipboard, shell, viewport) {
                        return;
                    }
                }
                // A wheel that steps: a coast stops, and it scrolls as ever.
                None => tree.state.downcast_mut::<Kinetic>().stop(),
            },
            _ => {}
        }
        let (offset, moved) = moved(tree);
        let (cursor, viewport) = match offset != 0.0 {
            true => {
                let inverse = moved.inverse();
                (cursor * inverse, *viewport * inverse)
            }
            false => (cursor, *viewport),
        };
        self.content
            .as_widget_mut()
            .update(&mut tree.children[0], event, inner, cursor, renderer, clipboard, shell, &viewport);
    }

    /// Its content's accessibility tree, as is: this wrapper adds nothing a
    /// screen reader reads (COSMIC's accesskit, the fork's `a11y`).
    fn a11y_nodes(&self, layout: Layout<'_>, tree: &Tree, cursor: mouse::Cursor) -> iced_accessibility::A11yTree {
        self.content
            .as_widget()
            .a11y_nodes(layout.children().next().unwrap_or(layout), &tree.children[0], cursor)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let bounds = layout.bounds();
        let inner = layout.children().next().expect("the content's layout");
        let content = self.content.as_widget();
        let (offset, moved) = moved(tree);
        if offset == 0.0 {
            return content.draw(&tree.children[0], renderer, theme, style, inner, cursor, viewport);
        }
        // Moved, but only ever drawn where it is: what it pulls away from
        // shows what is behind it, and nothing below it is drawn over.
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let inverse = moved.inverse();
        // The scrollbar, drawn here unmoved rather than by the scrollable
        // moved with what it scrolls: its style is asked first, and
        // `style::bars` hands it back undrawn.
        HIDE_BAR.with(|h| h.set(true));
        renderer.with_layer(clip, |renderer| {
            renderer.with_transformation(moved, |renderer| {
                content.draw(&tree.children[0], renderer, theme, style, inner, cursor * inverse, &(clip * inverse));
            });
        });
        // Not asked: a scrollable of another style drew its own, moved.
        if HIDE_BAR.with(|h| h.replace(false)) {
            return;
        }
        let Some(seen) = tree.state.downcast_ref::<Kinetic>().seen else {
            return;
        };
        let at = seen.bounds + Vector::new(inner.bounds().x, inner.bounds().y);
        let Some((top, length)) = thumb(at.y, at.height, at.height, seen.content, seen.scrolled, offset) else {
            return;
        };
        let look = crate::style::bars(
            theme,
            iced::widget::scrollable::Status::Active {
                is_horizontal_scrollbar_disabled: true,
                is_vertical_scrollbar_disabled: false,
            },
        )
        .vertical_rail;
        let rail = Rectangle {
            x: at.x + at.width - THUMB_MIN,
            y: at.y,
            width: THUMB_MIN,
            height: at.height,
        };
        renderer.with_layer(clip, |renderer| {
            if let Some(ground) = look.background {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: rail,
                        border: look.border,
                        ..renderer::Quad::default()
                    },
                    ground,
                );
            }
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        y: top,
                        height: length,
                        ..rail
                    },
                    border: look.scroller.border,
                    ..renderer::Quad::default()
                },
                look.scroller.background,
            );
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let inner = layout.children().next().expect("the content's layout");
        let inverse = moved(tree).1.inverse();
        self.content
            .as_widget()
            .mouse_interaction(&tree.children[0], inner, cursor * inverse, &(*viewport * inverse), renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn widget::Operation) {
        let inner = layout.children().next().expect("the content's layout");
        self.content.as_widget_mut().operate(&mut tree.children[0], inner, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        offset: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, iced::Renderer>> {
        let inner = layout.children().next().expect("the content's layout");
        let (by, _) = moved(tree);
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], inner, renderer, viewport, offset + Vector::new(0.0, by))
    }
}

/// What became of the wheel handed to the scrollable.
enum Scrolled {
    /// It scrolled, and this much of it was past its end.
    Past(f32),
    /// Something else took it, or nothing did.
    Elsewhere,
}

impl<M> Elastic<'_, M> {
    /// A frame at `now`: a coast's way since the last one is handed to the
    /// scrollable, from where the pointer was; what it could not take is
    /// past its end, and the coast goes on into the band.
    #[allow(clippy::too_many_arguments)]
    fn frame(
        &mut self,
        tree: &mut Tree,
        now: Instant,
        inner: Layout<'_>,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let view = inner.bounds().height;
        let kinetic = tree.state.downcast_mut::<Kinetic>();
        // Fingers that came to rest since the last frame stop it first.
        kinetic.feel(crate::touchpad::fingers(), now);
        let (step, wake) = kinetic.tick(now, view, told());
        let aim = kinetic.aim.unwrap_or(inner.bounds().center());
        ask(shell, wake);
        if step == 0.0 {
            return;
        }
        let cursor = mouse::Cursor::Available(aim);
        match self.scroll(tree, 0.0, step, inner, cursor, renderer, clipboard, shell, viewport) {
            Scrolled::Past(0.0) => {}
            Scrolled::Past(past) => tree.state.downcast_mut::<Kinetic>().overshoot(past, now, view),
            // Nowhere to coast to (the list went, or something took it).
            Scrolled::Elsewhere => tree.state.downcast_mut::<Kinetic>().stop(),
        }
        shell.request_redraw();
    }

    /// The wheel, `x` and `y` pixels, handed to the scrollable as it was
    /// before the wheel came: how much of it went past its end, or that it
    /// went elsewhere.
    #[allow(clippy::too_many_arguments)]
    fn scroll(
        &mut self,
        tree: &mut Tree,
        x: f32,
        y: f32,
        inner: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) -> Scrolled {
        let Some((outer, content, scrolled)) = self.look(tree, inner, renderer).outer else {
            return Scrolled::Elsewhere;
        };
        // Where the scroll would go, and how much of the wheel's motion it
        // has no room for.
        let room = (content.height - outer.height).max(0.0);
        let wanted = scrolled - y;
        let reached = wanted.clamp(0.0, room);
        let past = reached - wanted;
        let event = Event::Mouse(mouse::Event::WheelScrolled {
            delta: ScrollDelta::Pixels { x, y },
        });
        self.content
            .as_widget_mut()
            .update(&mut tree.children[0], &event, inner, cursor, renderer, clipboard, shell, viewport);
        // It scrolled as far as it could, and the rest is past its end —
        // unless something else in it took the wheel.
        let after = self.look(tree, inner, renderer).outer.map(|(_, _, s)| s);
        // The scrollable says where it is to the pixel, rounded, so what
        // it was seen at and where it went are each half a pixel out.
        match after.is_some_and(|s| (s - reached).abs() <= 1.0) {
            true => Scrolled::Past(past),
            false => Scrolled::Elsewhere,
        }
    }

    /// A trackpad's wheel, `x` and `y` pixels, over the content: whether it
    /// was handled here (the band took it, or it was handed on scaled to
    /// what the band left), rather than to be handed on as it came.
    #[allow(clippy::too_many_arguments)]
    fn wheel(
        &mut self,
        tree: &mut Tree,
        x: f32,
        y: f32,
        inner: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) -> bool {
        let now = Instant::now();
        let view = inner.bounds().height;
        if tree.state.downcast_mut::<Kinetic>().feel(crate::touchpad::fingers(), now) {
            shell.request_redraw();
        }
        // Moved nowhere: the fingers lifted (or, on a Mac, touched). Any
        // such word stops a coast.
        if x == 0.0 && y == 0.0 {
            TOLD.store(true, Ordering::Relaxed);
            let kinetic = tree.state.downcast_mut::<Kinetic>();
            if kinetic.active() {
                kinetic.lift(now, true);
                shell.request_redraw();
            }
            return false;
        }
        let Some(over) = cursor.position_over(inner.bounds()) else {
            return false;
        };
        let look = self.look(tree, inner, renderer);
        let Some((_, _, scrolled)) = look.outer else {
            return false;
        };
        // A scrollable inside, under the pointer, has the wheel.
        let at = over + Vector::new(0.0, scrolled);
        if look.inner.iter().any(|b| b.contains(at)) {
            tree.state.downcast_mut::<Kinetic>().stop();
            return false;
        }
        let kinetic = tree.state.downcast_mut::<Kinetic>();
        kinetic.aim = Some(over);
        let left = kinetic.wheel(y, now, view);
        shell.request_redraw();
        if left == 0.0 && x == 0.0 {
            shell.capture_event();
            return true;
        }
        if let Scrolled::Past(past) = self.scroll(tree, x, left, inner, cursor, renderer, clipboard, shell, viewport) {
            if past != 0.0 {
                tree.state.downcast_mut::<Kinetic>().overshoot(past, now, view);
                shell.capture_event();
            }
        }
        true
    }
}
