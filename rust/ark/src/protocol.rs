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

use std::collections::BTreeMap;

use crate::eval::{Args, Ctx};
use crate::hash::{Closure, FnHash};
use crate::ir::decode::{closure_from_value, DecodeError};
use crate::ir::encode::closure_value;
use crate::live::{self, ConnId, Machine, Rooms};
use crate::log::{snapshot_of, Entry, Facts, Page, Seq};
use crate::peer::{Authority, Replica, Sequenced};
use crate::schema::Schema;
use crate::store::{Change, MemoryStore, Refusal, Row, Store};
use crate::value::{FieldName, Id, TableName, Value};

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMsg {
    Hello { sub: Subscription, token: Option<String>, spec: i64 },
    Push { entries: Vec<Entry> },
    NeedFacts { seqs: Vec<Seq> },
    NeedClosures { hashes: Vec<FnHash> },
    Verify { seq: Seq, hash: Vec<u8> },
    Say { frame: Vec<u8> },
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
    },
    Ack {
        ids: Vec<Id>,
        seqs: Vec<Seq>,
    },
    Reject {
        id: Id,
        reason: String,
    },
    Denied {
        reason: String,
    },
    Closures {
        items: Vec<(FnHash, Closure)>,
    },
    Agree {
        seq: Seq,
        hash: Vec<u8>,
        ok: bool,
    },
    Heard {
        frame: Vec<u8>,
    },
}

/// Entries per page.
pub const BATCH_LIMIT: usize = 256;

fn node(t: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut m: BTreeMap<FieldName, Value> = BTreeMap::new();
    m.insert("t".into(), Value::text(t));
    for (k, v) in fields {
        m.insert(k.into(), v);
    }
    Value::Struct(m)
}

fn txt(s: &str) -> Value {
    Value::text(s)
}

fn int(n: i64) -> Value {
    Value::Int(n)
}

