//! The shell (`ark_app::shell`, ui/docs/design.md) in the DOM, a standard
//! Mac window's way and `arkui::view::shell`'s: the sections a sidebar on
//! a wide window (groups under their headings, the footer at its foot), a
//! tab bar at the top of a narrower regular one and at the foot of a
//! compact one; a toolbar row — the way back and the navigation items,
//! the title, the actions, the inspector's toggle and the search field at
//! its trailing end — with a large title under it short of a sidebar; the
//! inspector a pane at the trailing edge (a sheet when compact); a
//! question or a notice over everything.
//!
//! The size class is the window's width against `[layout]`'s tokens,
//! read from the theme's CSS variables (`--ark-compact-width`, …), so the
//! numbers are written once, in ui/theme.toml; what only moves (the title
//! inline or large, the search in the bar or under the title) is the
//! generated container queries' (`.ark-if-sidebar`, `.ark-if-compact`).

use std::cell::Cell;
use std::rc::Rc;

use ark_app::shell::compact_tag;
use ark_app::view::{Input, Part, Placement, View};
use leptos::html;
use leptos::prelude::*;

use crate::parts::{dialog, flatten, nodes, page};
use crate::{glyph, in_pane, row_drawn, send, Pane, Row};

/// How wide the window is, as the parts that adapt read it
/// (`arkui::shell::SizeClass`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeClass {
    Compact,
    Medium,
    Expanded,
}

/// How the sections are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sections {
    Sidebar,
    TopBar,
    BottomBar,
}

thread_local! {
    static TOKENS: Cell<Option<(f64, f64, f64)>> = const { Cell::new(None) };
}

/// `[layout]`'s compact, expanded and sidebar widths, from the theme's
/// CSS: the page's computed variables in a browser, and natively the
/// same variables as ui/theme.py wrote them into `theme.css`.
pub fn widths() -> (f64, f64, f64) {
    if let Some(t) = TOKENS.with(|t| t.get()) {
        return t;
    }
    let written = |name: &str| -> Option<f64> {
        let at = crate::THEME_CSS.find(&format!("{name}:"))? + name.len() + 1;
        let rest = &crate::THEME_CSS[at..];
        rest[..rest.find(';')?].trim().trim_end_matches("px").parse().ok()
    };
    let read = |name: &str| -> f64 {
        let computed = || {
            web_sys::window()
                .and_then(|w| {
                    let root = w.document()?.document_element()?;
                    let style = w.get_computed_style(&root).ok()??;
                    style.get_property_value(name).ok()
                })
                .and_then(|v| v.trim().trim_end_matches("px").parse().ok())
        };
        let found = if cfg!(target_arch = "wasm32") {
            computed().or_else(|| written(name))
        } else {
            written(name)
        };
        found.unwrap_or_else(|| panic!("theme.css has no {name}"))
    };
    let t = (read("--ark-compact-width"), read("--ark-expanded-width"), read("--ark-sidebar-min-width"));
    TOKENS.with(|c| c.set(Some(t)));
    t
}

/// The window's width, in CSS pixels: the band's being drawn when a page
/// is drawn for every width (`page::Band`).
pub fn width() -> f64 {
    if let Some(b) = crate::page::band() {
        return b.width();
    }
    if let Some(w) = crate::page::width() {
        return w;
    }
    let sidebar = || widths().2;
    if !cfg!(target_arch = "wasm32") {
        return sidebar();
    }
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or_else(sidebar)
}

pub fn size_class() -> SizeClass {
    let (compact, expanded, _) = widths();
    let w = width();
    if w < compact {
        SizeClass::Compact
    } else if w < expanded {
        SizeClass::Medium
    } else {
        SizeClass::Expanded
    }
}

pub fn sections() -> Sections {
    match size_class() {
        SizeClass::Compact => Sections::BottomBar,
        _ if width() >= widths().2 => Sections::Sidebar,
        _ => Sections::TopBar,
    }
}

/// What a part drawn inside the shell is handed from the parts around it.
struct Ctx<'h> {
    back: Option<crate::Action>,
    /// Where the way back goes, said beside its chevron (a group's
    /// heading, as an iPhone's Back says the page it returns to).
    back_label: Option<String>,
    top_tabs: Option<AnyView>,
    hosts: &'h dyn Fn(&'static str) -> AnyView,
}

