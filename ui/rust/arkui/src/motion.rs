//! Motion that runs a while and stops: a value on a spring ([`Slide`]),
//! something shown and hidden on one ([`Shown`]), the spring itself
//! ([`Spring`]), whether to move at all ([`reduced`]), and a widget that
//! draws what it holds moved, scaled and veiled ([`transformed`]).
//!
//! **A move is a spring, not a curve over a time.** A curve has a length:
//! sent back half way, it either finishes first or starts again from
//! somewhere and jumps, and either way its speed breaks. A spring has only
//! somewhere to be and how fast it is going, so sent somewhere else on its
//! way it turns from where it is drawn at the speed it was drawn at — a
//! sheet closed as it slides in slows, stops and goes back, and a row let
//! go mid-drag carries on at the pointer's speed. It is set as Apple sets
//! one ([`Spring`]): how much it is damped, and how quickly it gets there.
//!
//! **Nothing ticks at rest.** A [`Slide`] is where it set off from, how
//! fast, toward where and when; where it is, is worked out from the time
//! whenever it is drawn — a spring has a closed form, so no step is kept.
//! So an app keeps a frame clock running while [`Shown::moving`] says so,
//! draws each frame from [`Shown::at`], and drops what was hidden once
//! [`Shown::gone`].
//!
//! **A fade is a veil.** iced draws each widget's colours as they are and
//! has no opacity for a group of them, so something cannot be faded as a
//! whole. What it can do is lay over it the colour of what is behind it,
//! less and less: over a page dimmed to its own ground, which is where a
//! dialog or a sheet is, that looks the same as fading it.
//!
//! **Less motion is a fade.** Where the system asks for less motion
//! ([`reduced`]), what slides or grows only fades, quickly, and a row out
//! of the way of a drag is there at once: the change is still seen, but
//! nothing travels across the screen.
use std::sync::OnceLock;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::time::{Duration, Instant};
use iced::{Border, Color, Element, Event, Length, Rectangle, Size, Transformation, Vector};

use crate::motion_tokens as tokens;

/// A spring as Apple's interface designers set one: how much it is damped
/// and how quickly it responds, rather than a mass, a stiffness and a
/// damping that each change both.
///
/// `damping` is the ratio to just enough to come to rest without going
/// past: 1 settles as fast as it can and never overshoots, below it the
/// value swings past and back. `response` is roughly how long it takes to
/// get there, in seconds — the period it would swing at undamped. It is
/// not a duration: a spring has none, and settles when it settles. Zero is
/// no spring at all: there at once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub damping: f32,
    pub response: f32,
}

/// What most moves go on: a page, a dialog, a sheet, a row out of a
/// drag's way, the small header. Critically damped ([`tokens::MOVE`]), so
/// it lands without a bounce, at the one response every toolkit here moves
/// at ([`tokens::RESPONSE`], the phones' 700 stiffness: about 0.24 s) —
/// written once, in `ui/theme.toml`'s `[motion]`.
pub const SMOOTH: Spring = Spring {
    damping: tokens::MOVE,
    response: tokens::RESPONSE,
};

/// What a thing thrown settles on: the same response, a little under
/// critically damped ([`tokens::MOMENTUM`]), so one let go at speed swings
/// a hair past and comes back as a thing thrown does. Only ever chosen by
/// [`released`], never for a move a tap or the app made.
pub const THROWN: Spring = Spring {
    damping: tokens::MOMENTUM,
    response: tokens::RESPONSE,
};

/// How large a thing is and how much of it shows — a scale, a dim, a
/// veil: never past its size ([`tokens::SIZE`]).
pub const SIZE: Spring = Spring {
    damping: tokens::SIZE,
    response: tokens::RESPONSE,
};

/// What a release settles on, set off at `speed` (pixels a second): one
/// at least a flick's speed ([`tokens::FLICK`]) was thrown, and spends
/// its momentum ([`THROWN`]); slower, it was put down ([`SMOOTH`]). The
/// phones' `Springs.settle` and `Springs.released`.
pub fn released(speed: f32) -> Spring {
    match speed.abs() >= tokens::FLICK {
        true => THROWN,
        false => SMOOTH,
    }
}

