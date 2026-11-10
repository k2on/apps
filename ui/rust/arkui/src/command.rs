//! A command box: one search box over everything an app can go to or do,
//! opened by Ctrl+K (⌘K on a Mac) from anywhere, as shadcn/ui's command
//! dialog is. What it finds is drawn under the box in groups, each under a
//! quiet heading; the row the keys are on is lit, ↑ and ↓ (or Ctrl+P and
//! Ctrl+N) walk it, ⏎ chooses it, and a click chooses the row clicked.
//!
//! **The app searches; this draws.** It hands over the groups for what has
//! been typed, each row with the message choosing it sends, and gets back
//! the box with them under it — so what a library app finds, and in what
//! order, is the app's.
//!
//! **It stands where a dialog does and moves as one** ([`crate::dialog`]):
//! fading in and growing from 95% over the page dimmed, and back.
//!
//! **Its rows are declared heights**, as a panel's are ([`crate::panel`]),
//! so where the row with the keys is down the list is arithmetic, and the
//! list scrolls to keep it in sight without anything being measured.
//!
//! A text box takes `<Esc>` for itself, and the arrows are no text box's;
//! [`keys`] hears all three, and Ctrl+K, whatever has the keyboard, so the
//! app can answer them while the box has it.
//!
//! **What is typed is the box's from the key that opens it.** The keyboard
//! comes to its text box by a task (`focus`), done after the update that
//! opened it, so a key typed straight after Ctrl+K can come before it. No
//! text box takes such a key, and [`keys`] hears it as what it types
//! ([`Press::Type`], [`Press::Erase`], [`Press::Choose`]), which the app
//! puts in the box ([`Command::missed`]) as the box would have: typing as
//! fast as the hands go, on however slow a machine, finds the same thing.
use crate::text::{self, text};
use crate::text_input::{self, text_input};
use iced::keyboard::{key::Named, Key, Modifiers};
use iced::time::Instant;
use iced::widget::{column, container, mouse_area, row, rule, scrollable, Column, Space};
use iced::{Alignment, Background, Border, Element, Length, Padding, Task};

use crate::motion::Shown;
use crate::theme::of;
use crate::{dialog, glyphs, icon};

/// How wide it is: `max-w-lg`, 32 rem.
pub const WIDTH: f32 = 512.0;

/// The tallest its list gets before it scrolls: `max-h-[300px]`.
pub const LIST_HEIGHT: f32 = 300.0;

/// The box's line, a group's heading, a row, and the room around a group:
/// `h-12`, the heading's `py-1.5` over `text-xs`, the row's `py-1.5` over
/// `text-sm`, and the group's `p-1`.
pub const INPUT: f32 = 48.0;
pub const HEADING: f32 = 28.0;
pub const ROW: f32 = 34.0;
pub const GROUP_PAD: f32 = 4.0;

/// What the list says when nothing is found, and how tall it is: `py-6`.
pub const EMPTY: &str = "No results found.";
pub const EMPTY_HEIGHT: f32 = 68.0;

/// One thing it found: a glyph, what it is called, a quieter word beside it
/// (who made it, what kind it is), and the message choosing it sends.
#[derive(Clone, Debug)]
pub struct Item<M> {
    pub glyph: &'static [u8],
    pub label: String,
    pub detail: Option<String>,
    pub choose: M,
}

/// Things found of one kind, under a heading.
#[derive(Clone, Debug)]
pub struct Group<M> {
    pub heading: String,
    pub items: Vec<Item<M>>,
}

/// The box while it is up, or on its way out: what is typed, which row has
/// the keys, how far down its list is scrolled, and how far open it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub query: String,
    pub at: usize,
    pub shown: Shown,
    scrolled: f32,
}

/// A key the box answers, heard whatever has the keyboard: [`keys`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Press {
    /// Ctrl+K or ⌘K: open it, or close it.
    Toggle,
    /// `<Esc>`.
    Close,
    /// ↑ or Ctrl+P, ↓ or Ctrl+N.
    Up,
    Down,
    /// What a key typed that no text box took, while it was up: typed
    /// before its box had the keyboard.
    Type(String),
    /// `<Backspace>` no text box took.
    Erase,
    /// `<Enter>` no text box took: the row with the keys chosen.
    Choose,
}

/// Whether `key` with `mods` is Ctrl+K (⌘K on a Mac).
pub fn opens(key: &Key, mods: Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character("k")) && (mods.control() || mods.logo()) && !mods.alt() && !mods.shift()
}

