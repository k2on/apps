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

use crate::data::{crud_of, CrudVerbs, NoLog, Source, Writer};
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

/// A user table with its CRUD exposed, a team table with none, and one
/// query.
fn module() -> ark::ir::Module {
    let people = router::<People>("people");
    Module::new(people.routes((
        people.crud::<User>(),
        people.input::<OfTeam>().query("members", |_ctx, db, input| {
            db.user.filter(User::team.eq(input.team)).order_by(User::name.asc())
        }),
    )))
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
    fn author(&mut self, function: &str, args: Args) -> Result<(), String> {
        self.authored.push((function.into(), args));
        Ok(())
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
