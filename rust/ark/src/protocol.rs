//! §12 The protocol, as `Ark.Protocol` defines it.
//!
//! The frames two peers exchange, as values (so that [`crate::canon`] is
//! their wire form), and the two state machines around them: a [`Client`]
//! holding a replica of the log, and a [`Server`] holding its authority,
//! the connections it has identified, and the live rooms. Both are sans-io.
//!
//! Identity is asked once, at `Hello`; every pushed entry is held to it (or
//! to an older login of the same user, where [`Server::with_owns`] says
//! so). The authority applies before it appends, so a `Reject` is a
//! verdict, and it carries a sentence a person can read ([`refusal_text`]).
//! After every message the server sends every connection every entry above
//! what it has been sent, a page at a time. A page carries facts for a peer
//! that asked to be fed by facts, and not for one that replays.

use std::collections::{BTreeMap, BTreeSet};

use crate::eval::{Args, Ctx};
use crate::hash::{Closure, FnHash};
use crate::ir::decode::{closure_from_value, DecodeError};
use crate::ir::encode::closure_value;
use crate::live::{self, ConnId, Machine, Rooms};
use crate::log::{snapshot_of, widened, Entry, Facts, Page, Seq};
use crate::peer::{Authority, Replica, Sequenced};
use crate::schema::Schema;
use crate::scope::{Holdings, Scopes, Who};
use crate::store::{project_row, Change, MemoryStore, Overlay, Refusal, Row, Store};
use crate::value::{hex, FieldName, Id, TableName, Value};

// ---------------------------------------------------------------------
// Frames

/// How a client holds the log: `Whole` replays intents and is exact;
/// `ByFacts` is fed the facts of every entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Whole,
    ByFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    /// The cursor: the last sequence applied.
    pub since: Seq,
    pub mode: Mode,
    /// Which log `since` is a sequence of (§10), as the peer last heard it
    /// named; `None` from a peer that has not been told, or one older than
    /// logs having names, which is served as before (§12.4). On the wire,
    /// `log`, an id, absent for `None`.
    pub log_id: Option<Id>,
    /// `docs/plan-guards.md` D2 The peer holds a union of the module's
    /// scopes — what its person was last served — and not the log whole. A
    /// server that finds this identity whole answers with its snapshot at
    /// the head rather than paging intents onto a store missing rows; one
    /// that finds it partial starts it from a snapshot of what it holds
    /// whatever this says. On the wire `partial: true`, absent otherwise,
    /// so a `hello` from a whole peer is the bytes it was.
    pub partial: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMsg {
    Hello {
        sub: Subscription,
        token: Option<String>,
        spec: i64,
    },
    Push {
        entries: Vec<Entry>,
    },
    NeedFacts {
        seqs: Vec<Seq>,
    },
    NeedClosures {
        hashes: Vec<FnHash>,
    },
    /// Whether the authority holds the state `hash` at `seq`.
    Verify {
        seq: Seq,
        hash: Vec<u8>,
        /// The log `seq` is a sequence of, as the client last heard it named
        /// (`docs/plan-db.md` D2): `log` on the wire, an id, absent for
        /// `None` — a client that has never heard a server name its log,
        /// or a peer alone. An authority on another log answers `unknown`
        /// rather than comparing a state of one log with a state of
        /// another. Additive, as the ack's `log` was: a `verify` naming
        /// none is the bytes it was and is compared as it always was, so a
        /// client older than the field is answered as before.
        log_id: Option<Id>,
        /// `docs/plan-guards.md` D2 The hash is of what a partial replica
        /// holds: a connection served the other kind answers `unknown`. On
        /// the wire `partial: true`, absent otherwise.
        partial: bool,
    },
    Say {
        frame: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerMsg {
    Batch {
        items: Vec<(Seq, Entry, Option<Facts>)>,
        has_more: bool,
        /// The log these entries are of: how a peer that never knew —
        /// opened from storage written before logs had names, or new —
        /// learns it from the first page it is sent. `log` on the wire.
        log_id: Option<Id>,
        /// The hash of the module the server runs (`docs/plan-db.md` D1):
        /// how a peer learns that the facts it is fed are of another
        /// schema than its own, and that it is `behind`. `module` on the
        /// wire, bytes, absent for `None` — so a frame from a server that
        /// does not say is the bytes it was, and a peer that does not read
        /// it ignores a field it does not know.
        module: Option<Vec<u8>>,
        /// `docs/plan-guards.md` D2 A page for a peer that is not whole:
        /// the sequences it covers, `after` its last and through `upto`,
        /// every one of which has passed whether or not an item names it.
        /// Its items are the entries with a fact the peer holds, or its
        /// own, each with those facts only ([`crate::scope::Holdings::filter_facts`]),
        /// and another person's intent carried as its envelope — id,
        /// author, function, its arguments and autos empty — since a
        /// partial peer applies facts and never replays an intent. `after`
        /// and `upto` on the wire, both absent on a page for a whole peer,
        /// which is the bytes it was.
        covers: Option<Covers>,
    },
    FactsFor {
        items: Vec<(Seq, Facts)>,
    },
    SnapshotOf {
        seq: Seq,
        hash: Vec<u8>,
        rows: BTreeMap<TableName, Vec<Value>>,
        /// The log this is a state of; the peer re-opened from it holds
        /// that log from here. `log` on the wire.
        log_id: Option<Id>,
        /// As a batch's: the server's module hash, `module` on the wire.
        module: Option<Vec<u8>>,
        /// `docs/plan-guards.md` D2 The rows are what a peer that is not
        /// whole holds of the state at `seq`, projected, and `hash` their
        /// state hash: the peer holds this from here, and is paged with
        /// `after` and `upto`. The map is every table held in part, with the
        /// columns held of it — the device's schema is the module's with
        /// those tables narrowed to those columns, and a reference kept
        /// only to a table held whole. On the wire `partial: true`, and
        /// `held: {table: [column…]}`, both absent for a whole snapshot.
        held: Option<BTreeMap<TableName, Vec<FieldName>>>,
    },
    Ack {
        ids: Vec<Id>,
        seqs: Vec<Seq>,
        /// The log the sequences are of, as a page names it: `log` on the
        /// wire, absent for `None`. A peer confirmed by an ack alone — the
        /// page that would have named the log lost — learns it here, so
        /// that its next `hello` names it and a server that has since lost
        /// that log answers with its own snapshot rather than paging on
        /// from a cursor in another log (`arkc fuzz`,
        /// `rebase/fleet-fuzz-an-ack-names-no-log.json`).
        log_id: Option<Id>,
        /// The facts of each acknowledged entry the authority stamped with
        /// other roles than its device froze in it, and whose closure reads
        /// a role beyond a guard's refusal (`docs/plan-guards.md` D1;
        /// [`crate::hash::reads_roles`]), by sequence: the device's own run
        /// of it was a preview under a belief the log does not hold, so it
        /// confirms by these and not by its record. In the acknowledgement
        /// and not a frame of their own, so they arrive with the log they
        /// are of — facts alone, for a sequence of a log a peer has not been
        /// told the name of, would wait in its inbox for whichever entry the
        /// next log puts there (`arkc fuzz` found it, seed 112; held by
        /// `tests/roles.rs`'s `the_stamps_facts_arrive_with_the_log_they_are_of`).
        /// `facts` on the wire, a list of `{seq, facts}`, present only when
        /// not empty: every other acknowledgement is the bytes it was.
        facts: Vec<(Seq, Facts)>,
    },
    Reject {
        id: Id,
        reason: String,
    },
    /// An intent the server cannot run yet: it names a function no module
    /// this server has run shipped (`docs/plan-db.md` D1). Not a verdict —
    /// nothing was refused of what the person did; a server older than the
    /// peer cannot judge it — so the peer keeps it pending and pushes it
    /// again on its next connection, when the server may have been
    /// upgraded. Its own kind, `held`, beside `reject`, so that a `reject`
    /// is the bytes it was and a peer that predates holding, which cannot
    /// read the frame, keeps the intent pending too.
    Held {
        id: Id,
        reason: String,
    },
    Denied {
        reason: String,
    },
    Closures {
        items: Vec<(FnHash, Closure)>,
    },
    /// The answer to a `Verify`. `unknown` is the authority saying it
    /// cannot say — the sequence is below its horizon or past its head, so
    /// it holds no state there to compare (`docs/plan-db.md` D3), or the
    /// `Verify` named another log than its own (D2) — and `ok`
    /// is then false and means nothing. On the wire `unknown` is present
    /// only when true, so the two answers there were before it are the
    /// bytes they were; `unknown: false` is refused as a second spelling.
    Agree {
        seq: Seq,
        hash: Vec<u8>,
        ok: bool,
        unknown: bool,
    },
    Heard {
        frame: Vec<u8>,
    },
}

/// `docs/plan-guards.md` D2 What a partial page covers: the sequences
/// after `after` through `upto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Covers {
    pub after: Seq,
    pub upto: Seq,
}

/// Entries per page.
pub const BATCH_LIMIT: usize = 256;

fn node(t: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut m: BTreeMap<FieldName, Value> = BTreeMap::new();
    m.insert("t".into(), Value::text(t));
    for (k, v) in fields {
        m.insert(k.into(), v);
    }
    Value::from(m)
}

fn txt(s: &str) -> Value {
    Value::text(s)
}

fn int(n: i64) -> Value {
    Value::Int(n)
}

fn strct(pairs: Vec<(&str, Value)>) -> Value {
    Value::Struct(Box::new(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
}

// A log's identity on the wire: `log`, an id, and absent where none is
// known — so a frame that names no log is the bytes it was before logs had
// names, and a runtime that never names one is conformant as it was
// (Round 4).
fn named<'a>(mut fields: Vec<(&'a str, Value)>, id: &Option<Id>) -> Vec<(&'a str, Value)> {
    if let Some(i) = id {
        fields.push(("log", Value::Id(*i)));
    }
    fields
}

// The server's module hash on the wire: `module`, bytes, absent where the
// server does not say — the encoding `log` has, for the reason it has it
// (`docs/plan-db.md` D1).
fn of_module<'a>(mut fields: Vec<(&'a str, Value)>, module: &Option<Vec<u8>>) -> Vec<(&'a str, Value)> {
    if let Some(m) = module {
        fields.push(("module", Value::Bytes(m[..].into())));
    }
    fields
}

