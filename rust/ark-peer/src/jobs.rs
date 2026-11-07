//! Handlers: native code acting on the world outside, answering with
//! mutations (effects as rows, `ark::effect`).
//!
//! A **handler** ([`Peer::handle`]) is registered for a job row `R` on the
//! one process that runs it. It keeps the rows of `R` that are not over,
//! as every durable change moves them, and the nearest time one is due
//! ([`Peer::deadline`] wakes the driver for it): a row pending at a time to
//! come — a job scheduled for later — waits for it. Each due row is claimed
//! (`<table>.claim`, by this registration's own name), the oldest first;
//! once the claim is confirmed — the authority's order decided it — the
//! handler runs with the row, whose key is its idempotency key, and a
//! [`Work`] to author what it found under the claim's span; its answer is
//! the `<table>.finish`. Nothing runs during a replay or an open: rows only
//! move what is kept, and a handler runs only on a peer that registered it
//! — a server's work is a device of its own (a worker,
//! `Handle::<Authority>::worker`). A claim from before a restart is
//! another's: its row is due again only when its lease runs out.
//!
//! **Where it runs.** On a peer on a thread of its own ([`crate::Handle`],
//! as a worker is), each run is on a thread of its own, as many at once as
//! the job says (`#[effect(at_once = ..)]`, one by default), and a job is
//! claimed only when there is room for it: its claim and its finish are
//! the peer's, and a download of hours holds up neither the peer's link
//! nor another handler, nor anything the server sequences. On a peer
//! pumped by hand it runs inside the pump ([`Peer::pump`]), as there is no
//! other thread to run it on (ark-peer has no executor).
//!
//! **When it runs again.** A failure is retried after the job's
//! `RETRY_MS`, and after twice as long at each failure in a row, to its
//! `RETRY_MAX_MS` (`#[effect(retry_max_ms = ..)]`); the count is the
//! handler's, so a restart starts it again. A handler may say when the job
//! is due again ([`Work::again`]): a failure retried then, or a job done
//! that recurs — a feed read every hour — pending again then. What each
//! run came to is said under its span (`job`, then `done` or `failed`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ark::effect::{Claim, Effect, Finish};
use ark::log::{EntryId, Span, TraceId};
use ark::peer::Told;
use ark::store::Change;

use crate::app::Outcome;
use crate::link::Sendable;
use crate::{monotonic_ms, now_ms, Peer};

/// One registration of a handler, over its job's rows.
pub(crate) trait Runner: Sendable {
    fn changed(&mut self, cs: &[Told]);
    /// When the next row not claimed here is due.
    fn next(&self) -> Option<i64>;
    fn run(&mut self, p: &mut Peer, now: i64);
}

/// What a handler is given beside its row: the peer, to read and to author
/// under the job's span, and when the job is due again.
pub struct Work<'a> {
    at: At<'a>,
    span: Span,
    again: Option<i64>,
}

enum At<'a> {
    /// The peer it runs inside the pump of.
    Here(&'a mut Peer),
    /// The peer's handle, from the run's own thread: gone once the peer is.
    #[cfg(not(target_arch = "wasm32"))]
    Off(&'a crate::handle::Weak<Peer>),
}

impl Work<'_> {
    /// `f` with the peer, what it authors under the job's span; from a
    /// run's own thread, the peer held meanwhile, and an error once the
    /// peer is gone (its worker stopped).
    pub fn with<T>(&mut self, f: impl FnOnce(&mut Peer) -> T) -> Result<T, String> {
        let span = Some(self.span);
        match &mut self.at {
            At::Here(p) => Ok(p.under(span, f)),
            #[cfg(not(target_arch = "wasm32"))]
            At::Off(w) => match w.upgrade() {
                Some(h) => Ok(h.lock().under(span, f)),
                None => Err("the worker stopped".into()),
            },
        }
    }

    /// An intent authored under the job's span ([`Peer::mutate`]).
    pub fn mutate(&mut self, name: &str, input: impl crate::Arg) -> Result<EntryId, String> {
        self.with(|p| p.mutate(name, input))?
    }

    /// The job due again at `at_ms` whatever this run comes to: done, it
    /// is pending again then; failed, it is retried then.
    pub fn again(&mut self, at_ms: i64) {
        self.again = Some(at_ms);
    }
}

