//! §11 The peer, as `Ark.Peer` defines it: the log machine every peer runs
//! ([`Replica`]), and the authority role a peer takes for the log it
//! sequences ([`Authority`]). Both are sans-io.
//!
//! A replica's view is always `replay(confirmed) then replay(pending)`.
//! Confirmed state moves only forward; the only thing ever undone is the
//! replica's own pending, replayed on top when confirmed entries land. That
//! is the rebase. A replica that holds the closure an entry names replays
//! the intent; one that does not asks for the facts; when it holds both it
//! replays and compares, and a disagreement is recorded in `diverged` and
//! resolved in the authority's favour.
//!
//! An entry is applied by a native procedure when the peer holds one for
//! its hash ([`crate::authoring::Procedure`], the domain's own code run
//! `Native`), otherwise through the closure the hash names
//! ([`crate::eval::apply_closure`]), otherwise by facts. The two are held to
//! each other by the runtime's tests on every procedure; both are this
//! file's "the closure is held".
//!
//! The stores here are [`MemoryStore`]s, as the spec's are: the optimistic
//! view is a value recomputed on every rebase.

use std::collections::{BTreeMap, BTreeSet};

pub use crate::authoring::Procedure;
use crate::eval::{apply_closure, Args, Ctx, EvalError};
use crate::hash::{state_hash, Closure, FnHash};
use crate::log::{Entry, Facts, Log, Page, Seq};
use crate::schema::Schema;
use crate::store::{Change, MemoryStore, Overlay, Refusal, Store};
use crate::value::{hex, Id};

// ---------------------------------------------------------------------
// A replica

/// One peer's copy of the log (`Ark.Peer.Replica`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replica {
    pub schema: Schema,
    /// The closures this peer can run: its module's, plus any it was sent.
    /// Keyed by the hash an entry names.
    pub bodies: BTreeMap<FnHash, Closure>,
    /// The procedures this peer runs natively, by the same hash; preferred
    /// over the closure when both are held.
    pub natives: BTreeMap<FnHash, Procedure>,
    /// The confirmed store: the tables exactly as the authority had them at
    /// `cursor`. Durable; moves only forward.
    pub confirmed: MemoryStore,
    pub cursor: Seq,
    /// Intents authored here that no verdict has answered, in authoring
    /// order. Durable.
    pub pending: Vec<Entry>,
    /// The optimistic store: `confirmed` with `pending` replayed. Never
    /// durable; recomputed on every rebase.
    pub view: MemoryStore,
    /// Confirmed entries received and not yet applied.
    pub inbox: BTreeMap<Seq, Inbox>,
    /// Verdicts against this peer's own intents, newest last.
    pub rejections: Vec<(Id, Refusal)>,
    /// Sequences at which this peer's replay disagreed with the authority's
    /// facts. Empty on a conformant runtime; never empty silently.
    pub diverged: Vec<Seq>,
    /// What has happened to `view` since `take_changes` last asked.
    pub rebuilt: bool,
    /// Oldest first.
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Inbox {
    pub entry: Option<Entry>,
    pub facts: Option<Facts>,
}

/// What a view is told: the changes to the optimistic store since it last
/// asked, or that it was rebuilt and must re-hydrate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Changes {
    Applied(Vec<Change>),
    Rebuilt,
}

fn ctx_of(e: &Entry) -> Ctx {
    Ctx {
        user: e.actor.clone(),
        session: e.session.clone(),
    }
}

fn bug_text(e: &EvalError) -> String {
    format!("bug: {e:?}")
}

type Applied = Result<Result<Vec<Change>, Refusal>, EvalError>;

#[allow(clippy::too_many_arguments)]
/// Apply an entry's function by its hash: natively when a procedure is
/// held, through the closure otherwise; `None` when neither is.
fn run(
    schema: &Schema,
    bodies: &BTreeMap<FnHash, Closure>,
    natives: &BTreeMap<FnHash, Procedure>,
    fh: &FnHash,
    ctx: &Ctx,
    autos: &Args,
    args: &Args,
    store: &mut dyn Store,
) -> Option<Applied> {
    if let Some(p) = natives.get(fh) {
        return Some(p.apply(ctx, autos, args, store));
    }
    bodies.get(fh).map(|c| apply_closure(schema, c, ctx, autos, args, store))
}