/// A shell's view in the DOM; each hosted screen is what `hosts` draws for
/// its name.
pub fn shell<M: Clone + 'static>(view: View<M>, hosts: &dyn Fn(&'static str) -> AnyView) -> AnyView {
    let mut ctx = Ctx {
        back: None,
        back_label: None,
        top_tabs: None,
        hosts,
    };
    html::div().class("ark-shell-root").child(draw(view, &mut ctx)).into_any()
}

fn draw<M: Clone + 'static>(v: View<M>, ctx: &mut Ctx<'_>) -> AnyView {
    let mut body: Vec<AnyView> = vec![];
    let mut over: Vec<AnyView> = vec![];
    for v in flatten(v) {
        let View::Part(p) = v else { continue };
        match p {
            p @ (Part::Confirm { .. } | Part::Alert { .. } | Part::Sheet { .. } | Part::Prompt { .. }) => over.push(dialog(p)),
            p => body.push(shell_part(p, ctx)),
        }
    }
    let base = match body.len() {
        1 => body.pop().unwrap_or_else(|| ().into_any()),
        _ => html::div().class("ark-column ark-fill").child(body).into_any(),
    };
    match over.is_empty() {
        true => base,
        false => html::div().class("ark-fill ark-stack").child((base, over)).into_any(),
    }
}

/// A section as the shell draws it.
struct Tab<M> {
    title: String,
    glyph: String,
    tag: String,
    badge: Option<i64>,
    group: Option<&'static str>,
    content: View<M>,
}

/// A group of sections that is a place of its own (`tab_section`): its
/// heading, glyph and tag, what it shows, and where its sections start.
struct Group<M> {
    title: &'static str,
    glyph: &'static str,
    tag: &'static str,
    index: View<M>,
    first: usize,
}

fn tabs_of<M>(v: View<M>, group: Option<&'static str>, out: &mut Vec<Tab<M>>, groups: &mut Vec<Group<M>>) {
    for v in flatten(v) {
        match v {
            View::Part(Part::SectionTab {
                title,
                glyph,
                tag,
                badge,
                content,
            }) => out.push(Tab {
                title: title.get(),
                glyph: glyph.get(),
                tag: tag.get(),
                badge: badge.and_then(|b| b.get()),
                group,
                content: *content,
            }),
            View::Part(Part::TabGroup { title, tabs, place }) => {
                if let Some((glyph, tag, index)) = place {
                    groups.push(Group {
                        title,
                        glyph,
                        tag,
                        index: *index,
                        first: out.len(),
                    });
                }
                tabs_of(*tabs, Some(title), out, groups)
            }
            _ => {}
        }
    }
}

