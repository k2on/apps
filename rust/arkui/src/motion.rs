//! Motion that runs a while and stops: something shown and hidden with a
//! move each way ([`Shown`]), the curves it moves along ([`Ease`]), and a
//! widget that draws what it holds moved, scaled and veiled
//! ([`transformed`]).
//!
//! **Nothing ticks at rest.** A [`Shown`] is a moment and a direction; how
//! far along it is, is worked out from the time whenever it is drawn. So an
//! app keeps a frame clock running while [`Shown::moving`] says so, draws
//! each frame from [`Shown::at`], and drops what was hidden once
//! [`Shown::gone`].
//!
//! **A fade is a veil.** iced draws each widget's colours as they are and
//! has no opacity for a group of them, so something cannot be faded as a
//! whole. What it can do is lay over it the colour of what is behind it,
//! less and less: over a page dimmed to its own ground, which is where a
//! dialog or a sheet is, that looks the same as fading it.
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::time::{Duration, Instant};
use iced::{Border, Color, Element, Event, Length, Rectangle, Size, Transformation, Vector};

/// A cubic Bézier curve from (0, 0) to (1, 1) through two control points, as
/// CSS's `cubic-bezier(x1, y1, x2, y2)`: how far along a move is (y) at how
/// far through its time (x).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ease(pub f32, pub f32, pub f32, pub f32);

/// CSS's `ease`: what Tailwind's `animate-in` and `animate-out` move along
/// unless told otherwise, so what shadcn/ui's dialog does.
pub const EASE: Ease = Ease(0.25, 0.1, 0.25, 1.0);

/// Tailwind's `ease-in-out`: shadcn/ui's sheet.
pub const EASE_IN_OUT: Ease = Ease(0.4, 0.0, 0.2, 1.0);

impl Ease {
    /// How far along at `t` of the way through, both 0 to 1.
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 || t == 1.0 {
            return t;
        }
        let Ease(x1, y1, x2, y2) = self;
        // One coordinate of the curve at parameter `s`, from its two
        // control points' own.
        let curve = |a: f32, b: f32, s: f32| {
            let r = 1.0 - s;
            3.0 * r * r * s * a + 3.0 * r * s * s * b + s * s * s
        };
        // The parameter where the curve is `t` across: it only ever goes
        // right, so halving the interval finds it.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..32 {
            let mid = (lo + hi) / 2.0;
            if curve(x1, x2, mid) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        curve(y1, y2, (lo + hi) / 2.0)
    }
}

/// How long showing takes, how long hiding does, and the curve both go
/// along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub opening: Duration,
    pub closing: Duration,
    pub ease: Ease,
}

/// Something shown, or being hidden, and from when: how far shown it is at
/// a moment, 0 (not at all) to 1 (all the way).
///
/// Hidden part of the way through showing, it goes back from where it had
/// got to, not from all the way; and so does a show part of the way through
/// hiding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    timing: Timing,
    shown: bool,
    since: Instant,
    from: f32,
}

impl Shown {
    /// Coming into view, from nothing, at `now`.
    pub fn new(timing: Timing, now: Instant) -> Self {
        Shown {
            timing,
            shown: true,
            since: now,
            from: 0.0,
        }
    }

