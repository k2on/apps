//! The explorer's state and what each message does to it — every decision
//! the component makes, tested without a window; the view only draws what
//! is decided here. One generic browser, driven by the domain's schema and
//! its functions' descriptions (`docs/plan-guards.md` D4):
//!
//! - **tables → grids**: a table's columns from the schema, its rows from
//!   the store a page at a time, sortable by any column (`s`, or a click on
//!   a heading); a cell is edited and a row deleted through
//!   [`crate::edit`]'s routing;
//! - **functions → a call screen by name**: the form of its input
//!   ([`crate::form`]), an id field chosen from its table's rows by a
//!   picker, and a query's answer as a grid;
//! - **the log → its entries**, each with its facts and where it stands,
//!   pending intents among them, and the connections.
//!
//! What changes in the host's store while a table is on screen is lit and
//! fades over [`FADE_MS`]: by the row's key and the column, never by place;
//! the host says the time and its store's generation through
//! [`Explorer::observe`].

use std::collections::BTreeMap;

use arkui::vim::{self, Action, Grid as Steps, Motion};
use iced::keyboard::{Key, Modifiers};

use crate::data::{CrudVerbs, Source, Ticket, Writer};
use crate::edit::{self, Via};
use crate::form::{self, Callable, Form, Kind, Picker};
use crate::value::{self, Ty, Value};

/// How many rows of a table are drawn at once: the page the cursor is on.
pub const PAGE_ROWS: usize = 50;
/// How long a change stays lit, fading all the while.
pub const FADE_MS: u64 = 2000;
/// A column's narrowest and widest, in characters: past the widest a value
/// loses its middle to an ellipsis, and the detail line says it whole.
pub const MIN_CHARS: usize = 6;
pub const MAX_CHARS: usize = 36;
/// How many hex digits of an id a cell draws.
pub const ID_DIGITS: usize = 8;

/// What is on screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    /// Every table of the store's space, with how many rows and what CRUD
    /// it exposes.
    #[default]
    Tables,
    Table(String),
    Log,
    /// Every query and mutation of the module, by router.
    Functions,
    /// One function's form, and what calling it came to.
    Call(String),
}

/// A cell being edited: the row by its key, so a row that moves under the
/// editor is still the one edited, and one taken away meanwhile is said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editing {
    pub table: String,
    pub key: Vec<u8>,
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
    Open(String),
    /// A click on a cell: the row's place in the table as drawn, the column's.
    Cell(usize, usize),
    /// A click on a heading: sort by that column, or the other way.
    Sort(usize),
    /// A click on a line of a list: the tables, the log or the functions.
    At(usize),
    Edit,
    EditText(String),
    Commit,
    CancelEdit,
    /// Delete the row under the cursor.
    Delete,
    /// The switch to write raw.
    ToggleRaw,
    Close,
    /// Open function `i` of [`form::callables`] on its call screen.
    Function(usize),
    /// Type into a field of the form, named by its place (the first of the
    /// path; a field is one line).
    FormText(Vec<usize>, String),
    FormAt(usize),
    /// Open the picker for the id field at a path; filter it; choose the
    /// row at this place among those the filter leaves; close it.
    Pick(Vec<usize>),
    PickFilter(String),
    Picked(usize),
    ClosePicker,
    /// Call the open form's function.
    Call,
    AnswerCell(usize, usize),
    /// The keyboard and the pointer went somewhere else — the app beside
    /// a docked explorer: an edit in progress ends as a click away ends it.
    Blur,
}

/// Rows of values under headings, each column with its width in
/// characters: a table's page, or a query's answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub columns: Vec<String>,
    /// What the headings say: the name, that it is the key, how it sorts.
    pub headings: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub widths: Vec<usize>,
}

/// A value as one line of a cell: an id short, the rest as
/// [`value::text_of`] says it, line breaks laid flat.
pub fn shown(v: &Value) -> String {
    match v {
        Value::Id(id) => ark::hash::hex(id)[..ID_DIGITS].to_string(),
        v => value::text_of(v).chars().map(|c| if c.is_control() { ' ' } else { c }).collect(),
    }
}

