//! The explorer's model, without a window (`docs/plan-guards.md` D4): what a
//! cell edit and a row delete become and which writer they reach, what the
//! console runs, what a cell's text reads back as, and how the keys walk it.

// The row structs are the vocabulary's: a body reads their fields only
// natively.
#![allow(dead_code)]

use ark::authoring::*;
use ark::eval::{Args, Ctx};
use ark::store::{Change, MemoryStore, Store};
use ark::value::{TableName, Value};
use iced::keyboard::{key::Named, Key, Modifiers};

use crate::data::{crud_of, CrudVerbs, NoLog, Source, Ticket, Writer};
use crate::edit::{cell_text, console, parse_cell, route_delete, route_edit, Route, Via};
use crate::model::{Explorer, Msg, Outcome, Screen};

pub struct People {
    pub team: Table<Team>,
    pub user: Table<User>,
}
impl Tables for People {
    fn open() -> Self {
        People {
            team: table(),
            user: table(),
        }
    }
}

pub struct Team {
    pub name: Text,
    pub size: Int,
}
impl Row for Team {
    const NAME: &str = "team";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns().text(Self::name).int(Self::size).key((Self::name,))
    }
}
#[allow(non_upper_case_globals)]
impl Team {
    pub const name: Col<Self, Text> = col("name");
    pub const size: Col<Self, Int> = col("size");
}

pub struct User {
    pub id: Id<User>,
    pub name: Text,
    pub team: Text,
    pub admin: Bool,
    pub email: Opt<Text>,
}
impl Row for User {
    const NAME: &str = "user";
    type Key = (Id<User>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::team)
            .refs::<Team>()
            .bool(Self::admin)
            .text(Self::email)
            .nullable()
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl User {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const team: Col<Self, Text> = col("team");
    pub const admin: Col<Self, Bool> = col("admin");
    pub const email: Col<Self, Opt<Text>> = col("email");
}

pub struct OfTeam {
    pub team: Text,
}
impl Input for OfTeam {
    fn schema() -> Object<Self> {
        object().field("team", text())
    }
}

pub struct NewTeam {
    pub name: Text,
    pub size: Int,
}
impl Input for NewTeam {
    fn schema() -> Object<Self> {
        object()
            .field("name", text().trim().min(1).why("a team needs a name").max(12))
            .field("size", int().range(1, 10))
    }
}

pub struct Level;
impl Variants for Level {
    const VARIANTS: &'static [&'static str] = &["low", "high"];
}

/// A mutation over every kind of field a form draws.
pub struct Kick {
    pub user: Id<User>,
    pub why: Opt<Text>,
    pub tags: List<Text>,
    pub level: Text,
    pub loud: Bool,
}
impl Input for Kick {
    fn schema() -> Object<Self> {
        object()
            .field("user", id::<User>().exists())
            .field("why", opt(text()))
            .field("tags", text().list().non_empty())
            .field("level", enum_::<Level>())
            .field("loud", bool_())
    }
}

/// A user table with its CRUD exposed, a team table with none, one query,
/// and two mutations of the module's own on a second router.
fn module() -> ark::ir::Module {
    let people = router::<People>("people");
    let teams = router::<People>("teams");
    Module::new((
        people.routes((
            people.crud::<User>(),
            people.input::<OfTeam>().query("members", |_ctx, db, input| {
                db.user.filter(User::team.eq(input.team)).order_by(User::name.asc())
            }),
        )),
        teams.routes((
            teams.input::<NewTeam>().mutation("make_team", |_ctx, db, input| {
                db.team.insert(Team {
                    name: input.name,
                    size: input.size,
                })
            }),
            teams.input::<Kick>().mutation("kick", |_ctx, db, input| db.user.delete((input.user,))),
        )),
    ))
    .build()
    .clone()
}

fn uid(k: u8) -> Value {
    let mut i = [0u8; 16];
    i[15] = k;
    Value::Id(i)
}

fn store(m: &ark::ir::Module) -> MemoryStore {
    let mut st = MemoryStore::empty(m.schema.clone());
    let row = |pairs: &[(&str, Value)]| pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    st.put("team", row(&[("name", Value::text("blue")), ("size", Value::Int(2))])).unwrap();
    st.put("team", row(&[("name", Value::text("red")), ("size", Value::Int(1))])).unwrap();
    for (k, name, team) in [(1, "ann", "blue"), (2, "bob", "blue"), (3, "cat", "red")] {
        st.put(
            "user",
            row(&[
                ("id", uid(k)),
                ("name", Value::text(name)),
                ("team", Value::text(team)),
                ("admin", Value::Bool(false)),
                ("email", Value::Null),
            ]),
        )
        .unwrap();
    }
    st
}

/// A writer that remembers what it was asked: a client's (no raw) or the
/// authority's.
#[derive(Default)]
struct Recorder {
    exposed: Vec<(TableName, CrudVerbs)>,
    authority: bool,
    authored: Vec<(String, Args)>,
    raws: Vec<Change>,
    /// Every raw write asked for, refused or not.
    asked_raw: usize,
}

impl Writer for Recorder {
    fn exposed(&self) -> Vec<(TableName, CrudVerbs)> {
        self.exposed.clone()
    }
    fn author(&mut self, function: &str, args: Args) -> Result<Ticket, String> {
        self.authored.push((function.into(), args));
        Ok(Ticket(self.authored.len().to_string()))
    }
    fn raw(&mut self, change: Change) -> Result<(), String> {
        self.asked_raw += 1;
        if !self.authority {
            return Err("not the authority".into());
        }
        self.raws.push(change);
        Ok(())
    }
    fn can_raw(&self) -> bool {
        self.authority
    }
}

fn writer(m: &ark::ir::Module, authority: bool) -> Recorder {
    Recorder {
        exposed: crud_of(m).into_iter().map(|(t, v)| (t, CrudVerbs { may_author: true, ..v })).collect(),
        authority,
        ..Recorder::default()
    }
}

fn source<'a>(m: &'a ark::ir::Module, st: &'a MemoryStore) -> Source<'a> {
    Source {
        store: st,
        schema: &m.schema,
        module: m,
        log: &NoLog,
        ctx: Ctx::new("alice", "s1"),
    }
}

fn key(c: &str) -> Msg {
    Msg::Key(Key::Character(c.into()), Modifiers::empty())
}

