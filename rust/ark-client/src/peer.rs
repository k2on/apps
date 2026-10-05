//! The peer: the replica of an app's log, persisted; the engine's `Client`
//! around it; a local authority when there is no server; the link to the
//! server when there is one; and what a screen does — mutate, query, check,
//! hold a view.
//!
//! **Alone is a way of being used, not a kind of peer** (`docs/plan-alone.md`).
//! A peer opened with [`Options::alone`] is its own authority: every intent
//! is sequenced as it is authored, and the entries are kept — on the
//! storage, never in memory — as its **local history**, above the **fork**,
//! the `(log, cursor)` it last shared with a server. [`Peer::join`] hands
//! that history to a server: the confirmed store goes back to the fork and
//! every local entry is pending again, in order, pushed and rebased as any
//! offline work is. [`Peer::leave`] is the other way. Opening a storage the
//! other way from how it was last used is the same two transitions, so a
//! directory used alone for a year and then opened with a server is joined,
//! and nothing is refused.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ark::canon;
use ark::eval::{self, Args, Checked, Ctx, EvalFault};
use ark::journal::{self as records, Journal as LogJournal, Layout};
use ark::log::{snapshot_of, Entry, Facts, Log, Seq, Snapshot};
use ark::peer::{local_commit, Authority, Changes, Journal, Replica};
use ark::protocol::{Client, ClientMsg, Mode, Received, ServerMsg};
use ark::schema::Schema;
use ark::store::{MemoryStore, Refusal, Store};
use ark::value::{Id, Value};

use crate::autos::Autos;
use crate::link::{platform_dial, Dial, Link, State, Timing};
use crate::storage::{
    count_pages, encode_page, encode_pending_page, encode_pending_snapshot, encode_replica_of, encode_who, load_pending, BoxStorage, Fork, Memory,
    PendingStored, ReplicaFile, Stored,
};
use crate::view::View;
use crate::{Domain, Error};

/// How a peer is opened.
#[derive(Clone, Debug)]
pub struct Options {
    /// Who authors: the entry's actor. Under `ark-auth` the login's
    /// `user.id`; under the engine's dev `trusting`, the token.
    pub user: String,
    /// The login it authors under: `Login::session`, or `"dev"` under
    /// `trusting`. The server holds every pushed entry to both.
    pub session: String,
    /// What the `Hello` proves the login with.
    pub token: Option<String>,
    /// No server: this peer is the authority for the log, and nothing
    /// stays pending (the demo, a peer working alone). Over a storage last
    /// used with a server, opening alone is [`Peer::leave`]; without it,
    /// over one last used alone, it is a join (`docs/plan-alone.md` §1).
    pub alone: bool,
    pub autos: Autos,
    pub timing: Timing,
}

impl Options {
    /// A peer that syncs with a server, as `user` under `session`, proving
    /// it with `token`.
    pub fn server(user: impl Into<String>, session: impl Into<String>, token: Option<String>) -> Options {
        Options {
            user: user.into(),
            session: session.into(),
            token,
            alone: false,
            autos: Autos::system(),
            timing: Timing::default(),
        }
    }

    /// Dev auth (`ark::protocol::trusting`): the token is the name and the
    /// session is `"dev"`.
    pub fn dev(user: impl Into<String>) -> Options {
        let user = user.into();
        Options::server(user.clone(), "dev", Some(user))
    }

    /// A peer nobody has signed in on yet, for a server it will sync with
    /// once somebody does. What it authors is authored as nobody
    /// (`Ctx::nobody`), kept pending and durable — not committed, which is
    /// the difference from [`Options::alone`] — and nothing is dialled.
    /// [`Peer::sign_in`] makes all of it the signer's and connects.
    ///
    /// Opened over storage that has had a login, it authors as that login,
    /// still offline: see [`Peer::sign_out`].
    pub fn signed_out() -> Options {
        Options::server("", "", None)
    }

    /// A peer that is its own authority, authoring as `user` under the
    /// session `"local"`. What it sequences is kept as its local history,
    /// which [`Peer::join`] later hands a server.
    pub fn alone(user: impl Into<String>) -> Options {
        Options {
            alone: true,
            ..Options::server(user, "local", None)
        }
    }

    /// A peer alone that nobody has signed in on: it authors as nobody
    /// (`Ctx::nobody`), as a signed-out peer of a server does, so a
    /// [`Peer::join`] with no login changes no row and no view is told to
    /// start over — and the [`Peer::sign_in`] after it makes everything the
    /// signer's, as it always has (`docs/plan-alone.md` §4).
    pub fn alone_as_nobody() -> Options {
        Options {
            alone: true,
            ..Options::server("", "", None)
        }
    }

    pub fn with_autos(mut self, autos: Autos) -> Options {
        self.autos = autos;
        self
    }

    pub fn with_timing(mut self, timing: Timing) -> Options {
        self.timing = timing;
        self
    }
}

/// Who a peer signs in as at a server: what [`Options::server`] takes, as
/// one value for [`Peer::join`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Login {
    pub user: String,
    pub session: String,
    pub token: Option<String>,
}

impl Login {
    pub fn new(user: impl Into<String>, session: impl Into<String>, token: Option<String>) -> Login {
        Login {
            user: user.into(),
            session: session.into(),
            token,
        }
    }

    /// Dev auth: the token is the name and the session is `"dev"`.
    pub fn dev(user: impl Into<String>) -> Login {
        let user = user.into();
        Login::new(user.clone(), "dev", Some(user))
    }
}

/// A verdict against one of this peer's own intents: it will never be in
/// the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub id: Id,
    pub reason: String,
}

/// Where one of this peer's own intents stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Applied here, not yet answered by the authority.
    Pending,
    /// In the log.
    Confirmed,
    /// Never going to be in the log, and the reason every replica reaches:
    /// what a screen shows beside the item that did not happen.
    Rejected(String),
    /// Not an intent this peer authored (in this run, or pending from the
    /// last).
    Unknown,
}

/// What one [`Peer::pump`] came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pumped {
    /// A frame arrived (the log, an ack, a live frame).
    pub moved: bool,
    /// The socket opened, and `Hello` went out.
    pub opened: bool,
    /// The socket that was open went, and why. The link dials again.
    pub dropped: Option<String>,
    /// The server turned this login away. The link stops; a new token and
    /// [`Peer::reconnect`] are the way back. Nothing pending is lost.
    pub denied: Option<String>,
    /// Verdicts that arrived.
    pub rejected: usize,
    /// Something went wrong that is worth a status line: a frame that did
    /// not decode, a replica that could not be written.
    pub note: Option<String>,
}

/// Everything a status line or a debug screen shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub user: String,
    pub session: String,
    pub alone: bool,
    /// Nobody is signed in: nothing is dialled. `user` is empty if nobody
    /// ever was, or the login this peer goes on authoring as.
    pub signed_out: bool,
    /// The engine is linked: the socket is open and `Hello` was said.
    pub linked: bool,
    /// The link in a word: `alone`, `signed out`, `offline`, `idle`,
    /// `connecting`, `joining` (open or connecting with local history still
    /// pending), `open`, `waiting`.
    pub link: String,
    /// Intents this peer sequenced alone that a join re-queued and the
    /// server has not yet answered: 0 once it has taken them all.
    pub joining: usize,
    /// Where this peer last shared a log with a server.
    pub fork: Fork,
    pub url: Option<String>,
    /// Connections opened.
    pub opens: u64,
    /// The connection count the engine keeps: what a live room compares.
    pub epoch: i64,
    /// The last confirmed sequence.
    pub cursor: Seq,
    pub pending: usize,
    /// Pending intents the server answered `held`: it has not run the
    /// module that ships their function — this peer is newer than it — so
    /// they wait, pending, and are pushed again on every connection until
    /// an upgraded server takes them (`docs/plan-db.md` D1).
    pub held: usize,
    /// The server runs another module than this peer's: its facts are
    /// applied projected to this peer's schema, and no `Verify` is said
    /// (`docs/plan-db.md` D1). A peer older than its server stays usable
    /// on everything its schema can see.
    pub behind: bool,
    pub diverged: usize,
    pub denied: Option<String>,
    pub last_close: Option<String>,
    /// Live frames received, decoded or not.
    pub heard_frames: u64,
    pub bad_frames: u64,
}

/// One peer of an app. See the crate docs for the shape.
pub struct Peer {
    domain: Domain,
    schema: Schema,
    client: Client,
    authority: Option<Authority>,
    storage: BoxStorage,
    ctx: Ctx,
    autos: Autos,
    timing: Timing,
    alone: bool,
    /// Nobody is signed in, so nothing is dialled.
    signed_out: bool,
    /// What the storage holds of the confirmed store: the snapshot and the
    /// journal after it.
    durable: Durable,
    /// What the `who` record holds, and what the `pending` snapshot and its
    /// pages hold together — the intents' ids, in order — or `None` where
    /// it is behind in a way those do not show (nothing written yet;
    /// pending re-stamped by a sign-in; a page that may or may not have
    /// landed), which makes the next write a snapshot.
    wrote_who: Option<Ctx>,
    wrote_pending: Option<Vec<Id>>,
    /// What the storage holds of the pending intents besides their ids:
    /// the snapshot and the pages after it.
    pending_file: PendingFile,
    link: Option<Link>,
    rejections: Vec<Rejection>,
    /// Every intent authored here this run or found pending at open.
    authored: BTreeSet<Id>,
    /// Every verdict, by intent, kept for [`Peer::standing`].
    rejected: BTreeMap<Id, String>,
    heard_frames: u64,
    bad_frames: u64,
    /// Where this replica last shared a log with a server
    /// (`docs/plan-alone.md` §1); alone, the base of its local history.
    fork: Fork,
    /// Alone: the local history as the storage has it.
    alone_log: Option<AloneLog>,
    /// The intents the last join re-queued, for [`Status::joining`].
    local: BTreeSet<Id>,
    /// Verify after every settle (`docs/plan-db.md` D3): what this
    /// connection has asked and been answered.
    checks: Checks,
}

