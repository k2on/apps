//! `docs/plan-phones.md` S2 A trace: one client's life in a session,
//! written down as its inputs and its outputs, so that a client in any
//! language is tested as a function of what it was given — with nothing
//! left to a scheduler, and no server in it: what the server said is
//! already the frames.
//!
//! The client is the engine's, [`Client`] over its [`Replica`], with what
//! it makes durable ([`Kept`]) and, alone, its own [`Authority`] — the
//! sans-io machine whose bytes the protocol pins and whose state a rebase
//! moves. Not `ark_client::Peer`: what a peer adds over it is a driver's
//! policy — a dial and a backoff on a clock, a verify after every settle,
//! ids and times drawn from the platform, a storage's own encoding — and
//! each of those is either a choice a phone may make otherwise or a thing
//! no trace can pin. Everything a trace's outputs depend on is in its
//! inputs; `tests/traces.rs` proves it by replaying every trace into a
//! fresh client ([`replay`]).
//!
//! A step is one input and what the client did after it. The inputs
//! ([`Input`]): opened fresh, or again from what it made durable; a
//! connection opened or lost; a mutation, with its author and the autos
//! the originating peer drew, frozen; a sign-in; the token the next
//! `hello` carries; a frame from the server, as its bytes; the end of a
//! pump (settle); a verify; going alone, and joining again; and views
//! hydrated. The outputs ([`Observer`]), after every input: the frames it
//! queued, as canonical bytes; a local refusal; for each view, what it
//! reports; and its state — cursor, log, pending ids, verdicts kept,
//! divergences, verify answers, a denial, the union it holds, the hash of
//! its confirmed store and of its view. A state field is written only
//! where it differs from the step before, and every one after an open,
//! so that a trace is the size of what happened. `spec/RUNNERS.md` § trace
//! is the contract a runner in another language reads.

use std::collections::{BTreeMap, BTreeSet};

use crate::canon::encode;
use crate::eval::{middleware, Args, Ctx, EvalFault};
use crate::hash::{closures, state_hash, Closure, FnHash};
use crate::ir::{module_from_value, module_value, FnKind, Module};
use crate::log::{Log, Seq, Snapshot};
use crate::peer::{local_commit, Authority, Changes, Journal, Replica};
use crate::protocol::{refusal_text, Client, ClientMsg, Mode, Received, ServerMsg};
use crate::sim::{client_bodies, Kept};
use crate::store::{MemoryStore, Refusal, Store};
use crate::value::{Id, Value};
use crate::view::{self, Env, Patch, View};

/// A view a trace keeps up to date: a query of the module, read with
/// these arguments by this author, hydrated over the client's view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewSpec {
    pub query: String,
    pub ctx: Ctx,
    pub args: Args,
}

/// One input to a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Opened at 0 over an empty store of the module's schema: holding the
    /// module's closures, every private block stripped, or none; fed whole
    /// or by facts; with the token its `hello`s carry.
    Open {
        mode: Mode,
        closures: bool,
        token: Option<String>,
    },
    /// Stopped and opened again from what it made durable ([`Kept`]): the
    /// confirmed store, the cursor, the log, the union, and the pending
    /// intents; the closures, the mode and the token as it had them.
    Reopen,
    Connected,
    Disconnected,
    /// [`Client::mutate`], and alone the local commit after it.
    Mutate {
        ctx: Ctx,
        function: String,
        fn_hash: FnHash,
        id: Id,
        autos: Args,
        args: Args,
    },
    /// [`Client::sign_in`].
    SignIn {
        ctx: Ctx,
        token: Option<String>,
    },
    /// The token the next `hello` carries: a login granted other roles.
    Token(Option<String>),
    /// A frame from the server, as its canonical bytes, read as a client
    /// reads the socket ([`ServerMsg::decode_for`]).
    Receive(Vec<u8>),
    /// [`Client::settle`]: the end of a pump.
    Settle,
    /// [`Client::verify_all`].
    Verify,
    /// `docs/plan-alone.md` §1 Server → alone: the fork where the replica
    /// stands, an authority over its confirmed store, what is pending
    /// sequenced locally ([`alone`]).
    Alone,
    /// Alone → server: the local history taken back out and re-queued as
    /// this author's ([`join`]).
    Join(Ctx),
    /// These views, in place of any held, hydrated over the client's view.
    Hydrate(Vec<ViewSpec>),
}