fn named(n: Named) -> Msg {
    Msg::Key(Key::Named(n), Modifiers::empty())
}

/// `crud_of` finds the four a `crud` declares, by name and input — the
/// table without one has none, and a mutator of the right name that does
/// not take the row's columns (or the key's) is not one. Falsified by
/// recognising the verbs by name alone: the delete whose input was renamed
/// was still counted.
#[test]
fn the_exposed_crud_is_read_off_the_module() {
    let m = module();
    let crud = crud_of(&m);
    assert_eq!(crud.len(), 1, "{crud:?}");
    let (t, v) = &crud[0];
    assert_eq!(t, "user");
    assert!(v.insert && v.update && v.delete && v.put && !v.may_author);
    let mut odd = m.clone();
    for f in odd.functions.iter_mut().filter(|f| f.name == "delete_user") {
        f.input[0].0 = "who".into();
    }
    assert!(!crud_of(&odd)[0].1.delete, "a delete that does not take the key is not the CRUD verb");
}

/// A cell edit on a table whose CRUD is exposed goes through its
/// `update_<t>`, authored, the new row's columns as its arguments, and
/// reaches no raw writer — on a client and at the authority alike, unless
/// the switch says raw. Falsified by routing an edit raw whenever the
/// writer can: the authority's edit of a user went raw.
#[test]
fn an_edit_goes_through_the_exposed_update() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    for authority in [false, true] {
        let mut w = writer(&m, authority);
        let mut ex = Explorer::new();
        ex.update(Msg::Open("user".into()), &src, &mut w);
        ex.update(Msg::Cell(1, 1), &src, &mut w);
        ex.update(Msg::Edit, &src, &mut w);
        assert_eq!(ex.editing.as_ref().map(|e| (e.column.as_str(), e.text.as_str())), Some(("name", "bob")));
        ex.update(Msg::EditText("robert".into()), &src, &mut w);
        ex.update(Msg::Commit, &src, &mut w);
        assert!(w.raws.is_empty(), "{:?}", w.raws);
        let (f, args) = w.authored.last().expect("authored");
        assert_eq!(f, "update_user");
        assert_eq!(args.get("name"), Some(&Value::text("robert")));
        assert_eq!(args.get("id"), Some(&uid(2)));
        assert_eq!(args.len(), 5, "every column");
        assert!(ex.note.contains("through update_user"), "{}", ex.note);
    }
}

/// The switch, at the authority: the same edit written raw, as the edit
/// the row makes. On a client there is no switch: it says so and stays
/// off. Falsified by letting the switch turn on without a raw writer: the
/// client's switch said "writing raw".
#[test]
fn the_switch_writes_raw_at_the_authority_alone() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let mut w = writer(&m, true);
    let mut ex = Explorer::new();
    ex.update(Msg::Open("user".into()), &src, &mut w);
    ex.update(key("R"), &src, &mut w);
    assert!(ex.raw);
    ex.update(Msg::Cell(0, 3), &src, &mut w);
    ex.update(Msg::Edit, &src, &mut w);
    ex.update(Msg::EditText("true".into()), &src, &mut w);
    ex.update(Msg::Commit, &src, &mut w);
    assert!(w.authored.is_empty());
    match w.raws.as_slice() {
        [Change::Edit(t, old, new)] => {
            assert_eq!(t, "user");
            assert_eq!(old.get("admin"), Some(&Value::Bool(false)));
            assert_eq!(new.get("admin"), Some(&Value::Bool(true)));
            assert_eq!(old.get("id"), new.get("id"));
        }
        other => panic!("{other:?}"),
    }
    let mut client = writer(&m, false);
    let mut ex = Explorer::new();
    ex.update(Msg::ToggleRaw, &src, &mut client);
    assert!(!ex.raw);
    assert!(ex.note.contains("not the authority"), "{}", ex.note);
}

/// A table with no CRUD: the authority writes it raw, and a client cannot
/// write it at all — the refusal is said, the editor kept, and no writer
/// is reached. Falsified by sending a client's edit to `raw` anyway: the
/// recorder refused it, but it was asked.
#[test]
fn a_table_with_no_crud_is_the_authoritys_alone() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let edit_size = |w: &mut Recorder| {
        let mut ex = Explorer::new();
        ex.update(Msg::Open("team".into()), &src, w);
        ex.update(Msg::Cell(0, 1), &src, w);
        ex.update(Msg::Edit, &src, w);
        ex.update(Msg::EditText("7".into()), &src, w);
        ex.update(Msg::Commit, &src, w);
        ex
    };
    let mut authority = writer(&m, true);
    edit_size(&mut authority);
    assert!(matches!(authority.raws.as_slice(), [Change::Edit(t, _, new)] if t == "team" && new.get("size") == Some(&Value::Int(7))));
    let mut client = writer(&m, false);
    let ex = edit_size(&mut client);
    assert!(client.asked_raw == 0 && client.authored.is_empty(), "no writer is reached");
    assert!(ex.note.contains("not the authority"), "{}", ex.note);
    assert!(ex.editing.is_some(), "the edit is kept, to be fixed or dropped");
}

/// A row delete: through `delete_<t>` with the key as its arguments where
/// it is exposed, raw where the host is the authority, refused where it is
/// neither. Falsified by giving the delete every column: the arguments were
/// not the key's.
#[test]
fn a_delete_goes_through_delete_or_raw() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let mut w = writer(&m, true);
    let mut ex = Explorer::new();
    ex.update(Msg::Open("user".into()), &src, &mut w);
    ex.update(named(Named::ArrowDown), &src, &mut w);
    ex.update(key("x"), &src, &mut w);
    let (f, args) = w.authored.last().expect("a delete");
    assert_eq!(f, "delete_user");
    assert_eq!(args.keys().collect::<Vec<_>>(), ["id"]);
    assert_eq!(args["id"], uid(2));
    let team = st.scan("team")[0].clone();
    let authority = Via {
        verbs: None,
        can_raw: true,
        force_raw: false,
    };
    assert_eq!(
        route_delete(&m.schema, authority, "team", &team),
        Ok(Route::Raw(Change::Remove("team".into(), team.clone())))
    );
    assert!(route_delete(&m.schema, Via::default(), "team", &team).is_err());
}

