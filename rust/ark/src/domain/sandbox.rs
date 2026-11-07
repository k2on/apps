//! A domain run over one store, as the machine runs it, with no log: what
//! a seed authors a demo's rows through, and a domain's own tests run it
//! by.

use std::sync::Arc;

use crate::format::Encode;
use crate::log::EntryId;
use crate::peer::Ctx;
use crate::row::Row;
use crate::schema::Space;
use crate::store::{Change, Store};

use super::{Domain, Func};

/// One store of a domain's, each mutation run over it as the entry a peer
/// would have authored — its id the next of its session's, its clock a
/// millisecond on from the last — and each view asked of it.
pub struct Sandbox {
    pub domain: Arc<Domain>,
    pub store: Store,
    /// Who authors the next entry, and asks.
    pub ctx: Ctx,
    /// What the last mutation changed.
    pub last: Vec<Change>,
    entries: i64,
}

impl Sandbox {
    /// The durable store of `domain`, empty, authored in as `ctx` says.
    pub fn new(domain: Arc<Domain>, ctx: Ctx) -> Sandbox {
        let store = Store::new(domain.schema.clone(), Space::Durable);
        let last = vec![];
        Sandbox {
            domain,
            store,
            ctx,
            last,
            entries: 0,
        }
    }

    /// Who authors from now on, holding `roles`.
    pub fn as_(&mut self, actor: &str, roles: &[&str]) -> &mut Self {
        self.ctx.actor = actor.into();
        self.ctx.roles = roles.iter().map(|r| r.to_string()).collect();
        self
    }

    /// The next entry's context: numbered on in its session, its clock on.
    pub fn entry(&mut self) -> Ctx {
        self.entries += 1;
        self.ctx.now += 1;
        self.ctx.id = EntryId::of(&self.ctx.session, self.entries);
        self.ctx.clone()
    }

    /// `f` run on `input` as the next entry: how many changes it made, or
    /// its refusal.
    pub fn run<I: Encode>(&mut self, f: Func<I>, input: &I) -> Result<usize, String> {
        let ctx = self.entry();
        self.run_at(&ctx, f.name, &input.to_bytes())
    }

    /// The function `name` run on its input's bytes as the entry `ctx`.
    pub fn run_at(&mut self, ctx: &Ctx, name: &str, input: &[u8]) -> Result<usize, String> {
        let f = self.domain.get(name).and_then(|f| f.proc.clone());
        self.last = f.ok_or_else(|| format!("{name} writes nothing"))?(&mut self.store, ctx, input)?;
        Ok(self.last.len())
    }

    /// The view `f`'s answer on `input`, as [`Sandbox::ctx`] reads it.
    pub fn ask<I: Encode, T: Encode>(&self, f: Func<I, T>, input: &I) -> Result<Vec<T>, String> {
        let b = self.domain.ask(&self.store, &self.ctx, f.name, &input.to_bytes())?;
        Vec::from_bytes(&b).map_err(|e| format!("{}: {e}", f.name))
    }

    /// The rows of `R`'s table.
    pub fn rows<R: Row>(&self) -> Vec<R> {
        crate::plan::rows(&self.store)
    }
}