/// Apple's rubber band: `overshoot` pixels past an edge, in room
/// `dimension` tall (the view it is pulled in), drawn
/// `overshoot · dimension · c / (dimension + c · |overshoot|)` past it, `c`
/// [`tokens::RUBBER_BAND`] — the iPhone's `Momentum.band` and the same
/// token. It gives `c` of a pixel for each pixel at first and ever less
/// after, and never reaches `dimension`: "there is nothing more here",
/// not a wall.
pub fn rubberband(overshoot: f32, dimension: f32) -> f32 {
    let c = tokens::RUBBER_BAND;
    match dimension > 0.0 {
        true => overshoot * dimension * c / (dimension + c * overshoot.abs()),
        false => 0.0,
    }
}

/// How many pixels [`rubberband`] draws for each pixel more of pull, at
/// `overshoot` past the edge: its slope, `c · d² / (d + c · |x|)²`. What a
/// speed in pull becomes as drawn, so a band let go springs back as fast
/// as it was seen going.
pub fn rubberband_slope(overshoot: f32, dimension: f32) -> f32 {
    let c = tokens::RUBBER_BAND;
    match dimension > 0.0 {
        true => c * dimension * dimension / (dimension + c * overshoot.abs()).powi(2),
        false => 0.0,
    }
}

/// Where a release at `speed` (pixels a second) comes to rest if it keeps
/// slowing as a scroll does, [`tokens::DECELERATION`] of its speed kept each
/// millisecond: Apple's `project`, `(v / 1000) · d / (1 − d)`. The phones'
/// `Springs.project` and `Momentum.project`.
pub fn project(speed: f32) -> f32 {
    let d = tokens::DECELERATION;
    speed / 1000.0 * d / (1.0 - d)
}

/// A scroll let go at `speed` (pixels a second), `ms` milliseconds on: how
/// far it has gone, and how fast it is going — its speed kept
/// [`tokens::DECELERATION`] a millisecond, `v · dᵗ`, and the distance that
/// speed covers, `v · (dᵗ − 1) / (1000 · ln d)`, which comes to
/// [`project`] (to a tenth of a percent: this is the decay without the
/// millisecond steps). A closed form, so nothing is stepped and a late
/// frame lands where it would have.
pub fn coast(speed: f32, ms: f32) -> (f32, f32) {
    let d = tokens::DECELERATION as f64;
    let kept = d.powf(ms.max(0.0) as f64);
    let gone = speed as f64 * (kept - 1.0) / (1000.0 * d.ln());
    (gone as f32, (speed as f64 * kept) as f32)
}

/// How far past the edge a band drawn `drawn` past it was pulled: the
/// inverse of [`rubberband`], so a band grabbed again as it springs back
/// carries on from where it is drawn.
pub fn unband(drawn: f32, dimension: f32) -> f32 {
    let c = tokens::RUBBER_BAND;
    if dimension <= 0.0 {
        return 0.0;
    }
    // Never quite the whole room, which no pull reaches.
    let at = drawn.abs().min(dimension * 0.999);
    drawn.signum() * at * dimension / (c * (dimension - at))
}

/// The longest a settle may run, whatever a release handed it
/// ([`tokens::CAP_MS`]): a critically damped one is at rest first, so this
/// bounds only a release the tuning did not see.
pub const CAP: Duration = Duration::from_millis(tokens::CAP_MS);

/// No spring: there at once.
pub const INSTANT: Spring = Spring { damping: 1.0, response: 0.0 };

/// A fade, a hover, and what a move is where the system asks for less
/// motion: quick enough to read as a cut softened, not as travel
/// ([`reduced`], [`tokens::FADE_RESPONSE`], about 150 ms to rest).
pub const FADE: Spring = Spring {
    damping: tokens::SIZE,
    response: tokens::FADE_RESPONSE,
};

