//! What an app drives (§1.10, §1.15): a device's functions called by
//! name, its views, and what became of what it did.
//!
//! An app authors and asks by a function's name (`create_playlist`, or its
//! whole binding, `playlists.create_playlist`) — or calls the domain's own
//! view functions ([`Peer::answer`]) — and holds the
//! maintained views its lists are, each one's patches handed back as an
//! [`Update`]. The peer keeps every intent's [`Standing`] and the refusals
//! its server sent.
//!
//! **Nobody has to be signed in.** A device opened with no login
//! ([`Peer::open_as`]) authors as whoever last signed in on it — [`nobody`],
//! on a device where nobody ever did — and dials nothing; a login
//! ([`Peer::login`]) makes its pending intents the signer's, as
//! [`Peer::sign_in`] re-authors them, and dials. A device that joined a
//! server while alone hands its history over as intents pending under the
//! login that signs in there (§1.7's join hands a chain, which the server
//! adopts only from the session that made it — a session nobody's alone
//! history can have).

use std::collections::BTreeMap;

use ark::format::Encode;
use ark::hash::StateHash;
use ark::log::EntryId;
use ark::peer::{Ctx, Who};
use ark::schema::Space;
use ark::Domain;

use crate::{now_ms, Config, Disk, Patch, Peer};

/// What has become of an intent this device authored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    Pending,
    Confirmed,
    Rejected(String),
    Unknown,
}

/// What one view did since it was last asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Unchanged,
    Patched(Vec<Patch>),
    /// It could not be kept up: read it again.
    Reset,
}

/// What became of a device's own intents, and what its views did.
#[derive(Default)]
pub(crate) struct Told {
    standing: BTreeMap<EntryId, Standing>,
    rejections: Vec<(EntryId, String)>,
    live_refused: Vec<(EntryId, String)>,
    pub updates: BTreeMap<usize, Update>,
}

impl Told {
    pub fn refused(&mut self, space: Space, id: EntryId, why: &str) {
        if space == Space::Volatile {
            return self.live_refused.push((id, why.into()));
        }
        self.standing.insert(id, Standing::Rejected(why.into()));
        self.rejections.push((id, why.into()));
    }

    pub fn confirmed(&mut self, space: Space, id: EntryId) {
        if space == Space::Durable {
            self.standing.insert(id, Standing::Confirmed);
        }
    }

