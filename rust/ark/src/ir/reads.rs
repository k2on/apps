//! §1.7 What a function reads outside a plan, by static inspection.
//!
//! A maintained query runs its middleware at hydrate and must run it again
//! when a table the middleware read moves (`docs/plan-v4.md` §1.7): a
//! playlist renamed or deleted under an open page changes what `owned`
//! provides, or whether it refuses. Which tables those are is a property
//! of the IR, so the engine answers it here rather than each client
//! guessing: every table a `Select`, `Get` or `Exists` in the function's
//! body names (a `Select`'s related and looked-up tables too), every table
//! a write statement names (a write reads the row it replaces), and every
//! table an input field's `exists` check reads. Helpers cannot read
//! (§3.4), so a `Call` adds nothing.
//!
//! A query's own plan is deliberately left out: its reads are the plan's
//! nodes, which the view maintains (§1.5), and counting them here would
//! re-run the middleware on every change the view already routes.

use std::collections::BTreeSet;

use super::{Block, Check, Expr, Function, Plan, Pred, Stmt};
use crate::value::TableName;

/// The tables `f` reads outside its plan: its input checks (`exists`), its
/// refinements and its body. See the module documentation.
pub fn reads(f: &Function) -> BTreeSet<TableName> {
    let mut out = BTreeSet::new();
    for (_, field) in &f.input {
        for c in &field.checks {
            match c {
                Check::Exists(_) => {
                    if let Some(t) = crate::eval::id_table(&field.ty) {
                        out.insert(t.to_string());
                    }
                }
                Check::Refine(e, _) => expr(e, &mut out),
                _ => {}
            }
        }
    }
    for (e, _) in &f.refine {
        expr(e, &mut out);
    }
    block(&f.body, &mut out);
    out
}

fn block(b: &Block, out: &mut BTreeSet<TableName>) {
    for s in b {
        match s {
            Stmt::Let(_, e) | Stmt::Refuse(e) | Stmt::Return(Some(e)) => expr(e, out),
            Stmt::Return(None) => {}
            Stmt::If(c, t, f) => {
                expr(c, out);
                block(t, out);
                block(f, out);
            }
            Stmt::For(_, e, b) => {
                expr(e, out);
                block(b, out);
            }
            Stmt::Insert(t, e, _) | Stmt::Upsert(t, e, _) => {
                out.insert(t.clone());
                expr(e, out);
            }
            Stmt::Update(t, k, _, e) => {
                out.insert(t.clone());
                k.iter().for_each(|x| expr(x, out));
                expr(e, out);
            }
            Stmt::Delete(t, k) => {
                out.insert(t.clone());
                k.iter().for_each(|x| expr(x, out));
            }
        }
    }
}

fn plan(p: &Plan, out: &mut BTreeSet<TableName>) {
    out.insert(p.table().clone());
    if let Some(f) = &p.filter {
        pred(f, out);
    }
    for l in &p.lookups {
        out.insert(l.table.clone());
        l.key.iter().for_each(|e| expr(e, out));
    }
    for r in &p.related {
        r.on.iter().for_each(|(_, e)| expr(e, out));
        plan(&r.plan, out);
    }
    for e in p.having.iter().chain(&p.project) {
        expr(e, out);
    }
    for (k, _) in &p.order {
        if let super::Key::Expr(e) = k {
            expr(e, out);
        }
    }
}

fn pred(p: &Pred, out: &mut BTreeSet<TableName>) {
    match p {
        Pred::Cmp(_, _, e) => expr(e, out),
        Pred::In(_, es) => es.iter().for_each(|e| expr(e, out)),
        Pred::All(ps) | Pred::Any(ps) => ps.iter().for_each(|q| pred(q, out)),
        Pred::Not(q) => pred(q, out),
        Pred::Has(_, e) => expr(e, out),
    }
}

fn expr(e: &Expr, out: &mut BTreeSet<TableName>) {
    match e {
        Expr::Lit(_)
        | Expr::Arg(_)
        | Expr::Auto(_)
        | Expr::Var(_)
        | Expr::CtxUser
        | Expr::CtxSession
        | Expr::HasRole(_)
        | Expr::Provided(_)
        | Expr::None(_) => {}
        Expr::Field(x, _) | Expr::Some(x) => expr(x, out),
        Expr::Struct(fs) => fs.values().for_each(|x| expr(x, out)),
        Expr::List(xs) | Expr::Op(_, xs) | Expr::Call(_, xs) | Expr::Std(_, xs) => xs.iter().for_each(|x| expr(x, out)),
        Expr::Match(a, _, b, c) | Expr::If(a, b, c) => {
            expr(a, out);
            expr(b, out);
            expr(c, out);
        }
        Expr::Cmp(_, a, b) | Expr::Map(a, _, b) | Expr::Filter(a, _, b) | Expr::Any(a, _, b) | Expr::All(a, _, b) | Expr::SortBy(a, _, b) => {
            expr(a, out);
            expr(b, out);
        }
        Expr::Fold(a, b, _, _, c) => {
            expr(a, out);
            expr(b, out);
            expr(c, out);
        }
        Expr::Select(p) => plan(p, out),
        Expr::Get(t, k) | Expr::Exists(t, k) => {
            out.insert(t.clone());
            k.iter().for_each(|x| expr(x, out));
        }
    }
}