impl Grid {
    /// Every column as wide as its heading and its values, within
    /// [`MIN_CHARS`] and [`MAX_CHARS`].
    pub fn new(columns: Vec<String>, headings: Vec<String>, rows: Vec<Vec<Value>>) -> Grid {
        let width = |c: usize| {
            rows.iter()
                .filter_map(|r| r.get(c))
                .map(|v| shown(v).chars().count())
                .fold(headings[c].chars().count(), usize::max)
        };
        let widths = (0..columns.len()).map(|c| width(c).clamp(MIN_CHARS, MAX_CHARS)).collect();
        Grid {
            columns,
            headings,
            rows,
            widths,
        }
    }

    /// A query's answer: a list of records is a row each under the record's
    /// fields; anything else is one column, `value`, a row per element.
    pub fn of_value(ty: &Ty, v: &Value) -> Grid {
        let items = match v {
            Value::List(xs) => xs.clone(),
            other => vec![other.clone()],
        };
        let (columns, rows): (Vec<String>, Vec<Vec<Value>>) = match ty {
            Ty::Struct(fs) => {
                let row = |x: Value| if let Value::Struct(vs) = x { vs } else { vec![x] };
                (fs.iter().map(|(n, _)| n.clone()).collect(), items.into_iter().map(row).collect())
            }
            _ => (vec!["value".into()], items.into_iter().map(|x| vec![x]).collect()),
        };
        Grid::new(columns.clone(), columns, rows)
    }

    /// The cell as drawn: cut to its column around an ellipsis.
    pub fn cell(&self, r: usize, c: usize) -> String {
        self.rows
            .get(r)
            .and_then(|row| row.get(c))
            .map_or(String::new(), |v| arkui::fit::middle(&shown(v), self.widths[c]))
    }

    /// The whole of a cell, named: what the detail line says.
    pub fn detail(&self, r: usize, c: usize) -> Option<String> {
        Some(format!("{}: {}", self.columns.get(c)?, value::text_of(self.rows.get(r)?.get(c)?)))
    }
}

/// How lit each cell of one drawn row is, from 1 as it lands to 0.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowLight {
    pub cells: Vec<f32>,
}

/// From 1 at `since` to 0 at `since + FADE_MS`, eased so most of the fade
/// is late.
pub fn fade(since: u64, now: u64) -> f32 {
    let t = now.saturating_sub(since) as f32 / FADE_MS as f32;
    if t >= 1.0 {
        0.0
    } else {
        1.0 - t * t
    }
}

/// What a call came to: a query's answer or why it failed; a mutation
/// authored — its verdict is the host's, asked through the ticket — or
/// refused before it was.
#[derive(Clone, Debug, PartialEq)]
pub enum Called {
    Answer(Value),
    Failed(String),
    Authored(Ticket),
    Refused(String),
}

/// A call made from the explorer: the function, its input as its fields'
/// values, what a query's answer is a list of, and what it came to.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub function: String,
    pub kind: Kind,
    pub args: Value,
    pub returns: Ty,
    pub called: Called,
}

/// What is lit since when: a row by its key's bytes, and a column, or
/// [`WHOLE`] for a row that is new.
type Lit = BTreeMap<(Vec<u8>, usize), u64>;
const WHOLE: usize = usize::MAX;

/// The explorer: where it is and what it is in the middle of. Everything
/// it shows is read from the [`Source`] it is handed each time.
#[derive(Debug, Default)]
pub struct Explorer {
    pub screen: Screen,
    pub tables_at: usize,
    pub row_at: usize,
    pub column_at: usize,
    pub log_at: usize,
    pub functions_at: usize,
    pub editing: Option<Editing>,
    /// Write raw even where the table exposes CRUD: the visible switch,
    /// drawn only where the writer can.
    pub raw: bool,
    /// The last thing worth saying: what an edit or a call became, or why not.
    pub note: String,
    pub keys: vim::Keys,
    /// A table sorted by a column, and whether downwards.
    pub sort: Option<(String, usize, bool)>,
    /// A form per function opened this session, so leaving one keeps it.
    pub forms: BTreeMap<String, Form>,
    pub picker: Option<Picker>,
    pub calls: Vec<Call>,
    /// The keys walk a query's answer rather than its form, and where in it.
    pub in_answer: bool,
    pub answer_at: (usize, usize),
    /// Whether the keyboard is the explorer's: a host that docks it says
    /// when the app has it, and every cursor is then drawn as unfocused.
    pub has_keys: bool,
    /// The host's time and its store's generation, as it last said them;
    /// the table last diffed, its rows by key; and what is lit since when.
    pub now: u64,
    pub generation: Option<u64>,
    seen: (String, BTreeMap<Vec<u8>, Vec<Value>>),
    lit: Lit,
}

