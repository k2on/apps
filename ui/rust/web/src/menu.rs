//! A menu in the DOM, as `arkui::context` and `arkui::menu` draw one in
//! iced and AppKit draws an `NSMenu`: a row's context menu at the pointer
//! (right click, the menu key or Shift-F10), a control's menu under it
//! (a toolbar's Auto-Assign, a form's picker), each with its title across
//! the top and its entries, a submenu hanging off an entry.
//!
//! - **Where**: a context menu's corner on the pointer, a control's menu
//!   under the control with its right edge at the control's; kept on the
//!   window, flipped to the other side of the pointer (or of the entry, a
//!   submenu) where it would run off it, as AppKit does.
//! - **Keys**: `↓`/`j` and `↑`/`k` the entries, `→`/`l` into a submenu,
//!   `←`/`h` out of it, `Enter` runs, `Esc` closes one level; a key opens
//!   no motion (motion.md's frequency rule).
//! - **Pointer**: an entry lit under it; a submenu opens after the same
//!   dwell as the desktop's (`menu::DWELL`, 200 ms); a press outside closes
//!   every level.
//! - **Motion**: opened by the pointer, it grows from its corner (0.95, the
//!   theme's `enter-scale`, on the fade); by a key, it is there at once.
//!
//! The measures are arkui's `panel` (a 27 px entry, 4 px padding, a 6 px
//! radius, 198..420 wide), so the two toolkits' menus are one size.

use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;

use crate::{glyph, ui, ui_mut, Action};

/// arkui::panel's measures.
const ENTRY: f64 = 27.0;
const TITLE: f64 = 24.0;
const PADDING: f64 = 4.0;
const MIN_WIDTH: f64 = 198.0;
const MAX_WIDTH: f64 = 420.0;
/// What a menu keeps from the window's edge.
const EDGE: f64 = 8.0;
/// A character's width, for a menu's width before it is laid out
/// (`panel::ENTRY_CHAR`), and what an entry spends besides its words.
const CHAR: f64 = 7.6;
const CHROME: f64 = PADDING * 2.0 + 20.0 + 16.0 + 8.0;
/// How long the pointer rests on an entry before its submenu opens:
/// arkui's `menu::DWELL` of 50 ms ticks.
const DWELL_MS: i32 = 200;

/// One thing a menu offers.
#[derive(Clone)]
pub enum Entry {
    Run {
        glyph: Option<String>,
        label: String,
        destructive: bool,
        /// A check at its start: the one chosen of several.
        checked: bool,
        /// A shortcut said at its end (`⌘N`).
        shortcut: Option<String>,
        action: Action,
    },
    Sub {
        glyph: Option<String>,
        label: String,
        entries: Vec<Entry>,
    },
    Separator,
}

impl Entry {
    /// An entry that sends `m`.
    pub fn run<M: Clone + 'static>(glyph: Option<&str>, label: impl Into<String>, m: M) -> Entry {
        Entry::Run {
            glyph: glyph.map(Into::into),
            label: label.into(),
            destructive: false,
            checked: false,
            shortcut: None,
            action: Rc::new(move || crate::send(m.clone())),
        }
    }

    pub fn destructive(mut self) -> Entry {
        if let Entry::Run { destructive, .. } = &mut self {
            *destructive = true;
        }
        self
    }

    pub fn checked(mut self, on: bool) -> Entry {
        if let Entry::Run { checked, .. } = &mut self {
            *checked = on;
        }
        self
    }

    pub fn shortcut(mut self, s: impl Into<String>) -> Entry {
        if let Entry::Run { shortcut, .. } = &mut self {
            *shortcut = Some(s.into());
        }
        self
    }

    pub fn sub(glyph: Option<&str>, label: impl Into<String>, entries: Vec<Entry>) -> Entry {
        Entry::Sub {
            glyph: glyph.map(Into::into),
            label: label.into(),
            entries,
        }
    }

    fn label(&self) -> &str {
        match self {
            Entry::Run { label, .. } | Entry::Sub { label, .. } => label,
            Entry::Separator => "",
        }
    }

    fn selectable(&self) -> bool {
        !matches!(self, Entry::Separator)
    }

    fn height(&self) -> f64 {
        match self {
            Entry::Separator => 9.0,
            _ => ENTRY,
        }
    }
}

