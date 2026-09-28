//! §7.1 Alpha-normalisation (`Ark.Encode.normalize`).
//!
//! Symbols are renumbered 0, 1, 2… in the order their binders are met
//! walking the body top to bottom, left to right, binders before the
//! scopes they open — the evaluation order of `Ark.Eval`. Free symbols are
//! left as they are. A nested block's bindings do not escape it, but their
//! numbers are still consumed, so that numbering is a function of the whole
//! body.

use std::collections::BTreeMap;

use crate::ir::{Block, Check, Expr, Field, Function, Module, Plan, Pred, Stmt, Sym};

type Ren = BTreeMap<Sym, Sym>;

/// A function with its symbols renumbered in binding order and its names
/// carried across to the new numbers.
pub fn normalize(f: &Function) -> Function {
    let mut next: Sym = 0;
    let mut input = Vec::with_capacity(f.input.len());
    for (n, fd) in &f.input {
        let mut checks = Vec::with_capacity(fd.checks.len());
        for c in &fd.checks {
            checks.push(match c {
                Check::Refine(e, why) => {
                    let (e2, n2) = renumber_expr(&Ren::new(), next, e);
                    next = n2;
                    Check::Refine(e2, why.clone())
                }
                other => other.clone(),
            });
        }
        input.push((n.clone(), Field { ty: fd.ty.clone(), checks }));
    }
    let mut refine = Vec::with_capacity(f.refine.len());
    for (e, why) in &f.refine {
        let (e2, n2) = renumber_expr(&Ren::new(), next, e);
        next = n2;
        refine.push((e2, why.clone()));
    }
    let (body, mapping) = renumber_block(&Ren::new(), next, &f.body);
    let names = mapping
        .iter()
        .filter_map(|(old, new)| f.names.get(old).map(|n| (*new, n.clone())))
        .collect();
    Function {
        input,
        refine,
        body,
        names,
        ..f.clone()
    }
}

/// Every function of a module normalised.
pub fn normalize_module(m: &Module) -> Module {
    Module {
        functions: m.functions.iter().map(normalize).collect(),
        ..m.clone()
    }
}

fn renumber_block(ren: &Ren, next: Sym, blk: &Block) -> (Block, Ren) {
    let mut ren = ren.clone();
    let mut next = next;
    let mut out = Vec::with_capacity(blk.len());
    for s in blk {
        let (s2, ren2, next2) = renumber_stmt(&ren, next, s);
        out.push(s2);
        ren = ren2;
        next = next2;
    }
    (out, ren)
}

// The statement, the renaming extended by any binder it introduced for the
// statements after it, and the next free number.
fn renumber_stmt(ren: &Ren, next: Sym, s: &Stmt) -> (Stmt, Ren, Sym) {
    match s {
        Stmt::Let(x, e) => {
            let (e2, next2) = renumber_expr(ren, next, e);
            let mut ren2 = ren.clone();
            ren2.insert(*x, next2);
            (Stmt::Let(next2, e2), ren2, next2 + 1)
        }
        Stmt::If(c, a, b) => {
            let (c2, n1) = renumber_expr(ren, next, c);
            let (a2, n2) = inner(ren, n1, a);
            let (b2, n3) = inner(ren, n2, b);
            (Stmt::If(c2, a2, b2), ren.clone(), n3)
        }
        Stmt::For(x, xs, b) => {
            let (xs2, n1) = renumber_expr(ren, next, xs);
            let mut ren2 = ren.clone();
            ren2.insert(*x, n1);
            let (b2, n2) = inner(&ren2, n1 + 1, b);
            (Stmt::For(n1, xs2, b2), ren.clone(), n2)
        }
        Stmt::Insert(t, e, on) => {
            let (e2, n1) = renumber_expr(ren, next, e);
            (Stmt::Insert(t.clone(), e2, on.clone()), ren.clone(), n1)
        }
        Stmt::Upsert(t, e, on) => {
            let (e2, n1) = renumber_expr(ren, next, e);
            (Stmt::Upsert(t.clone(), e2, on.clone()), ren.clone(), n1)
        }
        // The key first, then the existing row's binder, then the new row.
        Stmt::Update(t, ks, x, e) => {
            let (ks2, n1) = renumber_many(ren, next, ks);
            let mut ren2 = ren.clone();
            ren2.insert(*x, n1);
            let (e2, n2) = renumber_expr(&ren2, n1 + 1, e);
            (Stmt::Update(t.clone(), ks2, n1, e2), ren.clone(), n2)
        }
        Stmt::Delete(t, ks) => {
            let (ks2, n1) = renumber_many(ren, next, ks);
            (Stmt::Delete(t.clone(), ks2), ren.clone(), n1)
        }
        Stmt::Refuse(e) => {
            let (e2, n1) = renumber_expr(ren, next, e);
            (Stmt::Refuse(e2), ren.clone(), n1)
        }
        Stmt::Return(None) => (Stmt::Return(None), ren.clone(), next),
        Stmt::Return(Some(e)) => {
            let (e2, n1) = renumber_expr(ren, next, e);
            (Stmt::Return(Some(e2)), ren.clone(), n1)
        }
    }
}

