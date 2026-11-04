//! What an app drives (§1.10, §1.15): a device's functions called by
//! name, its views, and what became of what it did.
//!
//! An app authors ([`Peer::mutate`], on the space the function writes) and
//! asks by a function's name (`create_playlist`, or its whole binding,
//! `playlists.create_playlist`) — or calls the domain's own
//! view functions ([`Peer::answer`]) — and holds the
//! maintained views its lists are, each one's patches handed back as an
//! [`Update`]. The peer keeps every intent's [`Outcome`] and, per space,
//! the refusals its server sent until they are taken.
//!
//! **Nobody has to be signed in.** A device opened with no login
//! ([`Peer::open_as`]) authors as whoever last signed in on it — [`nobody`],
//! on a device where nobody ever did — and dials nothing; a login
//! ([`Peer::login`]) makes its pending intents the signer's, as
//! [`Peer::sign_in`] re-authors them, and dials. A device that joined a
//! server while alone hands its history over as intents pending under the
//! login that signs in there (§1.7).

use std::collections::BTreeMap;

use ark::domain::{Domain, Function};
use ark::format::Encode;
use ark::frames::Reply;
use ark::hash::StateHash;
use ark::log::EntryId;
use ark::peer::{Ctx, Who};
use ark::schema::Space;

use crate::{now_ms, Config, Disk, Patch, Peer};

/// What a function is called with: a value, or its encoding already (a
/// host's that speaks JSON, [`Peer::input_json`]).
pub trait Arg {
    fn bytes(self) -> Vec<u8>;
}

impl<T: Encode> Arg for &T {
    fn bytes(self) -> Vec<u8> {
        self.to_bytes()
    }
}

impl Arg for Vec<u8> {
    fn bytes(self) -> Vec<u8> {
        self
    }
}

/// What has become of an intent this device authored: pending, taken by
/// the authority, or refused by it and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Ok,
    Refused(String),
}

/// What one view did since it was last asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Unchanged,
    Patched(Vec<Patch>),
    /// It could not be kept up and was made again, under its number: read
    /// its rows afresh.
    Reset,
}

impl Update {
    /// `patches` after what this holds already; a reset stays one.
    pub fn patch(&mut self, patches: Vec<Patch>) {
        match self {
            u @ Update::Unchanged => *u = Update::Patched(patches),
            Update::Patched(ps) => ps.extend(patches),
            Update::Reset => {}
        }
    }
}

/// What became of a device's own intents, and what its views did.
#[derive(Default)]
pub(crate) struct Told {
    /// What the authority said of each intent on the log, and the
    /// refusals of each space not yet taken.
    outcomes: BTreeMap<EntryId, Outcome>,
    pub refused: [Vec<(EntryId, String)>; 2],
    pub updates: BTreeMap<usize, Update>,
    /// The calls of its server's functions sent and not yet answered, the
    /// next one's id, and the answers not yet taken.
    asking: std::collections::BTreeSet<u64>,
    calls: u64,
    replies: BTreeMap<u64, Result<Vec<u8>, String>>,
}

impl Told {
    /// A call sent: its id.
    pub fn asked(&mut self) -> u64 {
        self.calls += 1;
        self.asking.insert(self.calls);
        self.calls
    }

    /// A call's answer; one nobody asked for is dropped.
    pub fn replied(&mut self, id: u64, reply: Reply) {
        if self.asking.remove(&id) {
            let r = match reply {
                Reply::Ok { output } => Ok(output),
                Reply::Refused { why } => Err(why),
            };
            self.replies.insert(id, r);
        }
    }

    /// Every call still waiting, answered `why` not.
    pub fn unanswered(&mut self, why: &str) {
        for id in std::mem::take(&mut self.asking) {
            self.replies.insert(id, Err(why.into()));
        }
    }

    pub fn refused(&mut self, space: Space, id: EntryId, why: &str) {
        if space == Space::Durable {
            self.outcomes.insert(id, Outcome::Refused(why.into()));
        }
        self.refused[space as usize].push((id, why.into()));
    }

    pub fn confirmed(&mut self, space: Space, id: EntryId) {
        if space == Space::Durable {
            self.outcomes.insert(id, Outcome::Ok);
        }
    }

    pub fn patched(&mut self, view: usize, patches: Vec<Patch>) {
        if patches.is_empty() {
            return;
        }
        self.updates.entry(view).or_insert(Update::Unchanged).patch(patches);
    }

    pub fn reset(&mut self, view: usize) {
        self.updates.insert(view, Update::Reset);
    }
}

/// Where a device stands, as a status line says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub link: String,
    pub linked: bool,
    /// How many connections have opened.
    pub epoch: u64,
    pub cursor: i64,
    pub pending: usize,
    pub user: String,
    pub alone: bool,
}