    /// Whether it is showing, or on its way out.
    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// How far through its time it is at `now`, by `timing` rather than
    /// its own: what goes with it at another pace, as a dialog's dimming
    /// does with the dialog.
    pub fn through(&self, timing: Timing, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.since).as_secs_f32();
        let of = |d: Duration| d.as_secs_f32().max(f32::EPSILON);
        if self.shown {
            (self.from + elapsed / of(timing.opening)).min(1.0)
        } else {
            (self.from - elapsed / of(timing.closing)).max(0.0)
        }
    }

    /// How far shown it is at `now`: through its time, along its curve.
    pub fn at(&self, now: Instant) -> f32 {
        self.timing.ease.at(self.through(self.timing, now))
    }

    /// How far shown, at `timing`'s pace and along its curve.
    pub fn at_pace(&self, timing: Timing, now: Instant) -> f32 {
        timing.ease.at(self.through(timing, now))
    }

    /// On its way out, from wherever it is at `now`.
    pub fn hide(&mut self, now: Instant) {
        if self.shown {
            self.from = self.through(self.timing, now);
            self.shown = false;
            self.since = now;
        }
    }

    /// Back into view, from wherever it is at `now`.
    pub fn show(&mut self, now: Instant) {
        if !self.shown {
            self.from = self.through(self.timing, now);
            self.shown = true;
            self.since = now;
        }
    }

    /// Whether it is still moving at `now`: a frame to draw yet. True for
    /// [`LAST_FRAME`] past its end too, so the frame drawn at the end, not
    /// the one just before it, is the last: an app runs its frame clock by
    /// this, and a clock stopped a tick early leaves the move a frame short
    /// of done on the screen until something else happens.
    pub fn moving(&self, now: Instant) -> bool {
        let t = self.through(self.timing, now.checked_sub(LAST_FRAME).unwrap_or(now));
        if self.shown {
            t < 1.0
        } else {
            t > 0.0
        }
    }

    /// Whether it has been hidden all the way: what to stop drawing.
    pub fn gone(&self, now: Instant) -> bool {
        !self.shown && self.through(self.timing, now) <= 0.0
    }
}

/// How long past its end a move still counts as moving: a few frames, so
/// that one is drawn at its end ([`Shown::moving`]).
pub const LAST_FRAME: Duration = Duration::from_millis(50);

/// A move from one place to another over a time, along a curve: where a
/// row is on its way out of the way of one being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    from: f32,
    to: f32,
    since: Instant,
    taking: Duration,
    ease: Ease,
}

impl Slide {
    /// At `at`, not moving.
    pub fn still(at: f32, now: Instant) -> Self {
        Slide {
            from: at,
            to: at,
            since: now,
            taking: Duration::ZERO,
            ease: EASE,
        }
    }

    /// From `from` to `to`, starting `now`, taking `taking` along `ease`.
    pub fn new(from: f32, to: f32, now: Instant, taking: Duration, ease: Ease) -> Self {
        Slide {
            from,
            to,
            since: now,
            taking,
            ease,
        }
    }

    /// Where it is going.
    pub fn target(&self) -> f32 {
        self.to
    }

    /// Where it is at `now`.
    pub fn at(&self, now: Instant) -> f32 {
        if self.taking.is_zero() {
            return self.to;
        }
        let t = now.saturating_duration_since(self.since).as_secs_f32() / self.taking.as_secs_f32();
        self.from + (self.to - self.from) * self.ease.at(t)
    }

    /// Off to `to` from wherever it is at `now`, unless it is already going
    /// there.
    pub fn toward(&mut self, to: f32, now: Instant, taking: Duration, ease: Ease) {
        if self.to != to {
            *self = Slide::new(self.at(now), to, now, taking, ease);
        }
    }

    /// Whether it is still on its way at `now`, and for [`LAST_FRAME`] after.
    pub fn moving(&self, now: Instant) -> bool {
        self.from != self.to && now.saturating_duration_since(self.since) < self.taking + LAST_FRAME
    }
}

/// `content` drawn moved by `by`, scaled by `scale` about its middle, and
/// under a veil of the theme's ground (`background.base`), `veil` thick
/// (0 for none, 1 for the ground alone) and rounded to `radius`. It
/// is laid out where it would be, unmoved, so nothing around it moves; and a
/// pointer over it is taken back through the same move, so what it holds
/// answers where it is drawn.
pub fn transformed<'a, M: 'a>(content: impl Into<Element<'a, M>>, by: Vector, scale: f32, veil: f32, radius: f32) -> Element<'a, M> {
    Element::new(Transformed {
        content: content.into(),
        by,
        scale,
        veil,
        radius,
    })
}

struct Transformed<'a, M> {
    content: Element<'a, M>,
    by: Vector,
    scale: f32,
    veil: f32,
    radius: f32,
}

