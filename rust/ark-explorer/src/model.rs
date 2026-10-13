//! The explorer's state and what each message does to it — every decision
//! the component makes, tested without a window. The view only draws what
//! is decided here.

use std::collections::BTreeMap;

use arkui::vim::{self, Action, Grid, Motion};
use iced::keyboard::{Key, Modifiers};

use ark::ir::FnKind;
use ark::schema::Ty;
use ark::store::Row;
use ark::value::{TableName, Value};

use crate::cells;
use crate::data::{CrudVerbs, Source, Writer};
use crate::edit::{self, Route};
use crate::flash;
use crate::form::{self, Call, Called, Form, Picker};

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
    /// Every query and mutation of the module, by router, and the calls
    /// made this session.
    Functions,
    /// One function's form, and what calling it came to.
    Call(String),
}

impl Screen {
    /// `<Tab>`'s order: a table belongs with the tables, a form with the
    /// functions.
    fn next(&self) -> Screen {
        match self {
            Screen::Tables | Screen::Table(_) => Screen::Log,
            Screen::Log => Screen::Console,
            Screen::Console => Screen::Functions,
            Screen::Functions | Screen::Call(_) => Screen::Tables,
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
    /// Open function `i` of the list ([`form::callables`]) in its form.
    Function(usize),
    /// Type into the form's input at a path.
    FormText(Vec<usize>, String),
    /// Flip a bool, or an option between none and a value.
    FormToggle(Vec<usize>),
    /// Choose an enum's variant.
    FormChoose(Vec<usize>, usize),
    /// Add an item to a list; take one out.
    FormAdd(Vec<usize>),
    FormRemove(Vec<usize>),
    /// Put the form's cursor on a line.
    FormAt(usize),
    /// Open the picker for the id field at a path.
    Pick(Vec<usize>),
    PickFilter(String),
    PickAt(usize),
    /// Choose the picker's row at this place among those the filter leaves.
    Picked(usize),
    ClosePicker,
    /// Call the open form's function.
    Call,
    /// Call a recent call again, as it was; or put it back in its form.
    Again(usize),
    Recall(usize),
    /// A click on a cell of a query's answer.
    AnswerCell(usize, usize),
    /// The keyboard and the pointer went somewhere else — the app beside
    /// a docked explorer: an edit in progress ends as a click away ends it.
    Blur,
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
    /// The cursor on the list of functions.
    pub functions_at: usize,
    /// A form per function opened this session, so leaving one keeps what
    /// was typed.
    pub forms: BTreeMap<String, Form>,
    /// The picker, open on an id field of the form on screen.
    pub picker: Option<Picker>,
    /// Every call made this session, oldest first.
    pub calls: Vec<Call>,
    /// The keys walk a query's answer rather than its form, and where in it.
    pub in_answer: bool,
    pub answer_at: (usize, usize),
    /// The time the host last said, and the generation of its store it last
    /// said: what is lit fades against the first, and the second says when
    /// there is anything to diff.
    pub now: u64,
    pub generation: Option<u64>,
    /// What was last drawn of the table on screen, and of a query's answer.
    pub table_seen: flash::Tracker,
    pub answer_seen: flash::Tracker,
    /// Whether the keyboard is the explorer's: a host that docks it beside
    /// the app says when the app has it, and every cursor here is then drawn
    /// as a selection that has lost the keys rather than lit.
    pub has_keys: bool,
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
            functions_at: 0,
            forms: BTreeMap::new(),
            picker: None,
            calls: vec![],
            in_answer: false,
            answer_at: (0, 0),
            has_keys: true,
            now: 0,
            generation: None,
            table_seen: flash::Tracker::default(),
            answer_seen: flash::Tracker::default(),
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

    /// A table's page as the view draws it: the page's rows, every column
    /// as wide as its header and its first [`cells::SAMPLE`] rows.
    pub fn table_grid(&self, src: &Source, table: &str) -> cells::Grid {
        let columns = Explorer::columns(src, table);
        let key: Vec<String> = src.store.schema().lookup_table(table).map(|t| t.key.clone()).unwrap_or_default();
        let labels: Vec<String> = columns
            .iter()
            .map(|c| if key.contains(c) { format!("{c} \u{b7} key") } else { c.clone() })
            .collect();
        let all = src.store.scan(table);
        let (page, _) = self.page(all.len());
        let from = (page * PAGE_ROWS).min(all.len());
        let to = (from + PAGE_ROWS).min(all.len());
        let sample = &all[..all.len().min(cells::SAMPLE)];
        cells::Grid::of_rows(&columns, &labels, &all[from..to], sample)
    }

    /// The whole of the cell under the cursor, on a table's screen: what the
    /// detail line says.
    pub fn detail(&self, src: &Source) -> Option<String> {
        let (t, row) = self.row_under(src)?;
        let column = Explorer::columns(src, &t).get(self.column_at)?.clone();
        Some(format!("{column}: {}", row.get(&column).map(edit::cell_text).unwrap_or_default()))
    }

    /// What a host says on every tick (`docs/plan-guards.md` D4): the time,
    /// and the generation of its store — a number it moves whenever the store
    /// moved. When it moved, the table on screen is diffed by key against
    /// what was last drawn, and a query's answer on screen is run again with
    /// its arguments and diffed the same way; what changed is lit and fades.
    /// Something shown for the first time is only remembered.
    pub fn observe(&mut self, src: &Source, generation: u64, now: u64) {
        self.now = now;
        let moved = self.generation.is_some_and(|g| g != generation);
        self.generation = Some(generation);
        match self.screen.clone() {
            Screen::Table(t) => {
                let columns = Explorer::columns(src, &t);
                let what = format!("table {t}");
                if moved || !self.table_seen.tracks(&what, &columns) {
                    let rows = table_rows(src, &t, &columns);
                    self.table_seen.observe(&what, &columns, rows, now);
                }
            }
            Screen::Call(f) => {
                let Some(i) = self.calls.iter().rposition(|c| c.function == f) else {
                    return;
                };
                if moved && self.calls[i].kind == FnKind::Query {
                    let args = self.calls[i].args.clone();
                    self.calls[i].called = match query(src, &f, &args) {
                        Ok(v) => Called::Answer(v),
                        Err(why) => Called::Failed(why),
                    };
                }
                if let Called::Answer(v) = &self.calls[i].called {
                    let g = cells::Grid::of_value(v);
                    let k = flash::key_column(&g.rows, g.columns.len());
                    let what = format!("answer {i} {k:?}");
                    if moved || !self.answer_seen.tracks(&what, &g.columns) {
                        let rows = g.rows.iter().map(|r| (answer_key(r, k), r.clone())).collect();
                        self.answer_seen.observe(&what, &g.columns, rows, now);
                    }
                }
            }
            _ => {}
        }
        self.table_seen.prune(now);
        self.answer_seen.prune(now);
    }

    /// Whether anything is still lit, so a host that redraws only when
    /// asked keeps a clock running.
    pub fn fading(&self) -> bool {
        self.table_seen.fading(self.now) || self.answer_seen.fading(self.now)
    }

    /// A table's page as drawn, lit: its grid with the rows that went put back
    /// where they were while they fade, and how each drawn row is lit.
    pub fn lit_table(&self, src: &Source, t: &str) -> (cells::Grid, Vec<flash::RowLight>) {
        let grid = self.table_grid(src, t);
        let all = src.store.scan(t).len();
        let (page, pages) = self.page(all);
        let from = (page * PAGE_ROWS).min(all);
        let tbl = src.store.schema().lookup_table(t);
        let keys: Vec<flash::Key> = src.store.scan(t)[from..(from + PAGE_ROWS).min(all)]
            .iter()
            .map(|r| tbl.map(|tbl| tbl.key_of(r)).unwrap_or_default())
            .collect();
        flash::light(&grid, &keys, from, page + 1 >= pages, &self.table_seen, self.now)
    }

    /// The last call's answer, lit, where it was a query's.
    pub fn lit_answer(&self) -> Option<(cells::Grid, Vec<flash::RowLight>)> {
        let g = self.answer_grid()?;
        let k = flash::key_column(&g.rows, g.columns.len());
        let keys: Vec<flash::Key> = g.rows.iter().map(|r| answer_key(r, k)).collect();
        Some(flash::light(&g, &keys, 0, true, &self.answer_seen, self.now))
    }

    /// Every function a person can call, by router.
    pub fn callables(src: &Source) -> Vec<form::Callable> {
        form::callables(src.module)
    }

    /// The form of the function on screen.
    pub fn form(&self) -> Option<&Form> {
        let Screen::Call(f) = &self.screen else { return None };
        self.forms.get(f)
    }

    fn form_mut(&mut self) -> Option<&mut Form> {
        let Screen::Call(f) = &self.screen else { return None };
        self.forms.get_mut(f)
    }

    /// The last call of the function on screen, if it has been called.
    pub fn last_call(&self) -> Option<&Call> {
        let Screen::Call(f) = &self.screen else { return None };
        self.calls.iter().rev().find(|c| &c.function == f)
    }

    /// The last call's answer, as a table, where it was a query's.
    pub fn answer_grid(&self) -> Option<cells::Grid> {
        match &self.last_call()?.called {
            Called::Answer(v) => Some(cells::Grid::of_value(v)),
            _ => None,
        }
    }

    /// What a call came to, as a person would say it: a mutation's verdict
    /// asked of the host now, so a server's answer is said when it arrives.
    pub fn outcome(call: &Call, w: &dyn Writer) -> String {
        match &call.called {
            Called::Answer(v) => match v {
                Value::List(xs) => format!("{} row{}", xs.len(), if xs.len() == 1 { "" } else { "s" }),
                _ => "one value".into(),
            },
            Called::Failed(why) => why.clone(),
            Called::Authored(t) => w.verdict(t).describe(),
            Called::Refused(why) => format!("refused: {why}"),
        }
    }

    /// The rows the picker offers, labelled, and which of them the filter
    /// leaves.
    pub fn pick_rows(&self, src: &Source) -> (Vec<form::Pickable>, Vec<usize>) {
        let Some(p) = &self.picker else { return (vec![], vec![]) };
        let all = form::pickables(src.store, &p.table);
        let shown = form::filter(&all, &p.filter);
        (all, shown)
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
        // A spreadsheet's rule: anything that is not the edit itself ends
        // it — submitted as <Enter> would when it changed and reads, dropped
        // as <Esc> would otherwise — before it is done, so the editor never
        // stays on the old row and never lands on the new one.
        if !matches!(msg, Msg::Key(..) | Msg::EditText(_) | Msg::Commit | Msg::CancelEdit) {
            self.end_edit(src, w);
        }
        match msg {
            Msg::Blur => {}
            Msg::Key(key, mods) => {
                let searching = matches!(self.keys.mode(), vim::Mode::Search(_));
                let action = self.keys.press(&key, mods);
                // A `/` search on the picker filters as it is typed.
                if let (Some(p), vim::Mode::Search(q)) = (&mut self.picker, self.keys.mode()) {
                    if p.filter != *q {
                        p.filter = q.clone();
                        p.at = 0;
                    }
                }
                let Some(action) = action else {
                    return Outcome::Stay;
                };
                // `<Esc>` out of a search drops what it typed, and leaves the
                // picker up.
                if let (true, Action::Cancel, Some(p)) = (searching, &action, &mut self.picker) {
                    p.filter.clear();
                    p.at = 0;
                    return Outcome::Stay;
                }
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
            Msg::Function(i) => self.open_function(src, i),
            Msg::FormText(path, text) => self.on_form(src, |f| f.set_text(&path, text)),
            Msg::FormToggle(path) => self.on_form(src, |f| f.toggle(&path)),
            Msg::FormChoose(path, i) => self.on_form(src, |f| f.choose(&path, i)),
            Msg::FormAdd(path) => self.on_form(src, |f| f.add(&path)),
            Msg::FormRemove(path) => self.on_form(src, |f| f.remove(&path)),
            Msg::FormAt(i) => {
                self.in_answer = false;
                if let Some(f) = self.form_mut() {
                    f.at = i;
                }
            }
            Msg::Pick(path) => self.open_picker(src, path),
            Msg::PickFilter(text) => {
                if let Some(p) = &mut self.picker {
                    p.filter = text;
                    p.at = 0;
                }
            }
            Msg::PickAt(i) => {
                if let Some(p) = &mut self.picker {
                    p.at = i;
                }
            }
            Msg::Picked(i) => self.picked(src, i),
            Msg::ClosePicker => self.picker = None,
            Msg::Call => self.call(src, w),
            Msg::Again(i) => self.again(src, w, i),
            Msg::Recall(i) => self.recall(src, i),
            Msg::AnswerCell(r, c) => {
                self.in_answer = true;
                self.answer_at = (r, c);
            }
        }
        Outcome::Stay
    }

    /// Open function `i` of the list in its form, kept from before if it was
    /// opened this session.
    fn open_function(&mut self, src: &Source, i: usize) {
        let Some(c) = Explorer::callables(src).into_iter().nth(i) else { return };
        self.functions_at = i;
        self.forms.entry(c.name.clone()).or_insert_with(|| Form::new(&c));
        self.picker = None;
        self.in_answer = false;
        self.answer_at = (0, 0);
        self.note = format!(
            "{}: j and k walk the fields, <Enter> chooses, toggles or adds, r calls, <Esc> goes back",
            c.name
        );
        self.screen = Screen::Call(c.name);
    }

    /// Change the form on screen, and check it again.
    fn on_form(&mut self, src: &Source, change: impl FnOnce(&mut Form)) {
        let Some(f) = self.form_mut() else { return };
        change(f);
        let n = f.slots().len();
        f.at = f.at.min(n);
        f.validate(src.module, src.store.schema(), &src.ctx, src.store);
    }

    /// The picker, on the id field at `path`: the rows of its table.
    fn open_picker(&mut self, src: &Source, path: Vec<usize>) {
        let Some(f) = self.form_mut() else { return };
        let Some((Ty::Id(table), _)) = f.at_mut(&path) else { return };
        let table = table.clone();
        if let Some(i) = f.slots().iter().position(|s| s.path == path) {
            f.at = i;
        }
        self.note = match form::pickables(src.store, &table).len() {
            0 => format!("{table} holds no rows here"),
            n => format!(
                "{n} row{} of {table}: j and k walk, / filters, <Enter> chooses",
                if n == 1 { "" } else { "s" }
            ),
        };
        self.picker = Some(Picker {
            path,
            table,
            filter: String::new(),
            at: 0,
        });
    }

    /// Choose the picker's row at `i` among those its filter leaves: its id
    /// goes in the field.
    fn picked(&mut self, src: &Source, i: usize) {
        let (all, shown) = self.pick_rows(src);
        let Some(p) = self.picker.take() else { return };
        let Some(row) = shown.get(i).map(|&k| &all[k]) else {
            self.note = format!("nothing of {} to choose", p.table);
            self.picker = Some(p);
            return;
        };
        let text = form::id_text(&row.id);
        self.note = format!("{}: {}", p.table, row.line());
        self.on_form(src, |f| f.set_text(&p.path, text));
    }

    /// Call the form on screen: checked first, with the module's messages;
    /// then a query run on the host's store, or a mutation authored through
    /// the host's writer as a button would author it.
    fn call(&mut self, src: &Source, w: &mut dyn Writer) {
        let Some(f) = self.form_mut() else { return };
        let Some(args) = f.validate(src.module, src.store.schema(), &src.ctx, src.store) else {
            let said: Vec<String> = f.messages.iter().map(|(_, m)| m.clone()).collect();
            self.note = format!("not called: {}", said.join("; "));
            return;
        };
        let (name, kind) = (f.function.clone(), f.kind);
        self.run_call(src, w, name, kind, args);
    }

    fn run_call(&mut self, src: &Source, w: &mut dyn Writer, function: String, kind: FnKind, args: ark::eval::Args) {
        let called = match kind {
            FnKind::Query => match query(src, &function, &args) {
                Ok(v) => Called::Answer(v),
                Err(why) => Called::Failed(why),
            },
            _ => match w.author(&function, args.clone()) {
                Ok(t) => Called::Authored(t),
                Err(why) => Called::Refused(why),
            },
        };
        let call = Call {
            function,
            kind,
            args,
            called,
        };
        self.note = format!("{}: {}", call.function, Explorer::outcome(&call, w));
        self.in_answer = matches!(&call.called, Called::Answer(_));
        self.answer_at = (0, 0);
        self.calls.push(call);
    }

    /// Call recent call `i` again, with the arguments it had.
    fn again(&mut self, src: &Source, w: &mut dyn Writer, i: usize) {
        let Some(c) = self.calls.get(i).cloned() else { return };
        if let Some(callable) = Explorer::callables(src).into_iter().find(|x| x.name == c.function) {
            self.forms.insert(c.function.clone(), Form::filled(&callable, &c.args));
            self.screen = Screen::Call(c.function.clone());
        }
        self.picker = None;
        self.run_call(src, w, c.function, c.kind, c.args);
    }

    /// Put recent call `i`'s arguments back in its form, to change and call.
    fn recall(&mut self, src: &Source, i: usize) {
        let Some(c) = self.calls.get(i).cloned() else { return };
        let Some(callable) = Explorer::callables(src).into_iter().find(|x| x.name == c.function) else {
            return;
        };
        let mut f = Form::filled(&callable, &c.args);
        f.validate(src.module, src.store.schema(), &src.ctx, src.store);
        self.forms.insert(c.function.clone(), f);
        self.picker = None;
        self.in_answer = false;
        self.screen = Screen::Call(c.function);
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
        self.picker = None;
        self.screen = s;
    }

    /// A finished command of the vim grammar (`arkui::vim`).
    pub fn act(&mut self, action: Action, src: &Source, w: &mut dyn Writer) -> Outcome {
        // The picker, while it is open, has the keys: it walks, filters,
        // chooses and closes, and nothing behind it moves.
        if self.picker.is_some() {
            self.act_picker(action, src);
            return Outcome::Stay;
        }
        if let Screen::Call(_) = self.screen {
            if let Some(o) = self.act_form(action.clone(), src, w) {
                return o;
            }
        }
        // The editor is open but its box does not have the keys: <Enter>
        // submits it, <Esc> drops it, and anything else ends it first.
        if self.editing.is_some() {
            match action {
                Action::Activate => {
                    self.commit(src, w);
                    return Outcome::Stay;
                }
                Action::Cancel => {}
                _ => self.end_edit(src, w),
            }
        }
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
                Screen::Functions => self.open_function(src, self.functions_at),
                Screen::Log | Screen::Call(_) => {}
            },
            Action::Cancel => {
                if self.editing.take().is_some() {
                    return Outcome::Stay;
                }
                match self.screen {
                    Screen::Table(_) => self.show(Screen::Tables),
                    Screen::Call(_) => self.show(Screen::Functions),
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
            Action::Key('f') => self.show(Screen::Functions),
            // Again: the last call, as it was.
            Action::Key('.') if matches!(self.screen, Screen::Functions) => {
                if let Some(i) = self.calls.len().checked_sub(1) {
                    self.again(src, w, i);
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// The keys on a form: `j` and `k` walk its lines (and a query's answer,
    /// once it is called, until `<Esc>` hands them back), `<Enter>` does
    /// what the line under the cursor does, `x` takes an item out of its
    /// list, `r` calls, `.` calls the last call again. `None` for what the
    /// screens share.
    fn act_form(&mut self, action: Action, src: &Source, w: &mut dyn Writer) -> Option<Outcome> {
        if self.in_answer {
            if let Some(g) = self.answer_grid() {
                match action {
                    Action::Move(m @ (Motion::Left(_) | Motion::Right(_))) => {
                        self.answer_at.1 = Grid::row(g.columns.len()).step(self.answer_at.1, m).unwrap_or(self.answer_at.1);
                        return Some(Outcome::Stay);
                    }
                    Action::Move(m) => {
                        self.answer_at.0 = Grid::column(g.rows.len()).step(self.answer_at.0, m).unwrap_or(self.answer_at.0);
                        return Some(Outcome::Stay);
                    }
                    Action::Cancel => {
                        self.in_answer = false;
                        return Some(Outcome::Stay);
                    }
                    _ => {}
                }
            }
        }
        let f = self.form_mut()?;
        let slots = f.slots();
        match action {
            Action::Move(m @ (Motion::Up(_) | Motion::Down(_) | Motion::First | Motion::Last | Motion::To(_))) => {
                // One past the last line is the button that calls.
                f.at = Grid::column(slots.len() + 1).step(f.at, m).unwrap_or(f.at);
                Some(Outcome::Stay)
            }
            Action::Activate | Action::Toggle => {
                let Some(slot) = slots.get(f.at) else {
                    self.call(src, w);
                    return Some(Outcome::Stay);
                };
                if matches!(slot.ty, Ty::Id(_)) && action == Action::Activate {
                    self.open_picker(src, slot.path.clone());
                } else {
                    let path = slot.path.clone();
                    self.on_form(src, |f| f.activate(&path));
                }
                Some(Outcome::Stay)
            }
            Action::Key('x') => {
                if let Some(slot) = slots.get(f.at).filter(|s| s.item) {
                    let path = slot.path.clone();
                    self.on_form(src, |f| f.remove(&path));
                }
                Some(Outcome::Stay)
            }
            Action::Key('r') => {
                self.call(src, w);
                Some(Outcome::Stay)
            }
            Action::Key('.') => {
                let Screen::Call(name) = &self.screen else { return None };
                if let Some(i) = self.calls.iter().rposition(|c| &c.function == name) {
                    self.again(src, w, i);
                }
                Some(Outcome::Stay)
            }
            _ => None,
        }
    }

    /// The keys on the picker: walk, `/` to filter (as it is typed), choose,
    /// close.
    fn act_picker(&mut self, action: Action, src: &Source) {
        let (_, shown) = self.pick_rows(src);
        let Some(p) = &mut self.picker else { return };
        match action {
            Action::Move(m) => p.at = Grid::column(shown.len()).step(p.at, m).unwrap_or(p.at),
            Action::Activate => {
                let at = p.at;
                self.picked(src, at);
            }
            Action::Search(q) => {
                p.filter = q;
                p.at = 0;
            }
            Action::Cancel => self.picker = None,
            _ => {}
        }
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
            Screen::Functions => self.functions_at = column(Explorer::callables(src).len(), self.functions_at),
            Screen::Console | Screen::Call(_) => {}
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

    /// End the edit in progress, as a click elsewhere in a spreadsheet does:
    /// a value that changed and reads as its column's is submitted exactly as
    /// <Enter> submits it; one that did not change, or does not read, is
    /// dropped as <Esc> drops it. Either way the editor is gone — a refusal
    /// is said, and not left open on a row the cursor has left.
    pub fn end_edit(&mut self, src: &Source, w: &mut dyn Writer) {
        let Some(e) = &self.editing else { return };
        let read = src
            .store
            .schema()
            .lookup_table(&e.table)
            .and_then(|t| t.column(&e.column))
            .map(|c| edit::parse_cell(c, &e.text));
        let old = src.store.get(&e.table, &e.key);
        match (read, old) {
            (Some(Ok(v)), Some(old)) if old.get(&e.column) != Some(&v) => {
                self.commit(src, w);
                self.editing = None;
            }
            (Some(Err(why)), _) => {
                self.note = format!("edit dropped, {why}");
                self.editing = None;
            }
            _ => self.editing = None,
        }
    }

    /// Where the row being edited is drawn, among a table's rows: found by
    /// its key, not by where the cursor is.
    pub fn editing_at(&self, src: &Source) -> Option<usize> {
        let e = self.editing.as_ref()?;
        let tbl = src.store.schema().lookup_table(&e.table)?;
        src.store.scan(&e.table).iter().position(|r| tbl.key_of(r) == e.key)
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

/// A query by name, run on the host's store as who the explorer reads as —
/// the console's path, with the arguments a form read.
fn query(src: &Source, name: &str, args: &ark::eval::Args) -> Result<Value, String> {
    let f = src.module.lookup_function(name).ok_or_else(|| format!("no function {name}"))?;
    if f.kind != FnKind::Query {
        return Err(format!("{name} is a {}, not a query", f.kind.name()));
    }
    let c = ark::hash::closure(src.module, f);
    ark::eval::query_closure(src.store.schema(), &c, &src.ctx, args, src.store).map_err(|e| match e {
        ark::eval::EvalFault::Verdict(r) => format!("refused: {}", ark::protocol::refusal_text(&r)),
        ark::eval::EvalFault::Bug(b) => format!("a bug: {b:?}"),
    })
}

/// A table's rows as the diff takes them: each its key and its columns'
/// values, in the store's order.
fn table_rows(src: &Source, t: &str, columns: &[String]) -> Vec<(flash::Key, Vec<Value>)> {
    let tbl = src.store.schema().lookup_table(t);
    src.store
        .scan(t)
        .iter()
        .map(|r| {
            let key = tbl.map(|tbl| tbl.key_of(r)).unwrap_or_default();
            (key, columns.iter().map(|c| r.get(c).cloned().unwrap_or(Value::Null)).collect())
        })
        .collect()
}

/// What names a row of an answer: its key column's value, or all of it.
fn answer_key(row: &[Value], k: Option<usize>) -> flash::Key {
    match k {
        Some(c) => vec![row[c].clone()],
        None => row.to_vec(),
    }
}
