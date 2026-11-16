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
//! - The page is an Ark app's ([`host::WebHost`]): its part of its host
//!   (`ark_app::window::Windowed`) run over the page, signed in to its
//!   server and synced ([`live`]), on its demo, or with no data of its own
//!   (a static site). [`host::start`] is what its generated entry point
//!   calls: [`run`] draws it in the browser alone, [`hydrate`] takes over a
//!   page written ahead ([`export`]) or by a server; either draws it again
//!   whenever a message was taken, keeps the browser's address and title
//!   the app's, and listens to the keyboard every Ark app has ([`keys`]):
//!   the shortcuts of its commands, the vim layer, the command box and the
//!   keys panel.
//! - The transient parts are this crate's: the context menu at the pointer
//!   and a control's menu with its submenus ([`menu`]), the date picker and
//!   a search field's suggestions ([`fields`]), sheets and dialogs.
//! - What a hosted screen is laid out of: [`figures`] (charts as inline
//!   SVG) and [`table`].
//! - What ArkUI documents of its own parts: their live [`specimens`]
//!   (`ark_app::shell::specimen`).
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

pub use ark_router::Address;
/// The app's own mark, from its icon (`arkui_glyphs::set_app`): set once at start.
pub use arkui_glyphs::set_app as set_app_mark;
pub use leptos::prelude::AnyView;

#[cfg(feature = "ssr")]
pub mod export;
pub mod fields;
pub mod figures;
pub mod host;
pub mod keys;
mod layout;
pub mod live;
pub mod menu;
pub mod page;
pub mod parts;
pub mod shell;
pub mod specimens;
pub mod table;
mod timeline;
pub mod writing;

pub use host::{HostMsg, WebHost, WebHosted};
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
    /// A search picker open, by its part's id: what is typed and lit.
    pub combo: Option<(String, ark_app::view::search::Searching)>,
    /// Where the pointer has been in each menu written open
    /// ([`menu::written`]), by its id: the entry lit at each level.
    pub written: Vec<(String, Vec<usize>)>,
    /// The cursor moved by the keys: kept in view after the next draw.
    reveal: bool,
    /// The keys have walked the rows: the cursor is shown from then on, as
    /// a Mac list shows its focus only once it has the keyboard.
    pub keyed: bool,
    /// Visual mode's other end, in the pane with the keys (`v`): the rows
    /// from it to the cursor are lit, and a bound key runs on each.
    pub anchor: Option<usize>,
    /// The rows a bound key ran on, found again by their titles when the
    /// page is next drawn: the cursor stays with them.
    pub(crate) follow: Option<Follow>,
    /// What the specimens of arkui-web's own parts are at ([`specimens`]).
    pub specimens: specimens::State,
}

/// What one draw left for the keyboard: each pane's rows' actions, every
/// row's menu by its title, the toolbar's menus by id, the search field.
#[derive(Default)]
pub(crate) struct Drawn {
    pub rows: [Vec<Row>; 3],
    pub contexts: Vec<(String, Opener)>,
    pub menus: Vec<(String, Rc<dyn Fn()>)>,
    pub pane: Pane,
    /// The sidebar's row of the section shown: where its cursor is, as the
    /// desktop's sidebar cursor is the section (`arkui::shell::Routed::Section`).
    pub sidebar_at: Option<usize>,
    /// The dismissal of the modal on top, while one is up: `Esc`'s.
    pub modal: Option<Action>,
    /// The section being drawn (its number, from one) and its heading's
    /// bound items, which each of its rows carries (`parts::section`).
    pub section: usize,
    pub sections: usize,
    pub heading: Vec<Bound>,
}

/// A row drawn: what it says, what opening it sends, its menu, and, set
/// as it is drawn, the section it is in and the items keys run on it
/// (`ark_app::view::MenuItem::keys`), as the desktop's
/// (`arkui::view::cursor::Row`).
#[derive(Clone, Default)]
pub(crate) struct Row {
    pub title: String,
    pub open: Option<Action>,
    pub menu: Option<Opener>,
    pub section: usize,
    pub bound: Vec<Bound>,
}

/// A menu item bound to keys, on a row drawn: its keys, its words, what it
/// does, and the section whose heading binds it (run once over a selection).
#[derive(Clone)]
pub(crate) struct Bound {
    pub bind: ark_app::view::Bind,
    pub title: String,
    pub run: Action,
    pub heading: Option<usize>,
}

/// Where the cursor and its selection's anchor were when a bound key ran
/// (`keys::run_bound`): the pane, and each one's row and title.
#[derive(Clone, Debug)]
pub(crate) struct Follow {
    pub pane: usize,
    pub at: (usize, String),
    pub anchor: Option<(usize, String)>,
}

