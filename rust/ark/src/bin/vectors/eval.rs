//! `verify/` (§9), `eval/` (§6, §8) and `hash/` (§8.1): the demo module
//! verifies and two one-line edits of it do not; `add_to_playlist` applied
//! four times, with the changes, the store and the hash after each; the
//! input checks as verdicts, and the form validator; and the state hash's
//! construction pinned part by part — every row's leaf, every table's
//! digest, the pairs they make (`docs/plan-db.md` D3); and a guard and a
//! body asking `has_role`, applied as authors holding different roles
//! (`docs/plan-guards.md` D1).

use ark::eval::{apply, check, Args, Ctx};
use ark::hash::{closure, function_hash, leaf, state_hash, state_hash_of, table_digest, Digest};
use ark::ir::{module_value, Module, Stmt};
use ark::protocol::change_value;
use ark::store::{Change, MemoryStore, Refusal, Store};
use ark::value::{hex, Value};
use ark::verify::{verify, Complaint, VerifyError};

use super::demo::{self, id_n};
use super::json::{array, json, obj, quoted};
use super::Out;

fn ctx_value() -> Value {
    Value::record(vec![("user", Value::text("alice")), ("session", Value::text("session-1"))])
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// The demo with one function edited.
fn editing(m: &Module, name: &str, f: impl Fn(&mut ark::ir::Function)) -> Module {
    let mut m = m.clone();
    for fun in m.functions.iter_mut().filter(|fun| fun.name == name) {
        f(fun);
    }
    m
}

/// A module the verifier must refuse, with a complaint `want` recognises.
fn refused(what: &str, m: &Module, want: impl Fn(&VerifyError) -> bool) {
    match verify(m) {
        Err(es) if es.iter().any(&want) => {}
        other => panic!("verify: {what} was not refused as it should be: {other:?}"),
    }
}

pub fn demo(out: &Out) {
    out.dir("verify/ eval/ hash/");
    let m = demo::module();
    out.write(
        "verify/demo-ok.json",
        &obj(&[("module", json(&module_value(&m))), ("verifies", "true".into())]),
    );
    // Two modules that must not verify, each a one-line edit of the demo:
    // an insert matched on columns that are not a declared unique index,
    // and a procedure running middleware its router does not declare.
    let on_name = editing(&m, "create_playlist", |f| {
        for st in f.body.iter_mut() {
            if let Stmt::Insert(_, _, on) = st {
                *on = vec!["name".into()];
            }
        }
    });
    refused("an insert on columns that are no unique index", &on_name, |e| {
        matches!(e, VerifyError::In(_, Complaint::OnNotUnique(..)))
    });
    let nope = editing(&m, "items", |f| f.uses = vec!["nope".into()]);
    refused("a procedure running middleware its router lacks", &nope, |e| {
        matches!(e, VerifyError::In(_, Complaint::UsesNotOnRouter(_)))
    });
    // The first of the two, claimed to verify.
    out.write(
        "verify/falsify/insert-on-no-unique-index.json",
        &obj(&[
            ("module", json(&module_value(&on_name))),
            ("verifies", "true".into()),
            ("expect", quoted("fail")),
        ]),
    );

    let pid = id_n(1);
    let mut st0 = MemoryStore::empty(m.schema.clone());
    let playlist = [
        ("id", Value::Id(pid)),
        ("name", Value::text("Favorites")),
        ("user_id", Value::text("alice")),
    ];
    st0.put("playlist", playlist.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
        .expect("the playlist row");
    let ctx = Ctx::new("alice", "session-1");
    let autos = Args::new();
    let add_args = |k: u8| args([("playlist_id", Value::Id(pid)), ("track_id", Value::text(format!("t{k}")))]);
    let step = |st: &MemoryStore, k: u8| {
        let mut st = st.clone();
        match apply(&m, "add_to_playlist", &ctx, &autos, &add_args(k), &mut st) {
            Ok(Ok(chs)) => (st, chs),
            other => panic!("apply: {other:?}"),
        }
    };
    // The function as verified: orders completed. Hashing the authored
    // form gave a hash no entry ever names, which two runtimes caught.
    let add = m.lookup_function("add_to_playlist").expect("add_to_playlist");
    let add_hash = function_hash(&closure(&m, add));
    let (st1, ch1) = step(&st0, 7);
    let (st2, ch2) = step(&st1, 9);
    let (st3, ch3) = step(&st2, 7); // already there: a no-op
    let (st4, ch4) = step(&st3, 11); // lands third: which is only true reading pos DESCENDING
    let step_json = |k: u8, chs: &[Change], st: &MemoryStore| {
        obj(&[
            ("args", json(&Value::from(add_args(k)))),
            ("changes", json(&Value::List(chs.iter().map(change_value).collect()))),
            ("store_after", json(&st.store_value())),
            ("hash_after", quoted(&hex(&state_hash(st)))),
        ])
    };
    out.write(
        "eval/add-to-playlist.json",
        &obj(&[
            ("module", json(&module_value(&m))),
            ("function", quoted("add_to_playlist")),
            ("function_hash", quoted(&hex(&add_hash))),
            ("store_before", json(&st0.store_value())),
            ("ctx", json(&ctx_value())),
            ("autos", json(&Value::from(autos.clone()))),
            (
                "steps",
                array([
                    step_json(7, &ch1, &st1),
                    step_json(9, &ch2, &st2),
                    step_json(7, &ch3, &st3),
                    step_json(11, &ch4, &st4),
                ]),
            ),
        ]),
    );
    // The four steps again, the last as a runtime would take it that read
    // the first item in ascending order: pos 2 where the spec says 3.
    let ascending: Vec<Change> = ch4
        .iter()
        .map(|c| match c {
            Change::Add(t, row) => {
                let mut row = row.clone();
                row.insert("pos".into(), Value::Int(2));
                Change::Add(t.clone(), row)
            }
            other => other.clone(),
        })
        .collect();
    let mut st4_ascending = st3.clone();
    st4_ascending.apply_changes(&ascending);
    out.write(
        "eval/falsify/add-reads-pos-ascending.json",
        &obj(&[
            ("module", json(&module_value(&m))),
            ("function", quoted("add_to_playlist")),
            ("function_hash", quoted(&hex(&add_hash))),
            ("store_before", json(&st0.store_value())),
            ("ctx", json(&ctx_value())),
            ("autos", json(&Value::from(autos.clone()))),
            (
                "steps",
                array([
                    step_json(7, &ch1, &st1),
                    step_json(9, &ch2, &st2),
                    step_json(7, &ch3, &st3),
                    step_json(11, &ascending, &st4_ascending),
                ]),
            ),
            ("expect", quoted("fail")),
        ]),
    );
    // The store after four steps with the hash of the store before the
    // last: a hash that did not look at every row would claim it.
    out.write(
        "hash/falsify/hash-of-the-store-before.json",
        &obj(&[
            ("module", json(&module_value(&m))),
            ("store", json(&st4.store_value())),
            ("hash", quoted(&hex(&state_hash(&st3)))),
            ("expect", quoted("fail")),
        ]),
    );
    // A state hash depends on the schema's table order, so the vector
    // carries the module the store belongs to.
    out.write(
        "hash/demo-state.json",
        &obj(&[
            ("module", json(&module_value(&m))),
            ("store", json(&st4.store_value())),
            ("hash", quoted(&hex(&state_hash(&st4)))),
        ]),
    );
    leaves_and_digests(out, &m, &st2);
    println!("  function hash {}", hex(&add_hash));
    println!("  state hash    {}", hex(&state_hash(&st4)));
    println!("  changes       ({},{},{},{})", ch1.len(), ch2.len(), ch3.len(), ch4.len());

    // The checks, the middleware order and the form validator, as verdicts.
    let cid = id_n(2);
    let verdict_of = |name: &str, autos: &Args, a: &Args| {
        let mut st = st4.clone();
        match apply(&m, name, &ctx, autos, a, &mut st) {
            Ok(Err(Refusal::Refused(t))) => Some(t),
            Ok(Err(other)) => Some(other.to_string()),
            Ok(Ok(_)) => None,
            Err(e) => panic!("apply {name}: {e:?}"),
        }
    };
    let create_autos = args([("id", Value::Id(cid))]);
    let cases: Vec<(&str, &str, Args, Args, Option<&str>)> = vec![
        (
            "trim-then-min",
            "create_playlist",
            create_autos.clone(),
            args([("name", Value::text("   "))]),
            Some("a playlist needs a name"),
        ),
        (
            "trimmed-name-lands",
            "create_playlist",
            create_autos.clone(),
            args([("name", Value::text("  Road  "))]),
            None,
        ),
        (
            "same-name-again-is-a-no-op",
            "create_playlist",
            create_autos.clone(),
            args([("name", Value::text("Favorites"))]),
            None,
        ),
        (
            "exists-check-default-message",
            "add_to_playlist",
            Args::new(),
            args([("playlist_id", Value::Id(cid)), ("track_id", Value::text("t1"))]),
            Some("playlist_id: no such playlist"),
        ),
        (
            "min-len-default-message",
            "add_to_playlist",
            Args::new(),
            args([("playlist_id", Value::Id(pid)), ("track_id", Value::text(""))]),
            Some("track_id: at least 1 characters"),
        ),
    ];
    for (what, name, a, i, want) in &cases {
        let got = verdict_of(name, a, i);
        assert_eq!(got.as_deref(), *want, "eval {what}");
    }
    let mut st = st4.clone();
    match apply(
        &m,
        "create_playlist",
        &ctx,
        &create_autos,
        &args([("name", Value::text("  Road  "))]),
        &mut st,
    ) {
        Ok(Ok(chs))
            if matches!(chs.as_slice(), [Change::Add(t, row)] if t == "playlist" && row.get("name") == Some(&Value::text("Road")))
                && st.rows("playlist").len() == 2 => {}
        other => panic!("eval trimmed-name-lands: {other:?}"),
    }
    let mut st = st4.clone();
    match apply(
        &m,
        "create_playlist",
        &ctx,
        &create_autos,
        &args([("name", Value::text("Favorites"))]),
        &mut st,
    ) {
        Ok(Ok(chs)) if chs.is_empty() => {}
        other => panic!("eval same-name-again: {other:?}"),
    }
    out.write(
        "eval/checks.json",
        &obj(&[
            ("module", json(&module_value(&m))),
            ("store_before", json(&st4.store_value())),
            ("ctx", json(&ctx_value())),
            (
                "cases",
                array(cases.iter().map(|(what, name, a, i, want)| {
                    obj(&[
                        ("name", quoted(what)),
                        ("function", quoted(name)),
                        ("autos", json(&Value::from(a.clone()))),
                        ("args", json(&Value::from(i.clone()))),
                        ("refused", want.map(quoted).unwrap_or_else(|| "null".into())),
                    ])
                })),
            ),
        ]),
    );

    // The form validator: every field's first failure, and the input as normalised.
    let form = |name: &str, input: &Args| {
        let f = m.lookup_function(name).unwrap_or_else(|| panic!("{name}"));
        check(&m.schema, &closure(&m, f), &ctx, input, &st4).unwrap_or_else(|e| panic!("check: {e:?}"))
    };
    let msgs = |xs: &[(&str, &str)]| xs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    assert_eq!(
        form("create_playlist", &args([("name", Value::text(" "))])).messages,
        msgs(&[("name", "a playlist needs a name")]),
        "check: empty name"
    );
    assert_eq!(
        form("create_playlist", &args([("name", Value::text(" Mix "))])).values,
        args([("name", Value::text("Mix"))]),
        "check: trim"
    );
    assert_eq!(
        form("add_to_playlist", &args([("track_id", Value::text(""))])).messages,
        msgs(&[("track_id", "track_id: at least 1 characters")]),
        "check: partial"
    );
    let form_cases: Vec<(&str, &str, Args)> = vec![
        ("empty-name", "create_playlist", args([("name", Value::text(" "))])),
        ("trimmed", "create_playlist", args([("name", Value::text(" Mix "))])),
        ("partial-input", "add_to_playlist", args([("track_id", Value::text(""))])),
        (
            "unknown-playlist",
            "add_to_playlist",
            args([("playlist_id", Value::Id(cid)), ("track_id", Value::text("t"))]),
        ),
    ];
    out.write(
        "eval/form-check.json",
        &obj(&[
            ("module", json(&module_value(&m))),
            ("store", json(&st4.store_value())),
            (
                "cases",
                array(form_cases.iter().map(|(what, name, input)| {
                    let got = form(name, input);
                    let messages = got
                        .messages
                        .iter()
                        .map(|(f, w)| Value::record(vec![("field", Value::text(f)), ("message", Value::text(w))]))
                        .collect();
                    obj(&[
                        ("name", quoted(what)),
                        ("function", quoted(name)),
                        ("input", json(&Value::from(input.clone()))),
                        ("messages", json(&Value::List(messages))),
                        ("normalised", json(&Value::from(got.values))),
                    ])
                })),
            ),
        ]),
    );
}

/// `eval/has-role.json` (`docs/plan-guards.md` D1): the guarded demo's
/// `feature`, applied as authors holding different roles. Its guard
/// `is_curator` refuses whoever does not hold `curator`, and its body reads
/// `editor` to decide whose the playlist is — so the roles decide both the
/// verdict and the row. Each case carries its own `ctx`, with `roles`, and
/// an accepted one its changes. Before writing, the verifier is held to
/// refusing `has_role` of the empty name.
pub fn has_role(out: &Out) {
    let m = demo::guarded();
    super::claim(
        "is_curator asks has_role",
        format!("{:?}", m.lookup_function("is_curator").map(|f| &f.body)).contains("HasRole(\"curator\")"),
    );
    fn blank(v: &mut Value) {
        match v {
            Value::Struct(fs) => {
                if fs.get("t") == Some(&Value::text("has_role")) {
                    fs.insert("role".into(), Value::text(""));
                }
                fs.values_mut().for_each(blank);
            }
            Value::List(xs) => xs.iter_mut().for_each(blank),
            _ => {}
        }
    }
    let mut v = module_value(&m);
    blank(&mut v);
    let empty = ark::ir::module_from_value(&v).expect("the module with an empty role decodes");
    refused("has_role of the empty name", &empty, |e| {
        matches!(e, VerifyError::In(_, Complaint::EmptyRole))
    });

    let st = MemoryStore::empty(m.schema.clone());
    let autos = args([("id", Value::Id(id_n(7)))]);
    let input = args([("name", Value::text("Picks"))]);
    let cases: Vec<(&str, Vec<&str>, Option<&str>)> = vec![
        ("held", vec!["curator"], None),
        ("held-and-an-editor", vec!["curator", "editor"], None),
        ("not-held", vec![], Some("only a curator features a playlist")),
        ("another-role-only", vec!["editor"], Some("only a curator features a playlist")),
    ];
    let ctx_of = |roles: &[&str]| Ctx::new("alice", "session-1").with_roles(roles.iter().copied());
    let ctx_json = |roles: &[&str]| {
        json(&Value::record(vec![
            ("user", Value::text("alice")),
            ("session", Value::text("session-1")),
            ("roles", Value::List(roles.iter().map(|r| Value::text(*r)).collect())),
        ]))
    };
    let mut written = vec![];
    for (what, roles, want) in &cases {
        let mut s2 = st.clone();
        let got = apply(&m, "feature", &ctx_of(roles), &autos, &input, &mut s2).unwrap_or_else(|e| panic!("feature: {e:?}"));
        let changes = match (got, want) {
            (Ok(chs), None) => {
                let owner = match chs.as_slice() {
                    [Change::Add(t, row)] if t == "playlist" => row.get("user_id").cloned(),
                    _ => None,
                };
                let whose = if roles.contains(&"editor") { "editors" } else { "curators" };
                super::claim(
                    &format!("has-role {what}: the playlist is the {whose}'"),
                    owner == Some(Value::text(whose)),
                );
                Some(chs)
            }
            (Err(Refusal::Refused(t)), Some(w)) if t == *w => None,
            other => panic!("has-role {what}: {other:?}"),
        };
        let mut parts = vec![
            ("name", quoted(what)),
            ("function", quoted("feature")),
            ("ctx", ctx_json(roles)),
            ("autos", json(&Value::from(autos.clone()))),
            ("args", json(&Value::from(input.clone()))),
            ("refused", want.map(quoted).unwrap_or_else(|| "null".into())),
        ];
        if let Some(chs) = changes {
            parts.push(("changes", json(&Value::List(chs.iter().map(change_value).collect()))));
        }
        written.push(obj(&parts));
    }
    out.write(
        "eval/has-role.json",
        &obj(&[
            ("module", json(&module_value(&m))),
            ("store_before", json(&st.store_value())),
            ("cases", array(written)),
        ]),
    );
}

/// `hash/leaves-and-digests.json` (§8.1, `docs/plan-db.md` D3): a store of
/// one playlist and two of its items, with every row's leaf, every table's
/// digest and the state hash of the pairs — so that a runtime is held to
/// each step of the construction rather than only to where it ends. A
/// one-row table's digest is its leaf; a two-row table's is the sum of the
/// two, which is not their exclusive or: the falsify case claims the xor,
/// and a runner that sums must refuse it.
fn leaves_and_digests(out: &Out, m: &Module, st: &MemoryStore) {
    let tables: Vec<(String, Vec<ark::store::Row>)> = m.schema.tables().map(|t| (t.name.clone(), st.scan(&t.name))).collect();
    let counts: Vec<usize> = tables.iter().map(|(_, rs)| rs.len()).collect();
    super::claim("one playlist and two items", counts == [1, 2]);
    let (pl, items) = (&tables[0], &tables[1]);
    super::claim(
        "a one-row table's digest is its leaf",
        table_digest(&pl.0, &pl.1) == Digest(leaf(&pl.0, &pl.1[0])),
    );
    let (a, b) = (leaf(&items.0, &items.1[0]), leaf(&items.0, &items.1[1]));
    let mut sum = Digest(a);
    sum.add(&b);
    super::claim(
        "a two-row table's digest is the sum of its leaves",
        table_digest(&items.0, &items.1) == sum,
    );
    super::claim("the store keeps what it would sum", st.digest(&items.0) == Some(sum));
    let xor = Digest(std::array::from_fn(|i| a[i] ^ b[i]));
    super::claim("the sum is not the exclusive or", xor != sum);
    let digests: Vec<(&str, Digest)> = tables.iter().map(|(t, rs)| (t.as_str(), table_digest(t, rs))).collect();
    super::claim("the state hash is of the pairs", state_hash_of(digests.clone()) == state_hash(st));
    let write = |name: &str, digests: &[(&str, Digest)], expect_fail: bool| {
        let entry = |(t, rs): &(String, Vec<ark::store::Row>), d: Digest| {
            obj(&[
                ("table", quoted(t)),
                (
                    "rows",
                    array(
                        rs.iter()
                            .map(|r| obj(&[("row", json(&r.to_value())), ("leaf", quoted(&hex(&leaf(t, r))))])),
                    ),
                ),
                ("digest", quoted(&hex(&d.0))),
            ])
        };
        let mut parts = vec![
            ("module", json(&module_value(m))),
            ("store", json(&st.store_value())),
            ("tables", array(tables.iter().zip(digests).map(|(t, (_, d))| entry(t, *d)))),
            ("hash", quoted(&hex(&state_hash_of(digests.iter().copied())))),
        ];
        if expect_fail {
            parts.push(("expect", quoted("fail")));
        }
        out.write(name, &obj(&parts));
    };
    write("hash/leaves-and-digests.json", &digests, false);
    let mut xored = digests.clone();
    xored[1].1 = xor;
    write("hash/falsify/digest-by-xor.json", &xored, true);
}

/// `docs/plan-guards.md` D4 The authority's raw writes, applied step by
/// step over the demo's tables: what each changes, or the constraint that
/// refuses it — exactly `put`'s and `delete`'s, since that is what they
/// are — with the store and the hash after each. Each is named by its fixed
/// hash, `sha256(enc(Text name))`, which no module carries.
pub fn raw(out: &Out) {
    use ark::raw::{self, Raw};
    let m = demo::module();
    let pid = id_n(1);
    let playlist = |name: &str| {
        Value::record(vec![
            ("id", Value::Id(pid)),
            ("name", Value::text(name)),
            ("user_id", Value::text("alice")),
        ])
    };
    let item = |track: &str, pos: i64| {
        Value::record(vec![
            ("playlist_id", Value::Id(pid)),
            ("track_id", Value::text(track)),
            ("pos", Value::Int(pos)),
        ])
    };
    let put = |table: &str, row: Value| args([("table", Value::text(table)), ("row", row)]);
    let del = |table: &str, key: Vec<Value>| args([("table", Value::text(table)), ("key", Value::List(key.into()))]);
    let write = |name: &str, raw: Raw, st0: &MemoryStore, steps: Vec<Args>| {
        super::claim(
            &format!("{} is named by the hash of its name", raw.name()),
            raw::of(&raw::hash_of(raw.name())) == Some(raw),
        );
        let mut st = st0.clone();
        let mut parts = vec![];
        for a in steps {
            let step = match raw::apply(raw, &a, &mut st).unwrap_or_else(|e| panic!("{name}: a bug: {e:?}")) {
                Ok(chs) => obj(&[
                    ("args", json(&Value::from(a.clone()))),
                    ("changes", json(&Value::List(chs.iter().map(change_value).collect()))),
                    ("store_after", json(&st.store_value())),
                    ("hash_after", quoted(&hex(&state_hash(&st)))),
                ]),
                Err(r) => obj(&[
                    ("args", json(&Value::from(a.clone()))),
                    ("refused", quoted(&r.to_string())),
                    ("store_after", json(&st.store_value())),
                    ("hash_after", quoted(&hex(&state_hash(&st)))),
                ]),
            };
            parts.push(step);
        }
        out.write(
            name,
            &obj(&[
                ("module", json(&module_value(&m))),
                ("raw", quoted(raw.name())),
                ("function_hash", quoted(&hex(raw.hash()))),
                ("store_before", json(&st0.store_value())),
                ("steps", array(parts)),
            ]),
        );
        st
    };
    let empty = MemoryStore::empty(m.schema.clone());
    // A playlist put, an item on it, the playlist renamed, the same row put
    // again (no change), and an item naming a playlist that is not there —
    // refused by the reference, as a mutator's `put` would be.
    let st = write(
        "eval/raw-put-row.json",
        Raw::PutRow,
        &empty,
        vec![
            put("playlist", playlist("Favorites")),
            put("item", item("t7", 1)),
            put("playlist", playlist("Kept")),
            put("playlist", playlist("Kept")),
            put(
                "item",
                Value::record(vec![
                    ("playlist_id", Value::Id(id_n(2))),
                    ("track_id", Value::text("t9")),
                    ("pos", Value::Int(1)),
                ]),
            ),
        ],
    );
    super::claim(
        "the puts left a playlist and an item",
        st.scan("playlist").len() == 1 && st.scan("item").len() == 1,
    );
    // From there: the playlist refused while its item references it, the
    // item taken by its key, a key nothing has (no change), the playlist.
    let after = write(
        "eval/raw-delete-row.json",
        Raw::DeleteRow,
        &st,
        vec![
            del("playlist", vec![Value::Id(pid)]),
            del("item", vec![Value::Id(pid), Value::text("t7")]),
            del("item", vec![Value::Id(pid), Value::text("t7")]),
            del("playlist", vec![Value::Id(pid)]),
        ],
    );
    super::claim(
        "the deletes left nothing",
        after.scan("playlist").is_empty() && after.scan("item").is_empty(),
    );
}
