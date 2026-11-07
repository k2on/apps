//! The server: a core that is the authority of each space (§1.7, §1.9)
//! and the connections it serves, routed — sans-io like the machines, so
//! the simulator drives the same server a socket does.
//!
//! **Routing.** A frame from a connection goes to the machine of its space
//! with the connection's verified identity. What the machine says goes:
//! a frame `Send` back to that connection, or to all — every connection
//! fed the log, each through its feed, or, for the volatile space, every
//! connection that said `Hello` in it, each encoded once for those fed
//! whole and once for each fed by a scope; and what it says to keep to the
//! disk **first** — nothing a step says is delivered until its records are
//! written, and while a write fails every delivery waits with it, in
//! order, for a write that succeeds ([`Core::failure`]).
//!
//! **Feeds.** Each connection is fed as the app says of its login
//! (`ServerConfig::feed`): whole, or by a scope (§1.8) — the authority of
//! each space asks the same function (`ark::peer::Peer::feed_of`) as the
//! connection is accepted, and every page sent to all goes to it through
//! its scope (`ark::peer::Feed::fed`), once it has said `Hello`: the live
//! space is fed by the log's rules, its scope named in the live domain.
//!
//! **Devices** (§1.9). A connection may say what its device is, in a call
//! before its `Hello`s (`ark::frames::DEVICE`); the server authors the live domain's `on_join` on what it
//! said, as the connection's login, when a session's first such connection
//! says it, and `on_leave` when its last closes, so a device on two
//! connections is one device. On a restart the
//! volatile space is what the server wrote of it when it last closed
//! (its rows whole, one record beside its log: `local.volatile`,
//! and its journal, `local.live`), and `on_restore` is authored over it.
//!
//! **The journal** beside the log keeps what the log cannot, as one record
//! beside the log (`local.journal`): each origin's last
//! `local` and actor and the intents refused, written whole when the
//! machine says it moved — a refusal, or the horizon — so that after a
//! restart a push past a refusal is not
//! taken for a gap, a refused intent pushed again is answered again, and
//! one sequenced below the horizon is acknowledged there. **The horizon**
//! moves as `keep` says: the log keeps that many entries above its
//! snapshot.
//!
//! **Calls** ([`ServerConfig::calls`]) are no machine's: a connection's
//! call is kept for whoever drives the server ([`Authority::take_calls`]),
//! which runs it off the log's thread and answers that connection alone.

use std::collections::BTreeMap;
use std::sync::Arc;

use ark::format::Encode;
use ark::frames::Frame;
use ark::log::{EntryId, Origin, Record};
use ark::peer::{Ctx, Feed, Input, Journal, Output, Role, To};
use ark::schema::Space;
use ark::store::Change;
use ark::Domain;

use crate::{Config, Core, Disk, Fold, Scoping};

/// What a server is made of: the log's domain and the live space's (§1.16,
/// §1.9); how each connection is fed (§1.8); the entries each space keeps
/// above its horizon (all of them when none); the functions of its own a
/// device may call over its link (`calls`); and the largest file it takes
/// from a device, in bytes.
///
/// It authors the live domain's `on_join` (a function whose name's last
/// part is that) when a session's first connection says what its device
/// is — on what it said, as the connection's login — its `on_leave` when
/// the last closes, and its `on_restore` over what a restart kept.
#[derive(Clone)]
pub struct ServerConfig {
    pub log: Arc<Domain>,
    pub live: Option<Arc<Domain>>,
    /// The scope each connection is fed by (§1.8), by who it is; none,
    /// the log whole.
    pub feed: Scoping,
    pub keep: Option<usize>,
    pub live_keep: Option<usize>,
    /// The server's functions a device calls by name over its link,
    /// outside both machines ([`Frame::Call`]): never logged, journaled,
    /// recorded or told to another connection.
    pub calls: BTreeMap<String, Call>,
    pub max_file: u64,
}

/// One of a server's [`ServerConfig::calls`]: given the caller, verified
/// as its connection is, and the call's input; what it gives back, or why
/// not. It runs off the log's thread, on one of the few a server keeps
/// for its calls ([`crate::handle::CALLS`]), so it may take its time.
pub type Call = Arc<dyn Fn(&Ctx, &[u8]) -> Result<Vec<u8>, String> + Send + Sync>;

