//! How the page is made (ui/docs/app.md, "The Browser's Modes"): drawn in
//! the browser alone (the SPA, [`Mode::Spa`]), or written as HTML first —
//! by the prerenderer at build time ([`Mode::Static`]) or by the app's
//! server for each request ([`Mode::Server`]) — and then taken over in
//! the browser (`hydrate`) when script runs.
//!
//! **The address is the route.** A page's [`Route`] is the launch
//! arguments every shell reads (`screen`, `inspector`, `pane`, …) as
//! segments of the path: `/budget/`, `/category:dining/inspector=1/`. The
//! SPA reads `?screen=…` as before and the path besides.
//!
//! **The no-JS contract.** Written as HTML, every control a draw makes is
//! recorded here in the order it is drawn ([`tap`], [`text`]), so the same
//! draw on the server finds it again: a button is a submit button of the
//! page's one form (`#ark-act`) carrying `<index>.<check>`, its check the
//! hash of its label, so a press the current draw cannot make is refused.
//! On a server the press is the control's own action, run as the click
//! would run it; in a static site there is no server, so a control whose
//! press only moves about is a link to the page it leads to (`formaction`
//! of the form `#ark-go`, found by the prerenderer pressing it), and one
//! that changes the data is drawn disabled. In the SPA none of this is
//! drawn and the markup is what it always was.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::Action;

/// How this page is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Drawn in the browser alone, as always.
    #[default]
    Spa,
    /// Prerendered at build time; no server answers its forms.
    Static,
    /// Written by the app's server for each request, which answers its forms.
    Server,
}

impl Mode {
    pub fn word(self) -> &'static str {
        match self {
            Mode::Spa => "spa",
            Mode::Static => "ssg",
            Mode::Server => "ssr",
        }
    }

    pub fn of_word(w: &str) -> Option<Mode> {
        match w {
            "spa" => Some(Mode::Spa),
            "ssg" => Some(Mode::Static),
            "ssr" => Some(Mode::Server),
            _ => None,
        }
    }
}

/// Where a static page's control leads, as the prerenderer found by
/// pressing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lead {
    /// To the page at this path.
    To(String),
    /// Nowhere a page shows without script, or a change to the data: drawn
    /// disabled in a static site.
    Stays,
}

pub(crate) enum Run {
    Tap(Action),
    Text { current: String, typed: Rc<dyn Fn(String)> },
}

pub(crate) struct Control {
    pub label: String,
    pub run: Run,
}

thread_local! {
    static MODE: Cell<Mode> = const { Cell::new(Mode::Spa) };
    static CONTROLS: RefCell<Vec<Control>> = const { RefCell::new(Vec::new()) };
    static LEADS: RefCell<Option<HashMap<usize, Lead>>> = const { RefCell::new(None) };
    /// The width the page was written at, while the browser takes it over:
    /// its first draw must be the server's.
    static WIDTH: Cell<Option<f64>> = const { Cell::new(None) };
    /// What the page was written with besides its route (`today=…`).
    static ARGS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// Whether the page is written for every width at once ([`Band`]), and
    /// the band being drawn.
    static EVERY: Cell<bool> = const { Cell::new(false) };
    static BAND: Cell<Option<Band>> = const { Cell::new(None) };
}

/// A range of the window's width the shell is laid out the same across:
/// `[layout]`'s size classes, and the sidebar's width within expanded. A
/// page written as HTML is written once per band, each in a
/// `.ark-at-<band>` the theme's media queries show at its widths alone
/// (ui/theme.py), so the page is right at every width with no script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// Narrower than `compact-width`: the tab bar at the foot.
    Compact,
    /// From `compact-width` to `expanded-width`: the tab bar at the top,
    /// a split view one column at a time.
    Medium,
    /// From `expanded-width` to `sidebar-min-width`: the tab bar at the
    /// top, a split view's columns side by side.
    Large,
    /// From `sidebar-min-width`: the sidebar.
    Sidebar,
}

impl Band {
    pub const ALL: [Band; 4] = [Band::Compact, Band::Medium, Band::Large, Band::Sidebar];

