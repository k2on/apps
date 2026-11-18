//! The first cut of a renderer for Pebble's layers: a fixed screen the C
//! shim lays out (`c/ark_pebble.c`), of text slots, a progress bar, a list
//! of rows and an action bar of glyphs, which an app fills through a
//! [`Screen`]. The screen keeps what it last showed and tells the shim
//! only what changed, so only those layers are marked dirty and redrawn.
//!
//! This is not the arkui backend: that comes later, writing each
//! primitive (text, a stack, a glyph) and native control (the list, the
//! action bar) as Pebble's layers from an app's `(glance)` views. The
//! README says how these map onto them.

use alloc::vec::Vec;

use crate::sys;

/// A text slot of the screen, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// A small line at the top: the link, what is pending.
    Status = 0,
    Title = 1,
    Subtitle = 2,
    /// Under the progress bar: where it is.
    Time = 3,
    /// Above the list.
    Header = 4,
}

const SLOTS: usize = 5;

/// A button, as PebbleOS numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Back = 0,
    Up = 1,
    Select = 2,
    Down = 3,
}

impl Button {
    pub fn from(id: u8) -> Option<Button> {
        [Button::Back, Button::Up, Button::Select, Button::Down].get(usize::from(id)).copied()
    }
}

/// A glyph of the action bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    None = 0,
    Play = 1,
    Pause = 2,
    Skip = 3,
}

/// The most rows the list shows.
pub const ROWS: usize = 4;

/// What the screen shows, kept so that only changes reach the shim.
#[derive(Default)]
pub struct Screen {
    texts: [Option<Vec<u8>>; SLOTS],
    rows: Option<Vec<(Vec<u8>, Vec<u8>)>>,
    glyphs: [Option<u8>; 4],
    progress: Option<u16>,
}

impl Screen {
    pub fn new() -> Screen {
        Screen::default()
    }

    pub fn text(&mut self, slot: Slot, s: &[u8]) {
        let held = &mut self.texts[slot as usize];
        if held.as_deref() != Some(s) {
            sys::ui_text(slot as u8, s);
            *held = Some(s.to_vec());
        }
    }

    /// The list's rows, each a title and a subtitle; at most [`ROWS`].
    pub fn rows(&mut self, rows: Vec<(Vec<u8>, Vec<u8>)>) {
        if self.rows.as_ref() == Some(&rows) {
            return;
        }
        for (i, (t, s)) in rows.iter().take(ROWS).enumerate() {
            let same = self.rows.as_ref().and_then(|old| old.get(i)) == Some(&(t.clone(), s.clone()));
            if !same {
                sys::ui_row(i as u8, t, s);
            }
        }
        sys::ui_rows(rows.len().min(ROWS) as u8);
        self.rows = Some(rows);
    }

    pub fn action(&mut self, button: Button, glyph: Glyph) {
        let held = &mut self.glyphs[button as usize];
        if *held != Some(glyph as u8) {
            sys::ui_action(button as u8, glyph as u8);
            *held = Some(glyph as u8);
        }
    }

    /// The progress bar, in thousandths.
    pub fn progress(&mut self, permille: u16) {
        let permille = permille.min(1000);
        if self.progress != Some(permille) {
            sys::ui_progress(permille);
            self.progress = Some(permille);
        }
    }
}
