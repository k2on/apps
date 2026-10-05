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
//! resolved in the authority's favour. An intent of this replica's own is
//! the exception, because it was already run here: what that run did is
//! kept beside it, and confirming it applies that record rather than
//! running it again (`docs/plan-perf.md` R2).
//!
//! An entry is applied by a native procedure when the peer holds one for
//! its hash ([`crate::authoring::Procedure`], the domain's own code run
//! `Native`), otherwise through the closure the hash names
//! ([`crate::eval::apply_closure`]), otherwise by facts. The two are held to
//! each other by the runtime's tests on every procedure; both are this
//! file's "the closure is held".
//!
//! The stores here are [`MemoryStore`]s, as the spec's are. The optimistic
//! view starts as a clone of the confirmed store when the replica is
//! opened, which shares every table (`docs/plan-db.md` D7.3); after that it
//! moves only by changes — its own intents' going on, a rebase's undoing
//! them, what landed, and their going on again — and reports every one of
//! them, so that a view of it is never told to start over by a rebase
//! (R2). A table the view has not written is the confirmed store's own
//! `Arc`, moved with it by the one wrapper every move of the confirmed
//! store goes through (`move_confirmed`); a table it has written is its own
//! copy, made by its first write and kept until the next replay.

use std::collections::{BTreeMap, BTreeSet};

pub use crate::authoring::Procedure;
use crate::eval::{apply_closure, Args, Ctx, EvalError};
use crate::hash::{state_hash, Closure, FnHash};
use crate::log::{Entry, Facts, Log, Page, Seq};
use crate::rules::{self, Who};
use crate::schema::Schema;
use crate::store::{Change, MemoryStore, Overlay, Refusal, Store};
use crate::value::{hex, Id, TableName};

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
    /// The identity of the log `confirmed` is a prefix of (§10, §12.4),
    /// once this peer knows it: what its `Hello` says beside its cursor, so
    /// that a server whose log is another — one that lost this peer's log
    /// and has sequenced since — answers with its own snapshot rather than
    /// with entries on top of a store that is not theirs. `None` until the
    /// server first says, and for a peer alone. Durable, beside the cursor.
    pub log_id: Option<Id>,
    /// Intents authored here that no verdict has answered, in authoring
    /// order. Durable.
    pub pending: Vec<Entry>,
    /// What each pending intent's run did to `view`, by its id: every
    /// transition, in the order it made them (`docs/plan-perf.md` R2).
    /// Applied in `pending`'s order over `confirmed`, they reach `view` —
    /// so they are what confirming an own intent applies to `confirmed`
    /// without running it again, and what a rebase undoes. Never durable:
    /// an intent opened from disk is run once at open, which records it.
    pub recorded: BTreeMap<Id, Facts>,
    /// The optimistic store: `confirmed` with `pending` replayed. Never
    /// durable. A clone of `confirmed` at open and at every replay, sharing
    /// its tables, and moved by changes after that: a table it writes is
    /// copied once and kept, and every other table is `confirmed`'s own
    /// (`docs/plan-db.md` D7.3, [`Replica::written`]).
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
    /// What has happened to `confirmed` since `take_confirmed` last asked:
    /// replaced wholesale — opened, or a snapshot adopted below the horizon
    /// — which no list of changes describes…
    pub replaced: bool,
    /// …or moved by these changes, one list per sequence applied, oldest
    /// first and contiguous. The durable half of the log, as it moves: what
    /// a store written at the old cursor needs to reach the new one.
    pub journal: Vec<(Seq, Facts)>,
    /// The server runs another module than this peer's (§12 `behind`,
    /// `docs/plan-db.md` D1; [`crate::protocol::Client`] sets it from the
    /// hash the server says): facts are applied *projected* to this
    /// replica's schema ([`crate::store::project_row`]) — a column it
    /// lacks dropped, a nullable one the fact lacks `Null` — before they
    /// are compared with a run or applied. Not durable: every connection's
    /// first answer says it again.
    pub behind: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Inbox {
    pub entry: Option<Entry>,
    pub facts: Option<Facts>,
}

/// What a view is told: the changes to the optimistic store since it last
/// asked — every transition it made, a rebase's included — or that it was
/// replaced whole and must re-hydrate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Changes {
    /// Oldest first. After a rebase: the inverse of what it undid, what
    /// landed, and what it re-applied (`docs/plan-perf.md` R2).
    Applied(Vec<Change>),
    /// The store was replaced whole: the replica was opened, or opened
    /// again over a snapshot adopted from the server.
    Rebuilt,
}

/// What whoever keeps the confirmed store durable is told (§11.1): the
/// changes it moved by since they last asked, per sequence, or that it was
/// replaced and must be written whole. Not [`Changes`]: those are the
/// optimistic store's, and under pending intents the two differ — the view
/// has the intents already, and a rebase undoes them and applies them
/// again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Journal {
    Facts(Vec<(Seq, Facts)>),
    Replaced,
}

// The entry's author, as a replay runs it: an entry carries no roles, and
// nothing a body does reads them.
fn ctx_of(e: &Entry) -> Ctx {
    Ctx::new(e.actor.clone(), e.session.clone())
}

fn bug_text(e: &EvalError) -> String {
    format!("bug: {e:?}")
}

type Applied = Result<Result<Vec<Change>, Refusal>, EvalError>;

// Every run of an entry's function a replica or an authority makes, per
// thread, as `store::clones` counts copies: tests only. What R2 of
// `docs/plan-perf.md` is held to — one run per intent alone, one per own
// intent against a server — is this count. The guards a debug build keeps
// (`runs_as`) run uncounted, so that the count is what a release
// build does.
#[cfg(test)]
thread_local! {
    static RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times this thread has run an entry's function: tests only.
#[cfg(test)]
pub(crate) fn runs() -> usize {
    RUNS.with(|n| n.get())
}

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
    #[cfg(test)]
    RUNS.with(|n| n.set(n.get() + 1));
    run_uncounted(schema, bodies, natives, fh, ctx, autos, args, store)
}