/// An entry as a value: its wire form in a `push` or a `batch`, and its
/// form on disk (`crate::journal`, a client's pending). `roles` is present
/// only when the entry holds one (`docs/plan-guards.md` D1): a list of
/// texts in ascending order, so an entry authored by somebody holding none
/// — every entry written before roles were frozen — is the bytes it was.
pub fn entry_value(e: &Entry) -> Value {
    let mut fields = vec![
        ("id", Value::Id(e.id)),
        ("actor", txt(&e.actor)),
        ("session", txt(&e.session)),
        ("fn", Value::Bytes(e.fn_hash[..].into())),
        ("args", Value::from(e.args.clone())),
        ("autos", Value::from(e.autos.clone())),
    ];
    if !e.roles.is_empty() {
        fields.push(("roles", Value::List(e.roles.iter().map(|r| txt(r)).collect())));
    }
    strct(fields)
}

pub fn change_value(c: &Change) -> Value {
    match c {
        Change::Add(t, r) => node("add", vec![("table", txt(t)), ("row", r.to_value())]),
        Change::Remove(t, r) => node("remove", vec![("table", txt(t)), ("row", r.to_value())]),
        Change::Edit(t, o, n) => node("edit", vec![("table", txt(t)), ("old", o.to_value()), ("new", n.to_value())]),
    }
}

fn facts_value(f: &Facts) -> Value {
    Value::List(f.iter().map(change_value).collect())
}

// Facts by sequence, as a `facts` frame's `items` and an ack's `facts` are
// written: a list of `{seq, facts}`.
fn seq_facts_value(items: &[(Seq, Facts)]) -> Value {
    Value::List(
        items
            .iter()
            .map(|(n, f)| strct(vec![("seq", int(*n)), ("facts", facts_value(f))]))
            .collect(),
    )
}

impl ClientMsg {
    /// The frame as a value; its wire form is `canon::encode` of this.
    pub fn to_value(&self) -> Value {
        match self {
            ClientMsg::Hello { sub, token, spec } => node(
                "hello",
                flagged(
                    named(
                        vec![
                            ("since", int(sub.since)),
                            ("mode", txt(if sub.mode == Mode::Whole { "whole" } else { "facts" })),
                            ("token", token.as_deref().map(txt).unwrap_or(Value::Null)),
                            ("spec", int(*spec)),
                        ],
                        &sub.log_id,
                    ),
                    "partial",
                    sub.partial,
                ),
            ),
            ClientMsg::Push { entries } => node("push", vec![("entries", Value::List(entries.iter().map(entry_value).collect()))]),
            ClientMsg::NeedFacts { seqs } => node("need_facts", vec![("seqs", Value::List(seqs.iter().map(|n| int(*n)).collect()))]),
            ClientMsg::NeedClosures { hashes } => node(
                "need_closures",
                vec![("hashes", Value::List(hashes.iter().map(|h| Value::Bytes(h[..].into())).collect()))],
            ),
            ClientMsg::Verify { seq, hash, log_id, partial } => node(
                "verify",
                flagged(
                    named(vec![("seq", int(*seq)), ("hash", Value::Bytes(hash[..].into()))], log_id),
                    "partial",
                    *partial,
                ),
            ),
            ClientMsg::Say { frame } => node("say", vec![("say", Value::Bytes(frame[..].into()))]),
        }
    }

    /// A frame from its value (`Ark.Protocol.clientFromValue`).
    pub fn from_value(v: &Value) -> Result<ClientMsg, DecodeError> {
        let m = strct_of(v)?;
        let t = text(need(m, "t")?)?;
        Ok(match t.as_str() {
            "hello" => ClientMsg::Hello {
                sub: sub(m)?,
                token: match need(m, "token")? {
                    Value::Null => None,
                    x => Some(text(x)?),
                },
                spec: int64(need(m, "spec")?)?,
            },
            "push" => ClientMsg::Push {
                entries: list(entry_from_value, need(m, "entries")?)?,
            },
            "need_facts" => ClientMsg::NeedFacts {
                seqs: list(int64, need(m, "seqs")?)?,
            },
            "need_closures" => ClientMsg::NeedClosures {
                hashes: list(bytes, need(m, "hashes")?)?,
            },
            "verify" => ClientMsg::Verify {
                seq: int64(need(m, "seq")?)?,
                hash: bytes(need(m, "hash")?)?,
                log_id: log_of(m)?,
                partial: flag(m, "partial")?,
            },
            "say" => ClientMsg::Say {
                frame: bytes(need(m, "say")?)?,
            },
            other => return bad(format!("unknown client frame {other}")),
        })
    }
}

fn sub(m: &BTreeMap<FieldName, Value>) -> Result<Subscription, DecodeError> {
    let mode = match text(need(m, "mode")?)?.as_str() {
        "whole" => Mode::Whole,
        "facts" => Mode::ByFacts,
        other => return bad(format!("unknown mode {other}")),
    };
    Ok(Subscription {
        since: int64(need(m, "since")?)?,
        mode,
        log_id: log_of(m)?,
        partial: flag(m, "partial")?,
    })
}

// `docs/plan-guards.md` D2 A flag that is `true` when present: absent is the
// one encoding of `false`, so a frame without it is the bytes it was, and
// `false` is refused as a second spelling.
fn flagged<'a>(mut fields: Vec<(&'a str, Value)>, name: &'a str, on: bool) -> Vec<(&'a str, Value)> {
    if on {
        fields.push((name, Value::Bool(true)));
    }
    fields
}

fn flag(m: &BTreeMap<FieldName, Value>, name: &str) -> Result<bool, DecodeError> {
    match m.get(name) {
        None => Ok(false),
        Some(Value::Bool(true)) => Ok(true),
        Some(_) => bad(format!("{name} is true or absent")),
    }
}

// `docs/plan-guards.md` D2 What a partial page covers: `after` and `upto`,
// both absent for a whole peer's page.
fn covers_fields<'a>(mut fields: Vec<(&'a str, Value)>, covers: &Option<Covers>) -> Vec<(&'a str, Value)> {
    if let Some(c) = covers {
        fields.push(("after", Value::Int(c.after)));
        fields.push(("upto", Value::Int(c.upto)));
    }
    fields
}

fn covers_of(m: &BTreeMap<FieldName, Value>) -> Result<Option<Covers>, DecodeError> {
    match (m.get("after"), m.get("upto")) {
        (None, None) => Ok(None),
        (Some(a), Some(u)) => Ok(Some(Covers {
            after: int64(a)?,
            upto: int64(u)?,
        })),
        _ => bad("a page's after and upto go together"),
    }
}

// `docs/plan-guards.md` D2 A partial snapshot's `partial: true` and `held`,
// both absent for a whole one.
fn held_fields<'a>(mut fields: Vec<(&'a str, Value)>, held: &Option<BTreeMap<TableName, Vec<FieldName>>>) -> Vec<(&'a str, Value)> {
    if let Some(h) = held {
        fields.push(("partial", Value::Bool(true)));
        fields.push((
            "held",
            Value::Struct(Box::new(
                h.iter()
                    .map(|(t, cs)| (t.clone(), Value::List(cs.iter().map(|c| txt(c)).collect())))
                    .collect(),
            )),
        ));
    }
    fields
}

fn held_of(m: &BTreeMap<FieldName, Value>) -> Result<Option<BTreeMap<TableName, Vec<FieldName>>>, DecodeError> {
    match (flag(m, "partial")?, m.get("held")) {
        (false, None) => Ok(None),
        (true, Some(h)) => {
            let mut out = BTreeMap::new();
            for (t, cs) in strct_of(h)? {
                out.insert(t.clone(), list(text, cs)?);
            }
            Ok(Some(out))
        }
        _ => bad("a snapshot's partial and held go together"),
    }
}

/// The `log` of a frame that may carry one: an id, or `None` where the
/// field is absent — a frame that names no log, which is also every frame
/// from a runtime older than logs having names (§12.4), served and serving
/// as it was. A null is refused: absence is the one encoding of "no log",
/// so that a frame has one form.
fn log_of(m: &BTreeMap<FieldName, Value>) -> Result<Option<Id>, DecodeError> {
    m.get("log").map(ident).transpose()
}

/// The `module` of a page or a snapshot, as `log` is read: bytes, or
/// `None` where the field is absent; a null is refused.
fn module_of(m: &BTreeMap<FieldName, Value>) -> Result<Option<Vec<u8>>, DecodeError> {
    m.get("module").map(bytes).transpose()
}

impl ServerMsg {
    /// The frame as a value.
    pub fn to_value(&self) -> Value {
        match self {
            ServerMsg::Batch {
                items,
                has_more,
                log_id,
                module,
                covers,
            } => node(
                "batch",
                covers_fields(
                    of_module(
                        named(
                            vec![
                                (
                                    "items",
                                    Value::List(
                                        items
                                            .iter()
                                            .map(|(n, e, f)| {
                                                strct(vec![
                                                    ("seq", int(*n)),
                                                    ("entry", entry_value(e)),
                                                    ("facts", f.as_ref().map(facts_value).unwrap_or(Value::Null)),
                                                ])
                                            })
                                            .collect(),
                                    ),
                                ),
                                ("has_more", Value::Bool(*has_more)),
                            ],
                            log_id,
                        ),
                        module,
                    ),
                    covers,
                ),
            ),
            ServerMsg::FactsFor { items } => node("facts", vec![("items", seq_facts_value(items))]),
            ServerMsg::SnapshotOf {
                seq,
                hash,
                rows,
                log_id,
                module,
                held,
            } => node(
                "snapshot",
                held_fields(
                    of_module(
                        named(
                            vec![
                                ("seq", int(*seq)),
                                ("hash", Value::Bytes(hash[..].into())),
                                (
                                    "rows",
                                    Value::Struct(Box::new(rows.iter().map(|(t, vs)| (t.clone(), Value::List(vs[..].into()))).collect())),
                                ),
                            ],
                            log_id,
                        ),
                        module,
                    ),
                    held,
                ),
            ),
            ServerMsg::Ack { ids, seqs, log_id, facts } => {
                let mut fields = named(
                    vec![
                        ("ids", Value::List(ids.iter().map(|i| Value::Id(*i)).collect())),
                        ("seqs", Value::List(seqs.iter().map(|n| int(*n)).collect())),
                    ],
                    log_id,
                );
                if !facts.is_empty() {
                    fields.push(("facts", seq_facts_value(facts)));
                }
                node("ack", fields)
            }
            ServerMsg::Reject { id, reason } => node("reject", vec![("id", Value::Id(*id)), ("reason", txt(reason))]),
            ServerMsg::Held { id, reason } => node("held", vec![("id", Value::Id(*id)), ("reason", txt(reason))]),
            ServerMsg::Denied { reason } => node("denied", vec![("reason", txt(reason))]),
            ServerMsg::Closures { items } => node(
                "closures",
                vec![(
                    "items",
                    Value::List(
                        items
                            .iter()
                            .map(|(h, c)| strct(vec![("hash", Value::Bytes(h[..].into())), ("closure", closure_value(c))]))
                            .collect(),
                    ),
                )],
            ),
            ServerMsg::Agree { seq, hash, ok, unknown } => {
                let mut fields = vec![("seq", int(*seq)), ("hash", Value::Bytes(hash[..].into())), ("ok", Value::Bool(*ok))];
                if *unknown {
                    fields.push(("unknown", Value::Bool(true)));
                }
                node("agree", fields)
            }
            ServerMsg::Heard { frame } => node("heard", vec![("hear", Value::Bytes(frame[..].into()))]),
        }
    }