impl Follow {
    /// The row titled as `(near, title)` was, nearest it, among `rows`.
    fn nearest(rows: &[Row], (near, title): &(usize, String)) -> Option<usize> {
        rows.iter()
            .enumerate()
            .filter(|(_, r)| r.title == *title)
            .min_by_key(|(i, _)| i.abs_diff(*near))
            .map(|(i, _)| i)
    }

    /// The cursor put on its rows as they are drawn now.
    fn find(&self, u: &mut Ui) {
        let found = drawn(|d| {
            let rows = &d.rows[self.pane];
            (
                Follow::nearest(rows, &self.at),
                self.anchor.as_ref().and_then(|a| Follow::nearest(rows, a)),
            )
        });
        if let Some(i) = found.0 {
            u.cursor[self.pane] = i;
        }
        if u.anchor.is_some() {
            u.anchor = found.1.or(u.anchor);
        }
    }
}

/// The items of `menu` that keys run, offered now.
pub(crate) fn bound<M: Clone + std::fmt::Debug + 'static>(menu: &[ark_app::view::MenuItem<M>]) -> Vec<Bound> {
    menu.iter()
        .filter(|m| m.offered())
        .filter_map(|m| {
            let tap = m.tap.clone();
            Some(Bound {
                bind: m.keys?,
                title: m.title.get(),
                run: Rc::new(move || send(tap.clone())),
                heading: None,
            })
        })
        .collect()
}

/// What `f` draws are one section's rows, whose heading binds `heading`.
pub(crate) fn in_section<R>(heading: Vec<Bound>, f: impl FnOnce() -> R) -> R {
    let was = drawn(|d| {
        d.sections += 1;
        let n = d.sections;
        let heading = heading.into_iter().map(|b| Bound { heading: Some(n), ..b }).collect();
        (std::mem::replace(&mut d.section, n), std::mem::replace(&mut d.heading, heading))
    });
    let r = f();
    drawn(|d| {
        d.section = was.0;
        d.heading = was.1;
    });
    r
}

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
    static DRAWN: RefCell<Drawn> = RefCell::new(Drawn::default());
    static VERSION: Cell<Option<RwSignal<u64>>> = const { Cell::new(None) };
    static QUEUE: RefCell<Vec<Box<dyn Any>>> = const { RefCell::new(Vec::new()) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
    /// Drawing a specimen ([`specimen`]): what goes over the page is
    /// drawn in its place.
    static SPECIMEN: Cell<bool> = const { Cell::new(false) };
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
        // A specimen's is the page's own; anything else is the app's.
        let m = match m.downcast::<specimens::Said>() {
            Ok(said) => {
                specimens::take(*said);
                continue;
            }
            Err(m) => m,
        };
        let sink = SINK.with(|s| s.borrow().clone());
        if let Some(sink) = sink {
            sink(m);
        }
    }
    BUSY.with(|b| b.set(false));
    follow_address();
    redraw();
}

/// A control that sends `m`, wired for the page without script
/// ([`page::tap`]): `label` is what it says.
pub fn tapped<M: Clone + std::fmt::Debug + 'static>(label: &str, m: &M) -> page::Wired {
    let m = m.clone();
    page::tap(label, Rc::new(move || send(m.clone())))
}

/// Where a part that is no button is pressed (a table's row, a toggle's
/// box) without script: a button over all of it, wired as [`page::tap`],
/// drawn in every mode but the SPA's (so the page the browser takes over
/// is the one the server wrote). With script it runs `run` itself.
pub fn hit(label: &str, run: Action) -> Option<AnyView> {
    if page::mode() == page::Mode::Spa {
        return None;
    }
    let w = page::tap(label, run.clone());
    Some(
        wired!(html::button(), w)
            .class("ark-hit")
            .attr("aria-label", label.to_string())
            .on(leptos::ev::click, move |e| {
                e.prevent_default();
                e.stop_propagation();
                run()
            })
            .into_any(),
    )
}

/// `view` drawn as a specimen, for a page that shows what a part is (the
/// docs): what goes over the page (a sheet, a question, a control's menu)
/// is drawn in the specimen's frame rather than over the window, so it can
/// be written open and read without script, and it neither takes the keys
/// nor dims the page. With script it answers as it does over a window:
/// its buttons close it, and what opens it opens it here.
pub fn specimen<M: Clone + std::fmt::Debug + 'static>(view: ark_app::view::View<M>) -> AnyView {
    specimen_of(|| render(view))
}

/// What `draw` draws, as a [`specimen`]: for arkui-web's own parts
/// called directly (`fields::picker`, `menu::button`).
pub fn specimen_of(draw: impl FnOnce() -> AnyView) -> AnyView {
    let was = SPECIMEN.with(|s| s.replace(true));
    let drawn = draw();
    SPECIMEN.with(|s| s.set(was));
    html::div().class("ark-specimen").child(drawn).into_any()
}