/// How a menu was opened, which decides where it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// A right click: its corner on the pointer.
    Pointer,
    /// A control: under it, its right edge at the control's right edge.
    Below { right: f64, bottom: f64, top: f64 },
}

/// A menu on the page, and the submenus open off it, to any depth (a
/// Mac's menu nests: Categorize ▸ Everyday ▸ Groceries).
pub struct Open {
    pub title: String,
    pub entries: Vec<Entry>,
    pub at: (f64, f64),
    pub anchor: Anchor,
    /// The entry each open submenu hangs off, from the menu inward.
    pub path: Vec<usize>,
    /// The entry lit at each level, the menu's first: one more than `path`.
    pub lits: Vec<Option<usize>>,
    /// Opened by a key: no motion.
    pub by_key: bool,
}

impl Open {
    /// The entries at `level` (0, the menu's own).
    fn level(&self, level: usize) -> &[Entry] {
        let mut es: &[Entry] = &self.entries;
        for &i in &self.path[..level] {
            match es.get(i) {
                Some(Entry::Sub { entries, .. }) => es = entries,
                _ => return &[],
            }
        }
        es
    }

    /// The level the keys are at: the innermost open.
    fn depth(&self) -> usize {
        self.lits.len() - 1
    }

    /// The submenu off entry `i` of the innermost level opened, its first
    /// entry lit when `lit`.
    fn enter(&mut self, i: usize, lit: bool) {
        let d = self.depth();
        if let Some(Entry::Sub { entries, .. }) = self.level(d).get(i) {
            let first = lit.then(|| step(entries, None, 1)).flatten();
            self.lits[d] = Some(i);
            self.path.push(i);
            self.lits.push(first);
        }
    }

    /// Back to `level`: what is open past it closed.
    fn back_to(&mut self, level: usize) {
        self.path.truncate(level);
        self.lits.truncate(level + 1);
    }
}

/// The next selectable entry of `entries` from `at` the way of `d`, round.
fn step(entries: &[Entry], at: Option<usize>, d: isize) -> Option<usize> {
    let n = entries.len() as isize;
    if n == 0 {
        return None;
    }
    let mut i = at.map_or(if d > 0 { -1 } else { n }, |a| a as isize);
    for _ in 0..n {
        i = (i + d).rem_euclid(n);
        if entries[i as usize].selectable() {
            return Some(i as usize);
        }
    }
    None
}

/// A menu opened at `(x, y)`.
pub fn open(title: impl Into<String>, entries: Vec<Entry>, at: (f64, f64), anchor: Anchor, by_key: bool) {
    let first = by_key.then(|| step(&entries, None, 1)).flatten();
    ui_mut(|u| {
        u.menu = Some(Open {
            title: title.into(),
            entries,
            at,
            anchor,
            path: vec![],
            lits: vec![first],
            by_key,
        })
    });
}

/// The menu open, lit as a key opened it: its first entry, no motion.
pub(crate) fn as_by_key() {
    ui_mut(|u| {
        if let Some(m) = &mut u.menu {
            m.by_key = true;
            m.lits[0] = step(&m.entries, None, 1);
        }
    });
}

/// A control's menu, under the element `el`.
pub fn open_below(title: impl Into<String>, entries: Vec<Entry>, el: &web_sys::Element, by_key: bool) {
    let r = el.get_bounding_client_rect();
    open(
        title,
        entries,
        (r.right(), r.bottom()),
        Anchor::Below {
            right: r.right(),
            bottom: r.bottom(),
            top: r.top(),
        },
        by_key,
    );
}

pub fn close() {
    ui_mut(|u| u.menu = None);
}

fn window_size() -> (f64, f64) {
    let w = web_sys::window();
    let get = |v: Option<Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>>| v.and_then(|v| v.ok()).and_then(|v| v.as_f64()).unwrap_or(0.0);
    (get(w.as_ref().map(|w| w.inner_width())), get(w.as_ref().map(|w| w.inner_height())))
}

fn width_of(title: &str, entries: &[Entry]) -> f64 {
    let longest = entries
        .iter()
        .map(|e| {
            let key = match e {
                Entry::Run { shortcut: Some(s), .. } => s.chars().count() as f64 * CHAR + 16.0,
                Entry::Sub { .. } => 22.0,
                _ => 0.0,
            };
            e.label().chars().count() as f64 * CHAR + CHROME + key
        })
        .chain(std::iter::once(title.chars().count() as f64 * CHAR + PADDING * 2.0 + 20.0))
        .fold(0.0, f64::max);
    longest.clamp(MIN_WIDTH, MAX_WIDTH)
}

