//! harken's domain against itself: the module it emits verifies and is the
//! committed `harken.ark`, and every procedure run natively agrees with the
//! interpreter over its own emit — verdicts, changes, stores and values —
//! step after step over one evolving store.

use std::collections::BTreeMap;
use std::path::Path;

use ark::authoring::Procedure;
use ark::eval::{Args, Ctx};
use ark::store::{MemoryStore, Refusal, Store};
use ark::value::Value;
use harken_domain::module;

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn id(k: u8) -> Value {
    let mut b = [0u8; 16];
    b[15] = k;
    Value::Id(b)
}

#[test]
fn the_module_verifies_and_is_the_committed_file() {
    let m = module();
    let bytes = m.emit();
    let decoded = ark::ir::module_from_value(&ark::canon::decode(&bytes).unwrap()).unwrap();
    let verified = ark::verify::verify(&decoded).unwrap_or_else(|es| panic!("{es:?}"));
    assert_eq!(ark::ir::module_value(&verified), ark::ir::module_value(&decoded), "emit is already the verified form");
    let names: Vec<&str> = decoded.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "add_track",
            "library",
            "signed_in",
            "owned",
            "create_playlist",
            "add_to_playlist",
            "remove_from_playlist",
            "playlists",
            "playlist_items"
        ]
    );
    let add = decoded.lookup_function("add_to_playlist").unwrap();
    assert_eq!(add.uses, ["signed_in", "owned"]);
    assert_eq!(decoded.lookup_function("create_playlist").unwrap().uses, ["signed_in"]);
    assert_eq!(decoded.lookup_router("playlists").unwrap().uses, ["signed_in", "owned"]);
    let committed = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("harken.ark")).unwrap();
    assert!(
        committed == bytes,
        "harken/domain/harken.ark is stale: `cargo run -p harken-domain -- ../harken/domain/harken.ark` rewrites it"
    );
}

struct Run {
    procs: BTreeMap<String, Procedure>,
    library: MemoryStore,
    playlists: MemoryStore,
}

impl Run {
    fn new() -> Run {
        let m = module();
        let schema = m.build().schema.clone();
        Run {
            procs: m.procedures().into_iter().map(|(_, p)| (p.name().to_string(), p)).collect(),
            library: MemoryStore::empty(schema.clone()),
            playlists: MemoryStore::empty(schema),
        }
    }

    fn store(&mut self, name: &str) -> &mut MemoryStore {
        if self.procs[name].function().scope.as_deref() == Some("library") {
            &mut self.library
        } else {
            &mut self.playlists
        }
    }

    /// Both ways over copies, compared; then applied for the next step.
    fn step(&mut self, name: &str, ctx: &Ctx, autos: Args, a: Args) -> Result<usize, String> {
        let p = self.procs[name].clone();
        let st = self.store(name).clone();
        let out = p.agrees(ctx, &autos, &a, &st).unwrap_or_else(|e| panic!("{e}"));
        let _ = p.apply(ctx, &autos, &a, self.store(name));
        match out {
            Ok(Ok(chs)) => Ok(chs.len()),
            Ok(Err(Refusal::Refused(t))) => Err(t),
            Ok(Err(other)) => Err(other.to_string()),
            Err(bug) => panic!("{name}: bug {bug:?}"),
        }
    }

    fn query(&mut self, name: &str, ctx: &Ctx, a: Args) -> Result<Value, String> {
        let p = self.procs[name].clone();
        let st = self.store(name).clone();
        match p.agrees_on_query(ctx, &a, &st).unwrap_or_else(|e| panic!("{e}")) {
            Ok(v) => Ok(v),
            Err(ark::eval::EvalFault::Verdict(Refusal::Refused(t))) => Err(t),
            Err(other) => panic!("{name}: {other:?}"),
        }
    }
}

fn track(title: &str, artist: &str, album: Option<&str>, ms: i64, file: &str) -> Args {
    args([
        ("title", Value::text(title)),
        ("artist", Value::text(artist)),
        ("album", Value::opt(album.map(Value::text))),
        ("duration_ms", Value::int(ms)),
        ("file", Value::text(file)),
    ])
}

fn now(k: u8) -> Args {
    args([("id", id(k)), ("added_ms", Value::int(1_000 + k as i64)), ("created_ms", Value::int(2_000 + k as i64))])
}