fn ctx_value(c: &Ctx) -> Value {
    Value::record(vec![
        ("user", Value::text(c.user.clone())),
        ("session", Value::text(c.session.clone())),
        ("roles", Value::from(c.roles.iter().map(|r| Value::text(r.clone())).collect::<Vec<_>>())),
    ])
}

fn ctx_of(v: &Value) -> Result<Ctx, String> {
    let m = strct(v, "a ctx")?;
    let text = |k: &str| match m.get(k) {
        Some(Value::Text(t)) => Ok(t.to_string()),
        other => Err(format!("ctx {k}: {other:?}")),
    };
    let roles = match m.get("roles") {
        None => BTreeSet::new(),
        Some(Value::List(rs)) => rs
            .iter()
            .map(|r| match r {
                Value::Text(t) => Ok(t.to_string()),
                other => Err(format!("a role: {other:?}")),
            })
            .collect::<Result<_, _>>()?,
        Some(other) => return Err(format!("ctx roles: {other:?}")),
    };
    Ok(Ctx::new(text("user")?, text("session")?).with_roles(roles))
}

fn token_value(t: &Option<String>) -> Value {
    t.as_ref().map_or(Value::Null, |t| Value::text(t.clone()))
}

fn strct<'a>(v: &'a Value, what: &str) -> Result<&'a BTreeMap<String, Value>, String> {
    match v {
        Value::Struct(m) => Ok(m),
        other => Err(format!("{what} is not a struct: {other:?}")),
    }
}