/// Hold native procedures, and the closures they carry.
fn hold(bodies: &mut BTreeMap<FnHash, Closure>, natives: &mut BTreeMap<FnHash, Procedure>, procs: impl IntoIterator<Item = (FnHash, Procedure)>) {
    for (h, p) in procs {
        bodies.entry(h.clone()).or_insert_with(|| p.closure().clone());
        natives.insert(h, p);
    }
}

impl Replica {
    /// §11.1 Open a replica from what was durable: the confirmed store and
    /// cursor, and the pending intents, which are replayed on top.
    pub fn open(schema: Schema, bodies: BTreeMap<FnHash, Closure>, confirmed: MemoryStore, cursor: Seq, pending: Vec<Entry>) -> Replica {
        let mut r = Replica {
            schema,
            bodies,
            natives: BTreeMap::new(),
            view: confirmed.clone(),
            confirmed,
            cursor,
            pending,
            inbox: BTreeMap::new(),
            rejections: vec![],
            diverged: vec![],
            rebuilt: true,
            changes: vec![],
        };
        r.replay();
        r
    }

    /// §11.2 Author an intent: apply it forward into the optimistic store,
    /// and if it is not refused, record it as pending. A refusal changes
    /// nothing and records nothing.
    pub fn mutate(&mut self, id: Id, ctx: &Ctx, fh: &FnHash, autos: &Args, args: &Args) -> Result<Entry, Refusal> {
        // Applied through an overlay, so a refusal leaves the view untouched
        // and an acceptance costs its changes, not a copy of the store.
        let out = {
            let mut over = Overlay::new(&self.view);
            run(&self.schema, &self.bodies, &self.natives, fh, ctx, autos, args, &mut over)
        };
        match out {
            None => Err(Refusal::Refused(format!("unknown function {}", hex(fh)))),
            Some(Err(bug)) => Err(Refusal::Refused(bug_text(&bug))),
            Some(Ok(Err(refusal))) => Err(refusal),
            Some(Ok(Ok(chs))) => {
                let e = Entry {
                    id,
                    actor: ctx.user.clone(),
                    session: ctx.session.clone(),
                    fn_hash: fh.clone(),
                    args: args.clone(),
                    autos: autos.clone(),
                };
                self.view.apply_changes(&chs);
                self.pending.push(e.clone());
                self.changes.extend(chs);
                Ok(e)
            }
        }
    }

    /// Hold native procedures (and the closures they carry): from here on
    /// an entry naming one of their hashes — authored here, replayed on a
    /// rebase, or confirmed — is applied natively.
    pub fn hold(&mut self, procs: impl IntoIterator<Item = (FnHash, Procedure)>) {
        hold(&mut self.bodies, &mut self.natives, procs);
    }

    /// Whether an entry naming this hash can be applied here by intent.
    pub fn can_apply(&self, fh: &FnHash) -> bool {
        self.natives.contains_key(fh) || self.bodies.contains_key(fh)
    }

    /// §11.3 A confirmed entry arrives, at its sequence; it waits in the
    /// inbox until everything before it has been applied.
    pub fn receive(&mut self, n: Seq, e: Entry) {
        if n <= self.cursor {
            return;
        }
        self.inbox.entry(n).or_default().entry = Some(e);
        self.advance();
    }

    /// An entry and its facts arrive together, as a batch delivers them.
    pub fn receive_with(&mut self, n: Seq, e: Entry, f: Facts) {
        if n <= self.cursor {
            return;
        }
        self.inbox.insert(
            n,
            Inbox {
                entry: Some(e),
                facts: Some(f),
            },
        );
        self.advance();
    }

    /// A page of the log arrives: every entry to the inbox, then one
    /// advance — so a replica with an intent of its own pending rebuilds
    /// its view once per page rather than once per entry.
    pub fn receive_batch(&mut self, items: impl IntoIterator<Item = (Seq, Entry, Option<Facts>)>) {
        for (n, e, f) in items {
            if n <= self.cursor {
                continue;
            }
            let ib = self.inbox.entry(n).or_default();
            ib.entry = Some(e);
            if let Some(f) = f {
                ib.facts = Some(f);
            }
        }
        self.advance();
    }