/// A key column names the row and is not edited, by either route. Falsified
/// by letting it through: the update carried a moved key.
#[test]
fn a_key_is_not_edited() {
    let m = module();
    let st = store(&m);
    let old = st.scan("user")[0].clone();
    let verbs = crud_of(&m)[0].1;
    let verbs = CrudVerbs { may_author: true, ..verbs };
    let via = Via {
        verbs: Some(&verbs),
        can_raw: true,
        force_raw: false,
    };
    let r = route_edit(&m.schema, via, "user", &old, "id", uid(9));
    assert!(matches!(&r, Err(why) if why.contains("part of the key")), "{r:?}");
}

/// A cell's text reads back as the value it shows, for every column type,
/// and what is not one is refused with its column named. Falsified by
/// showing a bool as `yes`: the round trip failed.
#[test]
fn a_cell_reads_back_as_its_column() {
    let m = module();
    let user = m.schema.lookup_table("user").unwrap();
    let col = |n: &str| user.column(n).unwrap();
    for (c, v) in [
        ("id", uid(7)),
        ("name", Value::text("  spaced  ")),
        ("admin", Value::Bool(true)),
        ("email", Value::Null),
        ("email", Value::text("a@b")),
    ] {
        assert_eq!(parse_cell(col(c), &cell_text(&v)), Ok(v.clone()), "{c}");
    }
    let team = m.schema.lookup_table("team").unwrap();
    assert_eq!(parse_cell(team.column("size").unwrap(), "-3"), Ok(Value::Int(-3)));
    assert!(parse_cell(team.column("size").unwrap(), "three").unwrap_err().contains("size"));
    assert!(parse_cell(col("admin"), "yes").is_err());
    assert!(parse_cell(col("id"), "beef").is_err());
    assert_eq!(parse_cell(col("email"), ""), Ok(Value::Null), "an empty nullable cell is nothing");
}

/// The console runs a query by name with its arguments, and a plan in the
/// IR's form, read-only; a mutator's name is refused. Falsified by running
/// whatever is named through `query_as` without the kind check: a mutator
/// was run as a query, and came back a bug rather than a refusal.
#[test]
fn the_console_runs_queries_alone() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let got = console(&src, "members", r#"{"team": "blue"}"#).expect("members of blue");
    let names: Vec<String> = got.as_list().iter().map(|r| r.field("name").as_text().to_string()).collect();
    assert_eq!(names, ["ann", "bob"]);
    let plan = m.lookup_function("members").and_then(|f| f.plan.clone()).unwrap();
    let mut whole = plan.clone();
    whole.filter = None;
    let text = ark::json::json(&ark::ir::plan_value(&whole));
    let every = console(&src, &text, "").expect("the plan runs");
    assert_eq!(every.as_list().len(), 3);
    let refused = console(&src, "put_user", "{}").unwrap_err();
    assert!(refused.contains("runs queries alone"), "{refused}");
    assert!(console(&src, "nope", "").is_err());
    let mut ex = Explorer::new();
    let mut w = writer(&m, false);
    ex.update(Msg::Show(Screen::Console), &src, &mut w);
    ex.update(Msg::Query("members".into()), &src, &mut w);
    ex.update(Msg::Args(r#"{"team": "red"}"#.into()), &src, &mut w);
    ex.update(named(Named::Enter), &src, &mut w);
    assert!(matches!(&ex.answer, Some(Ok(s)) if s.contains("cat")), "{:?}", ex.answer);
    assert!(w.authored.is_empty() && w.raws.is_empty(), "the console writes nothing");
}

/// The keys: `j` and `k` walk the tables, `<Enter>` opens one, `l` walks its
/// columns, `<Esc>` goes back and, from the top, closes; `<Tab>` goes between
/// the screens. Falsified by answering `<Esc>` on the tables with `Stay`:
/// the explorer could not be left by the keyboard.
#[test]
fn the_keys_walk_open_and_close() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let mut w = writer(&m, false);
    let mut ex = Explorer::new();
    ex.update(key("j"), &src, &mut w);
    assert_eq!(ex.tables_at, 1);
    ex.update(key("j"), &src, &mut w);
    assert_eq!(ex.tables_at, 1, "the last table refuses a step past it");
    ex.update(named(Named::Enter), &src, &mut w);
    assert_eq!(ex.screen, Screen::Table("user".into()));
    ex.update(key("l"), &src, &mut w);
    ex.update(key("l"), &src, &mut w);
    assert_eq!(ex.column_at, 2);
    ex.update(key("G"), &src, &mut w);
    assert_eq!(ex.row_at, 2);
    assert_eq!(ex.update(named(Named::Escape), &src, &mut w), Outcome::Stay);
    assert_eq!(ex.screen, Screen::Tables);
    ex.update(named(Named::Tab), &src, &mut w);
    assert_eq!(ex.screen, Screen::Log);
    ex.update(named(Named::Tab), &src, &mut w);
    assert_eq!(ex.screen, Screen::Console);
    assert_eq!(ex.update(named(Named::Escape), &src, &mut w), Outcome::Close);
}

/// Every screen draws, over a store and a writer of either kind, an editor
/// open included — what a test without a window can say of the view.
/// Falsified by a panic in the log's view: the test failed.
#[test]
fn every_screen_draws() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    for authority in [false, true] {
        let w = writer(&m, authority);
        let mut ex = Explorer::new();
        for s in [Screen::Tables, Screen::Table("user".into()), Screen::Log, Screen::Console] {
            ex.screen = s;
            let _ = ex.view(&src, &w);
        }
        ex.editing = Some(crate::model::Editing {
            table: "user".into(),
            key: vec![uid(1)],
            column: "name".into(),
            text: "ann".into(),
        });
        ex.screen = Screen::Table("user".into());
        let _ = ex.view(&src, &w);
    }
}

