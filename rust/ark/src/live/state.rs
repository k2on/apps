//! §14 Live state (`docs/plan-live.md`): a typed state machine per scope,
//! beside the log, on the one socket.
//!
//! A module declares a live state ([`crate::ir::State`]): its type, its
//! initial value, a scope — the keys of the instances a person holds, a
//! function of who they are — and transitions, each a pure function of the
//! state, the call's context and its input. An **instance** is one state
//! under one key (`listening` of `alice`). The server holds every instance
//! and sequences the calls on each, per instance, with a sequence of its
//! own that is not the log's; a client holds what the server last
//! confirmed of the instances it subscribes to, and its own calls not yet
//! answered, replayed on top — the log's rule, **confirmed, then pending
//! replayed on top**, without the log.
//!
//! What crosses the socket, beside the log's frames ([`crate::protocol`]):
//!
//! - a `hello` may declare this connection a **device** ([`DeviceDecl`]):
//!   an id that survives its reconnects, a kind, a name, whether it can
//!   play. A connection that declares one subscribes to the instances its
//!   scope gives it; one that declares none sees the bytes it always did.
//! - `call` ([`Call`]): run this transition on this instance with this
//!   input. An ordered call carries an id; an ephemeral one its counter.
//! - `applied` ([`Applied`]): to every subscriber, a call the server took —
//!   ordered, with its sequence, who made it, when, and the hash of the
//!   state it produced; or ephemeral, with the device's counter and no
//!   sequence or hash, to every subscriber but its author.
//! - `refused` ([`Refused`]): to its author alone, a call the server did
//!   not take, and why.
//! - `live_snapshot` ([`Snapshot`]): an instance whole, ephemeral slots
//!   included — on joining, on a gap in the sequence, on a hash that did not
//!   match.
//! - `resync`: a client's request for one.
//!
//! **Ordered and ephemeral** ([`crate::ir::ordering`]). A transition that
//! reads nothing of the state and writes only the caller's own slot of
//! ephemeral fields needs no order: the server relays it, coalesced and
//! rate-limited, and each replica takes a device's newest by its counter.
//! Every other transition is sequenced. Ephemeral fields are left out of
//! the state's hash and out of what the server keeps; a device's slots are
//! emptied when it leaves.
//!
//! **Time is the server's.** A transition reads `ctx.now`: the server's
//! clock when it sequences a call — stamped on the `applied` frame as `at`,
//! so every replica runs it at that time — and the device's own guess of it
//! when it previews one.
//!
//! Both machines here are sans-io, as every machine of this crate is: the
//! server's is handed the time, the client's is handed what arrived.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::authoring::NativeLive;
use crate::eval::{run_transition, Args, Ctx, EvalFault, LiveCtx};
use crate::hash::{live_closures, LiveClosure};
use crate::ir::ordering::Kind;
use crate::ir::{Module, ON_JOIN, ON_LEAVE, ON_RESTORE};
use crate::store::Refusal;
use crate::value::{FieldName, Id, Value};

use super::ConnId;

/// The most bytes an ephemeral slot's value may encode to: what a pointer
/// or a scrubber needs, and well short of what would make relaying one at
/// thirty a second a burden.
pub const SLOT_CAP: usize = 1024;

/// The least time between two ephemeral frames one connection's calls on
/// one instance are relayed at, in milliseconds: thirty a second. What
/// arrives faster is coalesced to the newest, never refused.
pub const RELAY_EVERY: i64 = 34;

/// How many ordered calls an instance remembers by id, so that one pushed
/// again — its `applied` lost with the connection — is answered with the
/// `applied` it was and not run twice.
pub const REMEMBERED: usize = 512;

// ---------------------------------------------------------------------
// Frames

/// One instance: a live state under one key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instance {
    pub state: String,
    pub key: String,
}

impl Instance {
    pub fn new(state: impl Into<String>, key: impl Into<String>) -> Instance {
        Instance {
            state: state.into(),
            key: key.into(),
        }
    }
}

/// What a connection declares itself as at `hello`: the device it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceDecl {
    /// What names it across reconnects: a login's session for a client, an
    /// entity for a speaker. Empty is the connection's session.
    pub id: String,
    pub kind: String,
    pub name: String,
    pub can_play: bool,
}

impl DeviceDecl {
    pub fn new(id: impl Into<String>, kind: impl Into<String>, name: impl Into<String>, can_play: bool) -> DeviceDecl {
        DeviceDecl {
            id: id.into(),
            kind: kind.into(),
            name: name.into(),
            can_play,
        }
    }

    /// On the wire, in a `hello`: `{id, kind, name, can_play}`.
    pub fn to_value(&self) -> Value {
        Value::record(vec![
            ("id", Value::text(self.id.clone())),
            ("kind", Value::text(self.kind.clone())),
            ("name", Value::text(self.name.clone())),
            ("can_play", Value::Bool(self.can_play)),
        ])
    }

    pub fn from_value(v: &Value) -> Result<DeviceDecl, String> {
        let m = strct(v, "a device")?;
        Ok(DeviceDecl {
            id: text(m, "id")?,
            kind: text(m, "kind")?,
            name: text(m, "name")?,
            can_play: boolean(m, "can_play")?,
        })
    }

    /// The device as a hook is handed it ([`crate::ir::device_ty`]), as the
    /// server verified its user.
    pub fn record(&self, user: &str) -> Value {
        Value::record(vec![
            ("can_play", Value::Bool(self.can_play)),
            ("id", Value::text(self.id.clone())),
            ("kind", Value::text(self.kind.clone())),
            ("name", Value::text(self.name.clone())),
            ("user", Value::text(user)),
        ])
    }
}

/// `call`: run a transition on an instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub instance: Instance,
    pub transition: String,
    pub input: Args,
    /// Names the call: what an `applied` and a `refused` answer, and what a
    /// call pushed again is recognised by.
    pub id: Id,
    /// An ephemeral call's counter: per device and instance, rising, set by
    /// the device. `None` for an ordered call.
    pub counter: Option<i64>,
}

