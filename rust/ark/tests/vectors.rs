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
use ark::protocol::{change_from_value, change_value, entry_from_value, ClientMsg, Received, ServerMsg, Snapshot};
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
        J::String(s) => Value::from(s.clone()),
        J::Array(xs) => Value::List(xs.iter().map(value).collect()),
        J::Object(m) => {
            if m.len() == 1 {
                if let Some(J::String(s)) = m.get("$int") {
                    return Value::Int(s.parse().expect("an int"));
                }
                if let Some(J::String(s)) = m.get("$bytes") {
                    return Value::bytes(decode_hex(s).expect("hex"));
                }
                if let Some(J::String(s)) = m.get("$id") {
                    return Value::Id(id_of_text(s).expect("an id"));
                }
            }
            Value::Struct(Box::new(m.iter().map(|(k, v)| (k.clone(), value(v))).collect()))
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
    ensure_eq!(Value::from(input), value(&v["sorted"]), "sorted");
    Ok(())
}

#[test]
fn order() {
    run("order", |_, v| check_order(v));
}

// hash/ -------------------------------------------------------------------

/// The sum of two 256-bit big-endian integers, modulo 2^256: written here
/// rather than taken from `ark::hash::Digest`, so that the runner checks
/// the construction (§8.1) and not the reference's arithmetic against
/// itself.
fn sum256(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    let mut carry = 0u16;
    for i in (0..32).rev() {
        let s = u16::from(a[i]) + u16::from(b[i]) + carry;
        out[i] = s as u8;
        carry = s >> 8;
    }
    out
}