fn shell_part<M: Clone + 'static>(p: Part<M>, ctx: &mut Ctx<'_>) -> AnyView {
    match p {
        Part::Tabs {
            selected,
            select,
            tabs,
            footer,
        } => {
            let chosen = selected.get();
            let mut all = vec![];
            let mut groups = vec![];
            tabs_of(*tabs, None, &mut all, &mut groups);
            let how = sections();
            // A group's own place, when that is what is chosen.
            let place = groups.iter().position(|g| g.tag == chosen);
            match how {
                Sections::Sidebar => {
                    let at = all.iter().position(|t| t.tag == chosen).or(match place {
                        Some(_) => None,
                        None => Some(0),
                    });
                    let mut items: Vec<AnyView> = vec![];
                    let mut content = View::Empty;
                    let mut heading = None;
                    for (i, t) in all.into_iter().enumerate() {
                        if t.group != heading {
                            heading = t.group;
                            match (heading, groups.iter().find(|g| g.first == i)) {
                                (Some(""), _) | (None, _) => {}
                                (Some(h), Some(g)) => {
                                    let tap = select.call(g.tag.to_string());
                                    items.push(
                                        crate::wired!(html::button(), crate::tapped(h, &tap))
                                            .class(if place.is_some_and(|p| groups[p].first == i) {
                                                "ark-sidebar-heading ark-on"
                                            } else {
                                                "ark-sidebar-heading"
                                            })
                                            .on(leptos::ev::click, move |_| send(tap.clone()))
                                            .child(h)
                                            .into_any(),
                                    );
                                }
                                (Some(h), None) => items.push(html::div().class("ark-sidebar-heading").child(h).into_any()),
                            }
                        }
                        if Some(i) == at {
                            content = t.content;
                            crate::drawn(|d| d.sidebar_at = Some(i));
                        }
                        let tap = select.call(t.tag.clone());
                        items.push(in_pane(Pane::Sidebar, || sidebar_item(&t.title, &t.glyph, t.badge, Some(i) == at, tap)));
                    }
                    if let Some(p) = place {
                        content = groups.swap_remove(p).index;
                    }
                    let foot = match *footer {
                        View::Empty => None,
                        f => Some(html::div().class("ark-sidebar-footer").child(draw(f, ctx))),
                    };
                    let side = html::nav()
                        .class("ark-sidebar")
                        .attr("aria-label", "Sections")
                        .attr("data-focused", crate::ui(|u| u.focus == Pane::Sidebar).then_some("true"))
                        .child((html::div().class("ark-sidebar-places").child(items), foot));
                    let shown = draw(content, ctx);
                    html::div()
                        .class("ark-split-sidebar")
                        .child((side, html::div().class("ark-shell-main").child(shown)))
                        .into_any()
                }
                Sections::TopBar | Sections::BottomBar => {
                    // The tab shown: a group's when its place or one of its
                    // sections is chosen, else the chosen tag's parent's.
                    let exact = all.iter().position(|t| t.tag == chosen);
                    let in_group = exact.and_then(|i| {
                        let g = all[i].group?;
                        groups.iter().position(|p| p.title == g)
                    });
                    let on_group = place.or(in_group);
                    let shown_tag = compact_tag(&chosen).to_string();
                    let at = match on_group {
                        Some(_) => exact,
                        None => Some(all.iter().position(|t| t.tag == shown_tag).unwrap_or(0)),
                    };
                    let mut heads = vec![];
                    let mut content = View::Empty;
                    for (i, t) in all.into_iter().enumerate() {
                        if let Some(k) = groups.iter().position(|g| g.first == i) {
                            let g = &groups[k];
                            heads.push((g.title.to_string(), g.glyph.to_string(), g.tag.to_string(), None, on_group == Some(k)));
                        }
                        if Some(i) == at {
                            content = t.content;
                        }
                        if t.group.is_none() {
                            heads.push((t.title, t.glyph, t.tag.clone(), t.badge, on_group.is_none() && t.tag == shown_tag));
                        }
                    }
                    match (place, in_group) {
                        (Some(p), _) => content = groups.swap_remove(p).index,
                        // A section of a group, pushed over its index.
                        (None, Some(g)) => {
                            let back = select.call(groups[g].tag.to_string());
                            ctx.back = Some(Rc::new(move || send(back.clone())));
                            ctx.back_label = Some(groups[g].title.to_string());
                        }
                        _ => {}
                    }
                    if how == Sections::TopBar {
                        let bar: Vec<AnyView> = heads
                            .into_iter()
                            .map(|(title, _, tag, badge, on)| top_tab(title, badge, on, select.call(tag)))
                            .collect();
                        ctx.top_tabs = Some(html::div().class("ark-top-tabs").role("tablist").child(bar).into_any());
                        return draw(content, ctx);
                    }
                    let bar: Vec<AnyView> = heads
                        .into_iter()
                        .map(|(title, glyph, tag, badge, on)| bottom_tab(title, glyph, badge, on, select.call(tag)))
                        .collect();
                    let shown = draw(content, ctx);
                    html::div()
                        .class("ark-column ark-fill")
                        .child((
                            html::div().class("ark-shell-main").child(shown),
                            html::nav().class("ark-bottom-tabs").role("tablist").child(bar),
                        ))
                        .into_any()
                }
            }
        }
        Part::SectionTab { content, .. } | Part::SettingsPane { content, .. } => draw(*content, ctx),
        Part::TabGroup { tabs, .. } => draw(*tabs, ctx),
        Part::Split {
            content,
            detail,
            showing,
            back,
        } => match (size_class(), showing.get()) {
            (SizeClass::Expanded, _) => html::div()
                .class("ark-split")
                .child((
                    html::div().class("ark-split-content").child(draw(*content, ctx)),
                    html::div().class("ark-split-detail").child(draw(*detail, ctx)),
                ))
                .into_any(),
            (_, true) => pushed(*detail, back, ctx),
            (_, false) => draw(*content, ctx),
        },
        Part::Push {
            content,
            detail,
            showing,
            back,
        } => match showing.get() {
            true => pushed(*detail, back, ctx),
            false => draw(*content, ctx),
        },
        Part::Toolbar { title, items, content } => toolbar(title.get(), *items, *content, ctx),
        Part::Searchable {
            prompt,
            text,
            typed,
            content,
        } => html::div()
            .class("ark-column ark-fill")
            .child((
                html::div().class("ark-search-row").child(search(prompt, text.get(), typed)),
                draw(*content, ctx),
            ))
            .into_any(),
        Part::Inspector {
            title,
            shown,
            toggle,
            content,
            inspector,
        } => {
            let content = draw(*content, ctx);
            if !shown.get() {
                return content;
            }
            let pane = in_pane(Pane::Inspector, || draw(*inspector, ctx));
            match size_class() {
                SizeClass::Compact => {
                    let close = toggle.call(false);
                    let away = close.clone();
                    html::div()
                        .class("ark-fill ark-stack")
                        .child((
                            content,
                            html::div().class("ark-scrim").on(leptos::ev::mousedown, move |_| send(away.clone())),
                            html::div()
                                .class("ark-inspector-sheet")
                                .attr("data-motion", "rise")
                                .role("dialog")
                                .attr("aria-label", title)
                                .child((
                                    html::div()
                                        .class("ark-sheet-bar")
                                        .child((html::span().class("ark-sheet-title").child(title), bar_button("Done", "x", true, close))),
                                    html::div().class("ark-sheet-body").child(pane),
                                )),
                        ))
                        .into_any()
                }
                _ => html::div()
                    .class("ark-with-inspector")
                    .child((
                        html::div().class("ark-inspected").child(content),
                        html::aside()
                            .class("ark-inspector")
                            .attr("aria-label", title)
                            .attr("data-focused", crate::ui(|u| u.focus == Pane::Inspector).then_some("true"))
                            .child(pane),
                    ))
                    .into_any(),
            }
        }
        Part::Hosted { name } => (ctx.hosts)(name),
        Part::SettingsPanes { selected, select, panes } => {
            let chosen = selected.get();
            let mut heads = vec![];
            let mut shown = View::Empty;
            for (i, v) in flatten(*panes).into_iter().enumerate() {
                if let View::Part(Part::SettingsPane { title, glyph: g, content }) = v {
                    let on = title == chosen || (chosen.is_empty() && i == 0);
                    if on {
                        shown = *content;
                    }
                    let tap = select.call(title.to_string());
                    heads.push(
                        crate::wired!(html::button(), crate::tapped(title, &tap))
                            .class(if on { "ark-pane-tab ark-on" } else { "ark-pane-tab" })
                            .role("tab")
                            .attr("aria-selected", if on { "true" } else { "false" })
                            .on(leptos::ev::click, move |_| send(tap.clone()))
                            .child((glyph(g, 22), html::span().child(title)))
                            .into_any(),
                    );
                }
            }
            html::div()
                .class("ark-column ark-fill")
                .child((
                    html::div().class("ark-pane-tabs").role("tablist").child(heads),
                    html::div().class("ark-settings-body").child(draw(shown, ctx)),
                ))
                .into_any()
        }
        Part::Page { top, content, .. } => page(View::List(vec![*top, *content])),
        p => nodes(View::Part(p)).into_iter().next().unwrap_or_else(|| ().into_any()),
    }
}