/// The admin page's wire reads back as itself: the state, a raw write's
/// body and a mutation's. Falsified by reading `may_author` from the
/// insert's field: the exposure came back authorable where it was not.
#[test]
fn the_admin_wire_reads_back() {
    use crate::data::{Connection, Line, Verified};
    use crate::wire::{author_body, author_from, raw_body, raw_from, State};
    let m = module();
    let st = store(&m);
    let entry = ark::log::Entry {
        id: [7; 16],
        actor: ark::raw::AUTHOR.into(),
        session: "server-1".into(),
        roles: Default::default(),
        fn_hash: ark::raw::Raw::PutRow.hash().clone(),
        args: Args::new(),
        autos: Args::new(),
    };
    let row = st.scan("team")[0].clone();
    let state = State {
        module: ark::canon::encode(&ark::ir::module_value(&m)),
        store: st.store_value(),
        head: 3,
        lines: vec![
            Line {
                seq: Some(3),
                entry: entry.clone(),
                function: "ark.put_row".into(),
                facts: Some(vec![Change::Add("team".into(), row.clone())]),
                standing: "in the log".into(),
            },
            Line {
                seq: None,
                entry,
                function: "put_user".into(),
                facts: None,
                standing: "pending".into(),
            },
        ],
        connections: vec![Connection {
            who: "alice".into(),
            cursor: 2,
            pending: None,
            note: "1 behind".into(),
        }],
        verifies: vec![
            Verified {
                who: "alice".into(),
                seq: 2,
                answer: Some(true),
            },
            Verified {
                who: "bob".into(),
                seq: 9,
                answer: None,
            },
        ],
        exposed: vec![(
            "user".into(),
            CrudVerbs {
                insert: true,
                update: true,
                delete: false,
                put: true,
                may_author: false,
            },
        )],
        raw: true,
        who: "authority".into(),
    };
    assert_eq!(State::from_json(&state.to_json()), Ok(state.clone()));
    let back = MemoryStore::from_value(m.schema.clone(), &State::from_json(&state.to_json()).unwrap().store);
    assert_eq!(back.scan("user"), st.scan("user"));
    let change = Change::Edit("team".into(), row.clone(), row.with("size", Value::Int(9)));
    assert_eq!(raw_from(&raw_body(&change)), Ok(change));
    let mut args = Args::new();
    args.insert("id".into(), uid(1));
    assert_eq!(author_from(&author_body("delete_user", &args)), Ok(("delete_user".to_string(), args)));
}