/// Whether a specimen is being drawn ([`specimen`]).
pub fn in_specimen() -> bool {
    SPECIMEN.with(|s| s.get())
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
pub(crate) fn row_drawn(mut row: Row) -> (usize, bool) {
    let (pane, i) = drawn(|d| {
        let p = d.pane;
        row.section = d.section;
        row.bound.extend(d.heading.iter().cloned());
        d.rows[p.index()].push(row);
        (p, d.rows[p.index()].len() - 1)
    });
    // The cursor's row, or every row of a visual selection (`v`).
    let lit = ui(|u| {
        let at = u.cursor[pane.index()];
        let from = u.anchor.unwrap_or(at);
        u.keyed && u.focus == pane && from.min(at) <= i && i <= from.max(at)
    });
    (i, lit)
}

thread_local! {
    /// The screen being drawn, among the app's (app.toml's `[screens]`
    /// keys): what its elements' walk ids are under.
    static SCREEN: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// `f` drawn as the screen `name` (a hosted name's last part: `register`
/// of `accounts.register`): the hosted page's.
pub(crate) fn under<R>(name: &str, f: impl FnOnce() -> R) -> R {
    let name = name.rsplit('.').next().unwrap_or(name).to_string();
    let was = SCREEN.with(|s| std::mem::replace(&mut *s.borrow_mut(), name));
    let r = f();
    SCREEN.with(|s| *s.borrow_mut() = was);
    r
}

/// The walk id of the element `p` is on the screen being drawn
/// (`ark_app::shots::recorded`, the id a step names it by): written as its
/// `data-ark-id`, never said.
pub(crate) fn walk_id<M: std::fmt::Debug>(p: &ark_app::view::Part<M>) -> Option<String> {
    let screen = SCREEN.with(|s| s.borrow().clone());
    if screen.is_empty() {
        return None;
    }
    let pieces = ark_app::shots::recorded(&screen, p)?;
    Some(
        pieces
            .into_iter()
            .map(|x| match x {
                ark_app::shots::Piece::Text(t) => t,
                ark_app::shots::Piece::Slug(p) => ark_app::shots::slug(&p),
            })
            .collect(),
    )
}

/// Where the page's app is and its title, asked.
type Where = Rc<dyn Fn() -> (Address, String)>;

thread_local! {
    /// Where the page's app is, as the address that brings it back there
    /// (`WebHost::route`), and its title: what the browser's address and
    /// title follow.
    static ROUTE: RefCell<Option<Where>> = const { RefCell::new(None) };
    /// The page is arriving where its address says (a load, back or
    /// forward): what the app moves to meanwhile replaces the entry.
    static ARRIVING: Cell<bool> = const { Cell::new(false) };
    /// A press or a key since the address last followed the app: a move it
    /// makes is the person's, a new entry in the history.
    static GESTURE: Cell<bool> = const { Cell::new(false) };
}

/// The query keys that are the page's own, not the app's (ui/docs/app.md,
/// "Addresses"): kept in the address whatever the app's place, so a load
/// of it comes back the same (`?today=…&shots=1`, a screenshot's).
const STICKY: [&str; 4] = ["today", "shots", "theme", "reduce-motion"];

/// The browser's address made the app's (`WebHost::route`), under the
/// site's base, with the page's own keys ([`STICKY`]) kept: a new entry in
/// the history for a move the person made to a new place (a press, a key),
/// and the same entry replaced for a refinement (what is typed in a field,
/// a move the app made on its own, the page arriving where its address
/// says). Back and forward are `popstate`, which [`run`] and [`hydrate`]
/// answer by bringing the app where the address says.
fn follow_address() {
    if !cfg!(target_arch = "wasm32") {
        return;
    }
    let Some(route) = ROUTE.with(|r| r.borrow().clone()) else { return };
    let (mut to, title) = route();
    if let Some(d) = web_sys::window().and_then(|w| w.document()) {
        if d.title() != title {
            d.set_title(&title);
        }
    }
    let here = address();
    for key in STICKY {
        if let Some((k, v)) = here.query.iter().find(|(k, _)| k == key) {
            if to.get(k).is_none() {
                to.query.push((k.clone(), v.clone()));
            }
        }
    }
    let gesture = GESTURE.with(|g| g.replace(false));
    if to == here {
        return;
    }
    let typing = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .is_some_and(|el| matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA"));
    let push = gesture && !ARRIVING.with(|a| a.get()) && (to.path != here.path || !typing);
    let url = to.url_under(&page_base());
    let Some(history) = web_sys::window().and_then(|w| w.history().ok()) else {
        return;
    };
    let _ = match push {
        true => history.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&url)),
        false => history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&url)),
    };
}

