//! A query held as a list and kept up to date.
//!
//! Hydrate once, then hand every [`Changes`] to [`View::update`] and splice
//! what it returns into whatever the screen built from the rows. A query
//! whose body is a single `select` with no middleware — `db.t.filter(..)
//! .order_by(..).all()` — is maintained incrementally by `ark::view`: the
//! cost of a change is the rows it moved, whatever the size of the list.
//! Any other query is re-run on a change and the difference
//! reported as patches, which is always correct and costs a query.
//!
//! `Changes::Rebuilt` — a rebase rolled the optimistic store back and
//! replayed pending on top — is a re-hydrate and [`Update::Reset`]: no
//! sequence of patches describes a rollback, so the view does not pretend
//! to have one.

use ark::eval::{Args, Ctx};
use ark::ir::{Expr, Function, Plan, Stmt};
use ark::peer::Changes;
use ark::store::{Change, MemoryStore, Store};
use ark::value::Value;
use ark::view::{self, Patch, ViewPlan};

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

enum How {
    Plan(view::View),
    Rerun,
}

/// A maintained query.
pub struct View {
    name: String,
    args: Args,
    how: How,
    rows: Vec<Value>,
}

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View")
            .field("name", &self.name)
            .field("incremental", &self.incremental())
            .field("rows", &self.rows.len())
            .finish()
    }
}

impl View {
    pub(crate) fn open(peer: &Peer, name: &str, args: Args) -> Result<View, Error> {
        let (_, f) = peer.domain().query(name)?;
        let rows = list(name, peer.query(name, &args)?)?;
        let how = match plan_of(f, peer, &args) {
            Some(vp) => {
                let v = view::hydrate(peer.schema(), &vp, peer.store());
                // Held to the query itself: a plan this reading got wrong
                // falls back to re-running, never to a wrong list.
                if v.rows() == rows {
                    How::Plan(v)
                } else {
                    How::Rerun
                }
            }
            None => How::Rerun,
        };
        Ok(View {
            name: name.into(),
            args,
            how,
            rows,
        })
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

    /// Whether changes are pushed through the plan rather than re-run.
    pub fn incremental(&self) -> bool {
        matches!(self.how, How::Plan(_))
    }

    /// Read the query again, whole.
    pub fn rehydrate(&mut self, peer: &Peer) -> Result<(), Error> {
        self.rows = list(&self.name, peer.query(&self.name, &self.args)?)?;
        if let How::Plan(v) = &mut self.how {
            *v = view::hydrate(peer.schema(), &v.plan, peer.store());
        }
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
            Changes::Applied(chs) if chs.is_empty() => return Ok(Update::Unchanged),
            Changes::Applied(chs) => chs,
        };
        let store = peer.store();
        let patches = match &mut self.how {
            How::Plan(v) => push_all(peer, store, chs, v),
            How::Rerun => {
                let new = list(&self.name, peer.query(&self.name, &self.args)?)?;
                diff(&self.rows, &new)
            }
        };
        if patches.is_empty() {
            return Ok(Update::Unchanged);
        }
        splice(&mut self.rows, &patches);
        Ok(Update::Patched(patches))
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

fn list(name: &str, v: Value) -> Result<Vec<Value>, Error> {
    match v {
        Value::List(xs) => Ok(xs),
        other => Err(Error::Bug(format!("{name} answers {other:?}, not a list; a view holds a list"))),
    }
}

/// Every change pushed through the plan, each against the store as it
/// stood just after that change: the store now, rolled back through the
/// later ones. One change needs no rolling.
fn push_all(peer: &Peer, now: &MemoryStore, chs: &[Change], v: &mut view::View) -> Vec<Patch> {
    let sch = peer.schema();
    let mut out = vec![];
    if let [ch] = chs {
        let (v2, ps) = view::push(sch, now, ch, v);
        *v = v2;
        return ps;
    }
    let mut st = now.clone();
    for ch in chs.iter().rev() {
        st.apply_change(&invert(ch));
    }
    for ch in chs {
        st.apply_change(ch);
        let (v2, ps) = view::push(sch, &st, ch, v);
        *v = v2;
        out.extend(ps);
    }
    out
}

fn invert(ch: &Change) -> Change {
    match ch {
        Change::Add(t, r) => Change::Remove(t.clone(), r.clone()),
        Change::Remove(t, r) => Change::Add(t.clone(), r.clone()),
        Change::Edit(t, o, n) => Change::Edit(t.clone(), n.clone(), o.clone()),
    }
}

/// The patches from one list to another: the common ends kept, the middle
/// removed and inserted.
pub fn diff(old: &[Value], new: &[Value]) -> Vec<Patch> {
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suf = old.len().min(new.len()) - pre;
    let suf = old.iter().rev().zip(new.iter().rev()).take(max_suf).take_while(|(a, b)| a == b).count();
    let mut ps: Vec<Patch> = (0..old.len() - pre - suf).map(|_| Patch::Remove { at: pre }).collect();
    ps.extend(new[pre..new.len() - suf].iter().enumerate().map(|(i, n)| Patch::Insert {
        at: pre + i,
        node: n.clone(),
    }));
    ps
}

/// The plan of a query that is one `select` and nothing else, with its
/// right-hand sides evaluated against the (checked) arguments and the
/// peer's identity; `None` for anything else.
fn plan_of(f: &Function, peer: &Peer, args: &Args) -> Option<ViewPlan> {
    if !f.uses.is_empty() {
        return None;
    }
    let plan: &Plan = match f.body.as_slice() {
        [Stmt::Return(Some(Expr::Select(p)))] => p,
        [Stmt::Let(s, Expr::Select(p)), Stmt::Return(Some(Expr::Var(v)))] if s == v => p,
        _ => return None,
    };
    let checked = peer.check(&f.name, args).ok()?;
    if !checked.messages.is_empty() {
        return None;
    }
    let ctx: &Ctx = peer.ctx();
    view::eval_plan(plan, &mut |e: &Expr| match e {
        Expr::Lit(v) => Ok(v.clone()),
        Expr::Arg(n) => checked.values.get(n).cloned().ok_or(()),
        Expr::CtxUser => Ok(Value::text(ctx.user.clone())),
        Expr::CtxSession => Ok(Value::text(ctx.session.clone())),
        _ => Err(()),
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_splices_into_the_new_list() {
        let n = |xs: &[i64]| xs.iter().map(|x| Value::Int(*x)).collect::<Vec<_>>();
        for (a, b) in [
            (vec![], vec![1, 2]),
            (vec![1, 2, 3], vec![1, 3]),
            (vec![1, 2, 3], vec![1, 9, 9, 3]),
            (vec![1, 1, 1], vec![1, 1]),
            (vec![5], vec![]),
            (vec![1, 2], vec![1, 2]),
        ] {
            let (old, new) = (n(&a), n(&b));
            let mut xs = old.clone();
            splice(&mut xs, &diff(&old, &new));
            assert_eq!(xs, new, "{a:?} -> {b:?}");
        }
        assert!(diff(&n(&[1, 2]), &n(&[1, 2])).is_empty());
    }
}