    /// The facts of an entry arrive, at its sequence.
    pub fn receive_facts(&mut self, n: Seq, f: Facts) {
        if n <= self.cursor {
            return;
        }
        self.inbox.entry(n).or_default().facts = Some(f);
        self.advance();
    }

    /// §11.4 An acknowledgement: this peer's own intent was sequenced at
    /// `n`. It goes to the inbox as if it had arrived, and is applied at its
    /// turn through the confirmed store, which is the rebase.
    pub fn ack(&mut self, id: &Id, n: Seq) {
        if let Some(e) = self.pending.iter().find(|e| e.id == *id).cloned() {
            self.receive(n, e);
        }
    }

    /// §11.2b Somebody signs in on a peer that has been used without an
    /// account (`Ark.Peer.signIn`): every pending intent authored as
    /// [`Ctx::nobody`] becomes theirs, under this login, and the view is
    /// replayed so every row those intents wrote says who they now say.
    /// Nothing but this peer has seen them — one authored as nobody can
    /// never have been accepted — so rewriting them is safe. Intents of
    /// anybody else are untouched; an older login of the same person is
    /// the server's question ([`crate::protocol::Server::with_owns`]). One
    /// the replay now refuses is dropped with its reason, as any rebase
    /// does.
    pub fn sign_in(&mut self, who: &Ctx) {
        for e in &mut self.pending {
            if e.actor.is_empty() && e.session.is_empty() {
                e.actor = who.user.clone();
                e.session = who.session.clone();
            }
        }
        self.replay();
    }

    /// §11.5 A verdict against this peer's own intent: it is dropped, the
    /// verdict is kept for the app to show, and the view is rebuilt.
    pub fn reject(&mut self, id: &Id, why: Refusal) {
        self.pending.retain(|e| e.id != *id);
        self.rejections.push((*id, why));
        self.replay();
    }

    /// The sequences the replica is waiting on facts for: entries whose
    /// closures it does not hold, or which it could not apply as the
    /// authority did.
    pub fn needs(&self) -> Vec<Seq> {
        self.inbox
            .iter()
            .filter_map(|(n, ib)| match (&ib.entry, &ib.facts) {
                (Some(e), None) if !self.can_apply(&e.fn_hash) || self.diverged.contains(n) => Some(*n),
                _ => None,
            })
            .collect()
    }

    /// Try the inbox again — after closures arrived, or facts.
    pub fn retry(&mut self) {
        self.advance();
    }

    /// What a view is told, and the slate wiped.
    pub fn take_changes(&mut self) -> Changes {
        let out = if self.rebuilt {
            Changes::Rebuilt
        } else {
            Changes::Applied(std::mem::take(&mut self.changes))
        };
        self.rebuilt = false;
        self.changes.clear();
        out
    }

    /// This replica's claim: its cursor and the hash of its confirmed state
    /// there. What `Verify { seq, hash }` carries.
    pub fn verify_at(&self) -> (Seq, Vec<u8>) {
        (self.cursor, state_hash(&self.confirmed))
    }

    // §11.6 Advancing --------------------------------------------------------

    // Apply from the inbox in order for as long as the next entry can be
    // applied; then decide what the view owes: nothing pending before or
    // after, the confirmed changes are the view's; every entry applied was
    // this peer's own next pending intent in order, nothing is reported;
    // anything else landed under pending intents, the view is rebuilt.
    fn advance(&mut self) {
        let had_pending = !self.pending.is_empty();
        let mut acc: Vec<Change> = Vec::new();
        let mut moved = false;
        let mut others = false;
        loop {
            let n = self.cursor + 1;
            let Some(ib) = self.inbox.get(&n) else { break };
            let Some(e) = ib.entry.clone() else { break };
            let mf = ib.facts.clone();
            let Some((chs, diverged)) = self.apply_one(n, &e, mf.as_ref()) else {
                break;
            };
            let own_next = self.pending.first().is_some_and(|p| p.id == e.id);
            self.confirmed.apply_changes(&chs);
            self.cursor = n;
            self.inbox.remove(&n);
            self.pending.retain(|p| p.id != e.id);
            if diverged {
                self.diverged.push(n);
            }
            acc.extend(chs);
            moved = true;
            others = others || !own_next || diverged;
        }
        if !moved {
            return;
        }
        if !had_pending {
            // Nothing was pending, so the view was the confirmed store: it
            // moves by the same changes.
            self.view.apply_changes(&acc);
            self.changes.extend(acc);
        } else if !others {
            if self.pending.is_empty() {
                self.view = self.confirmed.clone();
            }
        } else {
            self.replay();
        }
    }