    pub fn word(self) -> &'static str {
        match self {
            Band::Compact => "compact",
            Band::Medium => "medium",
            Band::Large => "large",
            Band::Sidebar => "sidebar",
        }
    }

    /// A width inside the band: where it starts, by `[layout]`'s tokens.
    pub fn width(self) -> f64 {
        let (compact, expanded, sidebar) = crate::shell::widths();
        match self {
            Band::Compact => 0.0,
            Band::Medium => compact,
            Band::Large => expanded,
            Band::Sidebar => sidebar,
        }
    }
}

/// Whether the page is drawn for every width at once: written natively
/// with no width given, and the browser's first draw of such a page.
pub fn every_width() -> bool {
    EVERY.with(|e| e.get())
}

pub fn set_every_width(on: bool) {
    EVERY.with(|e| e.set(on));
}

/// The band being drawn, while a page is drawn for every width.
pub fn band() -> Option<Band> {
    BAND.with(|b| b.get())
}

pub(crate) fn set_band(b: Option<Band>) {
    BAND.with(|c| c.set(b));
}

/// An element id that is the page's once: suffixed by the band being
/// drawn, as each band draws the same parts.
pub fn unique(id: &str) -> String {
    match band() {
        Some(b) => format!("{id}-{}", b.word()),
        None => id.to_string(),
    }
}

pub fn mode() -> Mode {
    MODE.with(|m| m.get())
}

pub fn set_mode(m: Mode) {
    MODE.with(|c| c.set(m));
}

/// Whether this draw is written as HTML by native code (the prerenderer,
/// the server), not drawn in a browser.
pub fn writing() -> bool {
    !cfg!(target_arch = "wasm32") && mode() != Mode::Spa
}

pub(crate) fn width() -> Option<f64> {
    WIDTH.with(|w| w.get())
}

pub fn set_width(w: Option<f64>) {
    WIDTH.with(|c| c.set(w));
}

/// The page's own arguments besides its route, as `query` reads them.
pub fn args() -> Vec<(String, String)> {
    ARGS.with(|a| a.borrow().clone())
}

pub fn set_args(a: Vec<(String, String)>) {
    ARGS.with(|c| *c.borrow_mut() = a);
}

/// The controls of the draw about to be made forgotten.
pub(crate) fn begin_draw() {
    CONTROLS.with(|c| c.borrow_mut().clear());
}

/// How many controls the last draw made.
pub fn controls() -> usize {
    CONTROLS.with(|c| c.borrow().len())
}

pub fn set_leads(l: Option<HashMap<usize, Lead>>) {
    LEADS.with(|c| *c.borrow_mut() = l);
}

/// The FNV-1a hash of a control's label: what its press is checked by.
pub fn check(label: &str) -> u32 {
    label.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// What a control is wired with in the HTML: the attributes a button (or
/// a field) carries so that it works with no script. All `None` in the
/// SPA and in the browser, so nothing it draws changes.
#[derive(Clone, Debug, Default)]
pub struct Wired {
    pub form: Option<&'static str>,
    pub name: Option<String>,
    pub value: Option<String>,
    pub formaction: Option<String>,
    pub formmethod: Option<&'static str>,
    pub disabled: Option<&'static str>,
    /// Disabled by the writer, not by the app: the browser that takes the
    /// page over takes the attribute away (`hydrate`).
    pub off: Option<&'static str>,
}

fn record(label: &str, run: Run) -> Option<usize> {
    if mode() == Mode::Spa {
        return None;
    }
    Some(CONTROLS.with(|c| {
        let mut c = c.borrow_mut();
        c.push(Control {
            label: label.to_string(),
            run,
        });
        c.len() - 1
    }))
}

/// A control pressed: `run` when it is, `label` what it says (its check).
pub fn tap(label: &str, run: Action) -> Wired {
    let Some(i) = record(label, Run::Tap(run)) else {
        return Wired::default();
    };
    if !writing() {
        return Wired::default();
    }
    match mode() {
        Mode::Server => Wired {
            form: Some("ark-act"),
            name: Some("ark".into()),
            value: Some(format!("{i}.{:x}", check(label))),
            ..Wired::default()
        },
        _ => match LEADS.with(|l| l.borrow().as_ref().and_then(|l| l.get(&i).cloned())) {
            Some(Lead::To(path)) => Wired {
                form: Some("ark-go"),
                formaction: Some(path),
                formmethod: Some("get"),
                ..Wired::default()
            },
            _ => Wired {
                disabled: Some(""),
                off: Some(""),
                ..Wired::default()
            },
        },
    }
}

/// A text field: `current` what it holds, `typed` what is sent with what
/// it holds once the form is sent.
pub fn text(label: &str, current: &str, typed: Rc<dyn Fn(String)>) -> Wired {
    let Some(i) = record(
        label,
        Run::Text {
            current: current.to_string(),
            typed,
        },
    ) else {
        return Wired::default();
    };
    if !writing() {
        return Wired::default();
    }
    match mode() {
        Mode::Server => Wired {
            form: Some("ark-act"),
            name: Some(format!("t{i}")),
            value: Some(current.to_string()),
            ..Wired::default()
        },
        _ => Wired {
            value: Some(current.to_string()),
            disabled: Some(""),
            off: Some(""),
            ..Wired::default()
        },
    }
}

/// The attributes of [`Wired`] on an element being built.
#[macro_export]
macro_rules! wired {
    ($el:expr, $w:expr) => {{
        let w: $crate::page::Wired = $w;
        $el.attr("form", w.form)
            .attr("name", w.name)
            .attr("value", w.value)
            .attr("formaction", w.formaction)
            .attr("formmethod", w.formmethod)
            .attr("disabled", w.disabled)
            .attr("data-ark-off", w.off)
    }};
}

/// Why a press was not taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not `<index>.<check>`, or too long.
    Malformed,
    /// No control of this draw is at the index with that check: the page
    /// was another's, or has changed since.
    Stale,
}

