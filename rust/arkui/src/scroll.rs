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
//! **A trackpad scrolls as a browser's does** ([`elastic`]). Let go while
//! moving, a list coasts on and slows as a scroll does — its speed read
//! from the fingers' last moves ([`crate::velocity`]), kept
//! [`crate::motion_tokens::DECELERATION`] a millisecond — and stops of
//! itself; the next touch of the wheel, a click or a key stops it at once.
//! Pulled past its top or its foot it goes on with resistance, the page
//! drawn [`band`] of the whole pull past its edge, and springs back when
//! the fingers lift, on the shared critically damped move
//! ([`crate::motion::SMOOTH`]) from the speed it was drawn going, never
//! past the edge; a coast that reaches an end carries on into the band as
//! fast as it was going and springs back the same way. The coast scrolls
//! the scrollable as more of the wheel would; the band is only drawn, and
//! the scroll is never moved past its end: [`follow`], the scroll edge and
//! the small header read the offset they always did.
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

use crate::motion::{self, rubberband, unband, Slide, Spring};
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

/// What a band past an end springs back on: the shared move
/// ([`motion::SMOOTH`]), critically damped, so it comes to the edge and
/// stops there.
pub const BACK: Spring = motion::SMOOTH;

/// The most a band is drawn past its end, as a share of the view
/// ([`tokens::BAND_REACH`]): however far it is pulled, never further.
pub const REACH: f32 = tokens::BAND_REACH;

/// A coast slower than this, a second, has stopped ([`tokens::COAST_STOP`]).
pub const STOP: f32 = tokens::COAST_STOP;

/// A lift seen only by its stillness, slower than this share of the
/// gesture's fastest, ran down by itself first — a browser's own coast
/// arriving as more of the wheel (Chrome on Windows or Wayland, Safari) —
/// and coasts no further ([`tokens::COAST_SPENT`]).
pub const SPENT: f32 = tokens::COAST_SPENT;

/// Where the system never says the fingers lifted, a trackpad still this
/// long has been let go ([`tokens::LIFT_MS`]).
pub const LIFT: Duration = Duration::from_millis(tokens::LIFT_MS);

/// The shortest the scrollbar's thumb is drawn while a band squeezes it:
/// about as long as it is wide (iced's scrollbar is 10 px wide).
pub const THUMB_MIN: f32 = 10.0;

/// Whether the system coasts a trackpad itself once the fingers lift,
/// sending its coast on as more of the wheel: macOS does (its momentum
/// phase), and so does a browser on a Mac, which hands a page the
/// system's coast as more `wheel` events. There the coast is followed,
/// never made, so it is not doubled. Everywhere else — Wayland, which
/// only says the fingers lifted, and a browser elsewhere, which says
/// nothing — it is made here.
pub fn system_coasts() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(target_arch = "wasm32")]
    {
        use std::sync::OnceLock;
        static MAC: OnceLock<bool> = OnceLock::new();
        *MAC.get_or_init(|| {
            web_sys::window()
                .and_then(|w| w.navigator().platform().ok())
                .is_some_and(|p| p.starts_with("Mac"))
        })
    }
    #[cfg(not(any(target_os = "macos", target_arch = "wasm32")))]
    {
        false
    }
}

/// The system says when fingers lift from a trackpad (a wheel event that
/// moved nowhere: Wayland's `axis_stop`, macOS's end of a phase), so only
/// that lets go. Learned the first time it happens, for every scrollable:
/// it is the system's, not one list's. Until then [`LIFT`] of stillness
/// is a lift.
static TOLD: AtomicBool = AtomicBool::new(false);

/// How far a band `pull` pixels past an end, in a view `view` tall, is
/// drawn past it: Apple's [`rubberband`] of the whole pull in the view,
/// held to [`REACH`] of it. Of the whole pull, never of each move: the same
/// pull is always drawn the same, however it came.
pub fn band(pull: f32, view: f32) -> f32 {
    let most = view * REACH;
    rubberband(pull, view).clamp(-most, most)
}

/// How far a band drawn `drawn` past an end was pulled: [`band`] undone.
fn unbanded(drawn: f32, view: f32) -> f32 {
    unband(drawn, view)
}

/// How fast a band pulled `pull` past its end is drawn going, for each
/// pixel a second of pull: [`rubberband`]'s slope, and nothing once held at
/// [`REACH`].
fn band_rate(pull: f32, view: f32) -> f32 {
    match rubberband(pull, view).abs() < view * REACH {
        true => motion::rubberband_slope(pull, view),
        false => 0.0,
    }
}

/// The fastest a band may set off outward from `from`, drawn, and not be
/// drawn past [`REACH`] of the view: a critically damped spring set off
/// at `v` from `x` goes no further than `x + v / (ω e)`.
fn capped(speed: f32, from: f32, side: f32, view: f32) -> f32 {
    let w = std::f32::consts::TAU / BACK.response;
    let most = ((view * REACH - from.abs()).max(0.0)) * w * std::f32::consts::E;
    match speed * side > 0.0 {
        true => speed.signum() * speed.abs().min(most),
        false => speed,
    }
}

/// The scrollbar's thumb, as `(top, length)`, on a rail from `rail_top`
/// `rail` long beside a view `view` tall over `content`, scrolled to
/// `scrolled` (the true offset, never past an end) and drawn `overshoot`
/// past an end (positive past the top). At rest, iced's own thumb: as long
/// as the share of the content seen, as far down as the scroll. Banded it
/// stays at that end and is drawn shorter, as if the band were more
/// content — `length · view / (view + |overshoot|)` — but never shorter
/// than [`THUMB_MIN`], as macOS and the iPhone draw it. `None` for content
/// that fits.
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
/// trackpad as a browser and a Mac scroll: let go while moving, it coasts
/// on and slows as a scroll does ([`motion::coast`]), and pulled past its
/// top or its foot it goes on with resistance ([`band`] of how far past
/// it went) and springs back when the fingers lift, on [`BACK`], from the
/// speed it was drawn going, and stops at the edge. A coast that reaches an
/// end carries on into the band as fast as it was going, and springs back
/// the same way. While it bands, the scrollbar's thumb stays at that end
/// and is drawn shorter ([`thumb`]).
///
/// Only a trackpad's pixels coast or band ([`pulls`]): a wheel that turns
/// in lines steps and stops at the end as it always did. Where the system
/// asks for less motion ([`motion::reduced`]) nothing coasts or bands at
/// all. A key pressed puts a band back at once (keyboard motion is never
/// animated) and stops a coast, as a click, a touch or the wheel does.
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
    /// The fingers are on the trackpad, `pull` pixels past an end: positive
    /// past the top (the content drawn lower), negative past the foot, none
    /// while it scrolls.
    Down { pull: f32 },
    /// Let go while moving: `speed` at `since`, and how much of its way has
    /// been handed to the scroll; `told` whether the system said the
    /// fingers lifted (so a word from it now is new fingers).
    Coasting { since: Instant, speed: f32, gone: f32, told: bool },
    /// The system's own coast ([`system_coasts`]): its wheel scrolls as it
    /// comes, and only an end is this one's.
    Following,
    /// Springing back to the edge, past the top (`side` 1) or the foot (−1).
    Settling { side: f32, back: Slide },
}

