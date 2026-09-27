//! `add_to_playlist` and `create_playlist` written the way `arkc gen rust`
//! writes them, against `ark::gen`, run as one transaction beside the
//! interpreter: the two must agree on changes, store and hash. This is the
//! compile-time proof that the spellings GENERATED.md and GENERATED-RUST.md
//! promise exist, and the run-time proof that the fast path means what
//! `Ark.Eval` says.

use std::path::Path;

use ark::eval::apply;
use ark::gen::*;
use ark::hash::state_hash;
use ark::ir::{module_from_value, Module};
use ark::store::MemoryStore;
use ark::value::hex;

// -- what the emitter writes ------------------------------------------------

#[allow(clippy::needless_return)]
fn add_to_playlist(db: &mut Db, ctx: &Ctx, autos: &Args, args: &Args) -> Result<(), Fault> {
    let v0: Value = db.exists("playlist", vec![Ops::arg(args, "playlist_id")]);
    if Ops::not(v0).as_bool() {
        return Ok(());
    }
    let v1: Value = db.exists("playlist_item", vec![Ops::arg(args, "playlist_id"), Ops::arg(args, "media_id")]);
    if (v1).as_bool() {
        return Ok(());
    }
    let v2: Value = db.select(
        &Plan::from("playlist_item")
            .filter(Pred::cmp("playlist_id", CmpOp::Eq, Ops::arg(args, "playlist_id")))
            .order_by("pos", Dir::Desc)
            .order_by("playlist_id", Dir::Asc)
            .order_by("media_id", Dir::Asc)
            .limit(1),
    );
    let v4: Value = Std::unwrap_or(
        Ops::match_opt(
            Std::first(v2)?,
            |v3| -> Result<Value, Fault> { Ok((v3).field("pos")) },
            || -> Result<Value, Fault> { Ok(Value::null()) },
        )?,
        Value::int(0),
    )?;
    db.put(
        "playlist_item",
        Value::record(vec![
            ("added_ms".to_string(), Ops::arg(autos, "added_ms")),
            ("media_id".to_string(), Ops::arg(args, "media_id")),
            ("playlist_id".to_string(), Ops::arg(args, "playlist_id")),
            ("pos".to_string(), Ops::add(v4, Value::int(1))?),
            ("user_id".to_string(), Value::text(ctx.user.clone())),
        ]),
    )?;
    return Ok(());
}

fn create_playlist(db: &mut Db, ctx: &Ctx, autos: &Args, args: &Args) -> Result<(), Fault> {
    if Std::is_empty(Std::trim(Ops::arg(args, "name"))?)?.as_bool() {
        return Err(Fault::refuse("a playlist needs a name"));
    }
    let v0: Value = db.exists("playlist", vec![Ops::arg(autos, "id")]);
    if (v0).as_bool() {
        return Ok(());
    }
    db.put(
        "playlist",
        Value::record(vec![
            ("id".to_string(), Ops::arg(autos, "id")),
            ("name".to_string(), Std::trim(Ops::arg(args, "name"))?),
            ("user_id".to_string(), Value::text(ctx.user.clone())),
        ]),
    )?;
    Ok(())
}

// -- the comparison ---------------------------------------------------------

fn demo() -> Module {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors/module/demo.json");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let bytes = ark::value::decode_hex(json["bytes"].as_str().unwrap()).unwrap();
    module_from_value(&ark::canon::decode(&bytes).unwrap()).unwrap()
}

#[test]
fn generated_code_agrees_with_the_interpreter() {
    let m = demo();
    let ctx = Ctx::new("alice", "session-1");
    let pid = Value::id_hex("00000000-0000-0000-0000-000000000001");
    let mut fast = MemoryStore::empty(m.schema.clone());
    let mut slow = fast.clone();

    // create, with a name that needs trimming; then a refusal; then adds.
    let autos = Args::from([("id".to_string(), pid.clone())]);
    let named = |n: &str| Args::from([("name".to_string(), Value::text(n))]);
    let a = run_mutator(&mut fast, |db| create_playlist(db, &ctx, &autos, &named("  Favorites ")))
        .unwrap()
        .unwrap();
    let b = apply(&m, "create_playlist", &ctx, &autos, &named("  Favorites "), &mut slow)
        .unwrap()
        .unwrap();
    assert_eq!(a, b);
    let refused = run_mutator(&mut fast, |db| create_playlist(db, &ctx, &autos, &named("   ")))
        .unwrap()
        .unwrap_err();
    let refused_slow = apply(&m, "create_playlist", &ctx, &autos, &named("   "), &mut slow).unwrap().unwrap_err();
    assert_eq!(refused, refused_slow);
    assert_eq!(refused, ark::store::Refusal::Refused("a playlist needs a name".into()));

    let now = Args::from([("added_ms".to_string(), Value::int(1577836800000))]);
    for k in [7u8, 9, 7, 3] {
        let args = Args::from([("playlist_id".to_string(), pid.clone()), ("media_id".to_string(), Value::bytes(vec![k]))]);
        let a = run_mutator(&mut fast, |db| add_to_playlist(db, &ctx, &now, &args)).unwrap().unwrap();
        let b = apply(&m, "add_to_playlist", &ctx, &now, &args, &mut slow).unwrap().unwrap();
        assert_eq!(a, b, "adding {k}");
        assert_eq!(fast, slow);
    }
    assert_eq!(hex(&state_hash(&fast)), hex(&state_hash(&slow)));
    assert_eq!(fast.scan("playlist_item").len(), 3);

    // A store refusal reaches the caller structured, not as text alone.
    let orphan = Value::id_hex("00000000-0000-0000-0000-0000000000ff");
    let args = Args::from([("playlist_id".to_string(), orphan), ("media_id".to_string(), Value::bytes(vec![1]))]);
    let nothing = run_mutator(&mut fast, |db| add_to_playlist(db, &ctx, &now, &args)).unwrap().unwrap();
    assert!(nothing.is_empty(), "a missing playlist is a no-op, as the mutator says");
    let direct = run_mutator(&mut fast, |db| {
        db.put(
            "playlist_item",
            Value::record(vec![
                ("playlist_id", Value::id_hex("000000000000000000000000000000ff")),
                ("media_id", Value::bytes(vec![1])),
                ("pos", Value::int(1)),
                ("added_ms", Value::int(0)),
                ("user_id", Value::text("x")),
            ]),
        )
    })
    .unwrap()
    .unwrap_err();
    assert_eq!(
        direct,
        ark::store::Refusal::MissingParent("playlist_item".into(), "playlist_id".into(), "playlist".into())
    );
    assert_eq!(fast.scan("playlist_item").len(), 3, "a verdict leaves the store untouched");
}