/// The admin page's host: a state as the server sends it, read into a store
/// the explorer edits like any other, and an edit asked of the server — raw
/// with the switch on, through the CRUD the module exposes without it — as
/// the request the server's routes read. Falsified by sending the edit's
/// old row rather than its new: the raw request carried the name it had.
#[test]
fn the_admin_page_asks_the_server_for_its_writes() {
    use crate::admin::{hello_of, Loaded, Message, Page, Request};
    use crate::wire::{raw_from, State};
    let m = module();
    let st = store(&m);
    let state = State {
        module: ark::canon::encode(&ark::ir::module_value(&m)),
        store: st.store_value(),
        head: 0,
        lines: vec![],
        connections: vec![],
        verifies: vec![],
        exposed: crud_of(&m).into_iter().map(|(t, v)| (t, CrudVerbs { may_author: true, ..v })).collect(),
        raw: true,
        who: ark::raw::AUTHOR.into(),
    };
    let mut page = Page::new("http://127.0.0.1:8788/".into(), None);
    assert_eq!(page.base, "http://127.0.0.1:8788");
    let _ = page.update(Message::Loaded(Ok(state.to_json())));
    let l: &Loaded = page.loaded.as_ref().expect("the state reads");
    assert_eq!(l.store.scan("user"), st.scan("user"));
    let edit = |page: &mut Page, table: &str, row: usize, col: usize, to: &str| {
        let _ = page.update(Message::Explorer(Msg::Open(table.into())));
        let _ = page.update(Message::Explorer(Msg::Cell(row, col)));
        let _ = page.update(Message::Explorer(Msg::Edit));
        let _ = page.update(Message::Explorer(Msg::EditText(to.into())));
        let _ = page.update(Message::Explorer(Msg::Commit));
    };
    edit(&mut page, "user", 0, 1, "anne");
    match page.sent.last() {
        Some(Request::Author(f, a)) => {
            assert_eq!(f, "update_user");
            assert_eq!(a.get("name"), Some(&Value::text("anne")));
        }
        other => panic!("{other:?}"),
    }
    let _ = page.update(Message::Explorer(Msg::ToggleRaw));
    edit(&mut page, "team", 1, 1, "5");
    let Some(Request::Raw(change)) = page.sent.last().cloned() else {
        panic!("{:?}", page.sent)
    };
    let (path, body) = Request::Raw(change.clone()).to();
    assert_eq!(path, "/admin/api/raw");
    assert_eq!(raw_from(&body), Ok(change.clone()));
    assert!(
        matches!(&change, Change::Edit(t, _, new) if t == "team" && new.get("size") == Some(&Value::Int(5))),
        "{change:?}"
    );
    assert_eq!(
        hello_of(r#"{"open":false,"server":"https://h.example"}"#),
        Ok((false, Some("https://h.example".into())))
    );
    assert_eq!(hello_of(r#"{"open":true,"server":null}"#), Ok((true, None)));
}

// -- the table's cells, the functions, their forms and the picker ----------

use crate::cells::{self, Grid};
use crate::form::{self, Callable, Called, Form};
use ark::ir::{Field, FnKind};
use ark::schema::Ty;

fn row_of(m: &ark::ir::Module, table: &str, pairs: &[(&str, Value)]) -> ark::store::Row {
    let tbl = m.schema.lookup_table(table).unwrap();
    ark::store::Row::of(tbl, pairs.iter().map(|(k, v)| (k.to_string(), v.clone())))
}

/// A long value is cut to its column and never drawn past it; a column is as
/// wide as its header and its values, within the bounds; an id is drawn as
/// eight hex digits; and the detail line holds the whole of any of them.
/// Falsified by returning the shown text uncut from `Grid::cell`: the
/// two-hundred-character name came out 200 characters into a column of 36.
#[test]
fn a_long_value_never_exceeds_its_column() {
    let m = module();
    let mut st = store(&m);
    let long = "Variatio 25 a 2 Clav. (Adagio), from the Goldberg Variations, BWV 988 ".repeat(3);
    st.put(
        "user",
        row_of(
            &m,
            "user",
            &[
                ("id", uid(9)),
                ("name", Value::text(long.clone())),
                ("team", Value::text("red")),
                ("admin", Value::Bool(true)),
                ("email", Value::Null),
            ],
        ),
    )
    .unwrap();
    let src = source(&m, &st);
    let mut w = writer(&m, false);
    let mut ex = Explorer::new();
    ex.update(Msg::Open("user".into()), &src, &mut w);
    let g = ex.table_grid(&src, "user");
    assert_eq!(g.columns, ["id", "name", "team", "admin", "email"]);
    assert_eq!(g.headings[0], "id \u{b7} key");
    for (c, w) in g.widths.iter().enumerate() {
        assert!((cells::MIN_CHARS..=cells::MAX_CHARS).contains(w), "column {c}: {w}");
        assert!(g.heading(c).chars().count() <= *w);
        for r in 0..g.rows.len() {
            assert!(g.cell(r, c).chars().count() <= *w, "{} is wider than {w}", g.cell(r, c));
        }
    }
    let at = g.rows.iter().position(|r| r[1] == Value::text(long.clone())).expect("the long row");
    assert_eq!(g.widths[1], cells::MAX_CHARS, "a long column stops at the most");
    assert!(g.cell(at, 1).ends_with('\u{2026}'), "{}", g.cell(at, 1));
    assert_eq!(g.widths[3], cells::MIN_CHARS, "a column of booleans is narrow");
    assert_eq!(g.cell(at, 0), "00000000", "an id is its first eight hex digits");
    assert_eq!(g.detail(at, 1), Some(format!("name: {long}")), "the detail line is the whole value");
    assert_eq!(g.detail(at, 0), Some("id: 00000000-0000-0000-0000-000000000009".into()));
    // And the explorer's own detail line follows the cursor.
    ex.update(Msg::Cell(at, 1), &src, &mut w);
    assert_eq!(ex.detail(&src), Some(format!("name: {long}")));
    assert!(g.width_px() >= g.widths.iter().map(|w| cells::px(*w)).sum::<f32>());
}

/// A query's answer as a table: a list of structs is a row each under the
/// union of their fields; a list of values one column; one value one row.
/// Falsified by taking the columns from the first struct alone: the second
/// row's `extra` had no column.
#[test]
fn a_query_answer_is_a_grid() {
    let s = |pairs: &[(&str, Value)]| Value::Struct(Box::new(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()));
    let g = Grid::of_value(&Value::list(vec![
        s(&[("name", Value::text("ann"))]),
        s(&[("name", Value::text("bob")), ("extra", Value::Int(2))]),
    ]));
    assert_eq!(g.columns, ["name", "extra"]);
    assert_eq!(g.rows[0][1], Value::Null);
    assert_eq!(g.cell(1, 1), "2");
    let g = Grid::of_value(&Value::list(vec![Value::Int(1), Value::Int(2)]));
    assert_eq!((g.columns.as_slice(), g.rows.len()), (&["value".to_string()][..], 2));
    let g = Grid::of_value(&Value::Int(7));
    assert_eq!(g.rows, vec![vec![Value::Int(7)]]);
}

/// The Functions screen lists every query and mutation of the module, by
/// router in the order each router first appears, with its input and its
/// checks as they are said. Falsified by listing mutations alone: `members`
/// was missing.
#[test]
fn every_query_and_mutation_is_listed_by_router() {
    let m = module();
    let all = form::callables(&m);
    let names: Vec<(&str, &str)> = all.iter().map(|c| (c.router.as_str(), c.name.as_str())).collect();
    let routers: Vec<&str> = names.iter().map(|(r, _)| *r).collect();
    let first_teams = routers.iter().position(|r| *r == "teams").unwrap();
    assert!(routers[..first_teams].iter().all(|r| *r == "people"), "{names:?}");
    assert!(routers[first_teams..].iter().all(|r| *r == "teams"), "{names:?}");
    for want in ["members", "insert_user", "update_user", "delete_user", "put_user", "make_team", "kick"] {
        assert!(names.iter().any(|(_, n)| *n == want), "{want} in {names:?}");
    }
    assert!(all.iter().all(|c| matches!(c.kind, FnKind::Query | FnKind::Mutator)));
    let kick = all.iter().find(|c| c.name == "kick").unwrap();
    let said: Vec<String> = kick.input.iter().map(|(n, f)| format!("{n}: {}", form::ty_name(&f.ty))).collect();
    assert_eq!(
        said,
        [
            "user: id(user)",
            "why: option(text)",
            "tags: list(text)",
            "level: enum(low | high)",
            "loud: bool"
        ]
    );
    let make = all.iter().find(|c| c.name == "make_team").unwrap();
    assert_eq!(
        form::checks_said(&make.input[0].1),
        ["trimmed", "min 1 char (\u{201c}a team needs a name\u{201d})", "max 12 chars"]
    );
    assert_eq!(form::checks_said(&make.input[1].1), ["1 to 10"]);
}

/// Every type reads from its input as its value, and what does not read is
/// said with the field's name: a text, an int (a word refused), a bool, an
/// enum, an option (none and some), a list (items added and taken out), a
/// struct, an id (pasted with dashes, or refused), bytes. Falsified by
/// dropping the option's "none" reading: `note`, at none, was refused as not
/// an input of its type rather than read as none.
#[test]
fn every_type_reads_from_its_input() {
    let fs = |pairs: &[(&str, Ty)]| Ty::Struct(pairs.iter().map(|(k, t)| (k.to_string(), t.clone())).collect());
    let callable = Callable {
        router: "r".into(),
        name: "f".into(),
        kind: FnKind::Mutator,
        input: [
            ("title", Ty::Text),
            ("count", Ty::Int),
            ("on", Ty::Bool),
            ("level", Ty::Enum(vec!["low".into(), "high".into()])),
            ("note", Ty::Option(Box::new(Ty::Text))),
            ("tags", Ty::List(Box::new(Ty::Int))),
            ("at", fs(&[("x", Ty::Int), ("label", Ty::Text)])),
            ("who", Ty::Id("user".into())),
            ("blob", Ty::Bytes),
        ]
        .into_iter()
        .map(|(n, t)| (n.to_string(), Field::plain(t)))
        .collect(),
    };
    let mut f = Form::new(&callable);
    let p = |xs: &[usize]| xs.to_vec();
    f.set_text(&p(&[0]), "a title".into());
    f.set_text(&p(&[1]), "twelve".into());
    f.toggle(&p(&[2]));
    f.choose(&p(&[3]), 1);
    f.set_text(&p(&[4, 0]), "x".into()); // typed while it is none
    f.add(&p(&[5]));
    f.add(&p(&[5]));
    f.add(&p(&[5]));
    f.set_text(&p(&[5, 0]), "1".into());
    f.set_text(&p(&[5, 1]), "2".into());
    f.set_text(&p(&[5, 2]), "3".into());
    f.remove(&p(&[5, 1]));
    f.set_text(&p(&[6, 1]), "4".into()); // the struct's fields are in its type's order: label, x
    f.set_text(&p(&[6, 0]), "here".into());
    f.set_text(&p(&[7]), "00000000-0000-0000-0000-000000000002".into());
    f.set_text(&p(&[8]), "beef".into());
    let (args, bad) = f.read();
    assert_eq!(bad, [("count".to_string(), "count: a whole number".to_string())]);
    assert_eq!(args["title"], Value::text("a title"));
    assert_eq!(args["on"], Value::Bool(true));
    assert_eq!(args["level"], Value::text("high"));
    assert_eq!(args["note"], Value::Null, "an option at none is none, whatever was typed");
    assert_eq!(args["tags"], Value::list(vec![Value::Int(1), Value::Int(3)]));
    assert_eq!(args["at"].field("x"), Value::Int(4));
    assert_eq!(args["at"].field("label"), Value::text("here"));
    assert_eq!(args["who"], uid(2));
    assert_eq!(args["blob"], Value::Bytes(vec![0xbe, 0xef].into()));
    f.set_text(&p(&[1]), " 12 ".into());
    f.toggle(&p(&[4]));
    f.set_text(&p(&[7]), "not an id".into());
    let (args, bad) = f.read();
    assert_eq!(args["count"], Value::Int(12));
    assert_eq!(args["note"], Value::text("x"));
    assert_eq!(bad, [("who".to_string(), "who: an id, 32 hex digits".to_string())]);
    // Every line of the form, nested ones under their field.
    let labels: Vec<(usize, String)> = f.slots().into_iter().map(|s| (s.depth, s.label)).collect();
    assert!(labels.contains(&(1, "value".into())), "the option's value, now it has one");
    assert!(labels.contains(&(1, "2".into())), "the list's second item");
    assert!(labels.contains(&(1, "label".into())) && labels.contains(&(1, "x".into())));
    // `<Enter>` on a line: a bool flips, an enum moves on, a list grows.
    f.activate(&p(&[2]));
    f.activate(&p(&[3]));
    f.activate(&p(&[5]));
    let (args, _) = f.read();
    assert_eq!(args["on"], Value::Bool(false));
    assert_eq!(args["level"], Value::text("low"));
    assert!(
        f.read().1.iter().any(|(n, m)| n == "tags" && m == "tags.3: a whole number"),
        "{:?}",
        f.read().1
    );
}

/// The checks run before calling, with the module's own messages, and a call
/// that fails them reaches no writer. Falsified by calling whatever the
/// checks said: `make_team` was authored with an empty name.
#[test]
fn the_checks_run_before_calling_with_the_modules_messages() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let mut w = writer(&m, false);
    let mut ex = Explorer::new();
    let at = form::callables(&m).iter().position(|c| c.name == "make_team").unwrap();
    ex.update(Msg::Function(at), &src, &mut w);
    assert_eq!(ex.screen, Screen::Call("make_team".into()));
    ex.update(Msg::FormText(vec![0], "   ".into()), &src, &mut w);
    ex.update(Msg::FormText(vec![1], "11".into()), &src, &mut w);
    let f = ex.form().unwrap();
    assert_eq!(f.message("name"), Some("a team needs a name"));
    assert_eq!(f.message("size"), Some("size: between 1 and 10"));
    ex.update(Msg::Call, &src, &mut w);
    assert!(w.authored.is_empty(), "nothing is called that the checks refuse");
    assert!(ex.note.starts_with("not called: a team needs a name"), "{}", ex.note);
    ex.update(Msg::FormText(vec![0], "  green ".into()), &src, &mut w);
    ex.update(Msg::FormText(vec![1], "4".into()), &src, &mut w);
    assert_eq!(ex.form().unwrap().messages, vec![]);
    ex.update(key("r"), &src, &mut w);
    let (f, args) = w.authored.last().expect("called");
    assert_eq!(f, "make_team");
    assert_eq!(args["name"], Value::text("green"), "sent as the checks normalised it");
    assert_eq!(args["size"], Value::Int(4));
}

/// A query called from its form runs on the host's store with those
/// arguments, and its rows are a table; the keys walk the answer until
/// `<Esc>` hands them back to the form. Falsified by running it with no
/// arguments: the query refused the missing team and there was no answer.
#[test]
fn a_query_call_returns_rows() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let mut w = writer(&m, false);
    let mut ex = Explorer::new();
    ex.update(key("f"), &src, &mut w);
    assert_eq!(ex.screen, Screen::Functions);
    let at = form::callables(&m).iter().position(|c| c.name == "members").unwrap();
    for _ in 0..at {
        ex.update(key("j"), &src, &mut w);
    }
    ex.update(named(Named::Enter), &src, &mut w);
    assert_eq!(ex.screen, Screen::Call("members".into()));
    ex.update(Msg::FormText(vec![0], "blue".into()), &src, &mut w);
    ex.update(Msg::Call, &src, &mut w);
    let g = ex.answer_grid().expect("an answer");
    let name = g.columns.iter().position(|c| c == "name").unwrap();
    assert_eq!(
        g.rows.iter().map(|r| r[name].clone()).collect::<Vec<_>>(),
        [Value::text("ann"), Value::text("bob")]
    );
    assert!(ex.note.contains("2 rows"), "{}", ex.note);
    assert!(ex.in_answer);
    ex.update(key("j"), &src, &mut w);
    assert_eq!(ex.answer_at.0, 1);
    ex.update(named(Named::Escape), &src, &mut w);
    assert!(!ex.in_answer && ex.screen == Screen::Call("members".into()));
    assert!(w.authored.is_empty() && w.raws.is_empty(), "a query writes nothing");
    // Kept for the session, and called again as it was.
    ex.update(named(Named::Escape), &src, &mut w);
    assert_eq!(ex.screen, Screen::Functions);
    ex.update(key("."), &src, &mut w);
    assert_eq!(ex.calls.len(), 2);
    assert_eq!(ex.calls[1].args, ex.calls[0].args);
}

/// A writer that refuses a mutation now, or authors it and says later that
/// it was refused: a client's two ways of saying no.
struct Refusing {
    now: bool,
}

impl Writer for Refusing {
    fn exposed(&self) -> Vec<(TableName, CrudVerbs)> {
        vec![]
    }
    fn author(&mut self, _f: &str, _a: Args) -> Result<Ticket, String> {
        match self.now {
            true => Err("kick: not yours to kick".into()),
            false => Ok(Ticket("t1".into())),
        }
    }
    fn verdict(&self, _t: &Ticket) -> crate::data::Verdict {
        crate::data::Verdict::Refused("the authority says no".into())
    }
    fn raw(&mut self, _c: Change) -> Result<(), String> {
        Err("not the authority".into())
    }
}

/// A mutation called from its form reaches the writer with exactly the
/// arguments typed — an id chosen in the picker, an option, a list, an
/// enum, a bool — as the host's author would author it; what it came to is
/// the writer's verdict; a refusal now is said with its text. Falsified by
/// not filling the field the picker chose: the checks refused the call and
/// no writer was reached.
#[test]
fn a_mutation_call_reaches_the_writer_and_its_verdict_is_said() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let fill = |ex: &mut Explorer, w: &mut dyn Writer| {
        let at = form::callables(&m).iter().position(|c| c.name == "kick").unwrap();
        ex.update(Msg::Function(at), &src, w);
        ex.update(Msg::Pick(vec![0]), &src, w);
        ex.update(Msg::PickFilter("bob".into()), &src, w);
        ex.update(Msg::Picked(0), &src, w);
        ex.update(Msg::FormToggle(vec![1]), &src, w);
        ex.update(Msg::FormText(vec![1, 0], "late".into()), &src, w);
        ex.update(Msg::FormAdd(vec![2]), &src, w);
        ex.update(Msg::FormText(vec![2, 0], "a".into()), &src, w);
        ex.update(Msg::FormChoose(vec![3], 1), &src, w);
        ex.update(Msg::FormToggle(vec![4]), &src, w);
        ex.update(Msg::Call, &src, w);
    };
    let mut w = writer(&m, false);
    let mut ex = Explorer::new();
    fill(&mut ex, &mut w);
    let (f, args) = w.authored.last().expect("authored");
    assert_eq!(f, "kick");
    let mut want = Args::new();
    want.insert("user".into(), uid(2));
    want.insert("why".into(), Value::text("late"));
    want.insert("tags".into(), Value::list(vec![Value::text("a")]));
    want.insert("level".into(), Value::text("high"));
    want.insert("loud".into(), Value::Bool(true));
    assert_eq!(args, &want);
    assert!(w.raws.is_empty(), "a client never writes raw");
    assert!(matches!(&ex.calls.last().unwrap().called, Called::Authored(_)));
    assert_eq!(
        Explorer::outcome(ex.calls.last().unwrap(), &w),
        "authored",
        "the recorder keeps no verdict"
    );
    // Refused now, with its text; refused later, the verdict's.
    for (now, says) in [(true, "refused: kick: not yours to kick"), (false, "refused: the authority says no")] {
        let mut w = Refusing { now };
        let mut ex = Explorer::new();
        fill(&mut ex, &mut w);
        let call = ex.calls.last().expect("a call");
        assert_eq!(Explorer::outcome(call, &w), says);
        assert!(ex.note.ends_with(says), "{}", ex.note);
    }
}