/// What a handler is: given its row, what the job came to.
type F<R> = dyn Fn(&mut Work, &R) -> Result<<R as Effect>::Outcome, String> + Send + Sync;

/// A run's answer, back from its thread: the job's key, what it came to,
/// and when it is due again.
#[cfg(not(target_arch = "wasm32"))]
type Ran<R> = (Vec<u8>, Result<<R as Effect>::Outcome, String>, Option<i64>);

/// What runs on threads of their own keep: the peer's handle, the runs
/// going and their spans, and what those done came to.
#[cfg(not(target_arch = "wasm32"))]
type Off<R> = (crate::handle::Weak<Peer>, BTreeMap<Vec<u8>, Span>, Arc<std::sync::Mutex<Vec<Ran<R>>>>);

struct Handler<R: Effect> {
    by: String,
    f: Arc<F<R>>,
    /// The rows not over, by key: when each is due, the span of the entry
    /// that last changed it, which its claim and its run are under, and
    /// the order they came in, which those due at once are run in.
    open: BTreeMap<Vec<u8>, (R, i64, Option<Span>, u64)>,
    came: u64,
    /// The claims made, until their verdict: each the claim's id, and its span.
    claims: BTreeMap<Vec<u8>, (EntryId, Span)>,
    /// The rows due whose files are not here yet (`crate::files`): not
    /// claimed, nor waited on by the clock, until they arrive.
    waiting: BTreeSet<Vec<u8>>,
    /// How many times in a row each has failed here.
    failed: BTreeMap<Vec<u8>, u32>,
    /// On a peer with a thread of its own, its runs on threads of theirs.
    #[cfg(not(target_arch = "wasm32"))]
    off: Option<Off<R>>,
}

impl<R: Effect> Handler<R> {
    /// Whether it takes another job now: in the pump, every one due; on
    /// threads of their own, as many at once as its job says.
    fn free(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((_, running, _)) = &self.off {
            return running.len() + self.claims.len() < R::AT_ONCE.max(1);
        }
        true
    }

    /// The finish of a run, under its span, and how long after a failure
    /// it is due again: twice as long at each in a row.
    fn finish(&mut self, p: &mut Peer, k: &[u8], span: Span, outcome: Result<R::Outcome, String>, again: Option<i64>) {
        let Some((r, ..)) = self.open.get(k) else { return };
        let failed = self.failed.remove(k).unwrap_or(0);
        let again = match &outcome {
            Ok(_) => again,
            Err(_) => {
                self.failed.insert(k.to_vec(), failed + 1);
                let after = R::RETRY_MS.saturating_mul(1 << failed.min(30)).min(R::RETRY_MAX_MS.max(R::RETRY_MS));
                again.or((after != R::RETRY_MS).then(|| now_ms() + after))
            }
        };
        let finish = Finish::<R> {
            id: r.id(),
            by: self.by.clone(),
            outcome,
            again_ms: again,
        };
        let _ = p.under(Some(span), |p| p.mutate(&format!("{}.finish", R::TABLE), &finish));
    }
}

/// The span a run is said under.
fn said<R: Effect>(peer: &str, span: &Span) -> tracing::Span {
    let (trace, id) = (ark::hash::hex(&span.trace.0), ark::hash::hex(&span.id));
    tracing::info_span!(target: "ark", "job", peer = %peer, table = R::TABLE, trace = %trace, span = %id)
}

/// What a run came to, said.
fn told<T>(outcome: &Result<T, String>) {
    match outcome {
        Ok(_) => tracing::info!(target: "ark", "done"),
        Err(why) => tracing::warn!(target: "ark", why = %why, "failed"),
    }
}