impl ServerConfig {
    /// A server of `log`, with no live space.
    pub fn of(log: Arc<Domain>) -> ServerConfig {
        ServerConfig {
            log,
            live: None,
            feed: Arc::new(|_| None),
            keep: None,
            live_keep: None,
            calls: BTreeMap::new(),
            max_file: crate::files::MAX_FILE,
        }
    }

    /// The live domain's function whose name's last part is `last`: what a
    /// server authors as devices come and go, and after a restart.
    fn hook(&self, last: &str) -> Option<String> {
        self.live.as_ref()?.fns.keys().find(|n| n.rsplit('.').next() == Some(last)).cloned()
    }
}

/// The server of a device's domains.
impl From<Config> for ServerConfig {
    fn from(c: Config) -> ServerConfig {
        ServerConfig {
            live: c.live,
            ..ServerConfig::of(c.log)
        }
    }
}

/// What a server keeps beside its log, each one record on its
/// disk: its volatile space as it last closed, the store's rows whole, and
/// the live journal then; and its journal, each origin's last `local` and
/// actor and what it refused.
const VOLATILE: &str = "volatile";
const LIVE: &str = "live";
const JOURNAL: &str = "journal";

/// Each origin's last `local`, and the actor it is bound to.
type Lasts = BTreeMap<Origin, (String, i64)>;

/// A record kept beside the log, read.
fn kept<T: Encode>(local: &mut Box<dyn Disk>, name: &str) -> Option<T> {
    T::from_bytes(&local.get(name).ok()??).ok()
}

/// What an authority refused, by id, as an older build kept it, and where
/// each stands, by origin and `local`, which it lacks.
type Ids = Vec<(EntryId, String)>;
type At = Vec<(Origin, i64)>;

/// A journal as it is kept beside its log (`local.journal`, or the live
/// space's, `local.live`): as an older build kept it — each origin's last
/// and what it refused, by id, in one order or the other — and, after, where
/// each refusal stands.
fn journal_bytes(j: &Journal, live: bool) -> Vec<u8> {
    let (ids, at): (Ids, At) = j.refused.iter().map(|(&(o, l), why)| ((EntryId::of(o, l), why.clone()), (o, l))).unzip();
    match live {
        false => ark::format::encode(&(&j.last, ids, at)),
        true => ark::format::encode(&(ids, &j.last, at)),
    }
    .expect("a journal serializes")
}

/// A journal as it was kept ([`journal_bytes`]): by an older build, its
/// refusals found among each origin's `local`s, up to its last.
fn journal(local: &mut Box<dyn Disk>, name: &str) -> Option<Journal> {
    let b = local.get(name).ok()??;
    let (last, ids, at): (Lasts, Ids, Option<At>) = match name == LIVE {
        false => <(Lasts, Ids, At)>::from_bytes(&b).map(|(l, i, a)| (l, i, Some(a))),
        true => <(Ids, Lasts, At)>::from_bytes(&b).map(|(i, l, a)| (l, i, Some(a))),
    }
    .or_else(|_| match name == LIVE {
        false => <(Lasts, Ids)>::from_bytes(&b).map(|(l, i)| (l, i, None)),
        true => <(Ids, Lasts)>::from_bytes(&b).map(|(i, l)| (l, i, None)),
    })
    .ok()?;
    let mut ids: BTreeMap<EntryId, String> = ids.into_iter().collect();
    let at = match at {
        Some(at) => at,
        None if ids.is_empty() => vec![],
        None => {
            let each = last.iter().flat_map(|(o, (_, n))| (1..=*n).map(move |l| (*o, l)));
            each.filter(|(o, l)| ids.contains_key(&EntryId::of(o, *l))).collect()
        }
    };
    let refused = at
        .into_iter()
        .filter_map(|(o, l)| Some(((o, l), ids.remove(&EntryId::of(o, l))?)))
        .collect();
    Some(Journal { last, refused })
}

pub type ConnId = u64;

/// A frame for a connection.
pub type Deliver = (ConnId, Vec<u8>);

/// Where a connection's frames go, on a server a thread drives
/// ([`crate::Handle::connect`]); false once it is gone.
pub type Sink = Box<dyn Fn(Vec<u8>) -> bool + Send>;

struct Conn {
    who: Ctx,
    hello: [bool; 2],
    /// How it is fed in each space: by a scope, or whole (§1.8).
    feed: [Option<Feed>; 2],
    device: Option<Vec<u8>>,
}