    /// A frame from its value (`Ark.Protocol.serverFromValue`).
    pub fn from_value(v: &Value) -> Result<ServerMsg, DecodeError> {
        let m = strct_of(v)?;
        let t = text(need(m, "t")?)?;
        Ok(match t.as_str() {
            "batch" => ServerMsg::Batch {
                items: list(
                    |x| {
                        let m = strct_of(x)?;
                        let facts = match need(m, "facts")? {
                            Value::Null => None,
                            f => Some(list(change_from_value, f)?),
                        };
                        Ok((int64(need(m, "seq")?)?, entry_from_value(need(m, "entry")?)?, facts))
                    },
                    need(m, "items")?,
                )?,
                has_more: boolean(need(m, "has_more")?)?,
                log_id: log_of(m)?,
                module: module_of(m)?,
                covers: covers_of(m)?,
            },
            "facts" => ServerMsg::FactsFor {
                items: seq_facts_of(need(m, "items")?)?,
            },
            "snapshot" => ServerMsg::SnapshotOf {
                seq: int64(need(m, "seq")?)?,
                hash: bytes(need(m, "hash")?)?,
                rows: {
                    let mut out = BTreeMap::new();
                    for (t, vs) in strct_of(need(m, "rows")?)? {
                        out.insert(t.clone(), list(|v| Ok(v.clone()), vs)?);
                    }
                    out
                },
                log_id: log_of(m)?,
                module: module_of(m)?,
                held: held_of(m)?,
            },
            "ack" => ServerMsg::Ack {
                ids: list(ident, need(m, "ids")?)?,
                seqs: list(int64, need(m, "seqs")?)?,
                log_id: log_of(m)?,
                facts: match m.get("facts") {
                    None => vec![],
                    Some(v) => match seq_facts_of(v)? {
                        fs if fs.is_empty() => return bad("ack: empty facts are written as no field"),
                        fs => fs,
                    },
                },
            },
            "reject" => ServerMsg::Reject {
                id: ident(need(m, "id")?)?,
                reason: text(need(m, "reason")?)?,
            },
            "held" => ServerMsg::Held {
                id: ident(need(m, "id")?)?,
                reason: text(need(m, "reason")?)?,
            },
            "denied" => ServerMsg::Denied {
                reason: text(need(m, "reason")?)?,
            },
            "closures" => ServerMsg::Closures {
                items: list(
                    |x| {
                        let m = strct_of(x)?;
                        Ok((bytes(need(m, "hash")?)?, closure_from_value(need(m, "closure")?)?))
                    },
                    need(m, "items")?,
                )?,
            },
            "agree" => ServerMsg::Agree {
                seq: int64(need(m, "seq")?)?,
                hash: bytes(need(m, "hash")?)?,
                ok: boolean(need(m, "ok")?)?,
                unknown: match m.get("unknown") {
                    None => false,
                    Some(Value::Bool(true)) => true,
                    Some(_) => return bad("an agree's unknown is true or absent"),
                },
            },
            "heard" => ServerMsg::Heard {
                frame: bytes(need(m, "hear")?)?,
            },
            other => return bad(format!("unknown server frame {other}")),
        })
    }
}

/// `docs/plan-db.md` D7.4 What a `snapshot` frame brings a client: the
/// state at `seq` as its rows, each already a [`Row`] — laid out as its
/// table's where its fields are exactly the table's columns, and otherwise
/// as it came, a row of no table — and what is not a struct gone, as
/// [`Client::recv_snapshot`] has always dropped it. The hash is not here:
/// a client re-opened from a snapshot does not check it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub seq: Seq,
    pub rows: BTreeMap<TableName, Vec<Row>>,
    pub log_id: Option<Id>,
    pub module: Option<Vec<u8>>,
    /// `docs/plan-guards.md` D2 The rows are what a partial peer holds,
    /// and these are the tables held in part with their columns.
    pub held: Option<BTreeMap<TableName, Vec<FieldName>>>,
}

impl Snapshot {
    /// A `SnapshotOf`'s rows read as a client of `schema` reads them: each
    /// struct [`Row::stored_in`] its table, or as it came where the schema
    /// has no such table; each element that is not a struct dropped, and a
    /// table left with no rows not named — as [`ServerMsg::decode_for`],
    /// which is handed rows and not tables, never names one.
    pub fn of_values(
        schema: &Schema,
        seq: Seq,
        rows: BTreeMap<TableName, Vec<Value>>,
        log_id: Option<Id>,
        module: Option<Vec<u8>>,
        held: Option<BTreeMap<TableName, Vec<FieldName>>>,
    ) -> Snapshot {
        let rows = rows
            .into_iter()
            .map(|(t, vs)| {
                let tbl = schema.lookup_table(&t);
                let rs = vs
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::Struct(m) => Some(match tbl {
                            Some(tbl) => Row::stored_in(tbl, &m),
                            None => Row::from_struct(*m),
                        }),
                        _ => None,
                    })
                    .collect();
                (t, rs)
            })
            .filter(|(_, rs): &(TableName, Vec<Row>)| !rs.is_empty())
            .collect();
        Snapshot {
            seq,
            rows,
            log_id,
            module,
            held,
        }
    }
}

/// A server frame as a client of a known schema reads it
/// ([`ServerMsg::decode_for`]): a snapshot with its rows built, or any
/// other frame as [`ServerMsg::from_value`] reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Received {
    Msg(ServerMsg),
    Snapshot(Snapshot),
}

impl ServerMsg {
    /// `docs/plan-db.md` D7.4 A frame from its bytes, for a client of
    /// `schema`: the one way a client reads the socket. Every frame is
    /// decoded with its `rows` — which only a `snapshot` has — read as a
    /// store's rows ([`crate::canon::decode_rows`]), each built into a
    /// [`Row`] as its fields are read, so the tree of a struct per row that
    /// [`crate::canon::decode`] would build, and adoption free, is never
    /// made: a snapshot of a whole store is the largest frame there is, and
    /// that tree was its high-water mark. What is left of the frame — the
    /// rest of it, and under `rows` only what is not a list of structs — is
    /// read by [`ServerMsg::from_value`], so exactly the frames refused
    /// before are refused, with the same words: `canon`'s for bytes that
    /// are not canonical, `from_value`'s for a value that is not a frame.
    /// A frame that is not a snapshot comes back as `from_value` reads it;
    /// one that carries `rows` all the same has them read and dropped, as
    /// `from_value` ignores them.
    pub fn decode_for(bytes: &[u8], schema: &Schema) -> Result<Received, String> {
        let mut rows: BTreeMap<TableName, Vec<Row>> = BTreeMap::new();
        let v = crate::canon::decode_rows(bytes, &["rows"], &mut |t, fields| {
            let row = Row::from_fields(schema.lookup_table(t), fields);
            match rows.get_mut(t) {
                Some(rs) => rs.push(row),
                None => {
                    rows.insert(t.to_string(), vec![row]);
                }
            }
        })
        .map_err(|e| e.to_string())?;
        Ok(match ServerMsg::from_value(&v).map_err(|e| e.to_string())? {
            // What is left under `rows` is only what is not a struct, which
            // adoption drops.
            ServerMsg::SnapshotOf {
                seq, log_id, module, held, ..
            } => Received::Snapshot(Snapshot {
                seq,
                rows,
                log_id,
                module,
                held,
            }),
            msg => Received::Msg(msg),
        })
    }
}

// Decoding primitives ------------------------------------------------------

type D<T> = Result<T, DecodeError>;

fn bad<T>(what: impl Into<String>) -> D<T> {
    Err(DecodeError {
        path: vec!["frame".into()],
        what: what.into(),
    })
}

fn strct_of(v: &Value) -> D<&BTreeMap<FieldName, Value>> {
    match v {
        Value::Struct(m) => Ok(m),
        _ => bad("expected a struct"),
    }
}

fn need<'a>(m: &'a BTreeMap<FieldName, Value>, k: &str) -> D<&'a Value> {
    m.get(k).map_or_else(|| bad(format!("missing {k}")), Ok)
}

fn text(v: &Value) -> D<String> {
    match v {
        Value::Text(t) => Ok(t.to_string()),
        _ => bad("expected text"),
    }
}

fn bytes(v: &Value) -> D<Vec<u8>> {
    match v {
        Value::Bytes(b) => Ok(b.to_vec()),
        _ => bad("expected bytes"),
    }
}

fn int64(v: &Value) -> D<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        _ => bad("expected an int"),
    }
}

fn boolean(v: &Value) -> D<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => bad("expected a bool"),
    }
}

fn ident(v: &Value) -> D<Id> {
    match v {
        Value::Id(i) => Ok(*i),
        _ => bad("expected an id"),
    }
}

fn list<T>(f: impl Fn(&Value) -> D<T>, v: &Value) -> D<Vec<T>> {
    match v {
        Value::List(xs) => xs.iter().map(f).collect(),
        _ => bad("expected a list"),
    }
}

fn args_of(v: &Value) -> D<Args> {
    strct_of(v).cloned()
}

