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
