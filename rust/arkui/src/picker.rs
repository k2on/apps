//! A picker: ticked rows, and a last row that makes another.
//!
//! A list rather than a menu, because however many choices exist is however
//! many rows this has, and a menu that scrolls is a list pretending not to be
//! one. It is a **toggle**: a list with nothing marked is a list you can put
//! the same thing on twice and never take it off, so every row says which it
//! is.
//!
//! The row that makes a new one is a *row* and not a key: one list, one
//! cursor, and `j` walks onto it like anything else. `<Enter>` there starts
//! naming, in a text box that takes its own keys — so none of them reach the
//! vim layer, which is what makes a modeless keymap safe beside a box you can
//! type into.
//!
//! One component, two places. Opened from a menu it is that menu's submenu,
//! pinned beside it with the parent still up, drawing no header of its own;
//! opened on its own it is centred over a dimmed page and says what it is
//! about itself. Two ways to one question should not be two panels to keep in
//! step. Which one it is is `origin`, and [`crate::context::Context`] is what
//! sets it.
use iced::widget::{column, container, mouse_area, rule, scrollable, text, text_input};
use iced::{Element, Length, Padding, Point};

use crate::fit::middle;
use crate::panel::{self, ENTRY_CHROME};
use crate::{glyphs, icon, menu, style, table, vim};

/// A picker is as wide as its longest name and no wider, within this range. A
/// fixed 340 beside three names was two thirds empty, which reads as a panel
/// that failed to fill rather than one sized to what is in it. The minimum is
/// what a `New …` row needs, so a panel holding one short name is not narrower
/// than its own last row.
pub const MIN_WIDTH: f32 = 180.0;
pub const MAX_WIDTH: f32 = 340.0;
/// Thirty rows scroll inside a panel of a known size rather than growing one
/// off the screen.
pub const MAX_HEIGHT: f32 = 420.0;

/// One row: what it stands for, what it is called, and whether it is ticked.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice<T> {
    pub value: T,
    pub name: String,
    pub on: bool,
}

/// A picker that is up.
#[derive(Debug, Clone)]
pub struct Picker<T> {
    /// What it is about. Drawn above the list when it stands alone; held
    /// rather than looked up, because what is underneath can change while it
    /// is open.
    pub title: String,
    pub choices: Vec<Choice<T>>,
    /// The label of the row that makes another (`New playlist…`), or `None`
    /// for a picker without one.
    pub new_row: Option<String>,
    /// Where the cursor is, over the choices and then the new row.
    pub at: usize,
    /// Where it is pinned when it is a submenu; `None` is centred.
    pub origin: Option<Point>,
    /// Whether the keyboard is in here.
    ///
    /// False for the one case that has no other way to say it: a submenu that
    /// opened because the pointer came to rest on its parent entry. It is
    /// *shown*, and the highlight stays on the entry it hangs off — the
    /// difference between a panel appearing beside what you are pointing at
    /// and the highlight jumping into a panel you have not reached yet. The
    /// pointer entering it, `l`, or `<Enter>` on the parent hands the keys
    /// over.
    pub keys: bool,
    /// What is being typed into the new-row box, while it is open.
    pub naming: Option<String>,
    /// How wide it is, worked out once from what is in it — stored because the
    /// view draws the panel and the submenu placement decides left or right
    /// from it, and one number cannot be two answers.
    pub width: f32,
}

/// What `<Enter>` on a picker did.
#[derive(Debug, Clone, PartialEq)]
pub enum Picked<T> {
    /// A row was ticked or unticked; `on` is what it is now. Marked in place
    /// rather than re-read: the answer is known, and a query per tap is a
    /// query per tap.
    Toggled { value: T, on: bool },
    /// The new row: a name is being asked for. Put the keyboard in the box
    /// ([`focus_naming`]) rather than make somebody reach for the mouse to
    /// finish what a key started.
    Naming,
}

impl<T: Clone> Picker<T> {
    /// A picker standing alone, centred, with the keyboard.
    pub fn new(title: impl Into<String>, choices: Vec<Choice<T>>, new_row: Option<String>) -> Self {
        let width = Self::width_for(&choices, new_row.as_deref());
        Picker {
            title: title.into(),
            choices,
            new_row,
            at: 0,
            origin: None,
            keys: true,
            naming: None,
            width,
        }
    }

    /// As wide as the longest name it holds, the new row's included, within
    /// [`MIN_WIDTH`]..=[`MAX_WIDTH`]. A name too long for the maximum gets an
    /// ellipsis through its middle rather than a wider panel.
    pub fn width_for(choices: &[Choice<T>], new_row: Option<&str>) -> f32 {
        panel::width_for(
            choices.iter().map(|c| (c.name.as_str(), 0.0)).chain(new_row.map(|n| (n, 0.0))),
            MIN_WIDTH,
            MAX_WIDTH,
        )
    }

    /// How many characters of a name the width it was built at affords. Read
    /// back from the answer, so the estimate is consulted once, not twice.
    pub fn label_chars(&self) -> usize {
        panel::chars(self.width, ENTRY_CHROME)
    }

