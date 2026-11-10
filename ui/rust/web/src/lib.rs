//! ArkUI in the browser: an `ark_app` [`View`] drawn by Leptos over the
//! real DOM (ui/docs/app.md, "The Layers"), in the theme's CSS
//! (`theme.css`, written by ui/theme.py from ui/theme.toml) and ArkUI's
//! own (`arkui.css`), as `arkui::view` draws the same view in iced.
//!
//! - [`render`] draws a page's view (`#[view]`'s `web(&record)`);
//!   [`shell`] the shell's (`ark_app::shell`): the sections a sidebar, a
//!   tab bar at the top or at the foot by the window's size class (CSS
//!   container queries over `[layout]`'s widths), each section's toolbar,
//!   search field and inspector, its pushed detail, its dialogs.
//! - [`run`] mounts an app ([`WebApp`]) on the page: its view drawn again
//!   whenever a message was taken, and the keyboard every Ark app has
//!   ([`keys`]): the shortcuts of its commands, the vim layer, the command
//!   box and the keys panel.
//! - The transient parts are this crate's: the context menu at the pointer
//!   and a control's menu with its submenus ([`menu`]), the date picker and
//!   a search field's suggestions ([`fields`]), sheets and dialogs.
//! - What a hosted screen is laid out of: [`figures`] (charts as inline
//!   SVG) and [`table`].
//!
//! A message is sent with [`send`]: the app's one sink takes it, updates,
//! and the page is drawn again. Everything here runs on the page's one
//! thread, so nothing is `Send`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

pub use leptos::prelude::AnyView;

pub mod fields;
pub mod figures;
pub mod keys;
pub mod menu;
pub mod parts;
pub mod shell;
pub mod table;

pub use parts::render;
pub use shell::shell;

/// The theme's variables, generated (ui/theme.py): `nix run .#update-theme`.
pub const THEME_CSS: &str = include_str!("../theme.css");
/// ArkUI's parts as CSS, over the theme's variables.
pub const ARKUI_CSS: &str = include_str!("../arkui.css");

/// Whether motion runs whatever the system asks: `motion::ALWAYS_ANIMATE`,
/// `ReducedMotion.ALWAYS_ANIMATE` and `Motion.alwaysAnimate`'s twin
/// (ui/docs/motion.md, "The override is on, on purpose"). While true the
/// system's setting is not read; `?reduce-motion=1` forces the gentle path
/// as `ARK_REDUCE_MOTION=1` does on the desktop, and `0` full motion.
pub const ALWAYS_ANIMATE: bool = true;

/// A pane that can have the keyboard, as `arkui::shell::Pane`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pane {
    Sidebar,
    #[default]
    Content,
    Inspector,
}

impl Pane {
    fn index(self) -> usize {
        match self {
            Pane::Sidebar => 0,
            Pane::Content => 1,
            Pane::Inspector => 2,
        }
    }
}

/// An action a part leaves behind for the keyboard to take: a row's open,
/// its menu.
pub type Action = Rc<dyn Fn()>;

/// What opens a row's context menu at a point of the window.
pub type Opener = Rc<dyn Fn(f64, f64)>;

/// The app's one sink for messages, whatever their type.
type Sink = Rc<dyn Fn(Box<dyn Any>)>;

/// What the page is in the middle of that is no app's: which pane has the
/// keys and where its cursor is, the menu or popover open, the command box
/// and the keys panel. The app's own state is the app's.
#[derive(Default)]
pub struct Ui {
    pub focus: Pane,
    pub cursor: [usize; 3],
    /// What of a key sequence is typed so far: `g`, `z`, a count.
    pub pending: String,
    pub menu: Option<menu::Open>,
    /// The command box: what is typed, and which match is lit.
    pub command: Option<(String, usize)>,
    pub keys_panel: bool,
    /// The popover open, by its part's id (a date field's calendar, a
    /// search field's suggestions) and where it is (a day, a lit row).
    pub popover: Option<(String, i64)>,
    /// The cursor moved by the keys: kept in view after the next draw.
    reveal: bool,
    /// The keys have walked the rows: the cursor is shown from then on, as
    /// a Mac list shows its focus only once it has the keyboard.
    pub keyed: bool,
}

