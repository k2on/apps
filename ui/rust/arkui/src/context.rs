//! A menu and the picker that is its submenu — or a picker on its own — and
//! the rules for which of them has the keyboard.
//!
//! **A menu and its submenu are one thing.** The submenu belongs to an entry of
//! its parent, both are up at once, and you asked one question. So `<Esc>`
//! closes both, the menu's backdrop closes both, and the cursor leaving the
//! entry closes the submenu. A picker opened on its own has no parent and is
//! only itself.
//!
//! **"Has the keyboard" is one question with one answer**, [`Context::focus`]:
//! the picker if it has the keys, else the menu if one is up, else neither —
//! the page. Three places each deciding it for themselves once drew two
//! accent-filled rows on screen, the row `j` used to move and the entry `j`
//! actually moves.
//!
//! **Moving is the same everywhere.** A menu and a picker are both a
//! [`vim::Grid::column`], which refuses `h` and `l`; [`Context::travel`] steps
//! the grid and, on a refusal, says what is on the other side of that edge —
//! the submenu to the right of the entry that owns it, the parent entry to the
//! left of a submenu. None of that is in `vim`: a shape knows it has an edge,
//! and only this knows what is beyond one.
//!
//! The app keeps one of these, draws [`crate::menu::view`] and
//! [`crate::picker::view`] from it with [`crate::layer`], and routes its
//! messages here. What an entry *does* stays the app's: running one hands the
//! app its message.
use iced::Size;

use crate::menu::{self, Menu};
use crate::picker::{self, Picked, Picker};
use crate::vim::Motion;

/// Which layer has the keyboard, when one does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Menu,
    Picker,
}

/// A menu, its submenu, or a picker alone.
#[derive(Debug, Clone)]
pub struct Context<M, T> {
    pub menu: Option<Menu<M>>,
    pub picker: Option<Picker<T>>,
    /// The dwell asked for the submenu, so the next picker opened beside the
    /// menu is *shown* rather than entered. Cleared by anything that moves.
    showing: bool,
}

impl<M, T> Default for Context<M, T> {
    fn default() -> Self {
        Context {
            menu: None,
            picker: None,
            showing: false,
        }
    }
}

