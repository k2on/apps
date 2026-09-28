//! The demo of `spec/AUTHORING.md` Appendix B, authored in the vocabulary:
//! its `emit` is `spec/vectors/module/demo.json`'s bytes (check 1 of §5),
//! and every procedure run `Native` agrees with the interpreter over its
//! own `Emit` (check 3).

use std::path::Path;

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::store::{MemoryStore, Refusal, Store};
use ark::value::{hex, Value};

pub struct Demo {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}
impl Scope for Demo {
    const NAME: &str = "demo";
    fn open() -> Self {
        Demo {
            playlist: table(),
            item: table(),
        }
    }
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const item: Rel<Self, Item> = rel("item");
}

pub struct Item {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
    pub pos: Int,
}
impl Row for Item {
    const NAME: &str = "item";
    type Key = (Id<Playlist>, Text);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .text(Self::track_id)
            .int(Self::pos)
            .key((Self::playlist_id, Self::track_id))
            .unique((Self::playlist_id, Self::pos))
    }
}
#[allow(non_upper_case_globals)]
impl Item {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const track_id: Col<Self, Text> = col("track_id");
    pub const pos: Col<Self, Int> = col("pos");
}

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name"))
    }
}

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
    }
}

pub struct PlaylistId {
    pub playlist_id: Id<Playlist>,
}
impl Input for PlaylistId {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

pub fn demo() -> Router<Demo> {
    let demo = router::<Demo>("demo");
    demo.routes((
        demo.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            db.playlist
                .insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                })
                .on((Playlist::user_id, Playlist::name))
        }),
        demo.input::<AddToPlaylist>().mutation("add_to_playlist", |_ctx, db, input| {
            let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
            db.item.insert(Item {
                playlist_id: input.playlist_id,
                track_id: input.track_id,
                pos: last.map_or(0, |row| row.pos).add(1),
            })
        }),
        demo.input::<PlaylistId>().query("items", |_ctx, db, input| {
            db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.asc()).all()
        }),
    ))
}

fn module() -> Module {
    Module::new((demo(),))
}

fn vector() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors/module/demo.json");
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

#[test]
fn the_demo_emits_what_the_spec_records() {
    let v = vector();
    let m = module();
    assert_eq!(hex(&m.emit()), v["bytes"].as_str().unwrap(), "canonical bytes");
    assert_eq!(hex(&m.hash()), v["hash"].as_str().unwrap(), "module hash");
}

#[test]
fn the_demo_emits_a_module_that_verifies_and_decodes_to_itself() {
    let m = module();
    let built = m.build();
    let again = ark::verify::verify(built).unwrap();
    assert_eq!(
        ark::ir::module_value(&again),
        ark::ir::module_value(built),
        "emit returns the verified form"
    );
    let back = ark::ir::module_from_value(&ark::canon::decode(&m.emit()).unwrap()).unwrap();
    assert_eq!(ark::ir::module_value(&back), ark::ir::module_value(built));
    let names: Vec<&str> = built.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["create_playlist", "add_to_playlist", "items"]);
    let add = built.lookup_function("add_to_playlist").unwrap();
    // §6: `.first()` is a bound select of one row, then `EStd First` of it, bound.
    assert!(matches!(&add.body[0], ark::ir::Stmt::Let(0, ark::ir::Expr::Select(p)) if p.limit == Some(1)));
    assert!(matches!(
        &add.body[1],
        ark::ir::Stmt::Let(1, ark::ir::Expr::Std(ark::ir::StdFn::First, _))
    ));
    assert_eq!(
        built.schema.scopes[0].tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["playlist", "item"]
    );
}