#[test]
fn every_procedure_agrees_with_the_interpreter() {
    let mut r = Run::new();
    let scanner = Ctx::new("library", "scan");
    let alice = Ctx::new("alice", "a");
    let bob = Ctx::new("bob", "b");
    let nobody = Ctx::new("", "x");

    // library: add_track and its checks.
    assert_eq!(r.step("add_track", &scanner, now(1), track(" Air ", "Bach", Some(" Suite 3 "), 300_000, "bach/air.flac")), Ok(1));
    assert_eq!(r.step("add_track", &scanner, now(2), track("Aria", "Bach", None, 250_000, "bach/aria.flac")), Ok(1));
    assert_eq!(r.step("add_track", &scanner, now(3), track("Air", "Bach", None, 1, "bach/air.flac")), Ok(0), "a rescan is a no-op");
    assert_eq!(
        r.step("add_track", &scanner, now(4), track("  ", "X", None, 1, "x.flac")),
        Err("a track needs a title".into())
    );
    assert_eq!(
        r.step("add_track", &scanner, now(5), track("T", "X", None, -1, "y.flac")),
        Err("duration_ms: at least 0".into())
    );
    assert_eq!(
        r.step("add_track", &scanner, now(6), track("T", "X", None, 1, "")),
        Err("file: at least 1 characters".into())
    );
    let lib = r.query("library", &alice, args([])).unwrap();
    let titles: Vec<Value> = lib.as_list().iter().map(|t| t.field("title")).collect();
    assert_eq!(titles, vec![Value::text("Aria"), Value::text("Air")], "by artist, album (None first), title");
    assert_eq!(lib.as_list()[1].field("album"), Value::text("Suite 3"), "an optional field is trimmed when Some");

    // playlists: the guard, the provide, the three writes.
    assert_eq!(
        r.step("create_playlist", &nobody, now(10), args([("name", Value::text("Mine"))])),
        Err("sign in first".into())
    );
    assert_eq!(r.step("create_playlist", &alice, now(11), args([("name", Value::text("  Favorites "))])), Ok(1));
    assert_eq!(r.step("create_playlist", &alice, now(12), args([("name", Value::text("Favorites"))])), Ok(0));
    assert_eq!(r.step("create_playlist", &bob, now(13), args([("name", Value::text("Favorites"))])), Ok(1));
    assert_eq!(
        r.step("create_playlist", &alice, now(14), args([("name", Value::text("   "))])),
        Err("a playlist needs a name".into())
    );
    assert_eq!(
        r.step("create_playlist", &alice, now(15), args([("name", Value::text("x".repeat(121)))])),
        Err("name: at most 120 characters".into())
    );
    let on = |p: u8, t: u8| args([("playlist_id", id(p)), ("track_id", id(t))]);
    let at = |k: u8| args([("added_ms", Value::int(k as i64))]);
    assert_eq!(r.step("add_to_playlist", &alice, at(20), on(11, 2)), Ok(1));
    assert_eq!(r.step("add_to_playlist", &alice, at(21), on(11, 1)), Ok(1));
    assert_eq!(r.step("add_to_playlist", &alice, at(22), on(11, 1)), Ok(0), "already there");
    assert_eq!(r.step("add_to_playlist", &alice, at(23), on(13, 1)), Err("not your playlist".into()));
    assert_eq!(r.step("add_to_playlist", &alice, at(24), on(99, 1)), Err("playlist_id: no such playlist".into()));
    assert_eq!(r.step("add_to_playlist", &nobody, at(25), on(11, 1)), Err("sign in first".into()));
    let items = r.query("playlist_items", &alice, args([("playlist_id", id(11))])).unwrap();
    let pos: Vec<(Value, Value)> = items.as_list().iter().map(|i| (i.field("track_id"), i.field("pos"))).collect();
    assert_eq!(pos, vec![(id(2), Value::int(1)), (id(1), Value::int(2))]);
    assert_eq!(r.query("playlist_items", &bob, args([("playlist_id", id(11))])), Err("not your playlist".into()));
    assert_eq!(r.step("remove_from_playlist", &bob, now(0), on(11, 2)), Err("not your playlist".into()));
    assert_eq!(r.step("remove_from_playlist", &alice, now(0), on(11, 2)), Ok(1));
    assert_eq!(r.step("remove_from_playlist", &alice, now(0), on(11, 2)), Ok(0));
    let mine = r.query("playlists", &alice, args([])).unwrap();
    assert_eq!(mine.as_list().len(), 1);
    assert_eq!(mine.as_list()[0].field("name"), Value::text("Favorites"));
    assert_eq!(r.query("playlists", &nobody, args([])), Err("sign in first".into()));
    assert_eq!(r.playlists.scan("playlist_item").len(), 1);
}