    // One entry against the confirmed store: by intent when the closure is
    // held, by facts otherwise; both when both are present, comparing them.
    // The changes the confirmed store moves by, and whether the two
    // disagreed; `None` means it cannot be applied yet. The intent runs
    // over an overlay, so the store is untouched until the caller commits.
    fn apply_one(&self, n: Seq, e: &Entry, mf: Option<&Facts>) -> Option<(Vec<Change>, bool)> {
        if self.can_apply(&e.fn_hash) && !self.diverged.contains(&n) {
            let mut over = Overlay::new(&self.confirmed);
            return match run(
                &self.schema,
                &self.bodies,
                &self.natives,
                &e.fn_hash,
                &ctx_of(e),
                &e.autos,
                &e.args,
                &mut over,
            ) {
                Some(Ok(Ok(chs))) => match mf {
                    Some(f) if *f != chs => Some((f.clone(), true)),
                    _ => Some((chs, false)),
                },
                _ => mf.map(|f| (f.clone(), true)),
            };
        }
        mf.map(|f| (f.clone(), false))
    }

    // Rebuild the view: the confirmed store, then every pending intent in
    // order. One that is now refused is dropped and recorded.
    fn replay(&mut self) {
        self.view = self.confirmed.clone();
        self.rebuilt = true;
        self.changes.clear();
        let pending = std::mem::take(&mut self.pending);
        let mut kept = Vec::with_capacity(pending.len());
        for e in pending {
            let out = {
                let mut over = Overlay::new(&self.view);
                run(
                    &self.schema,
                    &self.bodies,
                    &self.natives,
                    &e.fn_hash,
                    &ctx_of(&e),
                    &e.autos,
                    &e.args,
                    &mut over,
                )
            };
            match out {
                None => self.rejections.push((e.id, Refusal::Refused("no closure for a pending intent".into()))),
                Some(Ok(Ok(chs))) => {
                    self.view.apply_changes(&chs);
                    kept.push(e);
                }
                Some(Ok(Err(why))) => self.rejections.push((e.id, why)),
                Some(Err(bug)) => self.rejections.push((e.id, Refusal::Refused(bug_text(&bug)))),
            }
        }
        self.pending = kept;
    }
}

// ---------------------------------------------------------------------
// An authority

/// The peer that sequences the log (`Ark.Peer.Authority`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authority {
    pub schema: Schema,
    /// Every closure ever accepted, by hash.
    pub bodies: BTreeMap<FnHash, Closure>,
    /// The procedures this authority runs natively, by the same hash.
    pub natives: BTreeMap<FnHash, Procedure>,
    pub log: Log,
    /// The state at the head of the log.
    pub store: MemoryStore,
}

/// The answer to a pushed intent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sequenced {
    /// Applied and appended, at this sequence, with these facts.
    Appended(Seq, Facts),
    /// Seen before: re-acknowledged with the sequence it already has.
    Duplicate(Seq),
    /// The verdict.
    Rejected(Refusal),
}

/// Why a log offered for adoption was turned away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdoptError {
    /// Only a log from sequence 1 can be verified.
    NotFromTheBeginning,
    Gap,
    MissingClosure(Seq, FnHash),
    /// The authority's own application refused what the peer accepted.
    RefusedAt(Seq, Refusal),
    /// Replaying the intent did not produce the facts the peer recorded.
    FactsDiffer(Seq),
    /// The state after replay does not match what the peer's facts say.
    HashDiffers,
}

impl Authority {
    pub fn new(schema: Schema, bodies: BTreeMap<FnHash, Closure>) -> Authority {
        Authority {
            log: Log::empty(schema.clone()),
            store: MemoryStore::empty(schema.clone()),
            schema,
            bodies,
            natives: BTreeMap::new(),
        }
    }

