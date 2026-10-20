//! Traces (`spec5/RUNNERS.md` § trace): one client's life as the inputs
//! its two machines were given and what each said, so that a client is
//! held to being a pure function of its inputs — replayed into fresh
//! machines, every step says the same, byte for byte.
//!
//! A trace is the client's [`Act`]s as its driver recorded them: both
//! machines opened afresh as a login, a sign-in, or one input to one
//! machine with its outputs and the machine's state after it. The driver's
//! policies — when it is linked, the relay, its disk, its views — are not
//! in it: what they decided is (the frames received, the calls made, the
//! durable state an open was given).
//!
//! The file is JSON: the modules (each space's schema and function bodies,
//! hex), the device, and one step per line.

use std::sync::Arc;

use ark5::hash::{hex, unhex};
use ark5::ir::Node;
use ark5::log::{Durable, Entry, EntryId, Logged, Origin, Snapshot};
use ark5::module::Lib;
use ark5::peer::{Cas, Input, Output, Peer, Role, Who};
use ark5::schema::{Schema, Space};
use ark5::store::Change;
use ark5_client::{Act, At, Client, Config};
use serde_json::{json, Value};

/// A space's machine as a trace names it: its schema's bytes and every
/// function body it holds.
fn module(schema: &Schema, cas: &Cas) -> Value {
    json!({"functions": cas.bodies().map(|b| hex(b)).collect::<Vec<_>>(), "schema": hex(&schema.to_bytes())})
}

fn who(w: &Who) -> Value {
    json!({"actor": w.actor, "roles": w.roles, "session": w.session})
}

fn space(s: Space) -> &'static str {
    match s {
        Space::Durable => "durable",
        Space::Volatile => "volatile",
    }
}

type Kept = ((Snapshot, Vec<Logged>), (Vec<Entry>, i64));

fn input(i: &Input) -> Value {
    match i {
        Input::Frame { bytes, .. } => json!({"t": "frame", "bytes": hex(bytes)}),
        Input::Mutate { func, input, now } => json!({"t": "mutate", "fn": func.hex(), "input": hex(input), "now": now}),
        Input::Connected => json!({"t": "connected"}),
        Input::Alone => json!({"t": "alone"}),
        Input::Join => json!({"t": "join"}),
        Input::Compact(n) => json!({"t": "compact", "seq": n}),
        Input::Open(d) => {
            let k: Kept = ((d.snapshot.clone(), d.chain.clone()), (d.pending.clone(), d.local));
            json!({"t": "open", "durable": hex(&k.to_bytes())})
        }
        Input::Tick(n) => json!({"t": "tick", "now": n}),
        Input::Persisted(n) => json!({"t": "persisted", "seq": n}),
        Input::Feed { .. } | Input::Loaded => unreachable!("a client is never told a feed or a load"),
    }
}

fn output(o: &Output) -> Value {
    match o {
        Output::Send(b) => json!({"t": "send", "bytes": hex(b)}),
        Output::Fanout(b) => json!({"t": "fanout", "bytes": hex(b)}),
        Output::To { session, bytes } => json!({"t": "to", "session": session, "bytes": hex(bytes)}),
        Output::Changes(cs) => json!({"t": "changes", "changes": hex(&cs.to_bytes())}),
        Output::Persist(b) => json!({"t": "persist", "bytes": hex(b)}),
        Output::Refused { id, why } => json!({"t": "refused", "id": hex(&id.0), "why": why}),
        Output::Confirmed(id) => json!({"t": "confirmed", "id": hex(&id.0)}),
    }
}

/// Where a machine stands after a step: its head, its state hash, its
/// pending intents.
fn state(a: &At) -> Value {
    let pending: Vec<String> = a.pending.iter().map(|x| hex(&x.0)).collect();
    json!({"hash": a.head.1.hex(), "head": a.head.0, "pending": pending, "state": a.state.hex()})
}