/// A press or a key on the page: what the app moves to next is the
/// person's ([`follow_address`]).
fn listen_for_gestures() {
    use wasm_bindgen::closure::Closure;
    let Some(w) = web_sys::window() else { return };
    let on = Closure::<dyn FnMut(web_sys::Event)>::new(|_: web_sys::Event| GESTURE.with(|g| g.set(true)));
    for kind in ["pointerdown", "keydown", "click"] {
        let _ = w.add_event_listener_with_callback_and_bool(kind, on.as_ref().unchecked_ref(), true);
    }
    on.forget();
}

/// `f` with the page arriving where its address says: what the app moves
/// to meanwhile replaces the history's entry.
fn arriving<R>(f: impl FnOnce() -> R) -> R {
    let was = ARRIVING.with(|a| a.replace(true));
    let r = f();
    ARRIVING.with(|a| a.set(was));
    r
}

/// The page's address's query `name`, or else what the page was written
/// with (`page::args`): a page written as HTML reads the same arguments in
/// the browser as it was written with.
pub fn query(name: &str) -> Option<String> {
    let written = || page::args().into_iter().find(|(k, _)| k == name).map(|(_, v)| v);
    if !cfg!(target_arch = "wasm32") {
        return written();
    }
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name).or_else(written)
}

/// The page's address as a route, under the site's base.
pub fn address() -> Address {
    if !cfg!(target_arch = "wasm32") {
        return Address::default();
    }
    let Some(l) = web_sys::window().map(|w| w.location()) else {
        return Address::default();
    };
    let url = format!("{}{}", l.pathname().unwrap_or_default(), l.search().unwrap_or_default());
    Address::of_url(&url, &page_base())
}

/// Where the site is served from (`<html data-ark-base>`), `/` unless said.
pub fn page_base() -> String {
    root_element()
        .and_then(|r| r.get_attribute("data-ark-base"))
        .unwrap_or_else(|| "/".into())
}

/// Whether this page is its app's demo (`<html data-ark-demo>`): a page
/// published as the demo (the site's, nix/channel.py's `web … demo`) is
/// the demo by where it is, its household in the page with no server; the
/// same page served by the app's own server signs in there.
pub fn page_demo() -> bool {
    root_element().is_some_and(|r| r.has_attribute("data-ark-demo"))
}

/// Whether this page is a screenshot's (`?shots=1`, ui/docs/shots.md):
/// then nothing on it moves (`data-motion="shots"`: every animation and
/// transition none, the caret clear). When it is ready the app says itself
/// (`data-ark-idle`, `data-ark-version`: its runtimes' `is_idle` and
/// `version`), and the driver reads the page's own loading beside it.
pub fn shots() -> bool {
    query("shots").as_deref() == Some("1")
}

/// Whether motion takes its gentle path (ui/docs/motion.md): the address
/// says (`?reduce-motion=1` or `0`), else the system, only when
/// [`ALWAYS_ANIMATE`] is off.
pub fn reduced() -> bool {
    if !cfg!(target_arch = "wasm32") {
        return query("reduce-motion").as_deref() == Some("1");
    }
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
    if !cfg!(target_arch = "wasm32") {
        return None;
    }
    web_sys::window()?.document()?.document_element()
}

/// `app` on the page, drawn in the browser alone (the SPA), in its accent
/// and its own styles after ArkUI's: its styles put in, its view drawn and
/// drawn again after every message, the keyboard listened to, and what
/// the address asks for done.
pub fn run<W: WebHosted>(app: WebHost<W>) {
    let doc = web_sys::window().and_then(|w| w.document()).expect("a document");
    if let Some(root) = root_element() {
        let _ = root.set_attribute("data-accent", W::ACCENT);
        if let Some(t) = query("theme").filter(|t| t == "light" || t == "dark") {
            let _ = root.set_attribute("data-theme", &t);
        }
        let motion = if shots() {
            "shots"
        } else if reduced() {
            "reduced"
        } else {
            "full"
        };
        let _ = root.set_attribute("data-motion", motion);
    }
    if let (Some(head), Ok(style)) = (doc.head(), doc.create_element("style")) {
        style.set_text_content(Some(&format!("{THEME_CSS}\n{ARKUI_CSS}\n{}\n{}", writing::WRITING_CSS, W::CSS)));
        let _ = head.append_child(&style);
    }
    let app = install(app);
    listen::<W>(&app);
    let version = RwSignal::new(0u64);
    VERSION.with(|v| v.set(Some(version)));
    leptos::mount::mount_to_body(move || {
        move || {
            version.get();
            draw_window()
        }
    });
    // What the address asks for, after the first draw.
    arriving(|| arrive_here(&app));
}