// A change's row, as the struct it crossed as: a row of no table until the
// store it is applied to lays it out as its table's (`store.rs`'s module
// docs), its values copied and its names shared with its shape's.
fn row_of(v: &Value) -> D<Row> {
    strct_of(v).map(Row::from_struct_ref)
}

pub fn entry_from_value(v: &Value) -> D<Entry> {
    let m = strct_of(v)?;
    Ok(Entry {
        id: ident(need(m, "id")?)?,
        actor: text(need(m, "actor")?)?,
        session: text(need(m, "session")?)?,
        roles: roles_of(m)?,
        fn_hash: bytes(need(m, "fn")?)?,
        args: args_of(need(m, "args")?)?,
        autos: args_of(need(m, "autos")?)?,
    })
}

/// An entry's `roles` (`docs/plan-guards.md` D1): none where the field is
/// absent, which is the one encoding of none — an empty list is refused —
/// and otherwise texts in strictly ascending order, refused in any other,
/// so that an entry has one form and decoding then encoding it is the
/// bytes it came as.
fn roles_of(m: &BTreeMap<FieldName, Value>) -> D<BTreeSet<String>> {
    let Some(v) = m.get("roles") else {
        return Ok(BTreeSet::new());
    };
    let rs = list(text, v)?;
    if rs.is_empty() {
        return bad("roles: an empty list is written as no field");
    }
    if rs.windows(2).any(|w| w[0] >= w[1]) {
        return bad("roles: not in ascending order, or repeated");
    }
    Ok(rs.into_iter().collect())
}

fn seq_facts_of(v: &Value) -> D<Vec<(Seq, Facts)>> {
    list(
        |x| {
            let m = strct_of(x)?;
            Ok((int64(need(m, "seq")?)?, list(change_from_value, need(m, "facts")?)?))
        },
        v,
    )
}

pub fn change_from_value(v: &Value) -> D<Change> {
    let m = strct_of(v)?;
    let t = text(need(m, "t")?)?;
    let tbl = text(need(m, "table")?)?;
    Ok(match t.as_str() {
        "add" => Change::Add(tbl, row_of(need(m, "row")?)?),
        "remove" => Change::Remove(tbl, row_of(need(m, "row")?)?),
        "edit" => Change::Edit(tbl, row_of(need(m, "old")?)?, row_of(need(m, "new")?)?),
        other => return bad(format!("unknown change {other}")),
    })
}

// ---------------------------------------------------------------------
// The client

/// A peer's end of one connection: its replica of the log, and what it has
/// queued (`Ark.Protocol.Client`).
#[derive(Clone, Debug)]
pub struct Client {
    pub schema: Schema,
    pub replica: Replica,
    pub mode: Mode,
    pub token: Option<String>,
    pub linked: bool,
    /// Counts connections, so a live room that has never heard of this
    /// device can be told apart from one that has.
    pub epoch: i64,
    /// Oldest first.
    pub out: Vec<ClientMsg>,
    /// Oldest first.
    pub heard: Vec<Vec<u8>>,
    pub denied: Option<String>,
    /// Every answer to a `Verify`, oldest first: `Some(agreed)`, or `None`
    /// where the authority could not say (`ServerMsg::Agree::unknown`).
    pub agreed: Vec<(Seq, Option<bool>)>,
    /// A page arrived since the last [`Client::settle`]: the facts it
    /// leaves the replica waiting on are asked for there, once the inbox
    /// has been applied (R8).
    pub paged: bool,
    /// A page said there is more: the `Hello` that asks for it is said at
    /// the settle, at the cursor the page moved the replica to. Said at
    /// the frame, before the page is applied, it would name the old cursor
    /// and be sent the same page again.
    pub more: bool,
    /// The hash of this peer's own module, where the caller says it
    /// (`ark_client::Peer` does): what the server's is compared with.
    pub module: Option<Vec<u8>>,
    /// The server's module hash, as its last page or snapshot said it
    /// (`docs/plan-db.md` D1).
    pub server_module: Option<Vec<u8>>,
    /// Pending intents the server answered `held`: it cannot run them yet
    /// (`docs/plan-db.md` D1). They stay pending and are pushed again on
    /// the next connection; an id leaves this set when the server answers
    /// it otherwise. [`Client::held`] counts the ones still pending.
    pub held_ids: BTreeSet<Id>,
    /// `docs/plan-guards.md` D2 This connection has been sent a page or a
    /// snapshot: a partial replica says no `verify` before, since what it
    /// holds may be another identity's until the server has started it
    /// over.
    pub served: bool,
    /// This connection has brought a snapshot. Every partial connection
    /// starts from one, so a partial page before it is a page over a
    /// snapshot that was lost, for a union this replica may not be laid out
    /// for: it is dropped, and the peer asks again, which a server answers
    /// by starting it over (D2).
    pub started: bool,
}

impl Client {
    /// A client over the replica as opened from what was durable
    /// (`Ark.Protocol.openClient`).
    pub fn open(replica: Replica, mode: Mode, token: Option<String>) -> Client {
        Client {
            schema: replica.schema.clone(),
            replica,
            mode,
            token,
            linked: false,
            epoch: 0,
            out: vec![],
            heard: vec![],
            denied: None,
            agreed: vec![],
            paged: false,
            more: false,
            module: None,
            server_module: None,
            held_ids: BTreeSet::new(),
            served: false,
            started: false,
        }
    }

    /// `docs/plan-guards.md` D2 The module's schema, for a client opened
    /// over a replica that holds a union and is laid out under the
    /// device's: what every later snapshot's device schema is made from.
    pub fn with_schema(mut self, module: Schema) -> Client {
        self.schema = module;
        self
    }

    /// §12 `behind` (`docs/plan-db.md` D1): both module hashes are known
    /// and they differ. The schema the server's facts are of is then not
    /// this peer's — narrower or wider, a hash cannot say which — so its
    /// facts are applied projected to this peer's schema
    /// ([`crate::store::project_row`]: columns this schema lacks dropped,
    /// nullable ones it has and the fact lacks `Null`), and no `Verify` is
    /// said: a state hash over one schema compared with one over another
    /// would disagree whatever happened, and mean nothing.
    pub fn behind(&self) -> bool {
        matches!((&self.module, &self.server_module), (Some(ours), Some(theirs)) if ours != theirs)
    }

    /// Pending intents the server has answered `held`, and nothing since.
    pub fn held(&self) -> usize {
        self.replica.pending.iter().filter(|e| self.held_ids.contains(&e.id)).count()
    }

    // The server said which module it runs: the replica projects facts
    // from here on if that is not this peer's.
    fn heard_module(&mut self, module: Option<Vec<u8>>) {
        if module.is_some() {
            self.server_module = module;
        }
        self.replica.behind = self.behind();
    }

    // A pending intent of this peer's that the server cannot run yet.
    fn hold_intent(&mut self, id: Id) {
        if self.replica.pending.iter().any(|e| e.id == id) {
            self.held_ids.insert(id);
        }
    }

    // Unlinked: nothing is queued; `connected` says it all again.
    fn emit(&mut self, m: ClientMsg) {
        if self.linked {
            self.out.push(m);
        }
    }

    /// Somebody signed in (`Ark.Protocol.clientSignIn`): the token every
    /// later `Hello` carries, and every intent authored before anyone had
    /// signed in made theirs ([`Replica::sign_in`]). Call it before
    /// [`Client::connected`]; the first `Hello` after it pushes all of it.
    pub fn sign_in(&mut self, who: &Ctx, token: Option<String>) {
        self.replica.sign_in(who);
        self.token = token;
    }

    fn hello(&self) -> ClientMsg {
        ClientMsg::Hello {
            sub: Subscription {
                since: self.replica.cursor,
                mode: self.mode,
                log_id: self.replica.log_id,
                partial: self.replica.partial.is_some(),
            },
            token: self.token.clone(),
            spec: crate::ir::SPEC_VERSION,
        }
    }

    /// §12.1 A connection opened: say hello at the cursor, then push
    /// everything pending. What was queued before is dropped.
    pub fn connected(&mut self) {
        self.linked = true;
        self.epoch += 1;
        self.out.clear();
        self.more = false;
        self.served = false;
        self.started = false;
        self.heard.clear();
        let hello = self.hello();
        self.emit(hello);
        if !self.replica.pending.is_empty() {
            let entries = self.replica.pending.clone();
            self.emit(ClientMsg::Push { entries });
        }
    }

    pub fn disconnected(&mut self) {
        self.linked = false;
        self.out.clear();
        self.more = false;
        self.heard.clear();
    }

    /// Author an intent and push it if linked. A refusal here is the
    /// optimistic verdict, on the state this peer has; the authority's may
    /// differ, and arrives as a `Reject` with its own reason.
    pub fn mutate(&mut self, id: Id, ctx: &Ctx, fh: &FnHash, autos: &Args, args: &Args) -> Result<Entry, Refusal> {
        let e = self.replica.mutate(id, ctx, fh, autos, args)?;
        self.emit(ClientMsg::Push { entries: vec![e.clone()] });
        Ok(e)
    }

    /// Hold native procedures in the replica.
    pub fn hold(&mut self, procs: &[(FnHash, crate::authoring::Procedure)]) {
        self.replica.hold(procs.iter().cloned());
    }