// A nested block: its bindings do not escape, their numbers are consumed.
fn inner(ren: &Ren, n: Sym, blk: &Block) -> (Block, Sym) {
    let (blk2, _) = renumber_block(ren, n, blk);
    let next = count_binders(&blk2, n);
    (blk2, next)
}

// The next free number after a renumbered block: one past the largest
// binder in it, or the given floor.
fn count_binders(blk: &Block, n: Sym) -> Sym {
    let mut best = n;
    for s in blk {
        for b in stmt_binders(s) {
            best = best.max(b + 1);
        }
    }
    best
}

fn stmt_binders(s: &Stmt) -> Vec<Sym> {
    match s {
        Stmt::Let(x, e) => {
            let mut v = vec![*x];
            v.extend(expr_binders(e));
            v
        }
        Stmt::If(c, a, b) => {
            let mut v = expr_binders(c);
            v.extend(a.iter().chain(b.iter()).flat_map(stmt_binders));
            v
        }
        Stmt::For(x, xs, b) => {
            let mut v = vec![*x];
            v.extend(expr_binders(xs));
            v.extend(b.iter().flat_map(stmt_binders));
            v
        }
        Stmt::Insert(_, e, _) | Stmt::Upsert(_, e, _) | Stmt::Refuse(e) => expr_binders(e),
        Stmt::Update(_, ks, x, e) => {
            let mut v = vec![*x];
            v.extend(ks.iter().flat_map(expr_binders));
            v.extend(expr_binders(e));
            v
        }
        Stmt::Delete(_, ks) => ks.iter().flat_map(expr_binders).collect(),
        Stmt::Return(me) => me.as_ref().map(expr_binders).unwrap_or_default(),
    }
}

fn expr_binders(e: &Expr) -> Vec<Sym> {
    match e {
        Expr::Match(e, x, a, b) => {
            let mut v = vec![*x];
            v.extend([e, a, b].iter().flat_map(|e| expr_binders(e)));
            v
        }
        Expr::Map(xs, x, b) | Expr::Filter(xs, x, b) | Expr::Any(xs, x, b) | Expr::All(xs, x, b) | Expr::SortBy(xs, x, b) => {
            let mut v = vec![*x];
            v.extend(expr_binders(xs));
            v.extend(expr_binders(b));
            v
        }
        Expr::Fold(xs, z, acc, x, b) => {
            let mut v = vec![*acc, *x];
            v.extend([xs, z, b].iter().flat_map(|e| expr_binders(e)));
            v
        }
        Expr::Field(e, _) | Expr::Some(e) => expr_binders(e),
        Expr::Struct(fs) => fs.values().flat_map(expr_binders).collect(),
        Expr::List(es) | Expr::Op(_, es) | Expr::Call(_, es) | Expr::Std(_, es) | Expr::Get(_, es) | Expr::Exists(_, es) => {
            es.iter().flat_map(expr_binders).collect()
        }
        Expr::If(c, a, b) => [c, a, b].iter().flat_map(|e| expr_binders(e)).collect(),
        Expr::Cmp(_, a, b) => {
            let mut v = expr_binders(a);
            v.extend(expr_binders(b));
            v
        }
        Expr::Select(p) => plan_binders(p),
        Expr::Lit(_) | Expr::Arg(_) | Expr::Auto(_) | Expr::Var(_) | Expr::CtxUser | Expr::CtxSession | Expr::Provided(_) | Expr::None(_) => vec![],
    }
}