/// What one draw left for the keyboard: each pane's rows' actions, every
/// row's menu by its title, the toolbar's menus by id, the search field.
#[derive(Default)]
pub(crate) struct Drawn {
    pub rows: [Vec<Row>; 3],
    pub contexts: Vec<(String, Opener)>,
    pub menus: Vec<(String, Rc<dyn Fn()>)>,
    pub pane: Pane,
    /// The dismissal of the modal on top, while one is up: `Esc`'s.
    pub modal: Option<Action>,
}

#[derive(Clone)]
pub(crate) struct Row {
    pub title: String,
    pub open: Option<Action>,
    pub menu: Option<Opener>,
}

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
    static DRAWN: RefCell<Drawn> = RefCell::new(Drawn::default());
    static VERSION: Cell<Option<RwSignal<u64>>> = const { Cell::new(None) };
    static QUEUE: RefCell<Vec<Box<dyn Any>>> = const { RefCell::new(Vec::new()) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

/// `m` sent to the app: taken, and the page drawn again. A message sent
/// while one is being taken waits its turn.
pub fn send<M: 'static>(m: M) {
    QUEUE.with(|q| q.borrow_mut().push(Box::new(m)));
    if BUSY.with(|b| b.replace(true)) {
        return;
    }
    while let Some(m) = QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        (!q.is_empty()).then(|| q.remove(0))
    }) {
        let sink = SINK.with(|s| s.borrow().clone());
        if let Some(sink) = sink {
            sink(m);
        }
    }
    BUSY.with(|b| b.set(false));
    redraw();
}

/// The page drawn again.
pub fn redraw() {
    if let Some(v) = VERSION.with(|v| v.get()) {
        v.update(|n| *n += 1);
    }
}

/// `f` of the page's transient state.
pub fn ui<R>(f: impl FnOnce(&Ui) -> R) -> R {
    UI.with(|u| f(&u.borrow()))
}

/// The page's transient state changed by `f`, and drawn again.
pub fn ui_mut<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    let r = UI.with(|u| f(&mut u.borrow_mut()));
    redraw();
    r
}

pub(crate) fn drawn<R>(f: impl FnOnce(&mut Drawn) -> R) -> R {
    DRAWN.with(|d| f(&mut d.borrow_mut()))
}

/// The pane what is drawn next belongs to, while `f` draws it.
pub(crate) fn in_pane<R>(pane: Pane, f: impl FnOnce() -> R) -> R {
    let was = drawn(|d| std::mem::replace(&mut d.pane, pane));
    let r = f();
    drawn(|d| d.pane = was);
    r
}

/// A row drawn in the pane being drawn: its index there, and whether the
/// keyboard's cursor is on it.
pub(crate) fn row_drawn(row: Row) -> (usize, bool) {
    let (pane, i) = drawn(|d| {
        let p = d.pane;
        d.rows[p.index()].push(row);
        (p, d.rows[p.index()].len() - 1)
    });
    let lit = ui(|u| u.keyed && u.focus == pane && u.cursor[pane.index()] == i);
    (i, lit)
}

/// An app the page runs.
pub trait WebApp: 'static {
    type Msg: Clone + 'static;
    /// The window's view: its shell, or a page.
    fn view(&self) -> AnyView;
    /// A message taken.
    fn update(&mut self, msg: Self::Msg);
    /// The menu bar's commands: their shortcuts are the keys', their
    /// titles the command box's.
    fn commands(&self) -> Vec<ark_app::shell::CommandMenu<Self::Msg>> {
        vec![]
    }
    /// What the page's address asks for once the first draw is up
    /// (`?screen=`, `?run=` and the rest an app reads): the messages.
    fn arrive(&mut self, _query: &dyn Fn(&str) -> Option<String>) -> Vec<Self::Msg> {
        vec![]
    }
    /// What the field `field` (`search`, or a form's own id) suggests for
    /// what is typed in it.
    fn suggest(&self, _field: &str, _typed: &str) -> Vec<String> {
        vec![]
    }
}

