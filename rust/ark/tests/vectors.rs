//! The conformance runner: every directory under `spec/vectors`, checked as
//! `spec/README.md` ("The vectors") says. A vector is JSON with wrappers —
//! `{"$int": "…"}`, `{"$bytes": "hex"}`, `{"$id": "8-4-4-4-12"}` — and every
//! directory's `falsify/` vectors must fail its check: each check returns
//! its first unmet claim as an `Err` rather than panicking, so the one
//! definition runs both ways (falsified by dropping the module hash from
//! `check_module`, which lets `module/falsify/` pass and fails the test).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ark::canon::{decode, encode};
use ark::eval::{apply, Args, Ctx};
use ark::hash::{closure, closures, function_hash, module_hash, state_hash, FnHash};
use ark::ir::{module_from_value, module_value, Module};
use ark::log::{Entry, Seq};
use ark::peer::{local_commit, AdoptError, Authority, Changes, Replica, Sequenced};
use ark::protocol::{change_from_value, change_value, entry_from_value, ClientMsg, ServerMsg};
use ark::schema::{check_schema, Schema};
use ark::sim::Sim;
use ark::stdlib::id_of_text;
use ark::store::{Change, MemoryStore, Store};
use ark::value::{compare_value, decode_hex, hex, Id, Value};
use ark::view::{contract, hydrate, push_all, splice, Env, Patch};

fn vectors() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors")
}

