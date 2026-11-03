//! Faces every app carries rather than asks the system for. A browser has
//! no fonts to ask, and a desktop's `monospace` may be one iced cannot draw
//! (a line in it came out as nothing, but for the characters it lacked).

use iced::Font;

/// Fira Mono, Regular: Fira Sans's monospace, as tables and receipts read.
/// Load it with the app (`iced::application(..).font(FIRA_MONO)`).
pub const FIRA_MONO: &[u8] = include_bytes!("../fonts/FiraMono-Regular.ttf");

/// The face [`FIRA_MONO`] loads, by its name.
pub const MONO: Font = Font::with_name("Fira Mono");
