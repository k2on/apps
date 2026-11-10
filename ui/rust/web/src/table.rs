//! A table (SwiftUI's `Table` on the Mac and a regular iPad, `arkui::table`
//! on the desktop) in the DOM: its headings, its rows a grid of cells, the
//! cursor's row lit, a row's menu on right click. Narrower than compact a
//! table is its rows' first columns over the rest (a `List`), as the
//! iPhone draws one: the parts decide, never the app.

use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;

use crate::menu::{self, Entry};
use crate::{row_drawn, Action, Row};

/// How a column lines up its cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    End,
    Center,
}

/// A column: its heading and how wide (a CSS track: `112px`, `1fr`).
#[derive(Clone, Debug)]
pub struct Column {
    pub heading: &'static str,
    pub width: &'static str,
    pub align: Align,
    /// Dropped from a compact table's row.
    pub wide_only: bool,
    /// Where it goes in a compact table's row, a list's (none: dropped).
    pub slot: Option<Slot>,
}

/// Where a column's cell goes when a compact table is a list, as the
/// iPhone's `List` row has them: the title and the words under it at the
/// leading end, a figure and the words under it at the trailing end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Title,
    Subtitle,
    Trailing,
    TrailingSub,
}

pub fn column(heading: &'static str, width: &'static str, align: Align) -> Column {
    Column {
        heading,
        width,
        align,
        wide_only: false,
        slot: None,
    }
}

impl Column {
    pub fn wide_only(mut self) -> Column {
        self.wide_only = true;
        self
    }

    /// Where it goes in a compact table's row: with any column given one,
    /// a compact table is a list of two-line rows.
    pub fn slot(mut self, slot: Slot) -> Column {
        self.slot = Some(slot);
        self
    }
}

/// A row: its cells, what pressing it does, its menu.
pub struct TableRow {
    pub title: String,
    pub cells: Vec<AnyView>,
    pub open: Option<Action>,
    pub menu: Vec<Entry>,
    /// Shown selected (what the inspector is about).
    pub selected: bool,
    /// A group's heading among the rows: no cursor stops on it.
    pub heading: bool,
}

impl TableRow {
    /// A row of `cells`, titled `title` (what `?context=` and `n` find it by).
    pub fn new(title: impl Into<String>, cells: Vec<AnyView>) -> TableRow {
        TableRow {
            title: title.into(),
            cells,
            open: None,
            menu: vec![],
            selected: false,
            heading: false,
        }
    }
}

/// The table.
pub fn table(columns: &[Column], rows: Vec<TableRow>) -> AnyView {
    table_with(columns, rows, true, None)
}