/// The longest a press's value or a field's text may be.
pub const MAX_TEXT: usize = 4096;

/// What a form sent, taken against the draw just made: each field's text
/// that changed sent first, then the press. `fields` are `(t<index>,
/// text)`; `press` is `<index>.<check>`, or empty for a form sent from a
/// field (its Enter).
pub fn take(press: &str, fields: &[(String, String)]) -> Result<(), Refused> {
    if press.len() > 32 || fields.iter().any(|(k, v)| k.len() > 16 || v.len() > MAX_TEXT) {
        return Err(Refused::Malformed);
    }
    let pressed = match press {
        "" => None,
        p => {
            let (i, h) = p.split_once('.').ok_or(Refused::Malformed)?;
            let i: usize = i.parse().map_err(|_| Refused::Malformed)?;
            let h = u32::from_str_radix(h, 16).map_err(|_| Refused::Malformed)?;
            Some((i, h))
        }
    };
    // Every action found before any runs: running one draws nothing, but
    // a stale press must change nothing at all.
    let mut runs: Vec<Box<dyn FnOnce()>> = vec![];
    CONTROLS.with(|c| {
        let c = c.borrow();
        for (k, v) in fields {
            let Some(i) = k.strip_prefix('t').and_then(|i| i.parse::<usize>().ok()) else {
                return Err(Refused::Malformed);
            };
            match c.get(i).map(|c| &c.run) {
                Some(Run::Text { current, typed }) => {
                    if current != v {
                        let (typed, v) = (typed.clone(), v.clone());
                        runs.push(Box::new(move || typed(v)));
                    }
                }
                _ => return Err(Refused::Stale),
            }
        }
        if let Some((i, h)) = pressed {
            match c.get(i) {
                Some(Control { label, run: Run::Tap(a) }) if check(label) == h => {
                    let a = a.clone();
                    runs.push(Box::new(move || a()));
                }
                _ => return Err(Refused::Stale),
            }
        }
        Ok(())
    })?;
    for r in runs {
        r();
    }
    Ok(())
}

/// Press control `i` of the draw just made, as a click would: the
/// prerenderer's, to find where it leads. False when there is none.
pub fn press(i: usize) -> bool {
    let run = CONTROLS.with(|c| match c.borrow().get(i).map(|c| &c.run) {
        Some(Run::Tap(a)) => Some(a.clone()),
        _ => None,
    });
    match run {
        Some(a) => {
            a();
            true
        }
        None => false,
    }
}

/// A page's address: the launch arguments as segments of the path.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Route(pub Vec<(String, String)>);

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A percent-encoded segment read back.
pub fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

