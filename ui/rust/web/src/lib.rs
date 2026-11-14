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
//! - An Ark app hosted whole (feature `host`): [`host::WebHost`] runs its
//!   part of its host over the page (`ark_app::window::Windowed`), signed
//!   in to its server and synced ([`live`]) or on its demo, and
//!   [`host::run`] is what its generated entry point calls.
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

pub mod fields;
pub mod figures;
#[cfg(feature = "host")]
pub mod host;
pub mod keys;
mod layout;
#[cfg(feature = "host")]
pub mod live;
pub mod menu;
pub mod page;
pub mod parts;
pub mod shell;
pub mod table;
pub mod writing;

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
        let sink = SINK.with(|s| s.borrow().clone());
        if let Some(sink) = sink {
            sink(m);
        }
    }
    BUSY.with(|b| b.set(false));
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
pub(crate) fn row_drawn(row: Row) -> (usize, bool) {
    let (pane, i) = drawn(|d| {
        let p = d.pane;
        d.rows[p.index()].push(row);
        (p, d.rows[p.index()].len() - 1)
    });
    let lit = ui(|u| u.keyed && u.focus == pane && u.cursor[pane.index()] == i);
    (i, lit)
}

thread_local! {
    /// The screen being drawn, among the app's (app.toml's `[screens]`
    /// keys): what its elements' walk ids are under.
    static SCREEN: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// `f` drawn as the screen `name` (a hosted name's last part: `register`
/// of `accounts.register`): the hosted page's (feature `host`).
#[cfg_attr(not(feature = "host"), allow(dead_code))]
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

/// An app the page runs.
pub trait WebApp: 'static {
    type Msg: Clone + std::fmt::Debug + 'static;
    /// The window's view: its shell, or a page.
    fn view(&self) -> AnyView;
    /// A message taken.
    fn update(&mut self, msg: Self::Msg);
    /// The menu bar's commands: their shortcuts are the keys', their
    /// titles the command box's.
    fn commands(&self) -> Vec<ark_app::shell::CommandMenu<Self::Msg>> {
        vec![]
    }
    /// What the page's address asks for (`Address`: its path, the
    /// content, and its query, the UI's state): the messages that bring
    /// the app there. A static page arrives at its path alone and then,
    /// once the module runs, at the whole address.
    fn arrive(&mut self, _route: &Address) -> Vec<Self::Msg> {
        vec![]
    }
    /// What the field `field` (`search`, or a form's own id) suggests for
    /// what is typed in it.
    fn suggest(&self, _field: &str, _typed: &str) -> Vec<String> {
        vec![]
    }
    /// Where the app is, as the address that brings it back there
    /// (`arrive` reads it): its screen, its selection, its sheet. A page
    /// written as HTML is at its route, and a press that moves the app
    /// leads to its new one (`page`).
    fn route(&self) -> Address {
        Address::default()
    }
    /// Every path a static site writes a page for (Next.js's
    /// `generateStaticParams`): finite, and from the data. A query is never
    /// written: each page is its path at the UI's defaults.
    fn static_paths(&self) -> Vec<Address> {
        vec![]
    }
    /// How many changes the app has made to its data: a press that makes
    /// one is an action, not a way to another page.
    fn changes(&self) -> u64 {
        0
    }
    /// The app put back where it opens, its data as it is: what the
    /// prerenderer presses each control from, rather than opening the app
    /// again. False (the default) when it cannot.
    fn rewind(&mut self) -> bool {
        false
    }
    /// While `on`, a change the app would send its data is counted
    /// (`changes`) and not sent: the prerenderer presses every control,
    /// and one that would change the data is an action, not a way.
    fn dry_run(&mut self, _on: bool) {}
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
        style.set_text_content(Some(&format!("{THEME_CSS}\n{ARKUI_CSS}\n{css}")));
        let _ = head.append_child(&style);
    }
    let app = install(app);
    keys::listen::<A>(app.clone());
    let version = RwSignal::new(0u64);
    VERSION.with(|v| v.set(Some(version)));
    leptos::mount::mount_to_body(move || {
        move || {
            version.get();
            draw_window()
        }
    });
    // What the address asks for, after the first draw.
    arrive_here(&app);
    // And again whenever the address moves in place (the browser's back
    // and forward, or a screenshot's driver walking its shots in one page,
    // ui/docs/shots.md): the app put back where it opens and the page's own
    // menus, popovers, command box and focus with it, then brought where
    // the address says, as a fresh page would be. An app that cannot put
    // itself back (`rewind`) is loaded afresh at that address.
    if cfg!(target_arch = "wasm32") {
        use wasm_bindgen::closure::Closure;
        let app = app.clone();
        let moved = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            let rewound = app.borrow_mut().rewind();
            if !rewound {
                if let Some(w) = web_sys::window() {
                    let _ = w.location().reload();
                }
                return;
            }
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
            arrive_here(&app);
        });
        if let Some(w) = web_sys::window() {
            let _ = w.add_event_listener_with_callback("popstate", moved.as_ref().unchecked_ref());
        }
        moved.forget();
    }
}

