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
//!   `?` the keys panel; `Esc` drops it all, and puts away the sheet or
//!   question on top; `Shift-F10` or the menu key opens the cursor's row's
//!   menu at the row (`crate::context_menu::Keyed`).
//! - Visual mode: `v` lights the rows from the cursor as it walks, `vig`
//!   the cursor's section's; a key a row's menu binds
//!   (`ark_app::view::MenuItem::keys`: `cc`, `rg`, `J`, `{`; its own
//!   menu's and its section heading's) runs that item on the cursor's row,
//!   or on every row lit, and the cursor stays with its rows ([`Follow`]).
//!   The browser's keys do the same (arkui-web's `keys`).
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
use crate::view::cursor::{self, Cursor, Drawn};
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

/// The rows a bound key ran on, by their titles: where the cursor and its
/// selection's anchor were, to be found again once the rows are drawn in
/// their new order.
#[derive(Clone, Debug)]
pub struct Follow {
    pane: usize,
    at: (usize, String),
    anchor: Option<(usize, String)>,
}

/// The row of `rows` titled `title` nearest `near`.
fn nearest(rows: &[cursor::Row], (near, title): &(usize, String)) -> Option<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.title == *title)
        .min_by_key(|(i, _)| i.abs_diff(*near))
        .map(|(i, _)| i)
}

impl Follow {
    /// The cursor on its rows as `drawn` has them now; where it was for any
    /// it no longer finds.
    pub fn found(&self, mut c: Cursor, drawn: &Drawn) -> Cursor {
        let rows = &drawn.rows[self.pane];
        if let Some(i) = nearest(rows, &self.at) {
            c.at[self.pane] = i;
        }
        if let (Some(a), Some(_)) = (&self.anchor, c.anchor) {
            c.anchor = nearest(rows, a).or(c.anchor);
        }
        c
    }
}

/// The sends of the items bound to `keys` on rows `first..=last` of
/// `rows`, in the order the binding runs them; none where no row lit has
/// one.
pub fn bound_sends<'a>(rows: &'a [cursor::Row], first: usize, last: usize, keys: &str) -> Vec<&'a cursor::Bound> {
    let lit = rows.get(first..=last.min(rows.len().saturating_sub(1))).unwrap_or(&[]);
    let mut out: Vec<&cursor::Bound> = vec![];
    for b in lit.iter().filter_map(|r| r.bound.iter().find(|b| b.bind.keys == keys)) {
        // A heading's item once, however many of its rows are lit.
        if b.heading.is_none() || !out.iter().any(|o| o.heading == b.heading) {
            out.push(b);
        }
    }
    if out.first().is_some_and(|b| b.bind.from_last) {
        out.reverse();
    }
    out
}