fn files(dir: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(vectors().join(dir))
        .unwrap_or_else(|e| panic!("reading vectors/{dir}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no vectors under {dir}");
    out
}

fn read(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A JSON vector value as an Ark value, unwrapping the three wrappers.
fn value(j: &serde_json::Value) -> Value {
    use serde_json::Value as J;
    match j {
        J::Null => Value::Null,
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => Value::Int(n.as_i64().expect("an integer")),
        J::String(s) => Value::Text(s.clone()),
        J::Array(xs) => Value::List(xs.iter().map(value).collect()),
        J::Object(m) => {
            if m.len() == 1 {
                if let Some(J::String(s)) = m.get("$int") {
                    return Value::Int(s.parse().expect("an int"));
                }
                if let Some(J::String(s)) = m.get("$bytes") {
                    return Value::Bytes(decode_hex(s).expect("hex"));
                }
                if let Some(J::String(s)) = m.get("$id") {
                    return Value::Id(id_of_text(s).expect("an id"));
                }
            }
            Value::Struct(m.iter().map(|(k, v)| (k.clone(), value(v))).collect())
        }
    }
}

fn bytes_of(j: &serde_json::Value) -> Vec<u8> {
    decode_hex(j.as_str().expect("hex text")).expect("hex")
}

fn args_of(v: &Value) -> Args {
    v.as_struct().clone()
}

fn module_of(j: &serde_json::Value) -> Module {
    module_from_value(&value(j)).unwrap_or_else(|e| panic!("module: {e}"))
}

fn id_n(k: u8) -> Id {
    let mut id = [0u8; 16];
    id[15] = k;
    id
}

/// A claim a check makes: `Err` with the message when it does not hold,
/// so that the same check can be required to fail on a `falsify/` vector.
macro_rules! ensure {
    ($ok:expr, $($why:tt)+) => {
        if !$ok {
            return Err(format!($($why)+));
        }
    };
}

/// [`ensure!`] for two values that must be equal, naming both when not.
macro_rules! ensure_eq {
    ($a:expr, $b:expr, $($why:tt)+) => {{
        let (a, b) = (&$a, &$b);
        if a != b {
            return Err(format!("{}: {:?} != {:?}", format!($($why)+), a, b));
        }
    }};
}

/// Every vector under `dir/falsify/` names `"expect": "fail"` and fails
/// `check`: a runner that passes by not reading the expected value is
/// caught by it.
fn falsified(dir: &str, check: impl Fn(&Path, &serde_json::Value) -> Result<(), String>) {
    for p in files(&format!("{dir}/falsify")) {
        let v = read(&p);
        assert_eq!(v["expect"], "fail", "{}", p.display());
        assert!(check(&p, &v).is_err(), "{} passed and must not", p.display());
    }
}

/// Run `check` over every vector of `dir`, then require each of its
/// `falsify/` vectors to fail it.
fn run(dir: &str, check: impl Fn(&Path, &serde_json::Value) -> Result<(), String>) {
    for p in files(dir) {
        check(&p, &read(&p)).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    }
    falsified(dir, check);
}

// codec/ ------------------------------------------------------------------

fn check_codec(v: &serde_json::Value) -> Result<(), String> {
    let val = value(&v["value"]);
    let bytes = bytes_of(&v["bytes"]);
    if encode(&val) != bytes {
        return Err(format!("encode: {} != {}", hex(&encode(&val)), hex(&bytes)));
    }
    match decode(&bytes) {
        Ok(d) if d == val => Ok(()),
        other => Err(format!("decode: {other:?}")),
    }
}

#[test]
fn codec() {
    run("codec", |_, v| check_codec(v));
}

// order/ ------------------------------------------------------------------

fn check_order(v: &serde_json::Value) -> Result<(), String> {
    let mut input = value(&v["input"]).as_list();
    input.sort_by(compare_value);
    ensure_eq!(Value::List(input), value(&v["sorted"]), "sorted");
    Ok(())
}

#[test]
fn order() {
    run("order", |_, v| check_order(v));
}

// hash/ -------------------------------------------------------------------

fn check_hash(v: &serde_json::Value) -> Result<(), String> {
    let m = module_of(&v["module"]);
    let st = MemoryStore::from_value(m.schema.clone(), &value(&v["store"]));
    ensure_eq!(hex(&state_hash(&st)), v["hash"].as_str().unwrap_or(""), "state hash");
    Ok(())
}

#[test]
fn hash() {
    run("hash", |_, v| check_hash(v));
}

// module/ -----------------------------------------------------------------

fn check_module(v: &serde_json::Value) -> Result<(), String> {
    let val = value(&v["module"]);
    let bytes = bytes_of(&v["bytes"]);
    ensure_eq!(decode(&bytes).map_err(|e| e.to_string())?, val, "the bytes decode to the module value");
    let m = module_from_value(&val).map_err(|e| e.to_string())?;
    ensure_eq!(m.spec, ark::ir::SPEC_VERSION, "spec version");
    ensure_eq!(module_value(&m), val, "decode then encode is the identity");
    ensure_eq!(encode(&module_value(&m)), bytes, "the module encodes to the bytes");
    ensure_eq!(hex(&module_hash(&m)), v["hash"].as_str().unwrap_or(""), "module hash");
    ensure!(check_schema(&m.schema).is_empty(), "the schema is not well formed");
    Ok(())
}

#[test]
fn module() {
    run("module", |_, v| check_module(v));
}

// verify/ -----------------------------------------------------------------

fn check_verify(v: &serde_json::Value) -> Result<(), String> {
    let m = module_from_value(&value(&v["module"])).map_err(|e| e.to_string())?;
    ensure!(v["verifies"].as_bool() == Some(true), "only accepting modules are checked here");
    ensure!(check_schema(&m.schema).is_empty(), "the schema is not well formed");
    ensure_eq!(m.spec, ark::ir::SPEC_VERSION, "spec version");
    let verified = ark::verify::verify(&m).map_err(|es| format!("{es:?}"))?;
    ensure_eq!(module_value(&verified), module_value(&m), "a verified module is its own verified form");
    // Every function of a verified module hashes.
    ensure_eq!(closures(&m).len(), m.functions.len(), "one closure per function");
    Ok(())
}

#[test]
fn verify() {
    run("verify", |_, v| check_verify(v));
}

// protocol/ ---------------------------------------------------------------

fn check_protocol(name: &str, v: &serde_json::Value) -> Result<(), String> {
    let val = value(&v["frame"]);
    let bytes = bytes_of(&v["bytes"]);
    ensure_eq!(decode(&bytes).map_err(|e| e.to_string())?, val, "the bytes decode to the frame");
    ensure_eq!(encode(&val), bytes, "the frame encodes to the bytes");
    if name.starts_with("client-") {
        let f = ClientMsg::from_value(&val).map_err(|e| e.to_string())?;
        ensure_eq!(f.to_value(), val, "decode then encode");
        ensure_eq!(ClientMsg::from_value(&f.to_value()).map_err(|e| e.to_string())?, f, "encode then decode");
    } else {
        let f = ServerMsg::from_value(&val).map_err(|e| e.to_string())?;
        ensure_eq!(f.to_value(), val, "decode then encode");
        ensure_eq!(ServerMsg::from_value(&f.to_value()).map_err(|e| e.to_string())?, f, "encode then decode");
    }
    Ok(())
}

#[test]
fn protocol() {
    run("protocol", |p, v| check_protocol(p.file_name().unwrap().to_str().unwrap(), v));
}

// eval/ -------------------------------------------------------------------

/// One function applied step by step: the changes, the store and the hash
/// after each, and the same through the closure map as a peer replays it.
fn check_eval(v: &serde_json::Value) -> Result<(), String> {
    let m = module_of(&v["module"]);
    let name = v["function"].as_str().unwrap();
    let f = m.lookup_function(name).expect("the function");
    let fh = function_hash(&closure(&m, f));
    // The hash of the function as verified: the form every other vector
    // names it by (the `fn` of every entry in rebase/ and protocol/).
    let three_peers = read(&vectors().join("rebase/three-peers.json"));
    let named: Vec<FnHash> = entries_of(&three_peers).into_iter().map(|(_, e)| e.fn_hash).collect();
    ensure!(named.contains(&fh), "the function hash is not the one the log names");
    ensure_eq!(hex(&fh), v["function_hash"].as_str().unwrap_or(""), "function_hash");
    let bodies = closures(&m);
    let ctx_v = value(&v["ctx"]);
    let ctx = Ctx::new(ctx_v.field("user").as_text(), ctx_v.field("session").as_text());
    let autos = args_of(&value(&v["autos"]));
    let mut st = MemoryStore::from_value(m.schema.clone(), &value(&v["store_before"]));
    let mut by_closure = st.clone();
    for (i, step) in v["steps"].as_array().unwrap().iter().enumerate() {
        let args = args_of(&value(&step["args"]));
        let changes = apply(&m, name, &ctx, &autos, &args, &mut st)
            .map_err(|e| format!("step {i}: bug {e:?}"))?
            .map_err(|r| format!("step {i}: refused {r}"))?;
        let got = Value::List(changes.iter().map(change_value).collect());
        ensure_eq!(got, value(&step["changes"]), "step {i} changes");
        ensure_eq!(st.store_value(), value(&step["store_after"]), "step {i} store");
        ensure_eq!(hex(&state_hash(&st)), step["hash_after"].as_str().unwrap_or(""), "step {i} hash");
        let again = ark::eval::apply_closure(&m.schema, &bodies[&fh], &ctx, &autos, &args, &mut by_closure)
            .map_err(|e| format!("step {i}: closure bug {e:?}"))?
            .map_err(|r| format!("step {i}: closure refused {r}"))?;
        ensure_eq!(again, changes, "step {i} through the closure");
    }
    ensure!(by_closure == st, "the closure map reached another store");
    Ok(())
}

#[test]
fn eval() {
    for p in files("eval") {
        let v = read(&p);
        if v.get("cases").is_some() {
            eval_cases(&p, &v);
        } else {
            check_eval(&v).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        }
    }
    falsified("eval", |_, v| check_eval(v));
}

// eval/ with `cases`: the input checks as verdicts, and the form validator.
fn eval_cases(p: &Path, v: &serde_json::Value) {
    let m = module_of(&v["module"]);
    let bodies = closures(&m);
    let by_name = |n: &str| bodies.values().find(|c| c.function.name == n).unwrap_or_else(|| panic!("{n}"));
    let ctx = match v.get("ctx") {
        Some(c) => {
            let c = value(c);
            Ctx::new(c.field("user").as_text(), c.field("session").as_text())
        }
        None => Ctx::new("alice", "session-1"),
    };
    let store_key = if v.get("store_before").is_some() { "store_before" } else { "store" };
    let st = MemoryStore::from_value(m.schema.clone(), &value(&v[store_key]));
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let c = by_name(case["function"].as_str().unwrap());
        if let Some(refused) = case.get("refused") {
            let autos = args_of(&value(&case["autos"]));
            let a = args_of(&value(&case["args"]));
            let mut s2 = st.clone();
            let got = ark::eval::apply_closure(&m.schema, c, &ctx, &autos, &a, &mut s2).unwrap_or_else(|e| panic!("{name}: bug {e:?}"));
            match (refused.as_str(), got) {
                (None, Ok(_)) => {}
                (Some(want), Err(ark::store::Refusal::Refused(t))) => assert_eq!(t, want, "{}: {name}", p.display()),
                (want, got) => panic!("{}: {name}: wanted {want:?}, got {got:?}", p.display()),
            }
        } else {
            let input = args_of(&value(&case["input"]));
            let got = ark::eval::check(&m.schema, c, &ctx, &input, &st).unwrap_or_else(|e| panic!("{name}: bug {e:?}"));
            let want: Vec<(String, String)> = case["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| (x["field"].as_str().unwrap().to_string(), x["message"].as_str().unwrap().to_string()))
                .collect();
            assert_eq!(got.messages, want, "{}: {name} messages", p.display());
            assert_eq!(
                Value::Struct(got.values),
                value(&case["normalised"]),
                "{}: {name} normalised",
                p.display()
            );
        }
    }
}

// views/ ------------------------------------------------------------------

fn patch_value(p: &Patch) -> Value {
    match p {
        Patch::Insert { at, node } => Value::record(vec![("t", Value::text("insert")), ("at", Value::int(*at as i64)), ("node", node.clone())]),
        Patch::Remove { at } => Value::record(vec![("t", Value::text("remove")), ("at", Value::int(*at as i64))]),
        Patch::Update { at, node } => Value::record(vec![("t", Value::text("update")), ("at", Value::int(*at as i64)), ("node", node.clone())]),
    }
}

/// A query of the vector's module kept up to date: hydrated over
/// `store_before` in the vector's context and arguments, then each batch
/// applied to the store whole and pushed as one; after each, the patches,
/// the answer, the contract and the splice.
fn check_view(v: &serde_json::Value) -> Result<(), String> {
    let m = module_of(&v["module"]);
    let sch = &m.schema;
    let name = v["query"].as_str().unwrap_or("");
    let f = m.lookup_function(name).ok_or(format!("no query {name}"))?;
    let plan = f.plan.clone().ok_or(format!("{name} is not a plan"))?;
    let written = value(&v["module"])
        .field("functions")
        .as_list()
        .into_iter()
        .find(|g| g.field("name") == Value::text(name))
        .map(|g| g.field("plan"));
    ensure_eq!(written, Some(value(&v["plan"])), "the plan is the query's, as the module writes it");
    let ctx_v = value(&v["ctx"]);
    let ctx = Ctx::new(ctx_v.field("user").as_text(), ctx_v.field("session").as_text());
    let c = closure(&m, f);
    let mut st = MemoryStore::from_value(sch.clone(), &value(&v["store_before"]));
    let (args, provided) = ark::eval::middleware(sch, &c, &ctx, &args_of(&value(&v["args"])), &st).map_err(|e| format!("middleware: {e:?}"))?;
    let env = Env {
        helpers: c.helpers.clone(),
        ctx,
        args,
        provided,
    };
    let mut view = hydrate(sch, &plan, env, &st).map_err(|e| format!("hydrate: {e:?}"))?;
    ensure_eq!(Value::List(view.rows()), value(&v["rows_before"]), "the answer at hydrate");
    let batches = value(&v["batches"]).as_list();
    let steps = v["steps"].as_array().ok_or("no steps")?;
    ensure_eq!(batches.len(), steps.len(), "a step per batch");
    for (i, (batch, step)) in batches.iter().zip(steps).enumerate() {
        let batch: Vec<Change> = batch
            .as_list()
            .iter()
            .map(change_from_value)
            .collect::<Result<_, _>>()
            .map_err(|e| format!("batch {i}: {e}"))?;
        let before = view.rows();
        st.apply_changes(&batch);
        let patches = push_all(sch, &st, &batch, &mut view).map_err(|e| format!("batch {i}: {e:?}"))?;
        ensure!(contract(sch, &st, &view), "batch {i} breaks the contract");
        ensure_eq!(
            Value::List(patches.iter().map(patch_value).collect()),
            value(&step["patches"]),
            "batch {i} patches"
        );
        ensure_eq!(Value::List(view.rows()), value(&step["rows"]), "batch {i} rows");
        ensure_eq!(splice(&patches, &before), view.rows(), "batch {i} splice");
    }
    Ok(())
}

/// Falsified by not comparing the patches: `views/falsify/`, which drops
/// the refill after a removal from the window, then passes.
#[test]
fn views() {
    run("views", |_, v| check_view(v));
}

/// The playlist every scripted vector is about.
fn pid() -> Id {
    id_n(1)
}

// rebase/three-peers ---------------------------------------------------------

fn entries_of(v: &serde_json::Value) -> Vec<(Seq, Entry)> {
    value(&v["entries"])
        .as_list()
        .iter()
        .map(|e| {
            let seq = e.field("seq").as_int();
            let mut m = e.as_struct().clone();
            m.remove("seq");
            (seq, entry_from_value(&Value::Struct(m)).unwrap())
        })
        .collect()
}

fn hash_of(bodies: &BTreeMap<FnHash, ark::hash::Closure>, name: &str) -> FnHash {
    bodies
        .iter()
        .find(|(_, c)| c.function.name == name)
        .map(|(h, _)| h.clone())
        .expect("a function by that name")
}

#[test]
fn rebase_three_peers() {
    let v = read(&vectors().join("rebase/three-peers.json"));
    let m = module_of(&v["module"]);
    let sch = m.schema.clone();
    let bodies = closures(&m);
    let entries = entries_of(&v);
    let facts: Vec<Vec<Change>> = value(&v["facts"])
        .as_list()
        .iter()
        .map(|f| f.as_list().iter().map(|c| change_from_value(c).unwrap()).collect())
        .collect();
    let final_hash = v["final_hash"].as_str().unwrap();

    // The transcript through an authority: every entry lands at its
    // sequence with its facts, and the head hashes as claimed.
    let mut auth = Authority::new(sch.clone(), bodies.clone());
    for ((n, e), f) in entries.iter().zip(&facts) {
        match auth.sequence_entry(e) {
            Sequenced::Appended(got, got_facts) => {
                assert_eq!(got, *n);
                assert_eq!(&got_facts, f, "facts at {n}");
            }
            other => panic!("entry {n}: {other:?}"),
        }
    }
    assert_eq!(hex(&state_hash(&auth.store)), final_hash);
    assert_eq!(auth.store.store_value(), value(&v["final_store"]));

    // A whole replica reaches it by replaying intents.
    let mut whole = Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]);
    for (n, e) in entries.iter().rev() {
        whole.receive(*n, e.clone()); // out of order: the inbox holds them
    }
    assert_eq!(whole.cursor, entries.len() as Seq);
    assert_eq!(hex(&whole.verify_at().1), final_hash);
    assert!(whole.diverged.is_empty());

    // A replica with no closures at all reaches it by facts alone.
    let mut facts_only = Replica::open(sch.clone(), BTreeMap::new(), MemoryStore::empty(sch.clone()), 0, vec![]);
    for (n, e) in &entries {
        facts_only.receive(*n, e.clone());
    }
    assert_eq!(facts_only.needs(), (1..=entries.len() as Seq).collect::<Vec<_>>());
    for ((n, _), f) in entries.iter().zip(&facts) {
        facts_only.receive_facts(*n, f.clone());
    }
    assert_eq!(hex(&facts_only.verify_at().1), final_hash);
    assert!(facts_only.diverged.is_empty());

    // The scenario itself, as the generator asserts it step by step.
    let h_create = hash_of(&bodies, "create_playlist");
    let h_add = hash_of(&bodies, "add_to_playlist");
    let ctx = |who: &str| Ctx::new(who, format!("{who}-session"));
    let now: Args = Args::new();
    let add_args = |k: u8| {
        Args::from([
            ("playlist_id".to_string(), Value::Id(pid())),
            ("track_id".to_string(), Value::text(format!("t{k}"))),
        ])
    };
    let pos_of = |r: &Replica, k: u8| {
        r.view
            .get("item", &[Value::Id(pid()), Value::text(format!("t{k}"))])
            .and_then(|row| row.get("pos").cloned())
    };
    let push = |a: &mut Authority, e: &Entry| match a.sequence_entry(e) {
        Sequenced::Appended(n, f) => (n, f),
        other => panic!("push: {other:?}"),
    };

    let mut auth = Authority::new(sch.clone(), bodies.clone());
    let fresh = || Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]);
    let (mut alice, mut bob) = (fresh(), fresh());
    // step 1: alice creates the playlist; everybody sees it
    let e1 = alice
        .mutate(
            id_n(101),
            &ctx("alice"),
            &h_create,
            &Args::from([("id".to_string(), Value::Id(pid()))]),
            &Args::from([("name".to_string(), Value::text(" Favorites "))]),
        )
        .unwrap();
    let (s1, _) = push(&mut auth, &e1);
    alice.ack(&e1.id, s1);
    bob.receive(s1, e1.clone());
    assert_eq!(s1, 1, "the playlist was sequenced first");
    assert_eq!(
        alice.view.get("playlist", &[Value::Id(pid())]).unwrap()["name"],
        Value::text("Favorites"),
        "alice's name was trimmed"
    );
    assert_eq!(alice.verify_at(), bob.verify_at(), "alice and bob agree after step 1");
    // step 2: alice goes dark. bob adds two tracks; alice adds one alone.
    let e2 = bob.mutate(id_n(102), &ctx("bob"), &h_add, &now, &add_args(1)).unwrap();
    let (s2, _) = push(&mut auth, &e2);
    bob.ack(&e2.id, s2);
    let e3 = bob.mutate(id_n(103), &ctx("bob"), &h_add, &now, &add_args(2)).unwrap();
    let (s3, _) = push(&mut auth, &e3);
    bob.ack(&e3.id, s3);
    let _ = alice.take_changes(); // the ack rebuilt her view; a screen has drawn it since
    let e9 = alice.mutate(id_n(109), &ctx("alice"), &h_add, &now, &add_args(9)).unwrap();
    assert_eq!(
        pos_of(&alice, 9),
        Some(value(&v["alice_alone_pos_of_9"])),
        "alone, alice's track is first on her view"
    );
    assert!(
        matches!(alice.take_changes(), Changes::Applied(ref c) if c.len() == 1),
        "a local mutation reports its changes, not a rebuild"
    );
    assert_eq!(pos_of(&bob, 1), Some(Value::int(1)));
    assert_eq!(pos_of(&bob, 2), Some(Value::int(2)));
    // step 3: alice comes back. bob's entries land; her pending replays on top.
    alice.receive(s2, e2.clone());
    alice.receive(s3, e3.clone());
    // Each landing undoes her track, lands bob's, and puts hers back on
    // top: three transitions a view can be told, twice (plan-perf R2).
    assert!(
        matches!(alice.take_changes(), Changes::Applied(ref c) if c.len() == 6),
        "the rebase is reported as its transitions"
    );
    assert_eq!(
        pos_of(&alice, 9),
        Some(value(&v["alice_after_rebase_pos_of_9"])),
        "after the rebase alice's track is third"
    );
    assert_eq!(alice.verify_at(), bob.verify_at(), "alice's confirmed state is bob's");
    // alice pushes what she did alone; it lands after everything that happened while she was away
    let (s9, f9) = push(&mut auth, &e9);
    alice.receive_facts(s9, f9.clone());
    alice.ack(&e9.id, s9);
    bob.receive(s9, e9.clone());
    assert!(alice.pending.is_empty(), "nothing is pending on alice once acked");
    assert!(
        matches!(alice.take_changes(), Changes::Applied(_)),
        "with nothing pending the ack costs no rebuild"
    );
    assert_eq!(alice.view, alice.confirmed, "alice's view is her confirmed store");
    assert!(
        f9.iter().any(|c| matches!(c, Change::Add(_, row) if row["pos"] == Value::int(3))),
        "the authority's facts say pos 3 too"
    );
    assert_eq!(alice.verify_at(), bob.verify_at());
    assert_eq!(hex(&bob.verify_at().1), final_hash, "three replicas, one hash");
    // a duplicate delivery changes nothing
    let bob_before = bob.clone();
    bob.receive(s2, e2.clone());
    assert_eq!(bob, bob_before, "a duplicate delivery is a no-op");
    // dave's build of add_to_playlist is wrong: it steps by two. Facts catch it.
    let mut wrong = bodies.clone();
    let body = &mut wrong.get_mut(&h_add).unwrap().function.body;
    for s in body.iter_mut() {
        if let ark::ir::Stmt::Insert(_, ark::ir::Expr::Struct(fs), _) = s {
            if let Some(ark::ir::Expr::Op(ark::ir::Op::Add, args)) = fs.get_mut("pos") {
                args[1] = ark::ir::Expr::Lit(Value::int(2));
            }
        }
    }
    let mut dave = Replica::open(sch.clone(), wrong, MemoryStore::empty(sch.clone()), 0, vec![]);
    for ((n, e), f) in entries.iter().zip(&facts) {
        dave.receive_with(*n, e.clone(), f.clone());
    }
    assert_eq!(dave.diverged, vec![2, 3, 4], "a divergent runtime is detected");
    assert_eq!(hex(&dave.verify_at().1), final_hash, "and healed by the facts");
    // eve has no server: she is her own authority, and later hands the log over
    let mut eve = fresh();
    let mut eve_auth = Authority::new(sch.clone(), bodies.clone());
    eve.mutate(
        id_n(201),
        &ctx("eve"),
        &h_create,
        &Args::from([("id".to_string(), Value::Id(id_n(2)))]),
        &Args::from([("name".to_string(), Value::text("Road"))]),
    )
    .unwrap();
    eve.mutate(
        id_n(202),
        &ctx("eve"),
        &h_add,
        &now,
        &Args::from([
            ("playlist_id".to_string(), Value::Id(id_n(2))),
            ("track_id".to_string(), Value::text("t5")),
        ]),
    )
    .unwrap();
    local_commit(&mut eve_auth, &mut eve);
    assert!(
        eve.cursor == 2 && eve.pending.is_empty() && eve.view == eve.confirmed,
        "alone, eve confirms her own intents"
    );
    assert_eq!(eve.verify_at().1, state_hash(&eve_auth.store), "and her state is her authority's");
    let adopted = Authority::adopt(sch.clone(), bodies.clone(), &eve_auth.log).expect("a server adopts her log by replaying it");
    assert_eq!(state_hash(&adopted.store), eve.verify_at().1);
    let mut tampered = eve_auth.log.clone();
    for c in &mut tampered.entries.get_mut(&2).unwrap().1 {
        if let Change::Add(_, row) = c {
            row.insert("pos".into(), Value::int(99));
        }
    }
    assert_eq!(
        Authority::adopt(sch.clone(), bodies.clone(), &tampered).err(),
        Some(AdoptError::FactsDiffer(2)),
        "a log whose facts were touched is refused"
    );
    // compaction: the authority moves its horizon to 2
    assert!(auth.compact(2));
    assert!(
        matches!(auth.page(0, 10), ark::log::Page::BelowHorizon(ref sn) if sn.seq == 2),
        "a peer at 0 is sent the snapshot"
    );
    assert!(
        matches!(auth.page(2, 10), ark::log::Page::Entries(ref es, false) if es.iter().map(|(n, _, _)| *n).collect::<Vec<_>>() == vec![3, 4]),
        "a peer at 2 is sent the tail"
    );
    assert_eq!(
        auth.log.state_at(4).map(|st| state_hash(&st)),
        Some(state_hash(&auth.store)),
        "the state at the head, from facts, is the head state"
    );
}

