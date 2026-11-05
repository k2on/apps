//! Keeping a keyboard cursor on screen, as vim keeps its cursor line: the
//! list does not move while the cursor stays in view, and when the cursor
//! would leave it the list moves only as far as keeps it in, a margin of
//! [`SCROLLOFF`] rows short of the edge (vim's `scrolloff`). `zz`, `zt` and
//! `zb` put the cursor's row in the middle, at the top or at the foot
//! ([`Place`]).
//!
//! Where a row is is worked out from the scrollable's own geometry, read by
//! an operation when the task runs ([`follow`]): its height, how tall what
//! it scrolls is, and how far it is scrolled. Rows are taken to be one
//! height, so row `n` of `rows` starts `n / rows` of the way down: close
//! enough to keep it in view without the widgets reporting where each row
//! was drawn.
use iced::advanced::widget::operation::{Operation, Scrollable};
use iced::advanced::widget::Id;
use iced::widget::operation::{snap_to, RelativeOffset};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Rectangle, Task, Vector};

use crate::vim::Grid;

/// How many rows short of a scrollable's edge the cursor's row is kept.
pub const SCROLLOFF: f32 = 2.0;

/// Where the cursor's row is to be in its scrollable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Wherever it is, while it is in view: the list moves only when it
    /// would leave, and only as far as keeps it [`SCROLLOFF`] rows in.
    Near,
    /// `zz`: in the middle.
    Middle,
    /// `zt`: at the top, [`SCROLLOFF`] rows down.
    Top,
    /// `zb`: at the foot, [`SCROLLOFF`] rows up.
    Bottom,
}

/// Where a scrollable `viewport` tall over `content` scrolled to `offset`
/// is to be scrolled so row `row` of `rows` is where `place` says, or
/// `None` when it is to stay where it is.
pub fn offset_for(row: usize, rows: usize, viewport: f32, content: f32, offset: f32, place: Place) -> Option<f32> {
    if rows == 0 || content <= viewport {
        return None;
    }
    let height = content / rows as f32;
    let top = row.min(rows - 1) as f32 * height;
    let bottom = top + height;
    // The margin, but never so much that a row cannot be kept inside it.
    let margin = (SCROLLOFF * height).min(((viewport - height) / 2.0).max(0.0));
    let wanted = match place {
        Place::Near if top - margin < offset => top - margin,
        Place::Near if bottom + margin > offset + viewport => bottom + margin - viewport,
        Place::Near => return None,
        Place::Middle => top + height / 2.0 - viewport / 2.0,
        Place::Top => top - margin,
        Place::Bottom => bottom + margin - viewport,
    };
    let wanted = wanted.clamp(0.0, content - viewport);
    ((wanted - offset).abs() >= 0.5).then_some(wanted)
}

/// Scroll the scrollable `id` so row `row` of its `rows` is where `place`
/// says ([`offset_for`]), from where it is scrolled when the task runs.
pub fn follow<M: Send + 'static>(id: &'static str, row: usize, rows: usize, place: Place) -> Task<M> {
    struct Follow {
        target: Id,
        row: usize,
        rows: usize,
        place: Place,
    }
    impl<T> Operation<T> for Follow {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }
        fn scrollable(&mut self, id: Option<&Id>, bounds: Rectangle, content: Rectangle, translation: Vector, state: &mut dyn Scrollable) {
            if id != Some(&self.target) {
                return;
            }
            if let Some(y) = offset_for(self.row, self.rows, bounds.height, content.height, translation.y, self.place) {
                state.scroll_to(AbsoluteOffset { x: None, y: Some(y) });
            }
        }
    }
    iced::advanced::widget::operate(Follow {
        target: Id::new(id),
        row,
        rows,
        place,
    })
}