/// The picker: a table's rows labelled by the column that tells them apart
/// and a second where there is one, the short id otherwise; filtered by a
/// substring of the label; walked and chosen with the keys, `/` filtering as
/// it is typed; and a table with no rows says so. Pasting an id works too.
/// Falsified by labelling by the first text column that holds anything: the
/// table whose rows all share one name was labelled by that name.
#[test]
fn the_picker_labels_filters_and_chooses() {
    let m = module();
    let mut st = store(&m);
    // `email` tells two of them apart; nobody's is the null.
    for (k, email) in [(1, "ann@x"), (3, "cat@y")] {
        let mut r = st.get("user", &[uid(k)]).unwrap();
        r = r.with("email", Value::text(email));
        st.put("user", r).unwrap();
    }
    assert_eq!(form::label_columns(&st, "user"), (Some("name".into()), Some("email".into())));
    let picks = form::pickables(&st, "user");
    let lines: Vec<String> = picks.iter().map(|p| p.line()).collect();
    assert_eq!(lines[0], "ann \u{b7} ann@x \u{b7} 00000000");
    assert_eq!(picks[1].secondary, None);
    assert_eq!(form::filter(&picks, "CAT"), [2]);
    assert_eq!(form::filter(&picks, ""), [0, 1, 2]);
    assert_eq!(form::filter(&picks, "zzz"), Vec::<usize>::new());
    // A column holding one value on every row is passed over for one that
    // tells rows apart.
    let mut same = MemoryStore::empty(m.schema.clone());
    same.put("team", row_of(&m, "team", &[("name", Value::text("t")), ("size", Value::Int(1))]))
        .unwrap();
    for (k, name) in [(1, "x"), (2, "y")] {
        same.put(
            "user",
            row_of(
                &m,
                "user",
                &[
                    ("id", uid(k)),
                    ("name", Value::text("same")),
                    ("team", Value::text("t")),
                    ("admin", Value::Bool(false)),
                    ("email", Value::text(name)),
                ],
            ),
        )
        .unwrap();
    }
    assert_eq!(form::label_columns(&same, "user"), (Some("email".into()), None));
    // By the keys.
    let src = source(&m, &st);
    let mut w = writer(&m, false);
    let mut ex = Explorer::new();
    let at = form::callables(&m).iter().position(|c| c.name == "kick").unwrap();
    ex.update(Msg::Function(at), &src, &mut w);
    ex.update(named(Named::Enter), &src, &mut w);
    assert!(ex.picker.is_some(), "<Enter> on an id field opens the picker");
    for c in ["/", "c", "a"] {
        ex.update(key(c), &src, &mut w);
    }
    assert_eq!(ex.picker.as_ref().unwrap().filter, "ca", "filtered as it is typed");
    assert_eq!(ex.pick_rows(&src).1, [2]);
    ex.update(named(Named::Enter), &src, &mut w); // accepts the search
    ex.update(named(Named::Enter), &src, &mut w); // chooses
    assert!(ex.picker.is_none());
    let (args, _) = ex.form().unwrap().read();
    assert_eq!(args["user"], uid(3));
    // `j` walks it, `<Esc>` closes it and leaves the field as it was.
    ex.update(named(Named::Enter), &src, &mut w);
    ex.update(key("j"), &src, &mut w);
    assert_eq!(ex.picker.as_ref().unwrap().at, 1);
    ex.update(named(Named::Escape), &src, &mut w);
    assert!(ex.picker.is_none() && ex.screen == Screen::Call("kick".into()));
    assert_eq!(ex.form().unwrap().read().0["user"], uid(3));
    // Pasted, with dashes; and refused when it is not one.
    ex.update(Msg::FormText(vec![0], "00000000-0000-0000-0000-000000000001".into()), &src, &mut w);
    assert_eq!(ex.form().unwrap().read().0["user"], uid(1));
    ex.update(Msg::FormText(vec![0], "0001".into()), &src, &mut w);
    assert_eq!(ex.form().unwrap().message("user"), Some("user: an id, 32 hex digits"));
    // A table with no rows says so.
    let empty = MemoryStore::empty(m.schema.clone());
    let src = source(&m, &empty);
    let mut ex = Explorer::new();
    ex.update(Msg::Function(at), &src, &mut w);
    ex.update(Msg::Pick(vec![0]), &src, &mut w);
    assert_eq!(ex.note, "user holds no rows here");
    assert_eq!(ex.pick_rows(&src).0.len(), 0);
}