impl Spring {
    /// How far from where it is going it is, and how fast it is going, `t`
    /// seconds after it set off `x` from there at `v`: the spring's closed
    /// form, critically damped, under- or over-.
    pub fn state(self, x: f32, v: f32, t: f32) -> (f32, f32) {
        if self.response <= 0.0 {
            return (0.0, 0.0);
        }
        let (x, v, t) = (x as f64, v as f64, t.max(0.0) as f64);
        let w = std::f64::consts::TAU / self.response as f64;
        let z = self.damping.max(0.0) as f64;
        let (x, v) = if (z - 1.0).abs() < 1e-4 {
            let e = (-w * t).exp();
            let b = v + w * x;
            ((x + b * t) * e, (v - w * t * b) * e)
        } else if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            let e = (-z * w * t).exp();
            let b = (v + z * w * x) / wd;
            let (sin, cos) = (wd * t).sin_cos();
            let at = x * cos + b * sin;
            (e * at, e * (-z * w * at + wd * (b * cos - x * sin)))
        } else {
            let s = w * (z * z - 1.0).sqrt();
            let (r1, r2) = (-z * w + s, -z * w - s);
            let c2 = (v - r1 * x) / (r2 - r1);
            let c1 = x - c2;
            let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
            (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
        };
        (x as f32, v as f32)
    }

    /// This spring, or [`INSTANT`] where the system asks for less motion: for
    /// what travels and has no fade to become.
    pub fn or_instant(self) -> Spring {
        match reduced() {
            true => INSTANT,
            false => self,
        }
    }
}

/// A value on a spring: where it set off from and how fast, where it is
/// going, and when. Sent somewhere else on its way ([`Slide::toward`]), it
/// sets off again from where it is and as fast as it is going, so nothing
/// jumps and nothing stops dead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    /// How far from `to` it set off, and how fast.
    off: f32,
    speed: f32,
    to: f32,
    since: Instant,
    spring: Spring,
    /// How near its end, and how slow, is at rest.
    rest: f32,
}

/// How near its end a slide in pixels is at rest ([`tokens::REST_PX`]).
pub const REST: f32 = tokens::REST_PX;

impl Slide {
    /// At `at`, not moving.
    pub fn still(at: f32, now: Instant) -> Self {
        Slide::new(at, at, now, INSTANT)
    }

    /// From `from` to `to`, from a standstill at `now`, on `spring`.
    pub fn new(from: f32, to: f32, now: Instant, spring: Spring) -> Self {
        Slide {
            off: from - to,
            speed: 0.0,
            to,
            since: now,
            spring,
            rest: REST,
        }
    }

    /// Setting off at `speed` a second rather than from a standstill: what a
    /// drag let go hands on, so the slide carries on as the pointer went.
    pub fn moving_at(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// At rest within `rest` of its end rather than [`REST`]: for a value
    /// that is not in pixels.
    pub fn resting_within(mut self, rest: f32) -> Self {
        self.rest = rest;
        self
    }

    /// Where it is going.
    pub fn target(&self) -> f32 {
        self.to
    }

    /// How far from its end, and how fast, at `now`.
    fn state(&self, now: Instant) -> (f32, f32) {
        self.spring
            .state(self.off, self.speed, now.saturating_duration_since(self.since).as_secs_f32())
    }

    /// Whether it is at rest at `now`: near enough its end, and slow enough
    /// to stay there, that it is drawn at its end.
    fn at_rest(&self, now: Instant) -> bool {
        let (x, v) = self.state(now);
        x.abs() <= self.rest && v.abs() <= self.rest * 10.0
    }

    /// Where it is at `now`: exactly its end once at rest.
    pub fn at(&self, now: Instant) -> f32 {
        match self.at_rest(now) {
            true => self.to,
            false => self.to + self.state(now).0,
        }
    }

    /// How fast it is going at `now`, a second.
    pub fn speed(&self, now: Instant) -> f32 {
        match self.at_rest(now) {
            true => 0.0,
            false => self.state(now).1,
        }
    }

    /// Off to `to` on `spring` from wherever it is at `now`, as fast as it
    /// is going there, unless it is already going there.
    pub fn toward(&mut self, to: f32, now: Instant, spring: Spring) {
        if self.to != to {
            let (at, speed) = (self.at(now), self.speed(now));
            *self = Slide::new(at, to, now, spring).moving_at(speed).resting_within(self.rest);
        }
    }

    /// Whether it is still on its way at `now`, and for [`LAST_FRAME`] after
    /// it came to rest, so the frame drawn at rest, not the one just before
    /// it, is the last: an app runs its frame clock by this, and a clock
    /// stopped a tick early leaves the move a frame short of done on the
    /// screen until something else happens.
    pub fn moving(&self, now: Instant) -> bool {
        !self.at_rest(now.checked_sub(LAST_FRAME).unwrap_or(now))
    }
}

/// Something shown, or being hidden: how far shown it is at a moment, 0
/// (not at all) to 1 (all the way), on a spring.
///
/// Hidden part of the way through showing, it turns back from where it had
/// got to and as fast as it was going, not from all the way; and so does a
/// show part of the way through hiding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    slide: Slide,
}