/// How an `applied` call was taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Order {
    /// Sequenced: the instance's `seq`, the call it answers (`None` for a
    /// hook the server ran itself), its author, the server's time it ran
    /// at, and the hash of the state it produced ([`LiveClosure::value_hash`]).
    Ordered {
        seq: i64,
        id: Option<Id>,
        user: String,
        session: String,
        roles: BTreeSet<String>,
        at: i64,
        hash: Vec<u8>,
    },
    /// Relayed: the device's counter, which a replica takes only above the
    /// last it took from that device.
    Ephemeral { counter: i64 },
}

/// `applied`: a call the server took, for every subscriber to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub instance: Instance,
    pub transition: String,
    pub input: Args,
    /// The device that made it — for a hook, the device arriving or
    /// leaving.
    pub device: String,
    pub order: Order,
}

impl Applied {
    /// The sequence of an ordered one.
    pub fn seq(&self) -> Option<i64> {
        match &self.order {
            Order::Ordered { seq, .. } => Some(*seq),
            Order::Ephemeral { .. } => None,
        }
    }

    /// The context every replica runs it under.
    pub fn ctx(&self) -> LiveCtx {
        match &self.order {
            Order::Ordered {
                user, session, roles, at, ..
            } => LiveCtx {
                user: user.clone(),
                session: session.clone(),
                roles: roles.clone(),
                device: self.device.clone(),
                now: *at,
            },
            Order::Ephemeral { .. } => LiveCtx {
                device: self.device.clone(),
                ..LiveCtx::default()
            },
        }
    }
}

/// `refused`: a call the server did not take, to its author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub instance: Instance,
    pub id: Id,
    pub reason: String,
}

/// `live_snapshot`: an instance whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub instance: Instance,
    pub seq: i64,
    /// The state, ephemeral slots and all.
    pub value: Value,
    /// The last counter the server took from each device, for its
    /// ephemeral calls: where a replica starts taking them from. `counters`
    /// on the wire, present only when there is one.
    pub counters: BTreeMap<String, i64>,
}

/// What the server says of live state, to one connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Said {
    Applied(Applied),
    Refused(Refused),
    Snapshot(Snapshot),
}

// Wire forms. Every frame is a struct with a `t` tag, as every frame is;
// an instance is two fields, `live` (the state) and `key`.

fn instance_fields(i: &Instance) -> Vec<(&'static str, Value)> {
    vec![("live", Value::text(i.state.clone())), ("key", Value::text(i.key.clone()))]
}

fn tagged(t: &str, mut fields: Vec<(&'static str, Value)>) -> Value {
    fields.push(("t", Value::text(t)));
    Value::record(fields)
}

fn roles_value(rs: &BTreeSet<String>) -> Value {
    Value::from(rs.iter().map(|r| Value::text(r.clone())).collect::<Vec<_>>())
}

impl Call {
    pub fn to_value(&self) -> Value {
        let mut f = instance_fields(&self.instance);
        f.push(("transition", Value::text(self.transition.clone())));
        f.push(("input", Value::from(self.input.clone())));
        f.push(("id", Value::Id(self.id)));
        if let Some(c) = self.counter {
            f.push(("counter", Value::Int(c)));
        }
        tagged("call", f)
    }

    pub fn from_fields(m: &BTreeMap<FieldName, Value>) -> Result<Call, String> {
        Ok(Call {
            instance: instance_of(m)?,
            transition: text(m, "transition")?,
            input: strct(get(m, "input")?, "an input")?.clone(),
            id: ident(m, "id")?,
            counter: m.get("counter").map(|v| int(v, "counter")).transpose()?,
        })
    }
}

impl Applied {
    pub fn to_value(&self) -> Value {
        let mut f = instance_fields(&self.instance);
        f.push(("transition", Value::text(self.transition.clone())));
        f.push(("input", Value::from(self.input.clone())));
        f.push(("device", Value::text(self.device.clone())));
        match &self.order {
            Order::Ordered {
                seq,
                id,
                user,
                session,
                roles,
                at,
                hash,
            } => {
                f.push(("seq", Value::Int(*seq)));
                if let Some(id) = id {
                    f.push(("id", Value::Id(*id)));
                }
                f.push(("user", Value::text(user.clone())));
                f.push(("session", Value::text(session.clone())));
                if !roles.is_empty() {
                    f.push(("roles", roles_value(roles)));
                }
                f.push(("at", Value::Int(*at)));
                f.push(("hash", Value::bytes(hash.clone())));
            }
            Order::Ephemeral { counter } => f.push(("counter", Value::Int(*counter))),
        }
        tagged("applied", f)
    }

    pub fn from_fields(m: &BTreeMap<FieldName, Value>) -> Result<Applied, String> {
        let order = match (m.get("seq"), m.get("counter")) {
            (Some(seq), None) => Order::Ordered {
                seq: int(seq, "seq")?,
                id: m.get("id").map(|_| ident(m, "id")).transpose()?,
                user: text(m, "user")?,
                session: text(m, "session")?,
                roles: match m.get("roles") {
                    None => BTreeSet::new(),
                    Some(Value::List(rs)) if !rs.is_empty() => {
                        let rs: Vec<String> = rs
                            .iter()
                            .map(|r| match r {
                                Value::Text(t) => Ok(t.to_string()),
                                other => Err(format!("a role is text, not {other:?}")),
                            })
                            .collect::<Result<_, _>>()?;
                        if rs.windows(2).any(|w| w[0] >= w[1]) {
                            return Err("roles: not in ascending order, or repeated".into());
                        }
                        rs.into_iter().collect()
                    }
                    Some(_) => return Err("roles: a non-empty list, or no field".into()),
                },
                at: int(get(m, "at")?, "at")?,
                hash: match get(m, "hash")? {
                    Value::Bytes(b) => b.to_vec(),
                    other => return Err(format!("hash is bytes, not {other:?}")),
                },
            },
            (None, Some(c)) => {
                for k in ["id", "user", "session", "roles", "at", "hash"] {
                    if m.contains_key(k) {
                        return Err(format!("an ephemeral applied carries no {k}"));
                    }
                }
                Order::Ephemeral { counter: int(c, "counter")? }
            }
            _ => return Err("an applied carries a seq or a counter, and not both".into()),
        };
        Ok(Applied {
            instance: instance_of(m)?,
            transition: text(m, "transition")?,
            input: strct(get(m, "input")?, "an input")?.clone(),
            device: text(m, "device")?,
            order,
        })
    }
}

impl Refused {
    pub fn to_value(&self) -> Value {
        let mut f = instance_fields(&self.instance);
        f.push(("id", Value::Id(self.id)));
        f.push(("reason", Value::text(self.reason.clone())));
        tagged("refused", f)
    }

    pub fn from_fields(m: &BTreeMap<FieldName, Value>) -> Result<Refused, String> {
        Ok(Refused {
            instance: instance_of(m)?,
            id: ident(m, "id")?,
            reason: text(m, "reason")?,
        })
    }
}

impl Snapshot {
    pub fn to_value(&self) -> Value {
        let mut f = instance_fields(&self.instance);
        f.push(("seq", Value::Int(self.seq)));
        f.push(("value", self.value.clone()));
        if !self.counters.is_empty() {
            f.push((
                "counters",
                Value::Struct(Box::new(self.counters.iter().map(|(k, n)| (k.clone(), Value::Int(*n))).collect())),
            ));
        }
        tagged("live_snapshot", f)
    }

    pub fn from_fields(m: &BTreeMap<FieldName, Value>) -> Result<Snapshot, String> {
        let counters = match m.get("counters") {
            None => BTreeMap::new(),
            Some(v) => {
                let c = strct(v, "counters")?;
                if c.is_empty() {
                    return Err("no counters is written as no field".into());
                }
                c.iter()
                    .map(|(k, n)| Ok((k.clone(), int(n, "a counter")?)))
                    .collect::<Result<_, String>>()?
            }
        };
        Ok(Snapshot {
            instance: instance_of(m)?,
            seq: int(get(m, "seq")?, "seq")?,
            value: get(m, "value")?.clone(),
            counters,
        })
    }
}

/// `resync`'s value: a client asking for an instance whole.
pub fn resync_value(i: &Instance) -> Value {
    tagged("resync", instance_fields(i))
}

pub fn instance_of(m: &BTreeMap<FieldName, Value>) -> Result<Instance, String> {
    Ok(Instance {
        state: text(m, "live")?,
        key: text(m, "key")?,
    })
}

fn get<'a>(m: &'a BTreeMap<FieldName, Value>, k: &str) -> Result<&'a Value, String> {
    m.get(k).ok_or_else(|| format!("missing {k}"))
}