/// The keys it answers, from every key pressed — the ones a text box took
/// as well — while `open` says it is up; Ctrl+K whether or not.
pub fn press(key: &Key, mods: Modifiers, open: bool) -> Option<Press> {
    if opens(key, mods) {
        return Some(Press::Toggle);
    }
    if !open {
        return None;
    }
    let ctrl = mods.control();
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Press::Close),
        Key::Named(Named::ArrowUp) => Some(Press::Up),
        Key::Named(Named::ArrowDown) => Some(Press::Down),
        Key::Character("p") if ctrl => Some(Press::Up),
        Key::Character("n") if ctrl => Some(Press::Down),
        _ => None,
    }
}

/// What a key no text box took types into the box, while it is up: its
/// text, unless Ctrl, Alt or Super is held (a shortcut, not typing);
/// `<Backspace>`; `<Enter>`.
pub fn missed(key: &Key, mods: Modifiers, text: Option<&str>) -> Option<Press> {
    if mods.control() || mods.alt() || mods.logo() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::Backspace) => Some(Press::Erase),
        Key::Named(Named::Enter) => Some(Press::Choose),
        _ => text
            .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
            .map(|t| Press::Type(t.to_string())),
    }
}

/// An event as [`keys`] hears it, for a box `open` or not: a key pressed,
/// as [`press`] reads it whether or not a widget took it; and while the
/// box is up, one no widget took as what it types ([`missed`]). A key its
/// text box took is the box's already, and is not heard twice.
pub fn heard(event: &iced::Event, status: iced::event::Status, open: bool) -> Option<Press> {
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, text, .. }) = event else {
        return None;
    };
    press(key, *modifiers, open).or_else(|| match (open, status) {
        (true, iced::event::Status::Ignored) => missed(key, *modifiers, text.as_deref()),
        _ => None,
    })
}

/// Every key pressed, captured by a widget or not, as [`heard`] reads it
/// for a box `open` or not: `open` is a plain function of nothing, so a
/// subscription can be made of it, and the app passes the one for its
/// state (one when it is up, the other when it is not).
pub fn keys(open: bool) -> iced::Subscription<Press> {
    fn up(event: iced::Event, status: iced::event::Status, _: iced::window::Id) -> Option<Press> {
        heard(&event, status, true)
    }
    fn down(event: iced::Event, status: iced::event::Status, _: iced::window::Id) -> Option<Press> {
        heard(&event, status, false)
    }
    iced::event::listen_with(if open { up } else { down })
}

/// How many rows `groups` hold in all.
pub fn rows<M>(groups: &[Group<M>]) -> usize {
    groups.iter().map(|g| g.items.len()).sum()
}

/// How far down the list row `at` starts, and how tall the whole list is.
fn place<M>(groups: &[Group<M>], at: usize) -> (f32, f32) {
    let (mut y, mut i, mut found) = (0.0, 0, 0.0);
    for g in groups.iter().filter(|g| !g.items.is_empty()) {
        y += GROUP_PAD + HEADING;
        for _ in &g.items {
            if i == at {
                found = y;
            }
            y += ROW;
            i += 1;
        }
        y += GROUP_PAD;
    }
    (found, y)
}

/// How tall the list is drawn: all of it, up to [`LIST_HEIGHT`].
pub fn list_height<M>(groups: &[Group<M>]) -> f32 {
    if rows(groups) == 0 {
        return EMPTY_HEIGHT;
    }
    place(groups, 0).1.min(LIST_HEIGHT)
}

impl Command {
    /// The box up at `now`, empty — at once, with no spring: it is opened
    /// from the keyboard (Ctrl+K) many times a day, and what the keys open
    /// is there on the frame the key goes down (ui/docs/motion.md, the
    /// frequency rule; Raycast's box has no animation either).
    pub fn new(now: Instant) -> Self {
        let mut shown = dialog::shown(now);
        shown.snap(now);
        Command {
            query: String::new(),
            at: 0,
            shown,
            scrolled: 0.0,
        }
    }

    /// The box closed at `now`, at once, as it opened: gone on the next
    /// frame, whether a key or a click away closed it.
    pub fn close(&mut self, now: Instant) {
        self.shown.hide(now);
        self.shown.snap(now);
    }

