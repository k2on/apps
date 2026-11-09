//! The desktop's half of the shell every Ark app is made of
//! (ui/docs/design.md): which size class a window is in, which of its
//! panes has the keyboard, and the keyboard every app gets.
//!
//! - **[`SizeClass`]**: the one decision of how wide a window is, from
//!   `ui/theme.toml`'s `[layout]` (`layout_tokens`), as SwiftUI's
//!   horizontal size class and Compose's window size class make it. The
//!   parts that adapt ask it (`crate::view`'s shell); an app never does.
//! - **[`Pane`]** and **[`Frame`]**: the window as the shell draws it —
//!   its width and which pane has the keys, lit as a Mac's list is.
//! - **[`Keyboard`]**: the vim layer (`crate::vim`), the keys panel
//!   (`crate::which`) and the panes, for every app: `j`/`k`, `gg`/`G`,
//!   `zz`/`zt`/`zb`, `h`/`l` and `Tab` between the panes, `?`, `Esc`,
//!   `^O`/`^I`. What is left is the pane's own: an app is handed the
//!   [`Routed`] action for the pane that has the keys.
use iced::keyboard::{key::Named, Key, Modifiers};

use crate::layout_tokens::{COMPACT_WIDTH, EXPANDED_WIDTH};
use crate::scroll::Place;
use crate::vim::{self, Action, Motion};
use crate::which::Which;

/// How wide a window is, as the parts that adapt read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeClass {
    /// A phone upright, a narrow window: tabs at the foot, one column at a
    /// time, the inspector a sheet.
    Compact,
    /// A sidebar and one column; what a list opens is pushed.
    Medium,
    /// A sidebar, a list and what it opens, side by side.
    Expanded,
}

impl SizeClass {
    /// The class of a window `width` logical pixels wide.
    pub fn of(width: f32) -> SizeClass {
        if width < COMPACT_WIDTH {
            SizeClass::Compact
        } else if width < EXPANDED_WIDTH {
            SizeClass::Medium
        } else {
            SizeClass::Expanded
        }
    }
}

/// A pane of the window, in the order `Tab` and `l` walk them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    Sidebar,
    #[default]
    Content,
    Detail,
    Inspector,
}

/// The window as the shell draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub width: f32,
    /// The pane with the keys: its rows lit, its selection the accent.
    pub focus: Pane,
}

impl Frame {
    pub fn class(&self) -> SizeClass {
        SizeClass::of(self.width)
    }

    /// The panes that are on screen, left to right, for a window that has
    /// a detail and an inspector showing as said.
    pub fn panes(&self, detail: bool, inspector: bool) -> Vec<Pane> {
        let mut out = vec![];
        match self.class() {
            SizeClass::Compact => {
                out.push(if detail { Pane::Detail } else { Pane::Content });
            }
            SizeClass::Medium => {
                out.push(Pane::Sidebar);
                out.push(if detail { Pane::Detail } else { Pane::Content });
                if inspector {
                    out.push(Pane::Inspector);
                }
            }
            SizeClass::Expanded => {
                out.push(Pane::Sidebar);
                out.push(Pane::Content);
                if detail {
                    out.push(Pane::Detail);
                }
                if inspector {
                    out.push(Pane::Inspector);
                }
            }
        }
        out
    }
}

/// What a key came to, for the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Routed {
    /// Nothing more to do: a prefix half typed, the panel asked for, the
    /// keys moved to another pane.
    Done,
    /// `Esc`: whatever is over the window, closed.
    Escape,
    /// `^O`/`^I`: back and forward through where the window has been.
    Jump(isize),
    /// The sidebar's cursor moved to row `n`: show that section.
    Section(usize),
    /// `zz`, `zt`, `zb`: the focused list scrolled so its cursor sits there.
    Scroll(Place),
    /// The pane's own: `j`/`k`, `gg`/`G`, `Enter`, `/`, a bound sequence.
    Pane(Pane, Action),
}

/// The keyboard every Ark app gets on the desktop: the vim layer, the keys
/// panel, and which pane has the keys.
#[derive(Debug)]
pub struct Keyboard {
    pub keys: vim::Keys,
    pub which: Which,
    pub focus: Pane,
}

impl Default for Keyboard {
    fn default() -> Self {
        Keyboard {
            keys: vim::Keys::default(),
            which: Which::new(),
            focus: Pane::Content,
        }
    }
}

