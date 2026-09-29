//! A query held as a list and kept up to date.
//!
//! Hydrate once, then hand every [`Changes`] to [`View::update`] and splice
//! what it returns into whatever the screen built from the rows. Every
//! query is maintained (`docs/plan-v4.md` §1.5): a query is a plan, and
//! `ark::view` keeps a plan's entries, so a change costs the entries it
//! touches — rebuilt from the store through its indexes — whatever the size
//! of the list, and a batch of changes costs each touched entry once.
//!
//! A query's middleware — its input checks, guards and provides — runs
//! before the hydrate, over the same store, and decides the scope the plan
//! is pulled in (§1.7). The view keeps the tables that middleware reads
//! ([`ark::ir::reads`]); a change to one of them re-runs it before anything
//! is pushed, and a different outcome — another provided value, a refusal
//! appearing or clearing, or another user signed in — re-hydrates (to
//! nothing, on a refusal) and reports [`Update::Reset`].
//!
//! `Changes::Rebuilt` — a rebase rolled the optimistic store back and
//! replayed pending on top — is a re-hydrate and [`Update::Reset`] too: no
//! sequence of patches describes a rollback (§1.6), so the view does not
//! pretend to have one.

use std::collections::BTreeSet;

use ark::eval::{self, Args, Ctx, EvalFault};
use ark::hash::Closure;
use ark::peer::Changes;
use ark::store::Change;
use ark::value::{TableName, Value};
use ark::view::{self, Env, Patch};

use crate::peer::Peer;
use crate::Error;

/// What an update did to the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Unchanged,
    /// Apply these, in order, to the list as it stood ([`splice`]).
    Patched(Vec<Patch>),
    /// The list was read again: take [`View::rows`] whole.
    Reset,
}

/// What the middleware came to: the checked input and the provided values
/// the plan is pulled with, or the verdict that leaves the list empty.
type Outcome = Result<(Args, Args), ark::store::Refusal>;

/// A maintained query.
pub struct View {
    name: String,
    args: Args,
    /// Who the middleware and the plan ran as: another user signed in is
    /// another outcome.
    ctx: Ctx,
    /// The tables the middleware reads (§1.7).
    guards: BTreeSet<TableName>,
    outcome: Outcome,
    /// The plan's entries; `None` while the middleware refuses.
    held: Option<view::View>,
    rows: Vec<Value>,
}

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View")
            .field("name", &self.name)
            .field("refused", &self.outcome.is_err())
            .field("rows", &self.rows.len())
            .finish()
    }
}

impl View {
    /// Run the middleware, hydrate, and keep what the middleware read. A
    /// refusal at open is the caller's answer, as a query's would be.
    pub(crate) fn open(peer: &Peer, name: &str, args: Args) -> Result<View, Error> {
        let c = closure(peer, name)?;
        let mut guards = ark::ir::reads(&c.function);
        for u in &c.function.uses {
            if let Some(mw) = c.helpers.iter().find(|h| h.name == *u) {
                guards.extend(ark::ir::reads(mw));
            }
        }
        let outcome = middleware(peer, name, &c, &args)?;
        if let Err(r) = &outcome {
            return Err(Error::Refused(r.clone()));
        }
        let mut v = View {
            name: name.into(),
            args,
            ctx: peer.ctx().clone(),
            guards,
            outcome,
            held: None,
            rows: vec![],
        };
        v.hydrate(peer, &c)?;
        Ok(v)
    }

