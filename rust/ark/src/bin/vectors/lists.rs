//! The list editors (§5.1, `stdlib.rs`): `ListConcat`, `Insert`,
//! `RemoveAt`, `Move` and `Slice`, over a module of their own, so that no
//! module and no vector from before them moves — a module that does not
//! call one never carries its name.
//!
//! - `verify/list-ops.json`: the module verifies, a `NonEmpty` read as a
//!   list by every editor; and two `falsify/` modules the checker refuses —
//!   an element of the wrong type put in, a text where an index goes;
//! - `eval/list-ops.json`: each editor in a mutator, over every edge an
//!   index has — the front, the end, one past it, below zero, an empty list
//!   — the result written as the text of its elements joined, or the
//!   refusal's exact words; and one argument faulting before an index is
//!   judged, since arguments are evaluated first (§6). Its `falsify/` reads
//!   an index past the end as the end, which is clamping;
//! - `eval/list-ops-live.json`: a live state, `deck`, whose transitions edit
//!   a list and a `NonEmpty` — the `NonEmpty` kept by an insert, and refused
//!   at the transition's result when a removal empties it — and its
//!   `falsify/`, the same clamping.

// The records are the vocabulary's: a body reads their fields only
// natively, and the generator only ever emits.
#![allow(dead_code)]

use std::collections::BTreeMap;

use ark::authoring::*;
use ark::eval::{Args, Ctx as EvalCtx, EvalFault, LiveCtx};
use ark::hash::{live_closure, live_hash, state_type_hash};
use ark::ir::{module_value, Expr, Module as IrModule, StdFn, Stmt, ON_JOIN, ON_LEAVE, ON_RESTORE};
use ark::live::state::Runner;
use ark::protocol::change_value;
use ark::store::{MemoryStore, Refusal, Store};
use ark::value::{hex, Value};
use ark::verify::{verify, Complaint, VerifyError};

use super::json::{array, json, obj, quoted};
use super::{claim, Out};

// The domain ---------------------------------------------------------------

pub struct Lists {
    pub joined: Table<Joined>,
}
impl Tables for Lists {
    fn open() -> Self {
        Lists { joined: table() }
    }
}

