//! The keyboard every Ark app has (ui/docs/design.md, "The Keyboard, for
//! Every App"), in the desktop's window, as arkui-web's `keys` has it on
//! the page:
//!
//! - The app's commands (`Windowed::commands`, `ark_app::shell::CommandMenu`)
//!   by their shortcuts (`command_for`, Ctrl with the key; ⌘ on a Mac), and
//!   every one by name in the command box (`:`, or Ctrl-K), under its menu's
//!   heading.
//! - The vim layer (`crate::shell::Keyboard` over `crate::vim`) over the
//!   rows drawn (`crate::view::cursor`): `j`/`k` and the arrows, a count,
//!   `gg`/`G`/`{n}G`, `Ctrl-d`/`Ctrl-u`; `h`/`l` and `Tab` between the
//!   panes, the sidebar's cursor the section shown; `Enter`/`o` opens the
//!   row; `/` searches the rows' titles and `n`/`N` go to the next match;
//!   `?` the keys panel; `Esc` drops it all.
//! - A key moves nothing (ui/docs/motion.md, the frequency rule): the
//!   cursor is on its row on the frame the key went down.
//!
//! A field that has the keys keeps its own: typing a name walks no list
//! (`crate::vim::presses` hears only what no widget took).

use ark_app::shell::{command_for, CommandMenu};
use ark_app::window::Windowed;
use iced::keyboard::{Key, Modifiers};
use iced::{Element, Task};

use crate::command::{Command, Group, Item, Messages, Press};
use crate::shell::{Pane, Routed};
use crate::view::cursor;
use crate::vim::{Action, Grid};

use super::{Desktop, DesktopHost, Msg};

type AppMsg<W> = <<W as Desktop>::App as Windowed>::Msg;

/// The command box's ids: its field and its list.
const INPUT: &str = "ark-command-input";
const LIST: &str = "ark-command-list";

/// What the command box is told.
#[derive(Clone, Debug)]
pub enum CommandMsg {
    Typed(String),
    At(usize),
    Scrolled(f32),
    Press(Press),
}

/// The app's commands as the command box lists them: a group a menu,
/// each command what it runs, its shortcut beside it; only those whose
/// title has what is typed.
pub fn groups<M: Clone>(menus: &[CommandMenu<M>], query: &str) -> Vec<Group<M>> {
    let q = query.trim().to_lowercase();
    menus
        .iter()
        .map(|m| Group {
            heading: m.place.title().to_string(),
            items: m
                .items
                .iter()
                .filter(|c| q.is_empty() || c.title.to_lowercase().contains(&q))
                .map(|c| Item {
                    glyph: crate::glyphs::SEARCH,
                    label: c.title.to_string(),
                    detail: c.shortcut.map(|s| s.label()),
                    choose: c.run.clone(),
                })
                .collect(),
        })
        .filter(|g| !g.items.is_empty())
        .collect()
}

/// The command box over the window, the app's commands found under it.
pub fn command_box<'a, M: Clone + 'a>(command: &Command, menus: &[CommandMenu<M>]) -> Element<'a, Msg<M>> {
    let found = groups(menus, &command.query)
        .into_iter()
        .map(|g| Group {
            heading: g.heading,
            items: g
                .items
                .into_iter()
                .map(|i| Item {
                    glyph: i.glyph,
                    label: i.label,
                    detail: i.detail,
                    // The row with the keys: the one the pointer is on, or
                    // the keys walked to.
                    choose: Msg::Command(CommandMsg::Press(Press::Choose)),
                })
                .collect(),
        })
        .collect();
    let messages = Messages {
        input: INPUT,
        list: LIST,
        typed: Box::new(|q| Msg::Command(CommandMsg::Typed(q))),
        at: Box::new(|i| Msg::Command(CommandMsg::At(i))),
        scrolled: Box::new(|y| Msg::Command(CommandMsg::Scrolled(y))),
    };
    let panel = crate::command::view(command, "Type a Command", found, messages, 1.0);
    iced::widget::stack![
        crate::layer::dimmed(Msg::Command(CommandMsg::Press(Press::Close)), Msg::Swallow),
        crate::layer::centred(panel)
    ]
    .into()
}

