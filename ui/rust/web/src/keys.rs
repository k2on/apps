//! The keyboard every Ark app has (ui/docs/design.md, "The Keyboard, for
//! Every App"), on the page: key listeners on the document, as the desktop
//! has them (`arkui::shell::Keyboard`, `vim`, `which`, `command`).
//!
//! - The app's commands by their shortcuts (`ark_app::shell::command_for`,
//!   Control or ⌘ with the key), and every one by name in the command box
//!   (`:` or ⌘K / Ctrl-K).
//! - The vim layer over the rows drawn: `j`/`k` and the arrows, a count,
//!   `gg`/`G`/`{n}G`, `Ctrl-d`/`Ctrl-u`, `zz`/`zt`/`zb`, `h`/`l` and
//!   `Tab`/`Shift-Tab` between the panes, `Enter`/`o` open, `/` the search
//!   field, `n`/`N` the next row matching it, `Shift-F10` or the menu key
//!   the row's context menu, `?` the keys panel, `Esc` drops it all.
//! - A key moves nothing (motion.md's frequency rule): the cursor is on
//!   its row on the frame the key went down, and kept in view by a snap.
//!
//! A field that has the keys keeps its own: typing a name walks no list.

use std::cell::RefCell;
use std::rc::Rc;

use ark_app::shell::{command_for, CommandMenu};
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::{drawn, menu, send, ui, ui_mut, Pane, WebApp};

/// What the keys panel lists of the grammar: the keys, what they do.
pub const GRAMMAR: &[(&str, &str)] = &[
    ("j / ↓", "down"),
    ("k / ↑", "up"),
    ("h / l", "the pane that way"),
    ("Tab", "the next pane"),
    ("gg / G", "first / last"),
    ("{n}G", "the n-th row"),
    ("zz / zt / zb", "scroll: the cursor in the middle, at the top, at the foot"),
    ("Ctrl-d / Ctrl-u", "a page down / up"),
    ("Enter / o", "open"),
    ("/", "search"),
    ("n / N", "next / previous match"),
    ("Shift-F10", "the row's menu"),
    (": / ⌘K", "the command box"),
    ("?", "this panel"),
    ("Esc", "drop what is half-typed"),
];

/// How far a page moves (`vim::PAGE`).
const PAGE: usize = 10;

/// A command as the command box lists it: its title, its shortcut, what it does.
type Listed = (String, Option<String>, crate::Action);

struct Commands {
    titles: Vec<Listed>,
}

thread_local! {
    static COMMANDS: RefCell<Option<Rc<dyn Fn() -> Commands>>> = const { RefCell::new(None) };
}

fn mac() -> bool {
    web_sys::window()
        .and_then(|w| w.navigator().platform().ok())
        .is_some_and(|p| p.starts_with("Mac"))
}

/// A shortcut as this platform writes it: `⇧⌘N` on a Mac, `Ctrl-Shift-N` elsewhere.
pub fn shortcut_label(s: &ark_app::shell::Shortcut) -> String {
    if mac() {
        return s.label();
    }
    let mut out = String::from("Ctrl-");
    if s.option {
        out.push_str("Alt-");
    }
    if s.shift {
        out.push_str("Shift-");
    }
    out.push_str(&s.key.to_uppercase());
    out
}

fn commands_of<M: Clone + 'static>(menus: &[CommandMenu<M>]) -> Commands {
    Commands {
        titles: menus
            .iter()
            .flat_map(|m| &m.items)
            .map(|c| {
                let run = c.run.clone();
                (
                    c.title.to_string(),
                    c.shortcut.as_ref().map(shortcut_label),
                    Rc::new(move || send(run.clone())) as Rc<dyn Fn()>,
                )
            })
            .collect(),
    }
}

pub(crate) fn listen<A: WebApp>(app: Rc<RefCell<A>>) {
    COMMANDS.with(|c| {
        let app = app.clone();
        *c.borrow_mut() = Some(Rc::new(move || commands_of(&app.borrow().commands())));
    });
    let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| press::<A>(&app, e));
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        let _ = doc.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    }
    on_key.forget();
    let on_resize = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(crate::redraw);
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
    }
    on_resize.forget();
}

