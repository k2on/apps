//! The explorer's state and what each message does to it — every decision
//! the component makes, tested without a window. The view only draws what
//! is decided here.

use arkui::vim::{self, Action, Grid, Motion};
use iced::keyboard::{Key, Modifiers};

use ark::store::Row;
use ark::value::{TableName, Value};

use crate::data::{CrudVerbs, Source, Writer};
use crate::edit::{self, Route};

/// How many rows of a table are drawn at once: the page the cursor is on.
pub const PAGE_ROWS: usize = 50;

/// What is on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Every table the store has, with how many rows and what CRUD it
    /// exposes.
    Tables,
    /// One table's rows, a page at a time.
    Table(TableName),
    /// A query by name, or a plan, run read-only.
    Console,
    /// The log: its lines, the connections, the `Verify` answers.
    Log,
}

impl Screen {
    /// `<Tab>`'s order: a table belongs with the tables.
    fn next(&self) -> Screen {
        match self {
            Screen::Tables | Screen::Table(_) => Screen::Log,
            Screen::Log => Screen::Console,
            Screen::Console => Screen::Tables,
        }
    }
}

/// A cell being edited: the row by its key, so a row that moves under the
/// editor is still the one edited, and one taken away meanwhile is said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editing {
    pub table: TableName,
    pub key: Vec<Value>,
    pub column: String,
    pub text: String,
}

/// What a host is told back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    /// `<Esc>` with nothing left to close: the host closes the explorer.
    Close,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// A key no widget took.
    Key(Key, Modifiers),
    Show(Screen),
    Open(TableName),
    /// A click on a cell: the row's place in the table, the column's.
    Cell(usize, usize),
    /// A click on a line of a list: the tables, or the log.
    At(usize),
    /// Edit the cell under the cursor.
    Edit,
    EditText(String),
    Commit,
    CancelEdit,
    /// Delete the row under the cursor.
    Delete,
    /// The switch to write raw.
    ToggleRaw,
    Query(String),
    Args(String),
    Run,
    Close,
}

/// The explorer: where it is and what it is in the middle of. Everything
/// it shows is read from the [`Source`] it is handed each time.
#[derive(Debug)]
pub struct Explorer {
    pub screen: Screen,
    /// The cursor on the tables, on a table's rows and columns, and on the log.
    pub tables_at: usize,
    pub row_at: usize,
    pub column_at: usize,
    pub log_at: usize,
    pub editing: Option<Editing>,
    /// Write raw even where the table exposes CRUD: the visible switch,
    /// drawn only where the writer can.
    pub raw: bool,
    pub query: String,
    pub args: String,
    pub answer: Option<Result<String, String>>,
    /// The last thing worth saying: what an edit became, or why not.
    pub note: String,
    pub keys: vim::Keys,
}

impl Default for Explorer {
    fn default() -> Explorer {
        Explorer::new()
    }
}

impl Explorer {
    pub fn new() -> Explorer {
        Explorer {
            screen: Screen::Tables,
            tables_at: 0,
            row_at: 0,
            column_at: 0,
            log_at: 0,
            editing: None,
            raw: false,
            query: String::new(),
            args: String::new(),
            answer: None,
            note: String::new(),
            keys: vim::Keys::new(),
        }
    }

    /// The tables the store has, in its schema's order.
    pub fn tables(src: &Source) -> Vec<TableName> {
        src.store.schema().tables().map(|t| t.name.clone()).collect()
    }

    /// The CRUD exposed for `table`, as the writer says it.
    pub fn verbs(w: &dyn Writer, table: &str) -> Option<CrudVerbs> {
        w.exposed().into_iter().find(|(t, _)| t == table).map(|(_, v)| v)
    }

    /// The columns of `table` as the store has them: a column a server keeps
    /// is drawn where the store holds it, and one a person's union leaves
    /// out is not.
    pub fn columns(src: &Source, table: &str) -> Vec<String> {
        src.store
            .schema()
            .lookup_table(table)
            .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default()
    }