/// A detail pushed onto its section: its own toolbar with the way back at
/// its leading end, or a Back row over it when it has none.
fn pushed<M: Clone + 'static>(detail: View<M>, back: M, ctx: &mut Ctx<'_>) -> AnyView {
    let has_bar = flat_ref(&detail).iter().any(|v| matches!(v, View::Part(Part::Toolbar { .. })));
    let go = back.clone();
    if has_bar {
        ctx.back = Some(Rc::new(move || send(go.clone())));
        return draw(detail, ctx);
    }
    html::div()
        .class("ark-column ark-fill")
        .child((
            html::div().class("ark-back-row").child(bar_button("Back", "chevron-left", true, back)),
            draw(detail, ctx),
        ))
        .into_any()
}

fn flat_ref<M>(v: &View<M>) -> Vec<&View<M>> {
    match v {
        View::List(vs) => vs.iter().flat_map(flat_ref).collect(),
        View::Empty => vec![],
        v => vec![v],
    }
}

/// A section's toolbar over its content.
fn toolbar<M: Clone + 'static>(title: String, items: View<M>, content: View<M>, ctx: &mut Ctx<'_>) -> AnyView {
    let content = match content {
        View::Part(Part::Push {
            detail,
            showing,
            back,
            content,
        }) => match showing.get() {
            true => return pushed(*detail, back, ctx),
            false => *content,
        },
        c => c,
    };
    let how = sections();
    let (found, toggle, content) = peel(content);
    let mut lead: Vec<AnyView> = vec![];
    if let Some(back) = ctx.back.take() {
        let said = ctx.back_label.take();
        let name = said.clone().unwrap_or_else(|| "Back".into());
        lead.push(
            crate::wired!(html::button(), crate::page::tap(&name, back.clone()))
                .class(if said.is_some() {
                    "ark-bar-button ark-bar-back"
                } else {
                    "ark-bar-button"
                })
                .attr("title", name.clone())
                .attr("aria-label", name)
                .on(leptos::ev::click, move |_| back())
                .child((glyph("chevron-left", 17), said.map(|s| html::span().child(s))))
                .into_any(),
        );
    }
    let mut principal = vec![];
    let mut trailing = vec![];
    for v in flatten(items) {
        if let View::Part(Part::BarItem {
            title: t,
            glyph: g,
            placement,
            enabled,
            tap,
        }) = v
        {
            let b = bar_button(t, g, enabled.get(), tap);
            match placement {
                Placement::Navigation => lead.push(b),
                Placement::Principal => principal.push(b),
                Placement::Primary | Placement::Secondary => trailing.push(b),
            }
        }
    }
    if let Some((t, shown, tap)) = toggle {
        trailing.push(
            crate::wired!(html::button(), crate::tapped(t, &tap))
                .class(if shown { "ark-bar-button ark-on" } else { "ark-bar-button" })
                .attr("title", t)
                .attr("aria-label", t)
                .attr("aria-pressed", if shown { "true" } else { "false" })
                .on(leptos::ev::click, move |_| send(tap.clone()))
                .child(glyph("panel-right", 17))
                .into_any(),
        );
    }
    let inline = how == Sections::Sidebar;
    let middle = match ctx.top_tabs.take() {
        Some(tabs) => vec![tabs],
        None => principal,
    };
    let in_bar = match (how, &found) {
        (Sections::BottomBar, _) | (_, None) => None,
        (_, Some((prompt, text, typed))) => Some(html::div().class("ark-bar-search").child(search(prompt, text.clone(), typed.clone()))),
    };
    let under = match (how, &found) {
        (Sections::BottomBar, Some((prompt, text, typed))) => {
            Some(html::div().class("ark-under-search").child(search(prompt, text.clone(), typed.clone())))
        }
        _ => None,
    };
    let bar = html::header().class("ark-toolbar").child((
        html::div().class("ark-bar-lead").child(lead),
        inline.then(|| html::h1().class("ark-bar-title").child(title.clone())),
        html::div().class("ark-bar-middle").child(middle),
        html::div().class("ark-bar-trailing").child(trailing),
        in_bar,
    ));
    let large = (!inline).then(|| html::h1().class("ark-large-title").child(title));
    html::div()
        .class("ark-column ark-fill")
        .child((
            bar,
            large,
            under,
            html::div()
                .class("ark-toolbar-content")
                .child(in_pane(Pane::Content, || draw(content, ctx))),
        ))
        .into_any()
}