/// The verifies of one connection (`docs/plan-db.md` D3). Since the state
/// hash is read rather than computed (§8.1), a `Verify` costs both ends the
/// tables rather than the rows, so a linked peer asks after every settle
/// that moved its cursor — and a divergence is reported when it happens,
/// not when somebody thinks to ask. A connection answers in the order it
/// was asked, so which answer is to which question is a queue: an answer
/// to one [`Peer::verify`] asked for goes to [`Peer::agreed`] as it always
/// did; one to an automatic verify goes there only when it disagrees, so
/// that agreeing a hundred times a minute is not a hundred entries.
#[derive(Debug, Default)]
struct Checks {
    /// The connection these are of, as [`Client::epoch`] counts them.
    epoch: i64,
    /// The cursor last verified on it.
    at: Option<Seq>,
    /// For each verify said on it and not yet answered, oldest first,
    /// whether it was automatic.
    asked: VecDeque<bool>,
    /// It has sent a page or a snapshot. Until then the cursor is the one
    /// this peer came with, which may be past the head or on another log —
    /// the server is about to say so with a snapshot, and a verify there
    /// would report as a divergence what is only a peer being re-based.
    served: bool,
    /// Every answer to a verify asked for, and every disagreement.
    agreed: Vec<(Seq, bool)>,
    /// Every verify asked for that was answered "cannot say".
    unknown: Vec<Seq>,
}

/// A peer alone's local history as the storage has it
/// (`docs/plan-alone.md` §2): the journal, and the entries sequenced since
/// it was last written — which is before `mutate` returns, so this holds
/// entries only across a write that failed. The authority keeps none: its
/// log is a head and an id set.
struct AloneLog {
    journal: LogJournal,
    unwritten: Vec<(Seq, Entry, Facts)>,
}

/// The confirmed store as the storage has it (`storage` module docs): a
/// snapshot, and `pages` journal pages after it, together reaching
/// `cursor`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Durable {
    snapshot_bytes: usize,
    /// `facts.1` to `facts.<pages>`: the pages on the storage, which a
    /// compaction removes.
    pages: usize,
    page_bytes: usize,
    cursor: Seq,
    /// The log the snapshot names (`storage` module docs). The replica
    /// learning another is written as a snapshot, since a page cannot say
    /// it.
    log_id: Option<Id>,
    /// The journal cannot be written on from `cursor` — there is no
    /// snapshot yet, the confirmed store was replaced, a write failed,
    /// `open` found a page it could not use — so the next write is a
    /// snapshot.
    snapshot_due: bool,
}

/// The pending intents as the storage has them (`storage` module docs):
/// a snapshot of generation `gen`, and `pages` pages after it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PendingFile {
    gen: i64,
    snapshot_bytes: usize,
    /// `pending.1` to `pending.<pages>`: the pages on the storage, which a
    /// compaction removes.
    pages: usize,
    page_bytes: usize,
}

impl PendingFile {
    fn of(st: &PendingStored) -> PendingFile {
        PendingFile {
            gen: st.gen,
            snapshot_bytes: st.snapshot_bytes,
            pages: st.found,
            page_bytes: st.page_bytes,
        }
    }
}

impl Drop for Peer {
    /// Whatever is not yet written down is: a peer alone leaves its journal
    /// to the next `pump`, and a program that closes between the two closes
    /// through here.
    fn drop(&mut self) {
        let _ = self.persist();
    }
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("ctx", &self.ctx)
            .field("alone", &self.alone)
            .field("link", &self.link)
            .finish()
    }
}

// Two intents' ids compared, to decide what `persist_pending` writes:
// counted in a test, per thread, as the engine counts its copies.
fn same_id(a: &Id, b: &Id) -> bool {
    #[cfg(test)]
    COMPARED.with(|n| n.set(n.get() + 1));
    a == b
}