fn strct<'a>(v: &'a Value, what: &str) -> Result<&'a BTreeMap<FieldName, Value>, String> {
    match v {
        Value::Struct(m) => Ok(m),
        other => Err(format!("{what} is a struct, not {other:?}")),
    }
}

fn text(m: &BTreeMap<FieldName, Value>, k: &str) -> Result<String, String> {
    match get(m, k)? {
        Value::Text(t) => Ok(t.to_string()),
        other => Err(format!("{k} is text, not {other:?}")),
    }
}

fn boolean(m: &BTreeMap<FieldName, Value>, k: &str) -> Result<bool, String> {
    match get(m, k)? {
        Value::Bool(b) => Ok(*b),
        other => Err(format!("{k} is a bool, not {other:?}")),
    }
}

fn int(v: &Value, k: &str) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(format!("{k} is an int, not {other:?}")),
    }
}

fn ident(m: &BTreeMap<FieldName, Value>, k: &str) -> Result<Id, String> {
    match get(m, k)? {
        Value::Id(i) => Ok(*i),
        other => Err(format!("{k} is an id, not {other:?}")),
    }
}

// ---------------------------------------------------------------------
// Running

/// The live states of a module, and how each runs: natively where this
/// build holds the state's own Rust under the hash its module names
/// ([`NativeLive`]), through [`run_transition`] otherwise — exactly as a
/// mutator runs. Shared by the server and the client, so a transition means
/// one thing at both ends.
#[derive(Clone, Debug, Default)]
pub struct Runner {
    pub closures: BTreeMap<String, LiveClosure>,
    natives: BTreeMap<String, NativeLive>,
}

impl Runner {
    /// Every live state of `m`, interpreted.
    pub fn of_module(m: &Module) -> Runner {
        Runner {
            closures: live_closures(m),
            natives: BTreeMap::new(),
        }
    }

    /// Whether the module has any live state at all.
    pub fn is_empty(&self) -> bool {
        self.closures.is_empty()
    }

    /// Hold these natively; one whose hash is not its module's is dropped.
    pub fn hold(&mut self, natives: impl IntoIterator<Item = NativeLive>) {
        for n in natives {
            if self.closures.get(n.name()).is_some_and(|c| crate::hash::live_hash(c) == n.hash()) {
                self.natives.insert(n.name().to_string(), n);
            }
        }
    }

    fn closure(&self, state: &str) -> Result<&LiveClosure, EvalFault> {
        self.closures
            .get(state)
            .ok_or_else(|| EvalFault::Verdict(Refusal::Refused(format!("no live state {state}"))))
    }

    /// The kind of a transition.
    pub fn kind(&self, state: &str, transition: &str) -> Kind {
        self.closures.get(state).map_or(Kind::Ordered, |c| c.kind(transition))
    }

    /// The value an instance of `state` starts as.
    pub fn init(&self, state: &str) -> Result<Value, EvalFault> {
        crate::eval::constant(&self.closure(state)?.state.init)
    }