// rebase/fleet-seed-N -------------------------------------------------------------

/// A seeded fleet run from its script: every replica and the server reach
/// the expected head and hash, nothing pending, nothing refused.
fn check_fleet(v: &serde_json::Value) -> Result<(), String> {
    let m = module_of(&v["module"]);
    let sch: Schema = m.schema.clone();
    let bodies = closures(&m);
    let h_create = hash_of(&bodies, "create_playlist");
    let h_add = hash_of(&bodies, "add_to_playlist");
    let id_of = |k: i64| -> Id {
        let mut id = [0u8; 16];
        id[0] = (k / 256) as u8;
        id[1] = (k % 256) as u8;
        id
    };
    let pid = id_of(1);
    let now: Args = Args::new();
    let clients = value(&v["clients"]).as_int();
    let seed = value(&v["seed"]).as_int() as u64;
    let mut sim = Sim::new(sch, bodies, clients, seed);
    sim.mutate(
        0,
        id_of(1000),
        &h_create,
        &Args::from([("id".to_string(), Value::Id(pid))]),
        &Args::from([("name".to_string(), Value::text("Fleet"))]),
    );
    sim.settle();
    let mut n = 0;
    for op in value(&v["script"]).as_list() {
        match op.field("t").as_text() {
            "add" => {
                let peer = op.field("peer").as_int();
                let args = Args::from([("playlist_id".to_string(), Value::Id(pid)), ("track_id".to_string(), op.field("track"))]);
                sim.mutate(peer, id_of(2000 + n), &h_add, &now, &args);
                n += 1;
            }
            "partition" => sim.partition(op.field("peer").as_int()),
            "heal" => sim.heal(op.field("peer").as_int()),
            "step" => sim.step(),
            other => return Err(format!("unknown op {other}")),
        }
    }
    sim.settle();
    let expected_head = value(&v["expected_head"]).as_int();
    let expected_hash = v["expected_hash"].as_str().unwrap_or("");
    let (head, hash) = sim.server_hash();
    ensure_eq!(head, expected_head, "the head");
    ensure_eq!(hex(&hash), expected_hash, "the server's hash");
    for (i, n, h) in sim.client_hashes() {
        ensure_eq!((n, hex(&h)), (expected_head, expected_hash.to_string()), "client {i}");
    }
    ensure!(sim.quiet(), "something is still in flight after settle");
    for c in sim.clients.values() {
        ensure!(c.replica.rejections.is_empty(), "a rejection: {:?}", c.replica.rejections);
        ensure!(c.replica.diverged.is_empty(), "a divergence: {:?}", c.replica.diverged);
    }
    ensure_eq!(sim.server.authority.store.store_value(), value(&v["final_store"]), "the final store");
    Ok(())
}

#[test]
fn rebase_fleet() {
    for p in files("rebase")
        .into_iter()
        .filter(|p| p.file_name().unwrap().to_str().unwrap().starts_with("fleet-"))
    {
        check_fleet(&read(&p)).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    }
    falsified("rebase", |_, v| check_fleet(v));
}