/// How near shown or hidden is all the way ([`tokens::REST`]): well under
/// a pixel of a sheet's slide.
const SHOWN_REST: f32 = tokens::REST;

impl Shown {
    /// Coming into view, from nothing, at `now`, on `spring` — or a quick
    /// fade ([`FADE`]) where the system asks for less motion.
    pub fn new(spring: Spring, now: Instant) -> Self {
        let spring = match reduced() {
            true => FADE,
            false => spring,
        };
        Shown {
            slide: Slide::new(0.0, 1.0, now, spring).resting_within(SHOWN_REST),
        }
    }

    /// Whether it is showing, or on its way out.
    pub fn is_shown(&self) -> bool {
        self.slide.target() == 1.0
    }

    /// How far shown it is at `now`, 0 to 1: a spring that swings a little
    /// past either end is drawn at that end.
    pub fn at(&self, now: Instant) -> f32 {
        self.slide.at(now).clamp(0.0, 1.0)
    }

    /// On its way out, from wherever it is at `now`.
    pub fn hide(&mut self, now: Instant) {
        let spring = self.slide.spring;
        self.slide.toward(0.0, now, spring);
    }

    /// Where it is going, at once: what a key opened or closed is there or
    /// gone on the frame the key went down (ui/docs/motion.md, the frequency
    /// rule). Its spring is kept, for a later show or hide by the pointer.
    pub fn snap(&mut self, now: Instant) {
        let to = self.slide.target();
        self.slide = Slide::new(to, to, now, self.slide.spring).resting_within(self.slide.rest);
    }

    /// Back into view, from wherever it is at `now`.
    pub fn show(&mut self, now: Instant) {
        let spring = self.slide.spring;
        self.slide.toward(1.0, now, spring);
    }

    /// Whether it is still moving at `now`: a frame to draw yet
    /// ([`Slide::moving`]).
    pub fn moving(&self, now: Instant) -> bool {
        self.slide.moving(now)
    }

    /// Whether it has been hidden all the way: what to stop drawing.
    pub fn gone(&self, now: Instant) -> bool {
        !self.is_shown() && self.slide.at(now) <= 0.0
    }
}

/// What wakes a window that has something to draw by itself: the
/// display's frames while something moves ([`Clock::Frames`]) — never a
/// wall-clock timer, which drifts against a 60 Hz display and caps a 120
/// Hz one — or, for what only needs a nudge now and then (a scrubber, a
/// dwell), a timer at that period. A window at rest has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    /// Every frame the compositor draws (`iced::window::frames`).
    Frames,
    /// A timer, this often.
    Every(Duration),
}