fn plan_binders(p: &Plan) -> Vec<Sym> {
    let mut v = p.filter.as_ref().map(pred_binders).unwrap_or_default();
    v.extend(p.related.iter().flat_map(|r| plan_binders(&r.plan)));
    v
}

fn pred_binders(p: &Pred) -> Vec<Sym> {
    match p {
        Pred::Cmp(_, _, e) => expr_binders(e),
        Pred::In(_, es) => es.iter().flat_map(expr_binders).collect(),
        Pred::All(ps) | Pred::Any(ps) => ps.iter().flat_map(pred_binders).collect(),
        Pred::Not(q) => pred_binders(q),
    }
}

fn renumber_many(ren: &Ren, next: Sym, es: &[Expr]) -> (Vec<Expr>, Sym) {
    let mut n = next;
    let mut out = Vec::with_capacity(es.len());
    for e in es {
        let (e2, n2) = renumber_expr(ren, n, e);
        out.push(e2);
        n = n2;
    }
    (out, n)
}

fn renumber_expr(ren: &Ren, next: Sym, e: &Expr) -> (Expr, Sym) {
    let bx = Box::new;
    match e {
        Expr::Var(x) => (Expr::Var(*ren.get(x).unwrap_or(x)), next),
        Expr::Field(e, f) => {
            let (e2, n) = renumber_expr(ren, next, e);
            (Expr::Field(bx(e2), f.clone()), n)
        }
        Expr::Struct(fs) => {
            let vals: Vec<&Expr> = fs.values().collect();
            let mut n = next;
            let mut out = BTreeMap::new();
            for (k, v) in fs.keys().zip(vals) {
                let (v2, n2) = renumber_expr(ren, n, v);
                out.insert(k.clone(), v2);
                n = n2;
            }
            (Expr::Struct(out), n)
        }
        Expr::List(es) => {
            let (es2, n) = renumber_many(ren, next, es);
            (Expr::List(es2), n)
        }
        Expr::Some(e) => {
            let (e2, n) = renumber_expr(ren, next, e);
            (Expr::Some(bx(e2)), n)
        }
        Expr::Match(e, x, a, b) => {
            let (e2, n1) = renumber_expr(ren, next, e);
            let mut ren2 = ren.clone();
            ren2.insert(*x, n1);
            let (a2, n2) = renumber_expr(&ren2, n1 + 1, a);
            let (b2, n3) = renumber_expr(ren, n2, b);
            (Expr::Match(bx(e2), n1, bx(a2), bx(b2)), n3)
        }
        Expr::If(c, a, b) => {
            let (c2, n1) = renumber_expr(ren, next, c);
            let (a2, n2) = renumber_expr(ren, n1, a);
            let (b2, n3) = renumber_expr(ren, n2, b);
            (Expr::If(bx(c2), bx(a2), bx(b2)), n3)
        }
        Expr::Op(op, es) => {
            let (es2, n) = renumber_many(ren, next, es);
            (Expr::Op(*op, es2), n)
        }
        Expr::Cmp(op, a, b) => {
            let (a2, n1) = renumber_expr(ren, next, a);
            let (b2, n2) = renumber_expr(ren, n1, b);
            (Expr::Cmp(*op, bx(a2), bx(b2)), n2)
        }
        Expr::Call(f, es) => {
            let (es2, n) = renumber_many(ren, next, es);
            (Expr::Call(f.clone(), es2), n)
        }
        Expr::Std(f, es) => {
            let (es2, n) = renumber_many(ren, next, es);
            (Expr::Std(*f, es2), n)
        }
        Expr::Map(xs, x, b) => binder1(ren, next, xs, *x, b, |xs, x, b| Expr::Map(bx(xs), x, bx(b))),
        Expr::Filter(xs, x, b) => binder1(ren, next, xs, *x, b, |xs, x, b| Expr::Filter(bx(xs), x, bx(b))),
        Expr::Any(xs, x, b) => binder1(ren, next, xs, *x, b, |xs, x, b| Expr::Any(bx(xs), x, bx(b))),
        Expr::All(xs, x, b) => binder1(ren, next, xs, *x, b, |xs, x, b| Expr::All(bx(xs), x, bx(b))),
        Expr::SortBy(xs, x, k) => binder1(ren, next, xs, *x, k, |xs, x, k| Expr::SortBy(bx(xs), x, bx(k))),
        Expr::Fold(xs, z, acc, x, b) => {
            let (xs2, n0) = renumber_expr(ren, next, xs);
            let (z2, n1) = renumber_expr(ren, n0, z);
            let mut ren2 = ren.clone();
            ren2.insert(*acc, n1);
            ren2.insert(*x, n1 + 1);
            let (b2, n2) = renumber_expr(&ren2, n1 + 2, b);
            (Expr::Fold(bx(xs2), bx(z2), n1, n1 + 1, bx(b2)), n2)
        }
        Expr::Select(p) => {
            let (p2, n) = renumber_plan(ren, next, p);
            (Expr::Select(Box::new(p2)), n)
        }
        Expr::Get(t, ks) => {
            let (ks2, n) = renumber_many(ren, next, ks);
            (Expr::Get(t.clone(), ks2), n)
        }
        Expr::Exists(t, ks) => {
            let (ks2, n) = renumber_many(ren, next, ks);
            (Expr::Exists(t.clone(), ks2), n)
        }
        Expr::Lit(_) | Expr::Arg(_) | Expr::Auto(_) | Expr::CtxUser | Expr::CtxSession | Expr::Provided(_) | Expr::None(_) => (e.clone(), next),
    }
}

