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
//! **Past the ends, a rubber band** ([`elastic`]). iced's `scrollable`
//! stops dead at its top and its foot; wrapped in [`elastic`], a trackpad
//! pulling on past either goes on with resistance — the page drawn
//! [`crate::motion::rubberband`] of the pull past its edge — and springs
//! back when the fingers stop, on the shared critically damped move
//! ([`crate::motion::SMOOTH`]) from the speed it was drawn going, never
//! past the edge. It is drawn moved, not laid out moved, and the scroll
//! itself is never touched: [`follow`], the scroll edge and the small
//! header read the offset they always did.
use std::sync::atomic::{AtomicBool, Ordering};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::operation::{Operation, Scrollable};
use iced::advanced::widget::{self, tree, Id, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::mouse::ScrollDelta;
use iced::time::{Duration, Instant};
use iced::widget::operation::{snap_to, RelativeOffset};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{keyboard, window, Element, Event, Length, Rectangle, Size, Task, Theme, Transformation, Vector};

use crate::motion::{self, rubberband, unband, Slide, Spring};
use crate::swipe::{LET_GO, QUIET};
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

/// How long fingers held still past an end keep the band, where the system
/// says when they lift: past this it springs back anyway, rather than
/// staying pulled because a lift was lost.
pub const HELD: Duration = Duration::from_secs(1);

/// The system says when fingers lift from a trackpad (a wheel event that
/// moved nowhere: Wayland, macOS), so only that lets a band go. Learned
/// the first time it happens, for every scrollable: it is the system's,
/// not one list's.
static TOLD: AtomicBool = AtomicBool::new(false);

/// `content` — a `scrollable`, or something holding one — that a trackpad
/// can pull on past its top or its foot: the pull goes on with resistance,
/// [`rubberband`] of how far past the end it went, in the scrollable's
/// height, and when the fingers stop the content springs back to the edge
/// on [`BACK`], from the speed it was drawn going, and stops there.
///
/// Only a trackpad's pixels pull ([`pulls`]): a wheel that turns in lines
/// stops at the end as it always did. Where the system asks for less
/// motion ([`motion::reduced`]) nothing bands at all. A key pressed puts
/// it back at once (keyboard motion is never animated). The scroll's own
/// offset is never changed by this, so [`follow`], `zz`/`zt`/`zb`, the
/// scroll edge ([`crate::edge`]) and the small header ([`crate::compact`])
/// read what they always did; only the drawing is moved — and clipped to
/// where the scrollable is, so what it pulls away from shows what is
/// behind it and nothing below it is drawn over. The scrollbar moves with
/// the content while it bands: iced's scrollable gives no way to hold it.
///
/// The band belongs to the innermost scrollable under the pointer: with a
/// scrollable inside, the wheel over it is left to that one.
pub fn elastic<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Elastic { content: content.into() })
}

/// The wheel's motion, as a trackpad's pixels, if it pulls a band at all:
/// never a wheel that turns in lines, and never where motion is reduced.
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
    /// Pulled `pull` pixels past an end: positive past the top (the
    /// content drawn lower), negative past the foot.
    Pulling { pull: f32 },
    /// Let go, springing back from where it was drawn, `from`.
    Settling { from: f32, back: Slide },
}

/// One scrollable's band: how far past an end it is pulled, or how it is
/// springing back, and how fast it was drawn going. Pure, so it is tested
/// without a window; [`elastic`] feeds it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Band {
    phase: Phase,
    /// When the wheel last moved.
    last: Option<Instant>,
    /// Where it was last drawn while pulled, and when: what its speed is
    /// worked out from.
    drawn_at: Option<(f32, Instant)>,
    /// How fast it is drawn going, pixels a second.
    speed: f32,
    /// The frame last drawn, or the last event: when it is drawn at.
    now: Option<Instant>,
}

impl Band {
    /// How far past an end it is pulled now, in the wheel's pixels: while
    /// springing back, the pull that would draw it where it is.
    pub fn pull(&self, now: Instant, dimension: f32) -> f32 {
        match self.phase {
            Phase::Idle => 0.0,
            Phase::Pulling { pull } => pull,
            Phase::Settling { .. } => unband(self.offset(now, dimension), dimension),
        }
    }

    /// How far the content is drawn moved at `now`: down past the top,
    /// up past the foot, never across the edge.
    pub fn offset(&self, now: Instant, dimension: f32) -> f32 {
        match self.phase {
            Phase::Idle => 0.0,
            Phase::Pulling { pull } => rubberband(pull, dimension),
            Phase::Settling { from, back } => {
                let at = back.at(now);
                match at * from > 0.0 {
                    true => at,
                    false => 0.0,
                }
            }
        }
    }