    /// The row under the cursor, on a table's screen.
    pub fn row_under(&self, src: &Source) -> Option<(TableName, Row)> {
        let Screen::Table(t) = &self.screen else { return None };
        src.store.scan(t).into_iter().nth(self.row_at).map(|r| (t.clone(), r))
    }

    /// Which page of the rows the cursor is on, and how many there are.
    pub fn page(&self, rows: usize) -> (usize, usize) {
        (self.row_at / PAGE_ROWS, rows.div_ceil(PAGE_ROWS).max(1))
    }

    /// What an edit of a cell of `table` would go through here, as a person
    /// would say it — so the switch says what it switches.
    pub fn writes_through(&self, w: &dyn Writer, table: &str) -> String {
        match (
            Explorer::verbs(w, table).filter(|v| v.may_author && (v.update || v.put)),
            self.raw && w.can_raw(),
        ) {
            (Some(v), false) => format!("edits go through {}", if v.update { "update" } else { "put" }),
            _ if w.can_raw() => "edits are written raw".into(),
            _ => "read-only here".into(),
        }
    }

    /// Do what `msg` asks, against what `src` holds, writing through `w`.
    pub fn update(&mut self, msg: Msg, src: &Source, w: &mut dyn Writer) -> Outcome {
        match msg {
            Msg::Key(key, mods) => {
                let Some(action) = self.keys.press(&key, mods) else {
                    return Outcome::Stay;
                };
                return self.act(action, src, w);
            }
            Msg::Show(s) => self.show(s),
            Msg::Open(t) => self.show(Screen::Table(t)),
            Msg::Cell(r, c) => {
                self.row_at = r;
                self.column_at = c;
            }
            Msg::At(i) => match self.screen {
                Screen::Tables => self.tables_at = i,
                Screen::Log => self.log_at = i,
                _ => {}
            },
            Msg::Edit => self.edit(src),
            Msg::EditText(t) => {
                if let Some(e) = &mut self.editing {
                    e.text = t;
                }
            }
            Msg::Commit => self.commit(src, w),
            Msg::CancelEdit => self.editing = None,
            Msg::Delete => self.delete(src, w),
            Msg::ToggleRaw => {
                if w.can_raw() {
                    self.raw = !self.raw;
                    self.note = if self.raw {
                        "writing raw"
                    } else {
                        "writing through the domain where it can"
                    }
                    .into();
                } else {
                    self.raw = false;
                    self.note = "not the authority: there is no raw write here".into();
                }
            }
            Msg::Query(q) => self.query = q,
            Msg::Args(a) => self.args = a,
            Msg::Run => {
                self.answer = Some(edit::console(src, &self.query, &self.args).map(|v| ark::json::json(&v)));
            }
            Msg::Close => return Outcome::Close,
        }
        Outcome::Stay
    }

    fn show(&mut self, s: Screen) {
        if let Screen::Table(t) = &s {
            if self.screen != s {
                self.row_at = 0;
                self.column_at = 0;
                self.note = format!("{t}: e edits a cell, x deletes a row, <Esc> goes back");
            }
        }
        self.editing = None;
        self.screen = s;
    }

    /// A finished command of the vim grammar (`arkui::vim`).
    pub fn act(&mut self, action: Action, src: &Source, w: &mut dyn Writer) -> Outcome {
        match action {
            Action::Move(m) => self.travel(m, src),
            Action::Activate => match &self.screen {
                Screen::Tables => {
                    if let Some(t) = Explorer::tables(src).get(self.tables_at).cloned() {
                        self.show(Screen::Table(t));
                    }
                }
                Screen::Table(_) => self.edit(src),
                Screen::Console => {
                    return self.update(Msg::Run, src, w);
                }
                Screen::Log => {}
            },
            Action::Cancel => {
                if self.editing.take().is_some() {
                    return Outcome::Stay;
                }
                match self.screen {
                    Screen::Table(_) => self.show(Screen::Tables),
                    _ => return Outcome::Close,
                }
            }
            Action::Cycle => {
                let next = self.screen.next();
                self.show(next);
            }
            Action::Key('e') => self.edit(src),
            Action::Key('x') => self.delete(src, w),
            Action::Key('R') => return self.update(Msg::ToggleRaw, src, w),
            Action::Key('1') => self.show(Screen::Tables),
            Action::Key('2') => self.show(Screen::Log),
            Action::Key('3') => self.show(Screen::Console),
            _ => {}
        }
        Outcome::Stay
    }