#[allow(clippy::too_many_arguments)]
/// [`run`], for a debug build's guard: not counted.
fn run_uncounted(
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

/// Whether running an entry over `store` produces exactly `facts` — the
/// guard a debug build keeps wherever a record stands in for a run (R2):
/// the run it saves, made anyway and compared. Over an overlay, so the
/// store is untouched.
fn runs_as(
    schema: &Schema,
    bodies: &BTreeMap<FnHash, Closure>,
    natives: &BTreeMap<FnHash, Procedure>,
    e: &Entry,
    store: &dyn Store,
    facts: &Facts,
) -> bool {
    let mut over = Overlay::new(store);
    matches!(
        run_uncounted(schema, bodies, natives, &e.fn_hash, &ctx_of(e), &e.autos, &e.args, &mut over),
        Some(Ok(Ok(chs))) if chs == *facts
    )
}

/// Whether two stores hold the same rows. Not `==`, which also tells a
/// table never written from one whose rows were all taken out — a
/// difference no read and no hash can see, and one undoing an intent that
/// wrote a table's first row leaves behind. A table the two share (one
/// `Arc`, D7.3) is the same rows and is not read: a view compared with its
/// confirmed store reads only the tables it has written.
fn same_rows(a: &MemoryStore, b: &MemoryStore) -> bool {
    a.schema() == b.schema() && a.schema().tables().all(|t| a.shares(b, &t.name) || a.scan(&t.name) == b.scan(&t.name))
}

/// A change undone: the transition back, exact because a change a write
/// reports carries the whole row on each side of it — an `Add` is undone by
/// deleting its key, a `Remove` by putting the row back, an `Edit` by
/// putting the old row (`docs/plan-perf.md` R2). Applied raw, through
/// [`Store::apply_change`], as any fact is.
fn invert(c: Change) -> Change {
    match c {
        Change::Add(t, row) => Change::Remove(t, row),
        Change::Remove(t, row) => Change::Add(t, row),
        Change::Edit(t, old, new) => Change::Edit(t, new, old),
    }
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
            log_id: None,
            pending,
            recorded: BTreeMap::new(),
            inbox: BTreeMap::new(),
            rejections: vec![],
            diverged: vec![],
            rebuilt: true,
            changes: vec![],
            replaced: true,
            journal: vec![],
            behind: false,
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
            let out = run(&self.schema, &self.bodies, &self.natives, fh, ctx, autos, args, &mut over);
            // `docs/plan-auth.md` The device holds its own write to the
            // tables' `writable` rules for its own login's roles, as the
            // authority will for the connection's: a forbidden intent is
            // refused here and never pushed. Nothing is read where no rule
            // is declared.
            match out {
                Some(Ok(Ok(chs))) => {
                    let who = Who {
                        user: &ctx.user,
                        roles: &ctx.roles,
                    };
                    match rules::forbidden(&self.schema, &self.view, &over, &chs, who) {
                        Some(t) => Some(Ok(Err(Refusal::Forbidden(t)))),
                        None => Some(Ok(Ok(chs))),
                    }
                }
                other => other,
            }
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
                // A record for every pending intent and no other: one left
                // behind means `pending` was cut short from outside (a
                // benchmark's author does), and it is let go here rather
                // than kept for ever.
                if self.recorded.len() > self.pending.len() {
                    let live: BTreeSet<Id> = self.pending.iter().map(|p| p.id).collect();
                    self.recorded.retain(|id, _| live.contains(id));
                }
                self.recorded.insert(e.id, chs.clone());
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
    /// inbox until everything before it has been applied — by the next
    /// [`Replica::settle`], never by this call.
    ///
    /// Placing only (`docs/plan-perf.md` R8): with K intents pending, every
    /// advance that lands something under them re-runs all K, so a client
    /// that advanced per frame paid K runs for each live push. A pump
    /// hands every frame it received to the inbox and settles once, and
    /// the K runs are paid once per pump.
    pub fn receive(&mut self, n: Seq, e: Entry) {
        if n <= self.cursor {
            return;
        }
        self.inbox.entry(n).or_default().entry = Some(e);
    }

    /// An entry and its facts arrive together, as a batch delivers them.
    /// Placed in the inbox; applied by the next [`Replica::settle`] (R8).
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
    }

    /// A page of the log handed in whole, as its own pump: every entry to
    /// the inbox, then one [`Replica::settle`] — so a replica with an
    /// intent of its own pending rebases once per page rather than once
    /// per entry. [`crate::protocol::Client`] does not use it: a page is
    /// one of the frames of a pump there, and the pump settles (R8).
    pub fn receive_batch(&mut self, items: impl IntoIterator<Item = (Seq, Entry, Option<Facts>)>) {
        for (n, e, f) in items {
            match f {
                Some(f) => self.receive_with(n, e, f),
                None => self.receive(n, e),
            }
        }
        self.settle();
    }

    /// The facts of an entry arrive, at its sequence. Placed in the inbox;
    /// applied by the next [`Replica::settle`] (R8).
    pub fn receive_facts(&mut self, n: Seq, f: Facts) {
        if n <= self.cursor {
            return;
        }
        self.inbox.entry(n).or_default().facts = Some(f);
    }

    /// §11.4 An acknowledgement: this peer's own intent was sequenced at
    /// `n`. It goes to the inbox as if it had arrived, and is applied at its
    /// turn through the confirmed store, which is the rebase — by the next
    /// [`Replica::settle`], as any arrival is (R8).
    ///
    /// An acknowledgement at or below the cursor is of an intent the
    /// confirmed store already holds: the peer re-opened from a snapshot
    /// that covers `n` — below the horizon, or past the head — and pushed
    /// the intent again, and the authority answered the duplicate with the
    /// sequence it already had. Placed in the inbox it would be dropped
    /// there, and the intent would stay pending for ever, pushed on every
    /// reconnect and applied twice in the view (`arkc fuzz`,
    /// `rebase/fleet-fuzz-an-ack-at-or-below-the-cursor.json`). It is
    /// confirmed, not refused: dropped from pending with no verdict, and
    /// the view undoes it and every intent after it and runs those again,
    /// as [`Replica::reject`] does.
    pub fn ack(&mut self, id: &Id, n: Seq) {
        let Some(at) = self.pending.iter().position(|e| e.id == *id) else {
            return;
        };
        if n <= self.cursor {
            let undo: Vec<Id> = self.pending[at..].iter().map(|e| e.id).collect();
            self.pending.remove(at);
            self.rebase(&undo, vec![], at, &[]);
            return;
        }
        let e = self.pending[at].clone();
        self.receive(n, e);
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
    ///
    /// A rebase of the kind that lands nothing (R2): from the first
    /// intent rewritten, the view undoes what each did and runs them again
    /// as their new author, and a view of it is told every transition.
    pub fn sign_in(&mut self, who: &Ctx) {
        let nobody = |e: &Entry| e.actor.is_empty() && e.session.is_empty();
        let Some(from) = self.pending.iter().position(nobody) else {
            return;
        };
        let undo: Vec<Id> = self.pending[from..].iter().map(|e| e.id).collect();
        for e in &mut self.pending[from..] {
            if nobody(e) {
                e.actor = who.user.clone();
                e.session = who.session.clone();
            }
        }
        self.rebase(&undo, vec![], from, &[]);
    }

    /// §11.5 A verdict against this peer's own intent: it is dropped, the
    /// verdict is kept for the app to show, and the view undoes it and
    /// every intent after it and runs those again (R2) — the ones before it
    /// never saw it.
    ///
    /// Only for an intent still pending (`docs/plan-perf.md` Round 4). One
    /// that is not was answered already — dropped by a rebase whose replay
    /// refused it, which kept that reason, or confirmed — and the server's
    /// `Reject` that follows a replay's refusal, as it does whenever the
    /// peer pushed the intent before the rebase that dropped it, is the
    /// same verdict arriving a second time. Reporting it again told the app
    /// one intent was refused twice, with two reasons.
    pub fn reject(&mut self, id: &Id, why: Refusal) {
        let Some(at) = self.pending.iter().position(|e| e.id == *id) else {
            return;
        };
        self.rejections.push((*id, why));
        let undo: Vec<Id> = self.pending[at..].iter().map(|e| e.id).collect();
        self.pending.remove(at);
        self.rebase(&undo, vec![], at, &[]);
    }

    /// Alone → server (`docs/plan-alone.md` §1): the local history since
    /// the fork is taken back out of the confirmed store and becomes
    /// pending again. `local` is what each entry this peer sequenced alone
    /// above the fork changed, in sequence order — the confirmed store is
    /// at `fork + local.len()` — and `requeue` is what those intents are
    /// pushed as: the same ids, functions, autos and arguments, under the
    /// login that is taking them to a server. The cursor and the log move to
    /// the fork's, the requeued intents go ahead of anything already
    /// pending (an id already pending is not queued twice), and the view is
    /// told every transition: what pending held undone, the local history
    /// undone newest first — each change inverts exactly, so the confirmed
    /// store is the fork's state again without reading it — and every
    /// intent run again on top. A rebase of the kind that lands nothing,
    /// so a screen of the library is patched and not rebuilt; an intent the
    /// replay now refuses is dropped with its reason, as any rebase does.
    ///
    /// The confirmed store was moved backwards, which no page of a journal
    /// can say: whoever keeps it durable is told [`Journal::Replaced`].
    pub fn fork_back(&mut self, local: Vec<Facts>, fork: Seq, log_id: Option<Id>, requeue: Vec<Entry>) {
        debug_assert_eq!(
            self.cursor,
            fork + local.len() as Seq,
            "the confirmed store is the fork with the local history on it"
        );
        let order: Vec<Id> = self.pending.iter().map(|e| e.id).collect();
        let whole = order.iter().all(|id| self.recorded.contains_key(id));
        let mut told = Vec::new();
        if whole {
            for id in order.iter().rev() {
                let rec = self.recorded.remove(id).unwrap_or_default();
                for c in rec.into_iter().rev() {
                    let back = invert(c);
                    self.view.apply_change(&back);
                    told.push(back);
                }
            }
        }
        // The confirmed store moves backwards, through the one wrapper: a
        // table the view shares is the confirmed store's again after, and
        // takes nothing twice; one the view holds a copy of takes the same
        // step back (D7.3).
        self.move_confirmed(|r, shared| {
            for facts in local.into_iter().rev() {
                for c in facts.into_iter().rev() {
                    let back = invert(c);
                    r.confirmed.apply_change(&back);
                    if whole && !shared.iter().any(|t| t == back.table()) {
                        r.view.apply_change(&back);
                    }
                    told.push(back);
                }
            }
        });
        self.cursor = fork;
        self.log_id = log_id;
        self.inbox.clear();
        self.journal.clear();
        self.replaced = true;
        let queued: BTreeSet<Id> = requeue.iter().map(|e| e.id).collect();
        let rest: Vec<Entry> = std::mem::take(&mut self.pending)
            .into_iter()
            .filter(|e| !queued.contains(&e.id))
            .collect();
        self.pending = requeue;
        self.pending.extend(rest);
        if !whole {
            // A record missing: `pending` was changed from outside, and the
            // confirmed store is right whatever happened (`rebase`).
            return self.replay();
        }
        debug_assert!(
            same_rows(&self.view, &self.confirmed),
            "undoing pending and the local history did not reach the fork's state"
        );
        told.extend(self.run_pending(0));
        self.changes.extend(told);
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

    /// §11.6 Apply what the inbox holds, in order, for as long as the next
    /// entry can be applied — once, after everything a pump received was
    /// placed (`docs/plan-perf.md` R8), and after closures or facts arrive
    /// that may unblock what waits. Whatever landed under this peer's
    /// pending intents is one rebase, however many frames brought it: the
    /// K pending intents are re-run once, not once per frame.
    ///
    /// What `retry` was, under the name it has now that it is the only way
    /// the inbox moves. Settling with nothing new placed does nothing.
    pub fn settle(&mut self) {
        self.advance();
        debug_assert!(
            self.shares_exactly_unwritten(),
            "the view shares a table it has written, or holds its own copy of one it has not, at sequence {} (docs/plan-db.md D7.3)",
            self.cursor
        );
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

    /// What the confirmed store has done since the last ask, and the slate
    /// wiped: [`Journal::Replaced`] once it was replaced, whatever it moved
    /// by afterwards, since writing it whole covers that too.
    pub fn take_confirmed(&mut self) -> Journal {
        let facts = std::mem::take(&mut self.journal);
        if std::mem::take(&mut self.replaced) {
            Journal::Replaced
        } else {
            Journal::Facts(facts)
        }
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
    // anything else landed under pending intents, and the view is rebased
    // by changes (R2).
    fn advance(&mut self) {
        // Nothing next in the inbox, nothing to do: a settle with nothing
        // placed, which every pump that heard nothing makes (R8), costs a
        // lookup and not a list of the pending ids.
        if !self.inbox.contains_key(&(self.cursor + 1)) {
            return;
        }
        // The intents the view holds above `confirmed`, in the order it
        // applied them: what a rebase undoes, whichever of them this
        // advance confirms first.
        let order: Vec<Id> = self.pending.iter().map(|e| e.id).collect();
        // Every entry applied moves the confirmed store, so the loop runs
        // inside the wrapper that keeps the view's sharing (D7.3); the
        // view's own move follows it.
        let ((acc, moved, others), shared) = self.move_confirmed(|r, _| r.confirm_inbox());
        if !moved {
            return;
        }
        if order.is_empty() {
            // Nothing was pending, so the view was the confirmed store: it
            // let go of every table and took them back moved, and a view of
            // it is told the same changes. (A table a record still names —
            // `pending` cut short from outside — kept its own copy, which
            // moves by them as it always did.)
            self.land(&acc, &shared);
            self.changes.extend(acc);
        } else if !others {
            // Every entry applied was this peer's own next intent, in order,
            // applied over the state the view applied it over: the view is
            // `confirmed` with `pending` replayed, so the confirmed store
            // before the first of them is the view's base, and before each
            // next one it is the view after the one before. The confirmed
            // store has just reached, by the same changes — the records, or
            // facts equal to them — exactly the states the view passed
            // through, and the view owes nothing. Copying `confirmed` over
            // it here was a deep copy of every table and index per mutation
            // for a peer alone, which confirms each intent at once (§11.9).
            //
            // What makes the equality hold, case by case: an own intent is
            // applied by its record, which is its run over this same state,
            // or by its closure where there is no record — it was authored
            // here, so its function is held (bodies are only ever added);
            // a disagreement with the authority's facts sets `diverged`, and
            // an entry that is not the next one pending (another peer's, or
            // one of ours out of order or rewritten) sets `others`, and
            // both rebase; a refusal is `reject`, which rebases; a
            // `sign_in` rewrites pending and rebases, so the view is of the
            // rewritten entries, which are what is pushed and confirmed.
            // Held, not assumed: a divergence between the optimistic path
            // and the confirmed path is a bug every debug run names.
            for id in &order[..order.len() - self.pending.len()] {
                self.recorded.remove(id);
            }
            if self.pending.is_empty() {
                debug_assert!(
                    self.view == self.confirmed,
                    "the view is not the confirmed store at sequence {}, after only this peer's own intents were confirmed",
                    self.cursor
                );
            }
        } else {
            self.rebase(&order, acc, 0, &shared);
        }
    }

    // The advance's first half: apply from the inbox in order for as long
    // as the next entry can be applied, moving the confirmed store and
    // nothing else. What it moved by, whether it moved, and whether
    // anything other than this peer's own next intents, in order and
    // agreeing with their records, landed.
    fn confirm_inbox(&mut self) -> (Vec<Change>, bool, bool) {
        let mut acc: Vec<Change> = Vec::new();
        let mut moved = false;
        let mut others = false;
        loop {
            let n = self.cursor + 1;
            let Some(ib) = self.inbox.get(&n) else { break };
            let Some(e) = ib.entry.clone() else { break };
            let mf = ib.facts.clone();
            // This peer's own next intent, as it was authored — the entry
            // itself and not only its id, so that the view, which applied
            // exactly that entry, can be trusted to have reached what the
            // confirmed store reaches by applying it (see below).
            let own_next = self.pending.first() == Some(&e);
            // §R2 Confirmed by its record. Nothing but this peer's own
            // intents, in order, has landed since the view was last
            // `confirmed` with `pending` over it — at the start of every
            // advance it is, and `others` says whether anything else has
            // landed since — so the confirmed store is exactly the state
            // this intent was run over when it was authored, and `apply`
            // is a function of the state and the entry (§11.2): the record
            // is what running it again would produce. Facts, when they
            // came, are compared to the record, and a difference is a
            // divergence exactly as a run's would be.
            let by_record = match self.recorded.get(&e.id) {
                Some(rec) if own_next && !others => {
                    debug_assert!(
                        runs_as(&self.schema, &self.bodies, &self.natives, &e, &self.confirmed, rec),
                        "the record of this peer's own intent is not what running it over the confirmed store at sequence {} produces",
                        self.cursor
                    );
                    Some(match mf.as_ref().map(|f| self.here(f)) {
                        Some(f) if f != *rec => (f, true),
                        _ => (rec.clone(), false),
                    })
                }
                _ => None,
            };
            let Some((chs, diverged)) = by_record.or_else(|| self.apply_one(n, &e, mf.as_ref())) else {
                // A confirmed entry this replica holds the closure for and
                // whose replay refuses, with no facts in hand: the
                // authority applied it, so the replay disagrees with it —
                // a divergence, recorded so that `needs` asks for the
                // facts. Waiting for them unasked was waiting for ever: a
                // replica fed by replay is sent none (`arkc fuzz`,
                // `rebase/fleet-fuzz-an-ack-names-no-log-and-the-replay-refuses.json`).
                if mf.is_none() && self.can_apply(&e.fn_hash) && !self.diverged.contains(&n) {
                    self.diverged.push(n);
                }
                break;
            };
            self.confirmed.apply_changes(&chs);
            self.journal.push((n, chs.clone()));
            self.cursor = n;
            self.inbox.remove(&n);
            if own_next {
                self.pending.remove(0);
            } else {
                self.pending.retain(|p| p.id != e.id);
            }
            if diverged {
                self.diverged.push(n);
            }
            acc.extend(chs);
            moved = true;
            others = others || !own_next || diverged;
        }
        (acc, moved, others)
    }

    // One entry against the confirmed store: by intent when the closure is
    // held, by facts otherwise; both when both are present, comparing them.
    // The changes the confirmed store moves by, and whether the two
    // disagreed; `None` means it cannot be applied yet. The intent runs
    // over an overlay, so the store is untouched until the caller commits.
    fn apply_one(&self, n: Seq, e: &Entry, mf: Option<&Facts>) -> Option<(Vec<Change>, bool)> {
        let projected = mf.map(|f| self.here(f));
        let mf = projected.as_ref();
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

    // The authority's facts as this replica holds them (`docs/plan-db.md`
    // D1). `behind`, projected to this replica's schema: a change to a
    // table it lacks dropped, each row laid out as its table's with what
    // the table lacks dropped and what the row lacks `Null`. Otherwise
    // only widened: a row that names nothing this schema lacks but leaves
    // out a nullable column — a fact written under an older module than
    // this one, which the server may still serve — has it `Null`, as this
    // peer's own run of the same entry would; any other row as it came.
    // A fact whose row lacks a column this schema requires cannot be
    // projected (`MalformedRow`) and is applied as it came, which §4.5
    // already does with a fact; the advance compares it with the run, and
    // a difference is a divergence, never silent.
    fn here(&self, f: &Facts) -> Facts {
        let short = |c: &Change| {
            self.schema.lookup_table(c.table()).is_some_and(|tbl| {
                let rows: Vec<&crate::store::Row> = match c {
                    Change::Add(_, r) | Change::Remove(_, r) => vec![r],
                    Change::Edit(_, o, r) => vec![o, r],
                };
                rows.iter()
                    .any(|r| r.len() < tbl.columns.len() && r.keys().all(|k| tbl.column(k).is_some()))
            })
        };
        if !self.behind && !f.iter().any(short) {
            return f.clone();
        }
        let mut out = Vec::with_capacity(f.len());
        for c in f {
            if !self.behind && !short(c) {
                out.push(c.clone());
                continue;
            }
            match crate::store::project(&self.schema, c) {
                Ok(Some(c)) => out.push(c),
                Ok(None) => {}
                Err(_) => out.push(c.clone()),
            }
        }
        out
    }

    // Rebuild the view whole: a clone of the confirmed store — every table
    // shared, nothing written (D7.3) — then every pending intent in order,
    // recorded. What opening does — the one
    // wholesale replacement a replica makes of its own view — and the
    // fallback of a rebase that finds a record missing.
    fn replay(&mut self) {
        self.view = self.confirmed.clone();
        self.rebuilt = true;
        self.changes.clear();
        self.recorded.clear();
        let _ = self.run_pending(0);
    }

    // §R2 The rebase, by changes. `undo` is the intents whose records the
    // view holds above the state to return to, in the order it applied
    // them; `landed` is what the confirmed store moved by since that state
    // (nothing, for a verdict or a sign-in, which return to a state inside
    // the pending); then `pending[from..]` runs again over the view.
    //
    // 1. The records, undone newest first: each change inverts exactly
    //    (`invert`), so the view is the state before the first of them —
    //    the old confirmed store, when `from` is 0.
    // 2. What landed, applied: the view is the new confirmed store.
    // 3. The surviving intents, run again through an overlay in order,
    //    each one's new changes recorded; one that now refuses is dropped
    //    with its reason, as it always was.
    //
    // A view of it is told the concatenation — every transition the view
    // store made, in the order it made them — so `push_all`, which settles
    // each touched key once against the final store, costs the keys
    // touched, and nothing re-hydrates. No copy of the store is made.
    fn rebase(&mut self, undo: &[Id], landed: Vec<Change>, from: usize, shared: &[TableName]) {
        if undo.iter().any(|id| !self.recorded.contains_key(id)) {
            // Only when `pending` was changed from outside: the confirmed
            // store is right whatever happened, so a replay from it is too.
            return self.replay();
        }
        let mut told = Vec::new();
        for id in undo.iter().rev() {
            let rec = self.recorded.remove(id).unwrap_or_default();
            for c in rec.into_iter().rev() {
                let back = invert(c);
                self.view.apply_change(&back);
                told.push(back);
            }
        }
        self.land(&landed, shared);
        told.extend(landed);
        if from == 0 {
            debug_assert!(
                same_rows(&self.view, &self.confirmed),
                "undoing this peer's pending and applying what landed did not reach the confirmed store at sequence {}",
                self.cursor
            );
        }
        told.extend(self.run_pending(from));
        self.changes.extend(told);
    }

    // D7.3 The one way the confirmed store is moved after open: `f` writes
    // it — given the tables the view has let go of — between the view
    // releasing every table it shares with the confirmed store, which is
    // every table it has not written since the last replay, and taking
    // those tables back as the confirmed store's own `Arc`s. Released,
    // they are held once, so `f`'s writes to them are in place; without
    // the release the confirmed store's write would be the copy,
    // `make_mut` on an `Arc` the view also holds, and a quiet client would
    // copy `media` once per batch. A table the view has written it holds a
    // copy of, which takes what landed as R2 says (`rebase`, `land`).
    // Returns `f`'s answer and the tables taken back, which the view has
    // already moved with the confirmed store and must not move again.
    fn move_confirmed<T>(&mut self, f: impl FnOnce(&mut Replica, &[TableName]) -> T) -> (T, Vec<TableName>) {
        let shared = self.unwritten();
        debug_assert!(
            shared.iter().all(|t| self.view.shares(&self.confirmed, t)),
            "a table the view has not written is not the confirmed store's own (docs/plan-db.md D7.3)"
        );
        self.view.release(shared.iter().map(String::as_str));
        let out = f(self, &shared);
        self.view.retake(&self.confirmed, shared.iter().map(String::as_str));
        (out, shared)
    }

    /// D7.3 The tables the view has written since the last replay (or
    /// open): the design's `diverged`, named apart from
    /// [`Replica::diverged`], which is sequences. Each is the view's own
    /// copy until the next replay — not taken back when the intent that
    /// wrote it is confirmed, because freeing a copy costs what making one
    /// does, and a peer whose pending empties between mutations would pay
    /// both on every mutation (`docs/plan-db.md` D7.3). Kept by the view's
    /// own write path ([`MemoryStore::written`]), so no write to it can
    /// go unrecorded.
    pub fn written(&self) -> &BTreeSet<TableName> {
        self.view.written()
    }

    // D7.3 The tables of the schema the view has not written since the
    // last replay: equal to the confirmed store's, and held as its `Arc`s.
    fn unwritten(&self) -> Vec<TableName> {
        let written = self.view.written();
        self.schema
            .tables()
            .filter(|t| !written.contains(&t.name))
            .map(|t| t.name.clone())
            .collect()
    }

    // What landed on the confirmed store, applied to the view's own copies:
    // a change to a table the view shares with the confirmed store (one of
    // `shared`, taken back by `move_confirmed`) is already there.
    fn land(&mut self, landed: &[Change], shared: &[TableName]) {
        for c in landed {
            if !shared.iter().any(|t| t == c.table()) {
                self.view.apply_change(c);
            }
        }
    }

    /// D7.3 Whether the view holds as the confirmed store's own `Arc`
    /// exactly the tables it has not written ([`Replica::written`]), and
    /// a copy of its own of every one it has — what [`Replica::settle`]
    /// asserts after every pump, beside the equality the replica's tests
    /// assert, so that the sharing and the rows cannot drift apart
    /// silently.
    pub fn shares_exactly_unwritten(&self) -> bool {
        let written = self.view.written();
        self.schema
            .tables()
            .all(|t| self.view.shares(&self.confirmed, &t.name) != written.contains(&t.name))
    }

    // Run `pending[from..]` over the view in order, each through an overlay
    // so a refusal leaves it untouched, recording each one's changes; one
    // that is now refused is dropped and its reason kept. What the view
    // moved by, oldest first.
    fn run_pending(&mut self, from: usize) -> Vec<Change> {
        let rest = self.pending.split_off(from);
        let mut moved = Vec::new();
        for e in rest {
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
                    moved.extend(chs.iter().cloned());
                    self.recorded.insert(e.id, chs);
                    self.pending.push(e);
                }
                Some(Ok(Err(why))) => self.rejections.push((e.id, why)),
                Some(Err(bug)) => self.rejections.push((e.id, Refusal::Refused(bug_text(&bug)))),
            }
        }
        moved
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
    /// Every module this authority has started with, by module hash, and
    /// the function hashes each shipped (`docs/plan-db.md` D1, closure
    /// provenance): what [`Authority::retire`] keeps whatever the log
    /// still names. A peer authors at the hashes of the module it was
    /// built with, and a server that once ran that module has told it
    /// those hashes are good; dropping one because the module moved on
    /// and no retained entry happens to name it would turn an older
    /// client's ordinary intent into an unknown function. A hash from a
    /// module this authority never ran is still unknown — the honest
    /// answer to a peer older than the server's first deploy.
    pub modules: BTreeMap<Vec<u8>, BTreeSet<FnHash>>,
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
            modules: BTreeMap::new(),
        }
    }

    /// Hold native procedures (and the closures they carry).
    pub fn hold(&mut self, procs: impl IntoIterator<Item = (FnHash, Procedure)>) {
        hold(&mut self.bodies, &mut self.natives, procs);
    }

    /// Whether an intent naming this hash can be run here: a procedure or a
    /// closure is held for it.
    pub fn can_apply(&self, fh: &FnHash) -> bool {
        self.natives.contains_key(fh) || self.bodies.contains_key(fh)
    }

    /// Record that this authority has run `module` (its hash), which
    /// shipped `closures`: from here on [`Authority::retire`] keeps every
    /// one of them, and an intent naming one is sequenced through it
    /// whichever module is current (`docs/plan-db.md` D1). A closure
    /// already held under a hash is kept as it is — the left-biased union
    /// a received closure follows (§12.2), and a hash names one function.
    pub fn ran(&mut self, module: Vec<u8>, closures: impl IntoIterator<Item = (FnHash, Closure)>) {
        let shipped = self.modules.entry(module).or_default();
        for (h, c) in closures {
            shipped.insert(h.clone());
            self.bodies.entry(h).or_insert(c);
        }
    }

    /// §11.7 Sequence an intent: dedupe by id, apply to the head state, and
    /// append with the facts. An intent naming a closure the authority does
    /// not hold is refused, not stalled.
    ///
    /// Unjudged by the tables' `writable` rules: what a log being replayed
    /// or adopted, and a peer alone's own authority, sequence — every entry
    /// there was judged when it was first sequenced, or is the peer's own.
    /// A server sequences what a connection pushes with
    /// [`Authority::sequence_as`].
    pub fn sequence_entry(&mut self, e: &Entry) -> Sequenced {
        self.sequence(e, None)
    }

    /// `docs/plan-auth.md` [`Authority::sequence_entry`], with every row the
    /// run writes held to its table's `writable` rule for `who`, the
    /// connection's identity, after the run and before anything is
    /// appended: a row it does not admit is the verdict
    /// [`Refusal::Forbidden`], and nothing of the run is kept. A schema
    /// whose rules `who`'s roles decide true reads no row for it.
    pub fn sequence_as(&mut self, e: &Entry, who: Who) -> Sequenced {
        self.sequence(e, Some(who))
    }

    fn sequence(&mut self, e: &Entry, who: Option<Who>) -> Sequenced {
        if let Some(n) = self.log.seq_of(&e.id) {
            return Sequenced::Duplicate(n);
        }
        let out = {
            let mut over = Overlay::new(&self.store);
            let out = run(
                &self.schema,
                &self.bodies,
                &self.natives,
                &e.fn_hash,
                &ctx_of(e),
                &e.autos,
                &e.args,
                &mut over,
            );
            match (out, who) {
                (Some(Ok(Ok(facts))), Some(who)) => match rules::forbidden(&self.schema, &self.store, &over, &facts, who) {
                    Some(t) => Some(Ok(Err(Refusal::Forbidden(t)))),
                    None => Some(Ok(Ok(facts))),
                },
                (out, _) => out,
            }
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

    /// §R2 Append an intent this process has already run, with the changes
    /// that run produced, without running it again: what a peer alone's
    /// authority does with its replica's record ([`local_commit`]). The
    /// authority and the replica are one process over one store there, so
    /// the second run was the first run again. Deduped by id as
    /// [`Authority::sequence_entry`] is; never a verdict, since the record
    /// is of a run that was accepted. A server judges what it is sent, and
    /// uses `sequence_entry`.
    ///
    /// The caller holds `facts` to be the entry's run over this store at
    /// its head; a debug build runs it anyway and says so if not.
    pub fn append_as(&mut self, e: &Entry, facts: Facts) -> Sequenced {
        if let Some(n) = self.log.seq_of(&e.id) {
            return Sequenced::Duplicate(n);
        }
        debug_assert!(
            runs_as(&self.schema, &self.bodies, &self.natives, e, &self.store, &facts),
            "appended as recorded, but running the intent over the head state produces something else"
        );
        self.store.apply_changes(&facts);
        let n = self.log.append(e.clone(), facts.clone());
        Sequenced::Appended(n, facts)
    }

    /// What a peer at a cursor is sent: a page of entries with their facts,
    /// or the snapshot if it is below the horizon.
    pub fn page(&self, cursor: Seq, limit: usize) -> Page {
        self.log.entries_after(cursor, limit)
    }

    /// Move the horizon; `false` if the sequence is not retained. In
    /// place, as [`Log::compact_to`] would build it: the entries kept are
    /// moved rather than cloned, since a server compacting to its
    /// retention floor keeps ten thousand of them and would otherwise hold
    /// two copies at once (`docs/plan-perf.md` R10, `ark::retention`). At
    /// the head the state is this authority's store and nothing is
    /// replayed.
    pub fn compact(&mut self, n: Seq) -> bool {
        let st = if n == self.log.head_seq() && n >= self.log.horizon() {
            self.store.clone()
        } else {
            match self.log.state_at(n) {
                Some(st) => st,
                None => return false,
            }
        };
        let kept = self.log.entries.split_off(&(n + 1));
        self.log.entries = kept;
        self.log.base = crate::log::snapshot_of(n, st).of_log(self.log.id());
        true
    }

    /// Drop every closure that neither the current module, a retained
    /// entry, nor a module this authority has run ([`Authority::ran`])
    /// names. The last is closure provenance (`docs/plan-db.md` D1): an
    /// old client authors at the hashes its module shipped, and a server
    /// that ran that module answers them for as long as it lives, not only
    /// while its log happens to hold an entry naming one.
    pub fn retire(&mut self, current: &BTreeSet<FnHash>) {
        let named = self.log.named_hashes();
        let ran: BTreeSet<&FnHash> = self.modules.values().flatten().collect();
        let keep = |h: &FnHash| current.contains(h) || named.contains(h) || ran.contains(h);
        self.bodies.retain(|h, _| keep(h));
        self.natives.retain(|h, _| keep(h));
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
///
/// Sequenced by the replica's own record (R2, [`Authority::append_as`])
/// when the authority's head is the replica's cursor and the intent is the
/// first pending: the authority's store is then the replica's confirmed
/// store, which is the state the record was made over. The replica then
/// confirms it by the same record, so an intent alone is run once, when it
/// is authored. Anything else — an authority ahead of the replica, an
/// intent with no record — is sequenced by running it, as a server does.
pub fn local_commit(a: &mut Authority, r: &mut Replica) {
    for e in r.pending.clone() {
        let recorded = match r.recorded.get(&e.id) {
            Some(rec) if a.log.head_seq() == r.cursor && r.pending.first() == Some(&e) => Some(rec.clone()),
            _ => None,
        };
        let out = match recorded {
            Some(rec) => a.append_as(&e, rec),
            None => a.sequence_entry(&e),
        };
        match out {
            Sequenced::Appended(n, facts) => {
                r.receive_facts(n, facts);
                r.ack(&e.id, n);
            }
            Sequenced::Duplicate(n) => r.ack(&e.id, n),
            Sequenced::Rejected(why) => r.reject(&e.id, why),
        }
        // Settled per intent, not once at the end (R8): the next intent is
        // sequenced by its record only when the replica's cursor is the
        // authority's head and it is the first pending, which is true
        // after this one is confirmed and not before. Alone, nothing but
        // this peer's own intents lands, so each settle confirms by the
        // record and re-runs nothing.
        r.settle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::closures;
    use crate::value::Value;

    /// The spec's demo (`spec/AUTHORING.md` Appendix B), authored here in
    /// the vocabulary: a playlist and its items, where adding an item reads
    /// the last one — so an intent's changes depend on the state it meets.
    #[allow(dead_code)]
    mod fixture {
        use crate::authoring::*;

        pub struct Demo {
            pub playlist: Table<Playlist>,
            pub item: Table<Item>,
        }
        impl Tables for Demo {
            fn open() -> Self {
                Demo {
                    playlist: table(),
                    item: table(),
                }
            }
        }

        pub struct Playlist {
            pub id: Id<Playlist>,
            pub name: Text,
            pub user_id: Text,
        }
        impl Row for Playlist {
            const NAME: &str = "playlist";
            type Key = (Id<Playlist>,);
            fn columns() -> Columns<Self> {
                columns()
                    .id(Self::id)
                    .text(Self::name)
                    .text(Self::user_id)
                    .key((Self::id,))
                    .unique((Self::user_id, Self::name))
            }
        }
        #[allow(non_upper_case_globals)]
        impl Playlist {
            pub const id: Col<Self, Id<Self>> = col("id");
            pub const name: Col<Self, Text> = col("name");
            pub const user_id: Col<Self, Text> = col("user_id");
        }

        pub struct Item {
            pub playlist_id: Id<Playlist>,
            pub track_id: Text,
            pub pos: Int,
        }
        impl Row for Item {
            const NAME: &str = "item";
            type Key = (Id<Playlist>, Text);
            fn columns() -> Columns<Self> {
                columns()
                    .id(Self::playlist_id)
                    .refs::<Playlist>()
                    .text(Self::track_id)
                    .int(Self::pos)
                    .key((Self::playlist_id, Self::track_id))
                    .unique((Self::playlist_id, Self::pos))
            }
        }
        #[allow(non_upper_case_globals)]
        impl Item {
            pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
            pub const track_id: Col<Self, Text> = col("track_id");
            pub const pos: Col<Self, Int> = col("pos");
        }

        pub struct CreatePlaylist {
            pub name: Text,
        }
        impl Input for CreatePlaylist {
            fn schema() -> Object<Self> {
                object().field("name", text().trim().min(1))
            }
        }

        pub struct AddToPlaylist {
            pub playlist_id: Id<Playlist>,
            pub track_id: Text,
        }
        impl Input for AddToPlaylist {
            fn schema() -> Object<Self> {
                object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
            }
        }

        pub fn module() -> Module {
            let demo = router::<Demo>("demo");
            Module::new((demo.routes((
                demo.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
                    db.playlist
                        .insert(Playlist {
                            id: ctx.new_id("id"),
                            name: input.name,
                            user_id: ctx.user,
                        })
                        .on((Playlist::user_id, Playlist::name))
                }),
                demo.input::<AddToPlaylist>().mutation("add_to_playlist", |_ctx, db, input| {
                    let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
                    db.item.insert(Item {
                        playlist_id: input.playlist_id,
                        track_id: input.track_id,
                        pos: last.map_or(0, |row| row.pos).add(1),
                    })
                }),
            )),))
        }
    }

    fn idv(k: u32) -> Id {
        let mut b = [0u8; 16];
        b[12..].copy_from_slice(&k.to_be_bytes());
        b
    }

    fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    /// The demo's schema, closures and procedures, and the two hashes.
    struct Demo {
        schema: Schema,
        bodies: BTreeMap<FnHash, Closure>,
        procs: Vec<(FnHash, Procedure)>,
        create: FnHash,
        add: FnHash,
    }

    fn demo() -> Demo {
        let m = fixture::module();
        let procs = m.procedures();
        let hash = |n: &str| procs.iter().find(|(_, p)| p.name() == n).unwrap().0.clone();
        Demo {
            schema: m.build().schema.clone(),
            bodies: closures(m.build()),
            create: hash("create_playlist"),
            add: hash("add_to_playlist"),
            procs,
        }
    }

    impl Demo {
        fn replica(&self, native: bool) -> Replica {
            let mut r = Replica::open(
                self.schema.clone(),
                self.bodies.clone(),
                MemoryStore::empty(self.schema.clone()),
                0,
                vec![],
            );
            if native {
                r.hold(self.procs.clone());
            }
            r
        }

        fn authority(&self) -> Authority {
            let mut a = Authority::new(self.schema.clone(), self.bodies.clone());
            a.hold(self.procs.clone());
            a
        }

        fn create(&self, r: &mut Replica, ctx: &Ctx, n: u32, name: &str) -> Entry {
            r.mutate(
                idv(n),
                ctx,
                &self.create,
                &args([("id", Value::Id(idv(1000 + n)))]),
                &args([("name", Value::text(name))]),
            )
            .unwrap()
        }

        fn add(&self, r: &mut Replica, ctx: &Ctx, n: u32, playlist: u32, track: &str) -> Entry {
            let a = args([("playlist_id", Value::Id(idv(1000 + playlist))), ("track_id", Value::text(track))]);
            r.mutate(idv(n), ctx, &self.add, &Args::new(), &a).unwrap()
        }
    }

    /// The view as a rebase would compute it from scratch: `confirmed`, then
    /// every pending intent.
    fn replayed(r: &Replica) -> MemoryStore {
        let mut fresh = Replica::open(r.schema.clone(), r.bodies.clone(), r.confirmed.clone(), r.cursor, r.pending.clone());
        fresh.natives = r.natives.clone();
        fresh.view
    }

    /// Follow what a view is told, as a view does (R2): every change must
    /// be a transition from the row the store held — an `Add` onto an
    /// absent key, a `Remove` or an `Edit` from exactly the row there — and
    /// applied in order to a store that was the view when it last asked,
    /// they must reach the view. `Rebuilt` is only for a store replaced
    /// whole, which none of the callers do.
    fn follow(shadow: &mut MemoryStore, r: &mut Replica, what: &str) {
        let Changes::Applied(chs) = r.take_changes() else {
            panic!("{what}: told `Rebuilt`")
        };
        for c in &chs {
            let tbl = shadow.schema().lookup_table(c.table()).unwrap().clone();
            let (row, before) = match c {
                Change::Add(_, row) => (row, None),
                Change::Remove(_, row) => (row, Some(row)),
                Change::Edit(_, old, new) => (new, Some(old)),
            };
            assert_eq!(
                shadow.get(c.table(), &tbl.key_of(row)).as_ref(),
                before,
                "{what}: {c:?} is not a transition from the store it was told over"
            );
            shadow.apply_change(c);
        }
        assert!(same_rows(shadow, &r.view), "{what}: what the view was told reaches the view");
    }

    /// A replica whose view has been read, as a screen's has: nothing it
    /// was told is outstanding. Its store, as the screen now has it.
    fn drawn(r: &mut Replica) -> MemoryStore {
        let _ = r.take_changes();
        r.view.clone()
    }

    /// §11.9 A peer alone confirms every intent the moment it is authored,
    /// and that costs the intent's changes: not one copy of the store, with
    /// the procedures held natively or run through their closures. Counted
    /// by `store::clones`, a per-thread tally `MemoryStore::clone` keeps
    /// under `cfg(test)`. Falsified by putting back `self.view =
    /// self.confirmed.clone()` in `advance`: sixty copies, one per intent.
    #[test]
    fn a_peer_alone_copies_no_store_per_mutation() {
        let d = demo();
        for native in [true, false] {
            let (mut a, mut r) = (d.authority(), d.replica(native));
            let me = Ctx::new("me", "local");
            d.create(&mut r, &me, 1, "Mine");
            local_commit(&mut a, &mut r);
            let before = crate::store::clones();
            for i in 0..60 {
                d.add(&mut r, &me, 10 + i, 1, &format!("t{i}"));
                local_commit(&mut a, &mut r);
                assert!(r.pending.is_empty());
                assert_eq!(r.cursor, 2 + i as Seq);
            }
            assert_eq!(crate::store::clones() - before, 0, "copies of the store, native: {native}");
            assert_eq!(r.view, r.confirmed);
            assert_eq!(r.confirmed, a.store);
            assert_eq!(r.view.scan("item").len(), 60);
        }
    }

    /// With a server: intents confirmed a few at a time, with and without
    /// more pending behind them, another peer's entry landing between two
    /// of them, and more authored while some are pending. At every step the
    /// view is what a replay from scratch computes, and when nothing is
    /// pending it is the confirmed store. And at every step what a view was
    /// told since the last is transitions that reach it (R2): a rebase is
    /// `Applied`, never `Rebuilt`, and copies no store. Falsified by
    /// skipping the rebase when another peer's entry lands (`others` never
    /// set): the view keeps this peer's own positions and disagrees with
    /// the replay; and by reporting only what landed and what was re-run,
    /// not the inverses: "theirs landed under ours" is told an `Add` of
    /// "c" onto the key where "c" already is.
    #[test]
    fn the_view_is_confirmed_then_pending_at_every_step() {
        let d = demo();
        let mut a = d.authority();
        let (me, them) = (Ctx::new("me", "s"), Ctx::new("them", "t"));
        let (mut r, mut other) = (d.replica(true), d.replica(false));
        let mut shadow = drawn(&mut r);
        let confirm = |a: &mut Authority, r: &mut Replica, e: &Entry| match a.sequence_entry(e) {
            Sequenced::Appended(n, f) => {
                r.receive_facts(n, f);
                r.ack(&e.id, n);
                r.settle();
                n
            }
            other => panic!("{other:?}"),
        };
        let mut check = |r: &mut Replica, what: &str| {
            assert!(same_rows(&r.view, &replayed(r)), "{what}: the view is confirmed then pending");
            if r.pending.is_empty() {
                assert!(same_rows(&r.view, &r.confirmed), "{what}: nothing pending");
            }
            assert!(r.diverged.is_empty(), "{what}");
            assert_eq!(
                r.recorded.keys().collect::<BTreeSet<_>>(),
                r.pending.iter().map(|e| &e.id).collect(),
                "{what}: a record per pending intent"
            );
            follow(&mut shadow, r, what);
        };
        let e1 = d.create(&mut r, &me, 1, "Mine");
        let e2 = d.add(&mut r, &me, 2, 1, "a");
        let e3 = d.add(&mut r, &me, 3, 1, "b");
        check(&mut r, "three authored");
        confirm(&mut a, &mut r, &e1);
        check(&mut r, "the first confirmed, two behind it");
        confirm(&mut a, &mut r, &e2);
        confirm(&mut a, &mut r, &e3);
        check(&mut r, "all three confirmed, in order");
        // Somebody else adds to the same playlist while one of ours waits.
        let e4 = d.add(&mut r, &me, 4, 1, "c");
        other.receive(1, e1.clone());
        other.settle();
        let x = d.add(&mut other, &them, 5, 1, "x");
        let nx = confirm(&mut a, &mut other, &x);
        let mine = crate::store::clones();
        r.receive(nx, x.clone());
        r.settle();
        assert_eq!(crate::store::clones(), mine, "a rebase copies no store");
        check(&mut r, "theirs landed under ours");
        assert_eq!(
            r.view.get("item", &[Value::Id(idv(1001)), Value::text("c")]).unwrap()["pos"],
            Value::Int(4)
        );
        let e6 = d.add(&mut r, &me, 6, 1, "d");
        check(&mut r, "authored with one pending");
        confirm(&mut a, &mut r, &e4);
        check(&mut r, "the older one confirmed");
        // One page with ours after theirs: the batch a reconnect delivers.
        let y = d.add(&mut other, &them, 7, 1, "y");
        let (ny, fy) = match a.sequence_entry(&y) {
            Sequenced::Appended(n, f) => (n, f),
            o => panic!("{o:?}"),
        };
        let (n6, f6) = match a.sequence_entry(&e6) {
            Sequenced::Appended(n, f) => (n, f),
            o => panic!("{o:?}"),
        };
        let mine = crate::store::clones();
        r.receive_batch([(ny, y, Some(fy)), (n6, e6, Some(f6))]);
        assert_eq!(crate::store::clones(), mine, "a rebase copies no store");
        check(&mut r, "a page of theirs and ours");
        assert_eq!(r.confirmed, a.store);
    }

    /// §R2 Alone, an intent is run once — when it is authored — and never
    /// again: the authority appends the replica's record, and the replica
    /// confirms by it. Counted by `runs`, which a debug build's guards do
    /// not add to. Falsified by sequencing with `sequence_entry` in
    /// `local_commit` (two runs per intent), and by confirming by
    /// `apply_one` whatever is recorded (two again).
    #[test]
    fn a_peer_alone_runs_each_intent_once() {
        let d = demo();
        for native in [true, false] {
            let (mut a, mut r) = (d.authority(), d.replica(native));
            let me = Ctx::new("me", "local");
            let before = runs();
            d.create(&mut r, &me, 1, "Mine");
            local_commit(&mut a, &mut r);
            for i in 0..40 {
                d.add(&mut r, &me, 10 + i, 1, &format!("t{i}"));
                local_commit(&mut a, &mut r);
            }
            assert_eq!(runs() - before, 41, "runs for 41 intents, native: {native}");
            assert!(r.pending.is_empty() && r.recorded.is_empty());
            assert_eq!(r.confirmed, a.store);
            assert_eq!(r.view, r.confirmed);
            assert!(r.diverged.is_empty());
            // A log sequenced by records is one a server adopts by running it.
            assert!(Authority::adopt(d.schema.clone(), d.bodies.clone(), &a.log).is_ok());
        }
    }

    /// §R2 Against a server, an intent of this peer's own is run once here —
    /// when it is authored — and confirmed by its record, whether the
    /// acknowledgement comes with the authority's facts (compared) or
    /// without (the record is the facts). The authority's own run is the
    /// server's, in another process, and not counted with the replica's.
    /// Falsified by confirming by `apply_one` whatever is recorded: two
    /// runs per intent.
    #[test]
    fn against_a_server_an_own_intent_is_run_once() {
        let d = demo();
        let mut a = d.authority();
        let me = Ctx::new("me", "s");
        let mut r = d.replica(true);
        let mut here = 0;
        let mut author = |r: &mut Replica, f: &dyn Fn(&mut Replica) -> Entry| {
            let before = runs();
            let e = f(r);
            here += runs() - before;
            e
        };
        let e1 = author(&mut r, &|r| d.create(r, &me, 1, "Mine"));
        let es: Vec<Entry> = (0..20).map(|i| author(&mut r, &|r| d.add(r, &me, 10 + i, 1, &format!("t{i}")))).collect();
        assert_eq!(here, 21, "one run per intent authored");
        let mut sequenced = vec![];
        for e in std::iter::once(&e1).chain(&es) {
            match a.sequence_entry(e) {
                Sequenced::Appended(n, f) => sequenced.push((e.id, n, f)),
                o => panic!("{o:?}"),
            }
        }
        let before = runs();
        for (i, (id, n, f)) in sequenced.into_iter().enumerate() {
            if i % 2 == 0 {
                r.receive_facts(n, f);
            }
            r.ack(&id, n);
            r.settle();
        }
        assert_eq!(runs() - before, 0, "confirming ran nothing");
        assert!(r.pending.is_empty() && r.recorded.is_empty() && r.diverged.is_empty());
        assert_eq!(r.confirmed, a.store);
        assert!(same_rows(&r.view, &r.confirmed));
    }

    /// §R2 Facts that differ from the record are a divergence, exactly as
    /// facts that differ from a run were: the sequence is named in
    /// `diverged`, the confirmed store takes the authority's facts, and
    /// the view is rebased onto them — told as transitions — with what is
    /// still pending run again over them. Falsified by taking the record
    /// whatever the facts say: `diverged` is empty and the confirmed store
    /// has position 2, not 99.
    #[test]
    fn facts_that_differ_from_the_record_are_a_divergence() {
        let d = demo();
        let mut a = d.authority();
        let me = Ctx::new("me", "s");
        let mut r = d.replica(true);
        let e1 = d.create(&mut r, &me, 1, "Mine");
        let e2 = d.add(&mut r, &me, 2, 1, "a");
        let e3 = d.add(&mut r, &me, 3, 1, "b");
        let mut shadow = drawn(&mut r);
        let mut facts = vec![];
        for e in [&e1, &e2, &e3] {
            match a.sequence_entry(e) {
                Sequenced::Appended(n, f) => facts.push((e.id, n, f)),
                o => panic!("{o:?}"),
            }
        }
        // The authority says the first item went at 99.
        for c in &mut facts[1].2 {
            if let Change::Add(_, row) = c {
                row.insert("pos".into(), Value::int(99));
            }
        }
        let item = |st: &MemoryStore, t: &str| st.get("item", &[Value::Id(idv(1001)), Value::text(t)]).unwrap()["pos"].clone();
        for (id, n, f) in facts.into_iter().take(2) {
            r.receive_facts(n, f);
            r.ack(&id, n);
            r.settle();
        }
        assert_eq!(r.diverged, vec![2], "the record and the facts disagreed");
        assert_eq!(item(&r.confirmed, "a"), Value::int(99), "the authority's facts win");
        assert_eq!(r.pending, vec![e3.clone()]);
        assert_eq!(item(&r.view, "b"), Value::int(100), "what is still pending ran again over them");
        assert!(same_rows(&r.view, &replayed(&r)));
        follow(&mut shadow, &mut r, "a divergence under a pending intent");
    }

    /// §R2 A pending intent opened from disk has no record, since a record
    /// is never written: opening runs it once, which records it, and it is
    /// then confirmed by that record without running again. Falsified by
    /// not recording in the open's replay: nothing is recorded, and with
    /// that assertion taken out, confirming runs each of the three again.
    #[test]
    fn an_intent_opened_without_a_record_runs_once_then_confirms_by_it() {
        let d = demo();
        let mut a = d.authority();
        let me = Ctx::new("me", "s");
        let mut author = d.replica(true);
        let es = [
            d.create(&mut author, &me, 1, "Mine"),
            d.add(&mut author, &me, 2, 1, "a"),
            d.add(&mut author, &me, 3, 1, "b"),
        ];
        let before = runs();
        let mut r = Replica::open(d.schema.clone(), d.bodies.clone(), MemoryStore::empty(d.schema.clone()), 0, es.to_vec());
        r.hold(d.procs.clone());
        assert_eq!(runs() - before, 3, "opening runs each once");
        assert_eq!(r.recorded.len(), 3, "and records it");
        let before = runs();
        for e in &es {
            match a.sequence_entry(e) {
                Sequenced::Appended(n, _) => r.ack(&e.id, n),
                o => panic!("{o:?}"),
            }
            r.settle();
        }
        let ours = runs() - before - es.len();
        assert_eq!(ours, 0, "confirming ran nothing (the authority ran {} times)", es.len());
        assert!(r.pending.is_empty() && r.diverged.is_empty());
        assert_eq!(r.confirmed, a.store);
    }

    /// §R2 A verdict and a sign-in rebase by changes too: from the intent
    /// they touch, the view undoes and runs again, and a view is told the
    /// transitions — no copy, no `Rebuilt`. An intent the rebase now
    /// refuses (an item on a playlist whose creation was refused) is
    /// dropped with its reason, as it always was — and the server's
    /// verdict on it, arriving after, is not a second one (Round 4).
    /// Falsified by `reject` undoing only the refused intent's own record:
    /// the item on the refused playlist stays in the view and the replay
    /// disagrees; and by `reject` reporting before it asks whether the
    /// intent is pending: three rejections, the item's twice.
    #[test]
    fn a_verdict_and_a_sign_in_rebase_by_changes() {
        let d = demo();
        let me = Ctx::new("me", "s");
        let mut r = d.replica(true);
        let e1 = d.create(&mut r, &me, 1, "Mine");
        let e2 = d.create(&mut r, &me, 2, "Doomed");
        d.add(&mut r, &me, 3, 1, "a");
        let doomed = d.add(&mut r, &me, 4, 2, "x");
        d.add(&mut r, &me, 5, 1, "b");
        let mut shadow = drawn(&mut r);
        let copies = crate::store::clones();
        r.reject(&e2.id, Refusal::Refused("no".into()));
        assert_eq!(crate::store::clones(), copies, "a verdict copies no store");
        assert_eq!(r.pending.iter().map(|e| e.id).collect::<Vec<_>>(), [idv(1), idv(3), idv(5)]);
        assert_eq!(r.rejections.len(), 2, "the verdict, and the intent it left with nothing to apply to");
        assert_eq!(r.rejections[1].0, doomed.id);
        assert!(
            matches!(r.rejections[1].1, Refusal::Refused(_) | Refusal::MissingParent(..)),
            "{:?}",
            r.rejections[1]
        );
        assert!(r.view.get("playlist", &[Value::Id(idv(1002))]).is_none());
        assert!(same_rows(&r.view, &replayed(&r)));
        follow(&mut shadow, &mut r, "a verdict");
        let _ = e1;

        // The server refuses the item too, as it will have: the playlist is
        // not in its log either. That verdict is the one the replay
        // already gave, and is not reported again.
        r.reject(&doomed.id, Refusal::Refused("playlist_id: no such playlist".into()));
        assert_eq!(r.rejections.len(), 2, "one intent, one reason: {:?}", r.rejections);

        // Signed in: the intents authored as nobody become theirs.
        let nobody = Ctx::nobody();
        let mut r = d.replica(true);
        d.create(&mut r, &me, 1, "Mine");
        d.create(&mut r, &nobody, 2, "Theirs");
        d.add(&mut r, &nobody, 3, 2, "a");
        let mut shadow = drawn(&mut r);
        let copies = crate::store::clones();
        r.sign_in(&Ctx::new("you", "t"));
        assert_eq!(crate::store::clones(), copies, "a sign-in copies no store");
        assert_eq!(r.view.get("playlist", &[Value::Id(idv(1002))]).unwrap()["user_id"], Value::text("you"));
        assert_eq!(r.view.get("playlist", &[Value::Id(idv(1001))]).unwrap()["user_id"], Value::text("me"));
        assert!(same_rows(&r.view, &replayed(&r)));
        follow(&mut shadow, &mut r, "a sign-in");
    }

    /// An intent of this peer's that comes back under its own id but not as
    /// it was authored is somebody else's news: the view applied what was
    /// authored, so it is rebased rather than trusted — and a view of it is
    /// told so, as the transitions: "t1" going, "t2" arriving (R2).
    /// Falsified by matching the next pending intent by id alone: the view
    /// keeps "t1", the confirmed store has "t2", and the equality `advance`
    /// asserts fails.
    #[test]
    fn an_own_intent_confirmed_otherwise_is_replayed() {
        let d = demo();
        let me = Ctx::new("me", "s");
        let mut r = d.replica(true);
        let e1 = d.create(&mut r, &me, 1, "Mine");
        r.ack(&e1.id, 1);
        r.settle();
        let _ = r.take_changes();
        let e2 = d.add(&mut r, &me, 2, 1, "t1");
        let mut altered = e2.clone();
        altered.args.insert("track_id".into(), Value::text("t2"));
        r.receive(2, altered);
        r.settle();
        assert!(r.pending.is_empty());
        assert_eq!(r.view, r.confirmed);
        assert!(r.view.get("item", &[Value::Id(idv(1001)), Value::text("t2")]).is_some());
        let Changes::Applied(told) = r.take_changes() else {
            panic!("a rebase is its transitions")
        };
        let track = |c: &Change| match c {
            Change::Add(_, row) | Change::Remove(_, row) | Change::Edit(_, _, row) => row["track_id"].clone(),
        };
        assert!(
            matches!(&told[..], [Change::Add(..), Change::Remove(..), Change::Add(..)]),
            "authored, undone, landed: {told:?}"
        );
        assert_eq!(
            told.iter().map(track).collect::<Vec<_>>(),
            [Value::text("t1"), Value::text("t1"), Value::text("t2")]
        );
    }

    /// The journal is what the confirmed store moved by, per sequence, in
    /// order: replayed over the store it last reported, it reaches the
    /// store now — under pending intents too, where the view's changes are
    /// something else. An open is a replacement. Falsified by journaling
    /// only what lands while nothing is pending: this peer's own confirmed
    /// intent is missing and the replay falls short of the store.
    #[test]
    fn the_journal_is_what_the_confirmed_store_moved_by() {
        let d = demo();
        let mut a = d.authority();
        let (me, them) = (Ctx::new("me", "s"), Ctx::new("them", "t"));
        let (mut r, mut other) = (d.replica(true), d.replica(true));
        assert_eq!(r.take_confirmed(), Journal::Replaced, "an open is a replacement");
        assert_eq!(r.take_confirmed(), Journal::Facts(vec![]));
        let mut durable = r.confirmed.clone();
        let mut seqs = vec![];
        let mut catch_up = |r: &mut Replica, what: &str| {
            let Journal::Facts(fs) = r.take_confirmed() else {
                panic!("{what}: replaced")
            };
            for (n, f) in fs {
                seqs.push(n);
                durable.apply_changes(&f);
            }
            assert_eq!(durable, r.confirmed, "{what}");
            assert_eq!(seqs, (1..=r.cursor).collect::<Vec<_>>(), "{what}: contiguous");
        };
        let sequence = |a: &mut Authority, e: &Entry| match a.sequence_entry(e) {
            Sequenced::Appended(n, f) => (n, f),
            o => panic!("{o:?}"),
        };
        let e1 = d.create(&mut r, &me, 1, "Mine");
        let (n1, f1) = sequence(&mut a, &e1);
        r.receive_facts(n1, f1.clone());
        r.ack(&e1.id, n1);
        r.settle();
        catch_up(&mut r, "our own, nothing behind it");
        other.receive_with(n1, e1, f1);
        other.settle();
        let e2 = d.add(&mut r, &me, 2, 1, "a");
        let x = d.add(&mut other, &them, 3, 1, "x");
        let (nx, fx) = sequence(&mut a, &x);
        r.receive_with(nx, x, fx);
        r.settle();
        let _ = r.take_changes();
        catch_up(&mut r, "theirs, under ours");
        let (n2, f2) = sequence(&mut a, &e2);
        r.receive_with(n2, e2, f2);
        r.settle();
        catch_up(&mut r, "ours, after the rebase");
        assert_eq!(r.confirmed, a.store);
    }

    /// §R8 A pump is one rebase, however many frames it brought. K = 100
    /// intents of this peer's pending while fifty of another peer's land,
    /// each its own `Batch` as a live server's fan-out sends it, all
    /// delivered to the client in one pump: the frames apply nothing, and
    /// the settle runs each landing entry once and the hundred pending
    /// intents once — 150 runs, where a rebase per frame was 50 + 50 × 100.
    /// The view is the replay's, is told as transitions, and is the view a
    /// client settling per frame reaches. Falsified by settling in
    /// `Client::recv` (the advance per frame R8 removed): 5,050 runs.
    #[test]
    fn fifty_pushes_in_one_pump_rerun_the_pending_once() {
        use crate::protocol::{Client, Mode, ServerMsg};
        let d = demo();
        let mut a = d.authority();
        let (me, them) = (Ctx::new("me", "s"), Ctx::new("them", "t"));
        let page = |items: Vec<(Seq, Entry)>| ServerMsg::Batch {
            items: items.into_iter().map(|(n, e)| (n, e, None)).collect(),
            has_more: false,
            log_id: None,
            module: None,
        };
        let mut other = d.replica(true);
        let shared = d.create(&mut other, &them, 1, "Shared");
        let Sequenced::Appended(n1, _) = a.sequence_entry(&shared) else {
            panic!("the playlist")
        };
        let (mut one, mut each) = (
            Client::open(d.replica(true), Mode::Whole, None),
            Client::open(d.replica(true), Mode::Whole, None),
        );
        for c in [&mut one, &mut each] {
            c.recv(page(vec![(n1, shared.clone())]));
            c.settle();
            for i in 0..100 {
                d.add(&mut c.replica, &me, 100 + i, 1, &format!("m{i}"));
            }
        }
        let pushes: Vec<ServerMsg> = (0..50)
            .map(|i| {
                let x = d.add(&mut other, &them, 1000 + i, 1, &format!("x{i}"));
                match a.sequence_entry(&x) {
                    Sequenced::Appended(n, _) => page(vec![(n, x)]),
                    o => panic!("{o:?}"),
                }
            })
            .collect();
        let mut shadow = drawn(&mut one.replica);
        let before = runs();
        for m in pushes.iter().cloned() {
            one.recv(m);
        }
        assert_eq!(runs() - before, 0, "a frame only places what it brings");
        assert_eq!(one.replica.cursor, n1, "and nothing has landed");
        one.settle();
        assert_eq!(runs() - before, 50 + 100, "each landing entry once, the hundred pending once");
        assert_eq!(one.replica.cursor, n1 + 50);
        assert_eq!(one.replica.pending.len(), 100);
        assert!(
            same_rows(&one.replica.view, &replayed(&one.replica)),
            "the view is confirmed then pending"
        );
        follow(&mut shadow, &mut one.replica, "fifty pushes in one pump");
        // The same frames, a pump each: the K re-runs per frame R8 saves,
        // counted, and the same view at the end.
        let before = runs();
        for m in pushes {
            each.recv(m);
            each.settle();
        }
        assert_eq!(runs() - before, 50 + 50 * 100, "a pump per frame is a rebase per frame");
        assert!(same_rows(&each.replica.view, &one.replica.view));
        assert!(same_rows(&each.replica.confirmed, &one.replica.confirmed));
        assert!(same_rows(&one.replica.confirmed, &a.store));
    }

    /// Closure provenance (`docs/plan-db.md` D1): a server that once ran a
    /// module keeps that module's closures through [`Authority::retire`]
    /// when the current module no longer ships them and no retained entry
    /// names them, so an old client's intent at its old hash is sequenced
    /// rather than refused as an unknown function. A closure no module it
    /// ran shipped, and nothing names, still goes.
    ///
    /// Falsified once: without `ran` in `retire`'s rule, the old module's
    /// closures were dropped and the old client's `create_playlist` was
    /// `Rejected("unknown function …")`.
    #[test]
    fn retire_keeps_every_module_it_has_run() {
        let d = demo();
        let me = Ctx::new("me", "s");
        let mut r = d.replica(false);
        let e = d.create(&mut r, &me, 1, "Road");
        let mut a = Authority::new(d.schema.clone(), d.bodies.clone());
        a.ran(b"the old module".to_vec(), d.bodies.clone());
        let stray: FnHash = vec![9; 32];
        a.bodies.insert(stray.clone(), d.bodies[&d.create].clone());
        assert!(a.log.named_hashes().is_empty(), "nothing in the log names a closure");
        // A new module that ships none of them.
        a.retire(&BTreeSet::new());
        assert!(
            a.bodies.contains_key(&d.create) && a.bodies.contains_key(&d.add),
            "a module it ran is kept"
        );
        assert!(!a.bodies.contains_key(&stray), "a closure no module it ran shipped is retired");
        assert!(
            matches!(a.sequence_entry(&e), Sequenced::Appended(1, _)),
            "an old client's intent at its old hash is sequenced"
        );
    }

    /// `docs/plan-db.md` D7.3 Sequence `e` and hand it to `r` with its facts,
    /// as a page brings it; the sequence.
    fn land_on(a: &mut Authority, r: &mut Replica, e: &Entry) -> Seq {
        let Sequenced::Appended(n, f) = a.sequence_entry(e) else {
            panic!("sequenced")
        };
        r.receive_with(n, e.clone(), f);
        n
    }

    /// D7.3 A quiet client — nothing pending — takes a hundred landed
    /// batches, three entries each, and copies no table: the view lets go
    /// of every table it shares before the confirmed store is written, so
    /// the write is in place, and takes them back after. Counted by
    /// `store::copies`, around the settle alone. Falsified by dropping the
    /// release in `move_confirmed`: the first batch's write found its
    /// table shared and copied it (and the view, holding its own `Arc`,
    /// was then told to take back a table whose rows differ).
    #[test]
    fn a_quiet_client_copies_no_table_per_batch() {
        let d = demo();
        let mut a = d.authority();
        let bob = Ctx::new("bob", "b");
        let mut w = d.replica(true);
        let mut r = d.replica(true);
        let e = d.create(&mut w, &bob, 1, "Road");
        land_on(&mut a, &mut r, &e);
        r.settle();
        for b in 0..100u32 {
            for k in 0..3 {
                let e = d.add(&mut w, &bob, 10 + b * 3 + k, 1, &format!("t{b}.{k}"));
                land_on(&mut a, &mut r, &e);
            }
            let before = crate::store::copies();
            r.settle();
            assert_eq!(crate::store::copies() - before, 0, "batch {b}: tables copied");
            assert!(r.written().is_empty() && r.shares_exactly_unwritten());
        }
        assert_eq!(r.cursor, 301);
        assert_eq!(r.view, r.confirmed);
        assert_eq!(r.confirmed, a.store);
    }

    /// D7.3 One pending intent over a large table copies it once — the
    /// view's first write to a table it shares — and the view keeps that
    /// copy: confirming the intent copies nothing, and the next intent
    /// over the table, and its confirming, copy nothing more. Falsified by
    /// re-sharing on confirm as the first draft of D7.3 did (the view
    /// taking every table no record names back as the confirmed store's,
    /// and forgetting it wrote them): the second intent copied the table
    /// again, 1 where 0 was asserted.
    #[test]
    fn a_pending_intent_copies_its_table_once_and_keeps_it() {
        let d = demo();
        let mut a = d.authority();
        let me = Ctx::new("me", "m");
        let mut filled = d.replica(true);
        let e = d.create(&mut filled, &me, 1, "Long");
        land_on(&mut a, &mut filled, &e);
        filled.settle();
        for i in 0..500 {
            let e = d.add(&mut filled, &me, 10 + i, 1, &format!("t{i}"));
            land_on(&mut a, &mut filled, &e);
            filled.settle();
        }
        let (cursor, st) = (filled.cursor, filled.confirmed.clone());
        drop(filled);
        let mut r = Replica::open(d.schema.clone(), d.bodies.clone(), st, cursor, vec![]);
        r.hold(d.procs.clone());
        assert!(r.written().is_empty() && r.shares_exactly_unwritten());
        for (i, want) in [(0u32, 1usize), (1, 0), (2, 0)] {
            let before = crate::store::copies();
            let e = d.add(&mut r, &me, 2000 + i, 1, &format!("new{i}"));
            assert_eq!(crate::store::copies() - before, want, "intent {i}: tables copied by authoring it");
            assert_eq!(r.written().iter().map(String::as_str).collect::<Vec<_>>(), ["item"]);
            land_on(&mut a, &mut r, &e);
            let before = crate::store::copies();
            r.settle();
            assert_eq!(crate::store::copies() - before, 0, "intent {i}: tables copied by confirming it");
            assert!(r.pending.is_empty() && r.shares_exactly_unwritten());
            assert!(!r.view.shares(&r.confirmed, "item"), "the view keeps its copy");
            assert!(r.view.shares(&r.confirmed, "playlist"), "and shares what it never wrote");
            assert_eq!(r.view, r.confirmed);
        }
        assert_eq!(r.confirmed, a.store);
    }

    /// D7.3 A replay — what an open with pending intents does — starts the
    /// view as the confirmed store's tables, shared, and runs ten intents
    /// over one table: the first write copies it and the other nine write
    /// the copy, so one table copied, not ten. Falsified by making every
    /// write copy its table (`Arc::new((**t).clone())` in place of
    /// `make_mut`, counted each time): ten.
    #[test]
    fn a_replay_copies_each_table_it_writes_once() {
        let d = demo();
        let mut a = d.authority();
        let me = Ctx::new("me", "m");
        let mut r = d.replica(true);
        let e = d.create(&mut r, &me, 1, "Road");
        land_on(&mut a, &mut r, &e);
        r.settle();
        for i in 0..10 {
            d.add(&mut r, &me, 10 + i, 1, &format!("t{i}"));
        }
        assert_eq!(r.pending.len(), 10);
        let (st, pending) = (r.confirmed.clone(), r.pending.clone());
        let before = crate::store::copies();
        let opened = Replica::open(d.schema.clone(), d.bodies.clone(), st, r.cursor, pending);
        assert_eq!(crate::store::copies() - before, 1, "tables copied by a replay of ten intents over one");
        assert_eq!(opened.pending.len(), 10);
        assert!(same_rows(&opened.view, &r.view));
        assert!(opened.shares_exactly_unwritten());
    }
}