/// Scroll the scrollable `id` so cell `at` of `grid` is where `place` says:
/// its row of the grid's rows ([`follow`]).
pub fn reveal<M: Send + 'static>(id: &'static str, grid: Grid, at: usize, place: Place) -> Task<M> {
    let columns = grid.columns.max(1);
    follow(id, at / columns, grid.cells.div_ceil(columns), place)
}

/// Scroll the scrollable `id` to `y` of the way down, 0 to 1: where a
/// [`crate::vim::Page`] says its cursor sits.
pub fn reveal_to<M: Send + 'static>(id: &'static str, y: f32) -> Task<M> {
    snap_to(id, RelativeOffset { x: Some(0.0), y: Some(y) })
}

/// Scroll the scrollable `id` so row `at` of `rows` is in the middle of
/// it, where `seen` rows fit: what a list that moves by itself — a
/// transcript's line being said — keeps its row at, with as much to come
/// in sight as gone.
///
/// Uniform rows, as [`reveal`] has them: the offset that puts row `at`'s
/// middle at the middle of the view is its distance from there over how
/// far the list scrolls, and a list that fits does not scroll.
pub fn centre<M: Send + 'static>(id: &'static str, at: usize, rows: usize, seen: f32) -> Task<M> {
    reveal_to(id, centred(at, rows, seen))
}

/// Where [`centre`] scrolls to, 0 to 1.
pub fn centred(at: usize, rows: usize, seen: f32) -> f32 {
    let travel = rows as f32 - seen;
    match travel > 0.0 {
        true => ((at as f32 + 0.5 - seen / 2.0) / travel).clamp(0.0, 1.0),
        false => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Down a list of a hundred rows of 20 px in a 200 px view: the list
    /// stays put while the cursor is in view, and moves a row at a time once
    /// it is two rows from the foot; `zz`, `zt` and `zb` put it in the
    /// middle, the top and the foot. Falsified by scrolling to the row's
    /// share of the way down on every move, as `reveal` once did: row 3
    /// moved the list.
    #[test]
    fn the_list_moves_only_when_the_cursor_would_leave_it() {
        let at = |row, offset, place| offset_for(row, 100, 200.0, 2000.0, offset, place);
        assert_eq!(at(3, 0.0, Place::Near), None, "row 3 is in view");
        assert_eq!(at(7, 0.0, Place::Near), None, "row 7 is two rows from the foot");
        assert_eq!(at(8, 0.0, Place::Near), Some(20.0), "row 8 moves the list one row");
        assert_eq!(at(8, 20.0, Place::Near), None, "…and then it is in view");
        assert_eq!(at(3, 20.0, Place::Near), None, "two rows from the top, on the way back");
        assert_eq!(at(2, 20.0, Place::Near), Some(0.0), "row 2 brings the top back");
        assert_eq!(at(50, 0.0, Place::Middle), Some(910.0), "zz: row 50's middle is the view's");
        assert_eq!(at(50, 0.0, Place::Top), Some(960.0), "zt: two rows above it");
        assert_eq!(at(50, 0.0, Place::Bottom), Some(860.0), "zb: two rows below it");
        assert_eq!(at(0, 500.0, Place::Middle), Some(0.0), "the top stays the top");
        assert_eq!(offset_for(3, 5, 200.0, 100.0, 0.0, Place::Middle), None, "a list that fits does not move");
    }

    /// A row is put in the middle wherever there is room to, and the ends
    /// stay at the ends. Falsified by leaving out the half row: the tenth
    /// of twenty, ten seen, was not at the middle.
    #[test]
    fn a_row_is_centred_where_the_list_can_scroll() {
        assert_eq!(centred(0, 20, 10.0), 0.0, "the first stays at the top");
        assert_eq!(centred(19, 20, 10.0), 1.0, "the last at the foot");
        assert_eq!(centred(9, 20, 10.0), 0.45, "rows 4.5 to 14.5 in view: 9.5 its middle");
        assert_eq!(centred(5, 8, 10.0), 0.0, "a list that fits does not scroll");
    }
}