type Found<M> = (&'static str, String, Input<String, M>);

/// The search field and the inspector's toggle a toolbar takes from what
/// it is over, and what is left to draw.
#[allow(clippy::type_complexity)]
fn peel<M: Clone + 'static>(v: View<M>) -> (Option<Found<M>>, Option<(&'static str, bool, M)>, View<M>) {
    match v {
        View::Part(Part::Searchable {
            prompt,
            text,
            typed,
            content,
        }) => {
            let (_, toggle, rest) = peel(*content);
            (Some((prompt, text.get(), typed)), toggle, rest)
        }
        View::Part(Part::Inspector {
            title,
            shown,
            toggle,
            content,
            inspector,
        }) => {
            let (found, _, rest) = peel(*content);
            let on = shown.get();
            let tap = toggle.call(!on);
            let kept = Part::Inspector {
                title,
                shown,
                toggle,
                content: Box::new(rest),
                inspector,
            };
            (found, Some((title, on, tap)), View::Part(kept))
        }
        v => (None, None, v),
    }
}

/// A toolbar's button: its glyph alone, as a Mac toolbar draws one, its
/// title said when the pointer rests on it.
pub fn bar_button<M: Clone + 'static>(title: &'static str, g: &'static str, enabled: bool, tap: M) -> AnyView {
    crate::wired!(html::button(), crate::tapped(title, &tap))
        .class("ark-bar-button")
        .attr("title", title)
        .attr("aria-label", title)
        .prop("disabled", !enabled)
        .on(leptos::ev::click, move |_| send(tap.clone()))
        .child(glyph(g, 17))
        .into_any()
}

