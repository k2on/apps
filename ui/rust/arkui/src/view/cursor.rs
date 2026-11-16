//! The keyboard's cursor over the rows a view draws: what the browser's
//! keys walk (arkui-web's `row_drawn`), on the desktop. A window's host
//! (`crate::host`) draws its views inside [`drawing`], which hands each
//! pane's rows back as they were drawn — what opening each sends, what it
//! says — and lights the one the cursor is on, in the pane that has the
//! keys, once the keys have walked (a Mac list shows its focus only once it
//! has the keyboard). The app's views say nothing of it: a row is a row,
//! and walking rows is every app's.
//!
//! Visual mode (`v`) lights every row from where it was turned on to the
//! cursor, and a row's bound keys ([`Bound`], `ark_app::view::MenuItem::
//! keys`: its own menu's, and its section heading's) are what the host
//! runs on the rows lit.
//!
//! Nothing moves when the cursor does (ui/docs/motion.md, the frequency
//! rule): the row is lit on the frame the key went down.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use ark_app::view::{Bind, MenuItem};

use crate::context_menu::Keyed;
use crate::shell::Pane;

/// A row drawn: what it says (what `n`/`N` match a search against), what
/// opening it sends, as the app's message boxed, the section it was drawn
/// in (`vig` selects a section's rows), and the keys bound on it.
pub struct Row {
    pub title: String,
    pub open: Option<Box<dyn Any>>,
    pub section: usize,
    pub bound: Vec<Bound>,
}

/// A menu item bound to keys, on a row drawn: its keys, what it says (the
/// keys panel's words), and what it sends, as the app's message boxed.
#[derive(Clone)]
pub struct Bound {
    pub bind: Bind,
    pub title: String,
    pub send: Rc<dyn Any>,
    /// The section whose heading binds it, for one its rows carry: run
    /// once over a selection, however many of its rows are lit.
    pub heading: Option<usize>,
}

/// The items of `menu` that keys run, offered now.
pub(crate) fn bound<M: Clone + 'static>(menu: &[MenuItem<M>]) -> Vec<Bound> {
    menu.iter()
        .filter(|m| m.offered())
        .filter_map(|m| {
            Some(Bound {
                bind: m.keys?,
                title: m.title.get(),
                send: Rc::new(m.tap.clone()),
                heading: None,
            })
        })
        .collect()
}

/// Where the cursor is, as the host keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// The pane with the keys.
    pub focus: Pane,
    /// The cursor's row in the content, the detail and the inspector.
    pub at: [usize; 3],
    /// The keys have walked the rows: the cursor shows.
    pub keyed: bool,
    /// Visual mode's other end, in the pane with the keys: every row from
    /// it to the cursor is lit, and what a key binds runs on each.
    pub anchor: Option<usize>,
}

impl Cursor {
    /// The rows lit in pane `i`, first to last: the cursor's, or from the
    /// visual selection's anchor to it.
    pub fn lit(&self, i: usize) -> (usize, usize) {
        let at = self.at[i];
        let from = self.anchor.unwrap_or(at);
        (from.min(at), from.max(at))
    }
}

/// A pane's place in [`Cursor::at`] and [`Drawn::rows`]; none for the
/// sidebar, whose cursor is the section shown.
pub fn index(p: Pane) -> Option<usize> {
    match p {
        Pane::Sidebar => None,
        Pane::Content => Some(0),
        Pane::Detail => Some(1),
        Pane::Inspector => Some(2),
    }
}

/// What one draw left for the keys: each pane's rows, in order.
#[derive(Default)]
pub struct Drawn {
    pub rows: [Vec<Row>; 3],
    /// The sidebar's places, what choosing each sends, and the one shown:
    /// the sidebar's cursor is the section shown
    /// (`crate::shell::Routed::Section`).
    pub sections: Vec<Box<dyn Any>>,
    pub section_at: usize,
    /// What puts away the sheet or question on top, while one is up:
    /// `Esc`'s, as the browser's is.
    pub modal: Option<Box<dyn Any>>,
}

struct Drawing {
    cursor: Cursor,
    pane: Pane,
    drawn: Drawn,
    /// The section being drawn: its number, and its heading's bound items,
    /// which each of its rows carries.
    section: usize,
    sections: usize,
    heading: Vec<Bound>,
}

thread_local! {
    static DRAWING: RefCell<Option<Drawing>> = const { RefCell::new(None) };
}

/// `f` drawn with the cursor at `cursor`, and the rows it drew.
pub fn drawing<R>(cursor: Cursor, f: impl FnOnce() -> R) -> (R, Drawn) {
    let was = DRAWING.with(|d| {
        d.borrow_mut().replace(Drawing {
            cursor,
            pane: Pane::Content,
            drawn: Drawn::default(),
            section: 0,
            sections: 0,
            heading: vec![],
        })
    });
    let r = f();
    let drawn = DRAWING
        .with(|d| std::mem::replace(&mut *d.borrow_mut(), was))
        .map(|d| d.drawn)
        .unwrap_or_default();
    (r, drawn)
}

/// What `f` draws is in `pane` (the inspector's, a split's detail).
pub(crate) fn in_pane<R>(pane: Pane, f: impl FnOnce() -> R) -> R {
    let was = DRAWING.with(|d| d.borrow_mut().as_mut().map(|d| std::mem::replace(&mut d.pane, pane)));
    let r = f();
    if let Some(was) = was {
        DRAWING.with(|d| {
            if let Some(d) = d.borrow_mut().as_mut() {
                d.pane = was;
            }
        });
    }
    r
}