impl Explorer {
    pub fn new() -> Explorer {
        Explorer {
            has_keys: true,
            ..Explorer::default()
        }
    }

    /// The tables of the store's space, in its schema's order.
    pub fn tables(src: &Source) -> Vec<String> {
        let space = src.store.space();
        src.schema().tables.iter().filter(|t| t.space == space).map(|t| t.name.clone()).collect()
    }

    /// The CRUD exposed for `table`, as the writer says it.
    pub fn verbs(w: &dyn Writer, table: &str) -> Option<CrudVerbs> {
        w.exposed().into_iter().find(|(t, _)| t == table).map(|(_, v)| v)
    }

    /// A table's columns, from the schema.
    pub fn columns(src: &Source, table: &str) -> Vec<String> {
        src.schema()
            .table(table)
            .map(|(_, t)| t.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default()
    }

    /// A table's rows, each its key's bytes, its bytes and its columns'
    /// values: browsed ([`ark::dynamic::Browse`]) in the store's key order,
    /// or as the sort says, ties in key order. Read afresh at each call.
    pub fn rows(&self, src: &Source, table: &str) -> Vec<(Vec<u8>, Vec<u8>, Vec<Value>)> {
        let Some((_, t)) = src.schema().table(table) else { return vec![] };
        let read = |r: &Vec<u8>| Some((t.parse(r).ok()?.key, r.clone(), value::decode_all(t, r).ok()?));
        let mut browse = ark::dynamic::Browse::table(table);
        if let Some((_, c, down)) = self.sort.as_ref().filter(|(s, ..)| s == table) {
            browse = browse.sort(&t.columns[*c].name, *down);
        }
        browse.rows(src.store).unwrap_or_default().iter().filter_map(read).collect()
    }

    /// Which page of `rows` the cursor is on, and how many there are.
    pub fn page(&self, rows: usize) -> (usize, usize) {
        (self.row_at / PAGE_ROWS, rows.div_ceil(PAGE_ROWS).max(1))
    }

    /// A table's page as drawn, each cell lit by what changed under it.
    pub fn lit_table(&self, src: &Source, t: &str) -> (Grid, Vec<RowLight>) {
        let rows = self.rows(src, t);
        let from = self.page(rows.len()).0 * PAGE_ROWS;
        let page = &rows[from.min(rows.len())..(from + PAGE_ROWS).min(rows.len())];
        let key = src.schema().table(t).map_or(0, |(_, t)| t.key);
        let sorted = self.sort.as_ref().filter(|(s, ..)| s == t);
        let heading = |(i, c): (usize, &String)| {
            let arrow = match sorted {
                Some((_, s, down)) if *s == i => {
                    if *down {
                        " \u{25bc}"
                    } else {
                        " \u{25b2}"
                    }
                }
                _ => "",
            };
            format!("{c}{}{arrow}", if i < key { " \u{b7} key" } else { "" })
        };
        let columns = Explorer::columns(src, t);
        let headings = columns.iter().enumerate().map(heading).collect();
        let strength = |k: &[u8], c: usize| {
            [c, WHOLE]
                .iter()
                .filter_map(|c| self.lit.get(&(k.to_vec(), *c)))
                .map(|s| fade(*s, self.now))
                .fold(0.0, f32::max)
        };
        let lights = page
            .iter()
            .map(|(k, _, vs)| RowLight {
                cells: (0..vs.len()).map(|c| strength(k, c)).collect(),
            })
            .collect();
        (Grid::new(columns, headings, page.iter().map(|r| r.2.clone()).collect()), lights)
    }

    /// The whole of the cell under the cursor, on a table's screen.
    pub fn detail(&self, src: &Source) -> Option<String> {
        let Screen::Table(t) = &self.screen else { return None };
        let (_, _, vs) = self.rows(src, t).into_iter().nth(self.row_at)?;
        Some(format!(
            "{}: {}",
            Explorer::columns(src, t).get(self.column_at)?,
            value::text_of(vs.get(self.column_at)?)
        ))
    }

    /// What a host says on every tick: the time, and the generation of its
    /// store. When it moved, the table on screen is diffed by key against
    /// what was last seen — a new row lit whole, a changed cell alone — and
    /// a query's answer on screen is run again. A table shown for the first
    /// time is only remembered.
    pub fn observe(&mut self, src: &Source, generation: u64, now: u64) {
        let moved = self.generation.is_some_and(|g| g != generation);
        (self.now, self.generation) = (now, Some(generation));
        match self.screen.clone() {
            Screen::Table(t) if moved || self.seen.0 != t => {
                let rows: BTreeMap<Vec<u8>, Vec<Value>> = self.rows(src, &t).into_iter().map(|(k, _, vs)| (k, vs)).collect();
                if self.seen.0 != t {
                    self.lit.clear();
                }
                for (k, vs) in rows.iter().filter(|_| self.seen.0 == t) {
                    let was = self.seen.1.get(k);
                    let changed = (0..vs.len()).filter(|c| was.is_some_and(|was| was.get(*c) != vs.get(*c)));
                    for c in if was.is_none() { vec![WHOLE] } else { changed.collect() } {
                        self.lit.insert((k.clone(), c), now);
                    }
                }
                self.seen = (t, rows);
            }
            Screen::Call(f) if moved => {
                let at = self.calls.iter().rposition(|c| c.function == f && c.kind == Kind::Query);
                if let (Some(i), Some(c)) = (at, Explorer::callable(src, &f)) {
                    self.calls[i].called = form::query(src, &c, &self.calls[i].args).map_or_else(Called::Failed, Called::Answer);
                }
            }
            _ => {}
        }
        self.lit.retain(|_, since| fade(*since, now) > 0.0);
    }

    /// Whether anything is still lit, so a host that redraws only when
    /// asked keeps a clock running.
    pub fn fading(&self) -> bool {
        self.lit.values().any(|s| fade(*s, self.now) > 0.0)
    }

    fn callable(src: &Source, name: &str) -> Option<Callable> {
        form::callables(src.catalog).into_iter().find(|c| c.name == name)
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

    /// The last call of the function on screen.
    pub fn last_call(&self) -> Option<&Call> {
        let Screen::Call(f) = &self.screen else { return None };
        self.calls.iter().rev().find(|c| &c.function == f)
    }

    /// The last call's answer, as a grid, where it was a query's.
    pub fn answer_grid(&self) -> Option<Grid> {
        let c = self.last_call()?;
        let Called::Answer(v) = &c.called else { return None };
        Some(Grid::of_value(&c.returns, v))
    }

    /// What a call came to, as a person would say it: a mutation's verdict
    /// asked of the host now, so a server's answer is said when it arrives.
    pub fn outcome(call: &Call, w: &dyn Writer) -> String {
        match &call.called {
            Called::Answer(Value::List(xs)) => ark::row::plural(xs.len(), "row"),
            Called::Answer(_) => "one value".into(),
            Called::Failed(why) => why.clone(),
            Called::Authored(t) => w.verdict(t).describe(),
            Called::Refused(why) => format!("refused: {why}"),
        }
    }

    /// The rows the picker offers — each its id (the key of a table keyed
    /// by one) and its cells as one line — and which the filter leaves.
    pub fn pick_rows(&self, src: &Source) -> (Vec<([u8; 16], String)>, Vec<usize>) {
        let Some(p) = &self.picker else { return (vec![], vec![]) };
        let line = |vs: &[Value]| vs.iter().map(shown).collect::<Vec<_>>().join(" \u{b7} ");
        let all: Vec<_> = (self.rows(src, &p.table).iter())
            .filter_map(|(_, _, vs)| {
                if let Some(Value::Id(id)) = vs.first() {
                    Some((*id, line(vs)))
                } else {
                    None
                }
            })
            .collect();
        let needle = p.filter.trim().to_lowercase();
        let shown = (0..all.len()).filter(|i| all[*i].1.to_lowercase().contains(&needle)).collect();
        (all, shown)
    }

    /// What an edit of `table` goes through here, said beside the switch.
    pub fn writes_through(&self, w: &dyn Writer, table: &str) -> String {
        match Explorer::verbs(w, table).filter(|v| v.may_author && (v.update || v.put) && !(self.raw && w.can_raw())) {
            Some(v) => format!("edits go through {}", if v.update { "update" } else { "put" }),
            None if w.can_raw() => "edits are written raw".into(),
            None => "read-only here".into(),
        }
    }

    /// Do what `msg` asks, against what `src` holds, writing through `w`.
    pub fn update(&mut self, msg: Msg, src: &Source, w: &mut dyn Writer) -> Outcome {
        // A spreadsheet's rule: anything that is not the edit itself ends
        // it — submitted when it changed and reads, dropped otherwise.
        if !matches!(msg, Msg::Key(..) | Msg::EditText(_) | Msg::Commit | Msg::CancelEdit) {
            self.end_edit(src, w);
        }
        match msg {
            Msg::Blur => {}
            Msg::Key(key, mods) => {
                let searching = matches!(self.keys.mode(), vim::Mode::Search(_));
                let action = self.keys.press(&key, mods);
                // A `/` search on the picker filters as it is typed, and
                // `<Esc>` out of it drops what it typed.
                if let (Some(p), vim::Mode::Search(q)) = (&mut self.picker, self.keys.mode()) {
                    (p.filter, p.at) = (q.clone(), if p.filter == *q { p.at } else { 0 });
                }
                let Some(action) = action else { return Outcome::Stay };
                match (&action, &mut self.picker) {
                    (Action::Cancel, Some(p)) if searching => (p.filter, p.at) = (String::new(), 0),
                    _ => return self.act(action, src, w),
                }
            }
            Msg::Show(s) => self.show(s),
            Msg::Open(t) => self.show(Screen::Table(t)),
            Msg::Cell(r, c) => (self.row_at, self.column_at) = (r, c),
            Msg::Sort(c) => self.sort_by(c),
            Msg::At(i) => match self.screen {
                Screen::Tables => self.tables_at = i,
                Screen::Log => self.log_at = i,
                Screen::Functions => self.functions_at = i,
                _ => {}
            },
            Msg::Edit => self.edit(src),
            Msg::EditText(t) => self.editing.iter_mut().for_each(|e| e.text = t.clone()),
            Msg::Commit => self.commit(src, w),
            Msg::CancelEdit => self.editing = None,
            Msg::Delete => self.delete(src, w),
            Msg::ToggleRaw => {
                self.raw = w.can_raw() && !self.raw;
                self.note = match (w.can_raw(), self.raw) {
                    (false, _) => "not the authority: there is no raw write here",
                    (true, true) => "writing raw",
                    (true, false) => "writing through the domain where it can",
                }
                .into();
            }
            Msg::Close => return Outcome::Close,
            Msg::Function(i) => self.open_function(src, i),
            Msg::FormText(path, text) => self.type_into(src, path[0], text),
            Msg::FormAt(i) => {
                self.in_answer = false;
                self.form_mut().into_iter().for_each(|f| f.at = i);
            }
            Msg::Pick(path) => self.open_picker(src, path[0]),
            Msg::PickFilter(text) => self.picker.iter_mut().for_each(|p| (p.filter, p.at) = (text.clone(), 0)),
            Msg::Picked(i) => self.picked(src, i),
            Msg::ClosePicker => self.picker = None,
            Msg::Call => self.call(src, w),
            Msg::AnswerCell(r, c) => (self.in_answer, self.answer_at) = (true, (r, c)),
        }
        Outcome::Stay
    }

    fn show(&mut self, s: Screen) {
        if let (Screen::Table(t), false) = (&s, self.screen == s) {
            (self.row_at, self.column_at) = (0, 0);
            self.note = format!("{t}: e edits a cell, x deletes a row, s sorts by a column, <Esc> goes back");
        }
        (self.editing, self.picker, self.screen) = (None, None, s);
    }

    /// Sort the table on screen by column `c`: up, then down.
    fn sort_by(&mut self, c: usize) {
        let Screen::Table(t) = &self.screen else { return };
        let down = matches!(&self.sort, Some((s, sc, false)) if s == t && *sc == c);
        (self.sort, self.row_at, self.column_at) = (Some((t.clone(), c, down)), 0, c);
    }

    fn open_function(&mut self, src: &Source, i: usize) {
        let Some(c) = form::callables(src.catalog).into_iter().nth(i) else {
            return;
        };
        self.forms.entry(c.name.clone()).or_insert_with(|| Form::new(&c));
        (self.functions_at, self.picker, self.in_answer, self.answer_at) = (i, None, false, (0, 0));
        self.note = format!(
            "{}: j and k walk the fields, <Enter> chooses, toggles or adds, r calls, <Esc> goes back",
            c.name
        );
        self.screen = Screen::Call(c.name);
    }

    /// Type into field `i` of the form on screen, and check it again.
    fn type_into(&mut self, src: &Source, i: usize, text: String) {
        let Some(f) = self.form_mut() else { return };
        f.values.get_mut(i).into_iter().for_each(|v| *v = text.clone());
        if let Some(c) = Explorer::callable(src, &f.function) {
            f.validate(&c, src);
        }
    }

    /// The picker, on id field `i` (or an option of one): its table's rows.
    fn open_picker(&mut self, src: &Source, i: usize) {
        let Some(f) = self.form_mut() else { return };
        let table = match f.fields.get(i).map(|(_, f)| &f.ty) {
            Some(Ty::Id(t)) => t.clone(),
            Some(Ty::Option(t)) => match &**t {
                Ty::Id(t) => t.clone(),
                _ => return,
            },
            _ => return,
        };
        f.at = i;
        self.picker = Some(Picker {
            field: i,
            table: table.clone(),
            filter: String::new(),
            at: 0,
        });
        self.note = match self.pick_rows(src).0.len() {
            0 => format!("{table} holds no rows here"),
            n => format!("{} of {table}: j and k walk, / filters, <Enter> chooses", ark::row::plural(n, "row")),
        };
    }

    /// Choose the picker's row at `i` among those its filter leaves.
    fn picked(&mut self, src: &Source, i: usize) {
        let (all, shown) = self.pick_rows(src);
        let Some(p) = self.picker.take() else { return };
        let Some((id, line)) = shown.get(i).map(|&k| &all[k]) else {
            self.note = format!("nothing of {} to choose", p.table);
            return self.picker = Some(p);
        };
        let text = value::text_of(&Value::Id(*id));
        self.note = format!("{}: {line}", p.table);
        self.type_into(src, p.field, text);
    }

    /// Call the form on screen: checked first, with the module's messages;
    /// then a query run on the host's store, or a mutation authored through
    /// the host's writer as a button would author it.
    fn call(&mut self, src: &Source, w: &mut dyn Writer) {
        let Some(c) = self.form().and_then(|f| Explorer::callable(src, &f.function)) else {
            return;
        };
        let f = self.form_mut().expect("a form is on screen");
        let Some(args) = f.validate(&c, src) else {
            let said: Vec<String> = f.messages.iter().map(|(_, m)| m.clone()).collect();
            return self.note = format!("not called: {}", said.join("; "));
        };
        let called = match c.kind {
            Kind::Query => form::query(src, &c, &args).map_or_else(Called::Failed, Called::Answer),
            Kind::Mutation => c
                .input_bytes(&args)
                .and_then(|b| w.author(&c.name, b))
                .map_or_else(Called::Refused, Called::Authored),
        };
        let call = Call {
            function: c.name.clone(),
            kind: c.kind,
            args,
            returns: c.returns.clone(),
            called,
        };
        self.note = format!("{}: {}", call.function, Explorer::outcome(&call, w));
        (self.in_answer, self.answer_at) = (matches!(call.called, Called::Answer(_)), (0, 0));
        self.calls.push(call);
    }

    /// A finished command of the vim grammar (`arkui::vim`).
    pub fn act(&mut self, action: Action, src: &Source, w: &mut dyn Writer) -> Outcome {
        // The picker, while it is open, has the keys.
        if self.picker.is_some() {
            let (n, at) = (self.pick_rows(src).1.len(), self.picker.as_ref().map_or(0, |p| p.at));
            match action {
                Action::Move(m) => self.picker.iter_mut().for_each(|p| p.at = Steps::column(n).step(at, m).unwrap_or(at)),
                Action::Activate => self.picked(src, at),
                Action::Search(q) => self.picker.iter_mut().for_each(|p| (p.filter, p.at) = (q.clone(), 0)),
                Action::Cancel => self.picker = None,
                _ => {}
            }
            return Outcome::Stay;
        }
        if let Some(o) = self.act_form(action.clone(), src, w) {
            return o;
        }
        // The editor is open but its box does not have the keys: <Enter>
        // submits it, <Esc> drops it, and anything else ends it first.
        if self.editing.is_some() {
            match action {
                Action::Activate => self.commit(src, w),
                Action::Cancel => self.editing = None,
                _ => self.end_edit(src, w),
            }
            if matches!(action, Action::Activate | Action::Cancel) {
                return Outcome::Stay;
            }
        }
        match (action, self.screen.clone()) {
            (Action::Move(m), s) => self.travel(m, s, src),
            (Action::Activate, Screen::Tables) => {
                if let Some(t) = Explorer::tables(src).get(self.tables_at).cloned() {
                    self.show(Screen::Table(t));
                }
            }
            (Action::Activate | Action::Key('e'), Screen::Table(_)) => self.edit(src),
            (Action::Activate, Screen::Functions) => self.open_function(src, self.functions_at),
            (Action::Cancel, Screen::Table(_)) => self.show(Screen::Tables),
            (Action::Cancel, Screen::Call(_)) => self.show(Screen::Functions),
            (Action::Cancel, _) => return Outcome::Close,
            (Action::Cycle, s) => self.show(match s {
                Screen::Tables | Screen::Table(_) => Screen::Log,
                Screen::Log => Screen::Functions,
                Screen::Functions | Screen::Call(_) => Screen::Tables,
            }),
            (Action::Key('x'), Screen::Table(_)) => self.delete(src, w),
            (Action::Key('s'), Screen::Table(_)) => self.sort_by(self.column_at),
            (Action::Key('R'), _) => return self.update(Msg::ToggleRaw, src, w),
            (Action::Key('f'), _) => self.show(Screen::Functions),
            _ => {}
        }
        Outcome::Stay
    }

    /// The keys on a call screen: `j` and `k` walk its fields (and a
    /// query's answer, once it is called, until `<Esc>` hands them back),
    /// `<Enter>` on an id field opens its picker and on the button calls,
    /// `r` calls. `None` for what the screens share.
    fn act_form(&mut self, action: Action, src: &Source, w: &mut dyn Writer) -> Option<Outcome> {
        let g = self.answer_grid().filter(|_| self.in_answer);
        let (n, at) = self.form().map(|f| (f.fields.len(), f.at))?;
        match (action, g) {
            (Action::Move(m @ (Motion::Left(_) | Motion::Right(_))), Some(g)) => {
                self.answer_at.1 = Steps::row(g.columns.len()).step(self.answer_at.1, m).unwrap_or(self.answer_at.1)
            }
            (Action::Move(m), Some(g)) => self.answer_at.0 = Steps::column(g.rows.len()).step(self.answer_at.0, m).unwrap_or(self.answer_at.0),
            (Action::Cancel, Some(_)) => self.in_answer = false,
            (Action::Move(m @ (Motion::Up(_) | Motion::Down(_) | Motion::First | Motion::Last | Motion::To(_))), None) => {
                self.form_mut()?.at = Steps::column(n + 1).step(at, m).unwrap_or(at)
            }
            (Action::Activate, _) if at >= n => self.call(src, w),
            (Action::Activate, _) => self.open_picker(src, at),
            (Action::Key('r'), _) => self.call(src, w),
            _ => return None,
        }
        Some(Outcome::Stay)
    }

    // A motion in whichever list is on screen; on a table `h` and `l` walk
    // the columns and `j` and `k` the rows.
    fn travel(&mut self, m: Motion, s: Screen, src: &Source) {
        let step = |n: usize, at: usize| Steps::column(n).step(at, m).unwrap_or(at);
        match (s, m) {
            (Screen::Tables, _) => self.tables_at = step(Explorer::tables(src).len(), self.tables_at),
            (Screen::Log, _) => self.log_at = step(src.log.lines().len(), self.log_at),
            (Screen::Functions, _) => self.functions_at = step(form::callables(src.catalog).len(), self.functions_at),
            (Screen::Table(t), Motion::Left(_) | Motion::Right(_)) => {
                self.column_at = Steps::row(Explorer::columns(src, &t).len())
                    .step(self.column_at, m)
                    .unwrap_or(self.column_at)
            }
            (Screen::Table(t), _) => self.row_at = step(src.store.rows(&t).count(), self.row_at),
            (Screen::Call(_), _) => {}
        }
    }

    /// Start editing the cell under the cursor, with its text.
    fn edit(&mut self, src: &Source) {
        let Screen::Table(t) = &self.screen else { return };
        let Some((key, _, vs)) = self.rows(src, t).into_iter().nth(self.row_at) else {
            return;
        };
        let Some(column) = Explorer::columns(src, t).get(self.column_at).cloned() else {
            return;
        };
        let text = vs.get(self.column_at).map(value::text_of).unwrap_or_default();
        self.editing = Some(Editing {
            table: t.clone(),
            key,
            column,
            text,
        });
    }

    /// The edit's text read as its column's value, and the row as it stands
    /// now: the row, the value, and whether it is what the row holds.
    fn read_edit(src: &Source, e: &Editing) -> Result<(Vec<u8>, Value, bool), String> {
        let (_, tbl) = src.schema().table(&e.table).ok_or_else(|| format!("no table {}", e.table))?;
        let c = tbl.column(&e.column).ok_or_else(|| format!("{} has no column {}", e.table, e.column))?;
        let tys: Vec<Ty> = tbl.columns.iter().map(value::column_ty).collect();
        let value = value::parse_cell(&e.column, &tys[c], &e.text)?;
        let old = ark::store::Read::get(src.store, &e.table, &e.key).ok_or_else(|| format!("the row of {} is gone", e.table))?;
        let same = value::decode_all(tbl, &old).ok().and_then(|vs| vs.get(c).cloned()) == Some(value.clone());
        Ok((old, value, same))
    }

    fn via<'v>(&self, w: &dyn Writer, verbs: &'v Option<CrudVerbs>) -> Via<'v> {
        Via {
            verbs: verbs.as_ref(),
            can_raw: w.can_raw(),
            force_raw: self.raw,
        }
    }

