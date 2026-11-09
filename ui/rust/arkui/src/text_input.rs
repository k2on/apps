//! A text box, as every ArkUI window sets one: iced's `text_input` module
//! and its widget, its line [`crate::text::LINE_HEIGHT`] times its size, as
//! [`crate::text`] sets text (the fork's default is taller).
//!
//! A file takes both names at once, `use arkui::text_input::{self,
//! text_input};`, in place of iced's.
pub use iced::widget::text_input::*;

/// iced's `text_input`, its line as tall as ArkUI's text.
pub fn text_input<'a, Message, Theme, Renderer>(placeholder: &str, value: &str) -> TextInput<'a, Message, Theme, Renderer>
where
    Message: Clone,
    Theme: Catalog + 'a,
    Renderer: iced::advanced::text::Renderer,
{
    TextInput::new(placeholder, value).line_height(crate::text::LINE_HEIGHT)
}