impl Route {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn with(mut self, key: &str, value: impl Into<String>) -> Route {
        self.0.retain(|(k, _)| k != key);
        self.0.push((key.into(), value.into()));
        self
    }

    /// `/budget/`, `/category:dining/inspector=1/`: `screen` bare and first,
    /// the rest `key=value`.
    pub fn path(&self) -> String {
        let mut segs = vec![];
        if let Some(s) = self.get("screen") {
            segs.push(encode(s));
        }
        for (k, v) in &self.0 {
            if k != "screen" {
                segs.push(format!("{}={}", encode(k), encode(v)));
            }
        }
        match segs.is_empty() {
            true => "/".into(),
            false => format!("/{}/", segs.join("/")),
        }
    }

    /// A path read back, under `base` (where the site is served from);
    /// `None` for one that is not a route's.
    pub fn of_path(path: &str, base: &str) -> Option<Route> {
        let path = path.strip_prefix(base.trim_end_matches('/')).unwrap_or(path);
        let path = path.trim_end_matches("index.html");
        let mut r = vec![];
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            match seg.split_once('=') {
                Some((k, v)) => r.push((decode(k)?, decode(v)?)),
                None if r.is_empty() => r.push(("screen".into(), decode(seg)?)),
                None => return None,
            }
        }
        Some(Route(r))
    }

    /// An address's query read as a route (`?screen=budget&inspector=1`).
    pub fn of_query(q: &str) -> Route {
        Route(
            q.trim_start_matches('?')
                .split('&')
                .filter_map(|kv| {
                    let (k, v) = kv.split_once('=')?;
                    Some((decode(k)?, decode(v)?))
                })
                .collect(),
        )
    }

    /// The route as the launch arguments a shell reads: `-screen budget`.
    pub fn args(&self) -> Vec<String> {
        self.0.iter().flat_map(|(k, v)| [format!("-{k}"), v.clone()]).collect()
    }
}

/// What's in a page's text, escaped for HTML.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_round_trip() {
        for r in [
            Route::default(),
            Route(vec![("screen".into(), "budget".into())]),
            Route(vec![("screen".into(), "category:dining".into()), ("inspector".into(), "1".into())]),
            Route(vec![
                ("screen".into(), "transaction:a b/c".into()),
                ("pane".into(), "Connected Banks".into()),
            ]),
            Route(vec![("month".into(), "2026-10".into())]),
        ] {
            assert_eq!(Route::of_path(&r.path(), "").as_ref(), Some(&r), "{}", r.path());
            assert_eq!(Route::of_path(&format!("/money{}index.html", r.path()), "/money/").as_ref(), Some(&r));
        }
        assert_eq!(Route(vec![("screen".into(), "budget".into())]).path(), "/budget/");
        assert_eq!(Route::of_path("/budget/x/", ""), None);
    }

    #[test]
    fn a_press_is_checked_against_the_draw() {
        set_mode(Mode::Server);
        begin_draw();
        let hits = Rc::new(Cell::new(0));
        let typed = Rc::new(RefCell::new(String::new()));
        let h = hits.clone();
        tap("Approve", Rc::new(move || h.set(h.get() + 1)));
        let t = typed.clone();
        text("Payee", "", Rc::new(move |s| *t.borrow_mut() = s));
        let ok = format!("0.{:x}", check("Approve"));
        assert_eq!(take(&format!("0.{:x}", check("Delete")), &[]), Err(Refused::Stale));
        assert_eq!(take("1.0", &[]), Err(Refused::Stale));
        assert_eq!(take("zz", &[]), Err(Refused::Malformed));
        assert_eq!(take(&ok, &[("t0".into(), "x".into())]), Err(Refused::Stale));
        assert_eq!(hits.get(), 0, "a refused form runs nothing");
        assert_eq!(take(&ok, &[("t1".into(), "Target".into())]), Ok(()));
        assert_eq!((hits.get(), typed.borrow().as_str()), (1, "Target"));
        assert_eq!(take("", &[("t1".into(), "x".repeat(MAX_TEXT + 1))]), Err(Refused::Malformed));
        set_mode(Mode::Spa);
    }
}
