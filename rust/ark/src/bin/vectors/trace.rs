//! `trace/`: clients' lives, as `arkc fuzz --traces` writes them
//! (`docs/plan-phones.md` S2, `ark::trace`) — a few dozen, chosen for
//! coverage from a fixed run of the fuzzer, so that a runtime in another
//! language meets every feature of a client on a file it can read before it
//! meets the corpus. Each feature is a predicate over a trace's steps, and
//! the trace kept for it is the smallest of the run that has it; the run is
//! fixed (`SEED`, `CASES`), so the choice is too. Beside them, harken's:
//! sessions over `harken.ark` with its library and playlist views kept, so
//! the patches harken's screens splice are what a runtime must reproduce.
//! Each is replayed here before it is written ([`ark::trace::replay`]), and
//! `falsify/` holds one with a frame's byte changed, which must not.

use std::collections::{BTreeMap, BTreeSet};

use ark::canon::decode;
use ark::hash::{closures, FnHash};
use ark::ir::{module_from_value, Module};
use ark::protocol::ServerMsg;
use ark::value::Value;

use super::{claim, Out};
use crate::fuzz;

/// The run the generated traces are chosen from: `arkc fuzz --seed SEED
/// --cases CASES --traces DIR`, every trace of which is a candidate.
const SEED: u64 = 7000;
const CASES: u64 = 160;

/// harken's sessions: this many from this seed, its two views kept.
const HARKEN_SEED: u64 = 0x4a7e_7000;
const HARKEN_SESSIONS: u64 = 24;

/// The module harken's clients load, as the domain crate writes it.
const HARKEN: &[u8] = include_bytes!("../../../../../harken/domain/harken.ark");

/// A trace a feature may be chosen from.
struct Candidate {
    from: String,
    module: Module,
    steps: Vec<(Value, Value)>,
    text: String,
}

/// What a trace is read for: its inputs by kind, the frames it received, and
/// its state as it moved.
struct Read<'a> {
    c: &'a Candidate,
    received: Vec<ServerMsg>,
}