/// A call of one of the server's functions ([`ServerConfig::calls`]) a
/// connection made: who, verified as the connection is, and what.
#[derive(Clone, Debug)]
pub struct Called {
    pub conn: ConnId,
    pub who: Ctx,
    pub id: u64,
    pub name: String,
    pub input: Vec<u8>,
}

/// Who a server's authorities author as.
pub fn authority() -> Ctx {
    Ctx::who("authority", "authority", vec![])
}

/// A server: a core, the authority of each space, and the connections it
/// serves — who each is, how it is fed and whether it said its device —
/// the deliveries waiting for what is unwritten, the calls made and not
/// yet run, and where each connection's frames go, while a thread
/// delivers them.
pub struct Authority {
    pub(crate) core: Core,
    pub config: ServerConfig,
    conns: BTreeMap<ConnId, Conn>,
    devices: BTreeMap<String, (usize, Vec<u8>)>,
    next: ConnId,
    held: Vec<Deliver>,
    called: Vec<Called>,
    pub(crate) sinks: BTreeMap<ConnId, Sink>,
    /// What the log's entries changed since sign-in's watch last looked,
    /// kept while one watches (`crate::handle`).
    pub(crate) seen: Option<Vec<Change>>,
    /// Whether it stopped: its volatile space written for the next open.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) stopped: bool,
    /// The files offered it and wanted of it, and who may read which.
    pub(crate) transfers: crate::files::Server,
}

impl std::ops::Deref for Authority {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.core
    }
}

impl std::ops::DerefMut for Authority {
    fn deref_mut(&mut self) -> &mut Core {
        &mut self.core
    }
}

impl Authority {
    /// A server opened from what `disk` holds: the authority of each space;
    /// or why its log does not read.
    pub fn serve(config: impl Into<ServerConfig>, disk: Box<dyn Disk>, now: i64) -> Result<Authority, String> {
        let config = config.into();
        let (live, disk) = (config.live.as_ref(), crate::local::beside(disk));
        let (mut core, _) = Core::open(&config.log, live, Role::authority(), &authority(), disk, |_| {});
        let log = &mut core.log;
        log.fold = Fold::keep(config.keep);
        log.feed(config.feed.clone());
        let mut stored = log.stored();
        if let Some(e) = log.failure.take() {
            return Err(format!("the log on disk does not read: {e}"));
        }
        stored.extend(journal(&mut core.local, JOURNAL).map(Record::Journal));
        core.log.step(Input::Open(stored));
        if let Some(live) = &mut core.live {
            live.fold = Fold::keep(config.live_keep);
            live.feed(config.feed.clone());
        }
        let mut a = Authority {
            core,
            config,
            conns: BTreeMap::new(),
            devices: BTreeMap::new(),
            next: 1,
            held: vec![],
            called: vec![],
            sinks: BTreeMap::new(),
            seen: None,
            stopped: false,
            transfers: Default::default(),
        };
        if let (Some(Record::Rows(rows)), Some(live)) = (kept(&mut a.core.local, VOLATILE), &mut a.core.live) {
            let j = journal(&mut a.core.local, LIVE).map(Record::Journal);
            live.step(Input::Open([Record::Rows(rows)].into_iter().chain(j).collect()));
            if let Some(f) = a.config.hook("on_restore") {
                let _ = a.author(Space::Volatile, &f, vec![], now);
            }
        }
        Ok(a)
    }

    /// Writes the volatile space as it stands, whole, for the next open:
    /// its rows, its log's and its journal, so that the live authority
    /// opened again goes on judging each origin from where it was.
    pub fn close(&mut self) {
        let Some(live) = &mut self.core.live else { return };
        let (seq, hash) = live.log().head();
        let rows = Record::Rows(ark::log::standing(live.store.snapshot_rows(), seq, hash));
        let j = journal_bytes(live.journal().expect("the live authority's"), true);
        self.put_beside(VOLATILE, &rows.to_bytes());
        self.put_beside(LIVE, &j);
    }