fn idv(k: u8) -> Value {
    let mut b = [0u8; 16];
    b[15] = k;
    Value::Id(b)
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Every mutator of the demo, natively and through its emitted closure,
/// over copies of one store, step after step: the same verdicts, changes
/// and stores; and every query, the same value.
#[test]
fn native_agrees_with_the_interpreter_on_every_procedure() {
    let m = module();
    let procs = m.procedures();
    let get = |n: &str| procs.iter().find(|(_, p)| p.name() == n).unwrap().1.clone();
    let (create, add, items) = (get("create_playlist"), get("add_to_playlist"), get("items"));
    let alice = eval::Ctx::new("alice", "a");
    let bob = eval::Ctx::new("bob", "b");
    let mut st = MemoryStore::empty(m.build().schema.clone());
    let mut verdicts = Vec::new();
    let steps: Vec<(&Procedure, &eval::Ctx, Args, Args)> = vec![
        (&create, &alice, args([("id", idv(1))]), args([("name", Value::text("  Favorites "))])),
        (&create, &alice, args([("id", idv(2))]), args([("name", Value::text("Favorites"))])),
        (&create, &bob, args([("id", idv(3))]), args([("name", Value::text("Favorites"))])),
        (&create, &alice, args([("id", idv(4))]), args([("name", Value::text("   "))])),
        (&add, &alice, args([]), args([("playlist_id", idv(1)), ("track_id", Value::text("t1"))])),
        (&add, &alice, args([]), args([("playlist_id", idv(1)), ("track_id", Value::text("t2"))])),
        (&add, &alice, args([]), args([("playlist_id", idv(1)), ("track_id", Value::text("t1"))])),
        (&add, &alice, args([]), args([("playlist_id", idv(9)), ("track_id", Value::text("t1"))])),
        (&add, &bob, args([]), args([("playlist_id", idv(3)), ("track_id", Value::text(""))])),
    ];
    for (p, ctx, autos, a) in steps {
        let out = p.agrees(ctx, &autos, &a, &st).unwrap_or_else(|e| panic!("{e}"));
        verdicts.push(out.clone().map(|r| r.map(|chs| chs.len())));
        p.apply(ctx, &autos, &a, &mut st).unwrap().ok();
    }
    assert_eq!(
        verdicts,
        vec![
            Ok(Ok(1)),
            Ok(Ok(0)),
            Ok(Ok(1)),
            Ok(Err(Refusal::Refused("a playlist needs a name".into()))),
            Ok(Ok(1)),
            Ok(Ok(1)),
            Ok(Ok(0)),
            Ok(Err(Refusal::Refused("playlist_id: no such playlist".into()))),
            Ok(Err(Refusal::Refused("track_id: at least 1 characters".into()))),
        ]
    );
    assert_eq!(st.get("playlist", &[idv(1)]).unwrap()["name"], Value::text("Favorites"), "trimmed");
    let rows = items.agrees_on_query(&alice, &args([("playlist_id", idv(1))]), &st).unwrap().unwrap();
    let pos: Vec<(Value, Value)> = rows.as_list().iter().map(|r| (r.field("track_id"), r.field("pos"))).collect();
    assert_eq!(pos, vec![(Value::text("t1"), Value::int(1)), (Value::text("t2"), Value::int(2))]);
    // The form validator: messages per field, values normalised.
    let checked = create.check(&alice, &args([("name", Value::text("  x "))]), &st).unwrap();
    assert!(checked.messages.is_empty());
    assert_eq!(checked.values["name"], Value::text("x"));
    let checked = add.check(&alice, &args([("playlist_id", idv(7))]), &st).unwrap();
    assert_eq!(
        checked.messages,
        vec![("playlist_id".to_string(), "playlist_id: no such playlist".to_string())]
    );
}

/// The agreement check is a real check: a body that reads the host's own
/// state (here a counter) emits the value it saw once, and runs natively
/// with whatever it sees now — which `agrees` reports.
#[test]
fn a_body_that_reads_the_host_disagrees_and_is_caught() {
    use std::sync::atomic::{AtomicI64, Ordering};
    static SEEN: AtomicI64 = AtomicI64::new(0);
    let r = router::<Demo>("demo");
    let m = Module::new((r.routes((r.input::<AddToPlaylist>().mutation("sneaky", |_ctx, db, input| {
        let n = SEEN.fetch_add(1, Ordering::SeqCst);
        db.item.insert(Item {
            playlist_id: input.playlist_id,
            track_id: input.track_id,
            pos: Int::from(n),
        })
    }),)),));
    let (_, p) = m.procedure("sneaky").unwrap();
    let mut st = MemoryStore::empty(m.build().schema.clone());
    st.apply_change(&ark::store::Change::Add(
        "playlist".into(),
        [
            ("id".to_string(), idv(1)),
            ("name".to_string(), Value::text("P")),
            ("user_id".to_string(), Value::text("a")),
        ]
        .into(),
    ));
    let ctx = eval::Ctx::new("a", "s");
    let why = p.agrees(&ctx, &args([]), &args([("playlist_id", idv(1)), ("track_id", Value::text("t"))]), &st);
    assert!(why.is_err(), "{why:?}");
}

/// The v2 verifier rules, each on the demo with one thing broken.
#[test]
fn the_verifier_refuses_what_v2_forbids() {
    use ark::ir::{Check, Expr, Stmt};
    use ark::verify::{verify, Complaint, VerifyError};
    let good = module().build().clone();
    let refused = |m: &ark::ir::Module| -> Vec<Complaint> {
        match verify(m) {
            Ok(_) => vec![],
            Err(es) => es
                .into_iter()
                .filter_map(|e| match e {
                    VerifyError::In(_, c) => Some(c),
                    _ => None,
                })
                .collect(),
        }
    };
    let mut m = good.clone();
    if let Stmt::Insert(_, _, on) = &mut m.functions[0].body[0] {
        *on = vec!["name".into()];
    }
    assert_eq!(refused(&m), vec![Complaint::OnNotUnique("playlist".into(), vec!["name".into()])]);
    let mut m = good.clone();
    m.functions[1].input[1].1.checks.push(Check::Exists(None));
    assert!(matches!(refused(&m)[..], [Complaint::CheckOnWrongType(..)]));
    let mut m = good.clone();
    m.functions[0].uses = vec!["nope".into()];
    assert!(matches!(refused(&m)[..], [Complaint::UsesNotOnRouter(_)]));
    let mut m = good.clone();
    m.functions[2].body.insert(0, Stmt::Let(9, Expr::Provided("owned".into())));
    assert_eq!(refused(&m), vec![Complaint::NotProvided("owned".into())]);
    let mut m = good.clone();
    m.functions[2].body.insert(
        0,
        Stmt::Delete("item".into(), vec![Expr::Arg("playlist_id".into()), Expr::Lit(Value::text("t"))]),
    );
    assert_eq!(refused(&m), vec![Complaint::WriteOutsideMutator]);
    assert!(verify(&good).is_ok());
}
