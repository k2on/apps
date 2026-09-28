//! Vim's grammar, and the one shape a component says it is to get it.
//!
//! Three things are kept apart on purpose, because each is useful without the
//! others and mixing them is what makes keyboard code untestable:
//!
//! 1. **[`Keys`] turns key presses into an [`Action`].** It knows `5j` is five
//!    downs and `gg` is the top, and nothing about what is on screen. It is a
//!    state machine over characters and can be tested by pressing letters at
//!    it.
//! 2. **[`Grid`] turns a [`Motion`] into a new cursor.** A component says how
//!    many cells it has and how many columns they are in; it gets counts,
//!    `gg`, `G` and `{count}G` for free, and never sees a key.
//! 3. **The application does the rest** — which pane has the cursor, what
//!    `Activate` means, what a search matches against. None of that
//!    generalises, so none of it is here.
//!
//! The part worth explaining is what [`Grid::step`] returning `None` means:
//! *not mine*. A step is refused only when the cursor is already at that edge
//! (see [`along`]), and the caller is then free to read the refusal as "this
//! belongs to whatever is that way" — which is how a sidebar and a list are
//! moved between without either knowing the other exists, and how a menu's
//! right-hand edge becomes its submenu ([`crate::context`]).
//!
//! **`keyboard::listen` reports only the presses no widget took** ([`presses`]),
//! which is what makes a modeless vim layer safe beside a text box: a focused
//! box consumes its own keys and none reach the grammar, so typing a name
//! cannot also walk the cursor down a list. There is no insert mode because
//! there is nothing to get stuck in.

use iced::keyboard::{key::Named, Key, Modifiers};

/// How far a page moves. Vim's `^D` is half a window; a component here does not
/// know how tall its window is, so this is a fixed step and says so rather than
/// pretending to measure.
pub const PAGE: usize = 10;

/// Where a motion wants to go, with no idea what it is moving through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Down(usize),
    Up(usize),
    Left(usize),
    Right(usize),
    /// `gg`
    First,
    /// `G`
    Last,
    /// `{count}G` — a cell number, counting from one the way vim counts lines.
    To(usize),
}

/// A finished command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Move(Motion),
    /// `<Enter>` or `o` — open what the cursor is on.
    Activate,
    /// `<Space>` — the application's one toggle (play and pause, in harken).
    Toggle,
    /// A `/` search was typed and accepted.
    Search(String),
    /// `n` forwards, `N` back.
    Match(isize),
    /// `<Tab>` — the next pane, whatever the panes are.
    Cycle,
    /// `<Esc>` — drop whatever was half-typed.
    Cancel,
    /// Keys an application binds for itself: `a`, `m`, `?`. Held apart from the
    /// motions so that adding one cannot change the grammar.
    Key(char),
}

/// What [`Keys`] is in the middle of.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    /// `/` was pressed; this is what has been typed since.
    Search(String),
}

/// The keys pressed so far, and what they have not yet added up to.
#[derive(Debug, Default)]
pub struct Keys {
    count: Option<usize>,
    /// A prefix waiting for its second key — only `g` today.
    pending: Option<char>,
    mode: Mode,
}