/// A recent call is put back in its form, every field as it was called.
/// Falsified by filling the form blank: the team's name was empty.
#[test]
fn a_recent_call_is_put_back_in_its_form() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let mut w = writer(&m, false);
    let mut ex = Explorer::new();
    let at = form::callables(&m).iter().position(|c| c.name == "make_team").unwrap();
    ex.update(Msg::Function(at), &src, &mut w);
    ex.update(Msg::FormText(vec![0], "green".into()), &src, &mut w);
    ex.update(Msg::FormText(vec![1], "3".into()), &src, &mut w);
    ex.update(Msg::Call, &src, &mut w);
    ex.forms.clear();
    ex.update(Msg::Recall(0), &src, &mut w);
    let f = ex.form().expect("the form, again");
    assert_eq!(f.values[0], form::Input::Text("green".into()));
    assert_eq!(f.values[1], form::Input::Text("3".into()));
    ex.update(Msg::Again(0), &src, &mut w);
    assert_eq!(w.authored.len(), 2);
    assert_eq!(w.authored[0], w.authored[1]);
}

/// The form, the picker and an answer all draw. Falsified by a panic in
/// the picker's view: the test failed.
#[test]
fn the_functions_screens_draw() {
    let m = module();
    let st = store(&m);
    let src = source(&m, &st);
    let mut w = writer(&m, false);
    let mut ex = Explorer::new();
    ex.update(key("f"), &src, &mut w);
    let _ = ex.view(&src, &w);
    let kick = form::callables(&m).iter().position(|c| c.name == "kick").unwrap();
    ex.update(Msg::Function(kick), &src, &mut w);
    ex.update(Msg::FormAdd(vec![2]), &src, &mut w);
    ex.update(Msg::FormToggle(vec![1]), &src, &mut w);
    ex.update(Msg::Pick(vec![0]), &src, &mut w);
    let _ = ex.view(&src, &w);
    let members = form::callables(&m).iter().position(|c| c.name == "members").unwrap();
    ex.update(Msg::Function(members), &src, &mut w);
    ex.update(Msg::FormText(vec![0], "blue".into()), &src, &mut w);
    ex.update(Msg::Call, &src, &mut w);
    let _ = ex.view(&src, &w);
}