/// What every page listens to once it is the module's: the keyboard, a
/// press (whose move is a new history entry), a link to another page of
/// the app (taken in place), and the address moving in place — back and
/// forward, or a screenshot's driver walking its shots in one page
/// (ui/docs/shots.md) — which puts the app back where it opens, the page's
/// own menus, popovers, command box and focus with it, then brings it
/// where the address says, as a fresh page would be.
fn listen<W: WebHosted>(app: &Rc<RefCell<WebHost<W>>>) {
    keys::listen::<W>(app.clone());
    listen_for_gestures();
    if !cfg!(target_arch = "wasm32") {
        return;
    }
    use wasm_bindgen::closure::Closure;
    let Some(w) = web_sys::window() else { return };
    let moved = {
        let app = app.clone();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            app.borrow_mut().rewind();
            if let Some(el) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.active_element())
                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = el.blur();
            }
            UI.with(|u| *u.borrow_mut() = Ui::default());
            QUEUE.with(|q| q.borrow_mut().clear());
            if let Some(w) = web_sys::window() {
                w.scroll_to_with_x_and_y(0.0, 0.0);
            }
            redraw();
            arriving(|| arrive_here(&app));
        })
    };
    let _ = w.add_event_listener_with_callback("popstate", moved.as_ref().unchecked_ref());
    moved.forget();
    // A link to another of the app's pages (under the site's base, no file,
    // no fragment, not for another tab) is a move of the app's, in place: a
    // new history entry, the app brought where it leads.
    let app = app.clone();
    let click = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
        if e.default_prevented() || e.button() != 0 || e.meta_key() || e.ctrl_key() || e.shift_key() || e.alt_key() {
            return;
        }
        let Some(a) = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("a[href]").ok().flatten())
        else {
            return;
        };
        let href = a.get_attribute("href").unwrap_or_default();
        let base = page_base();
        let path = href.split(['?', '#']).next().unwrap_or_default();
        let file = path.rsplit('/').next().is_some_and(|last| last.contains('.'));
        if a.has_attribute("target") || a.has_attribute("download") || !href.starts_with(&base) || href.contains('#') || file {
            return;
        }
        e.prevent_default();
        if let Some(h) = web_sys::window().and_then(|w| w.history().ok()) {
            let _ = h.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&href));
        }
        if let Some(w) = web_sys::window() {
            w.scroll_to_with_x_and_y(0.0, 0.0);
        }
        arriving(|| arrive_here(&app));
    });
    if let Some(doc) = w.document() {
        let _ = doc.add_event_listener_with_callback("click", click.as_ref().unchecked_ref());
    }
    click.forget();
}

/// Where the page's address asks the app to be (its `arrive`), and what of
/// the page's own it asks open: a row's menu (`context`), a control's
/// (`menu`), a field's popover (`popover`), the command box (`command`),
/// the keys panel (`keys`).
fn arrive_here<W: WebHosted>(app: &Rc<RefCell<WebHost<W>>>) {
    let arrive = app.borrow_mut().arrive(&address());
    for m in arrive {
        send(m);
    }
    if let Some(title) = query("context") {
        after_draw(move || keys::open_context(&title));
    }
    if let Some(id) = query("menu") {
        after_draw(move || keys::open_menu(&id));
    }
    if let Some(id) = query("popover") {
        after_draw(move || fields::open_popover(&id, 0));
    }
    if query("command").is_some() {
        ui_mut(|u| u.command = Some((query("command").unwrap_or_default(), 0)));
    }
    if query("keys").is_some() {
        ui_mut(|u| u.keys_panel = true);
    }
}

/// `app` as the page's one app: the sink its messages go to, the view
/// drawn, what its fields suggest, where it is.
fn install<W: WebHosted>(app: WebHost<W>) -> Rc<RefCell<WebHost<W>>> {
    let app = Rc::new(RefCell::new(app));
    SINK.with(|s| {
        let app = app.clone();
        *s.borrow_mut() = Some(Rc::new(move |m: Box<dyn Any>| {
            if let Ok(m) = m.downcast::<HostMsg<host::Msg<W>>>() {
                app.borrow_mut().update(*m);
            }
        }));
    });
    {
        let app = app.clone();
        fields::suggest_with(move |f, q| app.borrow().suggest(f, q));
    }
    {
        let app = app.clone();
        keys::set_commands(Rc::new(move || app.borrow().commands()));
    }
    APP_VIEW.with(|v| {
        let app = app.clone();
        *v.borrow_mut() = Some(Rc::new(move || app.borrow().view()));
    });
    ROUTE.with(|r| {
        let app = app.clone();
        *r.borrow_mut() = Some(Rc::new(move || {
            let a = app.borrow();
            (a.route(), a.title())
        }));
    });
    UI.with(|u| *u.borrow_mut() = Ui::default());
    QUEUE.with(|q| q.borrow_mut().clear());
    BUSY.with(|b| b.set(false));
    app
}