impl<R: Effect> Runner for Handler<R> {
    fn changed(&mut self, cs: &[Told]) {
        for Told { by, change: c } in cs.iter().filter(|t| t.change.table() == R::TABLE) {
            let row = match c {
                Change::Add { row, .. } | Change::Edit { new: row, .. } => R::decode(row).ok(),
                Change::Remove { row, .. } => {
                    R::decode(row).ok().map(|r| self.open.remove(&r.key_bytes()));
                    None
                }
            };
            let Some(r) = row else { continue };
            match r.job().due(R::LEASE_MS) {
                Some(due) => {
                    let came = self.open.get(&r.key_bytes()).map_or(self.came, |o| o.3);
                    self.came += 1;
                    self.open.insert(r.key_bytes(), (r, due, *by, came));
                }
                None => drop(self.open.remove(&r.key_bytes())),
            }
        }
    }

    fn next(&self) -> Option<i64> {
        if !self.free() {
            return None;
        }
        self.open
            .iter()
            .filter(|(k, _)| !self.claims.contains_key(*k) && !self.waiting.contains(*k) && !self.running(k))
            .map(|(_, (_, d, ..))| *d)
            .min()
    }

    fn run(&mut self, p: &mut Peer, now: i64) {
        // What the runs on their own threads came to, finished.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((_, running, ran)) = &mut self.off {
            let done = std::mem::take(&mut *ran.lock().unwrap_or_else(|e| e.into_inner()));
            let done: Vec<_> = done.into_iter().filter_map(|(k, o, a)| Some((running.remove(&k)?, k, o, a))).collect();
            for (span, k, outcome, again) in done {
                self.finish(p, &k, span, outcome, again);
            }
        }
        let mut due: Vec<_> = (self.open.iter())
            .filter(|(k, (_, d, ..))| *d <= now && !self.claims.contains_key(*k) && !self.running(k))
            .collect();
        due.sort_by_key(|(_, (_, d, _, came))| (*d, *came));
        type Due<I> = (Vec<u8>, I, Option<Span>, Vec<String>);
        let due: Vec<Due<R::Id>> = due
            .into_iter()
            .map(|(k, (r, _, by, _))| (k.clone(), r.id(), *by, crate::files::named(r)))
            .collect();
        self.waiting.clear();
        for (k, id, by, files) in due {
            if !self.free() {
                break;
            }
            // A job runs with what its row names: its files fetched first.
            if !p.have_files(&files) {
                self.waiting.insert(k);
                continue;
            }
            // The claim is a child of what made the row due.
            let claim = Claim::<R> { id, by: self.by.clone() };
            if let Ok(e) = p.under(by, |p| p.mutate(&format!("{}.claim", R::TABLE), &claim)) {
                let span = p.authored(&e).map(|c| c.span()).unwrap_or(Span {
                    trace: by.map_or(TraceId(e.0), |b| b.trace),
                    id: e.0,
                    began: by.map_or(now, |b| b.began),
                });
                self.claims.insert(k, (e, span));
            }
        }
        for (k, (e, span)) in std::mem::take(&mut self.claims) {
            match p.outcome(&e) {
                Some(Outcome::Pending) => drop(self.claims.insert(k, (e, span))),
                Some(Outcome::Ok) => {
                    let Some((r, ..)) = self.open.get(&k) else { continue };
                    let r = r.clone();
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some((peer, running, ran)) = &mut self.off {
                        off(self.f.clone(), peer.clone(), ran.clone(), r, span, p.label());
                        running.insert(k, span);
                        continue;
                    }
                    // The run, and all it authors, under the claim.
                    let _in = said::<R>(&p.label(), &span).entered();
                    let mut work = Work {
                        at: At::Here(p),
                        span,
                        again: None,
                    };
                    let outcome = (self.f)(&mut work, &r);
                    told(&outcome);
                    let again = work.again;
                    self.finish(p, &k, span, outcome, again);
                }
                // Another's claim came first.
                _ => {}
            }
        }
    }
}

impl<R: Effect> Handler<R> {
    fn running(&self, _k: &[u8]) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((_, running, _)) = &self.off {
            return running.contains_key(_k);
        }
        false
    }
}