    /// Hold native procedures (and the closures they carry).
    pub fn hold(&mut self, procs: impl IntoIterator<Item = (FnHash, Procedure)>) {
        hold(&mut self.bodies, &mut self.natives, procs);
    }

    /// §11.7 Sequence an intent: dedupe by id, apply to the head state, and
    /// append with the facts. An intent naming a closure the authority does
    /// not hold is refused, not stalled.
    pub fn sequence_entry(&mut self, e: &Entry) -> Sequenced {
        if let Some(n) = self.log.seq_of(&e.id) {
            return Sequenced::Duplicate(n);
        }
        let out = {
            let mut over = Overlay::new(&self.store);
            run(
                &self.schema,
                &self.bodies,
                &self.natives,
                &e.fn_hash,
                &ctx_of(e),
                &e.autos,
                &e.args,
                &mut over,
            )
        };
        match out {
            None => Sequenced::Rejected(Refusal::Refused(format!("unknown function {}", hex(&e.fn_hash)))),
            Some(Err(bug)) => Sequenced::Rejected(Refusal::Refused(bug_text(&bug))),
            Some(Ok(Err(why))) => Sequenced::Rejected(why),
            Some(Ok(Ok(facts))) => {
                let n = self.log.append(e.clone(), facts.clone());
                self.store.apply_changes(&facts);
                Sequenced::Appended(n, facts)
            }
        }
    }

    /// What a peer at a cursor is sent: a page of entries with their facts,
    /// or the snapshot if it is below the horizon.
    pub fn page(&self, cursor: Seq, limit: usize) -> Page {
        self.log.entries_after(cursor, limit)
    }

    /// Move the horizon; `false` if the sequence is not retained.
    pub fn compact(&mut self, n: Seq) -> bool {
        match self.log.compact_to(n) {
            Some(l) => {
                self.log = l;
                true
            }
            None => false,
        }
    }

    /// Drop every closure that neither the current module nor a retained
    /// entry names.
    pub fn retire(&mut self, current: &BTreeSet<FnHash>) {
        let named = self.log.named_hashes();
        self.bodies.retain(|h, _| current.contains(h) || named.contains(h));
        self.natives.retain(|h, _| current.contains(h) || named.contains(h));
    }

    /// §11.8 Adopt a log a peer sequenced alone: replay every intent from
    /// the beginning through this authority's own closures, holding each to
    /// the facts the peer recorded, and become its authority.
    pub fn adopt(schema: Schema, bodies: BTreeMap<FnHash, Closure>, l: &Log) -> Result<Authority, AdoptError> {
        if l.horizon() != 0 || !l.base.store.is_empty() {
            return Err(AdoptError::NotFromTheBeginning);
        }
        if !l.contiguous() {
            return Err(AdoptError::Gap);
        }
        let mut a = Authority::new(schema, bodies);
        for (n, (e, recorded)) in &l.entries {
            match a.sequence_entry(e) {
                Sequenced::Appended(n2, facts) => {
                    if n2 != *n {
                        return Err(AdoptError::Gap);
                    }
                    if facts != *recorded {
                        return Err(AdoptError::FactsDiffer(*n));
                    }
                }
                Sequenced::Rejected(why) => return Err(AdoptError::RefusedAt(*n, why)),
                Sequenced::Duplicate(_) => return Err(AdoptError::Gap),
            }
        }
        let claimed = l.state_at(l.head_seq()).ok_or(AdoptError::HashDiffers)?;
        if state_hash(&a.store) == state_hash(&claimed) {
            Ok(a)
        } else {
            Err(AdoptError::HashDiffers)
        }
    }
}

// ---------------------------------------------------------------------
// Both at once

/// §11.9 A peer that is its own authority: everything pending is sequenced,
/// and every answer is delivered back, in order. After it, nothing is
/// pending and the view is the confirmed store.
pub fn local_commit(a: &mut Authority, r: &mut Replica) {
    for e in r.pending.clone() {
        match a.sequence_entry(&e) {
            Sequenced::Appended(n, facts) => {
                r.receive_facts(n, facts);
                r.ack(&e.id, n);
            }
            Sequenced::Duplicate(n) => r.ack(&e.id, n),
            Sequenced::Rejected(why) => r.reject(&e.id, why),
        }
    }
}