/// The table, its rows striped or not, and what sits over its headings
/// inside its scroll (a page's headline).
pub fn table_with(columns: &[Column], rows: Vec<TableRow>, zebra: bool, top: Option<AnyView>) -> AnyView {
    let tracks: Vec<&str> = columns.iter().map(|c| c.width).collect();
    let style = format!("grid-template-columns:{}", tracks.join(" "));
    let align = |c: &Column| match c.align {
        Align::Start => "ark-cell",
        Align::End => "ark-cell ark-end",
        Align::Center => "ark-cell ark-center",
    };
    let heads: Vec<AnyView> = columns
        .iter()
        .map(|c| html::div().class(align(c)).role("columnheader").child(c.heading).into_any())
        .collect();
    let compact = crate::shell::size_class() == crate::shell::SizeClass::Compact;
    let listed = compact && columns.iter().any(|c| c.slot.is_some());
    let body: Vec<AnyView> = rows
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let opener: Option<crate::Opener> = (!r.menu.is_empty()).then(|| {
                let (t, entries) = (r.title.clone(), r.menu.clone());
                Rc::new(move |x: f64, y: f64| menu::open(t.clone(), entries.clone(), (x, y), menu::Anchor::Pointer, false)) as crate::Opener
            });
            if let Some(o) = &opener {
                let (t, o) = (r.title.clone(), o.clone());
                crate::drawn(|d| d.contexts.push((t, o)));
            }
            let lit = !r.heading
                && row_drawn(Row {
                    title: r.title.clone(),
                    open: r.open.clone(),
                    menu: opener.clone(),
                })
                .1;
            let mut class = String::from(if r.heading { "ark-table-row ark-heading" } else { "ark-table-row" });
            if i % 2 == 1 && zebra {
                class.push_str(" ark-odd");
            }
            if lit {
                class.push_str(" ark-cursor");
            }
            if r.selected {
                class.push_str(" ark-selected");
            }
            let cells: Vec<AnyView> = match listed {
                true => list_cells(r.cells, columns, r.heading),
                false => r
                    .cells
                    .into_iter()
                    .zip(columns)
                    .filter(|(_, c)| !(compact && c.wide_only))
                    .map(|(cell, c)| html::div().class(align(c)).role("cell").child(cell).into_any())
                    .collect(),
            };
            let open = r.open.clone();
            let hit = r.open.clone().and_then(|o| crate::hit(&r.title, o));
            html::div()
                .class(class)
                .role("row")
                .attr("data-cursor", lit.then_some("true"))
                .attr("data-title", r.title)
                .on(leptos::ev::click, move |_| {
                    if let Some(o) = &open {
                        o()
                    }
                })
                .on(leptos::ev::contextmenu, move |e: web_sys::MouseEvent| {
                    if let Some(o) = &opener {
                        e.prevent_default();
                        o(e.client_x() as f64, e.client_y() as f64);
                    }
                })
                .child((cells, hit))
                .into_any()
        })
        .collect();
    let style = if listed {
        "grid-template-columns:minmax(0,1fr) auto".to_string()
    } else if compact {
        let kept: Vec<&str> = columns.iter().filter(|c| !c.wide_only).map(|c| c.width).collect();
        format!("grid-template-columns:{}", kept.join(" "))
    } else {
        style
    };
    let heads: Vec<AnyView> = if listed {
        vec![]
    } else if compact {
        heads.into_iter().zip(columns).filter(|(_, c)| !c.wide_only).map(|(h, _)| h).collect()
    } else {
        heads
    };
    html::div()
        .class(match (listed, compact) {
            (true, _) => "ark-table ark-compact ark-listed",
            (_, true) => "ark-table ark-compact",
            _ => "ark-table",
        })
        .role("table")
        .attr("style", style)
        .child((
            top.map(|t| html::div().class("ark-table-top").child(t)),
            html::div().class("ark-table-head").role("row").child(heads),
            body,
        ))
        .into_any()
}

/// A row's cells as a list's row: its title over the words under it at the
/// leading end, its figure over its words at the trailing end. A heading
/// keeps its title and its trailing figure.
fn list_cells(cells: Vec<AnyView>, columns: &[Column], heading: bool) -> Vec<AnyView> {
    let mut title = vec![];
    let mut sub = vec![];
    let mut trail = vec![];
    let mut trail_sub = vec![];
    for (cell, c) in cells.into_iter().zip(columns) {
        match c.slot {
            Some(Slot::Title) => title.push(cell),
            Some(Slot::Subtitle) if !heading => {
                if !sub.is_empty() {
                    sub.push(html::span().class("ark-dot-gap").child(" \u{b7} ").into_any());
                }
                sub.push(cell)
            }
            Some(Slot::Trailing) => trail.push(cell),
            Some(Slot::TrailingSub) if !heading => trail_sub.push(cell),
            _ => {}
        }
    }
    vec![
        html::div()
            .class("ark-cell ark-list-lead")
            .role("cell")
            .child((
                html::div().class("ark-list-title").child(title),
                (!sub.is_empty()).then(|| html::div().class("ark-list-sub").child(sub)),
            ))
            .into_any(),
        html::div()
            .class("ark-cell ark-end ark-list-trail")
            .role("cell")
            .child((
                html::div().class("ark-list-figure").child(trail),
                (!trail_sub.is_empty()).then(|| html::div().class("ark-list-sub").child(trail_sub)),
            ))
            .into_any(),
    ]
}