fn strct(pairs: Vec<(&str, Value)>) -> Value {
    Value::Struct(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
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

pub fn entry_value(e: &Entry) -> Value {
    strct(vec![
        ("id", Value::Id(e.id)),
        ("actor", txt(&e.actor)),
        ("session", txt(&e.session)),
        ("fn", Value::Bytes(e.fn_hash.clone())),
        ("args", Value::Struct(e.args.clone())),
        ("autos", Value::Struct(e.autos.clone())),
    ])
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

impl ClientMsg {
    /// The frame as a value; its wire form is `canon::encode` of this.
    pub fn to_value(&self) -> Value {
        match self {
            ClientMsg::Hello { sub, token, spec } => node(
                "hello",
                named(
                    vec![
                        ("since", int(sub.since)),
                        ("mode", txt(if sub.mode == Mode::Whole { "whole" } else { "facts" })),
                        ("token", token.as_deref().map(txt).unwrap_or(Value::Null)),
                        ("spec", int(*spec)),
                    ],
                    &sub.log_id,
                ),
            ),
            ClientMsg::Push { entries } => node("push", vec![("entries", Value::List(entries.iter().map(entry_value).collect()))]),
            ClientMsg::NeedFacts { seqs } => node("need_facts", vec![("seqs", Value::List(seqs.iter().map(|n| int(*n)).collect()))]),
            ClientMsg::NeedClosures { hashes } => node(
                "need_closures",
                vec![("hashes", Value::List(hashes.iter().map(|h| Value::Bytes(h.clone())).collect()))],
            ),
            ClientMsg::Verify { seq, hash } => node("verify", vec![("seq", int(*seq)), ("hash", Value::Bytes(hash.clone()))]),
            ClientMsg::Say { frame } => node("say", vec![("say", Value::Bytes(frame.clone()))]),
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
    })
}

/// The `log` of a frame that may carry one: an id, or `None` where the
/// field is absent — a frame that names no log, which is also every frame
/// from a runtime older than logs having names (§12.4), served and serving
/// as it was. A null is refused: absence is the one encoding of "no log",
/// so that a frame has one form.
fn log_of(m: &BTreeMap<FieldName, Value>) -> Result<Option<Id>, DecodeError> {
    m.get("log").map(ident).transpose()
}

impl ServerMsg {
    /// The frame as a value.
    pub fn to_value(&self) -> Value {
        match self {
            ServerMsg::Batch { items, has_more, log_id } => node(
                "batch",
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
            ),
            ServerMsg::FactsFor { items } => node(
                "facts",
                vec![(
                    "items",
                    Value::List(
                        items
                            .iter()
                            .map(|(n, f)| strct(vec![("seq", int(*n)), ("facts", facts_value(f))]))
                            .collect(),
                    ),
                )],
            ),
            ServerMsg::SnapshotOf { seq, hash, rows, log_id } => node(
                "snapshot",
                named(
                    vec![
                        ("seq", int(*seq)),
                        ("hash", Value::Bytes(hash.clone())),
                        (
                            "rows",
                            Value::Struct(rows.iter().map(|(t, vs)| (t.clone(), Value::List(vs.clone()))).collect()),
                        ),
                    ],
                    log_id,
                ),
            ),
            ServerMsg::Ack { ids, seqs } => node(
                "ack",
                vec![
                    ("ids", Value::List(ids.iter().map(|i| Value::Id(*i)).collect())),
                    ("seqs", Value::List(seqs.iter().map(|n| int(*n)).collect())),
                ],
            ),
            ServerMsg::Reject { id, reason } => node("reject", vec![("id", Value::Id(*id)), ("reason", txt(reason))]),
            ServerMsg::Denied { reason } => node("denied", vec![("reason", txt(reason))]),
            ServerMsg::Closures { items } => node(
                "closures",
                vec![(
                    "items",
                    Value::List(
                        items
                            .iter()
                            .map(|(h, c)| strct(vec![("hash", Value::Bytes(h.clone())), ("closure", closure_value(c))]))
                            .collect(),
                    ),
                )],
            ),
            ServerMsg::Agree { seq, hash, ok } => node(
                "agree",
                vec![("seq", int(*seq)), ("hash", Value::Bytes(hash.clone())), ("ok", Value::Bool(*ok))],
            ),
            ServerMsg::Heard { frame } => node("heard", vec![("hear", Value::Bytes(frame.clone()))]),
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
            },
            "facts" => ServerMsg::FactsFor {
                items: list(
                    |x| {
                        let m = strct_of(x)?;
                        Ok((int64(need(m, "seq")?)?, list(change_from_value, need(m, "facts")?)?))
                    },
                    need(m, "items")?,
                )?,
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
            },
            "ack" => ServerMsg::Ack {
                ids: list(ident, need(m, "ids")?)?,
                seqs: list(int64, need(m, "seqs")?)?,
            },
            "reject" => ServerMsg::Reject {
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
            },
            "heard" => ServerMsg::Heard {
                frame: bytes(need(m, "hear")?)?,
            },
            other => return bad(format!("unknown server frame {other}")),
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
        Value::Text(t) => Ok(t.clone()),
        _ => bad("expected text"),
    }
}

fn bytes(v: &Value) -> D<Vec<u8>> {
    match v {
        Value::Bytes(b) => Ok(b.clone()),
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
        fn_hash: bytes(need(m, "fn")?)?,
        args: args_of(need(m, "args")?)?,
        autos: args_of(need(m, "autos")?)?,
    })
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
    pub agreed: Vec<(Seq, bool)>,
    /// A page arrived since the last [`Client::settle`]: the facts it
    /// leaves the replica waiting on are asked for there, once the inbox
    /// has been applied (R8).
    pub paged: bool,
    /// A page said there is more: the `Hello` that asks for it is said at
    /// the settle, at the cursor the page moved the replica to. Said at
    /// the frame, before the page is applied, it would name the old cursor
    /// and be sent the same page again.
    pub more: bool,
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
            ServerMsg::Batch { items, has_more, log_id } => {
                let r = &mut self.replica;
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
            // Below the horizon, past the head (R6), or of another log than
            // the one this peer held (Round 4): the confirmed store is
            // replaced by the snapshot, the cursor moves to it, and the
            // peer holds the snapshot's log from here; pending intents are
            // kept and replay on top. Verdicts the app has not yet taken
            // are kept too, ahead of any the replay makes: a snapshot
            // replaces what is confirmed, not what this peer was told about
            // its own intents.
            //
            // What earlier frames of this pump placed is applied first, so
            // that everything before the snapshot is exactly what it was
            // when each frame advanced (an acknowledgement in the inbox
            // leaves pending as a confirmed intent, not as one the replay
            // runs again); the fresh replica has an empty inbox.
            ServerMsg::SnapshotOf { seq, rows, log_id, .. } => {
                self.replica.settle();
                let mut st = MemoryStore::empty(self.schema.clone());
                for (t, vs) in rows {
                    for v in vs {
                        if let Value::Struct(row) = v {
                            st.apply_change(&Change::Add(t.clone(), Row::from_struct(row)));
                        }
                    }
                }
                let r = &mut self.replica;
                let mut opened = Replica::open(r.schema.clone(), r.bodies.clone(), st, seq, r.pending.clone());
                opened.natives = r.natives.clone();
                opened.log_id = log_id;
                let mut told = std::mem::take(&mut r.rejections);
                told.append(&mut opened.rejections);
                opened.rejections = told;
                self.replica = opened;
            }
            ServerMsg::Ack { ids, seqs } => {
                for (i, n) in ids.iter().zip(seqs) {
                    self.replica.ack(i, n);
                }
            }
            ServerMsg::Reject { id, reason } => self.replica.reject(&id, Refusal::Refused(reason)),
            // New closures may unblock entries waiting in the inbox, at
            // the settle. A received closure replaces one already held
            // under its hash (the spec's left-biased union).
            ServerMsg::Closures { items } => {
                for (h, c) in items {
                    self.replica.bodies.insert(h, c);
                }
            }
            ServerMsg::Agree { seq, ok, .. } => self.agreed.push((seq, ok)),
        }
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
    pub fn verify_all(&mut self) {
        let (seq, hash) = self.replica.verify_at();
        self.emit(ClientMsg::Verify { seq, hash });
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

/// Who a connection is: the user, and the login. Every entry the connection
/// pushes is held to both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub user: String,
    pub session: String,
}

/// What a token proves; asked once, at `Hello`.
pub type Authenticate = Box<dyn Fn(Option<&str>) -> Option<Identity> + Send + Sync>;

/// May this identity receive the log? The read rule.
pub type Access = Box<dyn Fn(&Identity) -> bool + Send + Sync>;

/// Does this user own this session? Asked only about the connection's own
/// user, of an entry whose session is not the connection's.
pub type Owns = Box<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// Dev auth: anyone is whoever they say, and the token is their name.
pub fn trusting() -> Authenticate {
    Box::new(|tok| {
        Some(Identity {
            user: tok.unwrap_or("anonymous").to_string(),
            session: "dev".into(),
        })
    })
}

/// Everyone who signed in may read the log.
pub fn open_access() -> Access {
    Box::new(|_| true)
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
}

impl<M: Machine> Server<M> {
    /// A server that is the authority for the log (`openServer`). By
    /// default an entry must carry the connection's own session.
    pub fn open(auth: Authenticate, access: Access, machine: M, authority: Authority) -> Server<M> {
        Server {
            auth,
            owns: Box::new(|_, _| false),
            access,
            authority,
            conns: BTreeMap::new(),
            machine,
            rooms: Rooms::new(),
            out: vec![],
        }
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
                        },
                    );
                    let post = live::arrive(&self.machine, &mut self.rooms, peer);
                    self.deliver(post);
                    self.fanout();
                }
            },
            ClientMsg::Push { entries } => {
                let who = self.conns[&c].who.clone();
                let mut acks: Vec<(Id, Seq)> = Vec::new();
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
                    match self.authority.sequence_entry(e) {
                        Sequenced::Appended(n, _) | Sequenced::Duplicate(n) => acks.push((e.id, n)),
                        Sequenced::Rejected(why) => self.send(
                            c,
                            ServerMsg::Reject {
                                id: e.id,
                                reason: refusal_text(&why),
                            },
                        ),
                    }
                }
                if !acks.is_empty() {
                    self.send(
                        c,
                        ServerMsg::Ack {
                            ids: acks.iter().map(|(i, _)| *i).collect(),
                            seqs: acks.iter().map(|(_, n)| *n).collect(),
                        },
                    );
                }
                self.fanout();
            }
            ClientMsg::NeedFacts { seqs } => {
                let items = seqs
                    .iter()
                    .filter_map(|n| self.authority.log.entries.get(n).map(|(_, f)| (*n, f.clone())))
                    .collect();
                self.send(c, ServerMsg::FactsFor { items });
            }
            ClientMsg::NeedClosures { hashes } => {
                let items = hashes
                    .iter()
                    .filter_map(|h| self.authority.bodies.get(h).map(|cl| (h.clone(), cl.clone())))
                    .collect();
                self.send(c, ServerMsg::Closures { items });
            }
            // At the head the authority's store is the answer, hashed as
            // it stands; only a sequence below it is replayed (R4).
            ClientMsg::Verify { seq, hash } => {
                let ok = self.authority.log.hash_at(seq, &self.authority.store) == Some(hash.clone());
                self.send(c, ServerMsg::Agree { seq, hash, ok });
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
        let conns: Vec<(ConnId, Mode, Seq, bool)> = self.conns.iter().map(|(c, cn)| (*c, cn.mode, cn.sent, cn.elsewhere)).collect();
        for (c, md, sent, elsewhere) in conns {
            let mut next = self.fan_one(c, md, sent, elsewhere);
            while let Some(from) = next {
                next = self.fan_one(c, md, from, false);
            }
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
                        },
                        sn.seq,
                    )
                }
                Page::Entries(items, more) => {
                    let last = items.iter().map(|(n, _, _)| *n).max().unwrap_or(sent).max(sent);
                    let with_facts = items
                        .into_iter()
                        .map(|(n, e, f)| (n, e, if md == Mode::ByFacts { Some(f) } else { None }))
                        .collect();
                    (
                        ServerMsg::Batch {
                            items: with_facts,
                            has_more: more,
                            log_id: a.log.id(),
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