fn in_field(e: &web_sys::KeyboardEvent) -> bool {
    e.target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .is_some_and(|el| matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") || el.closest(".ark-calendar").ok().flatten().is_some())
}

fn press<A: WebApp>(app: &Rc<RefCell<A>>, e: web_sys::KeyboardEvent) {
    let key = e.key();
    let command = e.ctrl_key() || e.meta_key();
    // A menu has every key while it is up.
    if ui(|u| u.menu.is_some()) {
        if menu::key(&key) {
            e.prevent_default();
        }
        return;
    }
    if ui(|u| u.command.is_some()) || in_field(&e) {
        return;
    }
    if command {
        if key.eq_ignore_ascii_case("k") {
            e.prevent_default();
            ui_mut(|u| u.command = Some((String::new(), 0)));
            return;
        }
        if let Some(m) = command_for(&app.borrow().commands(), &key.to_lowercase(), e.shift_key(), e.alt_key()).map(|c| c.run.clone()) {
            e.prevent_default();
            send(m);
            return;
        }
        if e.ctrl_key() {
            if let Some(handled) = chord(&key) {
                if handled {
                    e.prevent_default();
                }
                return;
            }
        }
        return;
    }
    if key == "Escape" {
        let modal = drawn(|d| d.modal.clone());
        ui_mut(|u| {
            u.pending.clear();
            u.keys_panel = false;
            u.popover = None;
        });
        if let Some(m) = modal {
            m();
        }
        return;
    }
    if drawn(|d| d.modal.is_some()) {
        return;
    }
    if (key == "F10" && e.shift_key()) || key == "ContextMenu" {
        e.prevent_default();
        context_on_cursor();
        return;
    }
    if vim(&key, e.shift_key()) {
        e.prevent_default();
    }
}

/// `Ctrl` with a key of the grammar: a page, back and forward.
fn chord(key: &str) -> Option<bool> {
    match key {
        "d" => step(PAGE as isize),
        "u" => step(-(PAGE as isize)),
        _ => return None,
    }
    Some(true)
}

fn rows_in(pane: Pane) -> usize {
    drawn(|d| d.rows[pane_index(pane)].len())
}

fn pane_index(p: Pane) -> usize {
    match p {
        Pane::Sidebar => 0,
        Pane::Content => 1,
        Pane::Inspector => 2,
    }
}

/// The panes with rows, left to right.
fn panes() -> Vec<Pane> {
    [Pane::Sidebar, Pane::Content, Pane::Inspector]
        .into_iter()
        .filter(|p| rows_in(*p) > 0)
        .collect()
}

fn focus() -> Pane {
    let f = ui(|u| u.focus);
    match rows_in(f) > 0 {
        true => f,
        false => panes().first().copied().unwrap_or(Pane::Content),
    }
}

/// Where `pane`'s cursor is: the sidebar's is the section shown, as the
/// desktop's is (`arkui::shell::Routed::Section`).
fn cursor_of(pane: Pane) -> usize {
    match (pane, drawn(|d| d.sidebar_at)) {
        (Pane::Sidebar, Some(at)) => at,
        _ => ui(|u| u.cursor[pane_index(pane)]),
    }
}

/// The cursor put on row `i` of `pane`. On the sidebar the cursor is the
/// section, so moving it shows the section it lands on: `j`/`k` walk the
/// app's places (the docs' pages) as the desktop's sidebar does.
fn set_cursor(pane: Pane, i: usize) {
    let n = rows_in(pane);
    let i = i.min(n.saturating_sub(1));
    ui_mut(|u| {
        u.focus = pane;
        u.keyed = true;
        u.cursor[pane_index(pane)] = i;
        u.reveal = true;
    });
    if pane == Pane::Sidebar && drawn(|d| d.sidebar_at) != Some(i) {
        if let Some(open) = drawn(|d| d.rows[pane_index(pane)].get(i).and_then(|r| r.open.clone())) {
            open();
        }
    }
}

fn step(d: isize) {
    let p = focus();
    let at = cursor_of(p) as isize;
    set_cursor(p, (at + d).max(0) as usize);
}

fn count(pending: &str) -> Option<usize> {
    pending.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
}

/// A key of the vim layer: whether it was one.
fn vim(key: &str, shift: bool) -> bool {
    let pending = ui(|u| u.pending.clone());
    let n = count(&pending);
    let clear = || ui_mut(|u| u.pending.clear());
    match (pending.trim_start_matches(|c: char| c.is_ascii_digit()), key) {
        ("g", "g") => {
            clear();
            set_cursor(focus(), n.map_or(0, |n| n.saturating_sub(1)));
        }
        ("z", "z" | "t" | "b") => {
            clear();
            scroll_cursor(match key {
                "z" => web_sys::ScrollLogicalPosition::Center,
                "t" => web_sys::ScrollLogicalPosition::Start,
                _ => web_sys::ScrollLogicalPosition::End,
            });
        }
        (_, "g" | "z") => ui_mut(|u| u.pending = format!("{}{key}", n.map(|n| n.to_string()).unwrap_or_default())),
        (_, d) if d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()) && !(d == "0" && n.is_none()) => ui_mut(|u| u.pending.push_str(d)),
        (_, "j" | "ArrowDown") => {
            clear();
            step(n.unwrap_or(1) as isize);
        }
        (_, "k" | "ArrowUp") => {
            clear();
            step(-(n.unwrap_or(1) as isize));
        }
        (_, "G") => {
            clear();
            let p = focus();
            set_cursor(p, n.map_or(usize::MAX, |n| n.saturating_sub(1)));
        }
        (_, "h" | "ArrowLeft" | "l" | "ArrowRight" | "Tab") => {
            clear();
            let ps = panes();
            if ps.is_empty() {
                return true;
            }
            let at = ps.iter().position(|p| *p == focus()).unwrap_or(0) as isize;
            let d: isize = match key {
                "h" | "ArrowLeft" => -1,
                "Tab" if shift => -1,
                _ => 1,
            };
            let to = if key == "Tab" {
                (at + d).rem_euclid(ps.len() as isize)
            } else {
                (at + d).clamp(0, ps.len() as isize - 1)
            };
            let p = ps[to as usize];
            set_cursor(p, cursor_of(p));
        }
        (_, "Enter" | "o") => {
            clear();
            let p = focus();
            let c = cursor_of(p);
            if let Some(open) = drawn(|d| d.rows[pane_index(p)].get(c).and_then(|r| r.open.clone())) {
                open();
            }
        }
        (_, "/") => {
            clear();
            focus_search();
        }
        (_, "n" | "N") => {
            clear();
            next_match(if key == "n" { 1 } else { -1 });
        }
        (_, "?") => {
            clear();
            ui_mut(|u| u.keys_panel = !u.keys_panel);
        }
        (_, ":") => {
            clear();
            ui_mut(|u| u.command = Some((String::new(), 0)));
        }
        _ => {
            if !pending.is_empty() {
                clear();
            }
            return false;
        }
    }
    true
}

