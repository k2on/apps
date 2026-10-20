//! What a cell of the explorer's tables draws, and how wide its column is
//! (`docs/plan-guards.md` D4): a table's rows and a query's answer are the
//! same [`Grid`] of values under headings, every column a width of its own
//! and every cell cut to it, so no value ever draws over its neighbour.
//!
//! The widths are decided here, before iced lays anything out, so they are
//! in characters against an estimate — the same kind `arkui::fit` works in,
//! and for the same reason. Three rules make that safe:
//!
//! - **A column is as wide as its header and a sample of its values**, within
//!   [`MIN_CHARS`] and [`MAX_CHARS`]: a column of booleans is not given the
//!   room a title is, and one long file path does not take the whole pane.
//! - **A value that does not fit loses its middle to an ellipsis**, by
//!   characters, as `arkui`'s table cuts a cell ([`arkui::fit::middle`]): a
//!   value's ends are what tell it from its neighbours. A heading loses its
//!   end instead, as the table's do ([`arkui::fit::tail`]); and the view also
//!   clips each cell to its column, so an estimate that errs costs a few
//!   pixels of glyph and never a neighbour.
//! - **Nothing is lost by cutting**: [`Grid::detail`] is the whole value of
//!   the cell under the cursor, which the view draws on a line of its own.
//!   An id is drawn as its first eight hex digits — enough to tell rows
//!   apart, as a short commit hash is — and whole in the detail line.

use crate::edit::cell_text;
use crate::value::{Ty, Value};

/// The narrowest a column is, in characters.
pub const MIN_CHARS: usize = 6;
/// The widest a column is, in characters: past this a value is cut.
pub const MAX_CHARS: usize = 36;
/// How many rows a column's width is measured over.
pub const SAMPLE: usize = 200;
/// Roughly how wide one character of a cell is at size 13, in pixels. It
/// errs wide, as `arkui::panel::ENTRY_CHAR` does and a shade more, because
/// hex digits and capitals run wider than the average: erring wide is a
/// strip of empty column, erring narrow a glyph cut by the clip.
pub const CHAR_PX: f32 = 7.5;
/// The gap after every column: the same gap `arkui`'s table leaves.
pub const GAP_PX: f32 = arkui::table::SPACING;
pub use crate::form::ID_DIGITS;

/// A value as one line of a cell: an id short, everything else as the
/// editor would start it ([`cell_text`]), with its line breaks laid flat.
pub fn shown(v: &Value) -> String {
    match v {
        Value::Id(id) => ark::hash::hex(id)[..ID_DIGITS].to_string(),
        other => cell_text(other).chars().map(|c| if c.is_control() { ' ' } else { c }).collect(),
    }
}

/// A column's width in characters: its header's and its widest value's,
/// within [`MIN_CHARS`] and [`MAX_CHARS`].
pub fn width_of<'v>(header: &str, values: impl IntoIterator<Item = &'v Value>) -> usize {
    values
        .into_iter()
        .take(SAMPLE)
        .map(|v| shown(v).chars().count())
        .fold(header.chars().count(), usize::max)
        .clamp(MIN_CHARS, MAX_CHARS)
}

/// What of a value fits in `chars`: the text itself, or its start and its
/// end around an ellipsis.
pub fn clip(text: &str, chars: usize) -> String {
    arkui::fit::middle(text, chars)
}

/// A column of `chars` characters, in pixels, with its gap.
pub fn px(chars: usize) -> f32 {
    chars as f32 * CHAR_PX + GAP_PX
}

/// Rows of values under headings, each column with its width: a table's
/// page of rows, or a query's answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    /// The columns' names, as the detail line says them.
    pub columns: Vec<String>,
    /// What the headings say: a name, or a name and that it is the key.
    pub headings: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    /// Per column, in characters.
    pub widths: Vec<usize>,
}

impl Grid {
    /// Rows under `columns`, each its columns' values in order, measured
    /// over `sample` (a table measures over more rows than the page it
    /// draws, so a column does not change width from one page to the next).
    /// `labels` are what the headings say, when they say more than the
    /// column's name.
    pub fn of_rows(columns: &[String], labels: &[String], rows: &[Vec<Value>], sample: &[Vec<Value>]) -> Grid {
        let widths = labels
            .iter()
            .enumerate()
            .map(|(c, l)| width_of(l, sample.iter().filter_map(|r| r.get(c))))
            .collect();
        Grid {
            columns: columns.to_vec(),
            headings: labels.to_vec(),
            rows: rows
                .iter()
                .map(|r| (0..columns.len()).map(|c| r.get(c).cloned().unwrap_or(Value::Null)).collect())
                .collect(),
            widths,
        }
    }

    /// A query's answer as a table: a list of records is a row each under
    /// the record's fields, as the query's type names them; anything else is
    /// one column, `value`, a row per element.
    pub fn of_value(ty: &Ty, v: &Value) -> Grid {
        let items: Vec<Value> = match v {
            Value::List(xs) => xs.to_vec(),
            other => vec![other.clone()],
        };
        let (columns, rows): (Vec<String>, Vec<Vec<Value>>) = match ty {
            Ty::Struct(fs) => (
                fs.iter().map(|(n, _)| n.clone()).collect(),
                items
                    .into_iter()
                    .map(|x| match x {
                        Value::Struct(vs) => vs,
                        other => vec![other],
                    })
                    .collect(),
            ),
            _ => (vec!["value".to_string()], items.into_iter().map(|x| vec![x]).collect()),
        };
        let widths = columns
            .iter()
            .enumerate()
            .map(|(j, c)| width_of(c, rows.iter().filter_map(|r| r.get(j))))
            .collect();
        Grid {
            headings: columns.clone(),
            columns,
            rows,
            widths,
        }
    }

    /// The heading of column `c`, cut to its width.
    pub fn heading(&self, c: usize) -> String {
        arkui::fit::tail(&self.headings[c], self.widths[c])
    }

    /// The cell at row `r`, column `c`, as drawn: cut to its column.
    pub fn cell(&self, r: usize, c: usize) -> String {
        self.rows
            .get(r)
            .and_then(|row| row.get(c))
            .map_or(String::new(), |v| clip(&shown(v), self.widths[c]))
    }

    /// The whole of the cell at row `r`, column `c`, named: what the detail
    /// line says, an id in full.
    pub fn detail(&self, r: usize, c: usize) -> Option<String> {
        let v = self.rows.get(r)?.get(c)?;
        Some(format!("{}: {}", self.columns[c], cell_text(v)))
    }

    /// The whole width, every column and its gap, in pixels.
    pub fn width_px(&self) -> f32 {
        self.widths.iter().map(|w| px(*w)).sum()
    }
}