/// Where the page's address asks the app to be (its `arrive`), and what of
/// the page's own it asks open: a row's menu (`context`), a control's
/// (`menu`), a field's popover (`popover`), the command box (`command`),
/// the keys panel (`keys`).
fn arrive_here<A: WebApp>(app: &Rc<RefCell<A>>) {
    let arrive: Vec<A::Msg> = app.borrow_mut().arrive(&address());
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
/// drawn, what its fields suggest.
fn install<A: WebApp>(app: A) -> Rc<RefCell<A>> {
    let app = Rc::new(RefCell::new(app));
    SINK.with(|s| {
        let app = app.clone();
        *s.borrow_mut() = Some(Rc::new(move |m: Box<dyn Any>| {
            if let Ok(m) = m.downcast::<A::Msg>() {
                app.borrow_mut().update(*m);
            }
        }));
    });
    {
        let app = app.clone();
        fields::suggest_with(move |f, q| app.borrow().suggest(f, q));
    }
    APP_VIEW.with(|v| {
        let app = app.clone();
        *v.borrow_mut() = Some(Rc::new(move || app.borrow().view()));
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
pub fn hydrate<A: WebApp>(app: A) {
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
    keys::listen::<A>(app.clone());
    // A static page was written at its path alone, a server's at its whole
    // address: the first draw is the written one, and a static page's query
    // is taken once the page is the module's.
    let here = address();
    let written = match page::mode() {
        page::Mode::Server => here.clone(),
        _ => here.bare(),
    };
    let arrive: Vec<A::Msg> = app.borrow_mut().arrive(&written);
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
            let arrive: Vec<A::Msg> = app.borrow_mut().arrive(&here);
            for m in arrive {
                send(m);
            }
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
         <style id=\"ark-style\">{THEME_CSS}\n{ARKUI_CSS}\n{css}</style>\n{head}\n</head>\n<body>\n\
         <form id=\"ark-go\" method=\"get\"></form>\n\
         <form id=\"ark-act\" method=\"post\" action=\"{action}\"><button name=\"ark\" value=\"\" hidden></button>\
         <input type=\"hidden\" name=\"route\" value=\"{route}\">{hidden}</form>\n\
         {notice}<div id=\"ark-root\">{view}</div>\n</body>\n</html>\n",
        accent = e(&w.accent),
        motion = if reduced { "reduced" } else { "full" },
        mode = w.mode.word(),
        base = e(&w.base),
        width = w.width.map_or("every".to_string(), |w| w.to_string()),
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

/// An app drawn as HTML by native code: the prerenderer's, a server's. Its
/// draw records its controls (`page`), and a form sent to it is taken
/// against the draw it makes again ([`Served::take`]). One at a time on a
/// thread: the page's state is the thread's.
#[cfg(feature = "ssr")]
pub struct Served<A: WebApp> {
    app: Rc<RefCell<A>>,
}

#[cfg(feature = "ssr")]
impl<A: WebApp> Served<A> {
    /// `app` at `route`, in `mode`, drawn at `width` (none: for every
    /// width at once, `page::Band`), with `args`.
    pub fn new(app: A, mode: page::Mode, route: &Address, args: &[(String, String)], width: Option<f64>) -> Served<A> {
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

    /// Put back at `route` as it opened, if the app can be (`WebApp::rewind`).
    pub fn rewind(&self, route: &Address) -> bool {
        if !self.app.borrow_mut().rewind() {
            return false;
        }
        UI.with(|u| *u.borrow_mut() = Ui::default());
        let arrive = self.app.borrow_mut().arrive(route);
        for m in arrive {
            self.app.borrow_mut().update(m);
        }
        true
    }

    pub fn app(&self) -> std::cell::Ref<'_, A> {
        self.app.borrow()
    }

    pub fn app_mut(&self) -> std::cell::RefMut<'_, A> {
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
/// app declares (`WebApp::static_paths`), each at the UI's defaults, or
/// why not — more paths than `bound`, the sanity bound the app's
/// app.toml sets. A control whose press only moves the app to another
/// declared path, its query empty, is a link to that page; one that
/// changes the data, or leads to UI state (a query) or a path not
/// declared, is drawn disabled until script runs. Each control's lead is
/// found by pressing it on the app put back where the page opened
/// (`WebApp::rewind`) or opened afresh (`open`). Returns each page's
/// route and its HTML (written by `write` from its view).
#[cfg(feature = "ssr")]
pub fn prerender<A: WebApp>(
    open: &dyn Fn() -> A,
    bound: usize,
    width: Option<f64>,
    args: &[(String, String)],
    write: &dyn Fn(&Address, &str) -> String,
) -> Result<Vec<(Address, String)>, String> {
    use page::{Lead, Mode};
    use std::collections::{HashMap, HashSet};
    let mut paths: Vec<Address> = vec![];
    for p in open().static_paths() {
        let p = p.bare();
        if !paths.contains(&p) {
            paths.push(p);
        }
    }
    if paths.len() > bound {
        return Err(format!(
            "{} static paths, past the bound of {bound} app.toml's [web] static-bound sets: raise it, or declare fewer",
            paths.len()
        ));
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
        let mut kept: Option<Served<A>> = None;
        for i in 0..n {
            // An app that only moved is put back and pressed again; one
            // whose data changed, or that cannot be put back, opened afresh.
            // The controls of the draw before stay recorded: an app put
            // back is where that draw was made.
            let s = match kept.take() {
                Some(s) if s.rewind(r) => s,
                _ => {
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
        let html = write(r, &s.draw());
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
