//! The keyboard's cursor over the rows a view draws: what the browser's
//! keys walk (arkui-web's `row_drawn`), on the desktop. A window's host
//! (`crate::host`) draws its views inside [`drawing`], which hands each
//! pane's rows back as they were drawn — what opening each sends, what it
//! says — and lights the one the cursor is on, in the pane that has the
//! keys, once the keys have walked (a Mac list shows its focus only once it
//! has the keyboard). The app's views say nothing of it: a row is a row,
//! and walking rows is every app's.
//!
//! Nothing moves when the cursor does (ui/docs/motion.md, the frequency
//! rule): the row is lit on the frame the key went down.

use std::any::Any;
use std::cell::RefCell;

use crate::shell::Pane;

/// A row drawn: what it says (what `n`/`N` match a search against) and
/// what opening it sends, as the app's message boxed.
pub struct Row {
    pub title: String,
    pub open: Option<Box<dyn Any>>,
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
}

struct Drawing {
    cursor: Cursor,
    pane: Pane,
    drawn: Drawn,
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

/// A row drawn in the pane being drawn, opened by `open`: whether the
/// cursor is on it. Outside [`drawing`] (a view drawn alone) nothing is.
pub(crate) fn row_drawn<M: Clone + 'static>(title: &str, open: Option<&M>) -> bool {
    DRAWING.with(|d| {
        let mut d = d.borrow_mut();
        let Some(d) = d.as_mut() else { return false };
        let Some(i) = index(d.pane) else { return false };
        let rows = &mut d.drawn.rows[i];
        rows.push(Row {
            title: title.to_string(),
            open: open.map(|m| Box::new(m.clone()) as Box<dyn Any>),
        });
        d.cursor.keyed && d.cursor.focus == d.pane && d.cursor.at[i] == rows.len() - 1
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
}