/// One scrollable's coast and band: whether the fingers are down, how far
/// past an end it is pulled, how it is coasting or springing back, and the
/// wheel's last moves, which say how fast it was let go. Pure, so it is
/// tested without a window; [`elastic`] feeds it.
///
/// The wheel is in iced's sense throughout: `y` positive moves the content
/// down, toward the top.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Kinetic {
    phase: Phase,
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
    /// How far the content is drawn moved at `now`, in a view `view` tall:
    /// down past the top, up past the foot, never across the edge.
    pub fn offset(&self, now: Instant, view: f32) -> f32 {
        match self.phase {
            Phase::Down { pull } => band(pull, view),
            Phase::Settling { side, back } => {
                let at = back.at(now);
                match at * side > 0.0 {
                    true => at,
                    false => 0.0,
                }
            }
            _ => 0.0,
        }
    }

    /// How far past an end it is pulled at `now`, in the wheel's pixels:
    /// springing back, the pull that would draw it where it is.
    pub fn pull(&self, now: Instant, view: f32) -> f32 {
        match self.phase {
            Phase::Down { pull } => pull,
            Phase::Settling { .. } => unbanded(self.offset(now, view), view),
            _ => 0.0,
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

    /// The wheel moved `y` (a trackpad's pixels) at `now`, over a scrollable
    /// in a view `view` tall: how much of it is for the scrollable to
    /// scroll. The fingers are down: a coast stops where it is, and a band
    /// springing back is grabbed from where it is drawn. While it is pulled
    /// past an end the wheel goes to the band — further out, or back in, and
    /// once all the way in what is left scrolls.
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
        let pull = match self.phase {
            Phase::Down { pull } => pull,
            Phase::Settling { .. } => self.pull(now, view),
            _ => 0.0,
        };
        if !matches!(self.phase, Phase::Down { .. }) {
            // New fingers: a new drag, read afresh.
            self.moves.clear();
            self.travelled = 0.0;
            self.spending = false;
            self.fastest = 0.0;
        }
        self.travelled += y;
        self.moves.add(now, self.travelled);
        self.fastest = self.fastest.max(self.moves.speed().abs());
        if y != 0.0 {
            self.going = y.signum();
        }
        if pull == 0.0 {
            self.phase = Phase::Down { pull: 0.0 };
            return y;
        }
        let to = pull + y;
        match to.signum() == pull.signum() && to != 0.0 {
            true => {
                self.phase = Phase::Down { pull: to };
                0.0
            }
            false => {
                self.phase = Phase::Down { pull: 0.0 };
                to
            }
        }
    }

    /// The scrollable reached its end with `y` of the wheel's motion left
    /// over: that much past it, if the fingers are down; and if it was
    /// coasting, the coast carries on into the band as fast as it was going.
    pub fn overshoot(&mut self, y: f32, now: Instant, view: f32) {
        self.now = Some(now);
        match self.phase {
            Phase::Down { pull } => self.phase = Phase::Down { pull: pull + y },
            Phase::Coasting { since, speed, .. } => {
                let (_, speed) = motion::coast(speed, ms(now, since));
                self.hit(speed, now, view);
            }
            Phase::Following => {
                let speed = self.moves.speed();
                self.hit(speed, now, view);
                self.spending = true;
            }
            _ => {}
        }
    }

    /// A coast reached an end going at `speed`: on into the band from the
    /// edge, slowing on [`BACK`] as it goes (faster than it slowed while it
    /// scrolled), and back, never past the edge.
    fn hit(&mut self, speed: f32, now: Instant, view: f32) {
        let side = match speed > 0.0 {
            true => 1.0,
            false => -1.0,
        };
        let speed = capped(speed, 0.0, side, view);
        self.phase = Phase::Settling {
            side,
            back: Slide::new(0.0, 0.0, now, BACK).moving_at(speed),
        };
    }

    /// The fingers lifted at `now` (`told`: the system said so, rather than
    /// [`LIFT`] of stillness): a band springs back from where it is drawn
    /// and as fast as it was drawn going, and a scroll coasts on at the
    /// speed it was let go — or, where the system coasts, its coast is
    /// followed. A word from the system while a coast it was told of goes
    /// on is new fingers on the trackpad: the coast stops.
    pub fn lift(&mut self, now: Instant, view: f32, told: bool) {
        self.now = Some(now);
        self.heard = Some(now);
        match self.phase {
            Phase::Down { pull } => {
                // Held still before the lift: let go at no speed.
                let still = self.moves.last().is_none_or(|t| now.saturating_duration_since(t) > crate::velocity::GAP);
                let speed = match still && told {
                    true => 0.0,
                    false => self.moves.speed(),
                };
                // Seen only by its stillness, and run down by itself first:
                // a coast the browser or the system already made.
                let spent = !told && speed.abs() < self.fastest * SPENT;
                if pull != 0.0 {
                    let side = pull.signum();
                    let from = band(pull, view);
                    let speed = capped(speed * band_rate(pull, view), from, side, view);
                    self.phase = Phase::Settling {
                        side,
                        back: Slide::new(from, 0.0, now, BACK).moving_at(speed),
                    };
                    self.spending = system_coasts();
                    return;
                }
                // Read at the last move, so a lift only seen later has
                // slowed as the coast would have meanwhile.
                let late = self.moves.last().map_or(0.0, |t| ms(now, t));
                let (_, speed) = motion::coast(speed, late);
                self.phase = match () {
                    _ if system_coasts() => Phase::Following,
                    _ if spent => Phase::Idle,
                    _ if speed.abs() >= STOP => Phase::Coasting {
                        since: now,
                        speed,
                        gone: 0.0,
                        told,
                    },
                    _ => Phase::Idle,
                };
            }
            // The system's coast is over.
            Phase::Following => self.phase = Phase::Idle,
            Phase::Settling { .. } => self.spending = false,
            Phase::Coasting { told: true, .. } if told => self.phase = Phase::Idle,
            _ => {}
        }
    }

    /// A click, a touch or a wheel elsewhere: a coast stops where it is. A
    /// band goes on springing back.
    pub fn stop(&mut self) {
        if matches!(self.phase, Phase::Coasting { .. } | Phase::Following) {
            self.phase = Phase::Idle;
        }
        self.spending = false;
    }

    /// Put back at once: a key pressed.
    pub fn reset(&mut self) {
        *self = Kinetic {
            heard: self.heard,
            seen: self.seen,
            ..Kinetic::default()
        };
    }

    /// A frame at `now`, `told` whether the system says when fingers lift:
    /// how far a coast has gone since the last frame, for the scrollable
    /// to scroll, and what it wants next — a frame, a wake at a time, or
    /// nothing. Fingers held still are let go once still for [`LIFT`] where
    /// the system never says; a band comes to rest at the edge; and a band
    /// let go inward hard enough to cross it coasts on into the list.
    pub fn tick(&mut self, now: Instant, view: f32, told: bool) -> (f32, Wake) {
        self.now = Some(now);
        match self.phase {
            Phase::Idle => (0.0, Wake::Never),
            Phase::Down { .. } if told => (0.0, Wake::Never),
            Phase::Down { .. } | Phase::Following => {
                let since = self.heard.unwrap_or(now);
                match now.saturating_duration_since(since) >= LIFT {
                    true if self.phase == Phase::Following => {
                        self.phase = Phase::Idle;
                        (0.0, Wake::Never)
                    }
                    true => {
                        self.lift(now, view, false);
                        (0.0, Wake::Frame)
                    }
                    false => (0.0, Wake::At(since + LIFT)),
                }
            }
            Phase::Coasting { since, speed, gone, told } => {
                let (at, now_speed) = motion::coast(speed, ms(now, since));
                let step = at - gone;
                match now_speed.abs() < STOP {
                    true => {
                        self.phase = Phase::Idle;
                        (step, Wake::Never)
                    }
                    false => {
                        self.phase = Phase::Coasting {
                            since,
                            speed,
                            gone: at,
                            told,
                        };
                        (step, Wake::Frame)
                    }
                }
            }
            Phase::Settling { side, back } => {
                let at = back.at(now);
                if at * side < 0.0 {
                    // Across the edge, going inward: on into the list.
                    let speed = back.speed(now);
                    self.phase = match !system_coasts() && speed.abs() >= STOP {
                        true => Phase::Coasting {
                            since: now,
                            speed,
                            gone: 0.0,
                            told: false,
                        },
                        false => Phase::Idle,
                    };
                    return (0.0, Wake::Frame);
                }
                match back.moving(now) {
                    true => (0.0, Wake::Frame),
                    false => {
                        self.phase = Phase::Idle;
                        (0.0, Wake::Never)
                    }
                }
            }
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

/// How far the content in `tree` is drawn moved, in a scrollable `view`
/// tall, and the move itself.
fn moved(tree: &Tree, view: f32) -> (f32, Transformation) {
    let kinetic = tree.state.downcast_ref::<Kinetic>();
    let offset = kinetic.offset(kinetic.now.unwrap_or_else(Instant::now), view);
    (offset, Transformation::translate(0.0, offset))
}

/// The time an event is taken at: `now`, or, in a test that drives the
/// clock by hand, the time it says.
fn clock(now: Instant) -> Instant {
    #[cfg(test)]
    if let Some(fake) = tests::CLOCK.with(|c| c.get()) {
        return fake;
    }
    now
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

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
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
        let bounds = layout.bounds();
        let inner = layout.children().next().expect("the content's layout");
        let view = bounds.height;
        match event {
            Event::Window(window::Event::RedrawRequested(now)) => self.frame(tree, clock(*now), inner, renderer, clipboard, shell, viewport),
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
        let (offset, moved) = moved(tree, view);
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
        let (offset, moved) = moved(tree, bounds.height);
        #[cfg(test)]
        tests::trace(tree.state.downcast_ref::<Kinetic>(), offset);
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
        let inverse = moved(tree, layout.bounds().height).1.inverse();
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
        let (by, _) = moved(tree, layout.bounds().height);
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
        let (step, wake) = kinetic.tick(now, view, TOLD.load(Ordering::Relaxed));
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
        let now = clock(Instant::now());
        let view = inner.bounds().height;
        // Moved nowhere: the fingers lifted (or, on a Mac, touched).
        if x == 0.0 && y == 0.0 {
            TOLD.store(true, Ordering::Relaxed);
            let kinetic = tree.state.downcast_mut::<Kinetic>();
            if kinetic.active() {
                kinetic.lift(now, view, true);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Down a list of a hundred rows of 20 px in a 200 px view: the list
    /// stays put while the cursor is in view, and moves a row at a time once
    /// it is two rows from the foot; `zz`, `zt` and `zb` put it in the
    /// middle, the top and the foot. Falsified by scrolling to the row's
    /// share of the way down on every move, as `reveal` once did: row 3
    /// moved the list.
    #[test]
    fn the_list_moves_only_when_the_cursor_would_leave_it() {
        let at = |row, offset, place| offset_for(row, 100, 200.0, 2000.0, offset, place);
        assert_eq!(at(3, 0.0, Place::Near), None, "row 3 is in view");
        assert_eq!(at(7, 0.0, Place::Near), None, "row 7 is two rows from the foot");
        assert_eq!(at(8, 0.0, Place::Near), Some(20.0), "row 8 moves the list one row");
        assert_eq!(at(8, 20.0, Place::Near), None, "…and then it is in view");
        assert_eq!(at(3, 20.0, Place::Near), None, "two rows from the top, on the way back");
        assert_eq!(at(2, 20.0, Place::Near), Some(0.0), "row 2 brings the top back");
        assert_eq!(at(50, 0.0, Place::Middle), Some(910.0), "zz: row 50's middle is the view's");
        assert_eq!(at(50, 0.0, Place::Top), Some(960.0), "zt: two rows above it");
        assert_eq!(at(50, 0.0, Place::Bottom), Some(860.0), "zb: two rows below it");
        assert_eq!(at(0, 500.0, Place::Middle), Some(0.0), "the top stays the top");
        assert_eq!(offset_for(3, 5, 200.0, 100.0, 0.0, Place::Middle), None, "a list that fits does not move");
    }

    /// A row is put in the middle wherever there is room to, and the ends
    /// stay at the ends. Falsified by leaving out the half row: the tenth
    /// of twenty, ten seen, was not at the middle.
    #[test]
    fn a_row_is_centred_where_the_list_can_scroll() {
        assert_eq!(centred(0, 20, 10.0), 0.0, "the first stays at the top");
        assert_eq!(centred(19, 20, 10.0), 1.0, "the last at the foot");
        assert_eq!(centred(9, 20, 10.0), 0.45, "rows 4.5 to 14.5 in view: 9.5 its middle");
        assert_eq!(centred(5, 8, 10.0), 0.0, "a list that fits does not scroll");
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Where a list in an [`elastic`] is scrolled to after each of `wheel`'s
    /// events, as its `on_scroll` says, leaving out where it already was.
    #[cfg(not(target_arch = "wasm32"))]
    fn scrolled_by(wheel: &[ScrollDelta]) -> Vec<i32> {
        use iced::widget::{column, scrollable, text};
        let rows = column((0..50).map(|n| text(format!("row {n}")).height(40).into()));
        let list = scrollable(rows).height(Length::Fill).on_scroll(|v| v.absolute_offset().y.round() as i32);
        let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), (400.0, 300.0), elastic(list));
        ui.point_at(iced::Point::new(20.0, 150.0));
        for delta in wheel {
            let _ = ui.simulate([Event::Mouse(mouse::Event::WheelScrolled { delta: *delta })]);
        }
        let mut seen: Vec<i32> = ui.into_messages().filter(|&y| y != 0).collect();
        seen.dedup();
        seen
    }

    /// In a window: a trackpad pulled past the top bands the list rather
    /// than scrolling it, and pulled back gives the band back before the
    /// list moves — 50 out, 20 and then 40 back is 10 scrolled. A mouse's
    /// notches at the top do nothing, and the next scrolls at once.
    /// Falsified by the wheel handed to the list while it is pulled (it
    /// scrolled 60), or by banding the notches (the notch back gave the
    /// band back and did not scroll).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_list_pulled_past_its_top_gives_the_band_back_before_it_scrolls() {
        let px = |y| ScrollDelta::Pixels { x: 0.0, y };
        assert_eq!(scrolled_by(&[px(50.0), px(-20.0), px(-40.0)]), [10]);
        assert_eq!(scrolled_by(&[px(-30.0)]), [30], "not pulled, it scrolls as ever");
        let line = |y| ScrollDelta::Lines { x: 0.0, y };
        assert_eq!(scrolled_by(&[line(1.0), line(-1.0)]), [60]);
    }

    /// In a window, in real time: a trackpad's flick, its moves 8 ms apart,
    /// then the system's word that the fingers lifted — and the list goes
    /// on scrolling by itself, frame by frame, further than the fingers
    /// took it, slowing, and stops; it never goes the other way. Falsified
    /// by a list that stopped where the fingers left it (no coast), or by
    /// one that coasted for ever (no stop).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_flick_coasts_on_in_a_window() {
        use iced::widget::{column, scrollable, text};
        let rows = column((0..400).map(|n| text(format!("row {n}")).height(40).into()));
        let list = scrollable(rows).height(Length::Fill).on_scroll(|v| v.absolute_offset().y);
        let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), (400.0, 300.0), elastic(list));
        ui.point_at(iced::Point::new(20.0, 150.0));
        let wheel = |y| {
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: ScrollDelta::Pixels { x: 0.0, y },
            })
        };
        for _ in 0..10 {
            let _ = ui.simulate([wheel(-16.0)]);
            std::thread::sleep(ms(8));
        }
        let _ = ui.simulate([wheel(0.0)]);
        let lifted = Instant::now();
        for n in 1..=150u64 {
            let _ = ui.simulate([Event::Window(window::Event::RedrawRequested(lifted + ms(16 * n)))]);
        }
        let seen: Vec<f32> = ui.into_messages().collect();
        let fingers = 160.0;
        let last = *seen.last().expect("it scrolled");
        assert!(seen.windows(2).all(|w| w[1] >= w[0]), "never back: {seen:?}");
        assert!(last > fingers + 300.0, "it coasted on past the fingers' {fingers}: {last}");
        assert!(last < fingers + 1500.0, "and stopped: {last}");
    }

    const VIEW: f32 = 600.0;

    /// A list to feed a [`Kinetic`] as [`elastic`] does: `room` to scroll,
    /// scrolled `at`, the wheel and the frames handed on and what its end
    /// leaves over handed back.
    struct List {
        kinetic: Kinetic,
        at: f32,
        room: f32,
        t: Instant,
    }

    impl List {
        fn new(at: f32, room: f32) -> Self {
            List {
                kinetic: Kinetic::default(),
                at,
                room,
                t: Instant::now(),
            }
        }

        /// The scroll moved by the wheel's `y` (iced's: positive is up).
        fn scroll(&mut self, y: f32, now: Instant) {
            if y == 0.0 {
                return;
            }
            let wanted = self.at - y;
            let reached = wanted.clamp(0.0, self.room);
            self.at = reached;
            if reached != wanted {
                self.kinetic.overshoot(reached - wanted, now, VIEW);
            }
        }

        fn wheel(&mut self, y: f32, after: u64) {
            let now = self.t + ms(after);
            let left = self.kinetic.wheel(y, now, VIEW);
            self.scroll(left, now);
        }

        fn lift(&mut self, after: u64) {
            self.kinetic.lift(self.t + ms(after), VIEW, true);
        }

        fn frame(&mut self, after: u64) -> Wake {
            let now = self.t + ms(after);
            let (step, wake) = self.kinetic.tick(now, VIEW, true);
            self.scroll(step, now);
            wake
        }

        /// Where it is drawn: scrolled, less the band.
        fn drawn(&self, after: u64) -> f32 {
            self.at - self.kinetic.offset(self.t + ms(after), VIEW)
        }
    }

    /// A flick down at 1500 px a second, lifted: the list coasts on as far
    /// as the projection says, slower each frame, and asks for frames only
    /// while it moves. Falsified by a coast decayed per frame (it went far
    /// less far), or by a clock that kept ticking once it stopped.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_coast_goes_as_far_as_the_projection_and_stops() {
        let mut list = List::new(0.0, 100_000.0);
        for n in 0..=12u64 {
            list.wheel(-12.0, 8 * n);
        }
        let fingers = list.at;
        list.lift(96);
        assert!(list.kinetic.coasting());
        let mut last_step = f32::MAX;
        let mut frames = 0;
        let mut n = 1;
        loop {
            let before = list.at;
            let wake = list.frame(96 + 16 * n);
            let step = list.at - before;
            assert!(
                step >= 0.0 && step <= last_step + 1e-3,
                "{n}: slower each frame, {step} after {last_step}"
            );
            last_step = step;
            frames += 1;
            n += 1;
            if wake == Wake::Never || n > 2000 {
                break;
            }
            assert!(list.kinetic.coasting(), "{n}: asks for a frame only while it coasts");
        }
        assert!(!list.kinetic.active(), "at rest");
        assert_eq!(list.frame(96 + 16 * n + 16), Wake::Never, "nothing ticks at rest");
        let coasted = list.at - fingers;
        let projected = motion::project(1500.0);
        assert!((coasted - projected).abs() < 0.05 * projected, "{coasted} against {projected}");
        assert!(frames > 60, "{frames}");
    }

    /// Past the top the wheel pulls the band, drawn [`band`] of the whole
    /// pull however it came; back the other way it gives the band back
    /// first, continuously, and only what is left over scrolls; the scroll
    /// stays at its end the while. Falsified by a band compounded move by
    /// move (the same pull, in more moves, was drawn less far), or by the
    /// scroll moved while the band showed.
    #[test]
    fn the_band_follows_the_whole_pull_and_gives_it_back_first() {
        let mut list = List::new(0.0, 2000.0);
        list.wheel(30.0, 0);
        let mut drawn = Vec::new();
        for n in 1..=20 {
            list.wheel(10.0, 8 * n);
            drawn.push(list.kinetic.offset(list.t + ms(8 * n), VIEW));
            assert_eq!(list.at, 0.0, "pinned at the edge");
        }
        assert_eq!(list.kinetic.pull(list.t, VIEW), 230.0);
        assert_eq!(list.kinetic.offset(list.t, VIEW), band(230.0, VIEW));
        let mut once = List::new(0.0, 2000.0);
        once.wheel(230.0, 0);
        assert_eq!(
            once.kinetic.offset(once.t, VIEW),
            list.kinetic.offset(list.t, VIEW),
            "the same pull, drawn the same"
        );
        assert!(
            drawn.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] < 10.0 * tokens::RUBBER_BAND),
            "ever less"
        );
        // Back, in small moves: the band comes back, smoothly, to nothing.
        let mut last = list.kinetic.offset(list.t, VIEW);
        for n in 21..=43 {
            list.wheel(-10.0, 8 * n);
            let now = list.kinetic.offset(list.t, VIEW);
            assert!(now < last || now == 0.0, "{n}");
            assert!(last - now < 10.0, "{n}: no jump, {last} to {now}");
            last = now;
        }
        assert_eq!(list.kinetic.offset(list.t, VIEW), 0.0);
        assert_eq!(list.at, 0.0);
        list.wheel(-25.0, 8 * 44);
        assert_eq!(list.at, 25.0, "what is left over scrolls");
        // However far it is pulled, never more than REACH of the view.
        let mut far = List::new(2000.0, 2000.0);
        far.wheel(-1_000_000.0, 0);
        assert_eq!(far.kinetic.offset(far.t, VIEW), -VIEW * REACH);
        // Apple's band of the whole pull in the whole view, up to there.
        let mut some = List::new(2000.0, 2000.0);
        some.wheel(-300.0, 0);
        assert_eq!(some.kinetic.offset(some.t, VIEW), rubberband(-300.0, VIEW));
    }

    /// A flick into the foot: the coast reaches the end, carries on into the
    /// band as fast as it was going, slows, springs back and stops exactly
    /// at the edge — drawn first out and then back, each way once, never
    /// past the edge into the list, and no further than REACH. Falsified by
    /// a coast that stopped dead at the end (no band), or by the spring's
    /// overshoot drawn (the list dipped back past its foot).
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_coast_into_an_end_carries_into_the_band_and_springs_back() {
        let mut list = List::new(1500.0, 2000.0);
        for n in 0..=10u64 {
            list.wheel(-24.0, 8 * n);
        }
        list.lift(80);
        let mut drawn = Vec::new();
        let mut n = 1;
        loop {
            let wake = list.frame(80 + 8 * n);
            drawn.push(list.drawn(80 + 8 * n));
            n += 1;
            if wake == Wake::Never || n > 1000 {
                break;
            }
        }
        let peak = drawn.iter().cloned().fold(f32::MIN, f32::max);
        assert!(peak > 2000.0 + 10.0, "it went on into the band: {peak}");
        assert!(peak < 2000.0 + VIEW * REACH, "no further than the reach: {peak}");
        let top = drawn.iter().position(|&d| d == peak).unwrap();
        assert!(drawn[..=top].windows(2).all(|w| w[1] >= w[0]), "out, without turning");
        assert!(drawn[top..].windows(2).all(|w| w[1] <= w[0] + 1e-3), "back, without turning");
        assert!(drawn[top..].iter().all(|&d| d >= 2000.0 - 1e-3), "never back into the list");
        assert_eq!(*drawn.last().unwrap(), 2000.0, "exactly at the edge");
        assert_eq!(list.at, 2000.0);
        assert!(!list.kinetic.active());
    }

    /// Pulled past the top and let go, it springs back from where it is
    /// drawn, as fast as it was drawn going, comes to the edge and stops —
    /// only when the fingers lift: held still past the edge, told the
    /// system says, it stays. Falsified by a timer that let it go with the
    /// fingers still down, or by the spring's overshoot drawn (the page
    /// dipped past its top).
    #[test]
    fn let_go_it_springs_back_to_the_edge_and_no_further() {
        let mut list = List::new(0.0, 2000.0);
        for n in 0..=6u64 {
            list.wheel(20.0, 8 * n);
        }
        // Held still for two seconds: still pulled.
        assert_eq!(list.frame(2048), Wake::Never);
        let pulled = list.kinetic.offset(list.t + ms(2048), VIEW);
        assert!(pulled > 0.0, "still pulled with the fingers down");
        list.lift(2048);
        assert!((list.kinetic.offset(list.t + ms(2048), VIEW) - pulled).abs() < 1e-3, "no jump");
        let mut last = pulled;
        for n in 1..300u64 {
            let at = 2048 + 4 * n;
            list.frame(at);
            let now = list.kinetic.offset(list.t + ms(at), VIEW);
            assert!(now >= 0.0 && now <= last + 1e-3, "{n}: back, never past");
            last = now;
        }
        assert!(!list.kinetic.active(), "at rest");
        assert_eq!(list.at, 0.0);

        // Let go still moving out: it goes on out a moment, then back.
        let mut list = List::new(0.0, 2000.0);
        for n in 0..=6u64 {
            list.wheel(20.0, 8 * n);
        }
        let drawn = list.kinetic.offset(list.t + ms(48), VIEW);
        list.lift(50);
        assert!(list.kinetic.offset(list.t + ms(60), VIEW) > drawn, "still going out, a moment on");
        for n in 0..200u64 {
            assert!(list.kinetic.offset(list.t + ms(50 + 5 * n), VIEW) >= 0.0);
        }
    }

    /// Where the system never says the fingers lifted, a short stillness
    /// is a lift; where it does, only its word is. Falsified by waiting
    /// for a word that never comes (the band stayed pulled).
    #[test]
    fn untold_a_stillness_lets_go() {
        let mut kinetic = Kinetic::default();
        let t = Instant::now();
        let left = kinetic.wheel(40.0, t, VIEW);
        kinetic.overshoot(left, t, VIEW);
        let _ = kinetic.wheel(40.0, t + ms(8), VIEW);
        assert_eq!(kinetic.tick(t + ms(20), VIEW, false).1, Wake::At(t + ms(8) + LIFT), "not yet");
        assert_eq!(kinetic.tick(t + ms(8) + LIFT, VIEW, false).1, Wake::Frame, "still that long: let go");
        assert!(matches!(kinetic.phase, Phase::Settling { .. }));
    }

    /// Untold (a browser): a flick let go at speed coasts once it has been
    /// still for [`LIFT`]; but a stream that ran down by itself first — a
    /// browser's own coast, its moves shrinking a tenth each frame — is
    /// not coasted again at its tail. Falsified by coasting every lift
    /// (the browser's coast was doubled: it crept on after it stopped).
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn untold_a_coast_already_made_is_not_doubled() {
        let t = Instant::now();
        let mut flick = Kinetic::default();
        for n in 0..12u64 {
            let _ = flick.wheel(-16.0, t + ms(8 * n), VIEW);
        }
        let (_, wake) = flick.tick(t + ms(88) + LIFT, VIEW, false);
        assert_eq!(wake, Wake::Frame);
        assert!(flick.coasting(), "a flick let go coasts");

        let mut browser = Kinetic::default();
        let (mut y, mut at) = (-40.0f32, 0u64);
        while y.abs() >= 1.0 {
            let _ = browser.wheel(y, t + ms(at), VIEW);
            y *= 0.9;
            at += 16;
        }
        let _ = browser.tick(t + ms(at) + LIFT, VIEW, false);
        assert!(!browser.active(), "the browser's coast, run down, is not coasted again");
    }

    /// A wheel that turns in lines never coasts nor bands, nor does any
    /// wheel where motion is reduced. Falsified by banding a mouse's
    /// notches: a list scrolled to its end by a mouse wheel jumped past it.
    #[test]
    fn only_a_trackpad_coasts_and_never_with_less_motion() {
        assert_eq!(pulls(ScrollDelta::Pixels { x: 0.0, y: 12.0 }, false), Some((0.0, 12.0)));
        assert_eq!(pulls(ScrollDelta::Lines { x: 0.0, y: 1.0 }, false), None);
        assert_eq!(pulls(ScrollDelta::Pixels { x: 0.0, y: 12.0 }, true), None);
    }

    /// A coast stops at once for new fingers (the wheel), a click or a key;
    /// a band springing back is grabbed from where it is drawn, and a key
    /// puts it back at once. Falsified by a coast that went on under the
    /// fingers' next move (the two added up).
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn any_input_stops_a_coast() {
        let coasting = || {
            let mut list = List::new(5000.0, 100_000.0);
            for n in 0..=8u64 {
                list.wheel(-20.0, 8 * n);
            }
            list.lift(64);
            list.frame(80);
            assert!(list.kinetic.coasting());
            list
        };
        let mut list = coasting();
        list.wheel(-3.0, 90);
        assert!(!list.kinetic.coasting(), "new fingers");
        let at = list.at;
        list.frame(96);
        assert_eq!(list.at, at, "and it does not move by itself");

        let mut list = coasting();
        list.kinetic.stop();
        let at = list.at;
        assert_eq!(list.frame(96), Wake::Never, "a click");
        assert_eq!(list.at, at);

        let mut list = coasting();
        list.kinetic.reset();
        assert!(!list.kinetic.active(), "a key");

        // Grabbed springing back: carries on from where it is drawn.
        let mut list = List::new(0.0, 2000.0);
        list.wheel(120.0, 0);
        list.lift(0);
        list.frame(40);
        let drawn = list.kinetic.offset(list.t + ms(40), VIEW);
        list.wheel(-1.0, 40);
        let grabbed = list.kinetic.offset(list.t + ms(40), VIEW);
        assert!(grabbed > 0.0 && (grabbed - drawn).abs() < 1.0, "{drawn} to {grabbed}");
    }

    /// Where the system coasts by itself (macOS), a lift makes no coast of
    /// its own; the system's coast scrolls as it comes and, at an end, is
    /// carried into the band and the rest of it spent. Falsified by a coast
    /// made here as well (doubled).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_systems_coast_is_followed_not_doubled() {
        let mut list = List::new(0.0, 100_000.0);
        for n in 0..=8u64 {
            list.wheel(-20.0, 8 * n);
        }
        list.lift(64);
        let at = list.at;
        list.frame(80);
        assert_eq!(list.at, at, "no coast of its own");
        list.wheel(-18.0, 72);
        assert_eq!(list.at, at + 18.0, "the system's, followed");
    }

    /// The thumb: iced's own at rest; banded, pinned at that end and
    /// shorter by the band as if it were more content, never shorter than
    /// about its width (or than it was); and as the band goes it grows back
    /// to exactly the thumb at rest. Falsified by a thumb moved with the
    /// band (its top left the rail's), or by one squeezed to nothing.
    #[test]
    fn the_thumb_stays_at_its_end_and_shrinks_with_the_band() {
        let at = |scrolled, overshoot| thumb(0.0, 600.0, 600.0, 2400.0, scrolled, overshoot).unwrap();
        assert_eq!(at(0.0, 0.0), (0.0, 150.0), "a quarter of the content seen");
        assert_eq!(at(900.0, 0.0), (225.0, 150.0));
        assert_eq!(at(1800.0, 0.0), (450.0, 150.0), "at the foot");
        let (top, length) = at(0.0, 100.0);
        assert_eq!(top, 0.0, "pinned at the top");
        assert!((length - 150.0 * 600.0 / 700.0).abs() < 1e-3, "{length}");
        let (top, length) = at(1800.0, -200.0);
        assert!(
            (length - 112.5).abs() < 1e-3 && (top + length - 600.0).abs() < 1e-3,
            "pinned at the foot: {top} {length}"
        );
        assert_eq!(at(0.0, 1e7), (0.0, THUMB_MIN), "never shorter than its width");
        let tiny = thumb(0.0, 600.0, 600.0, 100_000.0, 0.0, 300.0).unwrap();
        assert!((tiny.1 - 3.6).abs() < 1e-4, "a thumb already short stays as it was: {}", tiny.1);
        // Growing back: longer as the band goes, and at none the rest.
        let lengths: Vec<f32> = [200.0, 120.0, 60.0, 20.0, 1.0, 0.0].iter().map(|&o| at(0.0, o).1).collect();
        assert!(lengths.windows(2).all(|w| w[1] > w[0]), "{lengths:?}");
        assert_eq!(*lengths.last().unwrap(), 150.0);
        assert_eq!(thumb(0.0, 600.0, 600.0, 500.0, 0.0, 40.0), None, "content that fits has none");
    }

    /// icy_ui's kinetic scroll, as its `scrollable.rs` has it, for the
    /// comparison below: the speed smoothed move by move (three tenths of
    /// each move's own speed, its time at least a millisecond), a coast
    /// stepped frame by frame and slowed by `e^(−5·dt)`, and a dead stop
    /// at an end.
    struct Icy {
        speed: f32,
        last: Option<Instant>,
        at: f32,
        room: f32,
        frame: Option<Instant>,
    }

    impl Icy {
        fn new(at: f32, room: f32) -> Self {
            Icy {
                speed: 0.0,
                last: None,
                at,
                room,
                frame: None,
            }
        }

        fn wheel(&mut self, y: f32, now: Instant) {
            if let Some(last) = self.last {
                let dt = now.saturating_duration_since(last).as_secs_f32().max(0.001);
                self.speed = self.speed * 0.7 + (-y / dt) * 0.3;
            }
            self.last = Some(now);
            self.at = (self.at - y).clamp(0.0, self.room);
        }

        fn lift(&mut self, now: Instant) {
            self.frame = Some(now);
        }

        fn tick(&mut self, now: Instant) {
            let Some(last) = self.frame else { return };
            if self.speed.abs() < 1.0 {
                return;
            }
            let dt = now.saturating_duration_since(last).as_secs_f32();
            self.frame = Some(now);
            let before = self.at;
            self.at = (self.at + self.speed * dt).clamp(0.0, self.room);
            self.speed *= (-5.0 * dt).exp();
            if self.at == before {
                self.speed = 0.0;
            }
        }
    }

    /// The two approaches on the same gestures: a steady flick at 2000 px a
    /// second, the same flick with its moves bunched as an event loop under
    /// load hands them on (two a millisecond apart, then a long gap), and a
    /// flick into the foot. fluid-scroll's least-squares speed reads both
    /// flicks within a few percent; icy_ui's smoothing reads the bunched
    /// one far out — a move a millisecond after the last is a speed of
    /// thousands — so the same fingers coast further one time than the
    /// next. icy_ui stops dead at the foot where this bands and springs
    /// back. With `ARK_KINETIC_FRAMES` set, each run's place frame by
    /// frame is written there as CSV (`compare.csv`).
    #[test]
    fn the_two_approaches_on_the_same_gestures() {
        // (ms after the last, pixels) for 12 moves, 2000 px/s on average.
        let steady: Vec<(u64, f32)> = (0..12).map(|_| (8, -16.0)).collect();
        // The same moves, read as they are handled rather than as they were
        // made: two a millisecond apart, or in the same one, then a gap.
        let bunched: Vec<(u64, f32)> = [8, 8, 1, 15, 8, 0, 16, 8, 1, 15, 8, 8].iter().map(|&dt| (dt, -16.0)).collect();
        let gestures = [
            ("steady", 100_000.0, 0.0, &steady),
            ("bunched", 100_000.0, 0.0, &bunched),
            ("edge", 2000.0, 1700.0, &steady),
        ];
        let mut csv = String::from("gesture,model,t_ms,drawn\n");
        let mut read = Vec::new();
        for (name, room, from, moves) in gestures {
            let mut ours = List::new(from, room);
            let mut icy = Icy::new(from, room);
            let mut t = 0;
            for &(dt, y) in moves.iter() {
                t += dt;
                ours.wheel(y, t);
                icy.wheel(y, ours.t + ms(t));
                csv += &format!("{name},fluid,{t},{}\n{name},icy,{t},{}\n", ours.drawn(t), icy.at);
            }
            let fingers = ours.at;
            ours.lift(t);
            icy.lift(ours.t + ms(t));
            let speed_ours = match ours.kinetic.phase {
                Phase::Coasting { speed, .. } => -speed,
                _ => -ours.kinetic.moves.speed(),
            };
            let speed_icy = icy.speed;
            read.push((speed_ours, speed_icy));
            let (mut peak_ours, mut peak_icy) = (ours.at, icy.at);
            for n in 1..=200u64 {
                let at = t + 16 * n;
                ours.frame(at);
                icy.tick(ours.t + ms(at));
                peak_ours = peak_ours.max(ours.drawn(at));
                peak_icy = peak_icy.max(icy.at);
                csv += &format!("{name},fluid,{at},{}\n{name},icy,{at},{}\n", ours.drawn(at), icy.at);
            }
            eprintln!(
                "{name}: speed read {speed_ours:.0} (fluid) / {speed_icy:.0} (icy); coasted {:.0} / {:.0}; furthest drawn {peak_ours:.1} / {peak_icy:.1}",
                ours.at - fingers,
                icy.at - fingers
            );
            if name == "edge" {
                assert!(peak_ours > room + 5.0, "fluid-scroll's carries into the band: {peak_ours}");
                assert_eq!(ours.at, room, "and back to the edge");
                assert_eq!(ours.drawn(t + 16 * 200), room);
                assert_eq!(peak_icy, room, "icy_ui's stops dead at the end");
            }
        }
        if let Ok(dir) = std::env::var("ARK_KINETIC_FRAMES") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join("compare.csv"), csv).unwrap();
        }
        let ((steady_ours, steady_icy), (bunched_ours, bunched_icy)) = (read[0], read[1]);
        assert!((steady_ours - 2000.0).abs() < 100.0, "{steady_ours}");
        assert!((bunched_ours - 2000.0).abs() < 200.0, "{bunched_ours}");
        assert!(
            (bunched_icy - steady_icy).abs() > 3.0 * (bunched_ours - steady_ours).abs(),
            "icy_ui's read moves with the bunching: {steady_icy} against {bunched_icy}"
        );
    }

    /// What each draw of an [`elastic`] drew: when, where the scroll truly
    /// was, how far banded, the thumb, and what it was doing.
    type Drawn = (Instant, f32, f32, Option<(f32, f32)>, String);

    thread_local! {
        /// The time every event is taken at, in a test that drives the
        /// clock by hand ([`super::clock`]).
        pub(super) static CLOCK: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
        /// The draws of an [`elastic`], while a test records them.
        static TRACE: std::cell::RefCell<Option<Vec<Drawn>>> = const { std::cell::RefCell::new(None) };
    }

    /// One draw, recorded if a test is recording.
    pub(super) fn trace(kinetic: &Kinetic, offset: f32) {
        TRACE.with(|t| {
            if let Some(log) = t.borrow_mut().as_mut() {
                let seen = kinetic.seen;
                let thumb = seen.and_then(|s| thumb(s.bounds.y, s.bounds.height, s.bounds.height, s.content, s.scrolled, offset));
                let phase = format!("{:?}", kinetic.phase);
                let phase = phase.split([' ', '{']).next().unwrap_or("").to_string();
                log.push((
                    kinetic.now.unwrap_or_else(Instant::now),
                    seen.map_or(0.0, |s| s.scrolled),
                    offset,
                    thumb,
                    phase,
                ));
            }
        });
    }

    /// Frames for the eye: a list in an [`elastic`] driven as a Linux
    /// trackpad drives it — pixel moves 8 ms apart, then the system's word
    /// that the fingers lifted — photographed frame by frame, 16 ms apart,
    /// as it coasts, bands and springs back, on a clock the test keeps. Three gestures: a flick
    /// (`fling`), a flick into the foot (`edge`), and a slow pull past the
    /// top, held, and let go (`pull`). Each frame is a PNG in
    /// `$ARK_KINETIC_FRAMES/<gesture>/`, and `<gesture>.csv` has every
    /// draw: when, the true offset, the band, the thumb.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "writes pictures: ARK_KINETIC_FRAMES=<dir> cargo test -p arkui --lib frames -- --ignored"]
    fn frames() {
        use iced::widget::{column, container, scrollable, text};
        let Ok(dir) = std::env::var("ARK_KINETIC_FRAMES") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let px = |y| {
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: ScrollDelta::Pixels { x: 0.0, y },
            })
        };
        for gesture in ["fling", "edge", "pull"] {
            let rows = column((0..60).map(|n| {
                container(text(format!("row {n}")).size(16))
                    .padding(10)
                    .width(Length::Fill)
                    .height(48)
                    .style(move |theme: &Theme| match n % 2 {
                        0 => container::Style::default().background(theme.extended_palette().background.weak.color),
                        _ => container::Style::default(),
                    })
                    .into()
            }));
            let list = scrollable(rows).height(Length::Fill).style(crate::style::bars);
            let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), (320.0, 480.0), elastic::<()>(list));
            ui.point_at(Point::new(100.0, 240.0));
            let out = dir.join(gesture);
            std::fs::create_dir_all(&out).unwrap();
            let mut shot = 0;
            let mut snap = |ui: &mut iced_test::Simulator<'_, ()>| {
                let frame = ui.snapshot(&Theme::Light).unwrap();
                let _ = frame.matches_image(out.join(format!("{shot:03}.png")));
                shot += 1;
            };
            if gesture == "edge" {
                // Near the foot, by the mouse's lines: no coast, no band.
                let _ = ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
                    delta: ScrollDelta::Lines { x: 0.0, y: -30.0 },
                })]);
            }
            TRACE.with(|t| *t.borrow_mut() = Some(Vec::new()));
            // Time is the test's: every event and frame is taken at `t`,
            // however long a picture takes to draw.
            let start = Instant::now();
            let mut t = 0u64;
            let at = |t: u64| {
                CLOCK.with(|c| c.set(Some(start + ms(t))));
                start + ms(t)
            };
            let frame = |ui: &mut iced_test::Simulator<'_, ()>, t: u64| {
                let _ = ui.simulate([Event::Window(window::Event::RedrawRequested(at(t)))]);
            };
            at(t);
            snap(&mut ui);
            match gesture {
                "pull" => {
                    for _ in 0..16 {
                        t += 16;
                        at(t);
                        let _ = ui.simulate([px(14.0)]);
                        snap(&mut ui);
                    }
                    // Held still past the top: the band stays.
                    for _ in 0..10 {
                        t += 32;
                        frame(&mut ui, t);
                        snap(&mut ui);
                    }
                }
                _ => {
                    for n in 0..12 {
                        t += 8;
                        at(t);
                        let _ = ui.simulate([px(-20.0)]);
                        if n % 2 == 1 {
                            snap(&mut ui);
                        }
                    }
                }
            }
            t += 8;
            at(t);
            let _ = ui.simulate([px(0.0)]);
            let lifted = t;
            while t < lifted + 2200 {
                t += 16;
                frame(&mut ui, t);
                snap(&mut ui);
            }
            CLOCK.with(|c| c.set(None));
            let log = TRACE.with(|t| t.borrow_mut().take()).unwrap_or_default();
            let mut csv = String::from("t_ms,scrolled,band,drawn,thumb_top,thumb_len,phase\n");
            for (at, scrolled, band, thumb, phase) in log {
                let t = at.saturating_duration_since(start).as_secs_f32() * 1000.0;
                let (top, len) = thumb.unwrap_or((0.0, 0.0));
                csv += &format!("{t:.1},{scrolled:.2},{band:.2},{:.2},{top:.2},{len:.2},{phase}\n", scrolled - band);
            }
            std::fs::write(dir.join(format!("{gesture}.csv")), csv).unwrap();
        }
    }
}