fn scroll_cursor(at: web_sys::ScrollLogicalPosition) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector("[data-cursor]").ok().flatten())
    {
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(at);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
    }
}

fn focus_search() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Ok(list) = doc.query_selector_all("[data-search]") {
        for i in 0..list.length() {
            if let Some(el) = list.item(i).and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
                if el.offset_parent().is_some() {
                    let _ = el.focus();
                    return;
                }
            }
        }
    }
}

/// The next row (or the one before) whose title has what the search field holds.
fn next_match(d: isize) {
    let needle = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|doc| doc.query_selector("[data-search]").ok().flatten())
        .and_then(|el| el.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.value().to_lowercase())
        .unwrap_or_default();
    if needle.is_empty() {
        return;
    }
    let p = focus();
    let titles: Vec<String> = drawn(|dr| dr.rows[pane_index(p)].iter().map(|r| r.title.to_lowercase()).collect());
    let n = titles.len() as isize;
    let at = cursor_of(p) as isize;
    for k in 1..=n {
        let i = (at + d * k).rem_euclid(n.max(1));
        if titles[i as usize].contains(&needle) {
            set_cursor(p, i as usize);
            return;
        }
    }
}

/// The cursor's row's menu, at the row: the menu key's, Shift-F10's.
fn context_on_cursor() {
    let p = focus();
    let c = cursor_of(p);
    let Some(open) = drawn(|d| d.rows[pane_index(p)].get(c).and_then(|r| r.menu.clone())) else {
        return;
    };
    let at = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector("[data-cursor]").ok().flatten())
        .map(|el| {
            let r = el.get_bounding_client_rect();
            (r.left() + 24.0, r.bottom() - 4.0)
        })
        .unwrap_or((200.0, 200.0));
    open(at.0, at.1);
    // By a key: its first entry lit, no motion.
    menu::as_by_key();
}