    // A motion in whichever list is on screen; on a table `h` and `l` walk
    // the columns and `j` and `k` the rows, each a grid of its own.
    fn travel(&mut self, m: Motion, src: &Source) {
        let column = |n: usize, at: usize| Grid::column(n).step(at, m).unwrap_or(at);
        match &self.screen {
            Screen::Tables => self.tables_at = column(Explorer::tables(src).len(), self.tables_at),
            Screen::Log => self.log_at = column(src.log.lines().len(), self.log_at),
            Screen::Table(t) => match m {
                Motion::Left(_) | Motion::Right(_) => {
                    let n = Explorer::columns(src, t).len();
                    self.column_at = Grid::row(n).step(self.column_at, m).unwrap_or(self.column_at);
                }
                _ => {
                    let n = src.store.scan(t).len();
                    self.row_at = column(n, self.row_at);
                }
            },
            Screen::Console => {}
        }
    }

    /// Start editing the cell under the cursor, with its text.
    fn edit(&mut self, src: &Source) {
        let Some((t, row)) = self.row_under(src) else { return };
        let Some(column) = Explorer::columns(src, &t).get(self.column_at).cloned() else {
            return;
        };
        let key = src.store.schema().lookup_table(&t).map(|tbl| tbl.key_of(&row)).unwrap_or_default();
        let text = row.get(&column).map(edit::cell_text).unwrap_or_default();
        self.editing = Some(Editing { table: t, key, column, text });
    }

    /// Commit the edit in progress: its text read as the column's value, the
    /// row as it stands now, and the change routed and handed to the writer.
    fn commit(&mut self, src: &Source, w: &mut dyn Writer) {
        let Some(e) = self.editing.take() else { return };
        let outcome = (|| {
            let tbl = src.store.schema().lookup_table(&e.table).ok_or_else(|| format!("no table {}", e.table))?;
            let col = tbl.column(&e.column).ok_or_else(|| format!("{} has no column {}", e.table, e.column))?;
            let value = edit::parse_cell(col, &e.text)?;
            let old = src.store.get(&e.table, &e.key).ok_or_else(|| format!("the row of {} is gone", e.table))?;
            if old.get(&e.column) == Some(&value) {
                return Ok("nothing to change".to_string());
            }
            let verbs = Explorer::verbs(w, &e.table);
            let via = edit::Via {
                verbs: verbs.as_ref(),
                can_raw: w.can_raw(),
                force_raw: self.raw,
            };
            let route = edit::route_edit(src.schema, via, &e.table, &old, &e.column, value)?;
            let said = route.describe();
            route.send(w)?;
            Ok(format!("{}.{} written, {said}", e.table, e.column))
        })();
        self.note = match outcome {
            Ok(s) => s,
            Err(why) => {
                // Kept, so a typo is fixed rather than typed again.
                self.editing = Some(e);
                why
            }
        };
    }

    /// Delete the row under the cursor, routed as an edit is.
    fn delete(&mut self, src: &Source, w: &mut dyn Writer) {
        let Some((t, row)) = self.row_under(src) else { return };
        let verbs = Explorer::verbs(w, &t);
        let via = edit::Via {
            verbs: verbs.as_ref(),
            can_raw: w.can_raw(),
            force_raw: self.raw,
        };
        let outcome = edit::route_delete(src.schema, via, &t, &row).and_then(|route: Route| {
            let said = route.describe();
            route.send(w).map(|()| said)
        });
        self.note = match outcome {
            Ok(said) => format!("a row of {t} deleted, {said}"),
            Err(why) => why,
        };
    }
}
