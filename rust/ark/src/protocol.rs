//! §12 The protocol, as `Ark.Protocol` defines it.
//!
//! The frames two peers exchange, as values (so that [`crate::canon`] is
//! their wire form), and the two state machines around them: a [`Client`]
//! holding replicas of the scopes it subscribes to, and a [`Server`]
//! holding authorities for the scopes it hosts, the connections it has
//! identified, and the live rooms. Both are sans-io.
//!
//! Identity is asked once, at `Hello`; every pushed entry is held to it.
//! The authority applies before it appends, so a `Reject` is a verdict.
//! After every message the server sends every connection every entry above
//! what it has been sent, a page at a time. A page carries facts for a peer
//! that asked to be fed by facts, and not for one that replays.

use std::collections::BTreeMap;

use crate::eval::{Args, Ctx};
use crate::hash::{state_hash, Closure, FnHash};
use crate::ir::decode::{closure_from_value, DecodeError};
use crate::ir::encode::closure_value;
use crate::live::{self, ConnId, Machine, Rooms};
use crate::log::{Entry, Facts, Page, Seq};
use crate::peer::{Authority, Replica, Sequenced};
use crate::schema::{Schema, ScopeName};
use crate::store::{Change, MemoryStore, Refusal, Row, Store};
use crate::value::{FieldName, Id, TableName, Value};

// ---------------------------------------------------------------------
// Frames