/// `app` taking over a page written as HTML (`page::Mode::Static` or
/// `Server`): it arrives where the page's route says, as the writer's did,
/// and its first draw is the written one at the width it was written at;
/// then it is the SPA's, at the window's own width. The forms the page
/// was written with are the browser's no more: a press is the click's.
#[cfg(feature = "hydrate")]
pub fn hydrate<W: WebHosted>(app: WebHost<W>) {
    use wasm_bindgen::closure::Closure;
    let doc = web_sys::window().and_then(|w| w.document()).expect("a document");
    let root = root_element().expect("a root");
    let attr = |n: &str| root.get_attribute(n);
    page::set_mode(attr("data-ark-mode").and_then(|m| page::Mode::of_word(&m)).unwrap_or(page::Mode::Static));
    let width = attr("data-ark-width");
    page::set_every_width(width.as_deref() == Some("every"));
    page::set_width(width.and_then(|w| w.parse().ok()));
    page::set_args(Address::query_of(&attr("data-ark-args").unwrap_or_default()));
    let _ = root.set_attribute("data-ark-js", "");
    let app = install(app);
    listen::<W>(&app);
    // A static page was written at its path alone, a server's at its whole
    // address: the first draw is the written one, and a static page's query
    // is taken once the page is the module's.
    let here = address();
    let written = match page::mode() {
        page::Mode::Server => here.clone(),
        _ => here.bare(),
    };
    let arrive = app.borrow_mut().arrive(&written);
    for m in arrive {
        app.borrow_mut().update(m);
    }
    // A form sent or a popover toggled by a control is the click's now.
    let stop = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| e.prevent_default());
    let _ = doc.add_event_listener_with_callback_and_bool("submit", stop.as_ref().unchecked_ref(), true);
    stop.forget();
    let popovers = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        let on = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        if on.and_then(|el| el.closest("[popovertarget]").ok().flatten()).is_some() {
            e.prevent_default();
        }
    });
    let _ = doc.add_event_listener_with_callback_and_bool("click", popovers.as_ref().unchecked_ref(), true);
    popovers.forget();
    // What the writer wired for the page without script taken away before
    // the browser takes the page over: its own properties are set then.
    if let Ok(list) = doc.query_selector_all("#ark-root [form], #ark-root [data-ark-off], #ark-root [popovertarget]") {
        for i in 0..list.length() {
            if let Some(el) = list.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) {
                for a in ["form", "formaction", "formmethod", "popovertarget"] {
                    let _ = el.remove_attribute(a);
                }
                if el.has_attribute("data-ark-off") {
                    let _ = el.remove_attribute("disabled");
                    let _ = el.remove_attribute("data-ark-off");
                }
            }
        }
    }
    let version = RwSignal::new(0u64);
    VERSION.with(|v| v.set(Some(version)));
    let mount = doc
        .get_element_by_id("ark-root")
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("#ark-root");
    let handle = leptos::mount::hydrate_from(mount, move || {
        move || {
            version.get();
            draw_window()
        }
    });
    std::mem::forget(handle);
    // At the window's own width from the next frame.
    request_frame(move || {
        page::set_every_width(false);
        page::set_width(None);
        if here != written {
            let arrive = app.borrow_mut().arrive(&here);
            arriving(|| {
                for m in arrive {
                    send(m);
                }
            });
        }
        redraw();
    });
}

/// What a page written as HTML says besides its view.
#[derive(Clone, Debug, Default)]
pub struct Written {
    pub mode: page::Mode,
    pub route: Address,
    /// Where the site is served from: `/`, or `/money/`.
    pub base: String,
    /// The page's own arguments, read back as the route is (`today=…`).
    pub args: Vec<(String, String)>,
    /// The width it is drawn at, or none for every width at once
    /// (`page::Band`): the browser's first draw is the same.
    pub width: Option<f64>,
    pub accent: String,
    pub css: String,
    /// The app's own `<head>`: its title, its icon, its script.
    pub head: String,
    /// Where `#ark-act` is sent (a server's), and its hidden fields (the
    /// CSRF token, the form's own idempotency token).
    pub action: String,
    pub hidden: Vec<(String, String)>,
    /// Why the last form was not taken (a refusal, in the domain's words),
    /// said over the page.
    pub notice: Option<String>,
}