    /// §12.2 A frame from the server. What it brings of the log — a page,
    /// facts, acknowledgements, closures — is placed in the replica's inbox
    /// and applied by [`Client::settle`], which the driver calls once after
    /// the last frame of a pump (`docs/plan-perf.md` R8). Only the driver
    /// knows where a pump ends: frames reach this one at a time, and
    /// advancing on each cost the K pending intents' re-runs per frame.
    pub fn recv(&mut self, msg: ServerMsg) {
        match msg {
            ServerMsg::Heard { frame } => self.heard.push(frame),
            ServerMsg::Denied { reason } => {
                self.denied = Some(reason);
                self.linked = false;
                self.out.clear();
                self.more = false;
            }
            ServerMsg::Batch {
                items,
                has_more,
                log_id,
                module,
                covers,
            } => {
                self.heard_module(module);
                self.served = true;
                let r = &mut self.replica;
                // `docs/plan-guards.md` D2 A page of the other kind than the
                // replica holds — a partial one for a whole replica, or the
                // reverse — is nothing it can apply: a server starts every
                // connection that changes kind with a snapshot, and this page
                // is from a connection whose snapshot was lost. So is a
                // partial page before this connection's snapshot. Either is
                // dropped and the peer asks again; a `hello` the server did
                // not page for starts it over.
                if covers.is_some() != r.partial.is_some() {
                    self.more = true;
                    return;
                }
                if covers.is_some() && !self.started {
                    self.more = true;
                    return;
                }
                if let Some(c) = covers {
                    // One already covered — the network's duplicate — is
                    // nothing new. One that continues from past where this
                    // peer has been told is a page missed: its items would
                    // be applied over a gap no `upto` could show, so it is
                    // dropped and the settle asks again from the cursor,
                    // which the server pages from.
                    if c.upto <= r.through {
                        return;
                    }
                    if c.after > r.through {
                        self.more = true;
                        return;
                    }
                    r.through = c.upto;
                }
                // A peer that did not know which log it holds learns it
                // from the first page (Round 4). One that did is never sent
                // a page of another: the server answered its `Hello` with a
                // snapshot first.
                if r.log_id.is_none() {
                    r.log_id = log_id;
                }
                for (n, e, f) in items {
                    match f {
                        Some(f) => r.receive_with(n, e, f),
                        None => r.receive(n, e),
                    }
                }
                self.paged = true;
                self.more |= has_more;
            }
            ServerMsg::FactsFor { items } => {
                for (n, f) in items {
                    self.replica.receive_facts(n, f);
                }
            }
            // Below the horizon, past the head, or of another log: the
            // rows read as `decode_for` reads them, then adopted
            // ([`Client::recv_snapshot`]).
            ServerMsg::SnapshotOf {
                seq,
                rows,
                log_id,
                module,
                held,
                ..
            } => {
                let s = Snapshot::of_values(&self.schema, seq, rows, log_id, module, held);
                self.recv_snapshot(s);
            }
            ServerMsg::Ack { ids, seqs, log_id, facts } => {
                // As from a page: a peer that did not know which log it
                // holds learns it from the first frame that says.
                if self.replica.log_id.is_none() {
                    self.replica.log_id = log_id;
                }
                // An entry stamped with other roles than this device froze
                // in it (`docs/plan-guards.md` D1) is confirmed by the
                // authority's facts, which arrive with the log they are of.
                for (n, f) in facts {
                    self.replica.receive_facts(n, f);
                }
                for (i, n) in ids.iter().zip(seqs) {
                    self.held_ids.remove(i);
                    self.replica.ack(i, n);
                }
            }
            // A server from before holding answered an unknown function
            // with a `reject` saying exactly that, of exactly this intent's
            // hash ([`unknown_function`], word for word): read as the hold
            // it was — the same server upgraded in place takes it
            // (`docs/plan-db.md` D1, the fleet's scenario 3). No mutator's
            // own refusal can name the hash of the intent it is refusing,
            // so nothing else reads this way.
            ServerMsg::Reject { id, reason } => {
                let unknown = self.replica.pending.iter().any(|e| e.id == id && reason == unknown_function(&e.fn_hash));
                if unknown {
                    self.hold_intent(id);
                } else {
                    self.held_ids.remove(&id);
                    self.replica.reject(&id, Refusal::Refused(reason));
                }
            }
            // Kept pending; nothing about the view moves (D1).
            ServerMsg::Held { id, .. } => self.hold_intent(id),
            // New closures may unblock entries waiting in the inbox, at
            // the settle. A received closure replaces one already held
            // under its hash (the spec's left-biased union).
            ServerMsg::Closures { items } => {
                for (h, c) in items {
                    self.replica.bodies.insert(h, c);
                }
            }
            ServerMsg::Agree { seq, ok, unknown, .. } => self.agreed.push((seq, (!unknown).then_some(ok))),
        }
    }

    /// §12.2 A snapshot from the server, its rows built: what a
    /// `SnapshotOf` is to [`Client::recv`], and what a transport that reads
    /// frames with [`ServerMsg::decode_for`] hands over in its place
    /// (`docs/plan-db.md` D7.4).
    ///
    /// Below the horizon, past the head (R6), or of another log than the
    /// one this peer held (Round 4): the confirmed store is replaced by the
    /// snapshot, the cursor moves to it, and the peer holds the snapshot's
    /// log from here; pending intents are kept and replay on top. Verdicts
    /// the app has not yet taken are kept too, ahead of any the replay
    /// makes: a snapshot replaces what is confirmed, not what this peer was
    /// told about its own intents.
    ///
    /// What earlier frames of this pump placed is applied first, so that
    /// everything before the snapshot is exactly what it was when each
    /// frame advanced (an acknowledgement in the inbox leaves pending as a
    /// confirmed intent, not as one the replay runs again); the fresh
    /// replica has an empty inbox.
    ///
    /// Behind (`docs/plan-db.md` D1), each row is projected to this peer's
    /// table as a fact is; one that cannot be is kept as it came, as a fact
    /// is applied raw (§4.5). Which is why the rows arrive as rows and are
    /// put in the store here, not as they are decoded: whether this peer is
    /// behind is said by the frame's `module`, which follows its `rows`.
    pub fn recv_snapshot(&mut self, s: Snapshot) {
        let Snapshot {
            seq,
            rows,
            log_id,
            module,
            held,
        } = s;
        self.heard_module(module);
        self.served = true;
        self.started = true;
        self.replica.settle();
        let behind = self.replica.behind;
        // `docs/plan-guards.md` D2 A partial snapshot is of the device's
        // schema — the module's with the tables held in part narrowed — and
        // the replica holds a union from here; a whole one, the module's.
        let schema = match &held {
            Some(h) => crate::scope::device_schema(&self.schema, h),
            None => self.schema.clone(),
        };
        let mut st = MemoryStore::empty(schema.clone());
        for (t, rs) in rows {
            let tbl = schema.lookup_table(&t);
            for row in rs {
                let row = match tbl {
                    Some(tbl) if behind => project_row(tbl, &row).unwrap_or(row),
                    _ => row,
                };
                st.apply_change(&Change::Add(t.clone(), row));
            }
        }
        let r = &mut self.replica;
        let mut opened = Replica::open(schema, r.bodies.clone(), st, seq, r.pending.clone());
        opened.natives = r.natives.clone();
        opened.log_id = log_id;
        opened.behind = r.behind;
        opened.partial = held;
        opened.through = seq;
        let mut told = std::mem::take(&mut r.rejections);
        told.append(&mut opened.rejections);
        opened.rejections = told;
        self.replica = opened;
    }

    /// §12.2 The end of a pump: the replica's inbox applied once
    /// ([`Replica::settle`]), then, if a page arrived, the facts it still
    /// waits on asked for, and the next page if it said there is one. Call it after the last frame of each pump —
    /// `ark_client::Peer::pump` does, after draining its link, and
    /// [`crate::sim::Sim`] does after each delivery to a client. Frames
    /// with nothing to apply cost nothing here. One advance per pump is
    /// what R8 of `docs/plan-perf.md` holds: with K intents pending, fifty
    /// pushes in one pump re-run the K once.
    pub fn settle(&mut self) {
        self.replica.settle();
        if std::mem::take(&mut self.paged) {
            let needs = self.replica.needs();
            if !needs.is_empty() {
                self.emit(ClientMsg::NeedFacts { seqs: needs });
            }
        }
        if std::mem::take(&mut self.more) {
            let hello = self.hello();
            self.emit(hello);
        }
    }

    /// A live frame; dropped while unlinked, never queued.
    pub fn say(&mut self, frame: Vec<u8>) {
        self.emit(ClientMsg::Say { frame });
    }

    /// Ask the authority whether it agrees with the replica's confirmed
    /// state.
    ///
    /// Not while [`Client::behind`] (`docs/plan-db.md` D1): the hash is
    /// over this peer's schema, which is not the server's.
    pub fn verify_all(&mut self) {
        let partial = self.replica.partial.is_some();
        if self.behind() || (partial && !self.served) {
            return;
        }
        let (seq, hash) = self.replica.verify_at();
        let log_id = self.replica.log_id;
        self.emit(ClientMsg::Verify { seq, hash, log_id, partial });
    }

    pub fn take_outgoing(&mut self) -> Vec<ClientMsg> {
        std::mem::take(&mut self.out)
    }

    pub fn take_heard(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.heard)
    }
}

// ---------------------------------------------------------------------
// The server

/// A change undone: the transition back, exact because a fact carries the
/// whole row on each side — what taking an entry's facts back off a later
/// state applies, newest first.
fn undo(c: &Change) -> Change {
    match c {
        Change::Add(t, r) => Change::Remove(t.clone(), r.clone()),
        Change::Remove(t, r) => Change::Add(t.clone(), r.clone()),
        Change::Edit(t, old, new) => Change::Edit(t.clone(), new.clone(), old.clone()),
    }
}

/// Who a connection is: the user, the login, and the roles the
/// authenticator says they hold. Every entry the connection pushes is held
/// to the user and the login, and stamped with the roles
/// (`docs/plan-guards.md` D1): what the connection pushes is judged and
/// logged under them, whatever the device believed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub user: String,
    pub session: String,
    /// A claim about a person that the log does not hold — granting one in
    /// the log would need a rule about who may grant, which is a role — so
    /// the authenticator makes it, at every `Hello`, from what it was
    /// configured with.
    pub roles: BTreeSet<String>,
}

impl Identity {
    /// A user under a login, holding no role.
    pub fn new(user: impl Into<String>, session: impl Into<String>) -> Identity {
        Identity {
            user: user.into(),
            session: session.into(),
            roles: BTreeSet::new(),
        }
    }

    /// The same identity, holding these roles.
    pub fn with_roles(mut self, roles: impl IntoIterator<Item = impl Into<String>>) -> Identity {
        self.roles = roles.into_iter().map(Into::into).collect();
        self
    }
}

/// What a token proves; asked once, at `Hello`.
pub type Authenticate = Box<dyn Fn(Option<&str>) -> Option<Identity> + Send + Sync>;

/// May this identity receive the log? The read rule.
pub type Access = Box<dyn Fn(&Identity) -> bool + Send + Sync>;

/// Does this user own this session? Asked only about the connection's own
/// user, of an entry whose session is not the connection's.
pub type Owns = Box<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// Dev auth: anyone is whoever they say, and the token is their name —
/// `alice`, or `alice:library,admin` for a name holding roles
/// ([`dev_identity`]).
pub fn trusting() -> Authenticate {
    Box::new(|tok| Some(dev_identity(tok.unwrap_or("anonymous"), "dev")))
}