    /// What is typed is now `query`: the keys go back to the first row, at
    /// the top of the list.
    pub fn typed<M>(&mut self, query: String, list: &'static str) -> Task<M> {
        self.query = query;
        self.at = 0;
        self.scrolled = 0.0;
        iced::widget::operation::scroll_to(list, scrollable::AbsoluteOffset { x: 0.0, y: 0.0 })
    }

    /// A key no text box took ([`missed`]), put in the box as its text box
    /// would have: what it types after what is typed, or the last
    /// character taken off. Anything else leaves it as it is.
    pub fn missed<M>(&mut self, press: &Press, list: &'static str) -> Task<M> {
        let mut query = self.query.clone();
        match press {
            Press::Type(t) => query.push_str(t),
            Press::Erase => {
                query.pop();
            }
            _ => return Task::none(),
        }
        self.typed(query, list)
    }

    /// The list scrolled to `y`, by the wheel or by [`Command::step`].
    pub fn scrolled(&mut self, y: f32) {
        self.scrolled = y;
    }

    /// The keys onto row `at` — the pointer over it.
    pub fn land(&mut self, at: usize) {
        self.at = at;
    }

    /// The keys one row up or down `groups`, stopping at either end, and the
    /// list scrolled just far enough to show it.
    pub fn step<M>(&mut self, down: bool, groups: &[Group<M>], list: &'static str) -> Task<M> {
        let n = rows(groups);
        if n == 0 {
            return Task::none();
        }
        self.at = if down { (self.at + 1).min(n - 1) } else { self.at.saturating_sub(1) };
        let (top, _) = place(groups, self.at);
        // The first row brings its heading with it.
        let top = if self.at == 0 { 0.0 } else { top };
        let view = list_height(groups);
        let y = if top < self.scrolled {
            top
        } else if top + ROW > self.scrolled + view {
            top + ROW - view
        } else {
            return Task::none();
        };
        self.scrolled = y;
        iced::widget::operation::scroll_to(list, scrollable::AbsoluteOffset { x: 0.0, y })
    }

    /// The message the row with the keys sends, if there is a row.
    pub fn chosen<M: Clone>(&self, groups: &[Group<M>]) -> Option<M> {
        groups.iter().flat_map(|g| &g.items).nth(self.at).map(|i| i.choose.clone())
    }
}

/// What it takes to draw it: the ids of its box and its list (for the keys
/// and for scrolling), what typing sends, what the pointer onto a row sends,
/// and what the wheel over the list sends.
pub struct Messages<'a, M> {
    pub input: &'static str,
    pub list: &'static str,
    pub typed: Box<dyn Fn(String) -> M + 'a>,
    pub at: Box<dyn Fn(usize) -> M + 'a>,
    pub scrolled: Box<dyn Fn(f32) -> M + 'a>,
}

/// The box, `open` of the way in: `placeholder` in it while nothing is
/// typed, and `groups` under it, the row the keys are on lit.
pub fn view<'a, M: Clone + 'a>(command: &Command, placeholder: &str, groups: Vec<Group<M>>, messages: Messages<'a, M>, open: f32) -> Element<'a, M> {
    dialog::present(panel(command, placeholder, groups, messages, WIDTH), open)
}

/// The box alone, `width` wide, for an app to stand where it wants: a
/// dropdown under the cell it chooses for, as a searchable menu
/// ([`crate::layer::pinned`]), where [`view`] stands it as a dialog.
pub fn panel<'a, M: Clone + 'a>(
    command: &Command,
    placeholder: &str,
    groups: Vec<Group<M>>,
    messages: Messages<'a, M>,
    width: f32,
) -> Element<'a, M> {
    let chosen = command.chosen(&groups);
    let mut input = text_input(placeholder, &command.query)
        .id(messages.input)
        .on_input(messages.typed)
        .size(14)
        .padding(Padding::ZERO)
        .style(|theme, status| {
            let palette = of(theme);
            let base = text_input::default(theme, status);
            text_input::Style {
                background: Background::Color(iced::Color::TRANSPARENT),
                border: Border::default(),
                placeholder: palette.background.strong.text.scale_alpha(0.8),
                ..base
            }
        });
    if let Some(m) = chosen {
        input = input.on_submit(m);
    }
    let search = row![icon::tinted(glyphs::SEARCH, 16.0, |p| p.background.strong.text), input]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding([0.0, 12.0])
        .height(Length::Fixed(INPUT));
    let list = found(command, groups, messages.at, messages.scrolled, messages.list);
    let panel = container(column![search, rule::horizontal(1).style(crate::style::edge), list])
        .width(Length::Fixed(width))
        .style(dialog::style);
    panel.into()
}