/// §8.1 A store and its hash: the store's own, kept as it was built; and,
/// where the vector pins the construction part by part (`tables`), every
/// leaf as `sha256(enc(Text table) ‖ enc(row))`, every digest as the sum of
/// its table's leaves, and the hash as that of the `[table, digest]` pairs
/// — each recomputed here from the canonical encoding and SHA-256 alone.
fn check_hash(v: &serde_json::Value) -> Result<(), String> {
    let m = module_of(&v["module"]);
    let st = MemoryStore::from_value(m.schema.clone(), &value(&v["store"]));
    let want = v["hash"].as_str().unwrap_or("");
    let Some(tables) = v["tables"].as_array() else {
        ensure_eq!(hex(&state_hash(&st)), want, "state hash");
        return Ok(());
    };
    let names: Vec<String> = m.schema.tables().map(|t| t.name.clone()).collect();
    ensure_eq!(tables.len(), names.len(), "one entry per table of the schema");
    let mut pairs = Vec::new();
    for (t, name) in tables.iter().zip(&names) {
        ensure_eq!(t["table"].as_str().unwrap_or(""), name.as_str(), "the tables in schema order");
        let mut digest = vec![0u8; 32];
        let rows = t["rows"].as_array().cloned().unwrap_or_default();
        ensure_eq!(rows.len(), st.scan(name).len(), "{name}: every row of the store");
        for r in &rows {
            let mut bytes = encode(&Value::text(name));
            bytes.extend(encode(&value(&r["row"])));
            let leaf = ark::sha256::sha256(&bytes);
            ensure_eq!(hex(&leaf), r["leaf"].as_str().unwrap_or(""), "{name}: a row's leaf");
            digest = sum256(&digest, &leaf);
        }
        ensure_eq!(
            hex(&digest),
            t["digest"].as_str().unwrap_or(""),
            "{name}: the digest is the sum of the leaves"
        );
        pairs.push(Value::list(vec![Value::text(name), Value::bytes(digest)]));
    }
    ensure_eq!(hex(&ark::sha256::sha256(&encode(&Value::from(pairs)))), want, "the hash of the pairs");
    ensure_eq!(hex(&state_hash(&st)), want, "state hash");
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
    // `docs/plan-guards.md` D3 A server's module with the module a client
    // loads beside it: the second is the first with every private block
    // stripped, at its own hash, and the two name every function alike.
    if !v["stripped"].is_null() {
        let sval = value(&v["stripped"]);
        ensure_eq!(
            decode(&bytes_of(&v["strippedBytes"])).map_err(|e| e.to_string())?,
            sval,
            "the stripped bytes decode to the stripped module"
        );
        let stripped = module_from_value(&sval).map_err(|e| e.to_string())?;
        let want = ark::verify::verify(&ark::ir::strip_module(&m)).map_err(|es| format!("{es:?}"))?;
        ensure_eq!(
            module_value(&want),
            sval,
            "the stripped module is the module with its private blocks taken out"
        );
        ensure_eq!(hex(&module_hash(&stripped)), v["public"].as_str().unwrap_or(""), "the public module hash");
        let names = |m: &Module| -> Vec<(String, String)> {
            let mut out: Vec<(String, String)> = closures(m).into_iter().map(|(h, c)| (c.function.name, hex(&h))).collect();
            out.sort();
            out
        };
        let listed: Vec<(String, String)> = v["functions"]
            .as_array()
            .map(|xs| {
                xs.iter()
                    .map(|p| (p[0].as_str().unwrap_or("").to_string(), p[1].as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        ensure_eq!(names(&m), listed, "the server's module names its functions by the listed hashes");
        ensure_eq!(names(&stripped), listed, "and so does the client's");
    }
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
        // As a client reads the socket (`docs/plan-db.md` D7.4): the same
        // frame, a snapshot's rows built as they are decoded — under the
        // demo's schema, whose tables these frames' rows are, and under
        // none, where every row is a row of no table.
        let demo = module_of(&read(&vectors().join("rebase/three-peers.json"))["module"]).schema;
        for sch in [demo, Schema::empty()] {
            let want = match f.clone() {
                ServerMsg::SnapshotOf {
                    seq,
                    rows,
                    log_id,
                    module,
                    held,
                    ..
                } => Received::Snapshot(Snapshot::of_values(&sch, seq, rows, log_id, module, held)),
                other => Received::Msg(other),
            };
            let got = ServerMsg::decode_for(&bytes, &sch)?;
            match (&got, &want) {
                // Closures decode with empty symbol names, so compare through their values.
                (Received::Msg(g), Received::Msg(w)) => ensure_eq!(g.to_value(), w.to_value(), "read as a client reads it"),
                _ => ensure_eq!(got, want, "read as a client reads it"),
            }
        }
    }
    Ok(())
}

#[test]
fn protocol() {
    run("protocol", |p, v| check_protocol(p.file_name().unwrap().to_str().unwrap(), v));
}

// eval/ -------------------------------------------------------------------

/// A vector's `ctx`: the user, the login, and the roles held where it says
/// any (`docs/plan-guards.md` D1) — none where `roles` is absent, as in
/// every vector written before roles were read.
fn ctx_of(c: &Value) -> Ctx {
    let roles: Vec<String> = match c.as_struct().get("roles") {
        Some(rs) => rs.as_list().iter().map(|r| r.as_text().to_string()).collect(),
        None => vec![],
    };
    Ctx::new(c.field("user").as_text(), c.field("session").as_text()).with_roles(roles)
}

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
    let ctx = ctx_of(&value(&v["ctx"]));
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

/// `docs/plan-guards.md` D4 A raw write applied step by step: named by the
/// hash of its name, each step's changes — or the constraint that refuses
/// it, with the store unmoved — and the store and the hash after.
fn check_raw(v: &serde_json::Value) -> Result<(), String> {
    let m = module_of(&v["module"]);
    let name = v["raw"].as_str().unwrap_or("");
    let fh = ark::raw::hash_of(name);
    ensure_eq!(hex(&fh), v["function_hash"].as_str().unwrap_or(""), "function_hash");
    let raw = ark::raw::of(&fh).ok_or_else(|| format!("{name} is no raw write"))?;
    let mut st = MemoryStore::from_value(m.schema.clone(), &value(&v["store_before"]));
    for (i, step) in v["steps"].as_array().unwrap().iter().enumerate() {
        let args = args_of(&value(&step["args"]));
        let before = st.clone();
        let got = ark::raw::apply(raw, &args, &mut st).map_err(|e| format!("step {i}: bug {e:?}"))?;
        match (got, step.get("refused")) {
            (Ok(chs), None) => ensure_eq!(
                Value::List(chs.iter().map(change_value).collect()),
                value(&step["changes"]),
                "step {i} changes"
            ),
            (Err(r), Some(want)) => {
                ensure_eq!(r.to_string(), want.as_str().unwrap_or(""), "step {i} refusal");
                ensure!(st == before, "step {i}: a refused write moved the store");
            }
            (got, want) => return Err(format!("step {i}: got {got:?}, wanted {want:?}")),
        }
        ensure_eq!(st.store_value(), value(&step["store_after"]), "step {i} store");
        ensure_eq!(hex(&state_hash(&st)), step["hash_after"].as_str().unwrap_or(""), "step {i} hash");
    }
    Ok(())
}

#[test]
fn eval() {
    for p in files("eval") {
        let v = read(&p);
        if v.get("cases").is_some() {
            check_cases(&v).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        } else if v.get("raw").is_some() {
            check_raw(&v).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        } else if v.get("live").is_some() {
            check_live(&v).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        } else {
            check_eval(&v).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        }
    }
    falsified("eval", |_, v| {
        if v.get("live").is_some() {
            check_live(v)
        } else if v.get("cases").is_some() {
            check_cases(v)
        } else {
            check_eval(v)
        }
    });
}

/// `eval/live-*` (`docs/plan-live.md`): a live state's closure and type
/// hashes, every transition's kind, its init and the hash of it, and a life
/// of it — each step run over the state the last left, as the engine runs it
/// (`ark::live::state::Runner::run`: a leave empties the leaver's ephemeral
/// slots first), its new state and that state's hash with the ephemeral
/// fields left out, or the refusal's words.
fn check_live(v: &serde_json::Value) -> Result<(), String> {
    use ark::live::state::Runner;
    let m = module_of(&v["module"]);
    let name = v["live"].as_str().ok_or("live names the state")?;
    let st = m.lookup_state(name).ok_or("the module has the state")?;
    let c = ark::hash::live_closure(&m, st);
    ensure_eq!(hex(&ark::hash::live_hash(&c)), v["live_hash"].as_str().unwrap_or(""), "the live hash");
    ensure_eq!(
        hex(&ark::hash::state_type_hash(&c)),
        v["type_hash"].as_str().unwrap_or(""),
        "the type hash"
    );
    let kinds = value(&v["kinds"]);
    for (t, k) in kinds.as_struct() {
        ensure_eq!(Value::text(c.kind(t).name()), k.clone(), "the kind of {t}");
    }
    let r = Runner::of_module(&m);
    let mut s = r.init(name).map_err(|e| format!("{e:?}"))?;
    ensure_eq!(s, value(&v["init"]), "the init");
    ensure_eq!(hex(&r.hash(name, &s)), v["init_hash"].as_str().unwrap_or(""), "the init's hash");
    for (k, step) in v["steps"].as_array().ok_or("steps")?.iter().enumerate() {
        let t = step["transition"].as_str().ok_or("a transition")?;
        let cv = value(&step["ctx"]);
        let ctx = ark::eval::LiveCtx {
            user: cv.field("user").as_text().to_string(),
            session: cv.field("session").as_text().to_string(),
            roles: cv.field("roles").as_list().iter().map(|r| r.as_text().to_string()).collect(),
            device: cv.field("device").as_text().to_string(),
            now: match cv.field("now") {
                Value::Int(n) => n,
                other => return Err(format!("step {k}: now {other:?}")),
            },
        };
        let input = args_of(&value(&step["input"]));
        match (r.run(name, t, &s, &ctx, &input), step.get("refused")) {
            (Ok(next), None) => {
                ensure_eq!(next, value(&step["state"]), "step {k} ({t}): the state");
                ensure_eq!(hex(&r.hash(name, &next)), step["hash"].as_str().unwrap_or(""), "step {k} ({t}): its hash");
                s = next;
            }
            (Err(ark::eval::EvalFault::Verdict(ark::store::Refusal::Refused(why))), Some(want)) => {
                ensure_eq!(Some(why.as_str()), want.as_str(), "step {k} ({t}): the refusal");
            }
            (got, want) => return Err(format!("step {k} ({t}): {got:?}, the vector says {want:?}")),
        }
    }
    Ok(())
}

// eval/ with `cases`: the input checks as verdicts, and the form validator.
/// `form: cases`: one store, each case over a fresh copy of it — a mutator
/// that must apply (its changes compared where given) or be refused by
/// exactly a text; or the form validator's messages and normalised values.
/// A result rather than a panic, so that a `falsify/` vector of this form
/// (`eval/falsify/list-ops-clamping.json`) is held to failing it.
fn check_cases(v: &serde_json::Value) -> Result<(), String> {
    let m = module_of(&v["module"]);
    let bodies = closures(&m);
    let by_name = |n: &str| bodies.values().find(|c| c.function.name == n).ok_or(format!("no function {n}"));
    let ctx = match v.get("ctx") {
        Some(c) => ctx_of(&value(c)),
        None => Ctx::new("alice", "session-1"),
    };
    let store_key = if v.get("store_before").is_some() { "store_before" } else { "store" };
    let st = MemoryStore::from_value(m.schema.clone(), &value(&v[store_key]));
    for case in v["cases"].as_array().ok_or("cases is a list")? {
        let name = case["name"].as_str().ok_or("a case has a name")?;
        let c = by_name(case["function"].as_str().ok_or("a case names its function")?)?;
        // A case may be authored by its own `ctx`, with `roles`
        // (`eval/has-role.json`, `docs/plan-guards.md` D1).
        let ctx = case.get("ctx").map_or_else(|| ctx.clone(), |c| ctx_of(&value(c)));
        if let Some(refused) = case.get("refused") {
            let autos = args_of(&value(&case["autos"]));
            let a = args_of(&value(&case["args"]));
            let mut s2 = st.clone();
            let got = ark::eval::apply_closure(&m.schema, c, &ctx, &autos, &a, &mut s2).map_err(|e| format!("{name}: bug {e:?}"))?;
            match (refused.as_str(), got) {
                (None, Ok(chs)) => {
                    if let Some(want) = case.get("changes") {
                        let got = Value::List(chs.iter().map(change_value).collect());
                        ensure_eq!(got, value(want), "{name} changes");
                    }
                }
                (Some(want), Err(ark::store::Refusal::Refused(t))) => ensure_eq!(t.as_str(), want, "{name}"),
                (want, got) => return Err(format!("{name}: wanted {want:?}, got {got:?}")),
            }
        } else {
            let input = args_of(&value(&case["input"]));
            let got = ark::eval::check(&m.schema, c, &ctx, &input, &st).map_err(|e| format!("{name}: bug {e:?}"))?;
            let want: Vec<(String, String)> = case["messages"]
                .as_array()
                .ok_or("messages is a list")?
                .iter()
                .map(|x| {
                    (
                        x["field"].as_str().unwrap_or("").to_string(),
                        x["message"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect();
            ensure_eq!(got.messages, want, "{name} messages");
            ensure_eq!(Value::from(got.values), value(&case["normalised"]), "{name} normalised");
        }
    }
    Ok(())
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
    let ctx = ctx_of(&value(&v["ctx"]));
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
    ensure_eq!(Value::from(view.rows()), value(&v["rows_before"]), "the answer at hydrate");
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
        // A step the fuzzer wrote has no patches: what a correct engine
        // patches is not what the broken one did, and the splice below
        // holds the patches to the answer either way.
        if step.get("patches").is_some() {
            ensure_eq!(
                Value::List(patches.iter().map(patch_value).collect()),
                value(&step["patches"]),
                "batch {i} patches"
            );
        }
        ensure_eq!(Value::from(view.rows()), value(&step["rows"]), "batch {i} rows");
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
            (seq, entry_from_value(&Value::from(m)).unwrap())
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
    whole.settle();
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
    facts_only.settle();
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
    alice.settle();
    bob.receive(s1, e1.clone());
    bob.settle();
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
    bob.settle();
    let e3 = bob.mutate(id_n(103), &ctx("bob"), &h_add, &now, &add_args(2)).unwrap();
    let (s3, _) = push(&mut auth, &e3);
    bob.ack(&e3.id, s3);
    bob.settle();
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
    // Two pumps, one entry each: the six transitions below are two
    // rebases, and a settle per landing is what makes them two (R8 of
    // `docs/plan-perf.md` — one settle over both would be one rebase, four).
    alice.receive(s2, e2.clone());
    alice.settle();
    alice.receive(s3, e3.clone());
    alice.settle();
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
    alice.settle();
    bob.receive(s9, e9.clone());
    bob.settle();
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
    bob.settle();
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
    dave.settle();
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
    // A session the fuzzer wrote (`arkc fuzz`, `docs/plan-db.md` D2): its
    // own script of ops, held to convergence and replay rather than to a
    // hash, since the hash a fixed engine reaches is not the broken one's.
    if v.get("fuzz").is_some() {
        let m = module_of(&v["module"]);
        let script = value(&v["script"]).as_list();
        let clients = value(&v["clients"]).as_int();
        let seed = value(&v["seed"]).as_int() as u64;
        return ark::sim::run_script(m.schema.clone(), closures(&m), clients, seed, &script).map(|_| ());
    }
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

// MANIFEST.json ------------------------------------------------------------------

/// `vectors/MANIFEST.json` names every vector there is, once, by the
/// directory it is in, failing exactly where it is a `falsify/` one, and
/// says who runs it: every `rebase/` fleet `rust-only` — Rust's simulator
/// and its scheduler — and everything else `client` (`spec/RUNNERS.md`,
/// `docs/plan-phones.md` S1). A file the manifest does not name, or one it
/// names that is not there, is a manifest out of date. Falsified by leaving
/// `trace/` out of the files the manifest is written from: every trace is
/// then a file it does not name.
#[test]
fn the_manifest_names_every_vector() {
    let m = read(&vectors().join("MANIFEST.json"));
    let listed = m["vectors"].as_array().expect("a list of vectors");
    let mut named = std::collections::BTreeSet::new();
    let mut sides: BTreeMap<String, usize> = BTreeMap::new();
    for e in listed {
        let path = e["path"].as_str().expect("a path");
        assert!(named.insert(path.to_string()), "{path} is listed twice");
        assert!(vectors().join(path).is_file(), "{path} is listed and is not there");
        assert_eq!(e["kind"].as_str(), path.split('/').next(), "{path}: the kind is its directory");
        let fails = path.contains("/falsify/");
        assert_eq!(e["expect"].as_str(), Some(if fails { "fail" } else { "pass" }), "{path}");
        let fleet = path.starts_with("rebase/") && read(&vectors().join(path)).get("script").is_some();
        // `docs/plan-live.md` A live vector is every runtime's since phase
        // 1b, so nothing is Rust's but the fleets.
        let side = if fleet { "rust-only" } else { "client" };
        assert_eq!(e["side"].as_str(), Some(side), "{path}");
        *sides.entry(e["side"].as_str().unwrap_or("").to_string()).or_default() += 1;
    }
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().is_some_and(|x| x == "json") {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut on_disk = vec![];
    walk(&vectors(), &vectors(), &mut on_disk);
    on_disk.retain(|p| p != "MANIFEST.json");
    for p in &on_disk {
        assert!(named.contains(p), "{p} is a vector the manifest does not name");
    }
    assert_eq!(on_disk.len(), named.len());
    println!("MANIFEST.json: {} vectors, by side {sides:?}", named.len());
}