/// Nobody, on a session of this device's own: who a device nobody ever
/// signed in on authors as.
pub fn nobody() -> Who {
    let here = 0u8;
    let seed = format!("{}:{}:{:p}", now_ms(), crate::monotonic_ms(), &here);
    let id = ark::hash::hex(&ark::hash::sha256(&[seed.as_bytes()])[..8]);
    Who {
        actor: String::new(),
        session: format!("device-{id}"),
        roles: vec![],
    }
}

impl Peer {
    /// The device `disk` holds, as whoever last signed in on it — nobody
    /// ([`nobody`]) on a disk nobody ever signed in on — and then, given a
    /// login and its token, as that login, its pending intents made the
    /// login's when they were somebody else's.
    pub fn open_as(config: Config, disk: Box<dyn Disk>, login: Option<(Who, String)>) -> Peer {
        let files = disk.files();
        let (disk, mut local) = crate::local::beside(disk);
        let kept = local.read::<crate::Local, _>(crate::local::replica).unwrap_or_default();
        let me = match kept.actor {
            Some(actor) => Who {
                actor,
                session: kept.session,
                roles: vec![],
            },
            None => {
                let me = nobody();
                let (actor, session) = (Some(me.actor.clone()), me.session.clone());
                let _ = local.write::<crate::Local, ark::db::Durable, _>(|db| db.replica.put(crate::local::Replica { actor, session, ..kept }));
                me
            }
        };
        let mut p = Peer::opened(config, me, disk, local, crate::link::Link::default());
        p.keep_files(files);
        match login {
            Some((who, token)) if (&who.actor, &who.session) != (&p.log.me.actor, &p.log.me.session) => p.login(who, token),
            Some((who, token)) => {
                p.set_roles(who.roles);
                p.link.token = Some(token);
            }
            None => {}
        }
        p
    }

    /// Somebody signed in: every pending intent theirs, and the connection
    /// dialled with their token.
    pub fn login(&mut self, who: Who, token: String) {
        let (actor, session) = (Some(who.actor.clone()), who.session.clone());
        self.replica(|r| (r.actor, r.session) = (actor, session));
        self.set_token(None);
        self.sign_in(who);
        self.set_token(Some(token));
    }

    pub fn is_signed_out(&self) -> bool {
        self.link.token.is_none()
    }

    /// The roles this login holds, which every intent it authors carries:
    /// a write is refused here as the server would refuse it.
    pub fn set_roles(&mut self, roles: impl IntoIterator<Item = impl Into<String>>) {
        let roles: Vec<String> = roles.into_iter().map(Into::into).collect();
        self.log.me.roles.clone_from(&roles);
        if let Some(l) = &mut self.live {
            l.me.roles = roles;
        }
    }

    pub fn me(&self) -> &Who {
        &self.log.me
    }

    /// Who a query reads as, and a view is kept for.
    pub fn ctx(&self) -> Ctx {
        let me = self.me();
        Ctx {
            id: EntryId::of(&me.session, 0),
            ..Ctx::new(me, now_ms())
        }
    }

    pub fn domain(&self) -> &Domain {
        &self.config.log
    }

    /// The function a name means ([`Domain::binding`]) and the domain it
    /// is of: the log's, else the live space's.
    fn found(&self, name: &str) -> Result<(&Domain, &Function), String> {
        let live = self.config.live.as_ref().map(|l| &*l.domain);
        [&*self.config.log]
            .into_iter()
            .chain(live)
            .find_map(|d| Some((d, d.get(d.binding(name)?)?)))
            .ok_or_else(|| format!("no function {name}"))
    }

    /// The function a name means, of the log's domain or the live space's:
    /// its whole name, its kind and space, what it takes and answers.
    pub fn function(&self, name: &str) -> Result<&Function, String> {
        Ok(self.found(name)?.1)
    }

    /// A query's or a view's answer over the optimistic state, as this
    /// login reads it: its encoding.
    pub fn ask(&self, name: &str, input: &[u8]) -> Result<Vec<u8>, String> {
        let b = &self.function(name)?.name;
        self.domain().ask(&self.log.store, &self.ctx(), b, input).map_err(|r| r.0)
    }

    /// An intent of the function `name` (its binding, or the binding's last
    /// part) on `input` — a value, or its encoding — on the space the
    /// function writes, now: its id, or the refusal its preview (or, alone
    /// or serving, its sequencing) gave it here and now. A live one the
    /// config relays (`Live::ephemeral`) goes through the relay, under the
    /// id it is sent with when nothing goes first.
    pub fn mutate(&mut self, name: &str, input: impl Arg) -> Result<EntryId, String> {
        self.mutate_at(name, input, now_ms())
    }

    /// [`Peer::mutate`] at `now`: a simulator's clock.
    pub fn mutate_at(&mut self, name: &str, input: impl Arg, now: i64) -> Result<EntryId, String> {
        let f = self.function(name)?;
        let (space, f) = (f.space, f.name.clone());
        let id = self.machine(space).ok_or_else(|| format!("no {space:?} machine here"))?.next_id();
        if let Err(why) = self.author(space, &f, input.bytes(), now) {
            // Said here, as the answer; not again among the refusals.
            self.told.refused[space as usize].retain(|(r, _)| *r != id);
            return Err(why);
        }
        self.flush();
        Ok(id)
    }

