//! `verify/` (§9), `eval/` (§6, §8) and `hash/` (§8.1): the demo module
//! verifies and two one-line edits of it do not; `add_to_playlist` applied
//! four times, with the changes, the store and the hash after each; the
//! input checks as verdicts, and the form validator.

use ark::eval::{apply, check, Args, Ctx};
use ark::hash::{closure, function_hash, state_hash};
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
            ("args", json(&Value::Struct(add_args(k)))),
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
            ("autos", json(&Value::Struct(autos.clone()))),
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
                        ("autos", json(&Value::Struct(a.clone()))),
                        ("args", json(&Value::Struct(i.clone()))),
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
                        ("input", json(&Value::Struct(input.clone()))),
                        ("messages", json(&Value::List(messages))),
                        ("normalised", json(&Value::Struct(got.values))),
                    ])
                })),
            ),
        ]),
    );
}