    /// Commit the edit in progress: its text read as the column's value, the
    /// row as it stands now, and the change routed and handed to the
    /// writer. A refusal keeps the editor, so a typo is fixed.
    fn commit(&mut self, src: &Source, w: &mut dyn Writer) {
        let Some(e) = self.editing.take() else { return };
        let verbs = Explorer::verbs(w, &e.table);
        let outcome = Explorer::read_edit(src, &e).and_then(|(old, value, same)| match same {
            true => Ok("nothing to change".to_string()),
            false => {
                let route = edit::route_edit(src.catalog, self.via(w, &verbs), &e.table, &old, &e.column, value)?;
                let said = route.describe();
                route.send(w).map(|()| format!("{}.{} written, {said}", e.table, e.column))
            }
        });
        self.note = outcome.unwrap_or_else(|why| {
            self.editing = Some(e);
            why
        });
    }

    /// End the edit in progress, as a click elsewhere in a spreadsheet does:
    /// a value that changed and reads is submitted as <Enter> submits it;
    /// one that did not change, or does not read, is dropped as <Esc> drops
    /// it, saying why. Either way the editor is gone.
    pub fn end_edit(&mut self, src: &Source, w: &mut dyn Writer) {
        let Some(e) = &self.editing else { return };
        match Explorer::read_edit(src, e) {
            Ok((_, _, false)) => self.commit(src, w),
            Err(why) if !why.contains("is gone") => self.note = format!("edit dropped, {why}"),
            _ => {}
        }
        self.editing = None;
    }

    /// Where the row being edited is, among the table's rows as drawn.
    pub fn editing_at(&self, src: &Source) -> Option<usize> {
        let e = self.editing.as_ref()?;
        self.rows(src, &e.table).iter().position(|(k, ..)| *k == e.key)
    }

    /// Delete the row under the cursor, routed as an edit is.
    fn delete(&mut self, src: &Source, w: &mut dyn Writer) {
        let Screen::Table(t) = &self.screen else { return };
        let Some((_, row, _)) = self.rows(src, t).into_iter().nth(self.row_at) else {
            return;
        };
        let verbs = Explorer::verbs(w, t);
        let outcome = edit::route_delete(src.catalog, self.via(w, &verbs), t, &row).and_then(|route| {
            let said = route.describe();
            route.send(w).map(|()| format!("a row of {t} deleted, {said}"))
        });
        self.note = outcome.unwrap_or_else(|why| why);
    }
}