    /// Whether anything is drawn moved, or is about to be.
    pub fn active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Pulled on to `pull`, at `now`: the speed it is drawn going is
    /// worked out from the last time.
    fn pull_to(&mut self, pull: f32, now: Instant, dimension: f32) {
        if pull == 0.0 {
            self.phase = Phase::Idle;
            self.drawn_at = None;
            self.speed = 0.0;
            return;
        }
        let drawn = rubberband(pull, dimension);
        match self.drawn_at {
            Some((was, then)) => {
                let dt = now.saturating_duration_since(then).as_secs_f32();
                // Events that come together are one: wait for time to pass.
                if dt >= 0.001 {
                    let speed = (drawn - was) / dt;
                    self.speed = match dt < 0.1 {
                        true => (self.speed + speed) / 2.0,
                        false => 0.0,
                    };
                    self.drawn_at = Some((drawn, now));
                }
            }
            None => {
                self.speed = 0.0;
                self.drawn_at = Some((drawn, now));
            }
        }
        self.phase = Phase::Pulling { pull };
    }

    /// The wheel moved `y` (a trackpad's pixels, up positive, as iced has
    /// them) at `now`, over a scrollable `dimension` tall: how much of it
    /// is left for the scrollable to scroll. While it is pulled past an end
    /// the wheel goes to the band — further out, or back in, and once all
    /// the way in what is left scrolls. Springing back, a pull grabs it from
    /// where it is drawn; but what still comes the same way soon after the
    /// fingers lifted is the trackpad coasting, and is spent.
    pub fn wheel(&mut self, y: f32, now: Instant, dimension: f32) -> f32 {
        let coast = match self.phase {
            Phase::Settling { from, .. } => from.signum() == y.signum() && self.last.is_some_and(|t| now.saturating_duration_since(t) <= QUIET),
            _ => false,
        };
        self.last = Some(now);
        self.now = Some(now);
        if coast {
            return 0.0;
        }
        let pull = self.pull(now, dimension);
        if pull == 0.0 {
            self.phase = Phase::Idle;
            return y;
        }
        let to = pull + y;
        match to.signum() == pull.signum() && to != 0.0 {
            true => {
                self.pull_to(to, now, dimension);
                0.0
            }
            false => {
                self.pull_to(0.0, now, dimension);
                to
            }
        }
    }

    /// The scrollable reached its end with `y` of the wheel's motion left
    /// over (up positive): that much past it.
    pub fn overshoot(&mut self, y: f32, now: Instant, dimension: f32) {
        let pull = self.pull(now, dimension) + y;
        self.last = Some(now);
        self.now = Some(now);
        self.pull_to(pull, now, dimension);
    }

    /// Let go at `now`: back to the edge on [`BACK`], from where it is drawn
    /// and as fast as it was drawn going — if the fingers were still going
    /// when they lifted.
    pub fn let_go(&mut self, now: Instant, dimension: f32) {
        let Phase::Pulling { pull } = self.phase else {
            return;
        };
        let recent = self.drawn_at.is_some_and(|(_, t)| now.saturating_duration_since(t) <= LET_GO);
        let speed = if recent { self.speed } else { 0.0 };
        let from = rubberband(pull, dimension);
        self.phase = Phase::Settling {
            from,
            back: Slide::new(from, 0.0, now, BACK).moving_at(speed),
        };
        self.drawn_at = None;
        self.speed = 0.0;
        self.now = Some(now);
    }

    /// Put back at once: a key pressed.
    pub fn reset(&mut self) {
        *self = Band {
            last: self.last,
            ..Band::default()
        };
    }

    /// A frame at `now`, `told` whether the system says when fingers lift:
    /// lets the band go once the wheel has been still long enough, and
    /// comes to rest once back. What it wants next: a frame, a wake at a
    /// time, or nothing.
    pub fn tick(&mut self, now: Instant, dimension: f32, told: bool) -> Wake {
        self.now = Some(now);
        match self.phase {
            Phase::Idle => Wake::Never,
            Phase::Pulling { .. } => {
                let wait = if told { HELD } else { LET_GO };
                let since = self.last.unwrap_or(now);
                match now.saturating_duration_since(since) >= wait {
                    true => {
                        self.let_go(now, dimension);
                        Wake::Frame
                    }
                    false => Wake::At(since + wait),
                }
            }
            Phase::Settling { from, back } => {
                let crossed = back.at(now) * from <= 0.0;
                match crossed || !back.moving(now) {
                    true => {
                        self.phase = Phase::Idle;
                        Wake::Never
                    }
                    false => Wake::Frame,
                }
            }
        }
    }
}

/// What a band wants after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// Nothing: at rest.
    Never,
    /// The next frame: springing back.
    Frame,
    /// A frame at this time: to see whether the fingers have let go.
    At(Instant),
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
        look
    }
}