/// On the admin page a mutation is asked of the server's author route and
/// its verdict is the server's answer when it arrives: pending until then,
/// in the log at the sequence it names, or refused with the server's text.
/// Falsified by answering every ticket from the first answer: the second
/// call read "in the log, at 12" though the server refused it.
#[test]
fn the_admin_page_says_the_servers_verdict_when_it_arrives() {
    use crate::admin::{Message, Outbox, Page, Request};
    use crate::wire::State;
    let m = module();
    let st = store(&m);
    let state = State {
        module: ark::canon::encode(&ark::ir::module_value(&m)),
        store: st.store_value(),
        head: 0,
        lines: vec![],
        connections: vec![],
        verifies: vec![],
        exposed: vec![],
        raw: true,
        who: ark::raw::AUTHOR.into(),
    };
    let mut page = Page::new("http://127.0.0.1:8788".into(), None);
    let _ = page.update(Message::Loaded(Ok(state.to_json())));
    let at = form::callables(&m).iter().position(|c| c.name == "make_team").unwrap();
    let call = |page: &mut Page, name: &str| {
        let _ = page.update(Message::Explorer(Msg::Function(at)));
        let _ = page.update(Message::Explorer(Msg::FormText(vec![0], name.into())));
        let _ = page.update(Message::Explorer(Msg::FormText(vec![1], "2".into())));
        let _ = page.update(Message::Explorer(Msg::Call));
    };
    let said = |page: &Page| {
        let w = Outbox::of(&state, page.sent.len(), &page.answers);
        Explorer::outcome(page.explorer.calls.last().unwrap(), &w)
    };
    call(&mut page, "green");
    assert!(
        matches!(page.sent.as_slice(), [Request::Author(f, _)] if f == "make_team"),
        "{:?}",
        page.sent
    );
    assert_eq!(said(&page), "accepted, pending");
    let _ = page.update(Message::Answered(0, Ok("12".into())));
    assert_eq!(said(&page), "in the log, at 12");
    call(&mut page, "blue");
    let _ = page.update(Message::Answered(1, Err("team: a row with this key exists".into())));
    assert_eq!(said(&page), "refused: team: a row with this key exists");
    let first = Explorer::outcome(&page.explorer.calls[0], &Outbox::of(&state, page.sent.len(), &page.answers));
    assert_eq!(first, "in the log, at 12", "each call keeps its own answer");
}