fn height_of(entries: &[Entry], titled: bool) -> f64 {
    PADDING * 2.0 + if titled { TITLE } else { 0.0 } + entries.iter().map(Entry::height).sum::<f64>()
}

/// Where a panel of `w` by `h` goes for `anchor` at `at`, kept on the
/// window: flipped to the other side where it would run off.
pub fn place(at: (f64, f64), anchor: Anchor, w: f64, h: f64) -> (f64, f64) {
    place_in(at, anchor, w, h, window_size())
}

/// [`place`] in a window `ww` by `wh`.
pub fn place_in(at: (f64, f64), anchor: Anchor, w: f64, h: f64, (ww, wh): (f64, f64)) -> (f64, f64) {
    let (mut x, mut y) = match anchor {
        Anchor::Pointer => at,
        Anchor::Below { right, bottom, .. } => (right - w, bottom + 4.0),
    };
    if x + w > ww - EDGE {
        x = match anchor {
            Anchor::Pointer => at.0 - w,
            _ => ww - EDGE - w,
        };
    }
    if y + h > wh - EDGE {
        y = match anchor {
            Anchor::Pointer => at.1 - h,
            Anchor::Below { top, .. } => top - 4.0 - h,
        };
    }
    (x.max(EDGE), y.max(EDGE))
}

/// The menu open, over the page: a sheet that a press outside closes, the
/// panel, and each submenu open off it, hung beside the entry it opens
/// from (its lit row meeting the submenu's edge: arkui's
/// `SUBMENU_OVERLAP`), on the other side where the window ends.
pub(crate) fn overlay() -> AnyView {
    let Some((title, at, anchor, by_key, levels)) = ui(|u| {
        u.menu.as_ref().map(|m| {
            let levels: Vec<(Vec<Entry>, Option<usize>)> = (0..m.lits.len()).map(|k| (m.level(k).to_vec(), m.lits[k])).collect();
            (m.title.clone(), m.at, m.anchor, m.by_key, levels)
        })
    }) else {
        return ().into_any();
    };
    let (ww, wh) = window_size();
    let mut layers = vec![html::div()
        .class("ark-menu-catch")
        .on(leptos::ev::mousedown, |_| close())
        .on(leptos::ev::contextmenu, |e: web_sys::MouseEvent| {
            e.prevent_default();
            close()
        })
        .into_any()];
    // Where the last panel was, and the row its next submenu hangs off.
    let mut last: Option<(f64, f64, f64, f64)> = None;
    let n = levels.len();
    for (k, (entries, lit)) in levels.iter().enumerate() {
        let titled = k == 0 && !title.is_empty();
        let w = width_of(if k == 0 { &title } else { "" }, entries);
        let h = height_of(entries, titled);
        let (x, y, style) = match last {
            None => {
                let (x, y) = place(at, anchor, w, h);
                let origin = match anchor {
                    Anchor::Pointer if x < at.0 => "right",
                    Anchor::Pointer => "left",
                    Anchor::Below { .. } => "right",
                };
                let vertical = if y < at.1 - 1.0 { "bottom" } else { "top" };
                (x, y, format!("transform-origin:{vertical} {origin}"))
            }
            Some((px, pw, row_top, _)) => {
                let mut sx = px + pw - PADDING;
                if sx + w > ww - EDGE {
                    sx = (px - w + PADDING).max(EDGE);
                }
                let sy = (row_top - PADDING).min(wh - EDGE - h).max(EDGE);
                (sx, sy, String::new())
            }
        };
        // The row the next level hangs off, if one is open.
        let opened = (k + 1 < n).then_some(()).and(*lit);
        if let Some(i) = opened {
            let row_top = y + PADDING + if titled { TITLE } else { 0.0 } + entries[..i].iter().map(Entry::height).sum::<f64>();
            last = Some((x, w, row_top, 0.0));
        }
        layers.push(panel(
            k,
            if k == 0 { &title } else { "" },
            entries,
            *lit,
            format!("left:{x}px;top:{y}px;width:{w}px;{style}"),
            if by_key || k > 0 { "none" } else { "grow" },
        ));
    }
    html::div().class("ark-menu-layer").child(layers).into_any()
}