    /// How many rows the cursor walks: the choices, and the new row.
    pub fn rows(&self) -> usize {
        self.choices.len() + usize::from(self.new_row.is_some())
    }

    /// Its shape for the keyboard: one column, so `h` is refused and a submenu
    /// can hand the keys back to its parent.
    pub fn grid(&self) -> vim::Grid {
        vim::Grid::column(self.rows())
    }

    /// Put the cursor on a row.
    pub fn land(&mut self, at: usize) {
        self.at = at.min(self.rows().saturating_sub(1));
    }

    /// How tall it comes out as a submenu, which is the only way anything
    /// places it — a centred panel's height is the layout's business.
    pub fn submenu_height(&self) -> f32 {
        menu::submenu_height(self.rows(), MAX_HEIGHT)
    }

    /// `<Enter>`: toggle the row, or start naming a new one.
    pub fn activate(&mut self) -> Option<Picked<T>> {
        match self.choices.get_mut(self.at) {
            Some(choice) => {
                choice.on = !choice.on;
                Some(Picked::Toggled {
                    value: choice.value.clone(),
                    on: choice.on,
                })
            }
            None if self.new_row.is_some() => {
                self.naming = Some(String::new());
                Some(Picked::Naming)
            }
            None => None,
        }
    }
}

/// What a picker's rows send.
pub struct Messages<'a, M> {
    /// The pointer reached row `i`, or it was clicked. Landing a cursor in a
    /// picker *is* entering it, however you got there.
    pub at: Box<dyn Fn(usize) -> M + 'a>,
    /// Run the row the cursor is on.
    pub activate: M,
    /// What is typed into the new-row box.
    pub name: Box<dyn Fn(String) -> M + 'a>,
    /// The name was submitted.
    pub create: M,
}

/// The new-row box's id, so opening it can put the keyboard in it.
pub const NAMING: &str = "arkui-picker-naming";

/// Put the keyboard in the new-row box.
pub fn focus_naming<M: Send + 'static>() -> iced::Task<M> {
    iced::widget::operation::focus(NAMING)
}

/// The picker, drawn.
///
/// **A submenu draws no header**, which is what `origin` decides: it is
/// `Some` exactly when this hangs off a menu, and that menu is still up with
/// its title across its own top. A title here would be the same sentence twice
/// one panel apart, and a hint under it three lines of chrome above a list of
/// three names. Opened on its own there is no parent to have said any of it,
/// so it says it itself. [`menu::SUBMENU_CHROME`] is this layout's height, and
/// has to move with it.
pub fn view<'a, M: Clone + 'a, T: Clone>(picker: &'a Picker<T>, focused: bool, msgs: Messages<'a, M>) -> Element<'a, M> {
    let budget = picker.label_chars();
    let rows = picker.choices.iter().enumerate().fold(column![].spacing(0), |col, (i, choice)| {
        let on_cursor = picker.at == i;
        let lit = on_cursor && focused;
        // The tick is the glyph column, drawn or not: a row that is not ticked
        // is still a row, and its name goes where every other name goes.
        let line = panel::entry(
            choice.on.then(|| Element::from(icon::tick(lit))),
            table::cell(middle(&choice.name, budget), Length::Fill, lit, choice.on, false),
            None,
        );
        col.push(
            mouse_area(line.style(move |theme| style::entry(theme, on_cursor, focused)))
                .on_enter((msgs.at)(i))
                .on_press((msgs.at)(i))
                .on_release(msgs.activate.clone()),
        )
    });

    let last = picker.choices.len();
    let rows = match (&picker.new_row, &picker.naming) {
        (_, Some(name)) => rows.push(
            text_input("a name for it", name)
                .id(NAMING)
                .on_input(msgs.name)
                .on_submit(msgs.create.clone())
                .size(13)
                .padding(Padding::from([4.0, 6.0])),
        ),
        (Some(label), None) => {
            let on_cursor = picker.at == last;
            let lit = on_cursor && focused;
            // The same shape every row has, so the words line up down the
            // panel — and a `+` in the glyph column, because what that column
            // holds is *what a row is* and this row makes one.
            let line = panel::entry(
                Some(icon::line(glyphs::PLUS, lit).into()),
                table::cell(label.clone(), Length::Fill, lit, false, true),
                None,
            );
            rows.push(
                mouse_area(line.style(move |theme| style::entry(theme, on_cursor, focused)))
                    .on_enter((msgs.at)(last))
                    .on_press((msgs.at)(last))
                    .on_release(msgs.activate.clone()),
            )
        }
        (None, None) => rows,
    };

    let body = scrollable(rows).style(style::bars).height(Length::Shrink);
    let inside: Element<'a, M> = match picker.origin {
        Some(_) => body.into(),
        None => column![
            container(text(middle(&picker.title, 38)).size(13)).padding(Padding::from([4.0, 10.0])),
            container(
                text("j k move  \u{00b7}  <Enter> toggles  \u{00b7}  <Esc> back")
                    .size(11)
                    .style(style::dim)
            )
            .padding(Padding::from([0.0, 10.0])),
            rule::horizontal(1),
            body,
        ]
        .spacing(4)
        .into(),
    };
    panel::panel(inside, picker.width).max_height(MAX_HEIGHT).into()
}
