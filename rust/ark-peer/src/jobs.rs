//! Handlers and reconcilers: native code acting on the world outside,
//! answering with mutations (effects as rows, `ark::effect`).
//!
//! A **handler** ([`Peer::handle`]) is registered for a job row `R` on the
//! one process that runs it. It keeps the rows of `R` that are not over,
//! as every durable change moves them, and the nearest time one is due
//! ([`Peer::deadline`] wakes the driver for it). Each due row is claimed
//! (`<table>.claim`, by this registration's own name); once the claim is
//! confirmed — the authority's order decided it — the handler runs with
//! the row, whose key is its idempotency key, and the peer, to author
//! what it found; its answer is the `<table>.finish`. Nothing runs during a replay or an open: rows only
//! move what is kept, and a handler runs when its driver pumps the peer
//! ([`Peer::jobs`]), never on a peer that did not register it. A claim
//! from before a restart is another's: its row is due again only when its
//! lease runs out. The handler is a plain function on the driver's
//! thread: ark-peer has no executor (it is pumped, natively and in a
//! browser), so a handler that waits blocks only its own peer's thread.
//!
//! A **reconciler** ([`Peer::reconcile`]) is told a view's rows, typed,
//! every time they change, and once at the start: no claim, latest wins.

use std::collections::BTreeMap;

use ark::effect::{Claim, Effect, Finish};
use ark::log::{EntryId, Span, TraceId};
use ark::peer::Told;
use ark::row::Value;
use ark::schema::Space;
use ark::store::Change;

use crate::app::Standing;
use crate::link::Sendable;
use crate::{monotonic_ms, now_ms, Peer};

/// One registration of a handler, over its job's rows.
pub(crate) trait Runner: Sendable {
    fn changed(&mut self, cs: &[Told]);
    /// When the next row not claimed here is due.
    fn next(&self) -> Option<i64>;
    fn run(&mut self, p: &mut Peer, now: i64);
}

struct Handler<R: Effect, F> {
    f: F,
    by: String,
    /// The rows not over, by key: when each is due, and the span of the
    /// entry that last changed it, which its claim and its run are under.
    open: BTreeMap<Vec<u8>, (R, i64, Option<Span>)>,
    /// The claims made, until their verdict: each the claim's id, and its span.
    claims: BTreeMap<Vec<u8>, (EntryId, Span)>,
}

impl<R: Effect, F: FnMut(&mut Peer, &R) -> Result<R::Outcome, String> + Sendable> Runner for Handler<R, F> {
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
                Some(due) => drop(self.open.insert(r.key_bytes(), (r, due, *by))),
                None => drop(self.open.remove(&r.key_bytes())),
            }
        }
    }

    fn next(&self) -> Option<i64> {
        self.open
            .iter()
            .filter(|(k, _)| !self.claims.contains_key(*k))
            .map(|(_, (_, d, _))| *d)
            .min()
    }

    fn run(&mut self, p: &mut Peer, now: i64) {
        let due = self.open.iter().filter(|(k, (_, d, _))| *d <= now && !self.claims.contains_key(*k));
        let due: Vec<(Vec<u8>, R::Id, Option<Span>)> = due.map(|(k, (r, _, by))| (k.clone(), r.id(), *by)).collect();
        for (k, id, by) in due {
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
            // An authority's own claim is decided as it is made.
            match if p.hub.serving { Standing::Confirmed } else { p.standing(&e) } {
                Standing::Pending => drop(self.claims.insert(k, (e, span))),
                Standing::Confirmed => {
                    let Some((r, _, _)) = self.open.get(&k) else { continue };
                    // The run, and all it authors, under the claim.
                    let run = tracing::info_span!(target: "ark", "job", peer = %p.label(), table = R::TABLE, span = ?span);
                    let _in = run.enter();
                    let outcome = p.under(Some(span), |p| (self.f)(p, r));
                    match &outcome {
                        Ok(_) => tracing::info!(target: "ark", "done"),
                        Err(why) => tracing::warn!(target: "ark", why = %why, "failed"),
                    }
                    let finish = Finish::<R> {
                        id: r.id(),
                        by: self.by.clone(),
                        outcome,
                    };
                    let _ = p.under(Some(span), |p| p.mutate(&format!("{}.finish", R::TABLE), &finish));
                }
                // Another's claim came first.
                _ => {}
            }
        }
    }
}

/// A view's rows, told on each change.
pub(crate) type Reconciler = Box<dyn FnMut(&[Vec<u8>]) + Send>;

impl Peer {
    /// `f` handles the jobs of `R`: each due row claimed, `f` run with this
    /// peer and the row once the claim is confirmed, its answer the row's
    /// finish.
    pub fn handle<R: Effect>(&mut self, f: impl FnMut(&mut Peer, &R) -> Result<R::Outcome, String> + Sendable + 'static) {
        let by = format!("{}:{}", self.me().session, ark::hash::hex(&crate::draw_origin().0[..6]));
        let mut h = Handler::<R, _> {
            f,
            by,
            open: BTreeMap::new(),
            claims: BTreeMap::new(),
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

    /// The handlers' turn: what is due claimed, what is confirmed run.
    pub fn jobs(&mut self, now: i64) {
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

    /// `f` is told the rows of the domain's view `name` on `input` over
    /// `space`, typed, now and on every change.
    pub fn reconcile<T: Value>(
        &mut self,
        space: Space,
        name: &str,
        input: &[u8],
        mut f: impl FnMut(Vec<T>) + Send + 'static,
    ) -> Result<usize, String> {
        let b = self.binding(name).ok_or_else(|| format!("no view {name}"))?;
        let v = self.watch(space, &b, input, self.ctx()).map_err(|r| r.0)?;
        let mut tell: Reconciler = Box::new(move |rows| f(rows.iter().filter_map(|b| T::decode(b).ok()).collect()));
        tell(&self.rows(v));
        self.reconcilers.insert(v, tell);
        Ok(v)
    }
}