/// A whole page: `view` (a [`Served::draw`]) inside the document
/// [`Written`] says, the theme's and the app's styles in its head, and the
/// two forms its controls are wired to (`page`).
pub fn document(w: &Written, view: &str) -> String {
    use page::escape as e;
    let theme = w
        .args
        .iter()
        .chain(w.route.query.iter())
        .find(|(k, v)| k == "theme" && (v == "light" || v == "dark"))
        .map(|(_, v)| format!(" data-theme=\"{v}\""))
        .unwrap_or_default();
    let reduced = w.args.iter().chain(w.route.query.iter()).any(|(k, v)| k == "reduce-motion" && v == "1");
    let args = w.args.iter().map(|(k, v)| format!("{}={}", e(k), e(v))).collect::<Vec<_>>().join("&amp;");
    let hidden: String = w
        .hidden
        .iter()
        .map(|(k, v)| format!("<input type=\"hidden\" name=\"{}\" value=\"{}\">", e(k), e(v)))
        .collect();
    format!(
        "<!doctype html>\n<html lang=\"en\" data-accent=\"{accent}\"{theme} data-motion=\"{motion}\" data-ark-mode=\"{mode}\" \
         data-ark-base=\"{base}\" data-ark-width=\"{width}\" data-ark-args=\"{args}\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"color-scheme\" content=\"light dark\">\n\
         <style id=\"ark-style\">{THEME_CSS}\n{ARKUI_CSS}\n{writing}\n{css}</style>\n{head}\n</head>\n<body>\n\
         <form id=\"ark-go\" method=\"get\"></form>\n\
         <form id=\"ark-act\" method=\"post\" action=\"{action}\"><button name=\"ark\" value=\"\" hidden></button>\
         <input type=\"hidden\" name=\"route\" value=\"{route}\">{hidden}</form>\n\
         {notice}<div id=\"ark-root\">{view}</div>\n</body>\n</html>\n",
        accent = e(&w.accent),
        motion = if reduced { "reduced" } else { "full" },
        mode = w.mode.word(),
        base = e(&w.base),
        width = w.width.map_or("every".to_string(), |w| w.to_string()),
        writing = writing::WRITING_CSS,
        css = w.css,
        head = w.head,
        action = e(&w.action),
        route = e(&w.route.url()),
        notice = w
            .notice
            .as_ref()
            .map(|n| format!("<p class=\"ark-notice\" role=\"alert\">{}</p>\n", e(n)))
            .unwrap_or_default(),
    )
}

/// An app drawn as HTML by native code: the export's, a server's. Its
/// draw records its controls (`page`), and a form sent to it is taken
/// against the draw it makes again ([`Served::take`]). One at a time on a
/// thread: the page's state is the thread's.
#[cfg(feature = "ssr")]
pub struct Served<W: WebHosted> {
    app: Rc<RefCell<WebHost<W>>>,
}

#[cfg(feature = "ssr")]
impl<W: WebHosted> Served<W> {
    /// `app` at `route`, in `mode`, drawn at `width` (none: for every
    /// width at once, `page::Band`), with `args`.
    pub fn new(app: WebHost<W>, mode: page::Mode, route: &Address, args: &[(String, String)], width: Option<f64>) -> Served<W> {
        ark_app::set_web();
        page::set_mode(mode);
        page::set_width(width);
        page::set_every_width(width.is_none());
        page::set_args(args.to_vec());
        let app = install(app);
        let arrive = app.borrow_mut().arrive(route);
        for m in arrive {
            app.borrow_mut().update(m);
        }
        Served { app }
    }

    /// Put back at `route` as it opened (`WebHost::rewind`).
    pub fn rewind(&self, route: &Address) {
        self.app.borrow_mut().rewind();
        UI.with(|u| *u.borrow_mut() = Ui::default());
        let arrive = self.app.borrow_mut().arrive(route);
        for m in arrive {
            self.app.borrow_mut().update(m);
        }
    }

    pub fn app(&self) -> std::cell::Ref<'_, WebHost<W>> {
        self.app.borrow()
    }

    pub fn app_mut(&self) -> std::cell::RefMut<'_, WebHost<W>> {
        self.app.borrow_mut()
    }

    /// The window's view as HTML: what `#ark-root` holds.
    pub fn draw(&self) -> String {
        use leptos::prelude::RenderHtml;
        (move || draw_window()).to_html()
    }

    /// A form sent: drawn again, then the fields that changed and the press
    /// taken against that draw (`page::take`).
    pub fn take(&self, press: &str, fields: &[(String, String)]) -> Result<(), page::Refused> {
        let _ = self.draw();
        page::take(press, fields)
    }
}