    /// One input to one machine, from a connection or from itself, and
    /// what it says routed: a frame back to that connection, or to every
    /// connection it fans out to, each encoded there; its journal kept
    /// beside its log (its instance writes its log's); what the log's
    /// entries changed kept for sign-in's watch. The verdict it gave an
    /// intent of its own, if it gave one.
    fn route(&mut self, space: Space, from: Option<ConnId>, input: Input) -> Option<Result<(), String>> {
        let mut verdict = None;
        for o in self.core.step(space, from, input) {
            match o {
                Output::Send(To::Back, f) => self.held.extend(from.map(|c| (c, f.enc()))),
                Output::Send(To::All, f) => self.fanout(space, &f),
                Output::Persist(Record::Journal(j)) => self.put_beside(JOURNAL, &journal_bytes(&j, false)),
                Output::Persist(_) => {}
                Output::Changes(told) if space == Space::Durable => {
                    if let Some(seen) = &mut self.seen {
                        seen.extend(told.into_iter().map(|t| t.change));
                    }
                }
                Output::Changes(_) => {}
                Output::Settled(_, r) => {
                    verdict.get_or_insert(r);
                }
            }
        }
        verdict
    }

    /// An intent of its own of the function `func` on `space`'s machine
    /// (a device's arrival, sign-in's), and the verdict it was given as it
    /// was made.
    fn author(&mut self, space: Space, func: &str, input: Vec<u8>, now: i64) -> Result<(), String> {
        let (func, cause) = (func.into(), None);
        let verdict = self.route(space, None, Input::Mutate { func, input, now, cause });
        verdict.unwrap_or(Err("no verdict".into()))
    }

    /// An intent the authority authors itself at `now` of the function
    /// `name` on `input`: its id,
    /// or why it was refused as it was sequenced.
    pub fn mutate_at(&mut self, name: &str, input: impl crate::Arg, now: i64) -> Result<EntryId, String> {
        let (space, f, id) = self.intent(name)?;
        self.author(space, &f, input.bytes(), now).map(|_| id)
    }

    /// Facts the authority writes itself, raw: an entry naming no function,
    /// its input their encoding, judged by the store as any write is.
    pub fn write(&mut self, facts: Vec<Change>, now: i64) -> Result<(), String> {
        let input = ark::format::encode(&facts).expect("changes encode");
        self.author(Space::Durable, "", input, now)
    }

    /// A connection verified as `who`.
    pub fn accept(&mut self, who: Ctx) -> ConnId {
        let c = self.next;
        self.accept_as(c, who);
        c
    }

    /// A connection verified as `who`, numbered by the caller.
    pub fn accept_as(&mut self, c: ConnId, who: Ctx) {
        self.next = self.next.max(c + 1);
        let feed = [Space::Durable, Space::Volatile].map(|s| self.machine(s).and_then(|m| m.feed_of(&who)));
        let conn = Conn {
            who,
            hello: [false; 2],
            feed,
            device: None,
        };
        self.conns.insert(c, conn);
    }

    /// Every connection open, and who it is.
    pub fn connections(&self) -> impl Iterator<Item = (ConnId, &Ctx)> {
        self.conns.iter().map(|(c, conn)| (*c, &conn.who))
    }

    /// Who a connection is.
    pub(crate) fn who(&self, c: ConnId) -> Option<Ctx> {
        self.conns.get(&c).map(|conn| conn.who.clone())
    }

    /// A frame from a connection: what it is answered with, and what
    /// everyone else is told of it.
    pub fn frame(&mut self, c: ConnId, bytes: Vec<u8>, now: i64) -> Vec<Deliver> {
        let Some(conn) = self.conns.get_mut(&c) else { return vec![] };
        let Ok(f) = Frame::dec(&bytes) else { return vec![] };
        if let Frame::Call { name, input, .. } = &f {
            if name == ark::frames::EVENTS {
                // What a device says of itself, kept with what this server
                // says, each named by the login that sent it: a device says
                // which of its peers, the connection whose.
                let (actor, session) = (conn.who.actor.clone(), conn.who.session.clone());
                let events = Vec::<ark::frames::Event>::from_bytes(input).unwrap_or_default();
                let events = events.into_iter().map(|mut e| {
                    e.fields.push(("from".into(), format!("{actor} \u{b7} {session}")));
                    e
                });
                crate::observe::collect(events.collect());
                return vec![];
            }
            if name == ark::frames::DEVICE {
                let input = input.clone();
                self.arrive(c, &input, now);
                return self.release();
            }
        }
        if let Frame::Call { id, name, input } = f {
            // No machine's: whoever drives the server runs it
            // ([`Authority::take_calls`]) and answers this connection alone.
            let who = conn.who.clone();
            self.called.push(Called {
                conn: c,
                who,
                id,
                name,
                input,
            });
            return vec![];
        }
        if let Frame::Reply { .. } = f {
            return vec![];
        }
        if f.is_file() {
            return self.file_frame(c, f);
        }
        let space = f.space();
        if let Frame::Hello { .. } = f {
            conn.hello[space as usize] = true;
        }
        let from = Some(conn.who.clone());
        self.route(space, Some(c), Input::Frame { from, frame: f });
        self.release()
    }