    /// The list, as it stands.
    pub fn rows(&self) -> &[Value] {
        &self.rows
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn args(&self) -> &Args {
        &self.args
    }

    /// Whether the view holds the plan's entries — every query's does,
    /// except while its middleware refuses and the list is empty.
    pub fn incremental(&self) -> bool {
        self.held.is_some()
    }

    /// The engine's view underneath, while there is one: its entries and
    /// indexes, to look at.
    pub fn entries(&self) -> Option<&view::View> {
        self.held.as_ref()
    }

    /// Run the middleware again and hydrate whole.
    pub fn rehydrate(&mut self, peer: &Peer) -> Result<(), Error> {
        let c = closure(peer, &self.name)?;
        self.ctx = peer.ctx().clone();
        self.outcome = middleware(peer, &self.name, &c, &self.args)?;
        self.hydrate(peer, &c)
    }

    // The plan pulled in the scope the outcome gives, or nothing on a
    // refusal.
    fn hydrate(&mut self, peer: &Peer, c: &Closure) -> Result<(), Error> {
        let Ok((args, provided)) = &self.outcome else {
            self.held = None;
            self.rows = vec![];
            return Ok(());
        };
        let plan = c
            .function
            .plan
            .as_ref()
            .ok_or_else(|| Error::Bug(format!("{}: a query with no plan", self.name)))?;
        let env = Env {
            helpers: c.helpers.clone(),
            ctx: self.ctx.clone(),
            args: args.clone(),
            provided: provided.clone(),
        };
        let v = view::hydrate(peer.schema(), plan, env, peer.store()).map_err(|e| fault(&self.name, e))?;
        self.rows = v.rows();
        self.held = Some(v);
        Ok(())
    }

    /// Bring the list up to date with what moved. `changes` is what
    /// [`Peer::take_changes`] returned, handed to every view in turn.
    pub fn update(&mut self, peer: &Peer, changes: &Changes) -> Result<Update, Error> {
        let chs = match changes {
            Changes::Rebuilt => {
                self.rehydrate(peer)?;
                return Ok(Update::Reset);
            }
            Changes::Applied(chs) => chs,
        };
        // §1.7 The middleware first: whoever is signed in, and whatever
        // the tables it reads now say.
        if *peer.ctx() != self.ctx || chs.iter().any(|c| self.guards.contains(c.table())) {
            let c = closure(peer, &self.name)?;
            let outcome = middleware(peer, &self.name, &c, &self.args)?;
            if *peer.ctx() != self.ctx || outcome != self.outcome {
                self.ctx = peer.ctx().clone();
                self.outcome = outcome;
                self.hydrate(peer, &c)?;
                return Ok(Update::Reset);
            }
        }
        let Some(v) = &mut self.held else {
            return Ok(Update::Unchanged);
        };
        if chs.is_empty() {
            return Ok(Update::Unchanged);
        }
        let patches = match push(peer, chs, v) {
            Ok(ps) => ps,
            // A fault leaves the engine's view as it was, and stale: read it
            // again, and say so.
            Err(_) => {
                self.rehydrate(peer)?;
                return Ok(Update::Reset);
            }
        };
        if patches.is_empty() {
            return Ok(Update::Unchanged);
        }
        splice(&mut self.rows, &patches);
        Ok(Update::Patched(patches))
    }
}

fn push(peer: &Peer, chs: &[Change], v: &mut view::View) -> Result<Vec<Patch>, EvalFault> {
    view::push_all(peer.schema(), peer.store(), chs, v)
}

fn closure(peer: &Peer, name: &str) -> Result<Closure, Error> {
    let (fh, _) = peer.domain().query(name)?;
    Ok(peer.domain().closures()[fh].clone())
}

// The middleware over the peer's store, as the peer's user: a verdict is an
// outcome, a bug is an error.
fn middleware(peer: &Peer, name: &str, c: &Closure, args: &Args) -> Result<Outcome, Error> {
    match eval::middleware(peer.schema(), c, peer.ctx(), args, peer.store()) {
        Ok(o) => Ok(Ok(o)),
        Err(EvalFault::Verdict(r)) => Ok(Err(r)),
        Err(EvalFault::Bug(b)) => Err(Error::Bug(format!("{name}: {b:?}"))),
    }
}

fn fault(name: &str, e: EvalFault) -> Error {
    match e {
        EvalFault::Verdict(r) => Error::Refused(r),
        EvalFault::Bug(b) => Error::Bug(format!("{name}: {b:?}")),
    }
}

/// Apply patches in order, in place: the definition a list is held to
/// (`ark::view::splice`, without the copy).
pub fn splice<T: Clone>(xs: &mut Vec<T>, ps: &[Patch])
where
    Value: Into<T>,
{
    for p in ps {
        match p {
            Patch::Insert { at, node } => {
                let at = (*at).min(xs.len());
                xs.insert(at, node.clone().into());
            }
            Patch::Remove { at } => {
                if *at < xs.len() {
                    xs.remove(*at);
                }
            }
            Patch::Update { at, node } => {
                if *at < xs.len() {
                    xs[*at] = node.clone().into();
                }
            }
        }
    }
}