impl Clock {
    /// Its ticks, as a subscription saying nothing but that it ticked.
    pub fn ticks(self) -> iced::Subscription<()> {
        match self {
            Clock::Frames => iced::window::frames().map(|_| ()),
            Clock::Every(every) => iced::time::every(every).map(|_| ()),
        }
    }
}

/// How long past coming to rest a move still counts as moving: a few
/// frames, so that one is drawn at rest ([`Slide::moving`]).
pub const LAST_FRAME: Duration = Duration::from_millis(50);

/// Animate whatever the system says. While true, the system's asking for
/// less motion is not listened to, and everything moves as it would
/// without it; `ARK_REDUCE_MOTION` still is ([`reduced`]).
///
/// **On for now, on purpose.** The desktop these windows are made on asks
/// for less motion, and its owner still wants to see the springs while they
/// are being worked on. Once that is done this goes, or becomes false, and
/// the system is listened to.
pub const ALWAYS_ANIMATE: bool = true;

/// Whether to keep motion down: what slides or grows fades instead, and a
/// row out of the way is there at once.
///
/// Asked once, the first time anything wants it, and kept — an app asks
/// at startup, so the system is not asked from inside a frame. In order:
/// `ARK_REDUCE_MOTION` (`1` less motion, `0` all of it), then, unless
/// [`ALWAYS_ANIMATE`], the system — GNOME's `enable-animations` on Linux,
/// Accessibility's Reduce motion on macOS — and otherwise all of it. A
/// system that cannot be asked (no `gsettings`) is all of it.
pub fn reduced() -> bool {
    static REDUCED: OnceLock<bool> = OnceLock::new();
    *REDUCED.get_or_init(|| decide(std::env::var("ARK_REDUCE_MOTION").ok().as_deref(), ALWAYS_ANIMATE, system_reduced))
}

/// [`reduced`], from what `ARK_REDUCE_MOTION` says, whether to listen to
/// the system, and the system — asked only if it is listened to.
fn decide(env: Option<&str>, always: bool, system: impl FnOnce() -> Option<bool>) -> bool {
    match env.map(str::trim) {
        Some("1" | "true" | "yes") => return true,
        Some("0" | "false" | "no") => return false,
        _ => {}
    }
    !always && system().unwrap_or(false)
}

/// What a command prints, trimmed, if it ran and said it went well.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn ask(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether the system asks for less motion: GNOME's animations turned off.
#[cfg(target_os = "linux")]
fn system_reduced() -> Option<bool> {
    match ask("gsettings", &["get", "org.gnome.desktop.interface", "enable-animations"])?.as_str() {
        "false" => Some(true),
        "true" => Some(false),
        _ => None,
    }
}