fn binder1(ren: &Ren, next: Sym, xs: &Expr, x: Sym, b: &Expr, mk: impl FnOnce(Expr, Sym, Expr) -> Expr) -> (Expr, Sym) {
    let (xs2, n1) = renumber_expr(ren, next, xs);
    let mut ren2 = ren.clone();
    ren2.insert(x, n1);
    let (b2, n2) = renumber_expr(&ren2, n1 + 1, b);
    (mk(xs2, n1, b2), n2)
}

fn renumber_plan(ren: &Ren, next: Sym, p: &Plan) -> (Plan, Sym) {
    let (filter, n1) = match &p.filter {
        None => (None, next),
        Some(f) => {
            let (f2, n) = renumber_pred(ren, next, f);
            (Some(f2), n)
        }
    };
    let mut n = n1;
    let mut related = Vec::with_capacity(p.related.len());
    for r in &p.related {
        let (rp, n2) = renumber_plan(ren, n, &r.plan);
        related.push(crate::ir::Related { plan: rp, ..r.clone() });
        n = n2;
    }
    (
        Plan {
            filter,
            related,
            ..p.clone()
        },
        n,
    )
}

fn renumber_pred(ren: &Ren, next: Sym, p: &Pred) -> (Pred, Sym) {
    match p {
        Pred::Cmp(c, op, e) => {
            let (e2, n) = renumber_expr(ren, next, e);
            (Pred::Cmp(c.clone(), *op, e2), n)
        }
        Pred::In(c, es) => {
            let (es2, n) = renumber_many(ren, next, es);
            (Pred::In(c.clone(), es2), n)
        }
        Pred::All(ps) => {
            let (ps2, n) = many_preds(ren, next, ps);
            (Pred::All(ps2), n)
        }
        Pred::Any(ps) => {
            let (ps2, n) = many_preds(ren, next, ps);
            (Pred::Any(ps2), n)
        }
        Pred::Not(q) => {
            let (q2, n) = renumber_pred(ren, next, q);
            (Pred::Not(Box::new(q2)), n)
        }
    }
}

fn many_preds(ren: &Ren, next: Sym, ps: &[Pred]) -> (Vec<Pred>, Sym) {
    let mut n = next;
    let mut out = Vec::with_capacity(ps.len());
    for p in ps {
        let (p2, n2) = renumber_pred(ren, n, p);
        out.push(p2);
        n = n2;
    }
    (out, n)
}