/// Every page of a static site (`page::Mode::Static`): one per path the
/// app declares (`Windowed::paths`), each at the UI's defaults, or why not
/// — more paths than `bound`. A control whose press only moves the app to
/// another declared path, its query empty, is a link to that page; one
/// that changes the data, or leads to UI state (a query) or a path not
/// declared, is drawn disabled until script runs. Each control's lead is
/// found by pressing it on the app put back where the page opened
/// (`WebHost::rewind`), its changes counted and never sent
/// (`WebHost::dry_run`). Returns each page's route, the app as that page shows
/// it (its title, its description) and its HTML (written by `write` from
/// its view).
#[cfg(feature = "ssr")]
pub fn prerender<W: WebHosted>(
    open: &dyn Fn() -> WebHost<W>,
    bound: usize,
    width: Option<f64>,
    args: &[(String, String)],
    write: &dyn Fn(&Address, &WebHost<W>, &str) -> String,
) -> Result<Vec<(Address, String)>, String> {
    use page::{Lead, Mode};
    use std::collections::{HashMap, HashSet};
    let paths = open().paths();
    if paths.len() > bound {
        return Err(format!("{} static paths, past the bound of {bound}: declare fewer", paths.len()));
    }
    let declared: HashSet<Address> = paths.iter().cloned().collect();
    let base = args
        .iter()
        .find(|(k, _)| k == "base")
        .map(|(_, v)| v.trim_end_matches('/').to_string())
        .unwrap_or_default();
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut out = vec![];
    for r in &paths {
        let n = {
            let s = Served::new(open(), Mode::Static, r, args, width);
            s.draw();
            page::controls()
        };
        let mut leads: HashMap<usize, Lead> = HashMap::new();
        let mut kept: Option<Served<W>> = None;
        for i in 0..n {
            // The app is put back and pressed again, or opened afresh after
            // a press that did not run. The controls of the draw before stay
            // recorded: an app put back is where that draw was made.
            let s = match kept.take() {
                Some(s) => {
                    s.rewind(r);
                    s
                }
                None => {
                    let s = Served::new(open(), Mode::Static, r, args, width);
                    s.draw();
                    s.app_mut().dry_run(true);
                    s
                }
            };
            let before = s.app().changes();
            let pressed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| page::press(i))).unwrap_or(false);
            let to = s.app().route();
            let leads_to = pressed && s.app().changes() == before && to.query.is_empty() && to != *r && declared.contains(&to);
            leads.insert(
                i,
                if leads_to {
                    Lead::To(format!("{base}{}", to.path_url()))
                } else {
                    Lead::Stays
                },
            );
            if pressed {
                kept = Some(s);
            }
        }
        page::set_leads(Some(leads));
        let s = Served::new(open(), Mode::Static, r, args, width);
        let view = s.draw();
        let html = write(r, &s.app(), &view);
        page::set_leads(None);
        out.push((r.clone(), html));
    }
    std::panic::set_hook(quiet);
    Ok(out)
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
    // A bound key moved its rows (`J`): a draw first finds them, so the
    // one shown lights them where they went.
    if let Some(follow) = UI.with(|u| u.borrow_mut().follow.take()) {
        drawn(|d| *d = Drawn::default());
        page::begin_draw();
        let _ = app_view();
        UI.with(|u| follow.find(&mut u.borrow_mut()));
    }
    drawn(|d| *d = Drawn::default());
    page::begin_draw();
    let view = app_view();
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

/// The app's view: once at the window's width, or — written for every
/// width (`page::every_width`) — once per band, each in its
/// `.ark-at-<band>`, which the theme's media queries show alone at its
/// widths. Every band's controls are the page's, in the order drawn, so a
/// form from any of them is taken against the same draw made again.
fn app_view() -> Option<AnyView> {
    let f = APP_VIEW.with(|v| v.borrow().clone())?;
    if !page::every_width() {
        return Some(f());
    }
    let bands: Vec<AnyView> = page::Band::ALL
        .into_iter()
        .map(|b| {
            page::set_band(Some(b));
            drawn(|d| *d = Drawn::default());
            let v = f();
            page::set_band(None);
            html::div().class(format!("ark-at ark-at-{}", b.word())).child(v).into_any()
        })
        .collect();
    Some(bands.into_any())
}

/// `f` on the next animation frame: after the DOM the draw made is there.
pub fn request_frame(f: impl FnOnce() + 'static) {
    if !cfg!(target_arch = "wasm32") {
        return;
    }
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
    let Ok(list) = doc.query_selector_all(".ark-popover-layer") else {
        return;
    };
    for i in 0..list.length() {
        let Some(layer) = list.item(i).and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) else {
            continue;
        };
        let Some(panel) = layer.query_selector(".ark-popover").ok().flatten() else {
            continue;
        };
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