/// What `f` draws are the rows of one section, whose heading binds
/// `heading` on each.
pub(crate) fn in_section<R>(heading: Vec<Bound>, f: impl FnOnce() -> R) -> R {
    let was = DRAWING.with(|d| {
        d.borrow_mut().as_mut().map(|d| {
            d.sections += 1;
            let n = d.sections;
            let heading = heading.into_iter().map(|b| Bound { heading: Some(n), ..b }).collect();
            (std::mem::replace(&mut d.section, n), std::mem::replace(&mut d.heading, heading))
        })
    });
    let r = f();
    if let Some((section, heading)) = was {
        DRAWING.with(|d| {
            if let Some(d) = d.borrow_mut().as_mut() {
                d.section = section;
                d.heading = heading;
            }
        });
    }
    r
}

/// A row drawn in the pane being drawn, opened by `open`: whether the
/// cursor (or a visual selection) is on it. Outside [`drawing`] (a view
/// drawn alone) nothing is.
#[cfg(test)]
pub(crate) fn row_drawn<M: Clone + 'static>(title: &str, open: Option<&M>) -> bool {
    row_bound(title, open, vec![]) == Keyed::Here
}

/// [`row_drawn`] for a row whose menu binds `bound`: where the cursor is
/// from it, for its menu's keys ([`Keyed`]).
pub(crate) fn row_bound<M: Clone + 'static>(title: &str, open: Option<&M>, mut bound: Vec<Bound>) -> Keyed {
    DRAWING.with(|d| {
        let mut d = d.borrow_mut();
        let Some(d) = d.as_mut() else { return Keyed::Away };
        let Some(i) = index(d.pane) else { return Keyed::Away };
        bound.extend(d.heading.iter().cloned());
        let rows = &mut d.drawn.rows[i];
        rows.push(Row {
            title: title.to_string(),
            open: open.map(|m| Box::new(m.clone()) as Box<dyn Any>),
            section: d.section,
            bound,
        });
        let n = rows.len() - 1;
        let (first, last) = d.cursor.lit(i);
        match (d.cursor.keyed, d.cursor.focus == d.pane && first <= n && n <= last) {
            (false, _) => Keyed::Away,
            (true, true) => Keyed::Here,
            (true, false) => Keyed::Elsewhere,
        }
    })
}

/// A sheet or a question drawn over the window, put away by `dismiss`:
/// the last drawn is the one on top, which `Esc` puts away.
pub(crate) fn modal_drawn<M: Clone + 'static>(dismiss: &M) {
    DRAWING.with(|d| {
        if let Some(d) = d.borrow_mut().as_mut() {
            d.drawn.modal = Some(Box::new(dismiss.clone()));
        }
    })
}

/// One of the sidebar's places drawn, chosen by `select`; `shown` when
/// it is the section on screen.
pub(crate) fn section_drawn<M: Clone + 'static>(select: &M, shown: bool) {
    DRAWING.with(|d| {
        if let Some(d) = d.borrow_mut().as_mut() {
            if shown {
                d.drawn.section_at = d.drawn.sections.len();
            }
            d.drawn.sections.push(Box::new(select.clone()));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows come back by pane, and only the cursor's is lit, once keyed.
    /// Falsified by lighting a row in a pane without the keys.
    #[test]
    fn rows_are_drawn_by_pane_and_lit_by_the_cursor() {
        let cursor = Cursor {
            focus: Pane::Content,
            at: [1, 0, 0],
            keyed: true,
            anchor: None,
        };
        let (lit, drawn) = drawing(cursor, || {
            let a = row_drawn("Rent", Some(&1u8));
            let b = row_drawn("Groceries", Some(&2u8));
            let c = in_pane(Pane::Inspector, || row_drawn("Assigned", None::<&u8>));
            (a, b, c)
        });
        assert_eq!(lit, (false, true, false));
        assert_eq!(drawn.rows[0].len(), 2);
        assert_eq!(drawn.rows[2].len(), 1);
        assert_eq!(drawn.rows[0][1].open.as_ref().and_then(|m| m.downcast_ref::<u8>()), Some(&2));
        let (unkeyed, _) = drawing(Cursor { keyed: false, ..cursor }, || {
            row_drawn("Rent", Some(&1u8));
            row_drawn("Groceries", Some(&2u8))
        });
        assert!(!unkeyed);
    }

    /// A visual selection lights every row from its anchor to the cursor,
    /// and a section's heading binds its keys on each of its rows, with
    /// the section they were drawn in. Falsified by lighting the cursor's
    /// row alone, or by rows that carry only their own menu's keys.
    #[test]
    fn a_selection_is_lit_and_a_heading_binds_its_rows() {
        let cursor = Cursor {
            focus: Pane::Content,
            at: [3, 0, 0],
            keyed: true,
            anchor: Some(1),
        };
        let heading = vec![Bound {
            bind: Bind::new("rg"),
            title: "Rename".into(),
            send: Rc::new(9u8),
            heading: None,
        }];
        let (lit, drawn) = drawing(cursor, || {
            let mut lit = vec![row_drawn("Rent", Some(&1u8))];
            in_section(heading, || {
                for (t, m) in [("Groceries", 2u8), ("Dining", 3), ("Fuel", 4)] {
                    lit.push(row_drawn(t, Some(&m)));
                }
            });
            lit.push(row_drawn("Gifts", Some(&5u8)));
            lit
        });
        assert_eq!(lit, [false, true, true, true, false]);
        let rows = &drawn.rows[0];
        assert!(rows[0].bound.is_empty() && rows[4].bound.is_empty());
        assert_eq!(rows[2].bound[0].bind.keys, "rg");
        assert_eq!(rows[2].bound[0].send.downcast_ref::<u8>(), Some(&9));
        assert_eq!(rows[2].bound[0].heading, Some(1));
        assert_eq!((rows[1].section, rows[3].section), (1, 1));
        assert_ne!(rows[0].section, rows[1].section);
    }
}