/// What was found alone, `width` wide, with no box over it: for an app
/// whose box is somewhere of its own — the cell being chosen for, typed
/// into where it is, YNAB's way — with this hanging under it. The keys and
/// [`Command`] are the same; `messages.input` and `typed` go unused.
pub fn list<'a, M: Clone + 'a>(command: &Command, groups: Vec<Group<M>>, messages: Messages<'a, M>, width: f32) -> Element<'a, M> {
    container(found(command, groups, messages.at, messages.scrolled, messages.list))
        .width(Length::Fixed(width))
        .style(dialog::style)
        .into()
}

/// The groups found, the row with the keys lit, scrolled in `list`.
fn found<'a, M: Clone + 'a>(
    command: &Command,
    groups: Vec<Group<M>>,
    at_of: Box<dyn Fn(usize) -> M + 'a>,
    scrolled: Box<dyn Fn(f32) -> M + 'a>,
    list_id: &'static str,
) -> Element<'a, M> {
    let height = list_height(&groups);
    if rows(&groups) == 0 {
        container(text(EMPTY).size(13))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(EMPTY_HEIGHT))
            .into()
    } else {
        let mut list = Column::new();
        let mut i = 0;
        for g in groups.into_iter().filter(|g| !g.items.is_empty()) {
            let mut group = column![container(text(g.heading).size(11).style(|t| text::Style {
                color: Some(of(t).background.strong.text),
                ..Default::default()
            }))
            .padding([0.0, 8.0])
            .center_y(Length::Fixed(HEADING))]
            .padding(GROUP_PAD);
            for item in g.items {
                let lit = i == command.at;
                let mut line = row![icon::line(item.glyph, false), text(item.label).size(14)]
                    .spacing(8)
                    .align_y(Alignment::Center);
                if let Some(d) = item.detail {
                    line = line.push(Space::new().width(Length::Fill)).push(text(d).size(12).style(|t| text::Style {
                        color: Some(of(t).background.strong.text),
                        ..Default::default()
                    }));
                }
                let drawn = container(line)
                    .padding([0.0, 8.0])
                    .center_y(Length::Fixed(ROW))
                    .width(Length::Fill)
                    .style(move |t| container::Style {
                        background: lit.then(|| Background::Color(crate::style::focus(t, true))),
                        border: Border {
                            radius: crate::style::FOCUS_RADIUS.into(),
                            ..Border::default()
                        },
                        ..container::Style::default()
                    });
                group = group.push(mouse_area(drawn).on_enter(at_of(i)).on_press(item.choose));
                i += 1;
            }
            list = list.push(group);
        }

        crate::scroll::elastic(
            scrollable(list)
                .id(list_id)
                .height(Length::Fixed(height))
                .style(crate::style::bars)
                .on_scroll(move |v| scrolled(v.absolute_offset().y)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::Modifiers;

    fn groups(sizes: &[usize]) -> Vec<Group<usize>> {
        let mut n = 0;
        sizes
            .iter()
            .map(|&k| Group {
                heading: "Some".into(),
                items: (0..k)
                    .map(|_| {
                        n += 1;
                        Item {
                            glyph: glyphs::SEARCH,
                            label: format!("row {n}"),
                            detail: None,
                            choose: n - 1,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    /// Ctrl+K and ⌘K open it from anywhere; with it up, `<Esc>`, the arrows
    /// and Ctrl+P and Ctrl+N are its; with it down, they are nobody's here.
    /// Falsified by a `k` alone opening it: typing a word with a k in it in
    /// any box would.
    #[test]
    fn ctrl_k_opens_it_and_its_keys_are_its_while_it_is_up() {
        let k = Key::Character("k".into());
        assert_eq!(press(&k, Modifiers::CTRL, false), Some(Press::Toggle));
        assert_eq!(press(&k, Modifiers::LOGO, true), Some(Press::Toggle));
        assert_eq!(press(&k, Modifiers::empty(), true), None);
        let esc = Key::Named(Named::Escape);
        assert_eq!(press(&esc, Modifiers::empty(), true), Some(Press::Close));
        assert_eq!(press(&esc, Modifiers::empty(), false), None);
        assert_eq!(press(&Key::Character("n".into()), Modifiers::CTRL, true), Some(Press::Down));
        assert_eq!(press(&Key::Named(Named::ArrowUp), Modifiers::empty(), true), Some(Press::Up));
    }

    fn pressed(key: Key, mods: Modifiers, text: Option<&str>) -> iced::Event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers: mods,
            text: text.map(Into::into),
            repeat: false,
        })
    }

    /// A key typed while the box is up that no text box took — typed
    /// before the focus its opening asked for had landed — is heard as what
    /// it types, and goes in the box; one its text box took is not heard
    /// again, and with the box down nothing is. Falsified by `keys` reading
    /// only `press`, as it did: "Elise" typed straight after Ctrl+K went
    /// nowhere until the focus came, and harken's search shot found nothing
    /// on a slow machine.
    #[test]
    fn a_key_typed_before_the_box_has_the_keyboard_goes_in_it() {
        use iced::event::Status;
        let e = pressed(Key::Character("e".into()), Modifiers::SHIFT, Some("E"));
        assert_eq!(heard(&e, Status::Ignored, true), Some(Press::Type("E".into())));
        assert_eq!(heard(&e, Status::Captured, true), None, "its box took it");
        assert_eq!(heard(&e, Status::Ignored, false), None, "the box is down");
        let ctrl_l = pressed(Key::Character("l".into()), Modifiers::CTRL, Some("\u{c}"));
        assert_eq!(heard(&ctrl_l, Status::Ignored, true), None, "a shortcut types nothing");
        let back = pressed(Key::Named(Named::Backspace), Modifiers::empty(), None);
        let enter = pressed(Key::Named(Named::Enter), Modifiers::empty(), None);
        assert_eq!(heard(&back, Status::Ignored, true), Some(Press::Erase));
        assert_eq!(heard(&enter, Status::Ignored, true), Some(Press::Choose));
        let ctrl_k = pressed(Key::Character("k".into()), Modifiers::CTRL, None);
        assert_eq!(heard(&ctrl_k, Status::Captured, false), Some(Press::Toggle));

        let mut c = Command::new(Instant::now());
        for p in [Press::Type("E".into()), Press::Type("lx".into()), Press::Erase, Press::Up] {
            let _: Task<usize> = c.missed(&p, "list");
        }
        assert_eq!(c.query, "El");
    }

    /// The keys walk the rows across groups and stop at either end; the
    /// row they are on is what ⏎ chooses; a new query starts them at the
    /// top. Falsified by a step that wrapped: ↑ on the first row chose the
    /// last.
    #[test]
    fn the_keys_walk_the_rows_and_stop_at_the_ends() {
        let g = groups(&[2, 3]);
        let mut c = Command::new(Instant::now());
        let _: Task<usize> = c.step(false, &g, "list");
        assert_eq!(c.chosen(&g), Some(0));
        for _ in 0..3 {
            let _: Task<usize> = c.step(true, &g, "list");
        }
        assert_eq!(c.chosen(&g), Some(3), "into the second group");
        for _ in 0..5 {
            let _: Task<usize> = c.step(true, &g, "list");
        }
        assert_eq!(c.chosen(&g), Some(4), "the last, and no further");
        let _: Task<usize> = c.typed("bach".into(), "list");
        assert_eq!((c.at, c.query.as_str()), (0, "bach"));
        assert_eq!(c.chosen::<usize>(&[]), None);
    }

    /// The list is as tall as what it holds up to 300, and the empty one
    /// the height of its sentence; a row past the bottom scrolls the list
    /// just far enough to show it. Falsified by scrolling to the row's top:
    /// the list jumped by a whole view at a time.
    #[test]
    fn the_list_is_as_tall_as_it_holds_and_scrolls_to_the_keys() {
        assert_eq!(list_height(&groups(&[2])), GROUP_PAD + HEADING + 2.0 * ROW + GROUP_PAD);
        assert_eq!(list_height(&groups(&[20])), LIST_HEIGHT);
        assert_eq!(list_height::<usize>(&[]), EMPTY_HEIGHT);
        let g = groups(&[20]);
        let mut c = Command::new(Instant::now());
        for _ in 0..8 {
            let _: Task<usize> = c.step(true, &g, "list");
        }
        // Row 8 starts at 4 + 28 + 8·34 = 304 and ends at 338.
        assert!((c.scrolled - (338.0 - LIST_HEIGHT)).abs() < 1e-3, "{}", c.scrolled);
    }
}