impl<M> Transformed<'_, M> {
    fn transformation(&self, bounds: Rectangle) -> Transformation {
        let (cx, cy) = (bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0);
        Transformation::translate(cx + self.by.x, cy + self.by.y) * Transformation::scale(self.scale) * Transformation::translate(-cx, -cy)
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Transformed<'_, M> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        let inverse = self.transformation(layout.bounds()).inverse();
        self.content
            .as_widget_mut()
            .update(tree, event, layout, cursor * inverse, renderer, clipboard, shell, &(*viewport * inverse));
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let bounds = layout.bounds();
        let transformation = self.transformation(bounds);
        let inverse = transformation.inverse();
        renderer.with_transformation(transformation, |renderer| {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor * inverse, &(*viewport * inverse));
            // In a layer of its own: within one, iced draws text after
            // every quad, so a veil in the content's would cover its
            // ground and leave its words showing.
            if self.veil > 0.0 {
                let ground: Color = crate::theme::of(theme).background.base.color;
                renderer.with_layer(bounds, |renderer| {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds,
                            border: Border {
                                radius: self.radius.into(),
                                ..Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        ground.scale_alpha(self.veil.min(1.0)),
                    );
                });
            }
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
        let inverse = self.transformation(layout.bounds()).inverse();
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor * inverse, &(*viewport * inverse), renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn widget::Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        offset: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Timing = Timing {
        opening: Duration::from_millis(200),
        closing: Duration::from_millis(100),
        ease: EASE,
    };

    /// CSS's `ease` starts slow, passes the middle early and settles: at a
    /// quarter of the time it is some 40% along, at half some 80%. Falsified
    /// by a linear curve, or one solved for y where x was meant: at a
    /// quarter it was a quarter, or past 60%.
    #[test]
    fn ease_is_css_ease() {
        assert_eq!(EASE.at(0.0), 0.0);
        assert_eq!(EASE.at(1.0), 1.0);
        assert!((EASE.at(0.25) - 0.409).abs() < 0.01, "{}", EASE.at(0.25));
        assert!((EASE.at(0.5) - 0.802).abs() < 0.01, "{}", EASE.at(0.5));
        // Tailwind's ease-in-out is not symmetrical: it lands softly, some
        // 78% of the way at half the time.
        assert!((EASE_IN_OUT.at(0.5) - 0.776).abs() < 0.01);
    }

    /// It shows over its opening, hides over its closing, and a hide halfway
    /// through showing goes back from halfway, so it is gone in half the
    /// closing. Falsified by a hide that began from all the way: it was
    /// still there after half the closing.
    #[test]
    fn a_hide_part_way_goes_back_from_where_it_got_to() {
        let at = Instant::now();
        let mut s = Shown::new(T, at);
        assert_eq!(s.at(at), 0.0);
        assert!(s.moving(at + T.opening / 2));
        assert_eq!(s.at(at + T.opening), 1.0);
        assert!(s.moving(at + T.opening), "a frame drawn at its end");
        assert!(!s.moving(at + T.opening + LAST_FRAME));
        s.hide(at + T.opening / 2);
        assert!(!s.is_shown());
        assert!(!s.gone(at + T.opening / 2 + T.closing / 4));
        assert!(s.gone(at + T.opening / 2 + T.closing / 2));
    }

    /// A slide sent somewhere else on its way starts from where it is, so
    /// nothing jumps. Falsified by one that restarted from where it began.
    #[test]
    fn a_slide_turned_on_its_way_starts_from_where_it_is() {
        let at = Instant::now();
        let d = Duration::from_millis(100);
        let mut s = Slide::new(0.0, 10.0, at, d, EASE_IN_OUT);
        let half = at + d / 2;
        let there = s.at(half);
        s.toward(-10.0, half, d, EASE_IN_OUT);
        assert!((s.at(half) - there).abs() < 1e-4);
        assert_eq!(s.at(half + d), -10.0);
        assert!(!s.moving(half + d + LAST_FRAME));
    }
}