/// Whether the system asks for less motion: Accessibility's Reduce motion.
#[cfg(target_os = "macos")]
fn system_reduced() -> Option<bool> {
    match ask("defaults", &["read", "com.apple.universalaccess", "reduceMotion"])?.as_str() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Nowhere else is asked.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn system_reduced() -> Option<bool> {
    None
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

    fn diff(&mut self, tree: &mut Tree) {
        self.content.as_widget_mut().diff(tree);
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

    /// Its content's accessibility tree, as is: this wrapper adds nothing a
    /// screen reader reads (COSMIC's accesskit, the fork's `a11y`).
    fn a11y_nodes(&self, layout: Layout<'_>, tree: &Tree, cursor: mouse::Cursor) -> iced_accessibility::A11yTree {
        self.content.as_widget().a11y_nodes(layout, tree, cursor)
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

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Critically damped from a standstill, it never goes past its end and
    /// is well on its way in a third of its response: some 62% by 80 ms of
    /// the shared 0.24 s. Falsified by a response read as a duration, or a
    /// stiffness off by 2π: by 80 ms it was a fifth of the way, or all.
    #[test]
    fn a_critically_damped_spring_lands_without_a_bounce() {
        let at = Instant::now();
        let s = Slide::new(0.0, 100.0, at, SMOOTH);
        assert_eq!(s.at(at), 0.0);
        let mid = s.at(at + ms(80));
        assert!((58.0..68.0).contains(&mid), "{mid}");
        let mut last = 0.0;
        for n in 0..100 {
            let x = s.at(at + ms(n * 10));
            assert!(x >= last && x <= 100.0, "{n}: {x} after {last}");
            last = x;
        }
        assert_eq!(s.at(at + Duration::from_secs(1)), 100.0, "at rest, exactly there");
        assert!(!s.moving(at + Duration::from_secs(1)));
    }

    /// Less damped, it swings past and comes back; more, it only creeps in.
    /// Falsified by the underdamped form's sine and cosine swapped: it began
    /// a long way off.
    #[test]
    fn a_looser_spring_swings_past() {
        let at = Instant::now();
        let loose = Spring { damping: 0.5, response: 0.3 };
        let s = Slide::new(0.0, 100.0, at, loose);
        assert!(s.at(at).abs() < 1e-3);
        let most = (0..100).map(|n| s.at(at + ms(n * 5))).fold(0.0f32, f32::max);
        assert!(most > 105.0, "{most}");
        let stiff = Slide::new(0.0, 100.0, at, Spring { damping: 2.0, response: 0.3 });
        assert!((0..100).all(|n| stiff.at(at + ms(n * 5)) <= 100.0));
        assert!(stiff.at(at + ms(120)) < s.at(at + ms(120)));
    }

    /// Sent back on its way, it turns from where it is drawn and as fast as
    /// it was going: no jump, and no speed thrown away — a moment later it
    /// is still going the old way, slowing. Falsified by a re-target that
    /// set off from a standstill: a moment later it had already turned.
    #[test]
    fn a_slide_turned_on_its_way_keeps_its_place_and_its_speed() {
        let at = Instant::now();
        let mut s = Slide::new(0.0, 100.0, at, SMOOTH);
        let half = at + ms(60);
        let (there, speed) = (s.at(half), s.speed(half));
        assert!(speed > 0.0);
        s.toward(0.0, half, SMOOTH);
        assert!((s.at(half) - there).abs() < 1e-3, "no jump");
        assert!((s.speed(half) - speed).abs() < 1e-2 * speed, "no break in its speed");
        assert!(s.at(half + ms(10)) > there, "still going the old way, a moment on");
        assert_eq!(s.at(half + Duration::from_secs(2)), 0.0);
        assert!(!s.moving(half + Duration::from_secs(2)));
    }

    /// It shows, comes to rest, and hides; hidden part of the way through
    /// showing it turns back from there, so it is gone sooner than one
    /// hidden from all the way. Falsified by a hide that began from all the
    /// way: it took as long to go as the other.
    #[test]
    fn a_hide_part_way_goes_back_from_where_it_got_to() {
        let at = Instant::now();
        let s = Shown::new(SMOOTH, at);
        assert_eq!(s.at(at), 0.0);
        assert!(s.moving(at + ms(100)));
        let open = at + Duration::from_secs(1);
        assert_eq!(s.at(open), 1.0);
        assert!(!s.moving(open + LAST_FRAME));
        let mut whole = s;
        whole.hide(open);
        let mut part = Shown::new(SMOOTH, open);
        part.hide(open + ms(60));
        assert!(!part.is_shown());
        assert!(part.at(open + ms(60)) < 1.0, "from where it got to");
        let ends = |s: &Shown, from: Instant| (0..400u64).find(|&n| s.gone(from + ms(n * 5))).expect("gone");
        assert!(ends(&part, open + ms(60)) < ends(&whole, open), "sooner gone from part of the way");
        assert!(!part.gone(open + ms(80)));
        assert!(part.gone(open + Duration::from_secs(2)));
        assert!(!part.moving(open + Duration::from_secs(2)));
    }

    /// Apple's rubber band: `c` of a pixel per pixel at first, ever less
    /// after, `x·d·c / (d + c·|x|)` exactly, the same either way, never as
    /// far as the room it is pulled in — and [`unband`] undoes it.
    /// Falsified by a band that follows one to one (no `c` on top), or by
    /// `c` read as anything but the token: the pull of a whole room drew
    /// it 0.55/1.55 of the way.
    #[test]
    fn a_pull_past_the_edge_follows_ever_less() {
        let d = 600.0;
        assert_eq!(rubberband(0.0, d), 0.0);
        assert!((rubberband(1.0, d) - tokens::RUBBER_BAND).abs() < 1e-3, "c at first");
        assert!((rubberband(d, d) - d * 0.55 / 1.55).abs() < 1e-3, "{}", rubberband(d, d));
        assert_eq!(rubberband(-120.0, d), -rubberband(120.0, d), "either way alike");
        let mut last = 0.0;
        for n in 1..200 {
            let x = rubberband(n as f32 * 50.0, d);
            assert!(x > last && x < d, "{n}: {x}");
            assert!(x - last < 50.0 * tokens::RUBBER_BAND, "ever less per pixel");
            last = x;
        }
        assert_eq!(rubberband(100.0, 0.0), 0.0, "no room, no band");
        for x in [-900.0, -40.0, 3.0, 250.0, 2000.0] {
            assert!((unband(rubberband(x, d), d) - x).abs() < 0.05 * x.abs().max(1.0), "{x}");
        }
    }

    /// A coast slows by the token a millisecond and goes, all told, as far
    /// as Apple's projection says; its speed is the slope of where it is.
    /// Falsified by a decay per frame rather than per millisecond (it went
    /// sixteen times as far), or by the sign lost (a coast up went down).
    #[test]
    fn a_coast_goes_as_far_as_the_projection() {
        for v in [-3000.0, -400.0, 250.0, 1800.0] {
            let (all, left) = coast(v, 60_000.0);
            assert!(left.abs() < 1e-3, "{v}: stopped in a minute");
            assert!((all - project(v)).abs() <= 0.002 * project(v).abs(), "{v}: {all} against {}", project(v));
            let (gone, speed) = coast(v, 200.0);
            assert!((speed - v * 0.998f32.powf(200.0)).abs() < 0.01 * v.abs(), "{v}: {speed}");
            let (on, _) = coast(v, 201.0);
            assert!(((on - gone) * 1000.0 - speed).abs() < 0.01 * v.abs(), "{v}: the slope is the speed");
        }
        assert!((project(1000.0) - 499.0).abs() < 0.5, "{}", project(1000.0));
        assert_eq!(coast(1000.0, 0.0), (0.0, 1000.0));
    }

    /// The band's slope is the rate of [`rubberband`]: `c` at the edge,
    /// less and less after. Falsified by a slope without the square (it
    /// fell off half as fast as the band does).
    #[test]
    fn the_band_slope_is_its_rate() {
        let d = 200.0;
        assert!((rubberband_slope(0.0, d) - tokens::RUBBER_BAND).abs() < 1e-6);
        for x in [-300.0f32, -10.0, 5.0, 80.0, 900.0] {
            let rate = (rubberband(x + 0.01, d) - rubberband(x - 0.01, d)) / 0.02;
            assert!(
                (rubberband_slope(x, d) - rate).abs() < 1e-3,
                "{x}: {} against {rate}",
                rubberband_slope(x, d)
            );
        }
    }

    /// `ARK_REDUCE_MOTION` says first; then the system, only when listened
    /// to — and not asked at all while [`ALWAYS_ANIMATE`] holds. Falsified
    /// by the override turned on its head: the system's "less" was obeyed.
    #[test]
    fn who_says_how_much_to_move() {
        let unasked = || -> Option<bool> { panic!("the system was asked") };
        assert!(decide(Some("1"), true, unasked));
        assert!(!decide(Some("0"), false, || Some(true)));
        assert!(!decide(None, true, unasked), "always animating, the system is not asked");
        assert!(decide(None, false, || Some(true)));
        assert!(!decide(None, false, || None), "a system that cannot be asked moves");
        assert!(!decide(Some("maybe"), false, || Some(false)));
    }
}