impl<M: Clone, T: Clone> Context<M, T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Which layer has the keyboard: the one on top that wants it. `None` is
    /// the page behind — which is also the whole of the rule that the page
    /// behind a context window does not follow the pointer: an app refuses a
    /// hover on its rows while this is `Some`, because a backdrop can swallow
    /// a click and a wheel but not a hover.
    pub fn focus(&self) -> Option<Layer> {
        if self.picker.as_ref().is_some_and(|p| p.keys) {
            return Some(Layer::Picker);
        }
        if self.menu.is_some() {
            return Some(Layer::Menu);
        }
        None
    }

    /// Whether anything is up.
    pub fn is_open(&self) -> bool {
        self.menu.is_some() || self.picker.is_some()
    }

    /// Put a menu up, in place of whatever was.
    pub fn open_menu(&mut self, menu: Menu<M>) {
        self.menu = Some(menu);
        self.picker = None;
        self.showing = false;
    }

    /// Close the menu — and its submenu with it, which has no life of its own:
    /// it belongs to the entry it hangs off, and now there is no entry. This is
    /// what lets the menu's backdrop close both, and why a submenu needs no
    /// backdrop of its own. A picker standing alone is left alone.
    pub fn close_menu(&mut self) {
        self.menu = None;
        if self.picker.as_ref().is_some_and(|p| p.origin.is_some()) {
            self.picker = None;
        }
        self.showing = false;
    }

    /// Close the picker, and only it. The menu, if any, has the keys again.
    pub fn close_picker(&mut self) {
        self.picker = None;
        self.showing = false;
    }

    /// Open a picker. With a menu up it is that menu's submenu, pinned beside
    /// the entry the cursor is on; alone, it is centred.
    ///
    /// It takes the keyboard — unless the dwell is what asked for it, in which
    /// case it is shown and the keys stay on the parent entry (see
    /// [`Context::tick`]). That is the only difference between resting on an
    /// entry and pressing `<Enter>` on it.
    pub fn open_picker(&mut self, mut picker: Picker<T>, window: Size) {
        picker.origin = self
            .menu
            .as_ref()
            .map(|m| m.submenu_origin(picker.rows(), picker.width, picker::MAX_HEIGHT, window));
        picker.keys = !std::mem::take(&mut self.showing);
        self.picker = Some(picker);
    }

    /// Put the menu's cursor on an entry, however it got there — a hover and a
    /// `j` have to mean the same thing, and everything the dwell does keys off
    /// *moving*.
    ///
    /// Moving off an entry closes the submenu it owned: a submenu belongs to
    /// its parent entry, so the cursor leaving is the submenu going — which is
    /// what makes pointing at the next entry mean the next entry.
    ///
    /// Landing on the entry it is already on is not nothing: the pointer may
    /// have come *back* out of the submenu, and the highlight belongs to
    /// whatever the pointer is over, so the keys come back to the parent. The
    /// submenu stays up — you pointed at the entry that owns it, not away.
    pub fn land(&mut self, at: usize) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let at = at.min(menu.entries.len().saturating_sub(1));
        self.showing = false;
        if menu.at == at {
            if let Some(picker) = &mut self.picker {
                picker.keys = false;
            }
            return;
        }
        menu.at = at;
        menu.dwell = 0;
        menu.offered = false;
        self.picker = None;
    }

    /// Put the picker's cursor on a row. Reaching a row of a submenu *is*
    /// entering it: the pointer has left the entry it hangs off.
    pub fn picker_at(&mut self, at: usize) {
        if let Some(picker) = &mut self.picker {
            picker.land(at);
            picker.keys = true;
        }
    }

    /// Whether [`Context::tick`] can still answer something: a menu is up,
    /// its cursor rests on an entry that owns a submenu, and that submenu has
    /// not been offered for this rest. The only time a window needs the
    /// dwell's clock (`docs/plan-reactive.md`); otherwise it runs none.
    pub fn dwelling(&self) -> bool {
        let Some(menu) = &self.menu else { return false };
        self.picker.is_none() && !menu.offered && menu.entries.get(menu.at).is_some_and(|e| e.opens_submenu())
    }

    /// One tick of the app's clock (50ms). When the cursor has rested on an
    /// entry that owns a submenu for [`menu::DWELL`] ticks, answers that
    /// entry's message: run it the way any message is run, and the picker it
    /// opens with [`Context::open_picker`] is *shown*, not entered.
    ///
    /// Offered once per rest: `<Esc>` out of the submenu leaves the cursor on
    /// its entry, and without that the next tick would open it again.
    pub fn tick(&mut self) -> Option<M> {
        let menu = self.menu.as_mut()?;
        if self.picker.is_some() || menu.offered {
            return None;
        }
        let entry = menu.entries.get(menu.at).filter(|e| e.opens_submenu())?;
        let message = entry.message().clone();
        menu.dwell = menu.dwell.saturating_add(1);
        if menu.dwell < menu::DWELL {
            return None;
        }
        menu.offered = true;
        self.showing = true;
        Some(message)
    }

    /// What `<Enter>` on the menu runs: the message of the entry the cursor is
    /// on. An entry that owns a submenu answers the message that opens it, and
    /// a picker opened now takes the keys — `<Enter>` enters.
    pub fn chosen(&mut self) -> Option<M> {
        self.showing = false;
        Some(self.menu.as_ref()?.current()?.message().clone())
    }

    /// `<Enter>` on the picker.
    pub fn activate_picker(&mut self) -> Option<Picked<T>> {
        self.picker.as_mut()?.activate()
    }

    /// `<Esc>`: the menu and its submenu together, from either side of the
    /// keyboard — or, with no menu, the picker alone.
    ///
    /// It closed the innermost once, which is what a *stack* of menus does,
    /// and this is not one: dismissing one question twice is the same
    /// complaint as a menu that stays up after it has been answered. The click
    /// already works this way — a submenu has no backdrop, so the menu's closes
    /// both — and this is the keyboard agreeing with the pointer.
    pub fn cancel(&mut self) {
        match self.menu.is_some() {
            true => self.close_menu(),
            false => self.close_picker(),
        }
    }

    /// Move the cursor of whichever layer has the keyboard.
    ///
    /// A refused step means "not mine", and this answers what is beyond each
    /// edge: `l` on the entry that owns a submenu steps into it — taking the
    /// keys if the pointer has already shown it, and otherwise answering the
    /// entry's message for the app to run, which opens it entered — and `h`
    /// in a submenu hands the keys back to the parent entry, leaving the panel
    /// up, which is exactly what pointing back at that entry does. A picker
    /// with no parent has nowhere for `h` to go.
    pub fn travel(&mut self, motion: Motion) -> Option<M> {
        match self.focus()? {
            Layer::Menu => {
                let menu = self.menu.as_ref()?;
                match menu.grid().step(menu.at, motion) {
                    Some(at) => {
                        self.land(at);
                        None
                    }
                    None if matches!(motion, Motion::Right(_)) => self.enter_submenu(),
                    None => None,
                }
            }
            Layer::Picker => {
                let picker = self.picker.as_mut()?;
                match picker.grid().step(picker.at, motion) {
                    Some(at) => picker.land(at),
                    None if matches!(motion, Motion::Left(_)) && self.menu.is_some() => picker.keys = false,
                    None => {}
                }
                None
            }
        }
    }

    /// Step into the submenu the menu's cursor is on, if that entry owns one.
    fn enter_submenu(&mut self) -> Option<M> {
        let entry = self.menu.as_ref()?.current().filter(|e| e.opens_submenu())?;
        // Already up because the pointer rested here: step in rather than
        // opening it again, which would rebuild it and put the cursor back on
        // the first row of a panel you are already looking at.
        if let Some(picker) = &mut self.picker {
            picker.keys = true;
            return None;
        }
        let message = entry.message().clone();
        self.showing = false;
        Some(message)
    }
}
