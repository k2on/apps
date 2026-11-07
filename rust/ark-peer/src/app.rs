//! What an app drives (§1.10, §1.15): a device's functions called by
//! name, its views, and what became of what it did.
//!
//! An app authors ([`Peer::mutate`], on the space the function writes) and
//! asks by a function's whole name (`playlists.create_playlist`) — or calls the domain's own
//! view functions ([`Core::answer`]), of a device or a server alike — and holds the
//! maintained views its lists are, each one's patches handed back. The peer keeps every intent's [`Outcome`] and, per space,
//! the refusals its server sent until they are taken.
//!
//! **Nobody has to be signed in.** A device opened with no login
//! ([`Peer::open_as`]) authors as whoever last signed in on it — [`nobody`],
//! on a device where nobody ever did — and dials nothing; a login
//! ([`Peer::login`]) makes its pending intents the signer's, as
//! [`Peer::sign_in`] re-authors them, and dials. What a device did with
//! no server is pending, and is handed over under the login that signs in
//! at one (§1.7).

use std::collections::BTreeMap;

use ark::domain::{Domain, Func, Function};

use ark::format::Encode;
use ark::frames::Reply;
use ark::hash::StateHash;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::schema::Space;

use crate::{now_ms, Config, Core, Disk, Patch, Peer};

/// What a function is called with: a value, or its encoding already (a
/// host's that speaks JSON, [`crate::Core::input_json`]).
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

/// What a caller names a function by, and its input as that name takes it:
/// the function's whole name with a value or an encoding ([`Arg`]), or its
/// typed handle ([`Func`], `playlists::CREATE_PLAYLIST`) with its own input.
pub trait Named<A> {
    fn name(&self) -> &str;
    fn bytes(input: A) -> Vec<u8>;
}

impl<A: Arg> Named<A> for &str {
    fn name(&self) -> &str {
        self
    }
    fn bytes(input: A) -> Vec<u8> {
        input.bytes()
    }
}

impl<A: Arg> Named<A> for &String {
    fn name(&self) -> &str {
        self
    }
    fn bytes(input: A) -> Vec<u8> {
        input.bytes()
    }
}