/// The page's address's `name` parameter.
pub fn query(name: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name)
}

/// Whether motion takes its gentle path (ui/docs/motion.md): the address
/// says (`?reduce-motion=1` or `0`), else the system, only when
/// [`ALWAYS_ANIMATE`] is off.
pub fn reduced() -> bool {
    match query("reduce-motion").as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ if ALWAYS_ANIMATE => false,
        _ => web_sys::window()
            .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten())
            .is_some_and(|m| m.matches()),
    }
}

fn root_element() -> Option<web_sys::Element> {
    web_sys::window()?.document()?.document_element()
}

/// `app` on the page, in `accent` (an `[accent.<name>]` of ui/theme.toml)
/// and its own `css` after ArkUI's: its styles put in, its view drawn and
/// drawn again after every message, the keyboard listened to, and what
/// the address asks for done.
pub fn run<A: WebApp>(app: A, accent: &str, css: &str) {
    let doc = web_sys::window().and_then(|w| w.document()).expect("a document");
    if let Some(root) = root_element() {
        let _ = root.set_attribute("data-accent", accent);
        if let Some(t) = query("theme").filter(|t| t == "light" || t == "dark") {
            let _ = root.set_attribute("data-theme", &t);
        }
        let _ = root.set_attribute("data-motion", if reduced() { "reduced" } else { "full" });
    }
    if let (Some(head), Ok(style)) = (doc.head(), doc.create_element("style")) {
        style.set_text_content(Some(&format!("{THEME_CSS}\n{ARKUI_CSS}\n{css}")));
        let _ = head.append_child(&style);
    }
    let app = Rc::new(RefCell::new(app));
    SINK.with(|s| {
        let app = app.clone();
        *s.borrow_mut() = Some(Rc::new(move |m: Box<dyn Any>| {
            if let Ok(m) = m.downcast::<A::Msg>() {
                app.borrow_mut().update(*m);
            }
        }));
    });
    keys::listen::<A>(app.clone());
    {
        let app = app.clone();
        fields::suggest_with(move |f, q| app.borrow().suggest(f, q));
    }
    let version = RwSignal::new(0u64);
    VERSION.with(|v| v.set(Some(version)));
    APP_VIEW.with(|v| {
        let app = app.clone();
        *v.borrow_mut() = Some(Rc::new(move || app.borrow().view()));
    });
    leptos::mount::mount_to_body(move || {
        move || {
            version.get();
            draw_window()
        }
    });
    // What the address asks for, after the first draw.
    let arrive: Vec<A::Msg> = app.borrow_mut().arrive(&|n| query(n));
    for m in arrive {
        send(m);
    }
    if let Some(title) = query("context") {
        after_draw(move || keys::open_context(&title));
    }
    if let Some(id) = query("menu") {
        after_draw(move || keys::open_menu(&id));
    }
    if query("command").is_some() {
        ui_mut(|u| u.command = Some((query("command").unwrap_or_default(), 0)));
    }
    if query("keys").is_some() {
        ui_mut(|u| u.keys_panel = true);
    }
}