impl<W: Desktop> DesktopHost<W>
where
    AppMsg<W>: Send,
{
    /// A key no widget took.
    pub(super) fn key(&mut self, key: Key, mods: Modifiers) -> Task<Msg<AppMsg<W>>> {
        // A command's shortcut, the menu bar's on a Mac: ⌘ there, Ctrl here.
        if mods.control() || mods.logo() {
            if let Key::Character(c) = &key {
                let menus = self.app().commands();
                if let Some(run) = command_for(&menus, c.as_str(), mods.shift(), mods.alt()).map(|c| c.run.clone()) {
                    return self.update(Msg::App(run));
                }
            }
        }
        if self.command.is_none() && (crate::command::opens(&key, mods) || matches!(&key, Key::Character(c) if c.as_str() == ":")) {
            self.command = Some(Command::new(iced::time::Instant::now()));
            return iced::widget::operation::focus(INPUT);
        }
        let panes = self.panes();
        let (sections, section) = {
            let d = self.drawn.borrow();
            (d.sections.len(), d.section_at)
        };
        match self.keyboard.press(&key, mods, &panes, section, sections, &[]) {
            Routed::Done | Routed::Jump(_) | Routed::Scroll(_) => {
                self.cursor.focus = self.keyboard.focus;
                Task::none()
            }
            Routed::Escape => {
                self.cursor.keyed = false;
                Task::none()
            }
            Routed::Section(i) => {
                let select = self
                    .drawn
                    .borrow()
                    .sections
                    .get(i)
                    .and_then(|m| m.downcast_ref::<Msg<AppMsg<W>>>())
                    .cloned();
                match select {
                    Some(m) => self.update(m),
                    None => Task::none(),
                }
            }
            Routed::Pane(pane, action) => {
                self.cursor.focus = pane;
                self.act(pane, action)
            }
        }
    }

    /// One command of the grammar on the pane that has the keys.
    fn act(&mut self, pane: Pane, action: Action) -> Task<Msg<AppMsg<W>>> {
        let Some(p) = cursor::index(pane) else { return Task::none() };
        let titles: Vec<String> = self.drawn.borrow().rows[p].iter().map(|r| r.title.clone()).collect();
        let n = titles.len();
        let at = self.cursor.at[p].min(n.saturating_sub(1));
        match action {
            Action::Move(m) => {
                // No cursor shown yet: the first key shows it where it is.
                let to = match self.cursor.keyed {
                    true => Grid::column(n).step(at, m),
                    false => Some(at),
                };
                if let Some(i) = to {
                    self.cursor.at[p] = i;
                }
                self.cursor.keyed = n > 0;
                Task::none()
            }
            Action::Activate => {
                let open = self.drawn.borrow().rows[p]
                    .get(at)
                    .and_then(|r| r.open.as_ref()?.downcast_ref::<Msg<AppMsg<W>>>().cloned());
                match open {
                    Some(m) => self.update(m),
                    None => Task::none(),
                }
            }
            Action::Search(q) => {
                self.search = q;
                self.find(p, &titles, at.saturating_sub(1), 1);
                Task::none()
            }
            Action::Match(delta) => {
                self.find(p, &titles, at, delta);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The cursor onto the next row whose title has what was searched.
    fn find(&mut self, p: usize, titles: &[String], from: usize, delta: isize) {
        match crate::vim::find(titles, &self.search, from, delta) {
            Some(i) => {
                self.cursor.at[p] = i;
                self.cursor.keyed = true;
            }
            None => self.said.push(format!("Nothing Matches \u{201c}{}\u{201d}", self.search)),
        }
    }

    /// What the command box was told: typed into, walked, chosen, closed.
    pub(super) fn command(&mut self, m: CommandMsg) -> Task<Msg<AppMsg<W>>> {
        let menus = self.app().commands();
        let Some(c) = &mut self.command else { return Task::none() };
        let found = groups(&menus, &c.query);
        match m {
            CommandMsg::Typed(q) => c.typed(q, LIST),
            CommandMsg::At(i) => {
                c.land(i);
                Task::none()
            }
            CommandMsg::Scrolled(y) => {
                c.scrolled(y);
                Task::none()
            }
            CommandMsg::Press(Press::Up) => c.step(false, &found, LIST).map(Msg::App),
            CommandMsg::Press(Press::Down) => c.step(true, &found, LIST).map(Msg::App),
            CommandMsg::Press(p @ (Press::Type(_) | Press::Erase)) => c.missed(&p, LIST),
            CommandMsg::Press(Press::Choose) => {
                let run = c.chosen(&found);
                self.command = None;
                match run {
                    Some(run) => self.update(Msg::App(run)),
                    None => Task::none(),
                }
            }
            CommandMsg::Press(Press::Close | Press::Toggle) => {
                self.command = None;
                Task::none()
            }
        }
    }
}