impl Input {
    /// The input as a trace writes it: `{"t": …}` and its fields.
    pub fn value(&self) -> Value {
        let t = |s: &str| ("t", Value::text(s));
        match self {
            Input::Open { mode, closures, token } => Value::record(vec![
                t("open"),
                ("mode", Value::text(if *mode == Mode::Whole { "whole" } else { "facts" })),
                ("closures", Value::Bool(*closures)),
                ("token", token_value(token)),
            ]),
            Input::Reopen => Value::record(vec![t("reopen")]),
            Input::Connected => Value::record(vec![t("connected")]),
            Input::Disconnected => Value::record(vec![t("disconnected")]),
            Input::Mutate {
                ctx,
                function,
                fn_hash,
                id,
                autos,
                args,
            } => Value::record(vec![
                t("mutate"),
                ("ctx", ctx_value(ctx)),
                ("function", Value::text(function.clone())),
                ("fn", Value::bytes(fn_hash.clone())),
                ("id", Value::Id(*id)),
                ("autos", Value::from(autos.clone())),
                ("args", Value::from(args.clone())),
            ]),
            Input::SignIn { ctx, token } => Value::record(vec![t("sign_in"), ("ctx", ctx_value(ctx)), ("token", token_value(token))]),
            Input::Token(token) => Value::record(vec![t("token"), ("token", token_value(token))]),
            Input::Receive(frame) => Value::record(vec![t("receive"), ("frame", Value::bytes(frame.clone()))]),
            Input::Settle => Value::record(vec![t("settle")]),
            Input::Verify => Value::record(vec![t("verify")]),
            Input::Alone => Value::record(vec![t("alone")]),
            Input::Join(ctx) => Value::record(vec![t("join"), ("ctx", ctx_value(ctx))]),
            Input::Hydrate(specs) => Value::record(vec![
                t("hydrate"),
                (
                    "views",
                    Value::from(
                        specs
                            .iter()
                            .map(|s| {
                                Value::record(vec![
                                    ("query", Value::text(s.query.clone())),
                                    ("ctx", ctx_value(&s.ctx)),
                                    ("args", Value::from(s.args.clone())),
                                ])
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
            ]),
        }
    }

    /// An input read back; `Err` names what is not one.
    pub fn from_value(v: &Value) -> Result<Input, String> {
        let m = strct(v, "an input")?;
        let get = |k: &str| m.get(k).ok_or_else(|| format!("an input without {k}: {v:?}"));
        let token = |k: &str| match get(k)? {
            Value::Null => Ok(None),
            Value::Text(t) => Ok(Some(t.to_string())),
            other => Err(format!("{k} is neither text nor null: {other:?}")),
        };
        let args = |k: &str| strct(get(k)?, k).cloned();
        let Value::Text(t) = get("t")? else {
            return Err(format!("an input whose t is not text: {v:?}"));
        };
        Ok(match &**t {
            "open" => Input::Open {
                mode: match get("mode")? {
                    Value::Text(m) if &**m == "whole" => Mode::Whole,
                    Value::Text(m) if &**m == "facts" => Mode::ByFacts,
                    other => return Err(format!("mode is neither whole nor facts: {other:?}")),
                },
                closures: match get("closures")? {
                    Value::Bool(b) => *b,
                    other => return Err(format!("closures is not a bool: {other:?}")),
                },
                token: token("token")?,
            },
            "reopen" => Input::Reopen,
            "connected" => Input::Connected,
            "disconnected" => Input::Disconnected,
            "mutate" => Input::Mutate {
                ctx: ctx_of(get("ctx")?)?,
                function: match get("function")? {
                    Value::Text(f) => f.to_string(),
                    other => return Err(format!("function is not text: {other:?}")),
                },
                fn_hash: match get("fn")? {
                    Value::Bytes(b) => b.to_vec(),
                    other => return Err(format!("fn is not bytes: {other:?}")),
                },
                id: match get("id")? {
                    Value::Id(i) => *i,
                    other => return Err(format!("id is not an id: {other:?}")),
                },
                autos: args("autos")?,
                args: args("args")?,
            },
            "sign_in" => Input::SignIn {
                ctx: ctx_of(get("ctx")?)?,
                token: token("token")?,
            },
            "token" => Input::Token(token("token")?),
            "receive" => match get("frame")? {
                Value::Bytes(b) => Input::Receive(b.to_vec()),
                other => return Err(format!("frame is not bytes: {other:?}")),
            },
            "settle" => Input::Settle,
            "verify" => Input::Verify,
            "alone" => Input::Alone,
            "join" => Input::Join(ctx_of(get("ctx")?)?),
            "hydrate" => {
                let Value::List(vs) = get("views")? else {
                    return Err("views is not a list".into());
                };
                Input::Hydrate(
                    vs.iter()
                        .map(|s| {
                            let f = strct(s, "a view")?;
                            Ok(ViewSpec {
                                query: match f.get("query") {
                                    Some(Value::Text(q)) => q.to_string(),
                                    other => return Err(format!("a view's query: {other:?}")),
                                },
                                ctx: ctx_of(f.get("ctx").ok_or("a view without ctx")?)?,
                                args: strct(f.get("args").ok_or("a view without args")?, "args")?.clone(),
                            })
                        })
                        .collect::<Result<_, String>>()?,
                )
            }
            other => return Err(format!("unknown input {other}")),
        })
    }
}

// ---------------------------------------------------------------------
// What a client does that is not a method of it: the durable copy, a
// re-open from it, alone and back. The simulation and a replay both call
// these, so a trace and the session it came from are one code path.

/// What a client's confirmed store moved by, applied to its durable copy
/// as a client's storage keeps it (§11.1): the store moved only by
/// [`Replica::take_confirmed`], the cursor, the log and the union beside it.
pub fn keep(kept: &mut Option<Kept>, r: &mut Replica) {
    let k = kept.get_or_insert_with(|| Kept {
        store: r.confirmed.clone(),
        cursor: r.cursor,
        log_id: r.log_id,
        partial: r.partial.clone(),
    });
    match r.take_confirmed() {
        Journal::Replaced => k.store = r.confirmed.clone(),
        Journal::Facts(moved) => {
            for (_, f) in moved {
                k.store.apply_changes(&f);
            }
        }
    }
    k.cursor = r.cursor;
    k.log_id = r.log_id;
    k.partial = r.partial.clone();
}

/// A client stopped and opened again from what it made durable, as a client
/// process starting does: the replica from the durable store, cursor, log
/// and union — a union kept is opened under the device's schema and says
/// so in its next `Hello` (`docs/plan-guards.md` D2) — with the pending
/// intents, the closures it held, its mode and its token. Unlinked.
pub fn reopen(old: &Client, k: Kept) -> Client {
    let schema = match &k.partial {
        Some(h) => crate::scope::device_schema(&old.schema, h),
        None => old.schema.clone(),
    };
    let mut r = Replica::open(schema, old.replica.bodies.clone(), k.store, k.cursor, old.replica.pending.clone());
    r.log_id = k.log_id;
    r.partial = k.partial;
    r.through = k.cursor;
    Client::open(r, old.mode, old.token.clone()).with_schema(old.schema.clone())
}

/// A client that is its own authority (`docs/plan-alone.md` §1): where it
/// forked — the log and cursor it last shared with a server — and the
/// authority over its local history since. The authority holds the state
/// at its head and the entries it sequenced, which a join takes back out.
#[derive(Clone, Debug)]
pub struct Alone {
    pub authority: Authority,
    pub fork: Seq,
    pub fork_log: Option<Id>,
}

/// Server → alone (`docs/plan-alone.md` §1, `ark_client::Peer::leave`):
/// the fork is where the replica stands, its log and cursor; the authority
/// starts from the confirmed store with nothing in its log, over the
/// closures the client holds, and is no server's — its private blocks are
/// not run (`docs/plan-guards.md` D3); the replica holds no named log from
/// here, and nothing of the server's it had not yet applied — a page past
/// a sequence that never came, an entry waiting on its facts: that is the
/// log it is leaving, paged again from the fork when it joins, and left in
/// the inbox it would be applied as the local history's own next sequence
/// (`arkc fuzz` found it the day clients went alone in its sessions); and
/// what is pending is sequenced, locally. A whole replica only: a union
/// has no authority of its own to run what it does not hold.
pub fn alone(c: &mut Client) -> Alone {
    let r = &mut c.replica;
    r.inbox.clear();
    r.through = r.cursor;
    let mut a = Authority::new(r.schema.clone(), r.bodies.clone());
    a.natives = r.natives.clone();
    a.log = Log {
        base: Snapshot {
            seq: r.cursor,
            store: MemoryStore::empty(r.schema.clone()),
            hash: vec![],
            log_id: None,
        },
        entries: BTreeMap::new(),
        ids: BTreeMap::new(),
        below: Default::default(),
    };
    a.store = r.confirmed.clone();
    let mut out = Alone {
        authority: a,
        fork: r.cursor,
        fork_log: r.log_id,
    };
    r.log_id = None;
    commit(&mut out, c);
    out
}

/// Alone, everything pending sequenced by the local authority
/// ([`local_commit`]).
pub fn commit(a: &mut Alone, c: &mut Client) {
    local_commit(&mut a.authority, &mut c.replica);
}

/// Alone → server (`docs/plan-alone.md` §1, `ark_client::Peer::join`): the
/// local history above the fork taken back out of the confirmed store and
/// every entry of it re-queued as pending, in order, as `who` — the same
/// ids, functions, autos and arguments ([`Replica::fork_back`]). The caller
/// connects.
pub fn join(c: &mut Client, a: Alone, who: &Ctx) {
    let (mut facts, mut requeue) = (vec![], vec![]);
    for (n, (e, f)) in a.authority.log.entries {
        if n > a.fork {
            let mut e = e;
            e.actor = who.user.clone();
            e.session = who.session.clone();
            requeue.push(e);
            facts.push(f);
        }
    }
    c.replica.fork_back(facts, a.fork, a.fork_log, requeue);
}

// ---------------------------------------------------------------------
// The outputs

struct Watched {
    spec: ViewSpec,
    /// `None` while its hydrate or its last push was refused: it is read
    /// again at the next rebuild.
    view: Option<View>,
}

/// What a client's step produced, read off it: what a recording writes
/// and a replay is held to. Holds the views the trace keeps and the state
/// the last step wrote, which is what lets a step write only what moved.
pub struct Observer {
    queries: BTreeMap<String, Closure>,
    views: Vec<Watched>,
    /// Per view, what a hydrate this step reported.
    events: Vec<Value>,
    last: BTreeMap<String, Value>,
}

fn patch_value(p: &Patch) -> Value {
    match p {
        Patch::Insert { at, node } => Value::record(vec![("t", Value::text("insert")), ("at", Value::int(*at as i64)), ("node", node.clone())]),
        Patch::Remove { at } => Value::record(vec![("t", Value::text("remove")), ("at", Value::int(*at as i64))]),
        Patch::Update { at, node } => Value::record(vec![("t", Value::text("update")), ("at", Value::int(*at as i64)), ("node", node.clone())]),
    }
}

fn fault_text(e: &EvalFault) -> String {
    match e {
        EvalFault::Verdict(r) => refusal_text(r),
        EvalFault::Bug(b) => format!("bug: {b:?}"),
    }
}

fn refused(why: String) -> Value {
    Value::record(vec![("refused", Value::text(why))])
}

impl Observer {
    /// An observer of a client of the module whose closures are `bodies`:
    /// its queries are what a view may be opened on.
    pub fn new(bodies: &BTreeMap<FnHash, Closure>) -> Observer {
        Observer {
            queries: bodies
                .values()
                .filter(|c| c.function.kind == FnKind::Query)
                .map(|c| (c.function.name.clone(), c.clone()))
                .collect(),
            views: vec![],
            events: vec![],
            last: BTreeMap::new(),
        }
    }

    /// The client was opened: the next step writes every state field.
    pub fn reset(&mut self) {
        self.last.clear();
    }

    // A view read over `st`: what it holds, and what it reports — its rows,
    // or the verdict that refused it.
    fn open_view(&self, spec: &ViewSpec, st: &MemoryStore) -> (Option<View>, Value) {
        let sch = st.schema();
        let Some(c) = self.queries.get(&spec.query) else {
            return (None, refused(format!("no query {}", spec.query)));
        };
        let Some(plan) = c.function.plan.clone() else {
            return (None, refused(format!("{} has no plan", spec.query)));
        };
        let (args, provided) = match middleware(sch, c, &spec.ctx, &spec.args, st) {
            Ok(x) => x,
            Err(e) => return (None, refused(fault_text(&e))),
        };
        let env = Env {
            helpers: c.helpers.clone(),
            ctx: spec.ctx.clone(),
            args,
            provided,
        };
        match view::hydrate(sch, &plan, env, st) {
            Ok(v) => {
                let rows = Value::record(vec![("rows", Value::from(v.rows()))]);
                (Some(v), rows)
            }
            Err(e) => (None, refused(fault_text(&e))),
        }
    }

    /// These views in place of any held, hydrated over `st`; what each
    /// reports is written by the next [`Observer::step`].
    pub fn hydrate(&mut self, specs: Vec<ViewSpec>, st: &MemoryStore) {
        self.views.clear();
        self.events.clear();
        for spec in specs {
            let (view, ev) = self.open_view(&spec, st);
            self.views.push(Watched { spec, view });
            self.events.push(ev);
        }
    }

    /// The step's outputs: `frames`, the client's queued frames as their
    /// canonical bytes, and `refused`, a local verdict on a mutation, both
    /// present only when there is something; `views`, what each view
    /// reported — rows when it was hydrated or the client rebuilt, the
    /// patches its changes made, a verdict, or `null` — present only when
    /// one did; and every state field that differs from the last step's.
    pub fn step(&mut self, c: &Client, frames: &[ClientMsg], changes: &Changes, refusal: Option<&Refusal>) -> Value {
        let mut out: BTreeMap<String, Value> = BTreeMap::new();
        if !frames.is_empty() {
            out.insert(
                "frames".into(),
                Value::from(frames.iter().map(|m| Value::bytes(encode(&m.to_value()))).collect::<Vec<_>>()),
            );
        }
        if let Some(r) = refusal {
            out.insert("refused".into(), Value::text(refusal_text(r)));
        }
        let st = &c.replica.view;
        self.events.resize(self.views.len(), Value::Null);
        match changes {
            Changes::Rebuilt => {
                for k in 0..self.views.len() {
                    let (view, ev) = self.open_view(&self.views[k].spec, st);
                    self.views[k].view = view;
                    self.events[k] = ev;
                }
            }
            Changes::Applied(cs) if !cs.is_empty() => {
                let sch = st.schema();
                for (w, ev) in self.views.iter_mut().zip(self.events.iter_mut()) {
                    let Some(v) = &mut w.view else { continue };
                    match view::push_all(sch, st, cs, v) {
                        Ok(ps) => *ev = Value::record(vec![("patches", Value::from(ps.iter().map(patch_value).collect::<Vec<_>>()))]),
                        Err(e) => {
                            w.view = None;
                            *ev = refused(fault_text(&e));
                        }
                    }
                }
            }
            Changes::Applied(_) => {}
        }
        let events = std::mem::take(&mut self.events);
        if events.iter().any(|e| !e.is_null()) {
            out.insert("views".into(), Value::from(events));
        }
        for (k, v) in state(c) {
            if self.last.get(k) != Some(&v) {
                out.insert(k.into(), v.clone());
                self.last.insert(k.into(), v);
            }
        }
        Value::from(out)
    }
}

/// The state a step is held to, field by field.
fn state(c: &Client) -> Vec<(&'static str, Value)> {
    let r = &c.replica;
    let opt_id = |i: &Option<Id>| i.map_or(Value::Null, Value::Id);
    vec![
        ("cursor", Value::Int(r.cursor)),
        ("log", opt_id(&r.log_id)),
        ("pending", Value::from(r.pending.iter().map(|e| Value::Id(e.id)).collect::<Vec<_>>())),
        (
            "rejections",
            Value::from(
                r.rejections
                    .iter()
                    .map(|(id, why)| Value::record(vec![("id", Value::Id(*id)), ("reason", Value::text(refusal_text(why)))]))
                    .collect::<Vec<_>>(),
            ),
        ),
        ("diverged", Value::from(r.diverged.iter().map(|n| Value::Int(*n)).collect::<Vec<_>>())),
        (
            "agreed",
            Value::from(
                c.agreed
                    .iter()
                    .map(|(n, ok)| Value::record(vec![("seq", Value::Int(*n)), ("ok", ok.map_or(Value::Null, Value::Bool))]))
                    .collect::<Vec<_>>(),
            ),
        ),
        ("denied", c.denied.as_ref().map_or(Value::Null, |d| Value::text(d.clone()))),
        (
            "union",
            match &r.partial {
                None => Value::Null,
                Some(h) => Value::from(
                    h.iter()
                        .map(|(t, cols)| (t.clone(), Value::from(cols.iter().map(|c| Value::text(c.clone())).collect::<Vec<_>>())))
                        .collect::<BTreeMap<_, _>>(),
                ),
            },
        ),
        ("hash", Value::bytes(state_hash(&r.confirmed))),
        ("view_hash", Value::bytes(state_hash(&r.view))),
    ]
}

// ---------------------------------------------------------------------
// Recording and replaying

/// One client's trace as it is recorded: its steps, input and output, and
/// what observes them.
pub struct Trace {
    pub steps: Vec<(Value, Value)>,
    pub observer: Observer,
}

impl Trace {
    pub fn new(bodies: &BTreeMap<FnHash, Closure>) -> Trace {
        Trace {
            steps: vec![],
            observer: Observer::new(bodies),
        }
    }
}

/// A trace as a file: where it came from, the module it is over —
/// embedded whole, private blocks and all, as the fleet's vectors carry
/// theirs — and one step per line; then any `extra` parts, already
/// printed (a `falsify/` trace's `"expect": "fail"`).
pub fn write(module: &Module, from: &str, steps: &[(Value, Value)], extra: &[(&str, String)]) -> String {
    use crate::json::{json, obj, quoted};
    let lines: Vec<String> = steps
        .iter()
        .map(|(i, o)| json(&Value::record(vec![("in", i.clone()), ("out", o.clone())])))
        .collect();
    let mut parts = vec![
        ("from", quoted(from)),
        ("module", json(&module_value(module))),
        ("steps", format!("[\n{}\n]", lines.join(",\n"))),
    ];
    parts.extend(extra.iter().cloned());
    obj(&parts)
}

/// A trace file read back: its module and its steps.
pub fn read(text: &str) -> Result<(Module, Vec<(Value, Value)>), String> {
    let v = crate::json::decode(text).map_err(|e| format!("{e:?}"))?;
    let m = strct(&v, "a trace")?;
    let module = module_from_value(m.get("module").ok_or("a trace without a module")?).map_err(|e| format!("module: {e}"))?;
    let Some(Value::List(steps)) = m.get("steps") else {
        return Err("a trace without steps".into());
    };
    let steps = steps
        .iter()
        .map(|s| {
            let f = strct(s, "a step")?;
            Ok((
                f.get("in").cloned().ok_or("a step without in")?,
                f.get("out").cloned().ok_or("a step without out")?,
            ))
        })
        .collect::<Result<_, String>>()?;
    Ok((module, steps))
}

/// A client driven by nothing but a trace's inputs: what a runner in any
/// language is, written in this one.
pub struct Player {
    module_schema: crate::schema::Schema,
    bodies: BTreeMap<FnHash, Closure>,
    client: Option<Client>,
    kept: Option<Kept>,
    alone: Option<Alone>,
    observer: Observer,
}

impl Player {
    pub fn new(m: &Module) -> Player {
        let bodies = closures(m);
        Player {
            module_schema: m.schema.clone(),
            observer: Observer::new(&bodies),
            bodies,
            client: None,
            kept: None,
            alone: None,
        }
    }

    /// One input, and what the client did after it.
    pub fn step(&mut self, input: &Input) -> Result<Value, String> {
        let mut refusal = None;
        if let Input::Open { mode, closures, token } = input {
            let bodies = if *closures { client_bodies(&self.bodies) } else { BTreeMap::new() };
            let st = MemoryStore::empty(self.module_schema.clone());
            let r = Replica::open(self.module_schema.clone(), bodies, st, 0, vec![]);
            self.client = Some(Client::open(r, *mode, token.clone()));
            self.kept = None;
            self.alone = None;
            self.observer.reset();
        }
        let c = self.client.as_mut().ok_or("an input before the client was opened")?;
        match input {
            Input::Open { .. } => {}
            Input::Reopen => {
                let k = self.kept.clone().ok_or("a reopen with nothing durable")?;
                *c = reopen(c, k);
                self.observer.reset();
            }
            Input::Connected => c.connected(),
            Input::Disconnected => c.disconnected(),
            Input::Mutate {
                ctx,
                fn_hash,
                id,
                autos,
                args,
                ..
            } => {
                if let Err(r) = c.mutate(*id, ctx, fn_hash, autos, args) {
                    refusal = Some(r);
                }
                if let Some(a) = &mut self.alone {
                    commit(a, c);
                }
            }
            Input::SignIn { ctx, token } => c.sign_in(ctx, token.clone()),
            Input::Token(t) => c.token = t.clone(),
            Input::Receive(bytes) => match ServerMsg::decode_for(bytes, &c.schema)? {
                Received::Snapshot(s) => c.recv_snapshot(s),
                Received::Msg(m) => c.recv(m),
            },
            Input::Settle => c.settle(),
            Input::Verify => c.verify_all(),
            Input::Alone => self.alone = Some(alone(c)),
            Input::Join(who) => {
                let a = self.alone.take().ok_or("a join while not alone")?;
                join(c, a, who);
            }
            Input::Hydrate(specs) => self.observer.hydrate(specs.clone(), &c.replica.view),
        }
        let frames = c.take_outgoing();
        let changes = c.replica.take_changes();
        keep(&mut self.kept, &mut c.replica);
        Ok(self.observer.step(c, &frames, &changes, refusal.as_ref()))
    }
}

/// Every step of a trace replayed into a fresh client and its outputs held
/// to the recorded ones, value for value — every frame byte for byte. The
/// first step that differs is named, with each field that does.
pub fn replay(m: &Module, steps: &[(Value, Value)]) -> Result<(), String> {
    let mut p = Player::new(m);
    for (k, (i, want)) in steps.iter().enumerate() {
        let input = Input::from_value(i).map_err(|e| format!("step {k}: {e}"))?;
        let got = p.step(&input).map_err(|e| format!("step {k} ({}): {e}", i.field("t")))?;
        if &got != want {
            return Err(format!("step {k} ({}): {}", i.field("t"), differs(want, &got)));
        }
    }
    Ok(())
}

/// The fields of two outputs that differ, each with both sides.
pub fn differs(want: &Value, got: &Value) -> String {
    let empty = BTreeMap::new();
    let (w, g) = match (want, got) {
        (Value::Struct(w), Value::Struct(g)) => (&**w, &**g),
        _ => (&empty, &empty),
    };
    let keys: BTreeSet<&String> = w.keys().chain(g.keys()).collect();
    keys.into_iter()
        .filter(|k| w.get(*k) != g.get(*k))
        .map(|k| {
            let show = |v: Option<&Value>| v.map_or("absent".to_string(), |v| crate::json::json(v).chars().take(400).collect());
            format!("{k}: recorded {}, replayed {}", show(w.get(k)), show(g.get(k)))
        })
        .collect::<Vec<_>>()
        .join("; ")
}
