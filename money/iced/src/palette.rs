//! money's colours: the shared neutrals and the green, ui/theme.toml's
//! `accent.green`.
//!
//! Everything else asks [`arkui::theme::of`], as harken's window does, so
//! the system's light or dark is followed without a colour written down
//! anywhere but there.
use arkui::theme::Palette;

/// money's, for [`arkui::theme::install`] — once, before the first frame.
pub const MONEY: Palette = arkui::tokens::GREEN;