/// A dev login's name read as dev auth reads it, under
/// `session`: `name` is that user holding no role, `name:role,role` the
/// user `name` holding each role named — what a laptop types to be the
/// scanner (`alice:library`). An empty role is not one.
pub fn dev_identity(name: &str, session: &str) -> Identity {
    match name.split_once(':') {
        None => Identity::new(name, session),
        Some((user, roles)) => Identity::new(user, session).with_roles(roles.split(',').map(str::trim).filter(|r| !r.is_empty())),
    }
}

/// Everyone who signed in may read the log.
pub fn open_access() -> Access {
    Box::new(|_| true)
}

/// What an intent naming a function no closure is held for is answered
/// with: a `held` from a server that holds (`docs/plan-db.md` D1), and
/// before that a `reject` with this reason, which a [`Client`] reads as
/// the hold it was.
pub fn unknown_function(fh: &[u8]) -> String {
    format!("unknown function {}", hex(fh))
}

/// §12.5 The reason a `Reject` carries (`Ark.Protocol.refusalText`): a
/// mutator's own refusal is its text, word for word, because that is what
/// an author wrote for a person to read; the store's constraint refusals
/// are named in a sentence.
pub fn refusal_text(r: &Refusal) -> String {
    match r {
        Refusal::Refused(t) => t.clone(),
        Refusal::NoSuchTable(t) => format!("no table {t}"),
        Refusal::MalformedRow(t, why) => format!("{t}: {why}"),
        Refusal::NotNull(t, c) => format!("{t}.{c} may not be empty"),
        Refusal::UniqueViolation(t, cs) => format!("{t}: another row has the same {}", cs.join(", ")),
        Refusal::MissingParent(t, c, p) => format!("{t}.{c} names no {p}"),
        Refusal::StillReferenced(t, child) => format!("{t}: still referenced by {child}"),
        Refusal::Forbidden(t) => format!("{t}: not this login's to write"),
    }
}

struct Conn {
    who: Identity,
    mode: Mode,
    /// The sequence the connection has been sent up to.
    sent: Seq,
    /// Its `Hello` named another log than this authority's: what it
    /// confirmed is of that log, and it is sent this one's snapshot at the
    /// head before anything else (§12.4, Round 4).
    elsewhere: bool,
    /// `docs/plan-guards.md` D2 What this identity holds, when it is not
    /// everything: the connection is served by facts filtered and
    /// projected to it, and `None` is today's path, by intents.
    holdings: Option<Holdings>,
    /// A partial connection not yet sent the snapshot that starts it.
    fresh: bool,
    /// The last page sent to this partial connection said there is more:
    /// a `hello` from it now is the log paging. Any other `hello` on a
    /// partial connection is a peer that missed something — the snapshot
    /// that started it, or a page — and is started over (`docs/plan-guards.md`
    /// D2).
    more_owed: bool,
}

/// An authority's end of every connection (`Ark.Protocol.Server`).
pub struct Server<M: Machine> {
    auth: Authenticate,
    owns: Owns,
    access: Access,
    pub authority: Authority,
    conns: BTreeMap<ConnId, Conn>,
    machine: M,
    pub rooms: Rooms<M::State>,
    /// Oldest first.
    out: Vec<(ConnId, ServerMsg)>,
    /// The hash of the module this server runs ([`Server::with_module`]):
    /// said on every page and snapshot as `module` (`docs/plan-db.md` D1),
    /// so a peer whose own module differs knows it is applying facts of a
    /// schema that is not its own. `None` only for a machine nobody told,
    /// a test's or a vector's, whose frames are the bytes they were.
    pub module: Option<Vec<u8>>,
    /// Whether each function's closure reads a role outside its guards
    /// ([`crate::hash::reads_roles`]), as asked: an entry of one that the
    /// stamp gave other roles is acknowledged with its facts, and one of
    /// any other is not (`docs/plan-guards.md` D1). A closure is fixed by
    /// its hash, so an answer is kept.
    role_readers: BTreeMap<FnHash, bool>,
    /// `docs/plan-guards.md` D2 The module's scopes ([`Server::with_scopes`]):
    /// what each connection holds is computed from them at its `Hello`.
    /// `None`, or a module with none, and every connection is whole.
    scopes: Option<Scopes>,
}

impl<M: Machine> Server<M> {
    /// A server that is the authority for the log (`openServer`). By
    /// default an entry must carry the connection's own session.
    pub fn open(auth: Authenticate, access: Access, machine: M, mut authority: Authority) -> Server<M> {
        // `docs/plan-guards.md` D3 A server's authority runs the private
        // blocks: its facts are the whole run's.
        authority.private = true;
        Server {
            auth,
            owns: Box::new(|_, _| false),
            access,
            authority,
            conns: BTreeMap::new(),
            machine,
            rooms: Rooms::new(),
            out: vec![],
            module: None,
            role_readers: BTreeMap::new(),
            scopes: None,
        }
    }

    /// `docs/plan-guards.md` D2 Serve each connection what its identity
    /// holds of the module's scopes: a server built from a module with any
    /// says so. With none, every connection is whole and this changes
    /// nothing.
    pub fn with_scopes(mut self, scopes: Scopes) -> Server<M> {
        self.set_scopes(scopes);
        self
    }

    /// [`Server::with_scopes`], in place: from the next `Hello` on.
    pub fn set_scopes(&mut self, scopes: Scopes) {
        self.scopes = (!scopes.is_empty()).then_some(scopes);
    }

    /// What `who` holds of this server's scopes; `None` when everything.
    pub fn holdings_of(&self, who: &Identity) -> Option<Holdings> {
        let h = self.scopes.as_ref()?.holdings(Who {
            user: &who.user,
            session: &who.session,
            roles: &who.roles,
        });
        (!h.is_whole()).then_some(h)
    }

    // Whether the closure under `fh` reads a role outside its guards; a
    // hash this server holds no closure for reads none.
    fn reads_roles(&mut self, fh: &FnHash) -> bool {
        if let Some(r) = self.role_readers.get(fh) {
            return *r;
        }
        let r = self.authority.bodies.get(fh).is_some_and(crate::hash::reads_roles);
        self.role_readers.insert(fh.clone(), r);
        r
    }

    /// Say this module hash on every page and snapshot (`docs/plan-db.md`
    /// D1): a server built from a module always does.
    pub fn with_module(mut self, module: Vec<u8>) -> Server<M> {
        self.module = Some(module);
        self
    }

    /// Install the sessions a user owns, which the authenticator's session
    /// store knows and the engine does not (`withOwns`). With it, an entry
    /// authored offline under one login and pushed after the same person
    /// signed in again is still theirs.
    pub fn with_owns(mut self, owns: Owns) -> Server<M> {
        self.owns = owns;
        self
    }

    fn send(&mut self, c: ConnId, m: ServerMsg) {
        self.out.push((c, m));
    }

    fn deliver(&mut self, post: live::Post) {
        for (to, f) in post.out {
            self.send(to, ServerMsg::Heard { frame: f });
        }
    }