fn panel(level: usize, title: &str, entries: &[Entry], lit: Option<usize>, style: String, motion: &'static str) -> AnyView {
    let head = (!title.is_empty()).then(|| html::div().class("ark-menu-title").child(title.to_string()).into_any());
    let rows: Vec<AnyView> = entries.iter().enumerate().map(|(i, e)| entry(level, i, e, lit == Some(i))).collect();
    html::div()
        .role("menu")
        .class("ark-menu")
        .attr("style", style)
        .attr("data-motion", motion)
        .child((head, rows))
        .into_any()
}

fn entry(level: usize, i: usize, e: &Entry, lit: bool) -> AnyView {
    let hover = move |_| {
        ui_mut(|u| {
            if let Some(m) = &mut u.menu {
                if level < m.lits.len() {
                    // Another entry rested on closes what is open past it
                    // at once; its own submenu opens after the dwell.
                    if m.path.get(level) != Some(&i) {
                        m.back_to(level);
                    }
                    m.lits[level] = Some(i);
                }
            }
        });
        dwell(level, i);
    };
    match e {
        Entry::Separator => html::div().class("ark-menu-separator").role("separator").into_any(),
        Entry::Run {
            glyph: g,
            label,
            destructive,
            checked,
            shortcut,
            action,
        } => {
            let action = action.clone();
            let mut class = String::from("ark-menu-entry");
            if lit {
                class.push_str(" ark-lit");
            }
            if *destructive {
                class.push_str(" ark-destructive");
            }
            let mark = match (checked, g) {
                (true, _) => glyph("check", 16),
                (false, Some(g)) => glyph(g, 16),
                (false, None) => html::span().class("ark-glyph-space").into_any(),
            };
            html::div()
                .class(class)
                .role("menuitem")
                .on(leptos::ev::mouseenter, hover)
                .on(leptos::ev::mouseup, move |_| {
                    close();
                    action();
                })
                .child((
                    mark,
                    html::span().class("ark-menu-label").child(label.clone()),
                    shortcut.clone().map(|s| html::span().class("ark-menu-key").child(s)),
                ))
                .into_any()
        }
        Entry::Sub { glyph: g, label, .. } => {
            let class = if lit { "ark-menu-entry ark-lit" } else { "ark-menu-entry" };
            html::div()
                .class(class)
                .role("menuitem")
                .attr("aria-haspopup", "menu")
                .on(leptos::ev::mouseenter, hover)
                .on(leptos::ev::mouseup, move |_| {
                    ui_mut(|u| {
                        if let Some(m) = &mut u.menu {
                            if level < m.lits.len() && m.path.get(level) != Some(&i) {
                                m.back_to(level);
                                m.enter(i, false);
                            }
                        }
                    })
                })
                .child((
                    match g {
                        Some(g) => glyph(g, 16),
                        None => html::span().class("ark-glyph-space").into_any(),
                    },
                    html::span().class("ark-menu-label").child(label.clone()),
                    glyph("chevron-right", 14),
                ))
                .into_any()
        }
    }
}

/// The submenu off entry `i` of `level` opened, if the pointer is still
/// on it after the dwell.
fn dwell(level: usize, i: usize) {
    let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
        let still = ui(|u| {
            u.menu
                .as_ref()
                .is_some_and(|m| m.depth() == level && m.lits[level] == Some(i) && matches!(m.level(level).get(i), Some(Entry::Sub { .. })))
        });
        if still {
            ui_mut(|u| {
                if let Some(m) = &mut u.menu {
                    m.enter(i, false);
                }
            });
        }
    });
    if let Some(w) = web_sys::window() {
        use wasm_bindgen::JsCast;
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), DWELL_MS);
    }
}