    /// The calls connections made since, taken, each with the function
    /// it names, none when the server has no such function: for whoever
    /// drives the server to run and answer ([`Authority::reply`]).
    pub fn take_calls(&mut self) -> Vec<(Called, Option<Call>)> {
        let called = std::mem::take(&mut self.called);
        called
            .into_iter()
            .map(|c| {
                let f = self.config.calls.get(&c.name).cloned();
                (c, f)
            })
            .collect()
    }

    /// A call's answer, for the connection that made it if it is open still.
    pub fn reply(&mut self, c: ConnId, id: u64, answer: Result<Vec<u8>, String>) -> Vec<Deliver> {
        if !self.conns.contains_key(&c) {
            return vec![];
        }
        let reply = match answer {
            Ok(output) => ark::frames::Reply::Ok { output },
            Err(why) => ark::frames::Reply::Refused { why },
        };
        vec![(c, Frame::Reply { id, reply }.enc())]
    }

    /// A connection gone; the device's `on_leave` when it was its last.
    pub fn leave(&mut self, c: ConnId, now: i64) -> Vec<Deliver> {
        self.files_left(c);
        self.sinks.remove(&c);
        if let Some(Conn { who, device: Some(_), .. }) = self.conns.remove(&c) {
            let (n, input) = self.devices.get_mut(&who.session).expect("a device counted");
            *n -= 1;
            if *n == 0 {
                let input = input.clone();
                self.devices.remove(&who.session);
                if let Some(f) = self.config.hook("on_leave") {
                    self.author_as(&who, &f, input, now);
                }
            }
        }
        self.release()
    }

    /// Every connection of a session gone: what to deliver of it.
    pub fn revoke(&mut self, session: &str, now: i64) -> Vec<Deliver> {
        let conns: Vec<ConnId> = self.connections().filter(|(_, w)| w.session == session).map(|(c, _)| c).collect();
        conns.into_iter().flat_map(|c| self.leave(c, now)).collect()
    }

    fn arrive(&mut self, c: ConnId, said: &[u8], now: i64) {
        let Some(conn) = self.conns.get_mut(&c).filter(|c| c.device.is_none()) else {
            return;
        };
        conn.device = Some(said.to_vec());
        let who = conn.who.clone();
        let n = &mut self.devices.entry(who.session.clone()).or_insert((0, said.to_vec())).0;
        *n += 1;
        if let (1, Some(f)) = (*n, self.config.hook("on_join")) {
            self.author_as(&who, &f, said.to_vec(), now);
        }
    }

    /// The live space's `f` on `input`, authored by the authority as `who`:
    /// a device's arrival or leaving is its login's.
    fn author_as(&mut self, who: &Ctx, f: &str, input: Vec<u8>, now: i64) {
        let Some(live) = &mut self.core.live else { return };
        let was = std::mem::replace(&mut live.me, who.clone());
        let _ = self.author(Space::Volatile, f, input, now);
        if let Some(live) = &mut self.core.live {
            live.me = was;
        }
    }

    /// A frame sent to all, for every connection of its space: the log's
    /// fed whole; and those that said `Hello` in it, each through its scope
    /// (§1.8) — the live space follows the log's rules.
    fn fanout(&mut self, space: Space, f: &Frame) {
        let whole = f.enc();
        let to = self.conns.iter().filter_map(|(c, conn)| match &conn.feed[space as usize] {
            None if space == Space::Durable || conn.hello[1] => Some((*c, whole.clone())),
            Some(feed) if conn.hello[space as usize] => feed.fed(f).map(|f| (*c, f.enc())),
            _ => None,
        });
        let to: Vec<Deliver> = to.collect();
        self.held.extend(to);
    }

    /// Writes what is unwritten; then everything waiting is delivered, or,
    /// while a write fails, waits on with what comes after it.
    pub fn release(&mut self) -> Vec<Deliver> {
        match self.log.flush() {
            true => std::mem::take(&mut self.held),
            false => vec![],
        }
    }
}