#[cfg(test)]
thread_local! {
    static COMPARED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Peer {
    /// Open the replica from `storage` (or empty), replay what was pending
    /// on top, and — alone — sequence it. No socket yet: [`Peer::connect`]
    /// dials.
    ///
    /// A storage last used the other way is carried across rather than
    /// refused (`docs/plan-alone.md` §1): opened alone over a replica of a
    /// server, the peer leaves it ([`Peer::leave`]); opened for a server
    /// over local history, the history is re-queued as [`Peer::join`] does
    /// — as the login in `opts`, or as nobody when it is signed out, for
    /// [`Peer::sign_in`] to make the signer's — and nothing is dialled
    /// until [`Peer::connect`]. A join that stopped halfway, with the
    /// re-queued intents and the fork's replica written and the local
    /// history not yet removed, finishes the same way.
    pub fn open(domain: Domain, storage: BoxStorage, opts: Options) -> Result<Peer, Error> {
        let schema = domain.module().schema.clone();
        let natives = domain.native_list();
        // What the records hold decides what is written first: a storage
        // with no replica yet has everything written at the end of this
        // open, and one whose journal could not all be read is compacted.
        let (confirmed, cursor, pending, was, durable, wrote_who, mut wrote_pending, pending_file, fork) = match Stored::load(&*storage, &schema)? {
            Some(st) => {
                let f = st.file;
                let was = Ctx::new(f.user, f.session);
                let ids: Vec<Id> = f.pending.iter().map(|e| e.id).collect();
                let durable = Durable {
                    snapshot_bytes: st.snapshot_bytes,
                    pages: st.found,
                    page_bytes: st.page_bytes,
                    cursor: f.cursor,
                    log_id: st.log_id,
                    snapshot_due: !st.clean,
                };
                // Pending pages that could not all be read, or none written
                // yet: the open's write is a snapshot of them.
                let ids = st.pending.clean.then_some(ids);
                let pending_file = PendingFile::of(&st.pending);
                (
                    f.confirmed,
                    f.cursor,
                    f.pending,
                    was.clone(),
                    durable,
                    Some(was),
                    ids,
                    pending_file,
                    f.fork,
                )
            }
            None => {
                // No store yet, but perhaps intents: a run that stopped between
                // writing them and writing the store. They are kept.
                let (pending, pending_stored) = load_pending(&*storage)?;
                let pending = pending.unwrap_or_default();
                let durable = Durable {
                    snapshot_bytes: 0,
                    pages: count_pages(&*storage)?,
                    page_bytes: 0,
                    cursor: 0,
                    log_id: None,
                    snapshot_due: true,
                };
                let pending_file = PendingFile::of(&pending_stored);
                (
                    MemoryStore::empty(schema.clone()),
                    0,
                    pending,
                    Ctx::nobody(),
                    durable,
                    None,
                    None,
                    pending_file,
                    Fork::default(),
                )
            }
        };
        // A `log` record is local history: the peer was last used alone.
        let had_log = storage.load(&Layout::alone().snapshot)?.is_some();
        let authored = pending.iter().map(|e| e.id).collect();
        let mut r = Replica::open(schema.clone(), domain.closures().clone(), confirmed, cursor, pending);
        // The log the cursor is of, as the storage names it; unnamed, the
        // server's first answer names it (Round 4).
        r.log_id = durable.log_id;
        r.hold(natives.iter().cloned());
        let mut client = Client::open(r, Mode::Whole, opts.token.clone());
        // What the server's module is compared with (`behind`,
        // `docs/plan-db.md` D1).
        client.module = Some(domain.hash());
        // What was just opened is what the storage holds; the journal starts
        // here.
        let _ = client.replica.take_confirmed();
        // Who authors. Opened signed out over storage somebody has used, it
        // is still them; opened with a login over work nobody authored, that
        // work is the login's — the same as `sign_in`, whichever order the
        // app did the two in.
        let mut ctx = Ctx::new(opts.user, opts.session);
        let signed_out = !opts.alone && ctx.is_nobody();
        if signed_out {
            // Who last used it — unless that was a peer alone, whose login
            // no server knows: a join signed out re-queues the local
            // history as nobody's, for a sign-in to make the signer's.
            ctx = if had_log { Ctx::nobody() } else { was };
        } else if !opts.alone && client.replica.pending.iter().any(|e| e.actor.is_empty() && e.session.is_empty()) {
            client.sign_in(&ctx, opts.token.clone());
            wrote_pending = None;
        }
        let mut peer = Peer {
            domain,
            schema,
            client,
            authority: None,
            storage,
            ctx,
            autos: opts.autos,
            timing: opts.timing,
            alone: false,
            signed_out,
            durable,
            wrote_who,
            wrote_pending,
            pending_file,
            link: None,
            rejections: vec![],
            authored,
            rejected: BTreeMap::new(),
            heard_frames: 0,
            bad_frames: 0,
            fork,
            alone_log: None,
            local: BTreeSet::new(),
            checks: Checks::default(),
        };
        match (had_log, opts.alone) {
            (true, _) => peer.resume_alone()?,
            (false, true) => peer.start_alone()?,
            (false, false) => {}
        }
        if had_log && !opts.alone {
            let who = peer.ctx.clone();
            peer.fork_back_to(&who)?;
        }
        // Alone, whatever a previous run left pending is sequenced now.
        peer.commit_alone();
        peer.collect_rejections();
        peer.persist()?;
        // Opening replayed pending on top of confirmed: a view hydrates from
        // here, so that `Rebuilt` is nobody's news.
        let _ = peer.take_changes();
        Ok(peer)
    }

    /// In memory: nothing survives the peer. Tests and the demo.
    pub fn open_memory(domain: Domain, opts: Options) -> Result<Peer, Error> {
        Peer::open(domain, Box::new(Memory::new()), opts)
    }

    /// A directory, holding the files `replica`, `facts.<n>`, `pending`,
    /// `pending.<n>` and `who` — and alone, `log` and `log.<n>`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_path(domain: Domain, dir: impl Into<std::path::PathBuf>, opts: Options) -> Result<Peer, Error> {
        Peer::open(domain, Box::new(crate::storage::Dir(dir.into())), opts)
    }

    /// The browser's `localStorage`, under keys starting `ark:{name}:`.
    #[cfg(target_arch = "wasm32")]
    pub fn open_local(domain: Domain, name: &str, opts: Options) -> Result<Peer, Error> {
        let prefix = format!("ark:{name}:");
        Peer::open(domain, Box::new(crate::storage::Local { prefix }), opts)
    }

    pub fn domain(&self) -> &Domain {
        &self.domain
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Who this peer authors as.
    pub fn ctx(&self) -> &Ctx {
        &self.ctx
    }

    pub fn is_alone(&self) -> bool {
        self.alone
    }

    /// Whether nobody is signed in: nothing is dialled until
    /// [`Peer::sign_in`].
    pub fn is_signed_out(&self) -> bool {
        self.signed_out
    }

    /// Somebody signed in: `user` under `session`, proven with `token`.
    /// Every intent still pending that was authored as nobody becomes
    /// theirs, under this login, and the view is replayed so every row
    /// those intents wrote says so; the ids [`Peer::mutate`] returned stay
    /// the same, so [`Peer::standing`] then follows each to its fate. The
    /// rewritten intents are written down before anything is said, and the
    /// link — if [`Peer::connect`] made one — dials, so the next `Hello`
    /// pushes all of it.
    ///
    /// Intents authored as somebody (before a [`Peer::sign_out`]) keep their
    /// author: the server takes them if they are this person's under an
    /// older login (`Auth::owns`), and refuses them as `not yours` if not.
    pub fn sign_in(&mut self, user: impl Into<String>, session: impl Into<String>, token: Option<String>) -> Result<(), Error> {
        if self.alone {
            return Err(Error::Alone);
        }
        self.ctx = Ctx::new(user, session);
        self.client.sign_in(&self.ctx, token);
        self.signed_out = false;
        self.collect_rejections();
        self.wrote_pending = None;
        self.persist()?;
        self.reconnect();
        Ok(())
    }

    /// Nobody is signed in any more: the token is forgotten and the link
    /// stops. The login is *not* forgotten — this peer goes on authoring as
    /// that person, offline, and what it authors stays theirs: pushed when
    /// they sign in again (under any login of theirs), refused if somebody
    /// else does. Only work done before anyone ever signed in on a peer is
    /// nobody's, because only that has never been anybody's.
    pub fn sign_out(&mut self) {
        if self.alone {
            return;
        }
        self.signed_out = true;
        self.client.token = None;
        if let Some(link) = &mut self.link {
            link.stop("signed out");
        }
        if self.client.linked {
            self.client.disconnected();
        }
    }

    /// Alone → server (`docs/plan-alone.md` §1): everything this peer
    /// sequenced alone since its fork goes to the server at `url`, in
    /// order, on top of whatever the server has. The confirmed store goes
    /// back to the fork — the log and cursor it last shared with a server,
    /// or nothing — every local entry is pending again, authored as
    /// `login` (or as nobody, for [`Peer::sign_in`] to make the signer's,
    /// when there is none yet), and the link dials: the `Hello` names the
    /// fork's log, the server pages it or sends its snapshot, and the
    /// intents are pushed and rebased on top, as offline work always is. A
    /// local intent that now refuses is dropped with its reason. What a
    /// view is told is a rebase, by changes: a screen of the library is
    /// patched through a join, not rebuilt.
    ///
    /// Durable before it dials, in the order [`Peer::open`] finishes from.
    /// [`Status::joining`] counts what the server has yet to take. On a
    /// peer that already has a server, it is [`Peer::sign_in`] (with a
    /// login) and [`Peer::connect`].
    pub fn join(&mut self, url: &str, login: Option<Login>) -> Result<(), Error> {
        let who = login.as_ref().map_or_else(Ctx::nobody, |l| Ctx::new(l.user.clone(), l.session.clone()));
        if self.alone {
            self.ctx = who.clone();
            self.signed_out = login.is_none();
            self.client.token = login.and_then(|l| l.token);
            self.fork_back_to(&who)?;
        } else if let Some(l) = login {
            self.sign_in(l.user, l.session, l.token)?;
        }
        self.connect(url);
        Ok(())
    }

    /// Server → alone (`docs/plan-alone.md` §1): the link closed and
    /// forgotten, the fork recorded where the replica stands, and this peer
    /// its own authority from its confirmed store on — what was pending
    /// sequenced locally at once. It authors as the login it had. Coming
    /// back is [`Peer::join`]. Alone already, nothing.
    pub fn leave(&mut self) -> Result<(), Error> {
        if self.alone {
            return Ok(());
        }
        if let Some(link) = &mut self.link {
            link.stop("alone");
        }
        self.link = None;
        if self.client.linked {
            self.client.disconnected();
        }
        self.client.denied = None;
        self.start_alone()?;
        self.commit_alone();
        self.collect_rejections();
        self.persist()
    }

    /// Author under this login from now on (entries already pending keep
    /// the session they were authored under).
    pub fn set_session(&mut self, session: impl Into<String>) {
        self.ctx.session = session.into();
    }

    /// What the next `Hello` proves the login with. Say [`Peer::reconnect`]
    /// for it to be said now.
    pub fn set_token(&mut self, token: Option<String>) {
        self.client.token = token;
    }

    // -- a screen's side ------------------------------------------------------

    /// Author an intent by name: its autos drawn here, once, frozen in the
    /// entry; applied to the optimistic store natively (or through the
    /// interpreter); kept durable as pending; pushed if linked. The entry's
    /// id, or the refusal — a refusal changes nothing and records nothing.
    pub fn mutate(&mut self, name: &str, args: Args) -> Result<Id, Error> {
        // Borrowed from the domain, field by field beside the draws: the
        // mutator's IR is read, never copied (`docs/plan-perf.md` R5).
        let (fh, f) = self.domain.mutator(name)?;
        let autos = self.autos.draw(f);
        let id = self.autos.new_id();
        self.client.mutate(id, &self.ctx, fh, &autos, &args).map_err(Error::Refused)?;
        self.authored.insert(id);
        self.commit_alone();
        self.collect_rejections();
        // The intent is written down now — alone, as the entry it became,
        // a page of the local history (`docs/plan-alone.md` §2); with a
        // server, as a pending page. The store follows on the next `pump`:
        // authoring costs the intent, not the store.
        self.persist_log()?;
        self.persist_pending()?;
        Ok(id)
    }

    /// Run a query by name over the optimistic store, as this peer's user.
    pub fn query(&self, name: &str, args: &Args) -> Result<Value, Error> {
        let (fh, _) = self.domain.query(name)?;
        let store = self.store();
        let out = match self.domain.natives().get(fh) {
            Some(p) => p.query(&self.ctx, args, store),
            None => eval::query_closure(&self.schema, &self.domain.closures()[fh], &self.ctx, args, store),
        };
        out.map_err(|e| match e {
            EvalFault::Verdict(r) => Error::Refused(r),
            EvalFault::Bug(b) => Error::Bug(format!("{name}: {b:?}")),
        })
    }

    /// The form validator (`spec/AUTHORING.md` §1.3) over a partial input
    /// to a mutator or query: a message per failing field, and the values
    /// as the checks normalised them. Nothing is written.
    pub fn check(&self, name: &str, partial: &Args) -> Result<Checked, Error> {
        let (fh, _) = self.domain.function(name).ok_or_else(|| Error::UnknownFunction(name.into()))?;
        let store = self.store();
        eval::check(&self.schema, &self.domain.closures()[fh], &self.ctx, partial, store).map_err(|b| Error::Bug(format!("{name}: {b:?}")))
    }

    /// A query held as a list and kept up to date: see [`View`].
    pub fn view(&self, name: &str, args: Args) -> Result<View, Error> {
        View::open(self, name, args)
    }

    /// The optimistic store — `confirmed` with `pending` replayed — which
    /// every query reads.
    pub fn store(&self) -> &MemoryStore {
        &self.client.replica.view
    }

    /// The replica, to look at.
    pub fn replica(&self) -> &Replica {
        &self.client.replica
    }

    /// What moved since the last ask: `Applied(changes)` to the optimistic
    /// store, or `Rebuilt` — a rebase rolled it back and replayed pending on
    /// top, which no list of changes describes. Hand it to every
    /// [`View::update`].
    pub fn take_changes(&mut self) -> Changes {
        self.client.replica.take_changes()
    }

    /// Verdicts against this peer's intents since the last ask.
    pub fn take_rejections(&mut self) -> Vec<Rejection> {
        self.collect_rejections();
        std::mem::take(&mut self.rejections)
    }

    /// Where one of this peer's intents stands: what a screen shows beside
    /// the item it made.
    pub fn standing(&self, id: &Id) -> Standing {
        if let Some(why) = self.rejected.get(id) {
            return Standing::Rejected(why.clone());
        }
        let r = &self.client.replica;
        if let Some((_, why)) = r.rejections.iter().find(|(i, _)| i == id) {
            return Standing::Rejected(refusal_text(why));
        }
        if r.pending.iter().any(|e| e.id == *id) {
            Standing::Pending
        } else if self.authored.contains(id) {
            Standing::Confirmed
        } else {
            Standing::Unknown
        }
    }

    /// Intents authored here and not yet answered.
    pub fn pending_len(&self) -> usize {
        self.client.replica.pending.len()
    }

    /// The last confirmed sequence.
    pub fn cursor(&self) -> Seq {
        self.client.replica.cursor
    }

    /// Ask the authority whether it agrees with the confirmed state; the
    /// answer arrives in [`Peer::agreed`]. Alone, at once — and at the head,
    /// which is where a peer alone is, by hashing the authority's store as
    /// it stands rather than replaying the log (`docs/plan-perf.md` R4).
    pub fn verify(&mut self) {
        let Some(a) = &self.authority else {
            self.ask_verify(false);
            return;
        };
        let (n, h) = self.client.replica.verify_at();
        match a.log.hash_at(n, &a.store) {
            Some(theirs) => self.checks.agreed.push((n, theirs == h)),
            None => self.checks.unknown.push(n),
        }
    }

    /// Every `(seq, agreed)` the authority has answered to a
    /// [`Peer::verify`], and every `(seq, false)` of the verify a linked
    /// peer makes after each settle on its own (`docs/plan-db.md` D3): a
    /// divergence is reported here whoever asked.
    pub fn agreed(&self) -> &[(Seq, bool)] {
        &self.checks.agreed
    }

    /// Every sequence a [`Peer::verify`] was answered "cannot say" at: the
    /// authority holds no state there — below its horizon, past its head —
    /// to compare (`docs/plan-db.md` D3). An automatic verify answered so
    /// reports nothing, here or in [`Peer::agreed`].
    pub fn unknown(&self) -> &[Seq] {
        &self.checks.unknown
    }

    /// A `Verify` said on this connection, and remembered as asked for or
    /// automatic until its answer comes back. Whatever the engine did not
    /// say — unlinked, or behind its server (`docs/plan-db.md` D1) — is not
    /// remembered, so that every answer still meets its own question.
    fn ask_verify(&mut self, auto: bool) {
        self.this_connection();
        let before = self.client.out.len();
        self.client.verify_all();
        if self.client.out.len() > before {
            self.checks.asked.push_back(auto);
            self.checks.at = Some(self.client.replica.cursor);
        }
    }

    // What was asked of another connection is not answered on this one.
    fn this_connection(&mut self) {
        if self.checks.epoch != self.client.epoch {
            self.checks.epoch = self.client.epoch;
            self.checks.at = None;
            self.checks.asked.clear();
            self.checks.served = false;
        }
    }

    /// The engine's settle, then what comes after it here: the answers that
    /// arrived sorted to their questions, and — linked, done paging, the
    /// cursor moved since the last — a `Verify` of the confirmed state
    /// (`docs/plan-db.md` D3). The disagreements it brought, as `(seq,
    /// false)`.
    fn settle(&mut self) -> Vec<Seq> {
        let paging = self.client.more;
        self.client.settle();
        self.this_connection();
        let mut disagreed = vec![];
        for (n, answer) in std::mem::take(&mut self.client.agreed) {
            let auto = self.checks.asked.pop_front().unwrap_or(false);
            match answer {
                // Cannot say: nothing is known, so nothing is reported
                // unless somebody asked.
                None if !auto => self.checks.unknown.push(n),
                None => {}
                Some(ok) => {
                    if !ok {
                        disagreed.push(n);
                    }
                    if !auto || !ok {
                        self.checks.agreed.push((n, ok));
                    }
                }
            }
        }
        let cursor = self.client.replica.cursor;
        let due = self.checks.served && !paging && self.checks.at != Some(cursor);
        if self.authority.is_none() && self.client.linked && due {
            self.ask_verify(true);
        }
        disagreed
    }

    // -- the live channel -------------------------------------------------------

    /// Say a frame to this account's room. Dropped, not queued, while
    /// unlinked: "pause" is not worth saying on Friday.
    pub fn say(&mut self, frame: Vec<u8>) {
        self.client.say(frame);
    }

    /// Say an ArkDB value, as its canonical CBOR.
    pub fn say_value(&mut self, v: &Value) {
        self.say(canon::encode(v));
    }

    /// What the room said since the last ask. Never durable: a frame is
    /// about now, and a reconnect clears what was not taken.
    pub fn heard(&mut self) -> Vec<Vec<u8>> {
        self.client.take_heard()
    }

    /// [`Peer::heard`], each decoded as canonical CBOR; a frame that does
    /// not decode is dropped (a newer peer's sentence) and counted.
    pub fn heard_values(&mut self) -> Vec<Value> {
        let mut out = vec![];
        for f in self.heard() {
            match canon::decode(&f) {
                Ok(v) => out.push(v),
                Err(_) => self.bad_frames += 1,
            }
        }
        out
    }

    /// How many connections this peer has had: a live room that has never
    /// heard of this device is one whose epoch this peer has not introduced
    /// itself on.
    pub fn epoch(&self) -> i64 {
        self.client.epoch
    }

    /// Whether the engine is linked: the socket is open and `Hello` said.
    pub fn linked(&self) -> bool {
        self.client.linked
    }

    // -- the socket ---------------------------------------------------------------

    /// Dial the server's sync socket (`ws://host/sync`) and keep it up:
    /// every [`Peer::pump`] moves frames, and a drop dials again with a
    /// backoff. Replaces any link there was.
    pub fn connect(&mut self, url: &str) {
        let dial = platform_dial(&self.timing);
        self.connect_with(url, dial);
    }

    /// [`Peer::connect`] over a transport of the caller's.
    ///
    /// Signed out, the link is kept and not dialled: [`Peer::sign_in`] is
    /// what starts it.
    pub fn connect_with(&mut self, url: &str, dial: Dial) {
        self.disconnect();
        self.client.denied = None;
        let mut link = Link::new(url, dial, self.timing.clone());
        if self.signed_out {
            link.stop("signed out");
        }
        self.link = Some(link);
    }

    /// Go offline: close the socket and stop dialling. Everything authored
    /// meanwhile is pending, durable, and pushed on the next connection.
    pub fn disconnect(&mut self) {
        if let Some(link) = &mut self.link {
            link.stop("offline");
        }
        if self.client.linked {
            self.client.disconnected();
        }
    }

    /// Dial again after [`Peer::disconnect`] or a denial — with a new token,
    /// usually. Signed out, nothing: [`Peer::sign_in`] is the way back.
    pub fn reconnect(&mut self) {
        if self.signed_out {
            return;
        }
        self.client.denied = None;
        if let Some(link) = &mut self.link {
            if link.is_open() {
                link.stop("reconnecting");
                self.client.disconnected();
            }
            link.resume();
        }
    }

    /// The link, if [`Peer::connect`] made one.
    pub fn link(&self) -> Option<&Link> {
        self.link.as_ref()
    }

    /// One turn of the link: dial if due, hand the engine what arrived,
    /// settle it once, send what it queued, and write whatever moved. Call
    /// it on a tick (fifty milliseconds is what the clients here use).
    ///
    /// The one [`ark::protocol::Client::settle`] of a pump is here, after
    /// the last frame the link polled (`docs/plan-perf.md` R8): every
    /// frame goes to the replica's inbox first, so with K intents pending
    /// the fifty pushes a busy second brings re-run the K once, not fifty
    /// times. It lives in the driver rather than in `Client::recv` because
    /// only the driver knows where a pump ends; the sans-io `Client` gets
    /// its frames one at a time and is told.
    pub fn pump(&mut self) -> Pumped {
        let mut p = Pumped::default();
        if let Some(link) = &mut self.link {
            let polled = link.poll();
            if polled.opened {
                self.client.connected();
                p.opened = true;
            }
            for f in polled.frames {
                p.moved = true;
                if let Err(e) = self.place_frame(&f) {
                    p.note = Some(e.to_string());
                }
            }
            if let Some(n) = self.settle().first() {
                p.note = Some(format!("the server's state at {n} is not this replica's"));
            }
            if let Some(why) = polled.closed {
                self.client.disconnected();
                p.dropped = Some(why);
            }
            if let Some(reason) = self.client.denied.clone() {
                if let Some(link) = &mut self.link {
                    if link.state() != &State::Idle {
                        link.stop(&format!("denied: {reason}"));
                        p.denied = Some(reason);
                    }
                }
            }
            let frames = self.take_outgoing_frames();
            if let Some(link) = &mut self.link {
                for f in frames {
                    link.send(f);
                }
            }
        }
        let before = self.rejections.len();
        self.collect_rejections();
        p.rejected = self.rejections.len() - before;
        if let Err(e) = self.persist() {
            p.note = Some(e.to_string());
        }
        p
    }

    /// Why the server last turned this login away, until a reconnect.
    pub fn denied(&self) -> Option<&str> {
        self.client.denied.as_deref()
    }

    // -- sans-io: for a transport of the caller's own ---------------------------

    /// A connection opened: `Hello` at the cursor, then everything pending.
    pub fn connected(&mut self) {
        self.client.connected();
    }

    /// The connection is gone; what was queued is dropped.
    pub fn disconnected(&mut self) {
        self.client.disconnected();
    }

    /// A message from the server, handed in by a transport of the
    /// caller's own: a pump of one frame, so it is settled at once (R8).
    pub fn recv(&mut self, msg: ServerMsg) {
        self.place(msg);
        self.settle();
    }

    /// A binary frame from the server: canonical CBOR of a `ServerMsg`. A
    /// pump of one frame, as [`Peer::recv`] is.
    pub fn recv_frame(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.place_frame(bytes)?;
        self.settle();
        Ok(())
    }

    // A message to the engine, not yet settled: what `pump` does with each
    // frame before its one settle.
    fn place(&mut self, msg: ServerMsg) {
        if matches!(msg, ServerMsg::Heard { .. }) {
            self.heard_frames += 1;
        }
        if matches!(msg, ServerMsg::Batch { .. } | ServerMsg::SnapshotOf { .. }) {
            self.served();
        }
        self.client.recv(msg);
    }

    // A page or a snapshot arrived on this connection.
    fn served(&mut self) {
        self.this_connection();
        self.checks.served = true;
    }

    // A frame decoded and placed, not yet settled. A snapshot's rows are
    // built as they are decoded (`docs/plan-db.md` D7.4,
    // `ServerMsg::decode_for`): the whole store, a struct per row, is
    // never a tree first.
    fn place_frame(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let got = ServerMsg::decode_for(bytes, &self.client.schema).map_err(|e| {
            self.bad_frames += 1;
            Error::Corrupt(format!("a frame from the server: {e}"))
        })?;
        match got {
            Received::Msg(msg) => self.place(msg),
            Received::Snapshot(s) => {
                self.served();
                self.client.recv_snapshot(s);
            }
        }
        Ok(())
    }

    /// What the engine queued, oldest first. Empty while unlinked.
    pub fn take_outgoing(&mut self) -> Vec<ClientMsg> {
        self.client.take_outgoing()
    }

    /// [`Peer::take_outgoing`], each as its wire bytes.
    pub fn take_outgoing_frames(&mut self) -> Vec<Vec<u8>> {
        self.client.take_outgoing().iter().map(|m| canon::encode(&m.to_value())).collect()
    }

    /// Write down what moved: the pending intents when they did, the login
    /// when it did, then what the confirmed store moved by — in that order,
    /// so a stop between the first and the last leaves an intent to be
    /// sent again rather than one applied twice. Each goes down as a page
    /// of what moved since the last write, or as a snapshot when a page
    /// cannot say it or the pages have outgrown the last one — and the
    /// pending intents as a snapshot, too, once none are left (the
    /// `storage` module docs). `pump` calls it on every turn, with or
    /// without a link, and so does dropping the peer; a caller driving the
    /// sans-io half by hand calls it after `recv`.
    ///
    /// Alone, the local history goes first: an entry on the storage and
    /// still pending there — a stop before the pending record moved — is
    /// confirmed by the log when the peer reopens, never sequenced twice
    /// (`docs/plan-alone.md` §2).
    pub fn persist(&mut self) -> Result<(), Error> {
        self.persist_log()?;
        self.persist_pending()?;
        self.compact_pending()?;
        if self.wrote_who.as_ref() != Some(&self.ctx) {
            self.storage.save(ReplicaFile::WHO, &encode_who(&self.ctx.user, &self.ctx.session))?;
            self.wrote_who = Some(self.ctx.clone());
        }
        self.persist_confirmed()
    }

    /// The journal since the last write, as one page; or a snapshot.
    fn persist_confirmed(&mut self) -> Result<(), Error> {
        let run = match self.client.replica.take_confirmed() {
            Journal::Replaced => {
                self.durable.snapshot_due = true;
                vec![]
            }
            Journal::Facts(run) => run,
        };
        // A run that does not start where the storage ends cannot be a page
        // (nothing should make one; a snapshot is right whatever did).
        let follows = run.first().is_none_or(|(n, _)| *n == self.durable.cursor + 1);
        let renamed = self.client.replica.log_id != self.durable.log_id;
        if self.durable.snapshot_due || !follows || renamed {
            return self.snapshot();
        }
        let Some((to, _)) = run.last() else { return Ok(()) };
        let to = *to;
        let bytes = encode_page(&run);
        if let Err(e) = self.storage.save(&ReplicaFile::page_key(self.durable.pages + 1), &bytes) {
            // The run is out of the replica's journal and not on the
            // storage: only a snapshot can catch up now.
            self.durable.snapshot_due = true;
            return Err(e);
        }
        self.durable.pages += 1;
        self.durable.page_bytes += bytes.len();
        self.durable.cursor = to;
        if self.durable.page_bytes > self.durable.snapshot_bytes {
            self.snapshot()?;
        }
        Ok(())
    }

    /// Compact: the confirmed store whole at the cursor, then the pages it
    /// now holds removed, newest first — so a stop anywhere leaves only
    /// pages at or below the snapshot's cursor, at the front, which `open`
    /// skips.
    fn snapshot(&mut self) -> Result<(), Error> {
        self.durable.snapshot_due = true;
        let r = &self.client.replica;
        let bytes = encode_replica_of(r.cursor, r.log_id, self.fork, &r.confirmed, &self.ctx.user, &self.ctx.session);
        let (cursor, log_id) = (r.cursor, r.log_id);
        self.storage.save(ReplicaFile::KEY, &bytes)?;
        for n in (1..=self.durable.pages).rev() {
            self.storage.remove(&ReplicaFile::page_key(n))?;
            self.durable.pages = n - 1;
        }
        self.durable = Durable {
            snapshot_bytes: bytes.len(),
            pages: 0,
            page_bytes: 0,
            cursor,
            log_id,
            snapshot_due: false,
        };
        Ok(())
    }

    /// The pending intents, if they moved since they were last written: one
    /// page of what moved — what `mutate` writes, before it returns, and
    /// nothing else (`docs/plan-perf.md` §R3) — or a snapshot where the
    /// storage's copy is unknown, the move is not a page's shape, or nothing
    /// is left pending.
    ///
    /// Whether the list only grew is decided by one comparison, not by
    /// reading the list (`docs/plan-perf.md` Round 4): comparing what was
    /// last written with what is pending, id by id, on every `mutate` made a
    /// tap cost the backlog — about 2 ns an intent, linear in it. The
    /// pending list changes in two ways only — intents leave it, in any
    /// place, and the rest keep their order (an answer, a rebase's replay
    /// dropping one, a snapshot re-opened with them on top), and new intents
    /// join at the end with ids never held before — so the last intent
    /// written is still at its place exactly when nothing before it left:
    /// then what is pending is what was written with the rest after it, and
    /// the same length is the same list. A debug build compares them whole
    /// and says so if not.
    fn persist_pending(&mut self) -> Result<(), Error> {
        let pending = &self.client.replica.pending;
        let Some(held) = &self.wrote_pending else {
            return self.snapshot_pending();
        };
        let n = held.len();
        let grew = n == 0 || pending.get(n - 1).is_some_and(|e| same_id(&e.id, &held[n - 1]));
        debug_assert!(
            !grew || held.iter().zip(pending).all(|(h, e)| *h == e.id),
            "the last intent written is in its place, and one before it is not"
        );
        if grew && n == pending.len() {
            return Ok(());
        }
        if pending.is_empty() {
            return self.snapshot_pending();
        }
        // What `mutate` makes — the same list with intents after it — is
        // found by that one comparison; a pump's answers take a set.
        let (add, drop): (Vec<&ark::log::Entry>, Vec<Id>) = if grew {
            (pending[n..].iter().collect(), vec![])
        } else {
            let now: BTreeSet<Id> = pending.iter().map(|e| e.id).collect();
            let was: BTreeSet<Id> = held.iter().copied().collect();
            let add: Vec<&ark::log::Entry> = pending.iter().filter(|e| !was.contains(&e.id)).collect();
            let drop: Vec<Id> = held.iter().filter(|i| !now.contains(*i)).copied().collect();
            // A page says "without these, then these": a list that moved
            // any other way is a snapshot.
            let says = held.iter().filter(|i| now.contains(*i)).chain(add.iter().map(|e| &e.id));
            if !says.eq(pending.iter().map(|e| &e.id)) {
                return self.snapshot_pending();
            }
            (add, drop)
        };
        let bytes = encode_pending_page(self.pending_file.gen, &add, &drop);
        let key = ReplicaFile::pending_page_key(self.pending_file.pages + 1);
        let added: Vec<Id> = add.iter().map(|e| e.id).collect();
        if let Err(e) = self.storage.save(&key, &bytes) {
            // Whether it landed is not known: a snapshot says it either way.
            self.wrote_pending = None;
            return Err(e);
        }
        self.pending_file.pages += 1;
        self.pending_file.page_bytes += bytes.len();
        let held = self.wrote_pending.as_mut().expect("checked above");
        if !drop.is_empty() {
            let gone: BTreeSet<Id> = drop.into_iter().collect();
            held.retain(|i| !gone.contains(i));
        }
        held.extend(added);
        Ok(())
    }

    /// Compact the pending intents once their pages outgrow the snapshot, or
    /// once nothing is pending and pages are left: on a pump, never inside
    /// `mutate`, so a tap costs its page and nothing else.
    fn compact_pending(&mut self) -> Result<(), Error> {
        let f = &self.pending_file;
        if f.pages > 0 && (f.page_bytes > f.snapshot_bytes || self.client.replica.pending.is_empty()) {
            self.snapshot_pending()?;
        }
        Ok(())
    }

    /// The pending intents whole, as a snapshot of the next generation,
    /// then the pages it now holds removed, newest first — so a stop
    /// anywhere leaves only pages of an older generation, which `open`
    /// skips.
    fn snapshot_pending(&mut self) -> Result<(), Error> {
        self.wrote_pending = None;
        let pending = &self.client.replica.pending;
        let gen = self.pending_file.gen + 1;
        let bytes = encode_pending_snapshot(gen, pending);
        let ids: Vec<Id> = pending.iter().map(|e| e.id).collect();
        self.storage.save(ReplicaFile::PENDING, &bytes)?;
        self.pending_file.gen = gen;
        self.pending_file.snapshot_bytes = bytes.len();
        for n in (1..=self.pending_file.pages).rev() {
            self.storage.remove(&ReplicaFile::pending_page_key(n))?;
            self.pending_file.pages = n - 1;
        }
        self.pending_file.page_bytes = 0;
        self.wrote_pending = Some(ids);
        Ok(())
    }

    pub fn status(&self) -> Status {
        let joining = self.joining();
        let link = match (&self.link, self.alone) {
            (_, true) => "alone",
            (_, false) if self.signed_out => "signed out",
            (None, false) => "offline",
            (Some(l), false) => match l.state() {
                State::Idle => "idle",
                State::Connecting | State::Open if joining > 0 => "joining",
                State::Connecting => "connecting",
                State::Open => "open",
                State::Waiting(_) => "waiting",
            },
        };
        Status {
            user: self.ctx.user.clone(),
            session: self.ctx.session.clone(),
            alone: self.alone,
            signed_out: self.signed_out,
            linked: self.client.linked,
            link: link.into(),
            joining,
            fork: self.fork,
            url: self.link.as_ref().map(|l| l.url().to_string()),
            opens: self.link.as_ref().map_or(0, |l| l.opens),
            epoch: self.client.epoch,
            cursor: self.client.replica.cursor,
            pending: self.pending_len(),
            held: self.client.held(),
            behind: self.client.behind(),
            diverged: self.client.replica.diverged.len(),
            denied: self.client.denied.clone(),
            last_close: self.link.as_ref().and_then(|l| l.last_close.clone()),
            heard_frames: self.heard_frames,
            bad_frames: self.bad_frames,
        }
    }

    // -- inside -------------------------------------------------------------------

    /// Alone, sequence what is pending, and take the entries out of the
    /// authority's memory into the log's next write: its log keeps a head
    /// and the ids, and the entries live on the storage
    /// (`docs/plan-alone.md` §2).
    fn commit_alone(&mut self) {
        if let Some(a) = &mut self.authority {
            local_commit(a, &mut self.client.replica);
            let taken = a.log.take_entries();
            if let Some(l) = &mut self.alone_log {
                l.unwritten.extend(taken);
            }
        }
    }

    /// Intents a join re-queued that are still pending.
    fn joining(&self) -> usize {
        if self.local.is_empty() {
            return 0;
        }
        self.client.replica.pending.iter().filter(|e| self.local.contains(&e.id)).count()
    }

    /// The entries sequenced alone since the last write, as one page of
    /// the local history; then its pages merged, as `ark::journal` merges
    /// them. A write that fails keeps them for the next, which saves the
    /// same page again: a page is whole or absent.
    fn persist_log(&mut self) -> Result<(), Error> {
        let Some(l) = &mut self.alone_log else { return Ok(()) };
        let Some((to, _, _)) = l.unwritten.last() else { return Ok(()) };
        let to = *to;
        let mut page = vec![];
        for (n, e, f) in &l.unwritten {
            page.extend(records::encode_record(*n, e, f));
        }
        l.journal.append(&mut *self.storage, &page, to).map_err(Error::Storage)?;
        l.unwritten.clear();
        l.journal.merge(&mut *self.storage).map_err(Error::Storage)
    }

    /// The authority a peer alone is: the confirmed store as its state at
    /// `head`, the ids its local history holds, and no entries — its log's
    /// base carries an empty store, since the authority holds the state
    /// and is asked about the head only (`ark::log::Log::take_entries`).
    fn alone_authority(&self, head: Seq, ids: BTreeMap<Id, Seq>) -> Authority {
        let mut a = Authority::new(self.schema.clone(), self.domain.closures().clone());
        a.log = Log {
            base: Snapshot {
                seq: head,
                store: MemoryStore::empty(self.schema.clone()),
                hash: vec![],
                log_id: None,
            },
            entries: BTreeMap::new(),
            ids,
            below: Default::default(),
        };
        a.store = self.client.replica.confirmed.clone();
        a.hold(self.domain.native_list());
        a
    }

    /// Carry on alone from the local history on the storage: read it for
    /// its head and its ids, and bring the confirmed store to the head —
    /// forward by the entries the store had not reached (a stop after the
    /// log was written and before the store was), or rebuilt from the fork
    /// and every entry's facts where the store is ahead of what the log
    /// kept (a page lost after the store was written, which nothing here
    /// does, but a storage can). Entries pending that the log already
    /// holds are confirmed by it, as a server's answer would, and are not
    /// sequenced again.
    fn resume_alone(&mut self) -> Result<(), Error> {
        let layout = Layout::alone();
        let cursor = self.client.replica.cursor;
        let mut ids = BTreeMap::new();
        let mut ahead = vec![];
        let (journal, o) = LogJournal::open(&mut *self.storage, layout.clone(), &self.schema, |n, e, f| {
            ids.insert(e.id, n);
            if n > cursor {
                ahead.push((n, e, Some(f)));
            }
        })
        .map_err(Error::Storage)?;
        let base = o.snapshot.ok_or_else(|| Error::Corrupt("the local history has no snapshot".into()))?;
        let fork = Fork {
            log_id: base.id(),
            cursor: base.horizon(),
        };
        let head = o.head;
        let r = &mut self.client.replica;
        if cursor > head || cursor < fork.cursor {
            let mut st = base.base.store;
            records::read(&*self.storage, &layout, &self.schema, |n, _, f| {
                if n <= head {
                    st.apply_changes(&f);
                }
            })
            .map_err(Error::Storage)?;
            let pending = std::mem::take(&mut r.pending);
            let mut fresh = Replica::open(r.schema.clone(), r.bodies.clone(), st, head, pending);
            fresh.natives = r.natives.clone();
            *r = fresh;
        } else if !ahead.is_empty() {
            r.receive_batch(ahead);
        }
        // An intent pending that the history already holds — a join that
        // stopped after writing its re-queued intents, reopened alone — is
        // the history's, and would never be answered: the replica is opened
        // again without it.
        if r.pending.iter().any(|e| ids.get(&e.id).is_some_and(|n| *n <= head)) {
            let pending: Vec<Entry> = std::mem::take(&mut r.pending)
                .into_iter()
                .filter(|e| ids.get(&e.id).is_none_or(|n| *n > head))
                .collect();
            let mut fresh = Replica::open(r.schema.clone(), r.bodies.clone(), r.confirmed.clone(), r.cursor, pending);
            fresh.natives = r.natives.clone();
            *r = fresh;
            self.wrote_pending = None;
        }
        // Alone, the sequences are the local history's, of no named log.
        r.log_id = None;
        self.authority = Some(self.alone_authority(head, ids));
        self.fork = fork;
        self.alone = true;
        self.signed_out = false;
        self.alone_log = Some(AloneLog { journal, unwritten: vec![] });
        Ok(())
    }

    /// Server → alone (`docs/plan-alone.md` §1): the fork is where the
    /// replica stands — its log and cursor — and the local history starts
    /// there, its snapshot the confirmed store, written before anything
    /// else moves; the authority starts from the same store with nothing
    /// in its log. What is pending is sequenced by the caller, locally.
    fn start_alone(&mut self) -> Result<(), Error> {
        let r = &self.client.replica;
        let fork = Fork {
            log_id: r.log_id,
            cursor: r.cursor,
        };
        let base = Log {
            base: snapshot_of(fork.cursor, r.confirmed.clone()).of_log(fork.log_id),
            entries: BTreeMap::new(),
            ids: BTreeMap::new(),
            below: Default::default(),
        };
        let journal = LogJournal::create(&mut *self.storage, Layout::alone(), &base).map_err(Error::Storage)?;
        drop(base);
        self.client.replica.log_id = None;
        self.authority = Some(self.alone_authority(fork.cursor, BTreeMap::new()));
        self.fork = fork;
        self.alone = true;
        self.signed_out = false;
        self.local.clear();
        self.alone_log = Some(AloneLog { journal, unwritten: vec![] });
        Ok(())
    }

    /// Alone → server, up to the connecting (`docs/plan-alone.md` §1): the
    /// local history read back from the storage, taken out of the confirmed
    /// store newest first ([`Replica::fork_back`]) — the store, the cursor
    /// and the log back at the fork — and every local entry re-queued as
    /// pending in order, as `who`: the same ids, functions, autos and
    /// arguments, nothing drawn again. Then, in this order, the re-queued
    /// intents, the login and the fork's replica written, and only then the
    /// local history removed — so a stop between the two reopens with both
    /// and finishes the join ([`Peer::open`]).
    fn fork_back_to(&mut self, who: &Ctx) -> Result<(), Error> {
        self.persist_log()?;
        let fork = self.fork;
        let layout = Layout::alone();
        let (mut facts, mut requeue) = (vec![], vec![]);
        let o = records::read(&*self.storage, &layout, &self.schema, |n, mut e, f| {
            if n > fork.cursor {
                e.actor = who.user.clone();
                e.session = who.session.clone();
                requeue.push(e);
                facts.push(f);
            }
        })
        .map_err(Error::Storage)?;
        let cursor = self.client.replica.cursor;
        if o.head != cursor || cursor - fork.cursor != facts.len() as Seq {
            return Err(Error::Corrupt(format!(
                "the local history runs from {} to {} and the store is at {cursor}",
                fork.cursor, o.head
            )));
        }
        self.local = requeue.iter().map(|e| e.id).collect();
        self.authored.extend(self.local.iter().copied());
        self.client.replica.fork_back(facts, fork.cursor, fork.log_id, requeue);
        self.authority = None;
        self.alone_log = None;
        self.alone = false;
        self.collect_rejections();
        self.wrote_pending = None;
        self.persist()?;
        LogJournal::destroy(&mut *self.storage, &layout).map_err(Error::Storage)
    }

    fn collect_rejections(&mut self) {
        // One intent can be refused twice: by this peer's own rebase, when a
        // confirmed entry leaves it nothing to apply to, and then by the
        // authority it had already been pushed to. It is one verdict.
        for (id, why) in std::mem::take(&mut self.client.replica.rejections) {
            if self.rejected.contains_key(&id) {
                continue;
            }
            let reason = refusal_text(&why);
            self.rejected.insert(id, reason.clone());
            self.rejections.push(Rejection { id, reason });
        }
    }
}

#[cfg(test)]
impl Peer {
    /// Entries this peer holds in memory: its authority's log, and the
    /// local history not yet written (`docs/plan-alone.md` §2).
    pub(crate) fn entries_held(&self) -> usize {
        self.authority.as_ref().map_or(0, |a| a.log.entries.len()) + self.alone_log.as_ref().map_or(0, |l| l.unwritten.len())
    }

    /// The authority's log: its head and its ids.
    pub(crate) fn authority_log(&self) -> Option<&Log> {
        self.authority.as_ref().map(|a| &a.log)
    }
}

/// A verdict as a person reads it: a mutator's own refusal word for word,
/// a constraint named in a sentence (`ark::protocol::refusal_text`).
pub fn refusal_text(r: &Refusal) -> String {
    ark::protocol::refusal_text(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{encode_pending, Storage};
    use crate::{args, demo};
    use ark::store::Store;

    /// An app that reopens with the login rather than calling `sign_in`
    /// gets the same answer: what nobody authored is the login's.
    #[test]
    fn opening_with_a_login_over_work_nobody_authored_makes_it_theirs() {
        let disk = Memory::new();
        let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), Options::signed_out()).unwrap();
        let id = p.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        drop(p);
        let p = Peer::open(demo::domain(), Box::new(disk.clone()), Options::server("bob", "s1", None)).unwrap();
        assert_eq!(p.replica().pending[0].actor, "bob");
        assert_eq!(p.replica().pending[0].session, "s1");
        assert_eq!(p.store().scan("playlist")[0]["user_id"], Value::text("bob"));
        assert_eq!(p.standing(&id), Standing::Pending);
        drop(p);
        // …and it was written down, not only replayed.
        let f = ReplicaFile::load(&disk, &demo::domain().module().schema).unwrap().unwrap();
        assert_eq!((f.pending[0].actor.as_str(), f.user.as_str()), ("bob", "bob"));
    }

    /// `mutate` writes the intents and not the store: with a server the
    /// store did not move; alone it did, and follows on `pump` — and on
    /// drop, for a program that closes between the two. Falsified by
    /// `mutate` calling `persist`: the store record moves on the first
    /// mutate, alone. (What a pump writes, a page or a snapshot, is
    /// `persistence_tests`'.)
    #[test]
    fn authoring_writes_the_intent_and_the_store_follows() {
        let disk = Memory::new();
        let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), Options::dev("alice")).unwrap();
        let store_at_open = disk.load(ReplicaFile::KEY).unwrap().unwrap();
        assert!(
            disk.load(ReplicaFile::PENDING).unwrap().is_some(),
            "both records exist from the first open"
        );
        let pending_at_open = disk.load(ReplicaFile::PENDING).unwrap().unwrap();
        p.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        assert_eq!(disk.load(ReplicaFile::KEY).unwrap().unwrap(), store_at_open, "the store did not move");
        assert_eq!(
            disk.load(ReplicaFile::PENDING).unwrap().unwrap(),
            pending_at_open,
            "the intents' snapshot did not move"
        );
        assert!(
            disk.load(&ReplicaFile::pending_page_key(1)).unwrap().is_some(),
            "the intent was written, as a page"
        );
        drop(p);

        let disk = Memory::new();
        let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), Options::alone("me")).unwrap();
        let store_at_open = disk.load(ReplicaFile::KEY).unwrap().unwrap();
        p.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        assert_eq!(p.cursor(), 1);
        assert_eq!(
            disk.load(ReplicaFile::KEY).unwrap().unwrap(),
            store_at_open,
            "alone, the store waits for a pump"
        );
        assert!(disk.load(&ReplicaFile::page_key(1)).unwrap().is_none(), "…the journal too");
        let schema = demo::domain().module().schema.clone();
        assert_eq!(ReplicaFile::load(&disk, &schema).unwrap().unwrap().cursor, 0);
        p.pump();
        assert_eq!(ReplicaFile::load(&disk, &schema).unwrap().unwrap().cursor, 1, "and moves on one");
        p.mutate("create_playlist", args([("name", Value::text("Two"))])).unwrap();
        drop(p);
        let p = Peer::open(demo::domain(), Box::new(disk.clone()), Options::alone("me")).unwrap();
        assert_eq!(p.cursor(), 2, "a drop writes what the next pump would have");
    }

    /// A storage holding intents and no store yet — a run that stopped
    /// between writing the one and the other — keeps them. Falsified by
    /// `open` reading only the store record.
    #[test]
    fn intents_written_before_any_store_are_kept() {
        let mut author = Peer::open_memory(demo::domain(), Options::dev("alice")).unwrap();
        author.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        let mut disk = Memory::new();
        disk.save(ReplicaFile::PENDING, &encode_pending(&author.replica().pending)).unwrap();
        let p = Peer::open(demo::domain(), Box::new(disk), Options::dev("alice")).unwrap();
        assert_eq!(p.pending_len(), 1);
        assert_eq!(p.store().scan("playlist").len(), 1, "and replayed on top of the empty store");
    }

    /// Round 4: a peer whose storage names no log — written before logs
    /// had names — syncs as it did, and learns the name from the first page
    /// a server that names its log sends it; the name is written as a
    /// snapshot at once, not left for a compaction, and a reopen says it in
    /// its `Hello`. Here twenty entries arrive from a server not yet
    /// naming its log, which then names it (the server upgraded) and
    /// sequences one more. Falsified by `persist_confirmed` not treating a
    /// renamed replica as due a snapshot: the page is appended, the
    /// snapshot on the storage stays unnamed, and the reopened peer's
    /// `Hello` names nothing.
    #[test]
    fn a_peer_learns_the_log_it_holds_and_writes_it_down() {
        use crate::storage::Stored;
        use ark::live::Silent;
        use ark::protocol::{open_access, trusting, Server};
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let mut a = ark::peer::Authority::new(schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let mut sv = Server::open(trusting(), open_access(), Silent, a);
        let exchange = |p: &mut Peer, sv: &mut Server<Silent>| loop {
            let up = p.take_outgoing();
            for m in up.iter().cloned() {
                sv.recv(1, m);
            }
            let down = sv.take_outgoing();
            if up.is_empty() && down.is_empty() {
                p.persist().unwrap();
                return;
            }
            for (_, m) in down {
                p.recv(m);
            }
        };
        let disk = Memory::new();
        let mut p = Peer::open(d.clone(), Box::new(disk.clone()), Options::dev("alice")).unwrap();
        p.connected();
        for i in 0..20 {
            p.mutate("create_playlist", args([("name", Value::text(format!("p{i:02}")))])).unwrap();
            exchange(&mut p, &mut sv);
        }
        assert_eq!((p.cursor(), p.replica().log_id), (20, None));
        let stored = || Stored::load(&disk, &schema).unwrap().unwrap();
        let (st, pages) = (stored(), stored().pages);
        assert!(
            pages > 0 && st.page_bytes + 2 * st.page_bytes / pages < st.snapshot_bytes,
            "the next page is not a compaction: {} bytes of {pages} pages beside {}",
            st.page_bytes,
            st.snapshot_bytes
        );

        sv.authority.log.name_if_unnamed([3; 16]);
        p.mutate("create_playlist", args([("name", Value::text("named"))])).unwrap();
        exchange(&mut p, &mut sv);
        assert_eq!((p.cursor(), p.replica().log_id), (21, Some([3; 16])));
        assert_eq!((stored().log_id, stored().pages), (Some([3; 16]), 0), "written as a snapshot");
        drop(p);

        let mut p = Peer::open(d, Box::new(disk.clone()), Options::dev("alice")).unwrap();
        assert_eq!(p.replica().log_id, Some([3; 16]));
        p.connected();
        let hello = p.take_outgoing().into_iter().next();
        assert!(
            matches!(&hello, Some(ClientMsg::Hello { sub, .. }) if sub.log_id == Some([3; 16]) && sub.since == 21),
            "{hello:?}"
        );
    }

    /// D3 of `docs/plan-db.md`: a linked peer verifies after every settle
    /// that moved its cursor, without being asked, and a divergence reaches
    /// [`Peer::agreed`] when it happens. Ten intents land with ten
    /// verifies said and nothing reported; then a raw fact is applied to
    /// the replica's stores behind the engine's back, and the
    /// next intent to land is answered `(11, false)` — and only that, with
    /// no `verify()` called. Falsified by never verifying after a settle:
    /// no verify is said and the divergence goes unreported.
    #[test]
    fn a_linked_peer_verifies_after_every_settle_and_reports_a_divergence() {
        use ark::live::Silent;
        use ark::protocol::{open_access, trusting, Server};
        use ark::store::{Change, Row};
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let mut a = ark::peer::Authority::new(schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let mut sv = Server::open(trusting(), open_access(), Silent, a);
        let said = std::cell::Cell::new(0);
        let exchange = |p: &mut Peer, sv: &mut Server<Silent>| loop {
            let up = p.take_outgoing();
            said.set(said.get() + up.iter().filter(|m| matches!(m, ClientMsg::Verify { .. })).count());
            for m in up.iter().cloned() {
                sv.recv(1, m);
            }
            let down = sv.take_outgoing();
            if up.is_empty() && down.is_empty() {
                return;
            }
            for (_, m) in down {
                p.recv(m);
            }
        };
        let mut p = Peer::open_memory(d.clone(), Options::dev("alice")).unwrap();
        p.connected();
        for i in 0..10 {
            p.mutate("create_playlist", args([("name", Value::text(format!("p{i}")))])).unwrap();
            exchange(&mut p, &mut sv);
        }
        assert_eq!(p.cursor(), 10);
        assert!(p.agreed().is_empty(), "agreeing is not news: {:?}", p.agreed());

        let tbl = schema.lookup_table("playlist").unwrap();
        let stray = Row::of(
            tbl,
            [
                ("id".to_string(), Value::Id([7; 16])),
                ("name".to_string(), Value::text("nobody wrote this")),
                ("user_id".to_string(), Value::text("alice")),
            ],
        );
        let stray = Change::Add("playlist".into(), stray);
        p.client.replica.confirmed.apply_change(&stray);
        p.client.replica.view.apply_change(&stray);
        p.mutate("create_playlist", args([("name", Value::text("p10"))])).unwrap();
        exchange(&mut p, &mut sv);
        assert_eq!(said.get(), 11, "a verify per settle that moved the cursor, and no more");
        assert_eq!(p.agreed(), [(11, false)], "the divergence, reported unasked");
    }

    /// D3: an answer of "cannot say" — the authority holds no state at the
    /// sequence, below its horizon or past its head — is not a divergence.
    /// To the verify a linked peer makes on its own it reports nothing at
    /// all; to one asked for it is an [`Peer::unknown`], not an
    /// [`Peer::agreed`] of `false`. Falsified by reading "cannot say" as
    /// `ok: false`: the automatic one is reported as `(1, false)`.
    #[test]
    fn a_verify_answered_cannot_say_reports_no_divergence() {
        use ark::live::Silent;
        use ark::protocol::{open_access, trusting, Server};
        let d = demo::domain();
        let mut a = ark::peer::Authority::new(d.module().schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let mut sv = Server::open(trusting(), open_access(), Silent, a);
        // Every frame to the server but a `Verify`, which is answered here,
        // as an authority past whose horizon the cursor has fallen would.
        let exchange = |p: &mut Peer, sv: &mut Server<Silent>| loop {
            let up = p.take_outgoing();
            let mut cannot = vec![];
            for m in up.iter().cloned() {
                match m {
                    ClientMsg::Verify { seq, hash } => cannot.push(ServerMsg::Agree {
                        seq,
                        hash,
                        ok: false,
                        unknown: true,
                    }),
                    m => sv.recv(1, m),
                }
            }
            let down: Vec<ServerMsg> = sv.take_outgoing().into_iter().map(|(_, m)| m).chain(cannot).collect();
            if up.is_empty() && down.is_empty() {
                return;
            }
            for m in down {
                p.recv(m);
            }
        };
        let mut p = Peer::open_memory(d.clone(), Options::dev("alice")).unwrap();
        p.connected();
        p.mutate("create_playlist", args([("name", Value::text("p"))])).unwrap();
        exchange(&mut p, &mut sv);
        assert_eq!(p.cursor(), 1);
        assert_eq!(
            (p.agreed(), p.unknown()),
            (&[][..], &[][..]),
            "an automatic verify, unanswerable: nothing"
        );
        p.verify();
        exchange(&mut p, &mut sv);
        assert_eq!((p.agreed(), p.unknown()), (&[][..], &[1][..]), "one asked for: unknown");
    }

    /// Round 4: what `mutate` writes is decided by one comparison however
    /// much is pending — one id compared at 300 pending and at 2,400 —
    /// and a pump that moved nothing compares one too. Falsified by
    /// deciding it as it was, id by id through the list written: a mutate
    /// compares 300 at 300 pending, and the pump after it 301.
    #[test]
    fn deciding_what_a_mutate_writes_compares_one_id() {
        let compared = || COMPARED.with(|n| n.get());
        let mut p = Peer::open_memory(demo::domain(), Options::dev("alice")).unwrap();
        let mut per = vec![];
        for n in 0..2_401usize {
            let before = compared();
            p.mutate("create_playlist", args([("name", Value::text(format!("p{n}")))])).unwrap();
            if n == 300 || n == 2_400 {
                per.push(compared() - before);
                let before = compared();
                p.pump();
                assert_eq!(compared() - before, 1, "a pump that moved nothing, at {n}");
            }
        }
        assert_eq!(per, [1, 1], "ids compared by a mutate at 300 and 2,400 pending");
        assert_eq!(p.pending_len(), 2_401);
    }

    /// R8 of `docs/plan-perf.md`, where it lives: a pump hands every frame
    /// its link polled to the engine and settles once. Bob has K = 100
    /// items of his own pending on a playlist while fifty of Alice's land,
    /// each its own `Batch` as the server's fan-out sends it, all polled in
    /// one pump: what his view is told is one rebase — his hundred undone,
    /// her fifty landed, his hundred again, 250 transitions — where a
    /// settle per frame told fifty rebases, 50 × 201. Falsified by settling
    /// in `place` (per frame): 10,050.
    #[test]
    fn a_pump_is_one_rebase_however_many_frames_it_polled() {
        use crate::link::{BoxTransport, Event, Queues};
        use ark::live::Silent;
        use ark::protocol::{open_access, trusting, Server};
        let d = demo::domain();
        let mut a = ark::peer::Authority::new(d.module().schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let mut sv = Server::open(trusting(), open_access(), Silent, a);
        let frame = |m: &ServerMsg| Event::Frame(canon::encode(&m.to_value()));
        let decode = |f: &[u8]| ClientMsg::from_value(&canon::decode(f).unwrap()).unwrap();

        let q = Queues::default();
        let dialled = q.clone();
        let mut bob = Peer::open_memory(d.clone(), Options::dev("bob")).unwrap();
        bob.connect_with("queues", Box::new(move |_: &str| Box::new(dialled.clone()) as BoxTransport));
        let events = |es: Vec<Event>| q.events.lock().unwrap().extend(es);
        let up = |sv: &mut Server<Silent>| {
            for f in std::mem::take(&mut *q.sent.lock().unwrap()) {
                sv.recv(2, decode(&f));
            }
        };
        let down = |sv: &mut Server<Silent>| -> Vec<Event> { sv.take_outgoing().iter().filter(|(c, _)| *c == 2).map(|(_, m)| frame(m)).collect() };
        bob.pump();
        events(vec![Event::Opened]);
        bob.pump();
        up(&mut sv);

        let mut alice = Peer::open_memory(d.clone(), Options::dev("alice")).unwrap();
        alice.connected();
        let mut to_bob = vec![];
        let exchange = |alice: &mut Peer, sv: &mut Server<Silent>, to_bob: &mut Vec<Event>| {
            for m in alice.take_outgoing() {
                sv.recv(1, m);
            }
            for (c, m) in sv.take_outgoing() {
                match c {
                    1 => alice.recv(m),
                    _ => to_bob.push(frame(&m)),
                }
            }
        };
        exchange(&mut alice, &mut sv, &mut to_bob);
        alice.mutate("create_playlist", args([("name", Value::text("Shared"))])).unwrap();
        exchange(&mut alice, &mut sv, &mut to_bob);
        exchange(&mut alice, &mut sv, &mut to_bob);
        let shared = alice.store().scan("playlist")[0]["id"].clone();
        to_bob.extend(down(&mut sv));
        events(std::mem::take(&mut to_bob));
        bob.pump();
        assert_eq!(bob.cursor(), 1, "the playlist reached bob");

        for i in 0..100 {
            bob.mutate(
                "add_to_playlist",
                args([("playlist_id", shared.clone()), ("track_id", Value::text(format!("b{i}")))]),
            )
            .unwrap();
        }
        for i in 0..50 {
            alice
                .mutate(
                    "add_to_playlist",
                    args([("playlist_id", shared.clone()), ("track_id", Value::text(format!("a{i}")))]),
                )
                .unwrap();
            exchange(&mut alice, &mut sv, &mut to_bob);
        }
        assert_eq!(to_bob.len(), 50, "a frame a push");
        let _ = bob.take_changes();
        events(to_bob);
        bob.pump();
        assert_eq!((bob.cursor(), bob.pending_len()), (51, 100));
        let Changes::Applied(told) = bob.take_changes() else {
            panic!("a rebase is its transitions")
        };
        assert_eq!(told.len(), 100 + 50 + 100, "one rebase for fifty frames");
        let items = bob.store().scan("item");
        assert_eq!(items.len(), 150);
        let last = items.iter().map(|r| r["pos"].clone()).max().unwrap();
        assert_eq!(last, Value::int(150), "his hundred after her fifty");
    }

    #[test]
    fn a_peer_alone_has_nobody_to_sign_in_to() {
        let mut p = Peer::open_memory(demo::domain(), Options::alone("me")).unwrap();
        assert_eq!(p.sign_in("alice", "s", None), Err(Error::Alone));
        assert_eq!(p.ctx().user, "me");
    }
}