    pub fn patched(&mut self, view: usize, patches: Vec<Patch>) {
        if patches.is_empty() {
            return;
        }
        match self.updates.entry(view).or_insert(Update::Unchanged) {
            u @ Update::Unchanged => *u = Update::Patched(patches),
            Update::Patched(ps) => ps.extend(patches),
            Update::Reset => {}
        }
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
    /// A chain joined and not answered yet.
    pub joining: bool,
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
        let (disk, mut local) = crate::local::beside(disk);
        let kept = local.read(crate::local::replica).unwrap_or_default();
        let me = match kept.actor {
            Some(actor) => Who {
                actor,
                session: kept.session,
                roles: vec![],
            },
            None => {
                let me = nobody();
                let (actor, session) = (Some(me.actor.clone()), me.session.clone());
                let _ = local.write(|db| db.replica.put(crate::local::Replica { actor, session, ..kept }));
                me
            }
        };
        let mut p = Peer::opened(config, me, disk, local, None, crate::link::Link::default());
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
        // A joined history goes as intents under the login, not as a chain
        // only nobody's session could hand over.
        self.log.joining = None;
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
            draws: Default::default(),
            id: EntryId::of(&me.session, 0),
            actor: me.actor.clone(),
            session: me.session.clone(),
            roles: me.roles.clone(),
            now: now_ms(),
        }
    }

    pub fn domain(&self) -> &Domain {
        self.config.domain.as_deref().expect("a peer called by name has a domain")
    }

    /// The binding a name means ([`Domain::binding`]).
    pub fn binding(&self, name: &str) -> Option<String> {
        self.config.domain.as_deref()?.binding(name).map(String::from)
    }

    fn func(&self, name: &str) -> Result<String, String> {
        self.binding(name).ok_or_else(|| format!("no function {name}"))
    }

    /// A query's or a view's answer over the optimistic state, as this
    /// login reads it: its encoding.
    pub fn ask(&self, name: &str, input: &[u8]) -> Result<Vec<u8>, String> {
        let b = self.func(name)?;
        self.domain().ask(&self.log.store, &self.ctx(), &b, input).map_err(|r| r.0)
    }

    /// An intent: its id, or the refusal its preview — or, alone, its
    /// sequencing — gave it here and now.
    pub fn mutate(&mut self, name: &str, input: &impl Encode) -> Result<EntryId, String> {
        self.mutate_bytes(name, input.to_bytes())
    }

    pub fn mutate_bytes(&mut self, name: &str, input: Vec<u8>) -> Result<EntryId, String> {
        let (f, id) = (self.func(name)?, self.log.next_id());
        if let Err(why) = self.author(Space::Durable, &f, input, now_ms()) {
            self.told.rejections.retain(|(r, _)| *r != id);
            return Err(why);
        }
        self.flush();
        Ok(id)
    }

    /// A call on the live space (§1.9) by a name of its module's domain (or
    /// the log's): an ephemeral one through the relay, any other at once;
    /// the preview's refusal said.
    pub fn call(&mut self, name: &str, input: Vec<u8>) -> Result<(), String> {
        let live = self.config.live.as_ref().and_then(|l| l.module.domain.as_deref());
        let f = match live.and_then(|d| d.binding(name)) {
            Some(f) => f.to_string(),
            None => self.func(name)?,
        };
        let id = self.live.as_ref().ok_or("no live space here")?.next_id();
        if let Err(why) = self.author(Space::Volatile, &f, input, now_ms()) {
            self.told.live_refused.retain(|(r, _)| *r != id);
            return Err(why);
        }
        self.flush();
        Ok(())
    }

    /// The domain a name of a function or a table is in, and the name
    /// there: the log's, else the live space's.
    fn named(&self, name: &str, table: bool) -> Result<(&Domain, String), String> {
        let live = self.config.live.as_ref().and_then(|l| l.module.domain.as_deref());
        for d in self.config.domain.as_deref().into_iter().chain(live) {
            match table {
                true if d.schema.table(name).is_some() => return Ok((d, name.into())),
                false => match d.binding(name) {
                    Some(b) => return Ok((d, b.into())),
                    None => continue,
                },
                true => continue,
            }
        }
        Err(format!("no {} {name}", if table { "table" } else { "function" }))
    }

    /// A function's input — of the log's domain or the live one's — from its
    /// JSON (§1.1's human form), as the value format writes it: what a host
    /// that speaks JSON authors and asks with.
    pub fn input_json(&self, name: &str, text: &str) -> Result<Vec<u8>, String> {
        let (d, b) = self.named(name, false)?;
        d.input_json(&b, text).map_err(|r| r.0)
    }

    /// What [`Peer::ask`] answered, as JSON.
    pub fn answer_json(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let (d, b) = self.named(name, false)?;
        d.answer_json(&b, bytes).map_err(|r| r.0)
    }

    /// One row of a view of `name`, as JSON.
    pub fn element_json(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let (d, b) = self.named(name, false)?;
        d.element_json(&b, bytes).map_err(|r| r.0)
    }

    /// A row of a table of either space, as JSON.
    pub fn row_json(&self, table: &str, bytes: &[u8]) -> Result<String, String> {
        let (d, t) = self.named(table, true)?;
        d.row_json(&t, bytes).map_err(|r| r.0)
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
        let (ctx, b) = (self.ctx(), self.func(name)?);
        self.watch(Space::Durable, &b, input, ctx).map_err(|r| r.0)
    }

    /// A view's rows as they stand, each its bytes.
    pub fn rows(&self, view: usize) -> Vec<Vec<u8>> {
        self.values(view).unwrap_or_default()
    }

    /// What a view did since it was last asked.
    pub fn update(&mut self, view: usize) -> Update {
        self.told.updates.remove(&view).unwrap_or(Update::Unchanged)
    }

    pub fn standing(&self, id: &EntryId) -> Standing {
        if self.log.pending.iter().any(|p| p.entry.id == *id) {
            return Standing::Pending;
        }
        self.told.standing.get(id).cloned().unwrap_or(Standing::Unknown)
    }

    /// What the server refused of this device's intents since the last ask.
    pub fn take_rejections(&mut self) -> Vec<(EntryId, String)> {
        std::mem::take(&mut self.told.rejections)
    }

    /// What the server refused of this device's live calls since the last
    /// ask.
    pub fn take_live_refusals(&mut self) -> Vec<String> {
        std::mem::take(&mut self.told.live_refused).into_iter().map(|(_, w)| w).collect()
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
            joining: self.log.joining.is_some(),
            user: self.me().actor.clone(),
            alone: self.is_alone(),
        }
    }
}