    /// A function's input — of the log's domain or the live one's — from its
    /// JSON (§1.1's human form), as the value format writes it: what a host
    /// that speaks JSON authors and asks with.
    pub fn input_json(&self, name: &str, text: &str) -> Result<Vec<u8>, String> {
        let (d, f) = self.found(name)?;
        d.input_json(&f.name, text).map_err(|r| r.0)
    }

    /// What [`Peer::ask`] answered, as JSON.
    pub fn answer_json(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let (d, f) = self.found(name)?;
        d.answer_json(&f.name, bytes).map_err(|r| r.0)
    }

    /// One row of a view of `name`, as JSON.
    pub fn element_json(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let (d, f) = self.found(name)?;
        d.element_json(&f.name, bytes).map_err(|r| r.0)
    }

    /// A row of a table of either space, as JSON.
    pub fn row_json(&self, table: &str, bytes: &[u8]) -> Result<String, String> {
        let live = self.config.live.as_ref().map(|l| &*l.domain);
        let mut ds = [&*self.config.log].into_iter().chain(live);
        let d = ds.find(|d| d.schema.table(table).is_some()).ok_or_else(|| format!("no table {table}"))?;
        d.row_json(table, bytes).map_err(|r| r.0)
    }

    /// A query's answer over the optimistic state, as this login reads it.
    pub fn query<T: Encode>(&self, name: &str, input: &impl Encode) -> Result<T, String> {
        let bytes = self.ask(name, &input.to_bytes())?;
        T::from_bytes(&bytes).map_err(|r| format!("{name}: {r}"))
    }

    /// A view function of the domain in Rust (§1.16) answered afresh over
    /// the optimistic state, as this login reads it: the domain's own
    /// function, called as it is.
    pub fn answer<S: ark::db::Tables, I, T: ark::plan::Elem>(&self, f: ark::domain::ViewFn<S, I, T>, input: I) -> Result<Vec<T>, String> {
        let ctx = self.ctx();
        let plan = ark::db::read::<S, _>(&self.log.store, |db| f(&ctx, db, input)).map_err(|r| r.0)?;
        Ok(plan.run(&self.log.store, &ctx))
    }

    /// A maintained view of a query (§1.10) over the optimistic state, as
    /// this login reads it: its number, or the query's refusal.
    pub fn view(&mut self, name: &str, input: &[u8]) -> Result<usize, String> {
        let (ctx, b) = (self.ctx(), self.function(name)?.name.clone());
        self.watch(Space::Durable, &b, input, ctx).map_err(|r| r.0)
    }

    /// What a view did since it was last asked.
    pub fn update(&mut self, view: usize) -> Update {
        self.told.updates.remove(&view).unwrap_or(Update::Unchanged)
    }

    /// What became of the intent `id` on the log: none for one this run
    /// did not author, or of a run before.
    pub fn outcome(&self, id: &EntryId) -> Option<Outcome> {
        if self.log.pending.iter().any(|p| p.entry.id == *id) {
            return Some(Outcome::Pending);
        }
        self.told.outcomes.get(id).cloned()
    }

    /// What the authority of `space` refused of this device's intents since
    /// the last ask: each one's id, and why.
    pub fn take_refusals(&mut self, space: Space) -> Vec<(EntryId, String)> {
        std::mem::take(&mut self.told.refused[space as usize])
    }

    /// What the server answered a call ([`Peer::request`]), once it has:
    /// what its function gave back, or why not. Taken, so it is told once.
    pub fn take_reply(&mut self, id: u64) -> Option<Result<Vec<u8>, String>> {
        self.told.replies.remove(&id)
    }

    /// How many calls' answers wait to be taken: a driver tells its app when
    /// this grows.
    pub fn replies_waiting(&self) -> usize {
        self.told.replies.len()
    }

    pub fn is_alone(&self) -> bool {
        self.log.is_authority()
    }

    pub fn pending_len(&self) -> usize {
        self.log.pending.len()
    }

    /// The head of the confirmed chain.
    pub fn cursor(&self) -> i64 {
        self.log.log().head().0
    }

    /// The confirmed and pending state's hash: what moves when the store
    /// does.
    pub fn state(&self) -> StateHash {
        self.log.store.state_hash()
    }

    pub fn status(&self) -> Status {
        let link = match (self.is_alone(), self.link.token.is_some(), self.linked, self.link.offline) {
            (true, ..) => "alone",
            (_, false, ..) => "signed out",
            (_, _, true, _) => "online",
            (_, _, false, true) => "offline",
            _ if self.dialling() => "connecting",
            _ => "waiting to dial",
        };
        Status {
            link: link.into(),
            linked: self.linked,
            epoch: self.link.epoch,
            cursor: self.cursor(),
            pending: self.pending_len(),
            user: self.me().actor.clone(),
            alone: self.is_alone(),
        }
    }
}