/// A key pressed while a menu is open: whether the menu took it.
pub(crate) fn key(key: &str) -> bool {
    if !ui(|u| u.menu.is_some()) {
        return false;
    }
    let mut run: Option<Action> = None;
    let took = ui_mut(|u| {
        let Some(m) = &mut u.menu else { return false };
        let d = m.depth();
        match key {
            "ArrowDown" | "j" | "ArrowUp" | "k" => {
                let dir = if key == "ArrowDown" || key == "j" { 1 } else { -1 };
                let next = step(m.level(d), m.lits[d], dir);
                m.lits[d] = next;
            }
            "ArrowRight" | "l" => {
                if let Some(i) = m.lits[d] {
                    m.enter(i, true);
                }
            }
            "ArrowLeft" | "h" => {
                if d > 0 {
                    m.back_to(d - 1);
                }
            }
            "Escape" => match d {
                0 => u.menu = None,
                d => m.back_to(d - 1),
            },
            "Enter" | " " => match m.lits[d].and_then(|i| m.level(d).get(i).cloned()) {
                Some(Entry::Run { action, .. }) => {
                    run = Some(action);
                    u.menu = None;
                }
                Some(Entry::Sub { .. }) => {
                    if let Some(i) = m.lits[d] {
                        m.enter(i, true);
                    }
                }
                _ => {}
            },
            // Every other key is the menu's while it is up, as AppKit's is.
            _ => {}
        }
        true
    });
    if let Some(a) = run {
        a();
    }
    took
}

/// The entries of an `ark_app` row's menu, as this menu offers them.
pub fn of_row<M: Clone + 'static>(items: &[ark_app::view::MenuItem<M>]) -> Vec<Entry> {
    items
        .iter()
        .filter(|m| m.offered())
        .map(|m| {
            let e = Entry::run(Some(&m.glyph.get()), m.title.get(), m.tap.clone());
            if m.destructive.get() {
                e.destructive()
            } else {
                e
            }
        })
        .collect()
}

/// A button that opens `entries` under itself: a control's menu (a
/// toolbar's, a form's picker). `id` is what `?menu=` opens it by.
pub fn button(id: &'static str, title: &'static str, face: AnyView, class: &'static str, entries: Rc<dyn Fn() -> Vec<Entry>>) -> AnyView {
    let opener: Rc<dyn Fn()> = {
        let entries = entries.clone();
        Rc::new(move || {
            if let Some(el) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector(&format!("[data-menu=\"{id}\"]")).ok().flatten())
            {
                open_below(title, entries(), &el, true);
            }
        })
    };
    crate::drawn(|d| d.menus.push((id.to_string(), opener)));
    html::button()
        .class(class)
        .attr("data-menu", id)
        .attr("title", title)
        .attr("aria-haspopup", "menu")
        .on(leptos::ev::click, move |e: web_sys::MouseEvent| {
            use wasm_bindgen::JsCast;
            if let Some(el) = e.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                // A click with no pointer (the keyboard's Enter) is a key's.
                open_below(title, entries(), &el, e.detail() == 0);
            }
        })
        .child(face)
        .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(label: &str) -> Entry {
        Entry::Run {
            glyph: None,
            label: label.into(),
            destructive: false,
            checked: false,
            shortcut: None,
            action: Rc::new(|| {}),
        }
    }

    /// The keys walk past a separator and round the ends. Falsified by a
    /// separator lit.
    #[test]
    fn the_keys_skip_a_separator() {
        let es = vec![run("a"), Entry::Separator, run("b")];
        assert_eq!(step(&es, None, 1), Some(0));
        assert_eq!(step(&es, Some(0), 1), Some(2));
        assert_eq!(step(&es, Some(2), 1), Some(0));
        assert_eq!(step(&es, None, -1), Some(2));
    }

    /// A context menu that would run off the window's right or foot opens
    /// the other side of the pointer, as AppKit's does; one that fits has
    /// its corner on it. Falsified by clamping to the edge instead.
    #[test]
    fn a_menu_flips_at_the_edges() {
        let win = (1000.0, 800.0);
        assert_eq!(place_in((100.0, 100.0), Anchor::Pointer, 200.0, 300.0, win), (100.0, 100.0));
        assert_eq!(place_in((900.0, 700.0), Anchor::Pointer, 200.0, 300.0, win), (700.0, 400.0));
        // A control's: under it, its right edge at the control's; above it
        // where there is no room under.
        let below = Anchor::Below {
            right: 600.0,
            bottom: 40.0,
            top: 10.0,
        };
        assert_eq!(place_in((600.0, 40.0), below, 200.0, 100.0, win), (400.0, 44.0));
        let low = Anchor::Below {
            right: 600.0,
            bottom: 790.0,
            top: 760.0,
        };
        assert_eq!(place_in((600.0, 790.0), low, 200.0, 100.0, win), (400.0, 656.0));
    }
}