/// The context menu of the row titled `title`, at the row (`?context=`).
pub fn open_context(title: &str) {
    let found = drawn(|d| d.contexts.iter().find(|(t, _)| t == title).map(|(_, o)| o.clone()));
    let Some(open) = found else { return };
    let at = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| {
            d.query_selector(&format!("[data-title=\"{}\"]", title.replace('"', "\\\"")))
                .ok()
                .flatten()
        })
        .map(|el| {
            let r = el.get_bounding_client_rect();
            (r.left() + r.width() * 0.4, r.top() + r.height() * 0.6)
        })
        .unwrap_or((300.0, 300.0));
    open(at.0, at.1);
}

/// The control's menu named `id` (`?menu=`).
pub fn open_menu(id: &str) {
    if let Some(open) = drawn(|d| d.menus.iter().find(|(i, _)| i == id).map(|(_, o)| o.clone())) {
        open();
        ui_mut(|u| {
            if let Some(m) = &mut u.menu {
                m.by_key = false;
            }
        });
    }
}

/// The command box and the keys panel, over the page.
pub(crate) fn overlay() -> AnyView {
    (command_box(), keys_panel()).into_any()
}

fn all_commands() -> Vec<Listed> {
    COMMANDS.with(|c| c.borrow().clone()).map(|f| f().titles).unwrap_or_default()
}

fn matching(q: &str) -> Vec<Listed> {
    let q = q.to_lowercase();
    all_commands().into_iter().filter(|(t, _, _)| t.to_lowercase().contains(&q)).collect()
}

fn command_box() -> AnyView {
    let Some((q, lit)) = ui(|u| u.command.clone()) else {
        return ().into_any();
    };
    let found = matching(&q);
    let rows: Vec<AnyView> = found
        .iter()
        .enumerate()
        .map(|(i, (t, k, run))| {
            let run = run.clone();
            html::div()
                .class(if i == lit { "ark-command-row ark-lit" } else { "ark-command-row" })
                .role("option")
                .on(leptos::ev::mousedown, move |e: web_sys::MouseEvent| {
                    e.prevent_default();
                    ui_mut(|u| u.command = None);
                    run();
                })
                .child((
                    html::span().child(t.clone()),
                    k.clone().map(|k| html::span().class("ark-menu-key").child(k)),
                ))
                .into_any()
        })
        .collect();
    let keys = move |e: web_sys::KeyboardEvent| {
        let n = matching(&ui(|u| u.command.clone().map(|c| c.0).unwrap_or_default())).len();
        match e.key().as_str() {
            "Escape" => {
                e.prevent_default();
                ui_mut(|u| u.command = None);
            }
            "ArrowDown" | "ArrowUp" => {
                e.prevent_default();
                let d: isize = if e.key() == "ArrowDown" { 1 } else { -1 };
                ui_mut(|u| {
                    if let Some(c) = &mut u.command {
                        c.1 = (c.1 as isize + d).clamp(0, n.saturating_sub(1) as isize) as usize;
                    }
                });
            }
            "Enter" => {
                e.prevent_default();
                let (q, lit) = ui(|u| u.command.clone()).unwrap_or_default();
                ui_mut(|u| u.command = None);
                if let Some((_, _, run)) = matching(&q).get(lit) {
                    run();
                }
            }
            _ => {}
        }
    };
    html::div()
        .class("ark-modal-layer ark-command-layer")
        .child((
            html::div()
                .class("ark-scrim ark-clear")
                .on(leptos::ev::mousedown, |_| ui_mut(|u| u.command = None)),
            html::div().class("ark-command").role("dialog").attr("aria-label", "Commands").child((
                html::input()
                    .class("ark-command-input")
                    .attr("placeholder", "Type a Command")
                    .attr("data-autofocus", "true")
                    .prop("value", q)
                    .on(leptos::ev::input, move |e| {
                        let v = event_target_value(&e);
                        ui_mut(|u| u.command = Some((v, 0)))
                    })
                    .on(leptos::ev::keydown, keys),
                html::div().class("ark-command-list").role("listbox").child(rows),
            )),
        ))
        .into_any()
}