impl Keys {
    pub fn new() -> Keys {
        Keys::default()
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// What to show so the typist can see what the editor thinks it has: `5`,
    /// `2g`, `/chop`. Empty when nothing is half-typed, which is most of the
    /// time.
    pub fn pending(&self) -> String {
        match &self.mode {
            Mode::Search(q) => format!("/{q}"),
            Mode::Normal => {
                let mut s = String::new();
                if let Some(n) = self.count {
                    s.push_str(&n.to_string());
                }
                if let Some(c) = self.pending {
                    s.push(c);
                }
                s
            }
        }
    }

    fn take_count(&mut self) -> usize {
        self.count.take().unwrap_or(1)
    }

    /// Feed one key press. `None` means "still typing".
    pub fn press(&mut self, key: &Key, mods: Modifiers) -> Option<Action> {
        // A search swallows everything until it is accepted or abandoned, so
        // that `/` then `j` searches for a j rather than moving.
        if let Mode::Search(query) = &mut self.mode {
            return match key {
                Key::Named(Named::Enter) => {
                    let q = std::mem::take(query);
                    self.mode = Mode::Normal;
                    (!q.is_empty()).then_some(Action::Search(q))
                }
                Key::Named(Named::Escape) => {
                    self.mode = Mode::Normal;
                    Some(Action::Cancel)
                }
                Key::Named(Named::Backspace) => {
                    query.pop();
                    None
                }
                Key::Named(Named::Space) => {
                    query.push(' ');
                    None
                }
                Key::Character(c) => {
                    query.push_str(c);
                    None
                }
                _ => None,
            };
        }

        // `^D` and `^U`, which are the only chorded keys in the grammar.
        if mods.control() {
            if let Key::Character(c) = key {
                return match c.as_str() {
                    "d" => Some(Action::Move(Motion::Down(PAGE))),
                    "u" => Some(Action::Move(Motion::Up(PAGE))),
                    _ => None,
                };
            }
            return None;
        }

        match key {
            Key::Named(Named::Escape) => {
                self.count = None;
                self.pending = None;
                Some(Action::Cancel)
            }
            Key::Named(Named::Enter) => {
                self.count = None;
                self.pending = None;
                Some(Action::Activate)
            }
            Key::Named(Named::Space) => {
                self.count = None;
                self.pending = None;
                Some(Action::Toggle)
            }
            Key::Named(Named::Tab) => Some(Action::Cycle),
            Key::Named(Named::ArrowDown) => Some(Action::Move(Motion::Down(self.take_count()))),
            Key::Named(Named::ArrowUp) => Some(Action::Move(Motion::Up(self.take_count()))),
            Key::Named(Named::ArrowLeft) => Some(Action::Move(Motion::Left(self.take_count()))),
            Key::Named(Named::ArrowRight) => Some(Action::Move(Motion::Right(self.take_count()))),
            Key::Character(c) => self.character(c.as_str()),
            _ => None,
        }
    }

    fn character(&mut self, c: &str) -> Option<Action> {
        // A count is built up digit by digit, and a leading zero is not a count
        // — in vim `0` is a motion. There is no `0` motion here, so it is
        // simply ignored rather than starting a count of nothing.
        if let Ok(d) = c.parse::<usize>() {
            if d == 0 && self.count.is_none() {
                return None;
            }
            self.count = Some(self.count.unwrap_or(0) * 10 + d);
            return None;
        }

        if self.pending == Some('g') {
            self.pending = None;
            let count = self.take_count();
            return match c {
                // `gg` is the top, and `{count}gg` is that cell, like vim.
                "g" => Some(Action::Move(if count > 1 { Motion::To(count) } else { Motion::First })),
                _ => None,
            };
        }

        match c {
            "g" => {
                self.pending = Some('g');
                None
            }
            "j" => Some(Action::Move(Motion::Down(self.take_count()))),
            "k" => Some(Action::Move(Motion::Up(self.take_count()))),
            "h" => Some(Action::Move(Motion::Left(self.take_count()))),
            "l" => Some(Action::Move(Motion::Right(self.take_count()))),
            // `{count}G` is a cell number; a bare `G` is the end.
            "G" => Some(Action::Move(match self.count.take() {
                Some(n) => Motion::To(n),
                None => Motion::Last,
            })),
            "o" => Some(Action::Activate),
            "/" => {
                self.count = None;
                self.mode = Mode::Search(String::new());
                None
            }
            "n" => Some(Action::Match(1)),
            "N" => Some(Action::Match(-1)),
            other => other.chars().next().map(Action::Key),
        }
    }
}

/// Clamp a cell number to a shape that has `cells` of them.
fn clamp(at: usize, cells: usize) -> usize {
    at.min(cells.saturating_sub(1))
}

/// Which way along an axis, by how much.
#[derive(Debug, Clone, Copy)]
enum Way {
    Back(usize),
    On(usize),
}

/// Where a motion lands on one axis, or `None` when there is nothing that way.
///
/// **This is the one rule, and every shape below is written in terms of it.**
/// A step is refused only when the cursor is *already* at that edge — so `5j`
/// three cells from the end goes to the end, because there was somewhere to
/// go, and `j` at the end refuses, because there was not.
///
/// That distinction is the whole edge mechanism. A refusal means "I have no
/// cell that way", which is exactly what the caller needs in order to say "then
/// it belongs to whatever is that way" — the sidebar, for a refused `h`. It
/// used to be the *shape* that decided: a `List` refused `h` and `l` outright
/// while a `Grid` clamped them, so a grid could be entered and never left.
fn along(at: usize, way: Way, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match way {
        Way::Back(n) => (at > 0).then(|| at.saturating_sub(n)),
        Way::On(n) => (at + 1 < len).then(|| clamp(at.saturating_add(n), len)),
    }
}

/// A cell number as vim writes it — counting from one — as an index.
fn nth(n: usize, cells: usize) -> usize {
    clamp(n.saturating_sub(1), cells)
}

/// Where the cursor can be, and how the cells are arranged: `columns` wide,
/// filled left to right and then down, so cell `n` is at row `n / columns` and
/// column `n % columns`.
///
/// **There is one shape, because a list and a row were special cases of it.**
/// There used to be three — a `List` that refused `h` and `l`, a `Row` that
/// refused `j` and `k`, and this — behind a `Navigate` trait so that a caller
/// could hold whichever it had. All three were the same arithmetic:
/// [`Grid::column`] is a grid one wide, where every cell is on the left edge
/// and the right edge at once, so both horizontal motions refuse without a
/// line of code saying so; [`Grid::row`] is a grid one tall, and the vertical
/// pair go the same way. Deleting the other two deleted the trait with them,
/// and a `Box<dyn Navigate>` per keypress with it.
///
/// So a caller says how its view is laid out and nothing else. The rule in
/// [`along`] does the rest.
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub cells: usize,
    pub columns: usize,
}