/// A client's recording as a trace file.
pub fn record(from: &str, c: &Client) -> String {
    let live = c.config.live.as_ref();
    let header = json!({
        "device": live.and_then(|l| l.device.as_ref()).map(|d| hex(d)),
        "from": from,
        "live": live.map(|l| module(&l.module.schema, &l.module.cas)),
        "log": module(&c.config.log.schema, &c.config.log.cas),
    });
    let steps: Vec<String> = c
        .record
        .iter()
        .flatten()
        .map(|(act, out, at)| {
            let step = match act {
                Act::Open(w, o, f, j) => {
                    let joining = j.clone().map(|(n, h, c)| hex(&((n, h), c).to_bytes()));
                    json!({"fork": f.map(|(n, h)| json!([n, h.hex()])), "joining": joining, "origin": hex(&o.0), "t": "open", "who": who(w)})
                }
                Act::SignIn(w, o) => json!({"origin": hex(&o.0), "t": "sign_in", "who": who(w)}),
                Act::Step(s, i) => {
                    let out: Vec<Value> = out.iter().map(output).collect();
                    json!({"after": at.as_ref().map(state), "in": input(i), "out": out, "space": space(*s), "t": "step"})
                }
            };
            step.to_string()
        })
        .collect();
    let mut head = header.to_string();
    head.pop();
    format!("{head},\"steps\":[\n{}\n]}}\n", steps.join(",\n"))
}

/// The machines a trace is replayed into.
pub struct Player {
    config: Config,
    log: Option<Peer>,
    live: Option<Peer>,
}

impl Player {
    pub fn new(config: Config) -> Player {
        Player {
            config,
            log: None,
            live: None,
        }
    }

    fn fresh(&self, who: &Who, origin: Origin, space: Space) -> Option<Peer> {
        let m = match space {
            Space::Durable => &self.config.log,
            Space::Volatile => &self.config.live.as_ref()?.module,
        };
        let mut p = Peer::new(m.schema.clone(), m.cas.clone(), Role::Replica, who.clone()).in_space(space);
        p.origin = origin;
        p.device = self
            .config
            .live
            .as_ref()
            .and_then(|l| l.device.clone())
            .filter(|_| space == Space::Volatile);
        Some(p)
    }

    pub fn peer(&self, s: Space) -> Option<&Peer> {
        match s {
            Space::Durable => self.log.as_ref(),
            Space::Volatile => self.live.as_ref(),
        }
    }

    /// One act; for a step, what the machine says.
    pub fn act(&mut self, a: &Act) -> Vec<Output> {
        match a {
            Act::Open(w, o, f, j) => {
                (self.log, self.live) = (self.fresh(w, *o, Space::Durable), self.fresh(w, *o, Space::Volatile));
                if let Some(l) = &mut self.log {
                    if f.is_some() {
                        l.role = ark5_client::alone_at(*f);
                    }
                    l.joining = j.clone();
                }
            }
            Act::SignIn(w, o) => {
                let mut same = true;
                if let Some(l) = &mut self.log {
                    same = l.origin == *o;
                    if !same {
                        (l.origin, l.local) = (*o, 0);
                    }
                    l.joining = l.joining.take().filter(|_| (&l.me.actor, &l.me.roles) == (&w.actor, &w.roles));
                    l.me = w.clone();
                }
                let local = self.live.as_ref().filter(|_| same).map_or(0, |x| x.local);
                self.live = self.fresh(w, *o, Space::Volatile);
                if let Some(x) = &mut self.live {
                    x.local = local;
                }
            }
            Act::Step(s, i) => {
                let p = match s {
                    Space::Durable => self.log.as_mut(),
                    Space::Volatile => self.live.as_mut(),
                };
                return p.expect("a machine opened").step(i.clone());
            }
        }
        vec![]
    }
}

// * Reading a trace back

fn bytes(v: &Value) -> Result<Vec<u8>, String> {
    v.as_str().and_then(unhex).ok_or_else(|| format!("not hex: {v}"))
}

fn origin_of(v: &Value) -> Result<Origin, String> {
    Ok(Origin(bytes(v)?.try_into().map_err(|_| "an origin is 16 bytes")?))
}

fn module_of(v: &Value) -> Result<ark5_client::Module, String> {
    let schema = Arc::new(Schema::from_bytes(&bytes(&v["schema"])?).map_err(|r| r.0)?);
    let lib = Lib::default();
    let mut cas = Cas::default().with_loader(lib.loader(schema.clone()));
    for b in v["functions"].as_array().ok_or("no functions")? {
        cas.add_body(bytes(b)?);
    }
    Ok(ark5_client::Module { schema, cas, lib })
}