impl<I: Encode, O> Named<&I> for Func<I, O> {
    fn name(&self) -> &str {
        self.name
    }
    fn bytes(input: &I) -> Vec<u8> {
        input.to_bytes()
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

/// What became of a device's own intents, and what its views did.
#[derive(Default)]
pub(crate) struct Told {
    /// What the authority said of each intent on the log, and the
    /// refusals of each space not yet taken.
    outcomes: BTreeMap<EntryId, Outcome>,
    pub refused: [Vec<(EntryId, String)>; 2],
    /// What each view did since it was last asked: its patches, in order;
    /// and how many it was ever told.
    pub updates: BTreeMap<usize, Vec<Patch>>,
    moves: u64,
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
        if !patches.is_empty() {
            self.moves += patches.len() as u64;
            self.updates.entry(view).or_default().extend(patches);
        }
    }
}

/// Nobody, on a session of this device's own: who a device nobody ever
/// signed in on authors as.
pub fn nobody() -> Ctx {
    let here = 0u8;
    let seed = format!("{}:{}:{:p}", now_ms(), crate::monotonic_ms(), &here);
    let id = ark::hash::hex(&ark::hash::sha256(&[seed.as_bytes()])[..8]);
    Ctx::who(String::new(), format!("device-{id}"), vec![])
}

impl Peer {
    /// The device `disk` holds, as whoever last signed in on it — nobody
    /// ([`nobody`]) on a disk nobody ever signed in on — and then, given a
    /// login and its token, as that login, its pending intents made the
    /// login's when they were somebody else's.
    pub fn open_as(config: Config, mut disk: Box<dyn Disk>, login: Option<(Ctx, String)>) -> Peer {
        let mut me = nobody();
        let _ = crate::local::preset(&mut *disk, |r| match &r.actor {
            Some(actor) => me = Ctx::who(actor.clone(), r.session.clone(), vec![]),
            None => (r.actor, r.session) = (Some(me.actor.clone()), me.session.clone()),
        });
        let mut p = Peer::open(config, me, disk);
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
    pub fn login(&mut self, who: Ctx, token: String) {
        let (actor, session) = (Some(who.actor.clone()), who.session.clone());
        self.replica(|r| (r.actor, r.session) = (actor, session));
        self.set_token(None);
        self.sign_in(who);
        self.set_token(Some(token));
    }

    pub fn is_signed_out(&self) -> bool {
        self.link.token.is_none()
    }

    /// An intent of the function `name` on `input` — a value, or its encoding — on the space the
    /// function writes, now: its id, or the refusal its preview gave it
    /// here and now. A live one the config relays (`Config::ephemeral`)
    /// goes through the relay, under the id it is sent with when nothing
    /// goes first.
    pub fn mutate<A, F: Named<A>>(&mut self, f: F, input: A) -> Result<EntryId, String> {
        self.mutate_at(f, input, now_ms())
    }

    /// [`Peer::mutate`] at `now`: a simulator's clock.
    pub fn mutate_at<A, F: Named<A>>(&mut self, f: F, input: A, now: i64) -> Result<EntryId, String> {
        let (space, f, id) = self.intent(f.name())?;
        if let Err(why) = self.author(space, &f, F::bytes(input), now) {
            // Said here, as the answer; not again among the refusals.
            self.told.refused[space as usize].retain(|(r, _)| *r != id);
            return Err(why);
        }
        self.flush();
        Ok(id)
    }

    /// A maintained view (§1.10) of the function `name` over the
    /// optimistic state of the space it is of, as whoever is signed in reads
    /// it — made again for each login — its number; and, when it cannot be
    /// made now, why. Refused, it is kept with no rows, and made again as
    /// its space changes.
    pub fn view(&mut self, name: &str, input: &[u8]) -> (usize, Result<(), String>) {
        let space = self.function(name).map_or(Space::Durable, |f| f.space);
        self.keep(space, name, input, self.ctx(), true)
    }

    /// What a view did since it was last asked: its patches, in order.
    pub fn update(&mut self, view: usize) -> Vec<Patch> {
        self.told.updates.remove(&view).unwrap_or_default()
    }

    /// How many patches its views were ever told: a count taken before
    /// says whether any moved since.
    pub fn moves(&self) -> u64 {
        self.told.moves
    }

    /// What became of the intent `id` on the log: none for one this run
    /// did not author, or of a run before.
    pub fn outcome(&self, id: &EntryId) -> Option<Outcome> {
        if self.log.pending().iter().any(|p| p.entry.id == *id) {
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

    pub fn pending_len(&self) -> usize {
        self.log.pending().len()
    }

    /// Why its server cannot serve this build, once it said so (§1.2): the
    /// app is to be updated; the link is closed until it is.
    pub fn update_needed(&self) -> Option<&str> {
        self.log.update_needed().or(self.live.as_ref().and_then(|l| l.update_needed()))
    }

    /// Where its link stands, as a status line says it: `update needed`
    /// (its server cannot serve this build), `alone` (no server), `signed
    /// out`, `online`, `offline`, `connecting` or `waiting to dial`.
    pub fn link_state(&self) -> &'static str {
        if self.update_needed().is_some() {
            return "update needed";
        }
        match (self.link.url.is_none(), self.link.token.is_some(), self.linked, self.link.offline) {
            // No server to dial: what it does is pending, kept here.
            (true, ..) => "alone",
            (_, false, ..) => "signed out",
            (_, _, true, _) => "online",
            (_, _, false, true) => "offline",
            _ if self.dialling() => "connecting",
            _ => "waiting to dial",
        }
    }
}

impl Core {
    /// The roles this login holds, which every intent it authors carries:
    /// a write is refused here as the server would refuse it.
    pub fn set_roles(&mut self, roles: impl IntoIterator<Item = impl Into<String>>) {
        let roles: Vec<String> = roles.into_iter().map(Into::into).collect();
        self.log.me.roles.clone_from(&roles);
        if let Some(l) = &mut self.live {
            l.me.roles = roles;
        }
    }

    pub fn me(&self) -> &Ctx {
        &self.log.me
    }

    /// Who a view is read and kept for.
    pub fn ctx(&self) -> Ctx {
        let me = self.me();
        Ctx {
            id: EntryId::of(&me.session, 0),
            now: now_ms(),
            ..me.clone()
        }
    }

    pub fn domain(&self) -> &Domain {
        &self.log.domain
    }

    /// The domains it has, the log's and the live space's.
    fn domains(&self) -> impl Iterator<Item = &Domain> {
        [&self.log].into_iter().chain(&self.live).map(|m| &*m.domain)
    }

    /// The function of that name and the domain it is of: the log's, else
    /// the live space's.
    fn found(&self, name: &str) -> Result<(&Domain, &Function), String> {
        self.domains()
            .find_map(|d| Some((d, d.get(name)?)))
            .ok_or_else(|| format!("no function {name}"))
    }

    /// The function a name means, of the log's domain or the live space's:
    /// its whole name, its kind and space, what it takes and answers.
    pub fn function(&self, name: &str) -> Result<&Function, String> {
        Ok(self.found(name)?.1)
    }

    /// A view's answer over the optimistic state, as this login reads it:
    /// its encoding.
    pub fn ask(&self, name: &str, input: &[u8]) -> Result<Vec<u8>, String> {
        let b = &self.function(name)?.name;
        self.domain().ask(&self.log.store, &self.ctx(), b, input)
    }

    /// A function's input — of the log's domain or the live one's — from its
    /// JSON (§1.1's human form), as the value format writes it: what a host
    /// that speaks JSON authors and asks with.
    pub fn input_json(&self, name: &str, text: &str) -> Result<Vec<u8>, String> {
        let (d, f) = self.found(name)?;
        d.input_json(&f.name, text)
    }

    /// What [`Core::ask`] answered, as JSON.
    pub fn answer_json(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let (d, f) = self.found(name)?;
        d.answer_json(&f.name, bytes)
    }

    /// One row of a view of `name`, as JSON.
    pub fn element_json(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let (d, f) = self.found(name)?;
        d.element_json(&f.name, bytes)
    }

    /// A row of a table of either space, as JSON.
    pub fn row_json(&self, table: &str, bytes: &[u8]) -> Result<String, String> {
        let d = self
            .domains()
            .find(|d| d.schema.table(table).is_some())
            .ok_or_else(|| format!("no table {table}"))?;
        d.row_json(table, bytes)
    }

    /// A view's answer over the optimistic state, as this login reads it.
    pub fn query<T: Encode>(&self, name: &str, input: &impl Encode) -> Result<T, String> {
        let bytes = self.ask(name, &input.to_bytes())?;
        T::from_bytes(&bytes).map_err(|r| format!("{name}: {r}"))
    }

    /// A view function of the domain in Rust (§1.16) answered afresh over
    /// the optimistic state, as this login reads it: the domain's own
    /// function, called as it is.
    pub fn answer<S: ark::db::Tables, I, T: ark::plan::Elem>(&self, f: ark::domain::ViewFn<S, I, T>, input: I) -> Result<Vec<T>, String> {
        let ctx = self.ctx();
        let plan = ark::db::read::<S, _>(&self.log.store, |db| f(&ctx, db, input))?;
        Ok(plan.run(&self.log.store, &ctx))
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
}