/// Every sequence of two keys or more bound on `rows`: what the grammar
/// waits for the rest of.
pub fn sequences(rows: &[cursor::Row]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = rows
        .iter()
        .flat_map(|r| r.bound.iter().map(|b| b.bind.keys))
        .filter(|k| k.chars().count() > 1)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

impl<W: Desktop> DesktopHost<W>
where
    AppMsg<W>: Send,
{
    /// What the keys panel lists of the keys the cursor's row binds, after
    /// `prefix`: each the key that comes next (the whole sequence when
    /// nothing is typed) and what it runs.
    pub(super) fn bound_entries(&self, prefix: &str) -> Vec<(String, String)> {
        let c = self.cursor.get();
        let Some(p) = cursor::index(c.focus) else { return vec![] };
        let drawn = self.drawn.borrow();
        let Some(row) = drawn.rows[p].get(c.at[p]) else { return vec![] };
        row.bound
            .iter()
            .filter_map(|b| {
                let rest = b.bind.keys.strip_prefix(prefix)?;
                (!rest.is_empty()).then(|| (rest.to_string(), b.title.clone()))
            })
            .collect()
    }

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
        // A sheet or a question is up: `Esc` puts it away, and the rows
        // under it are not walked.
        let modal = self
            .drawn
            .borrow()
            .modal
            .as_ref()
            .and_then(|m| m.downcast_ref::<Msg<AppMsg<W>>>())
            .cloned();
        if let Some(dismiss) = modal {
            return match key {
                Key::Named(iced::keyboard::key::Named::Escape) => self.update(dismiss),
                _ => Task::none(),
            };
        }
        let panes = self.panes();
        let (sections, section, bindings) = {
            let d = self.drawn.borrow();
            let p = cursor::index(self.keyboard.focus).unwrap_or(0);
            (d.sections.len(), d.section_at, sequences(&d.rows[p]))
        };
        let mut c = self.cursor.get();
        let routed = self.keyboard.press(&key, mods, &panes, section, sections, &bindings);
        match routed {
            Routed::Done | Routed::Jump(_) | Routed::Scroll(_) => {
                c.focus = self.keyboard.focus;
                // The keys went to another pane: a selection stays in its own.
                if c.focus != self.cursor.get().focus {
                    c.anchor = None;
                    self.keyboard.keys.leave_visual();
                }
                self.cursor.set(c);
                Task::none()
            }
            Routed::Escape => {
                c.keyed = false;
                c.anchor = None;
                self.cursor.set(c);
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
                c.focus = pane;
                self.cursor.set(c);
                self.act(pane, action)
            }
        }
    }

    /// One command of the grammar on the pane that has the keys.
    fn act(&mut self, pane: Pane, action: Action) -> Task<Msg<AppMsg<W>>> {
        let Some(p) = cursor::index(pane) else { return Task::none() };
        let titles: Vec<String> = self.drawn.borrow().rows[p].iter().map(|r| r.title.clone()).collect();
        let n = titles.len();
        let mut c = self.cursor.get();
        let at = c.at[p].min(n.saturating_sub(1));
        match action {
            Action::Move(m) => {
                // No cursor shown yet: the first key shows it where it is.
                let to = match c.keyed {
                    true => Grid::column(n).step(at, m),
                    false => Some(at),
                };
                if let Some(i) = to {
                    c.at[p] = i;
                }
                c.keyed = n > 0;
                self.cursor.set(c);
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
            // `v`: the selection stretches from here as the cursor walks.
            Action::Visual(on) => {
                c.keyed = n > 0;
                c.at[p] = at;
                c.anchor = (on && n > 0).then_some(at);
                self.cursor.set(c);
                Task::none()
            }
            // `vig`: every row of the cursor's section.
            Action::Inner('g') => {
                let section = self.drawn.borrow().rows[p].get(at).map(|r| r.section);
                let rows: Vec<usize> = match section {
                    Some(s) if s > 0 => self.drawn.borrow().rows[p]
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r.section == s)
                        .map(|(i, _)| i)
                        .collect(),
                    _ => vec![],
                };
                if let (Some(&first), Some(&last)) = (rows.first(), rows.last()) {
                    c.anchor = Some(first);
                    c.at[p] = last;
                    self.cursor.set(c);
                }
                Task::none()
            }
            Action::Bound(keys) => self.run_bound(p, keys),
            Action::Key(k) => {
                let mut buf = [0u8; 4];
                let keys: &str = k.encode_utf8(&mut buf);
                let keys = self.drawn.borrow().rows[p]
                    .get(at)
                    .and_then(|r| r.bound.iter().find(|b| b.bind.keys == keys).map(|b| b.bind.keys));
                match keys {
                    Some(keys) => self.run_bound(p, keys),
                    None => Task::none(),
                }
            }
            _ => Task::none(),
        }
    }

    /// The item bound to `keys` run on the rows lit in pane `p` (the
    /// cursor's, or a visual selection's), in its binding's order; the
    /// cursor follows them to where they are drawn next.
    fn run_bound(&mut self, p: usize, keys: &str) -> Task<Msg<AppMsg<W>>> {
        let c = self.cursor.get();
        let (first, last) = c.lit(p);
        let (sends, follow) = {
            let drawn = self.drawn.borrow();
            let rows = &drawn.rows[p];
            let sends: Vec<Msg<AppMsg<W>>> = bound_sends(rows, first, last, keys)
                .into_iter()
                .filter_map(|b| b.send.downcast_ref::<Msg<AppMsg<W>>>().cloned())
                .collect();
            let title = |i: usize| rows.get(i).map(|r| (i, r.title.clone()));
            let follow = title(c.at[p]).map(|at| Follow {
                pane: p,
                at,
                anchor: c.anchor.and_then(title),
            });
            (sends, follow)
        };
        if sends.is_empty() {
            return Task::none();
        }
        *self.follow.borrow_mut() = follow;
        Task::batch(sends.into_iter().map(|m| self.update(m)).collect::<Vec<_>>())
    }

    /// The cursor onto the next row whose title has what was searched.
    fn find(&mut self, p: usize, titles: &[String], from: usize, delta: isize) {
        match crate::vim::find(titles, &self.search, from, delta) {
            Some(i) => {
                let mut c = self.cursor.get();
                c.at[p] = i;
                c.keyed = true;
                self.cursor.set(c);
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

#[cfg(test)]
mod tests {
    use super::*;
    use ark_app::view::Bind;
    use std::rc::Rc;

    fn row(title: &str, binds: &[(Bind, u8)]) -> cursor::Row {
        cursor::Row {
            title: title.into(),
            open: None,
            section: 1,
            bound: binds
                .iter()
                .map(|(b, m)| cursor::Bound {
                    bind: *b,
                    title: format!("{m}"),
                    send: Rc::new(*m),
                    heading: (*m >= 20).then_some(1),
                })
                .collect(),
        }
    }

    fn sent(bs: Vec<&cursor::Bound>) -> Vec<u8> {
        bs.into_iter().map(|b| *b.send.downcast_ref::<u8>().unwrap()).collect()
    }

    /// Over a selection a binding runs on every row lit that has it, first
    /// to last, or last to first where it says so (a move down); the
    /// sequences waited for are those of two keys or more. Falsified by a
    /// move down run first to last: the rows passed one another.
    #[test]
    fn a_binding_runs_over_the_rows_lit_in_its_order() {
        let (down, up, rename) = (Bind::from_last("J"), Bind::new("K"), Bind::new("rg"));
        let rows = vec![
            row("Rent", &[(down, 1), (up, 11)]),
            row("Groceries", &[(down, 2), (up, 12), (rename, 20)]),
            row("Dining", &[(down, 3), (up, 13), (rename, 21)]),
            row("Fuel", &[]),
        ];
        assert_eq!(sent(bound_sends(&rows, 0, 2, "J")), [3, 2, 1]);
        assert_eq!(sent(bound_sends(&rows, 0, 2, "K")), [11, 12, 13]);
        // A heading's binding, on two of its rows lit, runs once.
        assert_eq!(sent(bound_sends(&rows, 1, 3, "rg")), [20]);
        assert!(bound_sends(&rows, 3, 3, "J").is_empty());
        assert_eq!(sequences(&rows), ["rg"]);
    }

    /// The cursor and its anchor are found again by their rows' titles in
    /// the new order. Falsified by keeping the old places: after a move
    /// down the cursor lit the row that came up past it.
    #[test]
    fn the_cursor_follows_its_rows() {
        let follow = Follow {
            pane: 0,
            at: (1, "Groceries".into()),
            anchor: Some((0, "Rent".into())),
        };
        let mut drawn = Drawn::default();
        drawn.rows[0] = vec![row("Dining", &[]), row("Rent", &[]), row("Groceries", &[])];
        let c = Cursor {
            at: [1, 0, 0],
            anchor: Some(0),
            keyed: true,
            ..Cursor::default()
        };
        let found = follow.found(c, &drawn);
        assert_eq!((found.at[0], found.anchor), (2, Some(1)));
    }
}