impl Grid {
    /// One column, top to bottom — a list: a sidebar, a table, a menu, a
    /// picker.
    pub fn column(cells: usize) -> Grid {
        Grid { cells, columns: 1 }
    }

    /// One row, left to right: the other degenerate case.
    pub fn row(cells: usize) -> Grid {
        Grid {
            cells,
            columns: cells.max(1),
        }
    }

    /// Where `motion` lands, starting from `from`, or `None` when there is
    /// nothing that way — which is what the caller reads as "then it belongs
    /// to whichever pane is that way".
    pub fn step(&self, from: usize, motion: Motion) -> Option<usize> {
        let columns = self.columns.max(1);
        let (row, column) = (from / columns, from % columns);
        // How many rows have anything in them, and how many cells this row has
        // — a short last row means a column can exist on one row and not the
        // next, and both edges have to be asked about the row they are on.
        let rows = self.cells.div_ceil(columns);
        let here = (self.cells.saturating_sub(row * columns)).min(columns);
        match motion {
            // Down and up keep the column, which is what makes a grid feel
            // like a grid: the cursor travels in a straight line rather than
            // wrapping through the end of a row.
            Motion::Down(n) => along(row, Way::On(n), rows).map(|r| clamp(r * columns + column, self.cells)),
            Motion::Up(n) => along(row, Way::Back(n), rows).map(|r| r * columns + column),
            // Left and right stay on their row, so `l` at the right-hand edge
            // does not drop to the next one — it is refused, like everything
            // else that has nowhere to go.
            Motion::Right(n) => along(column, Way::On(n), here).map(|c| row * columns + c),
            Motion::Left(n) => along(column, Way::Back(n), here).map(|c| row * columns + c),
            Motion::First => (self.cells > 0).then_some(0),
            Motion::Last => (self.cells > 0).then(|| self.cells - 1),
            Motion::To(n) => (self.cells > 0).then(|| nth(n, self.cells)),
        }
    }

