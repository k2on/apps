//! Keeping a keyboard cursor on screen.
use iced::widget::operation::{snap_to, RelativeOffset};
use iced::Task;

use crate::vim::Grid;

/// Scroll the scrollable `id` so cell `at` of `grid` is in view.
///
/// A relative offset rather than a measured one: rows are a uniform height, so
/// where a cell sits down the scrollable is its row over the rows, and that is
/// close enough to keep it in view without the widget reporting its geometry
/// back. The shape answers ([`Grid::progress`]), because only it knows whether
/// its cells are stacked one per row or six.
pub fn reveal<M: Send + 'static>(id: &'static str, grid: Grid, at: usize) -> Task<M> {
    snap_to(
        id,
        RelativeOffset {
            x: Some(0.0),
            y: Some(grid.progress(at)),
        },
    )
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