    /// The keys of the instances of `state` this author holds.
    pub fn keys(&self, state: &str, who: &Ctx) -> Result<Vec<String>, EvalFault> {
        crate::eval::scope_keys(self.closure(state)?, who)
    }

    /// The hash of an instance's value, ephemeral fields left out.
    pub fn hash(&self, state: &str, v: &Value) -> Vec<u8> {
        self.closures
            .get(state)
            .map_or_else(|| crate::sha256::sha256(&crate::canon::encode(v)), |c| c.value_hash(v))
    }

    /// The fields of `state` that are ephemeral.
    pub fn ephemeral(&self, state: &str) -> BTreeSet<FieldName> {
        self.closures.get(state).map(LiveClosure::ephemeral).unwrap_or_default()
    }

    /// §14 Run a transition — a hook by its reserved name — over `value`.
    /// Leaving empties the device's slot of every ephemeral field first,
    /// which is the engine's and no hook's to forget; and an ephemeral
    /// call's slot is held to [`SLOT_CAP`].
    pub fn run(&self, state: &str, transition: &str, value: &Value, ctx: &LiveCtx, input: &Args) -> Result<Value, EvalFault> {
        let c = self.closure(state)?;
        let cleared;
        let value = if transition == ON_LEAVE {
            cleared = empty_slots(&c.ephemeral(), value, &device_of(input));
            &cleared
        } else {
            value
        };
        let out = match self.natives.get(state) {
            Some(n) => n.run(transition, value, ctx, input),
            None => run_transition(c, transition, value, ctx, input),
        }?;
        if c.kind(transition) == Kind::Ephemeral {
            for f in c.ephemeral() {
                if let Value::Struct(m) = out.field(&f) {
                    if let Some(slot) = m.get(&ctx.device) {
                        let n = crate::canon::encode(slot).len();
                        if n > SLOT_CAP {
                            return Err(EvalFault::Verdict(Refusal::Refused(format!(
                                "{f}: a slot holds at most {SLOT_CAP} bytes, not {n}"
                            ))));
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

// The id of the device a hook is handed.
fn device_of(input: &Args) -> String {
    match input.get("device").map(|d| d.field("id")) {
        Some(Value::Text(t)) => t.to_string(),
        _ => String::new(),
    }
}

// `v` with `device`'s slot of every ephemeral field emptied.
fn empty_slots(eph: &BTreeSet<FieldName>, v: &Value, device: &str) -> Value {
    match v {
        Value::Struct(m) if !eph.is_empty() => {
            let mut m = m.clone();
            for f in eph {
                if let Some(Value::Struct(slots)) = m.get_mut(f) {
                    slots.remove(device);
                }
            }
            Value::Struct(m)
        }
        other => other.clone(),
    }
}

fn verdict_text(e: &EvalFault) -> String {
    match e {
        EvalFault::Verdict(r) => crate::protocol::refusal_text(r),
        EvalFault::Bug(b) => format!("bug: {b:?}"),
    }
}

// ---------------------------------------------------------------------
// The server's half

/// Who a connection is, as the server verified it at `hello`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    pub user: String,
    pub session: String,
    pub roles: BTreeSet<String>,
}

impl Who {
    fn ctx(&self) -> Ctx {
        Ctx::new(self.user.clone(), self.session.clone()).with_roles(self.roles.iter().cloned())
    }
}

/// One instance, as the server holds it.
#[derive(Clone, Debug)]
pub struct Held {
    pub value: Value,
    pub seq: i64,
    /// The devices standing in it, in the order they arrived, each with the
    /// connections it stands on and who it was verified as.
    pub devices: Vec<(DeviceDecl, String, BTreeSet<ConnId>)>,
    pub subscribers: BTreeSet<ConnId>,
    /// Ordered calls taken, by id, newest last ([`REMEMBERED`]).
    recent: VecDeque<(Id, Applied)>,
    /// The last counter taken from each device, for its ephemeral calls.
    pub counters: BTreeMap<String, i64>,
}

// One connection's standing.
#[derive(Clone, Debug)]
struct Stand {
    who: Who,
    device: DeviceDecl,
    instances: BTreeSet<Instance>,
}

// What one connection's ephemeral calls on one instance are relayed at: when
// last, and what waits — the newest per transition, in the order they came.
#[derive(Clone, Debug, Default)]
struct Relay {
    last: Option<i64>,
    waiting: Vec<Applied>,
}

/// §14 The server's live state: every instance, the connections standing
/// in them, the sequencer, and the relay. Sans-io: handed the time, it hands
/// back what to say to whom.
#[derive(Clone, Debug, Default)]
pub struct Desk {
    pub runner: Runner,
    pub instances: BTreeMap<Instance, Held>,
    stands: BTreeMap<ConnId, Stand>,
    relays: BTreeMap<(ConnId, Instance), Relay>,
    /// An instance's ordered part moved since [`Desk::take_moved`]: worth
    /// writing down.
    moved: bool,
}

/// What a desk says, to whom, in order.
pub type Out = Vec<(ConnId, Said)>;

impl Desk {
    pub fn new(runner: Runner) -> Desk {
        Desk { runner, ..Desk::default() }
    }

    /// Whether the module has live state.
    pub fn is_live(&self) -> bool {
        !self.runner.is_empty()
    }

    /// Whether a connection stands as a device here.
    pub fn stands(&self, c: ConnId) -> bool {
        self.stands.contains_key(&c)
    }

    /// The instances a connection is subscribed to.
    pub fn instances_of(&self, c: ConnId) -> Vec<Instance> {
        self.stands.get(&c).map(|s| s.instances.iter().cloned().collect()).unwrap_or_default()
    }

    /// An instance's ordered part moved since the last ask.
    pub fn take_moved(&mut self) -> bool {
        std::mem::take(&mut self.moved)
    }

    // The instance, opened from its init where nobody has held it.
    fn open(&mut self, i: &Instance) -> Result<&mut Held, String> {
        if !self.instances.contains_key(i) {
            let v = self.runner.init(&i.state).map_err(|e| verdict_text(&e))?;
            self.instances.insert(
                i.clone(),
                Held {
                    value: v,
                    seq: 0,
                    devices: vec![],
                    subscribers: BTreeSet::new(),
                    recent: VecDeque::new(),
                    counters: BTreeMap::new(),
                },
            );
        }
        Ok(self.instances.get_mut(i).expect("opened above"))
    }

    /// §14.4 A snapshot kept from before a restart: its ordered part at its
    /// sequence, opened with every ephemeral slot empty and the state's
    /// `on_restore` run over it as the hook it is, at `now`, with nobody
    /// present. A snapshot of a state the module no longer has, or of a type
    /// it no longer has, is the caller's to drop.
    pub fn restore(&mut self, i: Instance, seq: i64, ordered: Value, now: i64) {
        let eph = self.runner.ephemeral(&i.state);
        let v = crate::types::with_empty_slots(&eph, ordered);
        let ctx = LiveCtx { now, ..LiveCtx::default() };
        let (value, seq) = match self.runner.run(&i.state, ON_RESTORE, &v, &ctx, &Args::new()) {
            Ok(after) if after != v => (after, seq + 1),
            _ => (v, seq),
        };
        self.instances.insert(
            i,
            Held {
                value,
                seq,
                devices: vec![],
                subscribers: BTreeSet::new(),
                recent: VecDeque::new(),
                counters: BTreeMap::new(),
            },
        );
    }

    /// What a server keeps of every instance: its sequence and its ordered
    /// part, ephemeral fields left out.
    pub fn kept(&self) -> Vec<(Instance, i64, Value)> {
        self.instances
            .iter()
            .map(|(i, h)| (i.clone(), h.seq, crate::types::ordered_part(&self.runner.ephemeral(&i.state), &h.value)))
            .collect()
    }

    /// §14.1 A `hello` declaring a device: the connection subscribes to the
    /// instances every state's scope gives `who`, and stands in each as
    /// this device. A repeated `hello` on the same connection as the same
    /// device is the log paging, and changes nothing here.
    pub fn hello(&mut self, c: ConnId, who: Who, device: DeviceDecl, now: i64) -> Out {
        let device = DeviceDecl {
            id: if device.id.is_empty() { who.session.clone() } else { device.id },
            ..device
        };
        if let Some(s) = self.stands.get(&c) {
            if s.who == who && s.device == device {
                return vec![];
            }
        }
        let mut out = self.part(c, now);
        let mut instances = vec![];
        let ctx = who.ctx();
        for state in self.runner.closures.keys() {
            if let Ok(keys) = self.runner.keys(state, &ctx) {
                instances.extend(keys.into_iter().map(|k| Instance::new(state.clone(), k)));
            }
        }
        out.extend(self.stand(c, who, device, instances, now));
        out
    }

    /// §14.1 A server-side participant (a speaker a bridge stands in for):
    /// the connection subscribes to `instances` as this device, whoever's
    /// scope they are in. Its frames are the same as a client's.
    pub fn stand(&mut self, c: ConnId, who: Who, device: DeviceDecl, instances: Vec<Instance>, now: i64) -> Out {
        let mut out = vec![];
        let entry = self.stands.entry(c).or_insert_with(|| Stand {
            who: who.clone(),
            device: device.clone(),
            instances: BTreeSet::new(),
        });
        let new: Vec<Instance> = instances.into_iter().filter(|i| entry.instances.insert(i.clone())).collect();
        for i in new {
            let Ok(h) = self.open(&i) else { continue };
            let arrived = match h.devices.iter_mut().find(|(d, _, _)| d.id == device.id) {
                Some((_, _, conns)) => {
                    conns.insert(c);
                    false
                }
                None => {
                    h.devices.push((device.clone(), who.user.clone(), [c].into()));
                    true
                }
            };
            if arrived {
                let input: Args = [("device".to_string(), device.record(&who.user))].into();
                out.extend(self.sequence(&i, ON_JOIN, input, None, None, &who, &device.id, now));
            }
            let h = self.instances.get_mut(&i).expect("opened above");
            h.subscribers.insert(c);
            out.push((c, Said::Snapshot(snapshot_of(&i, h))));
        }
        out
    }

    /// §14.3 A connection is gone: it leaves every instance it stood in,
    /// and a device whose last connection this was leaves — its ephemeral
    /// slots emptied and the state's `on_leave` run, sequenced as any call.
    pub fn part(&mut self, c: ConnId, now: i64) -> Out {
        let Some(s) = self.stands.remove(&c) else { return vec![] };
        self.relays.retain(|(k, _), _| *k != c);
        let mut out = vec![];
        for i in s.instances {
            let Some(h) = self.instances.get_mut(&i) else { continue };
            h.subscribers.remove(&c);
            let mut gone = None;
            if let Some(at) = h.devices.iter().position(|(d, _, _)| d.id == s.device.id) {
                let (_, _, conns) = &mut h.devices[at];
                conns.remove(&c);
                if conns.is_empty() {
                    gone = Some(h.devices.remove(at));
                }
            }
            if let Some((d, user, _)) = gone {
                h.counters.remove(&d.id);
                let input: Args = [("device".to_string(), d.record(&user))].into();
                out.extend(self.sequence(&i, ON_LEAVE, input, None, None, &s.who, &d.id, now));
            }
        }
        out
    }

    /// `resync`: the instance whole, to a connection subscribed to it.
    pub fn resync(&mut self, c: ConnId, i: &Instance) -> Out {
        match self.instances.get(i) {
            Some(h) if h.subscribers.contains(&c) => vec![(c, Said::Snapshot(snapshot_of(i, h)))],
            _ => vec![],
        }
    }

    /// §14.2 A call from a connection standing as a device: run, at `now`,
    /// with who it is.
    pub fn call(&mut self, c: ConnId, call: Call, now: i64) -> Out {
        let refuse = |why: String| {
            vec![(
                c,
                Said::Refused(Refused {
                    instance: call.instance.clone(),
                    id: call.id,
                    reason: why,
                }),
            )]
        };
        let Some(s) = self.stands.get(&c).cloned() else {
            return refuse("no device declared on this connection".into());
        };
        if !s.instances.contains(&call.instance) {
            return refuse(format!("not subscribed to {} {}", call.instance.state, call.instance.key));
        }
        match self.runner.kind(&call.instance.state, &call.transition) {
            Kind::Ordered => {
                if matches!(call.transition.as_str(), ON_JOIN | ON_LEAVE | ON_RESTORE) {
                    return refuse(format!("{} is the server's to run", call.transition));
                }
                // One pushed again: answered as it was, to its author.
                if let Some(h) = self.instances.get(&call.instance) {
                    if let Some((_, a)) = h.recent.iter().find(|(id, _)| *id == call.id) {
                        return vec![(c, Said::Applied(a.clone()))];
                    }
                }
                self.sequence(
                    &call.instance,
                    &call.transition,
                    call.input.clone(),
                    Some(call.id),
                    Some(c),
                    &s.who,
                    &s.device.id,
                    now,
                )
            }
            Kind::Ephemeral => {
                let Some(counter) = call.counter else {
                    return refuse(format!("{}: an ephemeral call carries its counter", call.transition));
                };
                let Some(h) = self.instances.get(&call.instance) else {
                    return refuse("no such instance".into());
                };
                // Stale — a duplicate, or one from before a newer — is
                // nothing: the newest from a device wins.
                if h.counters.get(&s.device.id).is_some_and(|n| *n >= counter) {
                    return vec![];
                }
                let ctx = LiveCtx {
                    device: s.device.id.clone(),
                    ..LiveCtx::default()
                };
                match self.runner.run(&call.instance.state, &call.transition, &h.value, &ctx, &call.input) {
                    Err(e) => refuse(verdict_text(&e)),
                    Ok(v) => {
                        let h = self.instances.get_mut(&call.instance).expect("looked up above");
                        h.value = v;
                        h.counters.insert(s.device.id.clone(), counter);
                        let a = Applied {
                            instance: call.instance.clone(),
                            transition: call.transition.clone(),
                            input: call.input.clone(),
                            device: s.device.id.clone(),
                            order: Order::Ephemeral { counter },
                        };
                        self.relay(c, a, now)
                    }
                }
            }
        }
    }

    // An ordered call run on an instance and, taken, sequenced and told to
    // every subscriber; refused, told to its author. A hook is ordered and
    // has no author to refuse to: a hook that refuses moves nothing.
    #[allow(clippy::too_many_arguments)]
    fn sequence(
        &mut self,
        i: &Instance,
        transition: &str,
        input: Args,
        id: Option<Id>,
        author: Option<ConnId>,
        who: &Who,
        device: &str,
        now: i64,
    ) -> Out {
        let Some(h) = self.instances.get(i) else { return vec![] };
        let ctx = LiveCtx {
            user: who.user.clone(),
            session: who.session.clone(),
            roles: who.roles.clone(),
            device: device.to_string(),
            now,
        };
        match self.runner.run(&i.state, transition, &h.value, &ctx, &input) {
            Err(e) => match (id, author) {
                (Some(id), Some(c)) => vec![(
                    c,
                    Said::Refused(Refused {
                        instance: i.clone(),
                        id,
                        reason: verdict_text(&e),
                    }),
                )],
                _ => vec![],
            },
            Ok(v) => {
                // What waits to be relayed on this instance goes first: an
                // ephemeral frame never comes after an ordered one that
                // followed it.
                let mut out = self.flush(i, now);
                let hash = self.runner.hash(&i.state, &v);
                let h = self.instances.get_mut(i).expect("looked up above");
                h.seq += 1;
                h.value = v;
                let a = Applied {
                    instance: i.clone(),
                    transition: transition.to_string(),
                    input,
                    device: device.to_string(),
                    order: Order::Ordered {
                        seq: h.seq,
                        id,
                        user: who.user.clone(),
                        session: who.session.clone(),
                        roles: who.roles.clone(),
                        at: now,
                        hash,
                    },
                };
                if let Some(id) = id {
                    h.recent.push_back((id, a.clone()));
                    while h.recent.len() > REMEMBERED {
                        h.recent.pop_front();
                    }
                }
                out.extend(h.subscribers.iter().map(|c| (*c, Said::Applied(a.clone()))));
                self.moved = true;
                out
            }
        }
    }

    // An ephemeral call relayed to every subscriber but its author — now,
    // or, faster than [`RELAY_EVERY`], when the next is due, coalesced to the
    // newest per transition.
    fn relay(&mut self, c: ConnId, a: Applied, now: i64) -> Out {
        let i = a.instance.clone();
        let r = self.relays.entry((c, i.clone())).or_default();
        let due = r.last.is_none_or(|l| now - l >= RELAY_EVERY);
        if !due {
            r.waiting.retain(|w| w.transition != a.transition);
            r.waiting.push(a);
            return vec![];
        }
        r.last = Some(now);
        let mut out = vec![];
        let name = a.transition.clone();
        for w in std::mem::take(&mut r.waiting).into_iter().filter(|w| w.transition != name).chain([a]) {
            out.extend(self.to_others(c, w));
        }
        out
    }

    fn to_others(&self, author: ConnId, a: Applied) -> Out {
        match self.instances.get(&a.instance) {
            Some(h) => h
                .subscribers
                .iter()
                .filter(|s| **s != author)
                .map(|s| (*s, Said::Applied(a.clone())))
                .collect(),
            None => vec![],
        }
    }

    // Everything waiting to be relayed on an instance, now.
    fn flush(&mut self, i: &Instance, now: i64) -> Out {
        let keys: Vec<(ConnId, Instance)> = self.relays.keys().filter(|(_, j)| j == i).cloned().collect();
        let mut out = vec![];
        for k in keys {
            let waiting = match self.relays.get_mut(&k) {
                Some(r) if !r.waiting.is_empty() => {
                    r.last = Some(now);
                    std::mem::take(&mut r.waiting)
                }
                _ => continue,
            };
            for w in waiting {
                out.extend(self.to_others(k.0, w));
            }
        }
        out
    }

    /// When the next coalesced relay is due, if one waits.
    pub fn deadline(&self) -> Option<i64> {
        self.relays
            .values()
            .filter(|r| !r.waiting.is_empty())
            .filter_map(|r| r.last.map(|l| l + RELAY_EVERY))
            .min()
    }

    /// Relay what is due at `now`.
    pub fn tick(&mut self, now: i64) -> Out {
        let due: Vec<(ConnId, Instance)> = self
            .relays
            .iter()
            .filter(|(_, r)| !r.waiting.is_empty() && r.last.is_none_or(|l| now - l >= RELAY_EVERY))
            .map(|(k, _)| k.clone())
            .collect();
        let mut out = vec![];
        for k in due {
            let waiting = match self.relays.get_mut(&k) {
                Some(r) => {
                    r.last = Some(now);
                    std::mem::take(&mut r.waiting)
                }
                None => continue,
            };
            for w in waiting {
                out.extend(self.to_others(k.0, w));
            }
        }
        out
    }
}

fn snapshot_of(i: &Instance, h: &Held) -> Snapshot {
    Snapshot {
        instance: i.clone(),
        seq: h.seq,
        value: h.value.clone(),
        counters: h.counters.clone(),
    }
}

// ---------------------------------------------------------------------
// The client's half

/// An ordered call of this device's the server has not yet answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub id: Id,
    pub transition: String,
    pub input: Args,
    /// What the preview ran under, the time this device's guess at the
    /// server's when it was made.
    pub ctx: LiveCtx,
}

/// One instance as a client holds it: what the server last confirmed, at
/// its sequence, and the view — the confirmed state with this device's
/// pending calls replayed on top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watched {
    pub confirmed: Value,
    pub seq: i64,
    /// A snapshot has arrived on this connection: before one, nothing the
    /// server says of the instance is applied.
    pub synced: bool,
    /// A `resync` was asked and its snapshot has not arrived.
    pub asked: bool,
    pub pending: Vec<Pending>,
    pub view: Value,
    /// The last counter taken from each device for its ephemeral calls.
    pub counters: BTreeMap<String, i64>,
}

/// What a client's call came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Called {
    pub kind: Kind,
    /// The frame to send, if linked.
    pub call: Call,
}

/// §14 A client's live state: every instance it has heard of or called on,
/// and the device it declares itself as. Sans-io: what arrives is handed in,
/// and what to say is handed back.
#[derive(Clone, Debug, Default)]
pub struct Watch {
    pub runner: Runner,
    /// What this client's `hello` declares; `None`, and it is no device,
    /// subscribes to nothing, and its frames are the bytes they always were.
    pub device: Option<DeviceDecl>,
    pub instances: BTreeMap<Instance, Watched>,
    /// Verdicts the server gave this device's calls, oldest first, until
    /// taken.
    pub refused: Vec<(Id, String)>,
    /// Something here moved since [`Watch::take_moved`].
    moved: bool,
}

impl Watch {
    pub fn new(runner: Runner, device: Option<DeviceDecl>) -> Watch {
        Watch {
            runner,
            device,
            ..Watch::default()
        }
    }

    /// Something moved since the last ask: a view, a verdict.
    pub fn take_moved(&mut self) -> bool {
        std::mem::take(&mut self.moved)
    }

    /// The instance of `state` this author holds: its scope's first key.
    pub fn instance_of(&self, state: &str, who: &Ctx) -> Option<Instance> {
        let keys = self.runner.keys(state, who).ok()?;
        keys.into_iter().next().map(|k| Instance::new(state, k))
    }

    /// An instance's view: the confirmed state with pending calls on top.
    pub fn view(&self, i: &Instance) -> Option<&Value> {
        self.instances.get(i).map(|w| &w.view)
    }

    fn device_id(&self, who: &Ctx) -> String {
        match &self.device {
            Some(d) if !d.id.is_empty() => d.id.clone(),
            _ => who.session.clone(),
        }
    }

    fn watched(&mut self, i: &Instance) -> Result<&mut Watched, Refusal> {
        if !self.instances.contains_key(i) {
            let v = self.runner.init(&i.state).map_err(|e| Refusal::Refused(verdict_text(&e)))?;
            self.instances.insert(
                i.clone(),
                Watched {
                    confirmed: v.clone(),
                    seq: 0,
                    synced: false,
                    asked: false,
                    pending: vec![],
                    view: v,
                    counters: BTreeMap::new(),
                },
            );
        }
        Ok(self.instances.get_mut(i).expect("made above"))
    }

    /// §14.2 This device calls a transition: run at once over its view, so
    /// the screen moves under the thumb, with `now` its guess at the
    /// server's clock. Ordered, it is pending until the server answers;
    /// ephemeral, it is applied and done, `counter` its place among this
    /// device's. A refusal here is the preview's verdict, and nothing is
    /// sent.
    #[allow(clippy::too_many_arguments)]
    pub fn call(&mut self, who: &Ctx, i: &Instance, transition: &str, input: &Args, id: Id, now: i64, counter: i64) -> Result<Called, Refusal> {
        let kind = self.runner.kind(&i.state, transition);
        if kind == Kind::Ordered && matches!(transition, ON_JOIN | ON_LEAVE | ON_RESTORE) {
            return Err(Refusal::Refused(format!("{transition} is the server's to run")));
        }
        let ctx = LiveCtx {
            user: who.user.clone(),
            session: who.session.clone(),
            roles: who.roles.clone(),
            device: self.device_id(who),
            now,
        };
        let runner = self.runner.clone();
        let w = self.watched(i)?;
        let preview = runner.run(&i.state, transition, &w.view, &ctx, input).map_err(|e| match e {
            EvalFault::Verdict(r) => r,
            EvalFault::Bug(b) => Refusal::Refused(format!("bug: {b:?}")),
        })?;
        let call = match kind {
            Kind::Ephemeral => {
                if let Ok(v) = runner.run(&i.state, transition, &w.confirmed, &ctx, input) {
                    w.confirmed = v;
                }
                w.counters.insert(ctx.device.clone(), counter);
                Call {
                    instance: i.clone(),
                    transition: transition.into(),
                    input: input.clone(),
                    id,
                    counter: Some(counter),
                }
            }
            Kind::Ordered => {
                w.pending.push(Pending {
                    id,
                    transition: transition.into(),
                    input: input.clone(),
                    ctx,
                });
                Call {
                    instance: i.clone(),
                    transition: transition.into(),
                    input: input.clone(),
                    id,
                    counter: None,
                }
            }
        };
        w.view = preview;
        if kind == Kind::Ephemeral {
            replay(&runner, &i.state, w);
        }
        self.moved = true;
        Ok(Called { kind, call })
    }

    /// A connection opened: every instance waits for its snapshot, and
    /// every pending call is said again, in the order it was made.
    pub fn connected(&mut self) -> Vec<Call> {
        let mut out = vec![];
        for (i, w) in &mut self.instances {
            w.synced = false;
            w.asked = false;
            out.extend(w.pending.iter().map(|p| Call {
                instance: i.clone(),
                transition: p.transition.clone(),
                input: p.input.clone(),
                id: p.id,
                counter: None,
            }));
        }
        out
    }

    /// What the server said; the instances to ask a snapshot of — a gap in
    /// the sequence, or a state whose hash is not the server's.
    pub fn heard(&mut self, said: Said) -> Vec<Instance> {
        let runner = self.runner.clone();
        match said {
            Said::Snapshot(s) => {
                if runner.closures.contains_key(&s.instance.state) {
                    let w = self.instances.entry(s.instance.clone()).or_insert_with(|| Watched {
                        confirmed: s.value.clone(),
                        seq: s.seq,
                        synced: true,
                        asked: false,
                        pending: vec![],
                        view: s.value.clone(),
                        counters: BTreeMap::new(),
                    });
                    w.confirmed = s.value;
                    w.seq = s.seq;
                    w.synced = true;
                    w.asked = false;
                    w.counters = s.counters;
                    replay(&runner, &s.instance.state, w);
                    self.moved = true;
                }
                vec![]
            }
            Said::Refused(r) => {
                if let Some(w) = self.instances.get_mut(&r.instance) {
                    let before = w.pending.len();
                    w.pending.retain(|p| p.id != r.id);
                    if w.pending.len() != before {
                        self.refused.push((r.id, r.reason));
                        replay(&runner, &r.instance.state, w);
                        self.moved = true;
                    }
                }
                vec![]
            }
            Said::Applied(a) => {
                let Some(w) = self.instances.get_mut(&a.instance) else { return vec![] };
                if !w.synced {
                    return vec![];
                }
                match &a.order {
                    Order::Ephemeral { counter } => {
                        if w.counters.get(&a.device).is_some_and(|n| n >= counter) {
                            return vec![];
                        }
                        w.counters.insert(a.device.clone(), *counter);
                        if let Ok(v) = runner.run(&a.instance.state, &a.transition, &w.confirmed, &a.ctx(), &a.input) {
                            w.confirmed = v;
                            replay(&runner, &a.instance.state, w);
                            self.moved = true;
                        }
                        vec![]
                    }
                    Order::Ordered { seq, id, hash, .. } => {
                        // Already confirmed — the answer to a call pushed
                        // again — leaves pending what it answers.
                        if *seq <= w.seq {
                            if let Some(id) = id {
                                let before = w.pending.len();
                                w.pending.retain(|p| p.id != *id);
                                if w.pending.len() != before {
                                    replay(&runner, &a.instance.state, w);
                                    self.moved = true;
                                }
                            }
                            return vec![];
                        }
                        if *seq > w.seq + 1 {
                            return ask(w, &a.instance);
                        }
                        match runner.run(&a.instance.state, &a.transition, &w.confirmed, &a.ctx(), &a.input) {
                            Ok(v) if runner.hash(&a.instance.state, &v) == *hash => {
                                w.confirmed = v;
                                w.seq = *seq;
                                if let Some(id) = id {
                                    w.pending.retain(|p| p.id != *id);
                                }
                                replay(&runner, &a.instance.state, w);
                                self.moved = true;
                                vec![]
                            }
                            // Not the server's state: ask for it whole.
                            _ => ask(w, &a.instance),
                        }
                    }
                }
            }
        }
    }

    /// Verdicts on this device's calls since the last ask.
    pub fn take_refused(&mut self) -> Vec<(Id, String)> {
        std::mem::take(&mut self.refused)
    }
}

// A snapshot asked for, once until it arrives.
fn ask(w: &mut Watched, i: &Instance) -> Vec<Instance> {
    if w.asked {
        return vec![];
    }
    w.asked = true;
    vec![i.clone()]
}

// The view again: the confirmed state with every pending call replayed on
// top, in order, under what each was made with; one the state no longer
// takes is passed over, still pending, until the server answers it.
fn replay(runner: &Runner, state: &str, w: &mut Watched) {
    let mut v = w.confirmed.clone();
    for p in &w.pending {
        if let Ok(x) = runner.run(state, &p.transition, &v, &p.ctx, &p.input) {
            v = x;
        }
    }
    w.view = v;
}