/// How a client holds a scope: `Whole` replays intents and is exact;
/// `ByFacts` is fed the facts of every entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Whole,
    ByFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    pub scope: ScopeName,
    /// The cursor: the last sequence applied.
    pub since: Seq,
    pub mode: Mode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMsg {
    Hello {
        subs: Vec<Subscription>,
        token: Option<String>,
        spec: i64,
    },
    Push {
        scope: ScopeName,
        entries: Vec<Entry>,
    },
    NeedFacts {
        scope: ScopeName,
        seqs: Vec<Seq>,
    },
    NeedClosures {
        hashes: Vec<FnHash>,
    },
    Verify {
        scope: ScopeName,
        seq: Seq,
        hash: Vec<u8>,
    },
    Say {
        frame: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerMsg {
    Batch {
        scope: ScopeName,
        items: Vec<(Seq, Entry, Option<Facts>)>,
        has_more: bool,
    },
    FactsFor {
        scope: ScopeName,
        items: Vec<(Seq, Facts)>,
    },
    SnapshotOf {
        scope: ScopeName,
        seq: Seq,
        hash: Vec<u8>,
        rows: BTreeMap<TableName, Vec<Value>>,
    },
    Ack {
        scope: ScopeName,
        ids: Vec<Id>,
        seqs: Vec<Seq>,
    },
    Reject {
        scope: ScopeName,
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
        scope: ScopeName,
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
        Change::Add(t, r) => node("add", vec![("table", txt(t)), ("row", Value::Struct(r.clone()))]),
        Change::Remove(t, r) => node("remove", vec![("table", txt(t)), ("row", Value::Struct(r.clone()))]),
        Change::Edit(t, o, n) => node(
            "edit",
            vec![("table", txt(t)), ("old", Value::Struct(o.clone())), ("new", Value::Struct(n.clone()))],
        ),
    }
}

fn facts_value(f: &Facts) -> Value {
    Value::List(f.iter().map(change_value).collect())
}

impl ClientMsg {
    /// The frame as a value; its wire form is `canon::encode` of this.
    pub fn to_value(&self) -> Value {
        match self {
            ClientMsg::Hello { subs, token, spec } => node(
                "hello",
                vec![
                    (
                        "scopes",
                        Value::List(
                            subs.iter()
                                .map(|s| {
                                    node(
                                        "sub",
                                        vec![
                                            ("scope", txt(&s.scope)),
                                            ("since", int(s.since)),
                                            ("mode", txt(if s.mode == Mode::Whole { "whole" } else { "facts" })),
                                        ],
                                    )
                                })
                                .collect(),
                        ),
                    ),
                    ("token", token.as_deref().map(txt).unwrap_or(Value::Null)),
                    ("spec", int(*spec)),
                ],
            ),
            ClientMsg::Push { scope, entries } => node(
                "push",
                vec![("scope", txt(scope)), ("entries", Value::List(entries.iter().map(entry_value).collect()))],
            ),
            ClientMsg::NeedFacts { scope, seqs } => node(
                "need_facts",
                vec![("scope", txt(scope)), ("seqs", Value::List(seqs.iter().map(|n| int(*n)).collect()))],
            ),
            ClientMsg::NeedClosures { hashes } => node(
                "need_closures",
                vec![("hashes", Value::List(hashes.iter().map(|h| Value::Bytes(h.clone())).collect()))],
            ),
            ClientMsg::Verify { scope, seq, hash } => node(
                "verify",
                vec![("scope", txt(scope)), ("seq", int(*seq)), ("hash", Value::Bytes(hash.clone()))],
            ),
            ClientMsg::Say { frame } => node("say", vec![("say", Value::Bytes(frame.clone()))]),
        }
    }

    /// A frame from its value (`Ark.Protocol.clientFromValue`).
    pub fn from_value(v: &Value) -> Result<ClientMsg, DecodeError> {
        let m = strct_of(v)?;
        let t = text(need(m, "t")?)?;
        Ok(match t.as_str() {
            "hello" => ClientMsg::Hello {
                subs: list(sub, need(m, "scopes")?)?,
                token: match need(m, "token")? {
                    Value::Null => None,
                    x => Some(text(x)?),
                },
                spec: int64(need(m, "spec")?)?,
            },
            "push" => ClientMsg::Push {
                scope: text(need(m, "scope")?)?,
                entries: list(entry_from_value, need(m, "entries")?)?,
            },
            "need_facts" => ClientMsg::NeedFacts {
                scope: text(need(m, "scope")?)?,
                seqs: list(int64, need(m, "seqs")?)?,
            },
            "need_closures" => ClientMsg::NeedClosures {
                hashes: list(bytes, need(m, "hashes")?)?,
            },
            "verify" => ClientMsg::Verify {
                scope: text(need(m, "scope")?)?,
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

fn sub(x: &Value) -> Result<Subscription, DecodeError> {
    let m = strct_of(x)?;
    let mode = match text(need(m, "mode")?)?.as_str() {
        "whole" => Mode::Whole,
        "facts" => Mode::ByFacts,
        other => return bad(format!("unknown mode {other}")),
    };
    Ok(Subscription {
        scope: text(need(m, "scope")?)?,
        since: int64(need(m, "since")?)?,
        mode,
    })
}

impl ServerMsg {
    /// The frame as a value.
    pub fn to_value(&self) -> Value {
        match self {
            ServerMsg::Batch { scope, items, has_more } => node(
                "batch",
                vec![
                    ("scope", txt(scope)),
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
            ),
            ServerMsg::FactsFor { scope, items } => node(
                "facts",
                vec![
                    ("scope", txt(scope)),
                    (
                        "items",
                        Value::List(
                            items
                                .iter()
                                .map(|(n, f)| strct(vec![("seq", int(*n)), ("facts", facts_value(f))]))
                                .collect(),
                        ),
                    ),
                ],
            ),
            ServerMsg::SnapshotOf { scope, seq, hash, rows } => node(
                "snapshot",
                vec![
                    ("scope", txt(scope)),
                    ("seq", int(*seq)),
                    ("hash", Value::Bytes(hash.clone())),
                    (
                        "rows",
                        Value::Struct(rows.iter().map(|(t, vs)| (t.clone(), Value::List(vs.clone()))).collect()),
                    ),
                ],
            ),
            ServerMsg::Ack { scope, ids, seqs } => node(
                "ack",
                vec![
                    ("scope", txt(scope)),
                    ("ids", Value::List(ids.iter().map(|i| Value::Id(*i)).collect())),
                    ("seqs", Value::List(seqs.iter().map(|n| int(*n)).collect())),
                ],
            ),
            ServerMsg::Reject { scope, id, reason } => node("reject", vec![("scope", txt(scope)), ("id", Value::Id(*id)), ("reason", txt(reason))]),
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
            ServerMsg::Agree { scope, seq, hash, ok } => node(
                "agree",
                vec![
                    ("scope", txt(scope)),
                    ("seq", int(*seq)),
                    ("hash", Value::Bytes(hash.clone())),
                    ("ok", Value::Bool(*ok)),
                ],
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
                scope: text(need(m, "scope")?)?,
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
            },
            "facts" => ServerMsg::FactsFor {
                scope: text(need(m, "scope")?)?,
                items: list(
                    |x| {
                        let m = strct_of(x)?;
                        Ok((int64(need(m, "seq")?)?, list(change_from_value, need(m, "facts")?)?))
                    },
                    need(m, "items")?,
                )?,
            },
            "snapshot" => ServerMsg::SnapshotOf {
                scope: text(need(m, "scope")?)?,
                seq: int64(need(m, "seq")?)?,
                hash: bytes(need(m, "hash")?)?,
                rows: {
                    let mut out = BTreeMap::new();
                    for (t, vs) in strct_of(need(m, "rows")?)? {
                        out.insert(t.clone(), list(|v| Ok(v.clone()), vs)?);
                    }
                    out
                },
            },
            "ack" => ServerMsg::Ack {
                scope: text(need(m, "scope")?)?,
                ids: list(ident, need(m, "ids")?)?,
                seqs: list(int64, need(m, "seqs")?)?,
            },
            "reject" => ServerMsg::Reject {
                scope: text(need(m, "scope")?)?,
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
                scope: text(need(m, "scope")?)?,
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

fn row_of(v: &Value) -> D<Row> {
    strct_of(v).cloned()
}

pub fn entry_from_value(v: &Value) -> D<Entry> {
    let m = strct_of(v)?;
    Ok(Entry {
        id: ident(need(m, "id")?)?,
        actor: text(need(m, "actor")?)?,
        session: text(need(m, "session")?)?,
        fn_hash: bytes(need(m, "fn")?)?,
        args: row_of(need(m, "args")?)?,
        autos: row_of(need(m, "autos")?)?,
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

/// A peer's end of one connection: its replicas, and what it has queued
/// (`Ark.Protocol.Client`).
#[derive(Clone, Debug)]
pub struct Client {
    pub schema: Schema,
    pub scopes: BTreeMap<ScopeName, (Replica, Mode)>,
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
    pub agreed: Vec<(ScopeName, Seq, bool)>,
}

impl Client {
    pub fn open(schema: Schema, token: Option<String>) -> Client {
        Client {
            schema,
            scopes: BTreeMap::new(),
            token,
            linked: false,
            epoch: 0,
            out: vec![],
            heard: vec![],
            denied: None,
            agreed: vec![],
        }
    }

    /// Hold a scope, with the replica as opened from what was durable.
    pub fn subscribe(&mut self, mode: Mode, r: Replica) {
        self.scopes.insert(r.scope.clone(), (r, mode));
    }

    // Unlinked: nothing is queued; `connected` says it all again.
    fn emit(&mut self, m: ClientMsg) {
        if self.linked {
            self.out.push(m);
        }
    }

    /// §12.1 A connection opened: say hello for every scope at its cursor,
    /// then push everything pending. What was queued before is dropped.
    pub fn connected(&mut self) {
        self.linked = true;
        self.epoch += 1;
        self.out.clear();
        self.heard.clear();
        let subs = self
            .scopes
            .iter()
            .map(|(s, (r, md))| Subscription {
                scope: s.clone(),
                since: r.cursor,
                mode: *md,
            })
            .collect();
        self.emit(ClientMsg::Hello {
            subs,
            token: self.token.clone(),
            spec: 1,
        });
        let pushes: Vec<ClientMsg> = self
            .scopes
            .iter()
            .filter(|(_, (r, _))| !r.pending.is_empty())
            .map(|(s, (r, _))| ClientMsg::Push {
                scope: s.clone(),
                entries: r.pending.clone(),
            })
            .collect();
        for p in pushes {
            self.emit(p);
        }
    }

    pub fn disconnected(&mut self) {
        self.linked = false;
        self.out.clear();
        self.heard.clear();
    }

    /// Author an intent into a scope and push it if linked.
    pub fn mutate(&mut self, scope: &str, id: Id, ctx: &Ctx, fh: &FnHash, autos: &Args, args: &Args) -> Result<Entry, Refusal> {
        let Some((r, _)) = self.scopes.get_mut(scope) else {
            return Err(Refusal::Refused(format!("not holding scope {scope}")));
        };
        let e = r.mutate(id, ctx, fh, autos, args)?;
        self.emit(ClientMsg::Push {
            scope: scope.into(),
            entries: vec![e.clone()],
        });
        Ok(e)
    }

    /// §12.2 A frame from the server.
    pub fn recv(&mut self, msg: ServerMsg) {
        match msg {
            ServerMsg::Heard { frame } => self.heard.push(frame),
            ServerMsg::Denied { reason } => {
                self.denied = Some(reason);
                self.linked = false;
                self.out.clear();
            }
            ServerMsg::Batch { scope, items, has_more } => {
                let Some((r, md)) = self.scopes.get_mut(&scope) else { return };
                for (n, e, mf) in items {
                    match mf {
                        Some(f) => r.receive_with(n, e, f),
                        None => r.receive(n, e),
                    }
                }
                let needs = r.needs();
                let (cursor, md) = (r.cursor, *md);
                if !needs.is_empty() {
                    self.emit(ClientMsg::NeedFacts {
                        scope: scope.clone(),
                        seqs: needs,
                    });
                }
                if has_more {
                    let token = self.token.clone();
                    self.emit(ClientMsg::Hello {
                        subs: vec![Subscription {
                            scope,
                            since: cursor,
                            mode: md,
                        }],
                        token,
                        spec: 1,
                    });
                }
            }
            ServerMsg::FactsFor { scope, items } => {
                if let Some((r, _)) = self.scopes.get_mut(&scope) {
                    for (n, f) in items {
                        r.receive_facts(n, f);
                    }
                }
            }
            // Below the horizon: the confirmed store is replaced by the
            // snapshot and the cursor moves to it; pending intents are kept
            // and replay on top.
            ServerMsg::SnapshotOf { scope, seq, rows, .. } => {
                let schema = self.schema.clone();
                let Some((r, _)) = self.scopes.get_mut(&scope) else { return };
                let mut st = MemoryStore::empty(schema);
                for (t, vs) in rows {
                    for v in vs {
                        if let Value::Struct(row) = v {
                            st.apply_change(&Change::Add(t.clone(), row));
                        }
                    }
                }
                let opened = Replica::open(r.schema.clone(), &scope, r.bodies.clone(), st, seq, r.pending.clone());
                *r = opened;
            }
            ServerMsg::Ack { scope, ids, seqs } => {
                if let Some((r, _)) = self.scopes.get_mut(&scope) {
                    for (i, n) in ids.iter().zip(seqs) {
                        r.ack(i, n);
                    }
                }
            }
            ServerMsg::Reject { scope, id, reason } => {
                if let Some((r, _)) = self.scopes.get_mut(&scope) {
                    r.reject(&id, Refusal::Refused(reason));
                }
            }
            // New closures may unblock entries waiting in an inbox, so every
            // replica is asked to try again. A received closure replaces one
            // already held under its hash (the spec's left-biased union).
            ServerMsg::Closures { items } => {
                for (r, _) in self.scopes.values_mut() {
                    for (h, c) in &items {
                        r.bodies.insert(h.clone(), c.clone());
                    }
                    r.retry();
                }
            }
            ServerMsg::Agree { scope, seq, ok, .. } => self.agreed.push((scope, seq, ok)),
        }
    }

    /// A live frame; dropped while unlinked, never queued.
    pub fn say(&mut self, frame: Vec<u8>) {
        self.emit(ClientMsg::Say { frame });
    }

    /// Ask the authority whether it agrees with every replica's confirmed
    /// state.
    pub fn verify_all(&mut self) {
        let claims: Vec<ClientMsg> = self
            .scopes
            .iter()
            .map(|(s, (r, _))| {
                let (n, h) = r.verify_at();
                ClientMsg::Verify {
                    scope: s.clone(),
                    seq: n,
                    hash: h,
                }
            })
            .collect();
        for c in claims {
            self.emit(c);
        }
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
pub type Authenticate = Box<dyn Fn(Option<&str>) -> Option<Identity>>;

/// May this identity receive this scope? The scope-level read rule.
pub type Access = Box<dyn Fn(&Identity, &str) -> bool>;

/// Dev auth: anyone is whoever they say, and the token is their name.
pub fn trusting() -> Authenticate {
    Box::new(|tok| {
        Some(Identity {
            user: tok.unwrap_or("anonymous").to_string(),
            session: "dev".into(),
        })
    })
}

/// Everyone may read every scope.
pub fn open_access() -> Access {
    Box::new(|_, _| true)
}

struct Conn {
    who: Identity,
    /// Per scope: the mode, and the sequence the connection has been sent
    /// up to.
    scopes: BTreeMap<ScopeName, (Mode, Seq)>,
}

/// An authority's end of every connection (`Ark.Protocol.Server`).
pub struct Server<M: Machine> {
    auth: Authenticate,
    access: Access,
    pub scopes: BTreeMap<ScopeName, Authority>,
    conns: BTreeMap<ConnId, Conn>,
    machine: M,
    pub rooms: Rooms<M::State>,
    /// Oldest first.
    out: Vec<(ConnId, ServerMsg)>,
}

impl<M: Machine> Server<M> {
    pub fn open(auth: Authenticate, access: Access, machine: M) -> Server<M> {
        Server {
            auth,
            access,
            scopes: BTreeMap::new(),
            conns: BTreeMap::new(),
            machine,
            rooms: Rooms::new(),
            out: vec![],
        }
    }

    /// Host a scope: become its authority.
    pub fn host(&mut self, a: Authority) {
        self.scopes.insert(a.scope.clone(), a);
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
            ClientMsg::Hello { subs, token, .. } => match (self.auth)(token.as_deref()) {
                None => self.send(
                    c,
                    ServerMsg::Denied {
                        reason: "not signed in".into(),
                    },
                ),
                Some(who) => {
                    let scopes = subs
                        .iter()
                        .filter(|s| (self.access)(&who, &s.scope) && self.scopes.contains_key(&s.scope))
                        .map(|s| (s.scope.clone(), (s.mode, s.since)))
                        .collect();
                    let peer = live::Peer {
                        conn: c,
                        room: who.user.clone(),
                        who: who.session.clone(),
                    };
                    self.conns.insert(c, Conn { who, scopes });
                    let post = live::arrive(&self.machine, &mut self.rooms, peer);
                    self.deliver(post);
                    self.fanout();
                }
            },
            ClientMsg::Push { scope, entries } => {
                let who = self.conns[&c].who.clone();
                if !self.scopes.contains_key(&scope) {
                    self.send(
                        c,
                        ServerMsg::Denied {
                            reason: format!("unknown scope {scope}"),
                        },
                    );
                    return;
                }
                let mut acks: Vec<(Id, Seq)> = Vec::new();
                for e in &entries {
                    if e.actor != who.user || e.session != who.session {
                        self.send(
                            c,
                            ServerMsg::Reject {
                                scope: scope.clone(),
                                id: e.id,
                                reason: "not yours".into(),
                            },
                        );
                        continue;
                    }
                    let a = self.scopes.get_mut(&scope).expect("checked above");
                    match a.sequence_entry(e) {
                        Sequenced::Appended(n, _) | Sequenced::Duplicate(n) => acks.push((e.id, n)),
                        Sequenced::Rejected(why) => self.send(
                            c,
                            ServerMsg::Reject {
                                scope: scope.clone(),
                                id: e.id,
                                reason: why.to_string(),
                            },
                        ),
                    }
                }
                if !acks.is_empty() {
                    self.send(
                        c,
                        ServerMsg::Ack {
                            scope,
                            ids: acks.iter().map(|(i, _)| *i).collect(),
                            seqs: acks.iter().map(|(_, n)| *n).collect(),
                        },
                    );
                }
                self.fanout();
            }
            ClientMsg::NeedFacts { scope, seqs } => {
                if let Some(a) = self.scopes.get(&scope) {
                    let items = seqs.iter().filter_map(|n| a.log.entries.get(n).map(|(_, f)| (*n, f.clone()))).collect();
                    self.send(c, ServerMsg::FactsFor { scope, items });
                }
            }
            ClientMsg::NeedClosures { hashes } => {
                let items = hashes
                    .iter()
                    .filter_map(|h| self.scopes.values().find_map(|a| a.bodies.get(h)).map(|cl| (h.clone(), cl.clone())))
                    .collect();
                self.send(c, ServerMsg::Closures { items });
            }
            ClientMsg::Verify { scope, seq, hash } => {
                if let Some(a) = self.scopes.get(&scope) {
                    let ok = a.log.state_at(seq).map(|st| state_hash(&st)) == Some(hash.clone());
                    self.send(c, ServerMsg::Agree { scope, seq, hash, ok });
                }
            }
            ClientMsg::Say { frame } => {
                let post = live::speak(&self.machine, &mut self.rooms, c, &frame);
                self.deliver(post);
            }
        }
    }

    /// A connection closed: the room hears it, the cursors are forgotten.
    pub fn disconnect(&mut self, c: ConnId) {
        let post = live::depart(&self.machine, &mut self.rooms, c);
        self.conns.remove(&c);
        self.deliver(post);
    }

    /// §12.4 Fan-out: every connection, every scope it holds, everything
    /// above what it has been sent, a page at a time; a snapshot for one
    /// below the horizon. Run after every message.
    fn fanout(&mut self) {
        let conns: Vec<ConnId> = self.conns.keys().copied().collect();
        for c in conns {
            let scopes: Vec<(ScopeName, Mode, Seq)> = self.conns[&c].scopes.iter().map(|(s, (md, sent))| (s.clone(), *md, *sent)).collect();
            for (s, md, sent) in scopes {
                let Some(a) = self.scopes.get(&s) else { continue };
                if sent >= a.log.head_seq() {
                    continue;
                }
                let (msg, advanced) = match a.page(sent, BATCH_LIMIT) {
                    Page::BelowHorizon(sn) => {
                        let rows = sn
                            .store
                            .table_names()
                            .into_iter()
                            .map(|t| (t.clone(), sn.store.scan(&t).into_iter().map(Value::Struct).collect()))
                            .collect();
                        (
                            ServerMsg::SnapshotOf {
                                scope: s.clone(),
                                seq: sn.seq,
                                hash: sn.hash.clone(),
                                rows,
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
                                scope: s.clone(),
                                items: with_facts,
                                has_more: more,
                            },
                            last,
                        )
                    }
                };
                self.send(c, msg);
                if let Some(conn) = self.conns.get_mut(&c) {
                    if let Some(entry) = conn.scopes.get_mut(&s) {
                        entry.1 = advanced;
                    }
                }
            }
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