thread_local! {
    static APP_VIEW: RefCell<Option<Rc<dyn Fn() -> AnyView>>> = const { RefCell::new(None) };
    static AFTER: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// `f` run once the page has been drawn again.
pub fn after_draw(f: impl FnOnce() + 'static) {
    AFTER.with(|a| a.borrow_mut().push(Box::new(f)));
    redraw();
}

fn draw_window() -> AnyView {
    drawn(|d| *d = Drawn::default());
    let view = APP_VIEW.with(|v| v.borrow().clone()).map(|f| f());
    let over = (menu::overlay(), keys::overlay());
    // Kept to the rows drawn: a cursor past a list that shrank is on its last.
    UI.with(|u| {
        let mut u = u.borrow_mut();
        for p in [Pane::Sidebar, Pane::Content, Pane::Inspector] {
            let n = drawn(|d| d.rows[p.index()].len());
            let c = &mut u.cursor[p.index()];
            *c = (*c).min(n.saturating_sub(1));
        }
    });
    let reveal = UI.with(|u| std::mem::take(&mut u.borrow_mut().reveal));
    let after: Vec<Box<dyn FnOnce()>> = AFTER.with(|a| std::mem::take(&mut *a.borrow_mut()));
    request_frame(move || {
        if reveal {
            reveal_cursor();
        }
        autofocus();
        keep_on_screen();
        for f in after {
            f();
        }
    });
    html::div()
        .class("ark-window")
        .child(html::div().class("ark-frame").child((view, over)))
        .into_any()
}

/// `f` on the next animation frame: after the DOM the draw made is there.
pub fn request_frame(f: impl FnOnce() + 'static) {
    let cb = wasm_bindgen::closure::Closure::once_into_js(f);
    if let Some(w) = web_sys::window() {
        let _ = w.request_animation_frame(cb.unchecked_ref());
    }
}

/// What asks for the keys when it appears (a calendar, the command box)
/// given them, unless they are already inside it.
fn autofocus() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(Some(el)) = doc.query_selector("[data-autofocus]") else { return };
    let inside = doc.active_element().is_some_and(|a| el.contains(Some(&a)));
    if !inside {
        if let Ok(el) = el.dyn_into::<web_sys::HtmlElement>() {
            let _ = el.focus();
        }
    }
}

/// A popover that runs off the window moved back on: past the right
/// edge, it lines up with its control's right edge instead; past the foot,
/// it opens above its control, as AppKit's do.
fn keep_on_screen() {
    let Some(w) = web_sys::window() else { return };
    let Some(doc) = w.document() else { return };
    let (ww, wh) = (
        w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0),
        w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(0.0),
    );
    let Ok(list) = doc.query_selector_all(".ark-popover-layer") else { return };
    for i in 0..list.length() {
        let Some(layer) = list.item(i).and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) else { continue };
        let Some(panel) = layer.query_selector(".ark-popover").ok().flatten() else { continue };
        let r = panel.get_bounding_client_rect();
        let style = layer.style();
        if r.right() > ww - 8.0 {
            let _ = style.set_property("left", "auto");
            let _ = style.set_property("right", "0");
        }
        if r.bottom() > wh - 8.0 {
            let _ = style.set_property("top", "auto");
            let _ = style.set_property("bottom", "calc(100% + 4px)");
        }
    }
}

fn reveal_cursor() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Ok(Some(el)) = doc.query_selector("[data-cursor]") {
        // A snap, not a glide: the keyboard's motion is none (motion.md).
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
    }
}

/// A glyph of ArkUI's shared names (`arkui_glyphs::named`), drawn in the
/// text's colour at `size` pixels; a note where the name has no drawing.
pub fn glyph(name: &str, size: u32) -> AnyView {
    let svg = arkui_glyphs::named(name).unwrap_or(arkui_glyphs::NOTE);
    let svg = String::from_utf8_lossy(svg).replace("<svg ", &format!("<svg width=\"{size}\" height=\"{size}\" aria-hidden=\"true\" "));
    html::span().class("ark-glyph").inner_html(svg).into_any()
}

/// An amount in dollars, as every client writes one (`ark_format`).
pub fn money(cents: i64) -> String {
    ark_format::amount(cents, &ark_format::NumberStyle::us())
}

/// An amount's text: red below nothing, quiet when asked.
pub fn amount(cents: i64, quiet: bool) -> AnyView {
    let class = match (cents < 0, quiet) {
        (true, _) => "ark-amount ark-negative",
        (false, true) => "ark-amount ark-quiet",
        (false, false) => "ark-amount",
    };
    html::span().class(class).child(money(cents)).into_any()
}

/// A register's amount, as a Mac's table writes one: what came in signed
/// and in the positive colour, what went out in the ink.
pub fn signed(cents: i64) -> AnyView {
    match cents > 0 {
        true => html::span()
            .class("ark-amount ark-text-positive")
            .child(format!("+{}", money(cents)))
            .into_any(),
        false => html::span().class("ark-amount").child(money(cents)).into_any(),
    }
}

/// A word quiet and centred where there is nothing to show yet.
pub fn quiet(words: impl Into<String>) -> AnyView {
    html::div().class("ark-empty").child(words.into()).into_any()
}