impl Keyboard {
    /// A key pressed with `mods`, while `panes` are on screen and the
    /// sidebar's cursor is on `section` of `sections`. `bindings` are the
    /// focused pane's own sequences (`cc`, `rg`).
    pub fn press(&mut self, key: &Key, mods: Modifiers, panes: &[Pane], section: usize, sections: usize, bindings: &[&'static str]) -> Routed {
        if !panes.contains(&self.focus) {
            self.focus = panes.iter().copied().find(|p| *p != Pane::Sidebar).unwrap_or(Pane::Content);
        }
        if *key == Key::Named(Named::Escape) {
            self.keys.press(key, mods);
            self.which.dismiss();
            return Routed::Escape;
        }
        self.keys.set_bindings(match self.focus {
            Pane::Sidebar => &[],
            _ => bindings,
        });
        let action = self.keys.press(key, mods);
        let prefix = match self.keys.mode() {
            vim::Mode::Normal => self.keys.pending(),
            vim::Mode::Search(_) => String::new(),
        };
        self.which.typed(&prefix);
        let Some(action) = action else { return Routed::Done };
        match action {
            Action::Key('?') => {
                self.which.ask();
                Routed::Done
            }
            Action::Jump(by) => Routed::Jump(by),
            Action::Scroll(place) => Routed::Scroll(place),
            Action::Cycle => {
                self.step(panes, if mods.shift() { -1 } else { 1 });
                Routed::Done
            }
            Action::Move(Motion::Left(_)) if self.focus != Pane::Sidebar && panes.first() != Some(&self.focus) => {
                self.step(panes, -1);
                Routed::Done
            }
            Action::Move(Motion::Right(_)) | Action::Activate if self.focus == Pane::Sidebar => {
                self.step(panes, 1);
                Routed::Done
            }
            Action::Move(m) if self.focus == Pane::Sidebar => match vim::Grid::column(sections).step(section, m) {
                Some(i) => Routed::Section(i),
                None => Routed::Done,
            },
            a => Routed::Pane(self.focus, a),
        }
    }

    /// The keys to the next pane on screen, `by` that way, round the ends.
    fn step(&mut self, panes: &[Pane], by: isize) {
        if panes.is_empty() {
            return;
        }
        let at = panes.iter().position(|p| *p == self.focus).unwrap_or(0) as isize;
        let n = panes.len() as isize;
        self.focus = panes[((at + by).rem_euclid(n)) as usize];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(k: &mut Keyboard, c: &str, panes: &[Pane]) -> Routed {
        k.press(&Key::Character(c.into()), Modifiers::default(), panes, 1, 4, &[])
    }

    /// The size classes are the theme's widths, and nothing else: a phone
    /// is compact, a small window has a sidebar and one column, a Mac's
    /// has all three. Falsified by deciding on any other width.
    #[test]
    fn the_size_class_is_the_themes() {
        assert_eq!(SizeClass::of(390.0), SizeClass::Compact);
        assert_eq!(SizeClass::of(COMPACT_WIDTH), SizeClass::Medium);
        assert_eq!(SizeClass::of(EXPANDED_WIDTH - 1.0), SizeClass::Medium);
        assert_eq!(SizeClass::of(1440.0), SizeClass::Expanded);
        let wide = Frame {
            width: 1440.0,
            focus: Pane::Content,
        };
        assert_eq!(wide.panes(true, true), vec![Pane::Sidebar, Pane::Content, Pane::Detail, Pane::Inspector]);
        let phone = Frame { width: 390.0, ..wide };
        assert_eq!(phone.panes(true, true), vec![Pane::Detail], "one at a time; the inspector a sheet");
    }

    /// `h` and `l` walk the panes on screen, `j`/`k` on the sidebar walk
    /// the sections, and what is left is the focused pane's. Falsified by
    /// handing `j` to the content while the sidebar has the keys.
    #[test]
    fn the_keys_walk_the_panes_then_the_pane() {
        let panes = [Pane::Sidebar, Pane::Content, Pane::Detail];
        let mut k = Keyboard::default();
        assert_eq!(press(&mut k, "j", &panes), Routed::Pane(Pane::Content, Action::Move(Motion::Down(1))));
        assert_eq!(press(&mut k, "h", &panes), Routed::Done);
        assert_eq!(k.focus, Pane::Sidebar);
        assert_eq!(press(&mut k, "j", &panes), Routed::Section(2));
        assert_eq!(press(&mut k, "l", &panes), Routed::Done);
        assert_eq!(press(&mut k, "l", &panes), Routed::Pane(Pane::Content, Action::Move(Motion::Right(1))));
        let tab = k.press(&Key::Named(Named::Tab), Modifiers::default(), &panes, 1, 4, &[]);
        assert_eq!((tab, k.focus), (Routed::Done, Pane::Detail));
        assert_eq!(press(&mut k, "h", &panes), Routed::Done);
        assert_eq!(k.focus, Pane::Content);
    }
}