    /// §12.3 A frame from a connection.
    pub fn recv(&mut self, c: ConnId, msg: ClientMsg) {
        if !matches!(msg, ClientMsg::Hello { .. }) && !self.conns.contains_key(&c) {
            self.send(
                c,
                ServerMsg::Denied {
                    reason: "hello first".into(),
                },
            );
            return;
        }
        match msg {
            ClientMsg::Hello { sub, token, .. } => match (self.auth)(token.as_deref()) {
                None => self.send(
                    c,
                    ServerMsg::Denied {
                        reason: "not signed in".into(),
                    },
                ),
                Some(who) if who.user.is_empty() => self.send(
                    c,
                    ServerMsg::Denied {
                        reason: "not signed in".into(),
                    },
                ),
                Some(who) if !(self.access)(&who) => self.send(
                    c,
                    ServerMsg::Denied {
                        reason: "not allowed".into(),
                    },
                ),
                Some(who) => {
                    // A second Hello on one connection is the log paging,
                    // and says where to continue from; the room already has
                    // this peer, and arriving again changes nothing.
                    //
                    // A cursor is a place in one log, and the one this
                    // peer names may not be this authority's (Round 4).
                    // Only two names can disagree: a peer that names none
                    // — it has not been told, or it is older than logs
                    // having names — and an authority whose log nobody
                    // named are both served as they always were.
                    let elsewhere = matches!((sub.log_id, self.authority.log.id()), (Some(theirs), Some(ours)) if theirs != ours);
                    // `docs/plan-guards.md` D2 Whole or partial is decided
                    // by the scopes and this identity, never asked for. A
                    // peer that held a union and is whole now is sent the
                    // snapshot at the head, as one of another log is: paged,
                    // it would take intents onto a store missing rows. A
                    // partial connection asking again from anywhere but where
                    // it was sent to has missed something — a page, or the
                    // snapshot that started it — and is started over from a
                    // snapshot: what it holds may be another identity's.
                    let holdings = self.holdings_of(&who);
                    let again = self.conns.get(&c).is_none_or(|was| was.sent != sub.since || !was.more_owed);
                    let partial = holdings.is_some();
                    let elsewhere = elsewhere || (sub.partial && !partial);
                    let peer = live::Peer {
                        conn: c,
                        room: who.user.clone(),
                        who: who.session.clone(),
                    };
                    self.conns.insert(
                        c,
                        Conn {
                            who,
                            mode: sub.mode,
                            sent: sub.since,
                            elsewhere,
                            holdings,
                            fresh: partial && again,
                            more_owed: false,
                        },
                    );
                    let post = live::arrive(&self.machine, &mut self.rooms, peer);
                    self.deliver(post);
                    let before = self.out.len();
                    self.fanout();
                    // A server that says its module says it in answer to
                    // every `Hello` (`docs/plan-db.md` D1): a peer at the
                    // head is sent no page, and would otherwise learn
                    // only at the next entry that it is behind — after a
                    // `Verify` that could only disagree. An empty page,
                    // carrying the log and the module; a server that says
                    // no module is answered as it always was.
                    let paged = self.out[before..]
                        .iter()
                        .any(|(to, m)| *to == c && matches!(m, ServerMsg::Batch { .. } | ServerMsg::SnapshotOf { .. }));
                    if self.module.is_some() && !paged {
                        let head = self.authority.log.head_seq();
                        let covers = partial.then_some(Covers { after: head, upto: head });
                        self.send(
                            c,
                            ServerMsg::Batch {
                                items: vec![],
                                has_more: false,
                                log_id: self.authority.log.id(),
                                module: self.module.clone(),
                                covers,
                            },
                        );
                    }
                }
            },
            ClientMsg::Push { entries } => {
                let who = self.conns[&c].who.clone();
                let mut acks: Vec<(Id, Seq)> = Vec::new();
                let mut stamp_facts: Vec<(Seq, Facts)> = Vec::new();
                for e in &entries {
                    let theirs = e.actor == who.user && (e.session == who.session || (self.owns)(&e.actor, &e.session));
                    if !theirs {
                        self.send(
                            c,
                            ServerMsg::Reject {
                                id: e.id,
                                reason: "not yours".into(),
                            },
                        );
                        continue;
                    }
                    // A function this server cannot run — no module it
                    // has run shipped it — is held, not refused
                    // (`docs/plan-db.md` D1): the peer is newer than the
                    // server, and the same intent pushed after an upgrade
                    // is sequenced. One already in the log is the
                    // duplicate it is, whatever is held now.
                    if !self.authority.can_apply(&e.fn_hash) && self.authority.log.seq_of(&e.id).is_none() {
                        self.send(
                            c,
                            ServerMsg::Held {
                                id: e.id,
                                reason: unknown_function(&e.fn_hash),
                            },
                        );
                        continue;
                    }
                    // The authority stamps (`docs/plan-guards.md` D1): the
                    // entry is judged, and logged, under the roles this
                    // connection's identity holds, whatever the device
                    // believed when it authored it — its run was a preview.
                    // So a device that believed a role it was never given is
                    // refused by the guard here, and no claim of a device's
                    // ever reaches the log. A subset check would not do: a
                    // device could leave out a role a `!has_role(..)`
                    // depends on.
                    let restamped = e.roles != who.roles;
                    let stamped;
                    let pushed = e;
                    let e = if restamped {
                        stamped = Entry {
                            roles: who.roles.clone(),
                            ..e.clone()
                        };
                        &stamped
                    } else {
                        e
                    };
                    match self.authority.sequence_entry(e) {
                        // Stamped with other roles than the device authored
                        // under, and its closure reads a role beyond a
                        // guard's refusal: its run there was a preview of
                        // what was not logged, so the facts ride the
                        // acknowledgement and the device holds its record to
                        // them — and takes them where they differ — even
                        // when the page carrying the stamped entry never
                        // reaches it. Any other entry is acknowledged as it
                        // was: a device whose belief is right, and one whose
                        // function reads no role — every intent of a peer
                        // older than roles, which sends none.
                        // `docs/plan-guards.md` D3 So does every entry of a
                        // function with a server half: the device previewed
                        // the public body, and the log holds the whole run.
                        Sequenced::Appended(n, facts) => {
                            if (restamped && self.reads_roles(&e.fn_hash)) || self.authority.is_private(&e.fn_hash) {
                                stamp_facts.push((n, facts));
                            }
                            acks.push((e.id, n))
                        }
                        // A duplicate is held to the roles it was logged
                        // with: the device pushing it again may still be
                        // holding the preview of another belief.
                        // One below the horizon has no facts to send: the
                        // snapshot that peer is served covers it, and its
                        // acknowledgement at or below the cursor confirms it.
                        Sequenced::Duplicate(n) => {
                            if let Some((logged, facts)) = self.authority.log.entries.get(&n) {
                                let differs = logged.roles != pushed.roles;
                                let facts = facts.clone();
                                if (differs && self.reads_roles(&e.fn_hash)) || self.authority.is_private(&e.fn_hash) {
                                    stamp_facts.push((n, facts));
                                }
                            }
                            acks.push((e.id, n))
                        }
                        Sequenced::Rejected(why) => self.send(
                            c,
                            ServerMsg::Reject {
                                id: e.id,
                                reason: refusal_text(&why),
                            },
                        ),
                    }
                }
                // `docs/plan-guards.md` D2 On a partial connection the
                // stamp's facts are what that person holds of them, as the
                // page's are, so the acknowledgement and the page agree.
                if let Some(h) = self.conns.get(&c).and_then(|cn| cn.holdings.clone()) {
                    stamp_facts = stamp_facts.into_iter().map(|(n, f)| (n, self.facts_for(n, &f, &h))).collect();
                }
                if !acks.is_empty() {
                    self.send(
                        c,
                        ServerMsg::Ack {
                            ids: acks.iter().map(|(i, _)| *i).collect(),
                            seqs: acks.iter().map(|(_, n)| *n).collect(),
                            log_id: self.authority.log.id(),
                            facts: stamp_facts,
                        },
                    );
                }
                self.fanout();
            }
            ClientMsg::NeedFacts { seqs } => {
                // A partial peer asks for none — it is sent every fact it
                // holds — and one that asks anyway is answered with those
                // and no others (`docs/plan-guards.md` D2).
                let items = match self.conns.get(&c).and_then(|cn| cn.holdings.clone()) {
                    Some(h) => seqs
                        .iter()
                        .filter_map(|n| {
                            let (_, f) = self.authority.log.entries.get(n)?;
                            Some((*n, self.facts_for(*n, f, &h)))
                        })
                        .collect(),
                    None => seqs
                        .iter()
                        .filter_map(|n| self.authority.log.entries.get(n).map(|(_, f)| (*n, f.clone())))
                        .collect(),
                };
                self.send(c, ServerMsg::FactsFor { items });
            }
            ClientMsg::NeedClosures { hashes } => {
                // `docs/plan-guards.md` D3 As a client may hold one: a server
                // half is the server's, and never leaves it.
                let items = hashes
                    .iter()
                    .filter_map(|h| self.authority.bodies.get(h).map(|cl| (h.clone(), crate::hash::stripped(cl))))
                    .collect();
                self.send(c, ServerMsg::Closures { items });
            }
            // At the head the authority's store is the answer, hashed as
            // it stands; only a sequence below it is replayed (R4).
            ClientMsg::Verify { seq, hash, log_id, partial } => {
                // Below the horizon or past the head there is no state to
                // compare, and saying `ok: false` there would be reported
                // as a divergence: it is said to be unknown (D3). So is a
                // sequence of another log than this authority's — a client
                // that has not yet had the snapshot of a log that replaced
                // the one it held (D2) — which is never compared. One that
                // names no log is compared, as it always was; "another" is
                // read as a `hello`'s is, both named and not the same.
                let elsewhere = matches!((log_id, self.authority.log.id()), (Some(theirs), Some(ours)) if theirs != ours);
                let holdings = self.conns.get(&c).and_then(|cn| cn.holdings.as_ref());
                let a = &self.authority;
                let at = if elsewhere || partial != holdings.is_some() {
                    None
                } else if let Some(h) = holdings {
                    // `docs/plan-guards.md` D2 A partial peer holds its
                    // union, and is answered from the union's digest of the
                    // state at its sequence: the authority's store at the
                    // head, and below it the state the facts reach — O(what
                    // it holds) per verify, and verifies are rare.
                    if seq == a.log.head_seq() {
                        Some(h.digest(&a.store))
                    } else {
                        a.log.state_at(seq).map(|st| h.digest(&st))
                    }
                } else {
                    a.log.hash_at(seq, &a.store)
                };
                let (ok, unknown) = match at {
                    Some(h) => (h == hash, false),
                    None => (false, true),
                };
                self.send(c, ServerMsg::Agree { seq, hash, ok, unknown });
            }
            ClientMsg::Say { frame } => {
                let post = live::speak(&self.machine, &mut self.rooms, c, &frame);
                self.deliver(post);
            }
        }
    }

    /// A connection closed: the room hears it, the cursor is forgotten.
    pub fn disconnect(&mut self, c: ConnId) {
        let post = live::depart(&self.machine, &mut self.rooms, c);
        self.conns.remove(&c);
        self.deliver(post);
    }

    /// §12.4 Fan-out: every connection, everything above what it has been
    /// sent, a page at a time; a snapshot for one below the horizon. Run
    /// after every message.
    ///
    /// A connection whose cursor is *past* the head is the below-horizon
    /// case from the other side (`docs/plan-perf.md` R6): a peer confirmed
    /// entries of a log this authority no longer has — a server restarted
    /// over an emptied or older data directory. Serving it nothing left
    /// it at a cursor nothing would ever reach and its pending intents
    /// unacknowledged for ever. It is sent the authority's store as the
    /// snapshot at the head, the same `SnapshotOf` a peer below the
    /// horizon gets, and a client re-opens from any snapshot with its
    /// pending intents on top (§12.2) — which it pushed after its `Hello`,
    /// so they are sequenced here, acknowledged or refused, and confirmed
    /// by the page that follows. Entries it had confirmed that this log
    /// never held are gone from it: everyone is re-based onto what the
    /// authority has, which is the only log there is.
    ///
    /// A connection whose `Hello` named another log is the same case with
    /// the cursor anywhere (Round 4): at or below the head is no evidence
    /// the peer holds this log, only that it holds as many entries of *a*
    /// log — the lost one, when a server that lost its log has sequenced
    /// past where its peers were before they came back. Paged on from its
    /// cursor it would take this log's entries on top of the other's
    /// state, and nothing but a `Verify` would ever say so. It is sent the
    /// snapshot at the head, once, with this log's name on it.
    ///
    /// **A snapshot below the head is followed by the first page, in the
    /// same turn** (`docs/plan-perf.md` R10). A connection is otherwise sent
    /// one message per turn, and a client asks again only after a `Batch`
    /// with `has_more`; after a `SnapshotOf` it has nothing to ask with, so
    /// a peer with nothing pending that came back below the horizon of a
    /// quiet server sat at the horizon until somebody else spoke. The page
    /// is the one it would have been sent next; only which turn carries it
    /// moved.
    fn fanout(&mut self) {
        let conns: Vec<(ConnId, Mode, Seq, bool, bool)> = self
            .conns
            .iter()
            .map(|(c, cn)| (*c, cn.mode, cn.sent, cn.elsewhere, cn.holdings.is_some()))
            .collect();
        for (c, md, sent, elsewhere, partial) in conns {
            if partial {
                self.fan_partial(c);
                continue;
            }
            let mut next = self.fan_one(c, md, sent, elsewhere);
            while let Some(from) = next {
                next = self.fan_one(c, md, from, false);
            }
        }
    }