/// How far the band in `tree` is drawn moved, in a scrollable `dimension`
/// tall, and the move itself.
fn moved(tree: &Tree, dimension: f32) -> (f32, Transformation) {
    let band = tree.state.downcast_ref::<Band>();
    let offset = band.offset(band.now.unwrap_or_else(Instant::now), dimension);
    (offset, Transformation::translate(0.0, offset))
}

impl<M> Widget<M, Theme, iced::Renderer> for Elastic<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Band>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Band::default())
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
        let dimension = bounds.height;
        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                let band = tree.state.downcast_mut::<Band>();
                match band.tick(*now, dimension, TOLD.load(Ordering::Relaxed)) {
                    Wake::Never => {}
                    Wake::Frame => shell.request_redraw(),
                    Wake::At(at) => shell.request_redraw_at(window::RedrawRequest::At(at)),
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { .. }) => {
                let band = tree.state.downcast_mut::<Band>();
                if band.active() {
                    band.reset();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some((x, y)) = pulls(*delta, motion::reduced()) {
                    if self.wheel(tree, x, y, inner, cursor, renderer, clipboard, shell, viewport) {
                        return;
                    }
                }
            }
            _ => {}
        }
        let (offset, moved) = moved(tree, dimension);
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
        if offset == 0.0 {
            return content.draw(&tree.children[0], renderer, theme, style, inner, cursor, viewport);
        }
        // Moved, but only ever drawn where it is: what it pulls away from
        // shows what is behind it, and nothing below it is drawn over.
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let inverse = moved.inverse();
        renderer.with_layer(clip, |renderer| {
            renderer.with_transformation(moved, |renderer| {
                content.draw(&tree.children[0], renderer, theme, style, inner, cursor * inverse, &(clip * inverse));
            });
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

impl<M> Elastic<'_, M> {
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
        let dimension = inner.bounds().height;
        // Moved nowhere: the fingers lifted.
        if x == 0.0 && y == 0.0 {
            TOLD.store(true, Ordering::Relaxed);
            let band = tree.state.downcast_mut::<Band>();
            if band.active() {
                band.let_go(now, dimension);
                shell.request_redraw();
            }
            return false;
        }
        let Some(over) = cursor.position_over(inner.bounds()) else {
            return false;
        };
        let look = self.look(tree, inner, renderer);
        let Some((outer, content, scrolled)) = look.outer else {
            return false;
        };
        // A scrollable inside, under the pointer, has the wheel.
        let at = over + Vector::new(0.0, scrolled);
        if look.inner.iter().any(|b| b.contains(at)) {
            return false;
        }
        let band = tree.state.downcast_mut::<Band>();
        let was = band.active();
        let left = band.wheel(y, now, dimension);
        if was || band.active() {
            shell.request_redraw();
        }
        if left == 0.0 && x == 0.0 {
            shell.capture_event();
            return true;
        }
        // Where the scroll would go, and how much of the wheel's motion it
        // has no room for.
        let room = (content.height - outer.height).max(0.0);
        let wanted = scrolled - left;
        let reached = wanted.clamp(0.0, room);
        let past = reached - wanted;
        let event = Event::Mouse(mouse::Event::WheelScrolled {
            delta: ScrollDelta::Pixels { x, y: left },
        });
        self.content
            .as_widget_mut()
            .update(&mut tree.children[0], &event, inner, cursor, renderer, clipboard, shell, viewport);
        // It scrolled as far as it could, and the rest is past its end —
        // unless something else in it took the wheel.
        if past != 0.0 {
            let after = self.look(tree, inner, renderer).outer.map(|(_, _, s)| s);
            if after.is_some_and(|s| (s - reached).abs() < 0.5) {
                tree.state.downcast_mut::<Band>().overshoot(past, now, dimension);
                shell.capture_event();
                shell.request_redraw();
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

    /// Past the top the wheel pulls the band, ever less drawn; back the
    /// other way it gives the band back first, and only what is left over
    /// scrolls. Falsified by a band that let the scrollable have the wheel
    /// while still pulled: the way back scrolled the list under a page
    /// still drawn low.
    #[test]
    fn the_wheel_pulls_the_band_and_takes_it_back_first() {
        let (t, d) = (Instant::now(), 600.0);
        let mut band = Band::default();
        assert_eq!(band.wheel(40.0, t, d), 40.0, "not pulled: all of it scrolls");
        band.overshoot(30.0, t, d);
        assert_eq!(band.pull(t, d), 30.0);
        assert_eq!(band.wheel(50.0, t + ms(8), d), 0.0, "further out is the band's");
        assert_eq!(band.pull(t, d), 80.0);
        assert_eq!(band.offset(t, d), rubberband(80.0, d));
        assert!(band.offset(t, d) < 80.0 * 0.55);
        assert_eq!(band.wheel(-20.0, t + ms(16), d), 0.0, "back in, the band's first");
        assert_eq!(band.pull(t, d), 60.0);
        assert_eq!(band.wheel(-100.0, t + ms(24), d), -40.0, "what is left over scrolls");
        assert!(!band.active());
        // And the same past the foot, the other way.
        band.overshoot(-25.0, t, d);
        assert!(band.offset(t, d) < 0.0, "drawn up past the foot");
        assert_eq!(band.wheel(10.0, t + ms(8), d), 0.0);
        assert_eq!(band.pull(t, d), -15.0);
    }

    /// Let go, it springs back from where it is drawn and as fast as it was
    /// drawn going, comes to the edge and stops there — however hard it was
    /// going back in — and is at rest within a second, when it stops
    /// asking for frames. Falsified by a release from a standstill (a
    /// moment on it had not gone on out), or by the spring's overshoot
    /// drawn (the page dipped past its top).
    #[test]
    fn let_go_it_springs_back_to_the_edge_and_no_further() {
        let (t, d) = (Instant::now(), 600.0);
        let mut band = Band::default();
        band.overshoot(10.0, t, d);
        for n in 1..=6 {
            band.wheel(20.0, t + ms(8 * n), d);
        }
        let lift = t + ms(48);
        let drawn = band.offset(lift, d);
        band.let_go(lift, d);
        assert!((band.offset(lift, d) - drawn).abs() < 1e-3, "no jump");
        assert!(band.offset(lift + ms(10), d) > drawn, "still going out, a moment on");
        let mut frames = 0;
        for n in 0..200u64 {
            let at = lift + ms(n * 5);
            assert!(band.offset(at, d) >= 0.0, "{n}: past the edge");
            if band.tick(at, d, true) == Wake::Frame {
                frames += 1;
            }
        }
        assert!(frames > 10 && frames < 200, "{frames}");
        assert!(!band.active(), "at rest");
        assert_eq!(band.offset(lift + ms(1000), d), 0.0);

        // Going back in hard when let go: it reaches the edge, and stops.
        let mut band = Band::default();
        band.overshoot(400.0, t, d);
        for n in 1..=4 {
            band.wheel(-80.0, t + ms(4 * n), d);
        }
        let lift = t + ms(16);
        band.let_go(lift, d);
        for n in 0..200u64 {
            let at = lift + ms(n * 2);
            assert!(band.offset(at, d) >= 0.0, "{n}: past the edge");
        }
        assert_eq!(band.tick(lift + ms(400), d, true), Wake::Never);
    }

    /// A wheel that turns in lines never pulls, nor does any wheel where
    /// motion is reduced. Falsified by banding a mouse's notches: a list
    /// scrolled to its end by a mouse wheel jumped past it.
    #[test]
    fn only_a_trackpad_pulls_and_never_with_less_motion() {
        assert_eq!(pulls(ScrollDelta::Pixels { x: 0.0, y: 12.0 }, false), Some((0.0, 12.0)));
        assert_eq!(pulls(ScrollDelta::Lines { x: 0.0, y: 1.0 }, false), None);
        assert_eq!(pulls(ScrollDelta::Pixels { x: 0.0, y: 12.0 }, true), None);
    }

    /// Fingers that stop: where the system says when they lift, only that
    /// (or a long hold) lets go; where it never says, a short stillness
    /// does. After the lift, what comes on the same way is the trackpad
    /// coasting and is spent; a pull back in grabs the band from where it
    /// is drawn. Falsified by a coast that pulled the band out again as it
    /// sprang back.
    #[test]
    fn it_lets_go_when_the_fingers_stop_and_spends_the_coast() {
        let (t, d) = (Instant::now(), 600.0);
        let mut band = Band::default();
        band.overshoot(60.0, t, d);
        assert_eq!(band.tick(t + ms(100), d, false), Wake::At(t + LET_GO), "not yet");
        assert_eq!(band.tick(t + LET_GO, d, false), Wake::Frame, "still that long: let go");
        assert!(band.active());

        let mut band = Band::default();
        band.overshoot(60.0, t, d);
        assert_eq!(band.tick(t + LET_GO, d, true), Wake::At(t + HELD), "told: held still is still down");
        band.let_go(t + ms(20), d);
        let drawn = band.offset(t + ms(40), d);
        assert_eq!(band.wheel(15.0, t + ms(40), d), 0.0, "the coast is spent");
        assert!(band.offset(t + ms(40), d) <= drawn + 1e-3, "and does not pull it out");
        assert_eq!(band.wheel(-5.0, t + ms(48), d), 0.0, "a pull back in grabs it");
        let grabbed = band.pull(t + ms(48), d);
        assert!(grabbed > 0.0 && grabbed < 60.0, "{grabbed}");
        band.reset();
        assert!(!band.active(), "a key puts it back at once");
    }
}