/// The command box written open in the page's flow, for a page that shows
/// what it is (the docs): `query` typed, each row its title, its shortcut
/// and what it runs, the `lit` one lit. `typed` is what is typed sent;
/// a row pressed runs. The same box as `:` and ⌘K open over the window.
pub fn command_written(query: &str, rows: Vec<(String, Option<String>, crate::Action)>, lit: usize, typed: Rc<dyn Fn(String)>) -> AnyView {
    let rows: Vec<AnyView> = rows
        .into_iter()
        .enumerate()
        .map(|(i, (t, k, run))| {
            html::div()
                .class(if i == lit { "ark-command-row ark-lit" } else { "ark-command-row" })
                .role("option")
                .attr("aria-selected", (i == lit).then_some("true"))
                .on(leptos::ev::click, move |_| run())
                .child((html::span().child(t), k.map(|k| html::span().class("ark-menu-key").child(k))))
                .into_any()
        })
        .collect();
    html::div()
        .class("ark-command ark-command-written")
        .role("dialog")
        .attr("aria-label", "Commands")
        .child((
            html::input()
                .class("ark-command-input")
                .attr("placeholder", "Type a Command")
                .attr("aria-label", "Type a Command")
                .attr("value", query.to_string())
                .prop("value", query.to_string())
                .on(leptos::ev::input, move |e| typed(event_target_value(&e))),
            html::div().class("ark-command-list").role("listbox").child(rows),
        ))
        .into_any()
}

/// The keys panel written open in the page's flow (`?` opens it over the
/// window): the grammar every app has, and `commands`' shortcuts.
pub fn panel_written(commands: &[(String, String)]) -> AnyView {
    keys_drawn(commands.to_vec(), "ark-keys-panel ark-keys-written")
}

fn keys_panel() -> AnyView {
    if !ui(|u| u.keys_panel) {
        return ().into_any();
    }
    let commands: Vec<(String, String)> = all_commands().into_iter().filter_map(|(t, k, _)| k.map(|k| (k, t))).collect();
    keys_drawn(commands, "ark-keys-panel")
}

fn keys_drawn(commands: Vec<(String, String)>, class: &'static str) -> AnyView {
    let grammar: Vec<AnyView> = GRAMMAR
        .iter()
        .map(|(k, w)| {
            html::div()
                .class("ark-key-row")
                .child((html::kbd().child(*k), html::span().child(*w)))
                .into_any()
        })
        .collect();
    let commands: Vec<AnyView> = commands
        .into_iter()
        .map(|(k, t)| {
            html::div()
                .class("ark-key-row")
                .child((html::kbd().child(k), html::span().child(t)))
                .into_any()
        })
        .collect();
    html::div()
        .class(class)
        .role("dialog")
        .attr("aria-label", "Keys")
        .child((
            html::div().class("ark-keys-column").child((html::h3().child("Keys"), grammar)),
            html::div().class("ark-keys-column").child((html::h3().child("Commands"), commands)),
        ))
        .into_any()
}