fn who_of(v: &Value) -> Who {
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    let roles = v["roles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_str().map(String::from))
        .collect();
    Who {
        actor: s("actor"),
        session: s("session"),
        roles,
    }
}

fn hash<T>(v: &Value, f: impl Fn([u8; 32]) -> T) -> Result<T, String> {
    Ok(f(bytes(v)?.try_into().map_err(|_| "not 32 bytes")?))
}

fn input_of(v: &Value) -> Result<Input, String> {
    let n = |k: &str| v[k].as_i64().ok_or(format!("no {k}"));
    Ok(match v["t"].as_str().unwrap_or_default() {
        "frame" => Input::Frame {
            from: None,
            bytes: bytes(&v["bytes"])?,
        },
        "mutate" => Input::Mutate {
            func: hash(&v["fn"], ark5::hash::FnHash)?,
            input: bytes(&v["input"])?,
            now: n("now")?,
        },
        "connected" => Input::Connected,
        "alone" => Input::Alone,
        "join" => Input::Join,
        "compact" => Input::Compact(n("seq")?),
        "tick" => Input::Tick(n("now")?),
        "persisted" => Input::Persisted(n("seq")?),
        "open" => {
            let ((snapshot, chain), (pending, local)) = Kept::from_bytes(&bytes(&v["durable"])?).map_err(|r| r.0)?;
            Input::Open(Durable {
                snapshot,
                chain,
                pending,
                local,
            })
        }
        t => return Err(format!("no input {t}")),
    })
}

fn output_of(v: &Value) -> Result<Output, String> {
    let id = || -> Result<EntryId, String> { Ok(EntryId(bytes(&v["id"])?.try_into().map_err(|_| "not an id")?)) };
    Ok(match v["t"].as_str().unwrap_or_default() {
        "send" => Output::Send(bytes(&v["bytes"])?),
        "fanout" => Output::Fanout(bytes(&v["bytes"])?),
        "to" => Output::To {
            session: v["session"].as_str().unwrap_or_default().into(),
            bytes: bytes(&v["bytes"])?,
        },
        "changes" => Output::Changes(Vec::<Change>::from_bytes(&bytes(&v["changes"])?).map_err(|r| r.0)?),
        "persist" => Output::Persist(bytes(&v["bytes"])?),
        "refused" => Output::Refused {
            id: id()?,
            why: v["why"].as_str().unwrap_or_default().into(),
        },
        "confirmed" => Output::Confirmed(id()?),
        t => return Err(format!("no output {t}")),
    })
}

/// A trace replayed into fresh machines: the first step that says other
/// than it recorded, or stands elsewhere after, or none.
pub fn replay(text: &str) -> Result<usize, String> {
    let t: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let live = match &t["live"] {
        Value::Null => None,
        m => Some(ark5_client::Live {
            module: module_of(m)?,
            device: t["device"].as_str().map(|_| bytes(&t["device"])).transpose()?,
            ephemeral: Default::default(),
            every_ms: 0,
        }),
    };
    let mut p = Player::new(Config {
        log: module_of(&t["log"])?,
        live,
    });
    let steps = t["steps"].as_array().ok_or("no steps")?;
    for (n, s) in steps.iter().enumerate() {
        let act = match s["t"].as_str().unwrap_or_default() {
            "open" => {
                let f = &s["fork"];
                let fork = if f.is_null() {
                    None
                } else {
                    Some((f[0].as_i64().ok_or("no fork")?, hash(&f[1], ark5::hash::EntryHash)?))
                };
                let j = &s["joining"];
                let joining = if j.is_null() {
                    None
                } else {
                    Some(<((i64, ark5::hash::EntryHash), Vec<Logged>)>::from_bytes(&bytes(j)?).map_err(|r| r.0)?)
                };
                Act::Open(who_of(&s["who"]), origin_of(&s["origin"])?, fork, joining.map(|((n, h), c)| (n, h, c)))
            }
            "sign_in" => Act::SignIn(who_of(&s["who"]), origin_of(&s["origin"])?),
            _ => Act::Step(
                if s["space"] == "volatile" { Space::Volatile } else { Space::Durable },
                input_of(&s["in"])?,
            ),
        };
        let said = p.act(&act);
        let Act::Step(sp, _) = act else { continue };
        let want: Vec<Output> = s["out"].as_array().ok_or("no out")?.iter().map(output_of).collect::<Result<_, _>>()?;
        if said != want {
            return Err(format!("step {n}: said {said:?}, recorded {want:?}"));
        }
        if p.peer(sp).map(|x| state(&At::of(x))).as_ref() != Some(&s["after"]) {
            return Err(format!("step {n}: stands elsewhere than recorded"));
        }
    }
    Ok(steps.len())
}
