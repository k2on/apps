//! harken's colours: the shared neutrals and the gold, ui/theme.toml's
//! `accent.gold`, as the one value [`arkui::theme::install`] takes.
//! Everything else asks [`arkui::theme::of`] — nothing in this crate writes
//! a colour down, which is the whole reason dark mode works: iced picks Light
//! or Dark from the system and hands every style closure the one it picked,
//! and `of` answers with ours.
//!
//! The gold is not one colour. A dark page can take a bright leaf gold; a
//! white one cannot, because text in the gold has to be legible *on* it.
//! The squares are the gold ones, the phone's six.
use arkui::theme::Palette;

/// harken's, for [`arkui::theme::install`] — once, before the first frame.
pub const HARKEN: Palette = arkui::tokens::GOLD;
