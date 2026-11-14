//! Text, as every ArkUI window sets it: iced's `text` module and its `text`
//! widget, a line of it [`LINE_HEIGHT`] times its size.
//!
//! The line height is the theme's (`ui/theme.toml` `[layout]`): upstream
//! iced 0.14's 1.3, which every row, panel and keyboard step here was
//! measured at. COSMIC's fork defaults to 1.4, which made every line of
//! every window a tenth taller; text made here keeps ArkUI's measure.
//!
//! A file takes both names at once, `use arkui::text::{self, text};`, in
//! place of iced's: `text("…")` is the widget and `text::Style`,
//! `text::Wrapping` and the rest are iced's own.
pub use iced::widget::text::*;

use crate::layout_tokens;

/// The line height every ArkUI window sets its text at.
pub const LINE_HEIGHT: LineHeight = LineHeight::Relative(layout_tokens::LINE_HEIGHT);

/// iced's `text`, a line of it [`LINE_HEIGHT`] tall.
pub fn text<'a, Theme, Renderer>(content: impl IntoFragment<'a>) -> Text<'a, Theme, Renderer>
where
    Theme: Catalog + 'a,
    Renderer: iced::advanced::text::Renderer,
{
    Text::new(content).line_height(LINE_HEIGHT)
}