fn sidebar_item<M: Clone + 'static>(title: &str, g: &str, badge: Option<i64>, selected: bool, tap: M) -> AnyView {
    let t = tap.clone();
    let (_, lit) = row_drawn(Row {
        title: title.to_string(),
        open: Some(Rc::new(move || send(t.clone()))),
        menu: None,
    });
    let mut class = String::from("ark-sidebar-item");
    if selected {
        class.push_str(" ark-selected");
    }
    if lit {
        class.push_str(" ark-cursor");
    }
    crate::wired!(html::button(), crate::tapped(title, &tap))
        .class(class)
        .attr("data-cursor", lit.then_some("true"))
        .attr("aria-current", selected.then_some("page"))
        .on(leptos::ev::click, move |_| send(tap.clone()))
        .child((
            glyph(g, 16),
            html::span().class("ark-sidebar-label").child(title.to_string()),
            badge.map(|n| html::span().class("ark-count").child(n.to_string())),
        ))
        .into_any()
}

fn top_tab<M: Clone + 'static>(title: String, badge: Option<i64>, on: bool, tap: M) -> AnyView {
    crate::wired!(html::button(), crate::tapped(&title, &tap))
        .class(if on { "ark-top-tab ark-on" } else { "ark-top-tab" })
        .role("tab")
        .attr("aria-selected", if on { "true" } else { "false" })
        .on(leptos::ev::click, move |_| send(tap.clone()))
        .child((title, badge.map(|n| html::span().class("ark-count-badge").child(n.to_string()))))
        .into_any()
}

fn bottom_tab<M: Clone + 'static>(title: String, g: String, badge: Option<i64>, on: bool, tap: M) -> AnyView {
    crate::wired!(html::button(), crate::tapped(&title, &tap))
        .class(if on { "ark-bottom-tab ark-on" } else { "ark-bottom-tab" })
        .role("tab")
        .attr("aria-selected", if on { "true" } else { "false" })
        .on(leptos::ev::click, move |_| send(tap.clone()))
        .child((
            html::span()
                .class("ark-bottom-mark")
                .child((glyph(&g, 17), badge.map(|n| html::span().class("ark-count-badge").child(n.to_string())))),
            html::span().class("ark-bottom-title").child(title),
        ))
        .into_any()
}

/// A search field: what is typed sent as it is typed; `/` comes here.
pub fn search<M: Clone + 'static>(prompt: &'static str, value: String, typed: Input<String, M>) -> AnyView {
    crate::fields::search_field(prompt, value, Rc::new(move |s| send(typed.call(s))))
}