impl<'a> Read<'a> {
    fn of(c: &'a Candidate) -> Read<'a> {
        let received = c
            .steps
            .iter()
            .filter(|(i, _)| i.field("t") == Value::text("receive"))
            .filter_map(|(i, _)| decode(i.field("frame").as_bytes()).ok())
            .filter_map(|v| ServerMsg::from_value(&v).ok())
            .collect();
        Read { c, received }
    }

    fn inputs(&self, t: &str) -> usize {
        self.c.steps.iter().filter(|(i, _)| i.field("t") == Value::text(t)).count()
    }

    // Every value a state field took, in order, as each step wrote it.
    fn states(&self, k: &str) -> Vec<Value> {
        self.c.steps.iter().filter_map(|(_, o)| o.as_struct().get(k).cloned()).collect()
    }

    fn emitted(&self, kind: &str) -> bool {
        self.c.steps.iter().any(|(_, o)| {
            o.as_struct().get("frames").is_some_and(|fs| {
                fs.as_list()
                    .iter()
                    .filter_map(|f| decode(f.as_bytes()).ok())
                    .any(|v| v.as_struct().get("t") == Some(&Value::text(kind)))
            })
        })
    }

    fn private(&self) -> BTreeSet<FnHash> {
        closures(&self.c.module)
            .into_iter()
            .filter(|(_, c)| c.function.private)
            .map(|(h, _)| h)
            .collect()
    }

    // A settle that landed confirmed entries under intents still pending
    // after it: a rebase over what landed.
    fn rebased(&self) -> bool {
        let (mut cursor, mut pending) = (Value::Int(0), Value::List(Box::new([])));
        for (i, o) in &self.c.steps {
            let m = o.as_struct();
            let was_pending = !pending.as_list().is_empty();
            let moved = m.get("cursor").is_some_and(|n| *n != cursor);
            if let Some(n) = m.get("cursor") {
                cursor = n.clone();
            }
            if let Some(p) = m.get("pending") {
                pending = p.clone();
            }
            if i.field("t") == Value::text("settle") && moved && was_pending && !pending.as_list().is_empty() {
                return true;
            }
        }
        false
    }

    // Views that reported patches, by query.
    fn patched(&self, query: &str) -> bool {
        let mut specs: Vec<String> = vec![];
        for (i, o) in &self.c.steps {
            if i.field("t") == Value::text("hydrate") {
                specs = i
                    .field("views")
                    .as_list()
                    .iter()
                    .map(|v| v.field("query").as_text().to_string())
                    .collect();
            }
            if let Some(vs) = o.as_struct().get("views") {
                for (k, ev) in vs.as_list().iter().enumerate() {
                    let patches = ev.as_struct_opt().and_then(|m| m.get("patches")).is_some_and(|p| !p.as_list().is_empty());
                    if patches && specs.get(k).map(String::as_str) == Some(query) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

trait StructOpt {
    fn as_struct_opt(&self) -> Option<&BTreeMap<String, Value>>;
}

impl StructOpt for Value {
    fn as_struct_opt(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Struct(m) => Some(m),
            _ => None,
        }
    }
}

/// The features, each a name a trace is published under and what a trace
/// must show to have it.
#[allow(clippy::type_complexity)]
fn features() -> Vec<(&'static str, Box<dyn Fn(&Read) -> bool>)> {
    vec![
        // A whole client paged entries and acknowledged, and nothing else
        // a later feature names.
        (
            "plain-sync",
            Box::new(|r: &Read| {
                r.received
                    .iter()
                    .any(|m| matches!(m, ServerMsg::Batch { items, .. } if !items.is_empty()))
                    && r.received.iter().any(|m| matches!(m, ServerMsg::Ack { .. }))
                    && r.inputs("reopen") == 0
                    && r.inputs("alone") == 0
                    && r.states("union").iter().all(Value::is_null)
                    && !r.received.iter().any(|m| matches!(m, ServerMsg::SnapshotOf { .. }))
            }),
        ),
        ("rebase", Box::new(|r: &Read| r.rebased())),
        (
            "a-rejection",
            Box::new(|r: &Read| r.states("rejections").iter().any(|v| !v.as_list().is_empty())),
        ),
        (
            "a-local-refusal",
            Box::new(|r: &Read| r.c.steps.iter().any(|(_, o)| o.as_struct().contains_key("refused"))),
        ),
        ("a-restart", Box::new(|r: &Read| r.inputs("reopen") > 0 && r.inputs("mutate") > 0)),
        (
            "a-snapshot-below-the-horizon",
            Box::new(|r: &Read| r.received.iter().any(|m| matches!(m, ServerMsg::SnapshotOf { held: None, .. }))),
        ),
        (
            "a-log-lost-and-named",
            Box::new(|r: &Read| {
                let logs: Vec<Value> = r.states("log").into_iter().filter(|l| !l.is_null()).collect();
                logs.windows(2).any(|w| w[0] != w[1])
            }),
        ),
        (
            "alone-then-join",
            Box::new(|r: &Read| {
                let mut alone = false;
                let mut authored = false;
                for (i, o) in &r.c.steps {
                    match i.field("t").as_text() {
                        "alone" => alone = true,
                        "mutate" if alone && !o.as_struct().contains_key("refused") => authored = true,
                        "join" if authored => return true,
                        _ => {}
                    }
                }
                false
            }),
        ),
        (
            "roles-stamped",
            Box::new(|r: &Read| r.received.iter().any(|m| matches!(m, ServerMsg::Ack { facts, .. } if !facts.is_empty()))),
        ),
        (
            "a-partial-client-excluding-a-column-through-exists",
            Box::new(|r: &Read| {
                let m = &r.c.module;
                let exists = m
                    .functions
                    .iter()
                    .flat_map(|f| &f.holds)
                    .any(|h| format!("{:?}", h.filter).contains("Exists"));
                let narrowed = r.states("union").iter().any(|u| {
                    u.as_struct_opt().is_some_and(|h| {
                        h.iter()
                            .any(|(t, cols)| m.schema.lookup_table(t).is_some_and(|tb| cols.as_list().len() < tb.columns.len()))
                    })
                });
                exists && narrowed
            }),
        ),
        (
            "a-private-entry-taken-by-facts",
            Box::new(|r: &Read| {
                let private = r.private();
                r.received.iter().any(|m| match m {
                    ServerMsg::Batch { items, covers: None, .. } => items.iter().any(|(_, e, f)| f.is_some() && private.contains(&e.fn_hash)),
                    _ => false,
                })
            }),
        ),
        (
            "a-raw-write-replayed",
            Box::new(|r: &Read| {
                r.received.iter().any(|m| match m {
                    ServerMsg::Batch { items, covers: None, .. } => items.iter().any(|(_, e, _)| ark::raw::is_raw(&e.fn_hash)),
                    _ => false,
                })
            }),
        ),
        (
            "a-verify-answered-unknown",
            Box::new(|r: &Read| r.states("agreed").iter().any(|a| a.as_list().iter().any(|x| x.field("ok").is_null()))),
        ),
        (
            "a-verify-answered-agreed",
            Box::new(|r: &Read| {
                r.states("agreed")
                    .iter()
                    .any(|a| a.as_list().iter().any(|x| x.field("ok") == Value::Bool(true)))
            }),
        ),
        ("a-sign-in", Box::new(|r: &Read| r.inputs("sign_in") > 0 && r.inputs("mutate") > 0)),
        ("needing-facts", Box::new(|r: &Read| r.emitted("need_facts"))),
        (
            "fed-by-facts",
            Box::new(|r: &Read| {
                r.c.steps
                    .first()
                    .is_some_and(|(i, _)| i.field("t") == Value::text("open") && i.field("mode") == Value::text("facts"))
                    && r.inputs("mutate") > 0
            }),
        ),
        (
            "a-partial-client-reopened",
            Box::new(|r: &Read| r.inputs("reopen") > 0 && r.states("union").iter().any(|u| !u.is_null())),
        ),
        (
            "without-closures",
            Box::new(|r: &Read| {
                r.c.steps.first().is_some_and(|(i, _)| i.field("closures") == Value::Bool(false))
                    && r.received.iter().any(|m| matches!(m, ServerMsg::FactsFor { items } if !items.is_empty()))
            }),
        ),
        (
            "a-frame-delivered-twice",
            Box::new(|r: &Read| {
                r.c.steps
                    .windows(2)
                    .any(|w| w[0].0.field("t") == Value::text("receive") && w[0].0 == w[1].0 && r.inputs("mutate") > 0)
            }),
        ),
        (
            "the-demo",
            Box::new(|r: &Read| r.c.module.lookup_function("add_to_playlist").is_some() && r.rebased()),
        ),
        (
            "views-patched",
            Box::new(|r: &Read| {
                r.c.steps.iter().any(|(_, o)| {
                    o.as_struct().get("views").is_some_and(|vs| {
                        vs.as_list()
                            .iter()
                            .any(|ev| ev.as_struct_opt().and_then(|m| m.get("patches")).is_some_and(|p| p.as_list().len() >= 2))
                    })
                })
            }),
        ),
    ]
}

// Every trace a run of cases from `seed` records, of sessions that found
// nothing, with `only` limiting the views; for harken, its own sessions.
fn fuzzed(seed: u64, cases: u64) -> Vec<Candidate> {
    let mut out = vec![];
    for k in 0..cases {
        let case = seed + k;
        let mut stats = fuzz::Stats::default();
        let Some((m, natives, mut rng)) = fuzz::case_module(case, &mut stats) else {
            continue;
        };
        for s in 0..2 {
            let (found, _, traces) = fuzz::session::run_traced(&m, &natives, rng.next(), &[], &mut stats.tally, Some(None));
            claim(
                &format!("arkc fuzz --seed {case} --cases 1 finds nothing: {:?}", found.as_ref().map(|f| f.why())),
                found.is_none(),
            );
            for (i, t) in traces {
                let from = fuzz::trace_from(case, s, i);
                let text = ark::trace::write(&m, &from, &t.steps, &[]);
                out.push(Candidate {
                    from,
                    module: m.clone(),
                    steps: t.steps,
                    text,
                });
            }
        }
    }
    out
}

fn harken() -> Vec<Candidate> {
    let m = module_from_value(&decode(HARKEN).expect("harken.ark decodes")).expect("harken.ark is a module");
    let m = ark::verify::verify(&m).expect("harken.ark verifies");
    let only: BTreeSet<String> = ["library", "playlist"].into_iter().map(String::from).collect();
    let mut out = vec![];
    for k in 0..HARKEN_SESSIONS {
        let seed = HARKEN_SEED + k;
        let mut tally = fuzz::session::Tally::default();
        let (found, _, traces) = fuzz::session::run_traced(&m, &[], seed, &[], &mut tally, Some(Some(only.clone())));
        claim(&format!("harken session {seed} finds nothing"), found.is_none());
        for (i, t) in traces {
            let from = format!("a session over harken.ark, its library and playlist views kept (ark-vectors, session seed {seed}), client {i}");
            let text = ark::trace::write(&m, &from, &t.steps, &[]);
            out.push(Candidate {
                from,
                module: m.clone(),
                steps: t.steps,
                text,
            });
        }
    }
    out
}

// The smallest candidate a predicate holds of, the first by name on a tie —
// one not already published for another feature where there is one, so
// that a feature adds a trace rather than a second name for one.
fn smallest<'a>(cs: &'a [Candidate], p: &dyn Fn(&Read) -> bool, taken: &BTreeSet<String>) -> Option<&'a Candidate> {
    let mut have: Vec<&Candidate> = cs.iter().filter(|c| p(&Read::of(c))).collect();
    have.sort_by(|a, b| a.text.len().cmp(&b.text.len()).then(a.from.cmp(&b.from)));
    have.iter().find(|c| !taken.contains(&c.from)).or(have.first()).copied()
}

fn publish(out: &Out, name: &str, c: &Candidate) {
    let replayed = ark::trace::replay(&c.module, &c.steps);
    claim(&format!("trace/{name}.json ({}) replays: {replayed:?}", c.from), replayed.is_ok());
    out.write(&format!("trace/{name}.json"), &c.text);
}

pub fn traces(out: &Out) {
    out.dir("trace/");
    let cs = fuzzed(SEED, CASES);
    let mut taken = BTreeSet::new();
    for (name, p) in features() {
        let c = smallest(&cs, &*p, &taken).unwrap_or_else(|| panic!("no trace of seeds {SEED}..{} has {name}", SEED + CASES));
        taken.insert(c.from.clone());
        publish(out, name, c);
    }
    let hs = harken();
    let library = |r: &Read| r.patched("library");
    let playlist = |r: &Read| r.patched("playlist");
    let both = |r: &Read| r.patched("library") && r.patched("playlist") && r.rebased();
    for (name, p) in [
        ("harken-library", &library as &dyn Fn(&Read) -> bool),
        ("harken-playlist", &playlist),
        ("harken-rebase", &both),
    ] {
        let c = smallest(&hs, p, &taken).unwrap_or_else(|| panic!("no harken session has {name}"));
        taken.insert(c.from.clone());
        publish(out, name, c);
    }

    // The deliberately wrong one: the plain sync with the last byte of the
    // first frame it sent changed, which a runner must fail.
    let plain = smallest(&cs, &*features()[0].1, &BTreeSet::new()).expect("a plain sync");
    let mut steps = plain.steps.clone();
    let (_, o) = steps
        .iter_mut()
        .find(|(_, o)| o.as_struct().contains_key("frames"))
        .expect("a step that sent a frame");
    let mut m = o.as_struct().clone();
    let mut frames = m["frames"].as_list();
    let mut b = frames[0].as_bytes().to_vec();
    *b.last_mut().expect("a frame of some bytes") ^= 1;
    frames[0] = Value::bytes(b);
    m.insert("frames".into(), Value::from(frames));
    *o = Value::from(m);
    claim("trace/falsify fails to replay", ark::trace::replay(&plain.module, &steps).is_err());
    let text = ark::trace::write(&plain.module, &plain.from, &steps, &[("expect", "\"fail\"".into())]);
    out.write("trace/falsify/plain-sync-a-frame-byte-changed.json", &text);
}
