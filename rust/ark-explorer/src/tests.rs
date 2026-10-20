//! The explorer's model, without a window (`docs/plan-guards.md` D4): what a
//! cell edit and a row delete become and which writer they reach, what the
//! console runs, what a cell's text reads back as, and how the keys walk it
//! — over a module of two tables written with `#[ark::*]` (§1.12).

use std::sync::OnceLock;

use ark5::author::{self as ark, Domain, Id, Input, Int, Node, Row, Schema, Sum, Table, Text};
use ark5::log::EntryId;
use ark5::peer::Ctx;
use ark5::store::{Change, Store};
use iced::keyboard::{key::Named, Key, Modifiers};

use crate::data::{crud_of, CrudVerbs, NoLog, Source, Ticket, Writer};
use crate::edit::{cell_text, console, parse_cell, route_delete, route_edit, Route, Via};
use crate::model::{Explorer, Msg, Outcome, Screen};
use crate::value::{self, Value};

#[derive(Schema)]
pub struct People {
    pub team: Table<Team>,
    pub user: Table<User>,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct Team {
    #[ark(key)]
    pub name: Text,
    pub size: Int,
}

#[derive(Row, Node, Clone, Debug, PartialEq)]
pub struct User {
    #[ark(key)]
    pub id: Id<User>,
    pub name: Text,
    #[ark(refs = Team)]
    pub team: Text,
    pub admin: bool,
    pub email: Option<Text>,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct UserKey {
    pub id: Id<User>,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct OfTeam {
    pub team: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct NewTeam {
    #[ark(trim, non_empty, why = "a team needs a name", len = "..=12")]
    pub name: Text,
    #[ark(range = "1..=10", why = "between 1 and 10")]
    pub size: Int,
}

#[derive(Sum, Node, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    High,
}

/// A mutation over every kind of field a form draws.
#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Kick {
    pub user: Id<User>,
    pub why: Option<Text>,
    #[ark(non_empty)]
    pub tags: Vec<Text>,
    pub level: Level,
    pub loud: bool,
}

#[ark::mutator("people.insert_user")]
pub fn insert_user(db: People, input: User) {
    db.user.insert(input);
}

#[ark::mutator("people.update_user")]
pub fn update_user(db: People, input: User) {
    db.user.update(input.id, |_u| input);
}

#[ark::mutator("people.put_user")]
pub fn put_user(db: People, input: User) {
    if db.user.get(input.id).is_some() {
        db.user.update(input.id, |_u| input)
    } else {
        db.user.insert(input)
    }
}

#[ark::mutator("people.delete_user")]
pub fn delete_user(db: People, input: UserKey) {
    db.user.delete(input.id);
}

#[ark::query("people.members")]
pub fn members(db: People, input: OfTeam) -> Vec<User> {
    db.user.filter(|u| u.team == input.team).sort(|u| u.name)
}

#[ark::mutator("teams.make_team")]
pub fn make_team(db: People, input: NewTeam) {
    db.team.insert(Team {
        name: input.name,
        size: input.size,
    });
}

#[ark::mutator("teams.kick")]
pub fn kick(db: People, input: Kick) {
    if db.user.get(input.user).is_none() {
        refuse!("no such user");
    }
    db.user.delete(input.user);
}

/// A user table with its CRUD exposed, a team table with none, one query,
/// and two mutations of the module's own on a second router.
fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(|| {
        ark::module! {
            schema: People,
            functions: [insert_user, update_user, put_user, delete_user, members, make_team, kick],
        }
        .unwrap_or_else(|r| panic!("{r}"))
    })
}

fn id(k: u8) -> Id<User> {
    let mut i = [0u8; 16];
    i[15] = k;
    Id::new(i)
}

fn uid(k: u8) -> Value {
    Value::Id(id(k).0)
}

fn user(k: u8, name: &str, team: &str) -> User {
    User {
        id: id(k),
        name: name.into(),
        team: team.into(),
        admin: false,
        email: None,
    }
}

fn store() -> Store {
    let mut st = Store::new(domain().schema.clone());
    for (name, size) in [("blue", 2), ("red", 1)] {
        st.put("team", Team { name: name.into(), size }.to_bytes()).unwrap();
    }
    for (k, name, team) in [(1, "ann", "blue"), (2, "bob", "blue"), (3, "cat", "red")] {
        st.put("user", user(k, name, team).to_bytes()).unwrap();
    }
    st
}

/// A writer that remembers what it was asked: a client's (no raw) or the
/// authority's.
#[derive(Default)]
struct Recorder {
    exposed: Vec<(String, CrudVerbs)>,
    authority: bool,
    authored: Vec<(String, Vec<u8>)>,
    raws: Vec<Change>,
    /// Every raw write asked for, refused or not.
    asked_raw: usize,
}

impl Writer for Recorder {
    fn exposed(&self) -> Vec<(String, CrudVerbs)> {
        self.exposed.clone()
    }
    fn author(&mut self, function: &str, input: Vec<u8>) -> Result<Ticket, String> {
        self.authored.push((function.into(), input));
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

fn exposed() -> Vec<(String, CrudVerbs)> {
    let d = domain();
    crud_of(&d.module, &d.lib)
        .into_iter()
        .map(|(t, v)| (t, CrudVerbs { may_author: true, ..v }))
        .collect()
}

fn writer(authority: bool) -> Recorder {
    Recorder {
        exposed: exposed(),
        authority,
        ..Recorder::default()
    }
}

fn source(st: &Store) -> Source<'_> {
    Source {
        store: st,
        module: &domain().module,
        lib: &domain().lib,
        log: &NoLog,
        ctx: Ctx {
            id: EntryId::of("s1", 0),
            actor: "alice".into(),
            session: "s1".into(),
            roles: vec![],
            now: 0,
        },
    }
}

fn key(c: &str) -> Msg {
    Msg::Key(Key::Character(c.into()), Modifiers::empty())
}

fn named(n: Named) -> Msg {
    Msg::Key(Key::Named(n), Modifiers::empty())
}

/// What a mutation was authored with, read as its input.
fn input<T: Node>(w: &Recorder) -> (String, T) {
    let (f, b) = w.authored.last().expect("authored");
    (f.clone(), T::from_bytes(b).unwrap_or_else(|e| panic!("{f}: {e}")))
}

fn at(name: &str) -> usize {
    let d = domain();
    crate::form::callables(&d.module, &d.lib)
        .iter()
        .position(|c| c.name.ends_with(&format!(".{name}")))
        .unwrap_or_else(|| panic!("no {name}"))
}

/// `crud_of` finds the four a module binds, by name and input — the table
/// without one has none, and a mutator of the right name that does not take
/// the row (or the key) is not one. Falsified by recognising the verbs by
/// name alone: the delete bound to `make_team`'s body was still counted.
#[test]
fn the_exposed_crud_is_read_off_the_module() {
    let d = domain();
    let crud = crud_of(&d.module, &d.lib);
    assert_eq!(crud.len(), 1, "{crud:?}");
    let (t, v) = &crud[0];
    assert_eq!(t, "user");
    assert!(v.insert && v.update && v.delete && v.put && !v.may_author);
    let mut odd = d.module.clone();
    let make = d.get("teams.make_team");
    for b in odd.bindings.iter_mut().filter(|(n, _)| n == "people.delete_user") {
        b.1 = make;
    }
    assert!(!crud_of(&odd, &d.lib)[0].1.delete, "a delete that does not take the key is not the CRUD verb");
}

/// A cell edit on a table whose CRUD is exposed goes through its
/// `update_<t>`, authored with the new row as its input, and reaches no raw
/// writer — on a client and at the authority alike, unless the switch says
/// raw. Falsified by routing an edit raw whenever the writer can: the
/// authority's edit of a user went raw.
#[test]
fn an_edit_goes_through_the_exposed_update() {
    let st = store();
    let src = source(&st);
    for authority in [false, true] {
        let mut w = writer(authority);
        let mut ex = Explorer::new();
        ex.update(Msg::Open("user".into()), &src, &mut w);
        ex.update(Msg::Cell(1, 1), &src, &mut w);
        ex.update(Msg::Edit, &src, &mut w);
        assert_eq!(ex.editing.as_ref().map(|e| (e.column.as_str(), e.text.as_str())), Some(("name", "bob")));
        ex.update(Msg::EditText("robert".into()), &src, &mut w);
        ex.update(Msg::Commit, &src, &mut w);
        assert!(w.raws.is_empty(), "{:?}", w.raws);
        let (f, u): (String, User) = input(&w);
        assert_eq!(f, "people.update_user");
        assert_eq!(u, user(2, "robert", "blue"), "every column, the name changed");
        assert!(ex.note.contains("through people.update_user"), "{}", ex.note);
    }
}

/// The switch, at the authority: the same edit written raw, as the edit
/// the row makes. On a client there is no switch: it says so and stays
/// off. Falsified by letting the switch turn on without a raw writer: the
/// client's switch said "writing raw".
#[test]
fn the_switch_writes_raw_at_the_authority_alone() {
    let st = store();
    let src = source(&st);
    let mut w = writer(true);
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
        [Change::Edit { table, old, new }] => {
            assert_eq!(table, "user");
            assert_eq!(User::from_bytes(old).unwrap(), user(1, "ann", "blue"));
            assert_eq!(User::from_bytes(new).unwrap(), User { admin: true, ..user(1, "ann", "blue") });
        }
        other => panic!("{other:?}"),
    }
    let mut client = writer(false);
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
    let st = store();
    let src = source(&st);
    let edit_size = |w: &mut Recorder| {
        let mut ex = Explorer::new();
        ex.update(Msg::Open("team".into()), &src, w);
        ex.update(Msg::Cell(0, 1), &src, w);
        ex.update(Msg::Edit, &src, w);
        ex.update(Msg::EditText("7".into()), &src, w);
        ex.update(Msg::Commit, &src, w);
        ex
    };
    let mut authority = writer(true);
    edit_size(&mut authority);
    assert!(
        matches!(authority.raws.as_slice(), [Change::Edit { table, new, .. }] if table == "team" && Team::from_bytes(new).unwrap().size == 7),
        "{:?}",
        authority.raws
    );
    let mut client = writer(false);
    let ex = edit_size(&mut client);
    assert!(client.asked_raw == 0 && client.authored.is_empty(), "no writer is reached");
    assert!(ex.note.contains("not the authority"), "{}", ex.note);
    assert!(ex.editing.is_some(), "the edit is kept, to be fixed or dropped");
}

/// A row delete: through `delete_<t>` with the key as its input where it is
/// exposed, raw where the host is the authority, refused where it is
/// neither. Falsified by giving the delete the whole row: its input did not
/// read as the key.
#[test]
fn a_delete_goes_through_delete_or_raw() {
    let st = store();
    let src = source(&st);
    let mut w = writer(true);
    let mut ex = Explorer::new();
    ex.update(Msg::Open("user".into()), &src, &mut w);
    ex.update(named(Named::ArrowDown), &src, &mut w);
    ex.update(key("x"), &src, &mut w);
    let (f, k): (String, UserKey) = input(&w);
    assert_eq!(f, "people.delete_user");
    assert_eq!(k.id, id(2));
    let team = st.rows("team").next().unwrap().to_vec();
    let authority = Via {
        verbs: None,
        can_raw: true,
        force_raw: false,
    };
    let m = &domain().module;
    assert_eq!(
        route_delete(m, authority, "team", &team),
        Ok(Route::Raw(Change::Remove {
            table: "team".into(),
            row: team.clone()
        }))
    );
    assert!(route_delete(m, Via::default(), "team", &team).is_err());
}

/// A key column names the row and is not edited, by either route. Falsified
/// by letting it through: the update carried a moved key.
#[test]
fn a_key_is_not_edited() {
    let st = store();
    let old = st.rows("user").next().unwrap().to_vec();
    let verbs = CrudVerbs {
        may_author: true,
        ..exposed()[0].1
    };
    let via = Via {
        verbs: Some(&verbs),
        can_raw: true,
        force_raw: false,
    };
    let r = route_edit(&domain().module, via, "user", &old, "id", uid(9));
    assert!(matches!(&r, Err(why) if why.contains("part of the key")), "{r:?}");
}

/// A cell's text reads back as the value it shows, for every column type,
/// and what is not one is refused with its column named. Falsified by
/// showing a bool as `yes`: the round trip failed.
#[test]
fn a_cell_reads_back_as_its_column() {
    let d = domain();
    let cx = value::types(&d.module);
    let ty = |t: &str, c: &str| {
        let (_, tbl) = d.schema.table(t).unwrap();
        value::column_ty(&cx, &tbl.columns[tbl.column(c).unwrap()])
    };
    for (c, v) in [
        ("id", uid(7)),
        ("name", Value::text("  spaced  ")),
        ("admin", Value::Bool(true)),
        ("email", Value::Null),
        ("email", Value::text("a@b")),
    ] {
        assert_eq!(parse_cell(c, &ty("user", c), &cell_text(&v)), Ok(v.clone()), "{c}");
    }
    assert_eq!(parse_cell("size", &ty("team", "size"), "-3"), Ok(Value::Int(-3)));
    assert!(parse_cell("size", &ty("team", "size"), "three").unwrap_err().contains("size"));
    assert!(parse_cell("admin", &ty("user", "admin"), "yes").is_err());
    assert!(parse_cell("id", &ty("user", "id"), "beef").is_err());
    assert_eq!(parse_cell("email", &ty("user", "email"), ""), Ok(Value::Null), "an empty option is none");
}

/// The console runs a query by name with its arguments, read-only; a
/// mutation's name is refused, and so is the wrong number of arguments.
/// Falsified by running whatever is named without the kind check: a
/// mutation was run as a query, and came back an answer of nothing rather
/// than a refusal.
#[test]
fn the_console_runs_queries_alone() {
    let st = store();
    let src = source(&st);
    let got = console(&src, "members", "blue").expect("members of blue");
    let Value::List(rows) = got else { panic!("{got:?}") };
    let names: Vec<Value> = rows
        .iter()
        .map(|r| match r {
            Value::Struct(vs) => vs[1].clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(names, [Value::text("ann"), Value::text("bob")]);
    let refused = console(&src, "put_user", "").unwrap_err();
    assert!(refused.contains("runs queries alone"), "{refused}");
    assert!(console(&src, "members", "").unwrap_err().contains("takes 1 argument"));
    assert!(console(&src, "nope", "").is_err());
    let mut ex = Explorer::new();
    let mut w = writer(false);
    ex.update(Msg::Show(Screen::Console), &src, &mut w);
    ex.update(Msg::Query("people.members".into()), &src, &mut w);
    ex.update(Msg::Args("red".into()), &src, &mut w);
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
    let st = store();
    let src = source(&st);
    let mut w = writer(false);
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
    let st = store();
    let src = source(&st);
    for authority in [false, true] {
        let w = writer(authority);
        let mut ex = Explorer::new();
        for s in [Screen::Tables, Screen::Table("user".into()), Screen::Log, Screen::Console] {
            ex.screen = s;
            let _ = ex.view(&src, &w);
        }
        ex.editing = Some(crate::model::Editing {
            table: "user".into(),
            key: id(1).to_bytes(),
            column: "name".into(),
            text: "ann".into(),
        });
        ex.screen = Screen::Table("user".into());
        let _ = ex.view(&src, &w);
    }
}

/// The admin state as the server would send it for this module and store.
fn state(st: &Store, exposed: Vec<(String, CrudVerbs)>) -> crate::wire::State {
    let d = domain();
    crate::wire::State {
        module: d.module.to_bytes(),
        bodies: d.bodies.values().cloned().collect(),
        rows: st.snapshot_rows(),
        head: 0,
        lines: vec![],
        connections: vec![],
        exposed,
        raw: true,
        who: "authority".into(),
    }
}

/// The admin page's wire reads back as itself: the state, a raw write's
/// body and a mutation's. Falsified by reading `may_author` from the
/// insert's field: the exposure came back authorable where it was not.
#[test]
fn the_admin_wire_reads_back() {
    use crate::data::{Connection, Line};
    use crate::wire::{author_body, author_from, raw_body, raw_from, State};
    let st = store();
    let entry = ark5::log::Entry {
        id: EntryId([7; 16]),
        actor: "authority".into(),
        session: "server-1".into(),
        local: 1,
        roles: vec![],
        func: domain().get("teams.make_team"),
        input: vec![],
        now: 5,
    };
    let row = st.rows("team").next().unwrap().to_vec();
    let mut s = state(
        &st,
        vec![(
            "user".into(),
            CrudVerbs {
                insert: true,
                update: true,
                delete: false,
                put: true,
                may_author: false,
            },
        )],
    );
    s.head = 3;
    s.lines = vec![
        Line {
            seq: Some(3),
            entry: entry.clone(),
            function: "teams.make_team".into(),
            facts: Some(vec![Change::Add {
                table: "team".into(),
                row: row.clone(),
            }]),
            standing: "in the log".into(),
        },
        Line {
            seq: None,
            entry,
            function: "people.put_user".into(),
            facts: None,
            standing: "pending".into(),
        },
    ];
    s.connections = vec![Connection {
        who: "alice".into(),
        cursor: 2,
        pending: None,
        note: "1 behind".into(),
    }];
    assert_eq!(State::from_body(&s.to_body()), Ok(s.clone()));
    let back = crate::admin::Loaded::of(State::from_body(&s.to_body()).unwrap()).expect("it loads");
    assert_eq!(back.store.rows("user").collect::<Vec<_>>(), st.rows("user").collect::<Vec<_>>());
    let change = Change::Edit {
        table: "team".into(),
        old: row.clone(),
        new: Team { name: "blue".into(), size: 9 }.to_bytes(),
    };
    assert_eq!(raw_from(&raw_body(&change)), Ok(change));
    let k = UserKey { id: id(1) }.to_bytes();
    assert_eq!(author_from(&author_body("people.delete_user", &k)), Ok(("people.delete_user".to_string(), k)));
}

/// The admin page's host: a state as the server sends it, read into a store
/// and a library the explorer edits like any other, and an edit asked of
/// the server — raw with the switch on, through the CRUD the module exposes
/// without it — as the request the server's routes read. Falsified by
/// sending the edit's old row rather than its new: the raw request carried
/// the size it had.
#[test]
fn the_admin_page_asks_the_server_for_its_writes() {
    use crate::admin::{hello_of, Loaded, Message, Page, Request};
    use crate::wire::{body, raw_from, Hello};
    let st = store();
    let mut page = Page::new("http://127.0.0.1:8788/".into(), None);
    assert_eq!(page.base, "http://127.0.0.1:8788");
    let _ = page.update(Message::Loaded(Ok(state(&st, exposed()).to_body())));
    let l: &Loaded = page.loaded.as_ref().expect("the state reads");
    assert_eq!(l.store.rows("user").collect::<Vec<_>>(), st.rows("user").collect::<Vec<_>>());
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
            assert_eq!(f, "people.update_user");
            assert_eq!(User::from_bytes(a).unwrap().name, "anne");
        }
        other => panic!("{other:?}"),
    }
    let _ = page.update(Message::Explorer(Msg::ToggleRaw));
    edit(&mut page, "team", 1, 1, "5");
    let Some(Request::Raw(change)) = page.sent.last().cloned() else {
        panic!("{:?}", page.sent)
    };
    let (path, b) = Request::Raw(change.clone()).to();
    assert_eq!(path, "/admin/api/raw");
    assert_eq!(raw_from(&b), Ok(change.clone()));
    assert!(
        matches!(&change, Change::Edit { table, new, .. } if table == "team" && Team::from_bytes(new).unwrap().size == 5),
        "{change:?}"
    );
    let said = Hello {
        open: false,
        server: Some("https://h.example".into()),
    };
    assert_eq!(hello_of(&body(&said)), Ok((false, Some("https://h.example".into()))));
    assert_eq!(hello_of(&body(&Hello { open: true, server: None })), Ok((true, None)));
}

// -- the table's cells, the functions, their forms and the picker ----------

use crate::cells::{self, Grid};
use crate::form::{self, Callable, Called, Field, Form, Kind};
use crate::value::Ty;

/// A long value is cut to its column and never drawn past it; a column is as
/// wide as its header and its values, within the bounds; an id is drawn as
/// eight hex digits; and the detail line holds the whole of any of them.
/// Falsified by returning the shown text uncut from `Grid::cell`: the
/// 210-character name came out whole into a column of 36.
#[test]
fn a_long_value_never_exceeds_its_column() {
    let mut st = store();
    let long = "Variatio 25 a 2 Clav. (Adagio), from the Goldberg Variations, BWV 988 ".repeat(3);
    st.put(
        "user",
        User {
            admin: true,
            ..user(9, &long, "red")
        }
        .to_bytes(),
    )
    .unwrap();
    let src = source(&st);
    let mut w = writer(false);
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
    let drawn = g.cell(at, 1);
    let (start, end) = drawn.split_once('\u{2026}').expect("the long value is cut");
    assert!(
        long.starts_with(start) && long.ends_with(end) && !end.is_empty(),
        "{drawn}: its start and its end"
    );
    assert_eq!(g.widths[3], cells::MIN_CHARS, "a column of booleans is narrow");
    assert_eq!(g.cell(at, 0), "00000000", "an id is its first eight hex digits");
    assert_eq!(g.detail(at, 1), Some(format!("name: {long}")), "the detail line is the whole value");
    assert_eq!(g.detail(at, 0), Some("id: 00000000-0000-0000-0000-000000000009".into()));
    ex.update(Msg::Cell(at, 1), &src, &mut w);
    assert_eq!(ex.detail(&src), Some(format!("name: {long}")));
    assert!(g.width_px() >= g.widths.iter().map(|w| cells::px(*w)).sum::<f32>());
}

/// A query's answer as a table: a list of records is a row each under the
/// record's fields; a list of values one column; one value one row.
/// Falsified by taking every answer as one column: the records' two fields
/// were drawn as one.
#[test]
fn a_query_answer_is_a_grid() {
    let ty = Ty::Struct(vec![("name".into(), Ty::Text), ("extra".into(), Ty::Int)]);
    let g = Grid::of_value(
        &ty,
        &Value::List(vec![
            Value::Struct(vec![Value::text("ann"), Value::Int(1)]),
            Value::Struct(vec![Value::text("bob"), Value::Int(2)]),
        ]),
    );
    assert_eq!(g.columns, ["name", "extra"]);
    assert_eq!(g.cell(1, 1), "2");
    let g = Grid::of_value(&Ty::Int, &Value::List(vec![Value::Int(1), Value::Int(2)]));
    assert_eq!((g.columns.as_slice(), g.rows.len()), (&["value".to_string()][..], 2));
    let g = Grid::of_value(&Ty::Int, &Value::Int(7));
    assert_eq!(g.rows, vec![vec![Value::Int(7)]]);
}

/// The Functions screen lists every query and mutation of the module, by
/// router in the order each router first appears, with its input — each
/// field labelled by what the module says of it — and its checks, the
/// module's refinements with their words. A query of a table's rows is
/// labelled by the table's columns. Falsified by listing mutations alone:
/// `members` was missing.
#[test]
fn every_query_and_mutation_is_listed_by_router() {
    let d = domain();
    let all = form::callables(&d.module, &d.lib);
    let names: Vec<(&str, &str)> = all.iter().map(|c| (c.router.as_str(), c.name.as_str())).collect();
    let routers: Vec<&str> = names.iter().map(|(r, _)| *r).collect();
    let first_teams = routers.iter().position(|r| *r == "teams").unwrap();
    assert!(routers[..first_teams].iter().all(|r| *r == "people"), "{names:?}");
    assert!(routers[first_teams..].iter().all(|r| *r == "teams"), "{names:?}");
    for want in ["members", "insert_user", "update_user", "delete_user", "put_user", "make_team", "kick"] {
        assert!(names.iter().any(|(_, n)| n.ends_with(want)), "{want} in {names:?}");
    }
    let members = &all[at("members")];
    assert_eq!(members.kind, Kind::Query);
    assert_eq!(members.columns.as_deref(), Some(&["id", "name", "team", "admin", "email"].map(String::from)[..]));
    let kick = &all[at("kick")];
    let said: Vec<String> = kick.input.iter().map(|(n, f)| format!("{n}: {}", form::ty_name(&f.ty))).collect();
    assert_eq!(
        said,
        [
            "user: id(user)",
            "option<text>: option(text)",
            "tags: list(text)",
            "level: enum(#0 | #1)",
            "bool: bool"
        ]
    );
    let make = &all[at("make_team")];
    assert_eq!(
        form::checks_said(&make.input[0].1),
        ["trimmed", "non-empty (\u{201c}a team needs a name\u{201d})", "max 12 chars"]
    );
    assert_eq!(form::checks_said(&make.input[1].1), ["between 1 and 10"]);
}

/// Every type reads from its input as its value, and what does not read is
/// said with the field's name: a text, an int (a word refused), a bool, an
/// enum, an option (none and some), a list (items added and taken out), a
/// record, an id (pasted with dashes, or refused), bytes. Falsified by
/// dropping the option's "none" reading: `note`, at none, was refused as not
/// an input of its type rather than read as none.
#[test]
fn every_type_reads_from_its_input() {
    let fs = |pairs: &[(&str, Ty)]| Ty::Struct(pairs.iter().map(|(k, t)| (k.to_string(), t.clone())).collect());
    let callable = Callable {
        router: "r".into(),
        name: "r.f".into(),
        kind: Kind::Mutation,
        input: [
            ("title", Ty::Text),
            ("count", Ty::Int),
            ("on", Ty::Bool),
            ("level", Ty::Enum(vec!["#0".into(), "#1".into()])),
            ("note", Ty::Option(Box::new(Ty::Text))),
            ("tags", Ty::List(Box::new(Ty::Int))),
            ("at", fs(&[("x", Ty::Int), ("label", Ty::Text)])),
            ("who", Ty::Id("user".into())),
            ("blob", Ty::Bytes),
        ]
        .into_iter()
        .map(|(n, ty)| (n.to_string(), Field { ty, checks: vec![] }))
        .collect(),
        record: true,
        returns: Ty::Struct(vec![]),
        columns: None,
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
    f.set_text(&p(&[6, 0]), "4".into());
    f.set_text(&p(&[6, 1]), "here".into());
    f.set_text(&p(&[7]), "00000000-0000-0000-0000-000000000002".into());
    f.set_text(&p(&[8]), "beef".into());
    let (read, bad) = f.read();
    assert_eq!(read, None);
    assert_eq!(bad, [("count".to_string(), "count: a whole number".to_string())]);
    f.set_text(&p(&[1]), " 12 ".into());
    let field = |v: &Option<Value>, i: usize| match v {
        Some(Value::Struct(vs)) => vs[i].clone(),
        other => panic!("{other:?}"),
    };
    let (args, bad) = f.read();
    assert_eq!(bad, []);
    assert_eq!(field(&args, 0), Value::text("a title"));
    assert_eq!(field(&args, 1), Value::Int(12));
    assert_eq!(field(&args, 2), Value::Bool(true));
    assert_eq!(field(&args, 3), Value::Variant(1, vec![]));
    assert_eq!(field(&args, 4), Value::Null, "an option at none is none, whatever was typed");
    assert_eq!(field(&args, 5), Value::List(vec![Value::Int(1), Value::Int(3)]));
    assert_eq!(field(&args, 6), Value::Struct(vec![Value::Int(4), Value::text("here")]));
    assert_eq!(field(&args, 7), uid(2));
    assert_eq!(field(&args, 8), Value::Bytes(vec![0xbe, 0xef]));
    let bytes = callable.input_bytes(args.as_ref().unwrap()).expect("it encodes");
    let back = value::decode(&Ty::Struct(callable.input.iter().map(|(n, f)| (n.clone(), f.ty.clone())).collect()), &bytes);
    assert_eq!(back.as_ref().ok(), args.as_ref(), "the bytes read back as the form");
    f.toggle(&p(&[4]));
    f.set_text(&p(&[7]), "not an id".into());
    let (_, bad) = f.read();
    assert_eq!(bad, [("who".to_string(), "who: an id, 32 hex digits".to_string())]);
    let labels: Vec<(usize, String)> = f.slots().into_iter().map(|s| (s.depth, s.label)).collect();
    assert!(labels.contains(&(1, "value".into())), "the option's value, now it has one");
    assert!(labels.contains(&(1, "2".into())), "the list's second item");
    assert!(labels.contains(&(1, "label".into())) && labels.contains(&(1, "x".into())));
    f.activate(&p(&[2]));
    f.activate(&p(&[3]));
    f.activate(&p(&[5]));
    assert_eq!((&f.values[2], &f.values[3]), (&form::Input::Bool(false), &form::Input::Choice(0)));
    let (_, bad) = f.read();
    assert!(bad.iter().any(|(n, m)| n == "tags" && m == "tags.3: a whole number"), "{bad:?}");
}

/// The checks run before calling, with the module's own messages — `parse`'s
/// words, against the field they are in — and a call that fails them
/// reaches no writer. Falsified by calling whatever the checks said:
/// `make_team` was authored with an empty name.
#[test]
fn the_checks_run_before_calling_with_the_modules_messages() {
    let st = store();
    let src = source(&st);
    let mut w = writer(false);
    let mut ex = Explorer::new();
    ex.update(Msg::Function(at("make_team")), &src, &mut w);
    assert_eq!(ex.screen, Screen::Call("teams.make_team".into()));
    ex.update(Msg::FormText(vec![0], "   ".into()), &src, &mut w);
    ex.update(Msg::FormText(vec![1], "11".into()), &src, &mut w);
    let f = ex.form().unwrap();
    assert_eq!(f.message("name"), Some("a team needs a name"));
    assert_eq!(f.message("size"), Some("between 1 and 10"));
    ex.update(Msg::Call, &src, &mut w);
    assert!(w.authored.is_empty(), "nothing is called that the checks refuse");
    assert!(ex.note.starts_with("not called: a team needs a name"), "{}", ex.note);
    ex.update(Msg::FormText(vec![0], "  green ".into()), &src, &mut w);
    ex.update(Msg::FormText(vec![1], "4".into()), &src, &mut w);
    assert_eq!(ex.form().unwrap().messages, vec![]);
    ex.update(key("r"), &src, &mut w);
    let (f, t): (String, NewTeam) = input(&w);
    assert_eq!(f, "teams.make_team");
    assert_eq!(t.name, "  green ", "the entry keeps the raw input; parse trims it when it runs");
    assert_eq!(t.size, 4);
}

/// A query called from its form runs on the host's store with that input,
/// and its rows are a table under the table's columns; the keys walk the
/// answer until `<Esc>` hands them back to the form. Falsified by running
/// it with an empty team: there were no rows.
#[test]
fn a_query_call_returns_rows() {
    let st = store();
    let src = source(&st);
    let mut w = writer(false);
    let mut ex = Explorer::new();
    ex.update(key("f"), &src, &mut w);
    assert_eq!(ex.screen, Screen::Functions);
    for _ in 0..at("members") {
        ex.update(key("j"), &src, &mut w);
    }
    ex.update(named(Named::Enter), &src, &mut w);
    assert_eq!(ex.screen, Screen::Call("people.members".into()));
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
    assert!(!ex.in_answer && ex.screen == Screen::Call("people.members".into()));
    assert!(w.authored.is_empty() && w.raws.is_empty(), "a query writes nothing");
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
    fn exposed(&self) -> Vec<(String, CrudVerbs)> {
        vec![]
    }
    fn author(&mut self, _f: &str, _a: Vec<u8>) -> Result<Ticket, String> {
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
/// input typed — an id chosen in the picker, an option, a list, an enum, a
/// bool — as the host's author would author it; what it came to is the
/// writer's verdict; a refusal now is said with its text. Falsified by not
/// filling the field the picker chose: the field did not read as an id and
/// no writer was reached.
#[test]
fn a_mutation_call_reaches_the_writer_and_its_verdict_is_said() {
    let st = store();
    let src = source(&st);
    let fill = |ex: &mut Explorer, w: &mut dyn Writer| {
        ex.update(Msg::Function(at("kick")), &src, w);
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
    let mut w = writer(false);
    let mut ex = Explorer::new();
    fill(&mut ex, &mut w);
    let (f, k): (String, Kick) = input(&w);
    assert_eq!(f, "teams.kick");
    assert_eq!(
        k,
        Kick {
            user: id(2),
            why: Some("late".into()),
            tags: vec!["a".into()],
            level: Level::High,
            loud: true,
        }
    );
    assert!(w.raws.is_empty(), "a client never writes raw");
    assert!(matches!(&ex.calls.last().unwrap().called, Called::Authored(_)));
    assert_eq!(Explorer::outcome(ex.calls.last().unwrap(), &w), "authored", "the recorder keeps no verdict");
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
    let m = &domain().module;
    let mut st = store();
    for (k, name, team, email) in [(1, "ann", "blue", "ann@x"), (3, "cat", "red", "cat@y")] {
        st.put(
            "user",
            User {
                email: Some(email.into()),
                ..user(k, name, team)
            }
            .to_bytes(),
        )
        .unwrap();
    }
    // `team` refers to `team`, so it is never a label; `name` is column 1.
    assert_eq!(form::label_columns(&st, m, "user"), (Some(1), None), "email is an option, not a text");
    let picks = form::pickables(&st, m, "user");
    let lines: Vec<String> = picks.iter().map(|p| p.line()).collect();
    assert_eq!(lines[0], "ann \u{b7} 00000000");
    assert_eq!(form::filter(&picks, "CAT"), [2]);
    assert_eq!(form::filter(&picks, ""), [0, 1, 2]);
    assert_eq!(form::filter(&picks, "zzz"), Vec::<usize>::new());
    // A column holding one value on every row says nothing about which row
    // it is: the short ids label them.
    let mut same = Store::new(domain().schema.clone());
    same.put("team", Team { name: "t".into(), size: 1 }.to_bytes()).unwrap();
    for k in [1, 2] {
        same.put("user", user(k, "same", "t").to_bytes()).unwrap();
    }
    assert_eq!(form::label_columns(&same, m, "user"), (None, None));
    assert_eq!(form::pickables(&same, m, "user")[1].label, "00000000");
    let src = source(&st);
    let mut w = writer(false);
    let mut ex = Explorer::new();
    ex.update(Msg::Function(at("kick")), &src, &mut w);
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
    let user0 = |ex: &Explorer| ex.form().unwrap().values[0].clone();
    assert_eq!(user0(&ex), form::Input::Text(form::id_text(&id(3).0)));
    ex.update(named(Named::Enter), &src, &mut w);
    ex.update(key("j"), &src, &mut w);
    assert_eq!(ex.picker.as_ref().unwrap().at, 1);
    ex.update(named(Named::Escape), &src, &mut w);
    assert!(ex.picker.is_none() && ex.screen == Screen::Call("teams.kick".into()));
    assert_eq!(user0(&ex), form::Input::Text(form::id_text(&id(3).0)));
    ex.update(Msg::FormText(vec![0], "0001".into()), &src, &mut w);
    assert_eq!(ex.form().unwrap().message("user"), Some("user: an id, 32 hex digits"));
    let empty = Store::new(domain().schema.clone());
    let src = source(&empty);
    let mut ex = Explorer::new();
    ex.update(Msg::Function(at("kick")), &src, &mut w);
    ex.update(Msg::Pick(vec![0]), &src, &mut w);
    assert_eq!(ex.note, "user holds no rows here");
    assert_eq!(ex.pick_rows(&src).0.len(), 0);
}

/// A recent call is put back in its form, every field as it was called.
/// Falsified by filling the form blank: the team's name was empty.
#[test]
fn a_recent_call_is_put_back_in_its_form() {
    let st = store();
    let src = source(&st);
    let mut w = writer(false);
    let mut ex = Explorer::new();
    ex.update(Msg::Function(at("make_team")), &src, &mut w);
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
    let st = store();
    let src = source(&st);
    let mut w = writer(false);
    let mut ex = Explorer::new();
    ex.update(key("f"), &src, &mut w);
    let _ = ex.view(&src, &w);
    ex.update(Msg::Function(at("kick")), &src, &mut w);
    ex.update(Msg::FormAdd(vec![2]), &src, &mut w);
    ex.update(Msg::FormToggle(vec![1]), &src, &mut w);
    ex.update(Msg::Pick(vec![0]), &src, &mut w);
    let _ = ex.view(&src, &w);
    ex.update(Msg::Function(at("members")), &src, &mut w);
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
    let st = store();
    let s = state(&st, vec![]);
    let mut page = Page::new("http://127.0.0.1:8788".into(), None);
    let _ = page.update(Message::Loaded(Ok(s.to_body())));
    let call = |page: &mut Page, name: &str| {
        let _ = page.update(Message::Explorer(Msg::Function(at("make_team"))));
        let _ = page.update(Message::Explorer(Msg::FormText(vec![0], name.into())));
        let _ = page.update(Message::Explorer(Msg::FormText(vec![1], "2".into())));
        let _ = page.update(Message::Explorer(Msg::Call));
    };
    let said = |page: &Page| {
        let w = Outbox::of(&s, page.sent.len(), &page.answers);
        Explorer::outcome(page.explorer.calls.last().unwrap(), &w)
    };
    call(&mut page, "green");
    assert!(
        matches!(page.sent.as_slice(), [Request::Author(f, _)] if f == "teams.make_team"),
        "{:?}",
        page.sent
    );
    assert_eq!(said(&page), "accepted, pending");
    let _ = page.update(Message::Answered(0, Ok("12".into())));
    assert_eq!(said(&page), "in the log, at 12");
    call(&mut page, "blue");
    let _ = page.update(Message::Answered(1, Err("team: a row with this key exists".into())));
    assert_eq!(said(&page), "refused: team: a row with this key exists");
    let first = Explorer::outcome(&page.explorer.calls[0], &Outbox::of(&s, page.sent.len(), &page.answers));
    assert_eq!(first, "in the log, at 12", "each call keeps its own answer");
}

// -- an edit ends when the click goes elsewhere ------------------------------

/// Start editing the name of the user at row `r`, and type `text`.
fn editing(ex: &mut Explorer, src: &Source, w: &mut Recorder, r: usize, text: &str) {
    ex.update(Msg::Open("user".into()), src, w);
    ex.update(Msg::Cell(r, 1), src, w);
    ex.update(Msg::Edit, src, w);
    ex.update(Msg::EditText(text.into()), src, w);
}

fn authored_name(w: &Recorder) -> (Id<User>, String) {
    let (_, u): (String, User) = input(w);
    (u.id, u.name)
}

/// A click on another row while a changed, valid value is in the editor
/// submits it exactly as <Enter> would — to the row it was typed on, not the
/// one clicked — and the cursor lands on the clicked row with no editor
/// open. Falsified by not ending the edit on a click: the editor stayed
/// open and nothing was written.
#[test]
fn a_click_elsewhere_submits_a_changed_edit() {
    let st = store();
    let src = source(&st);
    let mut w = writer(false);
    let mut ex = Explorer::new();
    editing(&mut ex, &src, &mut w, 0, "annie");
    ex.update(Msg::Cell(2, 3), &src, &mut w);
    assert!(ex.editing.is_none(), "no editor is left open");
    assert_eq!((ex.row_at, ex.column_at), (2, 3), "the cursor is where the click was");
    assert_eq!(w.authored[0].0, "people.update_user");
    assert_eq!(authored_name(&w), (id(1), "annie".into()), "on the row it was typed on");
    assert_eq!(w.authored.len(), 1);
    editing(&mut ex, &src, &mut w, 1, "robert");
    ex.update(named(Named::Enter), &src, &mut w);
    assert!(ex.editing.is_none());
    assert_eq!(authored_name(&w).1, "robert");
}

/// An edit that changed nothing is dropped by a click elsewhere, as <Esc>
/// would drop it: nothing is written, and nothing is said. Falsified by
/// submitting whatever the editor holds: the note said "nothing to change".
#[test]
fn a_click_elsewhere_drops_an_unchanged_edit() {
    let st = store();
    let src = source(&st);
    let mut w = writer(false);
    let mut ex = Explorer::new();
    editing(&mut ex, &src, &mut w, 0, "ann");
    ex.note = "before".into();
    ex.update(Msg::Cell(1, 1), &src, &mut w);
    assert_eq!(ex.note, "before", "dropped as <Esc> drops it");
    assert!(ex.editing.is_none());
    assert_eq!(ex.row_at, 1);
    assert!(w.authored.is_empty() && w.raws.is_empty(), "{:?}", w.authored);
}

/// An edit that does not read as its column's value is dropped by a click
/// elsewhere, and says why: nothing is written, and the editor does not go
/// with the cursor. Falsified by keeping the editor when the text does not
/// read: it stayed open over the row the cursor had left.
#[test]
fn a_click_elsewhere_drops_an_invalid_edit() {
    let st = store();
    let src = source(&st);
    let mut w = writer(true);
    let mut ex = Explorer::new();
    ex.update(Msg::Open("team".into()), &src, &mut w);
    ex.update(Msg::Cell(0, 1), &src, &mut w);
    ex.update(Msg::Edit, &src, &mut w);
    ex.update(Msg::EditText("three".into()), &src, &mut w);
    ex.update(Msg::Cell(1, 0), &src, &mut w);
    assert!(ex.editing.is_none());
    assert_eq!((ex.row_at, ex.column_at), (1, 0));
    assert!(w.raws.is_empty() && w.authored.is_empty());
    assert!(ex.note.contains("size: a whole number"), "{}", ex.note);
}

/// A click outside the table — another screen, or the app beside a docked
/// explorer, which its host says as `Blur` — ends the edit the same way.
/// Falsified by leaving `Blur` out of what ends an edit: the editor stayed
/// open after the click into the app, and nothing was written.
#[test]
fn a_click_outside_the_table_ends_the_edit() {
    let st = store();
    let src = source(&st);
    let mut w = writer(false);
    let mut ex = Explorer::new();
    editing(&mut ex, &src, &mut w, 2, "catherine");
    ex.update(Msg::Show(Screen::Log), &src, &mut w);
    assert!(ex.editing.is_none() && ex.screen == Screen::Log);
    assert_eq!(authored_name(&w).1, "catherine");
    editing(&mut ex, &src, &mut w, 0, "anne");
    ex.update(Msg::Blur, &src, &mut w);
    assert!(ex.editing.is_none());
    assert_eq!(ex.screen, Screen::Table("user".into()), "the explorer stays where it was");
    assert_eq!(authored_name(&w).1, "anne");
    assert_eq!(w.authored.len(), 2);
}

// -- what changed, lit as it lands --------------------------------------------

/// A table on screen: when the host's store moves, a changed cell is lit,
/// and only it; a new row is lit whole; a removed row is drawn, as gone,
/// while it fades; all of it fades out over FADE_MS. By key, not by place:
/// a row added above every other moves them all down one and lights none of
/// them. Falsified by diffing by place: the row added at the top was
/// compared with ann, who used to be there, and lit only where the two
/// differ — [1, 1, 1, 0, 0] — rather than whole, as new.
#[test]
fn a_change_in_the_store_is_lit_by_key_and_fades() {
    let st = store();
    let mut w = writer(false);
    let mut ex = Explorer::new();
    ex.update(Msg::Open("user".into()), &source(&st), &mut w);
    ex.observe(&source(&st), 1, 0);
    let (_, quiet) = ex.lit_table(&source(&st), "user");
    assert!(quiet.iter().all(|l| l.cells.iter().all(|s| *s == 0.0)), "nothing is lit on first sight");
    // bob renamed, dan added above everyone, cat gone.
    let mut moved = st.clone();
    moved.apply(&Change::Edit {
        table: "user".into(),
        old: user(2, "bob", "blue").to_bytes(),
        new: user(2, "robert", "blue").to_bytes(),
    })
    .unwrap();
    moved.put("user", user(0, "dan", "red").to_bytes()).unwrap();
    moved.delete("user", &id(3).to_bytes()).unwrap();
    ex.observe(&source(&moved), 1, 500);
    assert!(!ex.fading(), "the same generation is not a move: nothing is diffed");
    ex.observe(&source(&moved), 2, 1000);
    let (g, lights) = ex.lit_table(&source(&moved), "user");
    let name_of = |r: &Vec<Value>| cell_text(&r[1]);
    let drawn: Vec<(String, Option<usize>)> = g.rows.iter().zip(&lights).map(|(r, l)| (name_of(r), l.origin)).collect();
    assert_eq!(
        drawn,
        [
            ("dan".into(), Some(0)),
            ("ann".into(), Some(1)),
            ("cat".into(), None),
            ("robert".into(), Some(2))
        ],
        "the gone row is drawn where it was"
    );
    assert!(lights[0].cells.iter().all(|s| *s == 1.0), "a new row is lit whole: {:?}", lights[0]);
    assert!(lights[1].cells.iter().all(|s| *s == 0.0), "ann did not change, only moved: {:?}", lights[1]);
    assert_eq!(lights[2].gone, 1.0, "cat is drawn as gone");
    assert_eq!(lights[3].cells, [0.0, 1.0, 0.0, 0.0, 0.0], "only robert's name is lit");
    assert!(ex.fading());
    ex.observe(&source(&moved), 2, 1000 + crate::flash::FADE_MS / 2);
    let (_, lights) = ex.lit_table(&source(&moved), "user");
    assert!((lights[3].cells[1] - 0.75).abs() < 1e-6, "{:?}", lights[3]);
    ex.observe(&source(&moved), 2, 1000 + crate::flash::FADE_MS + 1);
    let (g, lights) = ex.lit_table(&source(&moved), "user");
    assert_eq!(g.rows.len(), 3, "the gone row has gone");
    assert!(lights.iter().all(|l| l.cells.iter().all(|s| *s == 0.0) && l.gone == 0.0));
    assert!(!ex.fading());
    ex.update(Msg::Open("team".into()), &source(&moved), &mut w);
    ex.observe(&source(&moved), 3, 5000);
    assert!(!ex.fading(), "a table seen for the first time lights nothing");
    let _ = ex.view(&source(&moved), &w);
}

/// A query's answer on its form is run again when the store moves, and its
/// rows are lit the same way: a new row whole, a changed cell alone, by the
/// column that names its rows. Falsified by not running the answer again:
/// it stayed two rows and nothing was lit.
#[test]
fn a_query_answer_is_run_again_and_lit() {
    let st = store();
    let mut w = writer(false);
    let mut ex = Explorer::new();
    ex.update(Msg::Function(at("members")), &source(&st), &mut w);
    ex.update(Msg::FormText(vec![0], "blue".into()), &source(&st), &mut w);
    ex.update(Msg::Call, &source(&st), &mut w);
    ex.observe(&source(&st), 1, 0);
    let mut moved = st.clone();
    moved.put("user", user(5, "abe", "blue").to_bytes()).unwrap();
    moved
        .apply(&Change::Edit {
            table: "user".into(),
            old: user(2, "bob", "blue").to_bytes(),
            new: user(2, "bobby", "blue").to_bytes(),
        })
        .unwrap();
    ex.observe(&source(&moved), 2, 100);
    let (g, lights) = ex.lit_answer().expect("an answer");
    let name = g.columns.iter().position(|c| c == "name").unwrap();
    let lit: Vec<(String, Vec<f32>)> = g.rows.iter().zip(&lights).map(|(r, l)| (cell_text(&r[name]), l.cells.clone())).collect();
    assert_eq!(lit.len(), 3, "{lit:?}");
    for (who, cells) in &lit {
        match who.as_str() {
            "abe" => assert!(cells.iter().all(|s| *s > 0.99), "{lit:?}"),
            "ann" => assert!(cells.iter().all(|s| *s == 0.0), "{lit:?}"),
            "bobby" => {
                assert!(cells[name] > 0.99, "{lit:?}");
                assert_eq!(cells.iter().filter(|s| **s > 0.0).count(), 1, "{lit:?}");
            }
            other => panic!("{other}"),
        }
    }
    let _ = ex.view(&source(&moved), &w);
}