    /// `docs/plan-guards.md` D2 One message to a partial connection, if it
    /// is owed one: the snapshot of what it holds at the head when it is
    /// fresh, of another log, or below the horizon or past the head; else
    /// the next page of what it holds, covering every sequence it passes.
    fn fan_partial(&mut self, c: ConnId) {
        let Some(conn) = self.conns.get(&c) else { return };
        let (sent, snapshot) = (conn.sent, conn.fresh || conn.elsewhere);
        let Some(h) = conn.holdings.clone() else { return };
        let a = &self.authority;
        let head = a.log.head_seq();
        let msg = if snapshot || sent > head || sent < a.log.horizon() {
            let rows = h
                .held_rows(&a.store)
                .into_iter()
                .map(|(t, rs)| (t, rs.into_iter().map(Row::into_value).collect()))
                .collect();
            ServerMsg::SnapshotOf {
                seq: head,
                hash: h.digest(&a.store),
                rows,
                log_id: a.log.id(),
                module: self.module.clone(),
                held: Some(h.columns()),
            }
        } else if sent == head {
            return;
        } else {
            let Page::Entries(items, more) = a.page(sent, BATCH_LIMIT) else {
                unreachable!("a cursor at or above the horizon is paged")
            };
            let upto = items.last().map_or(sent, |(n, _, _)| *n);
            let user = self.conns[&c].who.user.clone();
            ServerMsg::Batch {
                items: self.partial_page(items, &h, &user),
                has_more: more,
                log_id: a.log.id(),
                module: self.module.clone(),
                covers: Some(Covers { after: sent, upto }),
            }
        };
        let advanced = match &msg {
            ServerMsg::SnapshotOf { seq, .. } => *seq,
            ServerMsg::Batch { covers, .. } => covers.map_or(sent, |c| c.upto),
            _ => sent,
        };
        let more = matches!(&msg, ServerMsg::Batch { has_more: true, .. });
        self.send(c, msg);
        if let Some(conn) = self.conns.get_mut(&c) {
            conn.sent = advanced;
            conn.elsewhere = false;
            conn.fresh = false;
            conn.more_owed = more;
        }
    }

    /// `docs/plan-guards.md` D2 A page of the log as a person holds it:
    /// each entry's facts through [`Holdings::filter_facts`], between the
    /// state before it and the state after it, and an entry none of whose
    /// facts they hold left out — its sequence passes — unless it is their
    /// own, which is sent with whatever survives so that the intent it
    /// confirms leaves pending. Another person's entry goes as its
    /// envelope: its arguments and autos are theirs, and a partial peer
    /// never runs an intent.
    ///
    /// The states are the authority's store at the head with the facts
    /// above each entry taken back, in overlays over it, newest first: a
    /// page at the head — every page but a peer's catching up, which a
    /// partial peer never does, since it starts from a snapshot — takes
    /// back nothing but its own entries.
    fn partial_page(&self, items: Vec<(Seq, Entry, Facts)>, h: &Holdings, user: &str) -> Vec<(Seq, Entry, Option<Facts>)> {
        let Some(last) = items.last().map(|(n, _, _)| *n) else { return vec![] };
        let a = &self.authority;
        let sch = &a.schema;
        let mut after = Overlay::new(&a.store);
        for (_, (_, f)) in a.log.entries.range(last + 1..).rev() {
            for c in f.iter().rev() {
                after.apply_change(&undo(&widened(sch, c)));
            }
        }
        let mut out = Vec::with_capacity(items.len());
        for (n, e, f) in items.into_iter().rev() {
            let back: Vec<Change> = f.iter().rev().map(|c| undo(&widened(sch, c))).collect();
            let seen = {
                let mut before = Overlay::new(&after);
                before.apply_changes(&back);
                h.filter_facts(&before, &after, &f)
            };
            after.apply_changes(&back);
            let own = e.actor == user;
            if seen.is_empty() && !own {
                continue;
            }
            let e = if own {
                e
            } else {
                Entry {
                    args: Args::new(),
                    autos: Args::new(),
                    ..e
                }
            };
            out.push((n, e, Some(seen)));
        }
        out.reverse();
        out
    }

    // One entry's facts as a person holds them, between the states the log's
    // facts reach before and after it: what a partial peer asking
    // `need_facts`, or acknowledged with the stamp's facts, is sent.
    fn facts_for(&self, n: Seq, f: &Facts, h: &Holdings) -> Facts {
        let a = &self.authority;
        let state = |m: Seq| {
            if m == a.log.head_seq() {
                Some(a.store.clone())
            } else {
                a.log.state_at(m)
            }
        };
        match (state(n - 1), state(n)) {
            (Some(before), Some(after)) => h.filter_facts(&before, &after, f),
            _ => vec![],
        }
    }

    /// One message to connection `c` of [`Server::fanout`], if it is owed
    /// one; `Some(seq)` when that was a snapshot at `seq`, after which the
    /// first page above it follows at once.
    fn fan_one(&mut self, c: ConnId, md: Mode, sent: Seq, elsewhere: bool) -> Option<Seq> {
        {
            let a = &self.authority;
            let head = a.log.head_seq();
            if sent == head && !elsewhere {
                return None;
            }
            let page = if sent > head || elsewhere {
                Page::BelowHorizon(snapshot_of(head, a.store.clone()).of_log(a.log.id()))
            } else {
                a.page(sent, BATCH_LIMIT)
            };
            let (msg, advanced) = match page {
                Page::BelowHorizon(sn) => {
                    let rows = sn
                        .store
                        .table_names()
                        .into_iter()
                        .map(|t| (t.clone(), sn.store.scan(&t).into_iter().map(Row::into_value).collect()))
                        .collect();
                    (
                        ServerMsg::SnapshotOf {
                            seq: sn.seq,
                            hash: sn.hash.clone(),
                            rows,
                            log_id: sn.log_id,
                            module: self.module.clone(),
                            held: None,
                        },
                        sn.seq,
                    )
                }
                Page::Entries(items, more) => {
                    let last = items.iter().map(|(n, _, _)| *n).max().unwrap_or(sent).max(sent);
                    // `docs/plan-guards.md` D3 A peer that replays is sent the
                    // facts of exactly the entries it cannot replay: those of
                    // a function with a server half, which it takes by them.
                    let with_facts = items
                        .into_iter()
                        .map(|(n, e, f)| {
                            let facts = md == Mode::ByFacts || a.is_private(&e.fn_hash);
                            (n, e, facts.then_some(f))
                        })
                        .collect();
                    (
                        ServerMsg::Batch {
                            items: with_facts,
                            has_more: more,
                            log_id: a.log.id(),
                            module: self.module.clone(),
                            covers: None,
                        },
                        last,
                    )
                }
            };
            let snapshot = matches!(msg, ServerMsg::SnapshotOf { .. });
            self.send(c, msg);
            if let Some(conn) = self.conns.get_mut(&c) {
                conn.sent = advanced;
                conn.elsewhere = false;
            }
            snapshot.then_some(advanced)
        }
    }

    pub fn take_outgoing(&mut self) -> Vec<(ConnId, ServerMsg)> {
        std::mem::take(&mut self.out)
    }

    /// Who a connection was identified as, if it has said hello.
    pub fn identity(&self, c: ConnId) -> Option<&Identity> {
        self.conns.get(&c).map(|cn| &cn.who)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::state_hash;
    use crate::live::Silent;

    /// `docs/plan-db.md` D2 A `verify` names the log its sequence is of,
    /// and an authority on another log answers it `unknown` — never
    /// compared, so never "disagreed" about two logs, which is what a
    /// client that had not yet had the snapshot of a server that came back
    /// emptied was told. The same `verify` naming no log, as every client
    /// before the field sends it, is compared as it always was: agreed
    /// with the right hash, disagreed with a wrong one; and one naming the
    /// authority's own log likewise. Falsified by ignoring the field on the
    /// server: the verify from log A was answered `ok`, not `unknown`.
    #[test]
    fn a_verify_of_another_log_is_answered_cannot_say() {
        let sch = Schema::empty();
        let (log_a, log_b) = ([0xa1; 16], [0xb2; 16]);
        let mut a = Authority::new(sch.clone(), BTreeMap::new());
        a.log.name_if_unnamed(log_b);
        let hash = state_hash(&a.store);
        let mut sv = Server::open(trusting(), open_access(), Silent, a);
        let hello = ClientMsg::Hello {
            sub: Subscription {
                partial: false,
                since: 0,
                mode: Mode::Whole,
                log_id: Some(log_b),
            },
            token: Some("alice".into()),
            spec: crate::ir::SPEC_VERSION,
        };
        sv.recv(1, hello);
        let _ = sv.take_outgoing();
        let mut answer = |hash: &[u8], log_id| {
            sv.recv(
                1,
                ClientMsg::Verify {
                    partial: false,
                    seq: 0,
                    hash: hash.to_vec(),
                    log_id,
                },
            );
            sv.take_outgoing()
                .into_iter()
                .find_map(|(_, m)| match m {
                    ServerMsg::Agree { ok, unknown, .. } => Some((ok, unknown)),
                    _ => None,
                })
                .expect("an answer")
        };
        assert_eq!(answer(&hash, Some(log_a)), (false, true), "another log: cannot say");
        assert_eq!(answer(&[0; 32], Some(log_a)), (false, true), "another log, whatever the hash");
        assert_eq!(answer(&hash, None), (true, false), "no log named: compared");
        assert_eq!(answer(&[0; 32], None), (false, false), "no log named, a wrong hash: disagreed");
        assert_eq!(answer(&hash, Some(log_b)), (true, false), "its own log: compared");

        // And the client names the log it holds when it asks.
        let r = Replica::open(sch.clone(), BTreeMap::new(), MemoryStore::empty(sch), 0, vec![]);
        let mut c = Client::open(r, Mode::Whole, None);
        c.connected();
        c.replica.log_id = Some(log_a);
        c.verify_all();
        assert!(matches!(c.out.last(), Some(ClientMsg::Verify { log_id: Some(l), .. }) if *l == log_a));
    }
}