impl Peer {
    /// `f` handles the jobs of `R`: each due row claimed, `f` run with the
    /// row once the claim is confirmed, its answer the row's finish. On a
    /// peer with a thread of its own ([`crate::Handle`]), each run is on a
    /// thread of its own, as many at once as `R` says (one by default); on
    /// one pumped by hand, in the pump.
    pub fn handle<R: Effect>(&mut self, f: impl Fn(&mut Work, &R) -> Result<R::Outcome, String> + Send + Sync + 'static) {
        let by = format!("{}:{}", self.me().session, ark::hash::hex(&crate::draw_origin().0[..6]));
        let mut h = Handler::<R> {
            by,
            f: Arc::new(f),
            open: BTreeMap::new(),
            came: 0,
            claims: BTreeMap::new(),
            waiting: Default::default(),
            failed: BTreeMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            off: self.driven.clone().map(|p| (p, BTreeMap::new(), Default::default())),
        };
        let rows: Vec<Told> = self
            .log
            .store
            .rows(R::TABLE)
            .map(|r| Told {
                by: None,
                change: Change::Add {
                    table: R::TABLE.into(),
                    row: r.to_vec(),
                },
            })
            .collect();
        h.changed(&rows);
        self.runners.push(Box::new(h));
    }

    /// An intent authored only when it would change what this replica
    /// holds — none when its run over it writes nothing — or the refusal
    /// its run gives here. A worker saying again what it read (a file
    /// already a song) is no entry for every peer to be sent. Its run is
    /// undone after: nothing else is told of it.
    pub fn mutate_new(&mut self, name: &str, input: impl crate::Arg) -> Result<Option<EntryId>, String> {
        let (input, f) = (input.bytes(), self.function(name)?.proc.clone());
        let ctx = ark::peer::Ctx {
            id: self.log.next_id(),
            now: now_ms(),
            ..self.log.me.clone()
        };
        if let Some(f) = f {
            let store = &mut self.core.log.store;
            let changes = f(store, &ctx, &input)?;
            store.undo(&changes)?;
            if changes.is_empty() {
                return Ok(None);
            }
        }
        self.mutate(name, input).map(Some)
    }

    /// The handlers' turn: what is due claimed, what is confirmed run.
    pub(crate) fn jobs(&mut self, now: i64) {
        let mut runners = std::mem::take(&mut self.runners);
        self.missed = Some(vec![]);
        for r in &mut runners {
            r.run(self, now);
        }
        // What the handlers' own mutations changed, told them after.
        let missed = self.missed.take().unwrap_or_default();
        runners.iter_mut().for_each(|r| r.changed(&missed));
        runners.append(&mut self.runners);
        self.runners = runners;
    }

    /// When a handler's next job is due, on the clock [`Peer::deadline`]
    /// keeps.
    pub(crate) fn due(&self) -> Option<u64> {
        let next = self.runners.iter().filter_map(|r| r.next()).min()?;
        Some(monotonic_ms() + (next - now_ms()).max(0) as u64)
    }
}

/// One run on a thread of its own, under its claim's span: what it came
/// to kept for the peer's next turn, and the peer rung for it.
#[cfg(not(target_arch = "wasm32"))]
fn off<R: Effect>(f: Arc<F<R>>, peer: crate::handle::Weak<Peer>, ran: Arc<std::sync::Mutex<Vec<Ran<R>>>>, r: R, span: Span, label: String) {
    let named = std::thread::Builder::new().name(format!("ark-{}", R::TABLE));
    let run = move || {
        let _in = said::<R>(&label, &span).entered();
        let mut work = Work {
            at: At::Off(&peer),
            span,
            again: None,
        };
        let outcome = f(&mut work, &r);
        told(&outcome);
        ran.lock().unwrap_or_else(|e| e.into_inner()).push((r.key_bytes(), outcome, work.again));
        peer.ring();
    };
    named.spawn(run).expect("a thread for the job");
}