    /// How far down its scrollable cell `at` sits: 0 at the top, 1 at the
    /// bottom.
    ///
    /// By *row*, because a row is what a grid scrolls past. It used to be
    /// `at / (cells - 1)` at the call site, which is the same answer for one
    /// column and wrong for six: the first card of the last row of forty-in-six
    /// scrolled to 92% of the way down instead of to the end.
    pub fn progress(&self, at: usize) -> f32 {
        let columns = self.columns.max(1);
        let rows = self.cells.div_ceil(columns);
        if rows < 2 {
            return 0.0;
        }
        ((at / columns) as f32 / (rows - 1) as f32).clamp(0.0, 1.0)
    }
}

/// Every key press no widget took, for the grammar.
///
/// Only presses: a release or a modifier change is not a command. A focused
/// text box takes its own keys, so none of them arrive here.
pub fn presses() -> iced::Subscription<(Key, Modifiers)> {
    iced::keyboard::listen().filter_map(|event| match event {
        iced::keyboard::Event::KeyPressed { key, modifiers, .. } => Some((key, modifiers)),
        _ => None,
    })
}

/// Where `/` and `n`/`N` land: the next of `labels` containing `needle`
/// (case-insensitively) after `at` — or before it, for a negative `delta` —
/// wrapping round the end the way vim does. `None` when nothing matches, which
/// is worth saying rather than stopping silently.
///
/// A search that has just been accepted should start from one *before* the
/// cursor, so that a match on the line you are on is found rather than skipped.
pub fn find(labels: &[String], needle: &str, at: usize, delta: isize) -> Option<usize> {
    let needle = needle.to_lowercase();
    let hits: Vec<usize> = labels
        .iter()
        .enumerate()
        .filter(|(_, l)| l.to_lowercase().contains(&needle))
        .map(|(i, _)| i)
        .collect();
    let next = match delta {
        d if d >= 0 => hits.iter().find(|&&h| h > at).or(hits.first()),
        _ => hits.iter().rev().find(|&&h| h < at).or(hits.last()),
    };
    next.copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keys: &mut Keys, s: &str) -> Option<Action> {
        let key = match s {
            "<Enter>" => Key::Named(Named::Enter),
            "<Esc>" => Key::Named(Named::Escape),
            "<Space>" => Key::Named(Named::Space),
            "<Tab>" => Key::Named(Named::Tab),
            c => Key::Character(c.into()),
        };
        keys.press(&key, Modifiers::default())
    }

    /// The count is the part that is easy to get wrong: it accumulates across
    /// presses and has to be spent exactly once.
    #[test]
    fn a_count_is_built_up_and_spent_once() {
        let mut keys = Keys::new();
        assert_eq!(press(&mut keys, "1"), None);
        assert_eq!(press(&mut keys, "2"), None);
        assert_eq!(keys.pending(), "12");
        assert_eq!(press(&mut keys, "j"), Some(Action::Move(Motion::Down(12))));
        // Spent: the next motion is a single step, not another twelve.
        assert_eq!(press(&mut keys, "j"), Some(Action::Move(Motion::Down(1))));
        assert_eq!(keys.pending(), "");
    }

    #[test]
    fn gg_is_the_top_and_g_is_the_end() {
        let mut keys = Keys::new();
        assert_eq!(press(&mut keys, "g"), None, "g waits for its second key");
        assert_eq!(keys.pending(), "g");
        assert_eq!(press(&mut keys, "g"), Some(Action::Move(Motion::First)));
        assert_eq!(press(&mut keys, "G"), Some(Action::Move(Motion::Last)));
        // A count turns either of them into "go to that cell".
        assert_eq!(press(&mut keys, "7"), None);
        assert_eq!(press(&mut keys, "G"), Some(Action::Move(Motion::To(7))));
    }

    #[test]
    fn escape_drops_what_was_half_typed() {
        let mut keys = Keys::new();
        press(&mut keys, "4");
        press(&mut keys, "g");
        assert_eq!(keys.pending(), "4g");
        assert_eq!(press(&mut keys, "<Esc>"), Some(Action::Cancel));
        assert_eq!(keys.pending(), "");
        assert_eq!(press(&mut keys, "j"), Some(Action::Move(Motion::Down(1))));
    }

    /// A search has to swallow the motion keys, or typing a composer's name
    /// walks the cursor down the list instead.
    #[test]
    fn a_search_swallows_everything_until_it_is_accepted() {
        let mut keys = Keys::new();
        assert_eq!(press(&mut keys, "/"), None);
        for c in ["j", "o", "h", "n"] {
            assert_eq!(press(&mut keys, c), None, "{c} is text, not a motion");
        }
        assert_eq!(keys.pending(), "/john");
        assert_eq!(press(&mut keys, "<Enter>"), Some(Action::Search("john".into())));
        assert_eq!(*keys.mode(), Mode::Normal);
        // And afterwards the same keys are motions again.
        assert_eq!(press(&mut keys, "j"), Some(Action::Move(Motion::Down(1))));
    }

    #[test]
    fn ctrl_d_and_ctrl_u_are_pages() {
        let mut keys = Keys::new();
        let ctrl = Modifiers::CTRL;
        let d = Key::Character("d".into());
        assert_eq!(keys.press(&d, ctrl), Some(Action::Move(Motion::Down(PAGE))));
    }

    /// The rule, stated where it is easiest to read: a step is refused only
    /// when the cursor is *already* at that edge.
    #[test]
    fn a_step_is_refused_only_at_the_edge_it_would_leave() {
        let list = Grid::column(5);
        assert_eq!(list.step(0, Motion::Down(2)), Some(2));
        // Three from the end with a count of nine: there was somewhere to go,
        // so it goes as far as it can rather than refusing.
        assert_eq!(list.step(1, Motion::Down(9)), Some(4), "clamped, not refused");
        // At the end there is nothing below, so this is the caller's problem.
        assert_eq!(list.step(4, Motion::Down(1)), None, "nothing below the end");
        assert_eq!(list.step(0, Motion::Up(1)), None, "nothing above the top");
        assert_eq!(list.step(2, Motion::First), Some(0));
        assert_eq!(list.step(2, Motion::Last), Some(4));
        assert_eq!(list.step(0, Motion::To(3)), Some(2), "cells count from one");
    }

    /// One column is a list, and it never had to be told to refuse `h` and
    /// `l`: every cell in it is on the left edge and the right edge at once,
    /// so the rule refuses both without a line of code about lists.
    ///
    /// This is what collapsed three shapes into one, so it is worth an
    /// assertion rather than a comment.
    #[test]
    fn one_column_refuses_the_horizontal_everywhere() {
        let list = Grid::column(5);
        for at in 0..5 {
            assert_eq!(list.step(at, Motion::Left(1)), None, "cell {at}");
            assert_eq!(list.step(at, Motion::Right(1)), None, "cell {at}");
        }
    }

    /// …and one row is the same fact turned ninety degrees.
    #[test]
    fn one_row_refuses_the_vertical_everywhere() {
        let row = Grid::row(3);
        assert_eq!(row.step(0, Motion::Right(2)), Some(2));
        assert_eq!(row.step(2, Motion::Right(1)), None, "nothing past the end");
        assert_eq!(row.step(0, Motion::Left(1)), None);
        for at in 0..3 {
            assert_eq!(row.step(at, Motion::Down(1)), None, "cell {at}");
            assert_eq!(row.step(at, Motion::Up(1)), None, "cell {at}");
        }
    }

    /// A grid with both axes answers `h` in the middle of a row and refuses it
    /// at column zero, which is how the cursor gets back out to the sidebar.
    #[test]
    fn a_grid_answers_in_the_middle_and_refuses_at_its_edges() {
        //  0 1 2
        //  3 4 5
        //  6 7
        let grid = Grid { cells: 8, columns: 3 };
        assert_eq!(grid.step(1, Motion::Down(1)), Some(4), "same column");
        assert_eq!(grid.step(4, Motion::Up(1)), Some(1));
        assert_eq!(grid.step(3, Motion::Right(1)), Some(4));
        // Down the short column: nothing sits under 5, so it lands on the last
        // cell rather than outside the grid.
        assert_eq!(grid.step(5, Motion::Down(1)), Some(7));
        assert_eq!(grid.step(0, Motion::Last), Some(7));

        // The four edges, each refused — and the left-hand one is the reason
        // this rule changed. A grid that clamped there was a page the keyboard
        // could walk into and never walk out of.
        assert_eq!(grid.step(3, Motion::Left(1)), None, "out to the sidebar");
        assert_eq!(grid.step(0, Motion::Left(1)), None);
        assert_eq!(grid.step(5, Motion::Right(1)), None, "the row ends here");
        assert_eq!(grid.step(7, Motion::Right(1)), None, "a short row ends sooner");
        assert_eq!(grid.step(1, Motion::Up(1)), None, "the top row");
        assert_eq!(grid.step(6, Motion::Down(1)), None, "the bottom row");
    }

    /// A grid scrolls by *row*, so the first card of the last row is the end
    /// of the scrollable and not 92% of the way down it.
    #[test]
    fn a_grid_scrolls_by_row_and_a_column_by_cell() {
        //  0 1 2
        //  3 4 5
        //  6 7
        let grid = Grid { cells: 8, columns: 3 };
        assert_eq!(grid.progress(0), 0.0);
        assert_eq!(grid.progress(2), 0.0, "still the first row");
        assert_eq!(grid.progress(4), 0.5, "the middle row of three");
        assert_eq!(grid.progress(6), 1.0, "the last row starts at the end");
        // The single-column default, which a grid had been borrowing: cell 6
        // of 8 is 86% of the way down a list and the end of this grid.
        assert_eq!(Grid::column(8).progress(6), 6.0 / 7.0);
        // One row is no scroll at all, rather than a division by zero.
        assert_eq!(Grid { cells: 2, columns: 3 }.progress(1), 0.0);
    }

    /// Nothing to point at is not somewhere to point: an empty shape refuses
    /// every motion rather than answering `Some(0)` for a cell it has not got.
    ///
    /// `List` used to answer `Some(0)` to `First` and `Last` when it was
    /// empty, and `Grid` refused — one of the small disagreements that came
    /// free of having three shapes. There is one answer now.
    #[test]
    fn an_empty_shape_has_nowhere_to_go() {
        for shape in [Grid::column(0), Grid::row(0), Grid { cells: 0, columns: 3 }] {
            for m in [
                Motion::Down(1),
                Motion::Up(1),
                Motion::Left(1),
                Motion::Right(1),
                Motion::First,
                Motion::Last,
                Motion::To(1),
            ] {
                assert_eq!(shape.step(0, m), None, "{m:?} on an empty shape");
            }
            assert_eq!(shape.progress(0), 0.0);
        }
    }

    /// `n` finds the next match and wraps; `N` goes back and wraps the other
    /// way; nothing matching is `None`. Falsified by dropping the `.or(…)`
    /// wraps: the third assertion comes back `None`.
    #[test]
    fn a_search_finds_the_next_match_and_wraps() {
        let labels: Vec<String> = ["Air", "Bach: Chaconne", "Handel", "bach: Air"].map(String::from).to_vec();
        assert_eq!(find(&labels, "bach", 0, 1), Some(1));
        assert_eq!(find(&labels, "BACH", 1, 1), Some(3), "case does not matter");
        assert_eq!(find(&labels, "bach", 3, 1), Some(1), "past the last it wraps to the first");
        assert_eq!(find(&labels, "bach", 1, -1), Some(3), "…and back past the first to the last");
        assert_eq!(find(&labels, "mozart", 0, 1), None);
    }
}