/// What a mutator made of its list: the elements joined into one text,
/// under the mutator's name.
pub struct Joined {
    pub id: Text,
    pub text: Text,
}
impl Row for Joined {
    const NAME: &str = "joined";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns().text(Self::id).text(Self::text).key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Joined {
    pub const id: Col<Self, Text> = col("id");
    pub const text: Col<Self, Text> = col("text");
}

pub struct Two {
    pub xs: List<Text>,
    pub ys: List<Text>,
}
impl Input for Two {
    fn schema() -> Object<Self> {
        object().field("xs", list(text())).field("ys", list(text()))
    }
}

pub struct Put {
    pub xs: List<Text>,
    pub at: Int,
    pub x: Text,
}
impl Input for Put {
    fn schema() -> Object<Self> {
        object().field("xs", list(text())).field("at", int()).field("x", text())
    }
}

pub struct One {
    pub xs: List<Text>,
    pub at: Int,
}
impl Input for One {
    fn schema() -> Object<Self> {
        object().field("xs", list(text())).field("at", int())
    }
}

pub struct FromTo {
    pub xs: List<Text>,
    pub from: Int,
    pub to: Int,
}
impl Input for FromTo {
    fn schema() -> Object<Self> {
        object().field("xs", list(text())).field("from", int()).field("to", int())
    }
}

pub struct Run {
    pub xs: List<Text>,
    pub start: Int,
    pub end: Int,
}
impl Input for Run {
    fn schema() -> Object<Self> {
        object().field("xs", list(text())).field("start", int()).field("end", int())
    }
}

/// A mutator's result, written: its name and the list's elements joined.
fn written(db: &Lists, name: &str, xs: List<Text>) -> Write<Joined> {
    db.joined.insert(Joined {
        id: Text::from(name),
        text: concat(xs),
    })
}

/// One mutator per editor, and `insert_after`, whose index is an
/// expression evaluated before the list is asked anything.
pub fn lists() -> Router<Lists> {
    let r = router::<Lists>("lists");
    r.routes((
        r.input::<Two>()
            .mutation("concat", |_ctx, db, input| written(db, "concat", input.xs.concat(input.ys))),
        r.input::<Put>()
            .mutation("insert", |_ctx, db, input| written(db, "insert", input.xs.insert(input.at, input.x))),
        r.input::<Put>().mutation("insert_after", |_ctx, db, input| {
            written(db, "insert_after", input.xs.insert(input.at.add(1), input.x))
        }),
        r.input::<One>()
            .mutation("remove_at", |_ctx, db, input| written(db, "remove_at", input.xs.remove_at(input.at))),
        r.input::<FromTo>()
            .mutation("move", |_ctx, db, input| written(db, "move", input.xs.move_at(input.from, input.to))),
        r.input::<Run>()
            .mutation("slice", |_ctx, db, input| written(db, "slice", input.xs.slice(input.start, input.end))),
    ))
}

/// The live state: a list edited by every editor, and a hand that is never
/// empty.
pub struct Deck {
    pub hand: NonEmpty<Text>,
    pub items: List<Text>,
}
impl Record for Deck {
    fn fields() -> Fields<Self> {
        fields()
            .named("deck")
            .field("hand", typed::<NonEmpty<Text>>())
            .field("items", typed::<List<Text>>())
    }
}

pub struct More {
    pub more: List<Text>,
}
impl Input for More {
    fn schema() -> Object<Self> {
        object().field("more", list(text()))
    }
}

pub struct Item {
    pub at: Int,
    pub item: Text,
}
impl Input for Item {
    fn schema() -> Object<Self> {
        object().field("at", int()).field("item", text())
    }
}

pub struct At {
    pub at: Int,
}
impl Input for At {
    fn schema() -> Object<Self> {
        object().field("at", int())
    }
}

pub struct Places {
    pub from: Int,
    pub to: Int,
}
impl Input for Places {
    fn schema() -> Object<Self> {
        object().field("from", int()).field("to", int())
    }
}

pub struct Bounds {
    pub start: Int,
    pub end: Int,
}
impl Input for Bounds {
    fn schema() -> Object<Self> {
        object().field("start", int()).field("end", int())
    }
}

/// The router that declares the deck.
pub fn deck() -> Router<Lists> {
    let r = router::<Lists>("table");
    r.live::<Deck>("deck")
        .scope(|ctx| ctx.user)
        .init(|| Deck {
            hand: list([Text::from("h")]).non_empty(),
            items: list([Text::from("a"), Text::from("b"), Text::from("c")]),
        })
        .transition::<More>("concat", |s, _ctx, input| Deck {
            items: s.items.concat(input.more),
            ..s
        })
        .transition::<Item>("insert", |s, _ctx, input| Deck {
            items: s.items.insert(input.at, input.item),
            ..s
        })
        .transition::<At>("remove_at", |s, _ctx, input| Deck {
            items: s.items.remove_at(input.at),
            ..s
        })
        .transition::<Places>("move", |s, _ctx, input| Deck {
            items: s.items.move_at(input.from, input.to),
            ..s
        })
        .transition::<Bounds>("slice", |s, _ctx, input| Deck {
            items: s.items.slice(input.start, input.end),
            ..s
        })
        // A card put into the hand: a `NonEmpty` stays one.
        .transition::<Item>("deal", |s, _ctx, input| Deck {
            hand: s.hand.insert(input.at, input.item),
            ..s
        })
        // A card taken out: a list, declared non-empty again, and refused
        // where the transition's result is held to it.
        .transition::<At>("discard", |s, _ctx, input| Deck {
            hand: s.hand.remove_at(input.at).non_empty(),
            ..s
        });
    r
}

fn domain() -> Module {
    Module::new((lists(), deck()))
}

/// The module, verified.
pub fn module() -> IrModule {
    domain().build().clone()
}

// The vectors ---------------------------------------------------------------

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn texts(ts: &[&str]) -> Value {
    Value::from(ts.iter().map(|t| Value::text(*t)).collect::<Vec<_>>())
}

pub fn lists_vectors(out: &Out) {
    out.dir("verify/ eval/ (list editors)");
    let m = module();
    claim("the list module verifies", verify(&m).is_ok());
    verify_vectors(out, &m);
    cases(out, &m);
    live(out, &m);
}

// The module with every `Std(f, ..)` of the named function rewritten.
fn editing(m: &IrModule, name: &str, f: StdFn, edit: &dyn Fn(&mut Vec<Expr>)) -> IrModule {
    fn walk(e: &mut Expr, f: StdFn, edit: &dyn Fn(&mut Vec<Expr>)) {
        if let Expr::Std(g, es) = e {
            if *g == f {
                edit(es);
            }
        }
        match e {
            Expr::Struct(fs) => fs.values_mut().for_each(|e| walk(e, f, edit)),
            Expr::Std(_, es) | Expr::List(es) | Expr::Op(_, es) => es.iter_mut().for_each(|e| walk(e, f, edit)),
            _ => {}
        }
    }
    let mut m = m.clone();
    for fun in m.functions.iter_mut().filter(|fun| fun.name == name) {
        for s in fun.body.iter_mut() {
            if let Stmt::Insert(_, e, _) = s {
                walk(e, f, edit);
            }
        }
    }
    m
}

fn verify_vectors(out: &Out, m: &IrModule) {
    out.write(
        "verify/list-ops.json",
        &obj(&[("module", json(&module_value(m))), ("verifies", "true".into())]),
    );
    let misuse = |what: &str, broken: &IrModule| {
        let es = verify(broken).err().unwrap_or_default();
        claim(
            &format!("{what} is refused: {es:?}"),
            es.iter().any(|e| matches!(e, VerifyError::In(_, Complaint::StdMisuse(_)))),
        );
    };
    // An int put into a list of texts.
    let wrong_element = editing(m, "insert", StdFn::Insert, &|es| es[2] = Expr::Lit(Value::Int(1)));
    misuse("an int inserted into a list of texts", &wrong_element);
    out.write(
        "verify/falsify/list-ops-wrong-element.json",
        &obj(&[
            ("module", json(&module_value(&wrong_element))),
            ("verifies", "true".into()),
            ("expect", quoted("fail")),
        ]),
    );
    // A text where a slice's end goes.
    let text_bound = editing(m, "slice", StdFn::Slice, &|es| es[2] = Expr::Lit(Value::text("end")));
    misuse("a slice ending at a text", &text_bound);
    out.write(
        "verify/falsify/list-ops-text-index.json",
        &obj(&[
            ("module", json(&module_value(&text_bound))),
            ("verifies", "true".into()),
            ("expect", quoted("fail")),
        ]),
    );
}

fn cases(out: &Out, m: &IrModule) {
    let d = domain();
    let st = MemoryStore::empty(m.schema.clone());
    let ctx = EvalCtx::new("alice", "session-1");
    let abc = || texts(&["a", "b", "c"]);
    let abcd = || texts(&["a", "b", "c", "d"]);
    let put = |xs: Value, at: i64| args([("xs", xs), ("at", Value::Int(at)), ("x", Value::text("x"))]);
    let one = |xs: Value, at: i64| args([("xs", xs), ("at", Value::Int(at))]);
    let from_to = |from: i64, to: i64| args([("xs", abcd()), ("from", Value::Int(from)), ("to", Value::Int(to))]);
    let run = |start: i64, end: i64| args([("xs", abcd()), ("start", Value::Int(start)), ("end", Value::Int(end))]);
    // (name, function, args, the text written, or the refusal)
    let all: Vec<(&str, &str, Args, Result<&str, &str>)> = vec![
        (
            "concat-two",
            "concat",
            args([("xs", texts(&["a", "b"])), ("ys", texts(&["c"]))]),
            Ok("abc"),
        ),
        ("concat-onto-empty", "concat", args([("xs", texts(&[])), ("ys", abc())]), Ok("abc")),
        ("concat-empty", "concat", args([("xs", texts(&[])), ("ys", texts(&[]))]), Ok("")),
        ("insert-at-the-front", "insert", put(abc(), 0), Ok("xabc")),
        ("insert-in-the-middle", "insert", put(abc(), 2), Ok("abxc")),
        ("insert-at-the-end", "insert", put(abc(), 3), Ok("abcx")),
        ("insert-into-empty", "insert", put(texts(&[]), 0), Ok("x")),
        (
            "insert-past-the-end",
            "insert",
            put(abc(), 4),
            Err("insert: index 4 out of range for a list of 3"),
        ),
        (
            "insert-below-zero",
            "insert",
            put(abc(), -1),
            Err("insert: index -1 out of range for a list of 3"),
        ),
        (
            "insert-far-past-the-end",
            "insert",
            put(abc(), i64::MAX),
            Err("insert: index 9223372036854775807 out of range for a list of 3"),
        ),
        ("insert-after", "insert_after", put(abc(), 0), Ok("axbc")),
        // The index is `at + 1`, which overflows before the list is asked.
        (
            "insert-after-overflows-first",
            "insert_after",
            put(abc(), i64::MAX),
            Err("integer overflow"),
        ),
        ("remove-the-middle", "remove_at", one(abc(), 1), Ok("ac")),
        ("remove-the-last", "remove_at", one(abc(), 2), Ok("ab")),
        ("remove-the-only", "remove_at", one(texts(&["a"]), 0), Ok("")),
        (
            "remove-past-the-end",
            "remove_at",
            one(abc(), 3),
            Err("remove_at: index 3 out of range for a list of 3"),
        ),
        (
            "remove-from-empty",
            "remove_at",
            one(texts(&[]), 0),
            Err("remove_at: index 0 out of range for a list of 0"),
        ),
        (
            "remove-below-zero",
            "remove_at",
            one(abc(), -1),
            Err("remove_at: index -1 out of range for a list of 3"),
        ),
        ("move-forward", "move", from_to(0, 2), Ok("bcad")),
        ("move-to-the-end", "move", from_to(0, 3), Ok("bcda")),
        ("move-backward", "move", from_to(3, 1), Ok("adbc")),
        ("move-in-place", "move", from_to(1, 1), Ok("abcd")),
        (
            "move-from-out-of-range",
            "move",
            from_to(4, 0),
            Err("move: index 4 out of range for a list of 4"),
        ),
        (
            "move-to-out-of-range",
            "move",
            from_to(0, -1),
            Err("move: index -1 out of range for a list of 4"),
        ),
        // Both out of range: the first index is the one named.
        (
            "move-both-out-of-range",
            "move",
            from_to(9, -1),
            Err("move: index 9 out of range for a list of 4"),
        ),
        ("slice-the-middle", "slice", run(1, 3), Ok("bc")),
        ("slice-all", "slice", run(0, 4), Ok("abcd")),
        ("slice-nothing", "slice", run(2, 2), Ok("")),
        ("slice-nothing-at-the-end", "slice", run(4, 4), Ok("")),
        ("slice-backwards", "slice", run(3, 1), Err("slice: 3..1 out of range for a list of 4")),
        ("slice-past-the-end", "slice", run(0, 5), Err("slice: 0..5 out of range for a list of 4")),
        ("slice-below-zero", "slice", run(-1, 2), Err("slice: -1..2 out of range for a list of 4")),
    ];
    let mut written = vec![];
    let mut clamped = vec![];
    for (name, function, input, want) in &all {
        let (_, p) = d.procedure(function).unwrap_or_else(|| panic!("no procedure {function}"));
        let got = p
            .agrees(&ctx, &Args::new(), input, &st)
            .unwrap_or_else(|e| panic!("list-ops {name}: native and interpreted disagree: {e}"));
        let mut parts = vec![
            ("name", quoted(name)),
            ("function", quoted(function)),
            ("autos", json(&Value::from(Args::new()))),
            ("args", json(&Value::from(input.clone()))),
        ];
        match (got, want) {
            (Ok(Ok(chs)), Ok(text)) => {
                let row = match chs.as_slice() {
                    [ark::store::Change::Add(t, row)] if t == "joined" => row.get("text").cloned(),
                    _ => None,
                };
                claim(&format!("list-ops {name} writes {text:?}"), row == Some(Value::text(*text)));
                parts.push(("refused", "null".into()));
                parts.push(("changes", json(&Value::List(chs.iter().map(change_value).collect()))));
            }
            (Ok(Err(Refusal::Refused(t))), Err(w)) if t == *w => parts.push(("refused", quoted(w))),
            other => panic!("list-ops {name}: {other:?}"),
        }
        let w = obj(&parts);
        // A runner that clamps an index past the end to the end applies
        // `insert-past-the-end` as an append: the falsify says that.
        clamped.push(if *name == "insert-past-the-end" {
            let mut p: Vec<(&str, String)> = parts.iter().filter(|(k, _)| *k != "refused").cloned().collect();
            p.push(("refused", "null".into()));
            p.push((
                "changes",
                json(&Value::from(vec![Value::record(vec![
                    ("t", Value::text("add")),
                    ("table", Value::text("joined")),
                    ("row", Value::record(vec![("id", Value::text("insert")), ("text", Value::text("abcx"))])),
                ])])),
            ));
            obj(&p)
        } else {
            w.clone()
        });
        written.push(w);
    }
    claim(
        "thirteen of the list-ops cases refuse",
        all.iter().filter(|(_, _, _, w)| w.is_err()).count() == 13,
    );
    let body = |cases: Vec<String>, fail: bool| {
        let mut p = vec![
            ("module", json(&module_value(m))),
            ("store_before", json(&st.store_value())),
            ("cases", array(cases)),
        ];
        if fail {
            p.push(("expect", quoted("fail")));
        }
        obj(&p)
    };
    out.write("eval/list-ops.json", &body(written, false));
    out.write("eval/falsify/list-ops-clamping.json", &body(clamped, true));
}

fn lctx_value(c: &LiveCtx) -> Value {
    Value::record(vec![
        ("user", Value::text(c.user.clone())),
        ("session", Value::text(c.session.clone())),
        ("roles", Value::from(Vec::<Value>::new())),
        ("device", Value::text(c.device.clone())),
        ("now", Value::Int(c.now)),
    ])
}

fn live(out: &Out, m: &IrModule) {
    let st = m.states.iter().find(|s| s.name == "deck").expect("the deck");
    let c = live_closure(m, st);
    let r = Runner::of_module(m);
    let natives = domain().lives();
    let native = natives.iter().find(|n| n.name() == "deck").expect("the deck, natively");
    let kinds: BTreeMap<String, Value> = st
        .transitions
        .iter()
        .map(|t| (t.name.clone(), Value::text(c.kind(&t.name).name())))
        .chain([ON_JOIN, ON_LEAVE, ON_RESTORE].map(|h| (h.to_string(), Value::text(c.kind(h).name()))))
        .collect();
    let item = |at: i64, t: &str| args([("at", Value::Int(at)), ("item", Value::text(t))]);
    let at = |at: i64| args([("at", Value::Int(at))]);
    let places = |from: i64, to: i64| args([("from", Value::Int(from)), ("to", Value::Int(to))]);
    let bounds = |start: i64, end: i64| args([("start", Value::Int(start)), ("end", Value::Int(end))]);
    let steps: Vec<(&str, Args)> = vec![
        ("insert", item(0, "x")),
        ("insert", item(4, "y")),
        ("insert", item(6, "z")),
        ("insert", item(-1, "z")),
        ("move", places(0, 4)),
        ("move", places(4, 1)),
        ("move", places(2, 2)),
        ("move", places(7, 9)),
        ("move", places(0, 5)),
        ("remove_at", at(1)),
        ("remove_at", at(4)),
        ("slice", bounds(1, 3)),
        ("slice", bounds(2, 1)),
        ("slice", bounds(2, 2)),
        ("remove_at", at(0)),
        ("slice", bounds(0, 1)),
        ("concat", args([("more", texts(&[]))])),
        ("concat", args([("more", texts(&["p", "q"]))])),
        ("insert", item(i64::MAX, "z")),
        ("deal", item(1, "j")),
        ("deal", item(0, "k")),
        ("deal", item(3, "l")),
        ("deal", item(5, "m")),
        ("discard", at(1)),
        ("discard", at(0)),
        ("discard", at(1)),
        ("discard", at(0)),
        ("discard", at(3)),
    ];
    let init = r.init("deck").expect("an init");
    let mut s = init.clone();
    let mut written = vec![];
    let mut clamped = vec![];
    for (k, (t, input)) in steps.iter().enumerate() {
        let ctx = LiveCtx {
            user: "alice".into(),
            session: "phone-login".into(),
            roles: Default::default(),
            device: "phone".into(),
            now: 2000 + k as i64,
        };
        let mut fields = vec![
            ("transition", Value::text(*t)),
            ("ctx", lctx_value(&ctx)),
            ("input", Value::from(input.clone())),
        ];
        let got = native
            .agrees(t, &s, &ctx, input)
            .unwrap_or_else(|e| panic!("deck {t}: native and interpreted disagree: {e}"));
        let ran = r.run("deck", t, &s, &ctx, input);
        claim(&format!("deck {t}: the runner is the closure"), ran == got);
        let mut wrong = None;
        match got {
            Ok(next) => {
                s = next;
                fields.push(("state", s.clone()));
                fields.push(("hash", Value::text(hex(&r.hash("deck", &s)))));
            }
            Err(EvalFault::Verdict(Refusal::Refused(why))) => {
                // Clamping `insert(6, "z")` into five appends it.
                if k == 2 {
                    let mut items = s.field("items").as_list().to_vec();
                    items.push(Value::text("z"));
                    let mut f = s.as_struct().clone();
                    f.insert("items".into(), Value::from(items));
                    let clamped_state = Value::from(f);
                    let mut fs = fields.clone();
                    fs.push(("state", clamped_state.clone()));
                    fs.push(("hash", Value::text(hex(&r.hash("deck", &clamped_state)))));
                    wrong = Some(Value::record(fs));
                }
                fields.push(("refused", Value::text(why)));
            }
            Err(other) => panic!("deck {t}: {other:?}"),
        }
        let w = Value::record(fields);
        clamped.push(wrong.unwrap_or_else(|| w.clone()));
        written.push(w);
    }
    let refusals: Vec<String> = written
        .iter()
        .filter_map(|w| w.as_struct().get("refused").map(|r| r.as_text().to_string()))
        .collect();
    claim(&format!("the deck refuses twelve times: {refusals:?}"), refusals.len() == 12);
    claim(
        "an emptied hand is refused at the transition's result",
        refusals.contains(&"discard: the state it makes is not a deck: hand: an empty list where a non-empty one is declared".to_string()),
    );
    claim(
        "the deck ends with p and q, and a hand of j",
        s == Value::record(vec![("hand", texts(&["j"])), ("items", texts(&["p", "q"]))]),
    );
    let parts = |steps: &[Value], fail: bool| {
        let mut p = vec![
            ("module", json(&module_value(m))),
            ("live", quoted("deck")),
            ("live_hash", quoted(&hex(&live_hash(&c)))),
            ("type_hash", quoted(&hex(&state_type_hash(&c)))),
            ("kinds", json(&Value::from(kinds.clone()))),
            ("init", json(&init)),
            ("init_hash", quoted(&hex(&r.hash("deck", &init)))),
            ("steps", format!("[\n{}\n]", steps.iter().map(json).collect::<Vec<_>>().join(",\n"))),
        ];
        if fail {
            p.push(("expect", quoted("fail")));
        }
        obj(&p)
    };
    out.write("eval/list-ops-live.json", &parts(&written, false));
    out.write("eval/falsify/list-ops-live-clamping.json", &parts(&clamped, true));
}
