//! §7 The module as a value (`Ark.Encode`): every node a struct with a `"t"`
//! tag, so that [`crate::canon::encode`] is the only encoder there is and
//! a module's bytes are canonical by the same rule as a row's. Symbol names
//! are not carried; the hash never sees them.

use std::collections::BTreeMap;

use crate::hash::Closure;
use crate::ir::normalize::normalize;
use crate::ir::{Auto, Check, Expr, Field, Function, Key, Lookup, Module, Plan, Pred, Related, Router, Source, Stmt};
use crate::schema::{Dir, Schema, Ty};
use crate::value::{FieldName, Value};

fn node(t: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut m: BTreeMap<FieldName, Value> = BTreeMap::new();
    m.insert("t".into(), Value::text(t));
    for (k, v) in fields {
        m.insert(k.into(), v);
    }
    Value::Struct(m)
}

fn txt(s: &str) -> Value {
    Value::text(s)
}

fn int(n: i64) -> Value {
    Value::Int(n)
}

fn list<T>(f: impl Fn(&T) -> Value, xs: &[T]) -> Value {
    Value::List(xs.iter().map(f).collect())
}

/// The whole module (`Ark.Encode.toValue`). Functions are carried without
/// their dependency hashes, because the module carries the helpers.
pub fn module_value(m: &Module) -> Value {
    node(
        "module",
        vec![
            ("spec", int(m.spec)),
            ("schema", schema_value(&m.schema)),
            ("functions", list(|f| function_value(&BTreeMap::new(), f), &m.functions)),
            ("routers", list(router_value, &m.routers)),
            ("live", list(|(n, t)| node("frame", vec![("name", txt(n)), ("ty", ty_value(t))]), &m.live)),
        ],
    )
}

/// §1.1 A router (`{"t":"router","name","uses"}`).
pub fn router_value(r: &Router) -> Value {
    node("router", vec![("name", txt(&r.name)), ("uses", list(|u| txt(u), &r.uses))])
}

pub fn schema_value(sch: &Schema) -> Value {
    list(table_value, &sch.tables)
}

fn table_value(t: &crate::schema::Table) -> Value {
    node(
        "table",
        vec![
            ("name", txt(&t.name)),
            (
                "columns",
                list(
                    |c| {
                        node(
                            "column",
                            vec![("name", txt(&c.name)), ("ty", ty_value(&c.ty)), ("nullable", Value::Bool(c.nullable))],
                        )
                    },
                    &t.columns,
                ),
            ),
            ("key", list(|k| txt(k), &t.key)),
            (
                "indexes",
                list(
                    |i| {
                        node(
                            "index",
                            vec![("columns", list(|c| txt(c), &i.columns)), ("unique", Value::Bool(i.unique))],
                        )
                    },
                    &t.indexes,
                ),
            ),
            (
                "refs",
                list(|r| node("ref", vec![("column", txt(&r.column)), ("table", txt(&r.table))]), &t.refs),
            ),
        ],
    )
}

pub fn ty_value(t: &Ty) -> Value {
    match t {
        Ty::Bool => node("bool", vec![]),
        Ty::Int => node("int", vec![]),
        Ty::Text => node("text", vec![]),
        Ty::Bytes => node("bytes", vec![]),
        Ty::Id(t) => node("id", vec![("table", txt(t))]),
        Ty::Enum(vs) => node("enum", vec![("variants", list(|v| txt(v), vs))]),
        Ty::Option(t) => node("option", vec![("of", ty_value(t))]),
        Ty::List(t) => node("list", vec![("of", ty_value(t))]),
        Ty::Struct(fs) => node(
            "struct",
            vec![("fields", Value::Struct(fs.iter().map(|(k, v)| (k.clone(), ty_value(v))).collect()))],
        ),
    }
}

/// One function, normalised, with the hashes of the helpers and middleware
/// it reaches directly (`Ark.Encode.functionValue`). Names are not carried.
/// A query's `plan` is written beside its (empty) body; every other kind
/// has none and writes no key for it, so its bytes are v3's (§1.8).
pub fn function_value(deps: &BTreeMap<String, Value>, fn0: &Function) -> Value {
    let f = normalize(fn0);
    let mut fields = vec![
        ("name", txt(&f.name)),
        ("deps", Value::Struct(deps.clone())),
        ("kind", txt(f.kind.name())),
        ("router", f.router.as_deref().map(txt).unwrap_or(Value::Null)),
        ("uses", list(|u| txt(u), &f.uses)),
        (
            "autos",
            list(
                |(n, a)| match a {
                    Auto::NewId(t) => node("new_id", vec![("name", txt(n)), ("table", txt(t))]),
                    Auto::Now => node("now", vec![("name", txt(n))]),
                },
                &f.autos,
            ),
        ),
        ("input", list(|(n, fd)| field_value(n, fd), &f.input)),
        (
            "refine",
            list(|(e, why)| node("refine", vec![("e", expr(e)), ("why", why_value(why))]), &f.refine),
        ),
        ("ret", f.ret.as_ref().map(ty_value).unwrap_or(Value::Null)),
        ("body", list(stmt, &f.body)),
    ];
    if let Some(p) = &f.plan {
        fields.push(("plan", plan(p)));
    }
    node("fn", fields)
}

fn why_value(why: &Option<String>) -> Value {
    why.as_deref().map(txt).unwrap_or(Value::Null)
}

fn opt_int(n: &Option<i64>) -> Value {
    n.map(int).unwrap_or(Value::Null)
}

/// §1.3 One input field (`{"t":"field","name","ty","checks"}`).
pub fn field_value(name: &str, f: &Field) -> Value {
    node(
        "field",
        vec![("name", txt(name)), ("ty", ty_value(&f.ty)), ("checks", list(check_value, &f.checks))],
    )
}

/// §1.3 One check.
pub fn check_value(c: &Check) -> Value {
    match c {
        Check::Trim => node("trim", vec![]),
        Check::MinLen(n, why) => node("min_len", vec![("n", int(*n)), ("why", why_value(why))]),
        Check::MaxLen(n, why) => node("max_len", vec![("n", int(*n)), ("why", why_value(why))]),
        Check::Range(lo, hi, why) => node("range", vec![("lo", opt_int(lo)), ("hi", opt_int(hi)), ("why", why_value(why))]),
        Check::NonEmpty(why) => node("non_empty", vec![("why", why_value(why))]),
        Check::Exists(why) => node("exists", vec![("why", why_value(why))]),
        Check::Refine(e, why) => node("refine", vec![("e", expr(e)), ("why", why_value(why))]),
    }
}

/// A closure as an authority sends one: `{ t: "closure", fn, helpers }`
/// (`Ark.Hash.closureValue`).
pub fn closure_value(c: &Closure) -> Value {
    node(
        "closure",
        vec![
            ("fn", function_value(&BTreeMap::new(), &c.function)),
            ("helpers", list(|h| function_value(&BTreeMap::new(), h), &c.helpers)),
        ],
    )
}

fn stmt(s: &Stmt) -> Value {
    match s {
        Stmt::Let(x, e) => node("let", vec![("sym", int(*x)), ("e", expr(e))]),
        Stmt::If(c, a, b) => node("if", vec![("c", expr(c)), ("then", list(stmt, a)), ("else", list(stmt, b))]),
        Stmt::For(x, xs, b) => node("for", vec![("sym", int(*x)), ("in", expr(xs)), ("body", list(stmt, b))]),
        Stmt::Insert(t, e, on) => node("insert", vec![("table", txt(t)), ("row", expr(e)), ("on", list(|c| txt(c), on))]),
        Stmt::Upsert(t, e, on) => node("upsert", vec![("table", txt(t)), ("row", expr(e)), ("on", list(|c| txt(c), on))]),
        Stmt::Update(t, ks, x, e) => node(
            "update",
            vec![("table", txt(t)), ("key", list(expr, ks)), ("sym", int(*x)), ("row", expr(e))],
        ),
        Stmt::Delete(t, ks) => node("delete", vec![("table", txt(t)), ("key", list(expr, ks))]),
        Stmt::Refuse(e) => node("refuse", vec![("e", expr(e))]),
        Stmt::Return(me) => node("return", vec![("e", me.as_ref().map(expr).unwrap_or(Value::Null))]),
    }
}

fn expr(e: &Expr) -> Value {
    match e {
        Expr::Lit(v) => node("lit", vec![("v", v.clone())]),
        Expr::Arg(a) => node("arg", vec![("name", txt(a))]),
        Expr::Auto(a) => node("auto", vec![("name", txt(a))]),
        Expr::Var(x) => node("var", vec![("sym", int(*x))]),
        Expr::CtxUser => node("ctx_user", vec![]),
        Expr::CtxSession => node("ctx_session", vec![]),
        Expr::Provided(n) => node("provided", vec![("fn", txt(n))]),
        Expr::Field(e, f) => node("field", vec![("e", expr(e)), ("name", txt(f))]),
        Expr::Struct(fs) => node(
            "struct",
            vec![("fields", Value::Struct(fs.iter().map(|(k, v)| (k.clone(), expr(v))).collect()))],
        ),
        Expr::List(es) => node("list", vec![("items", list(expr, es))]),
        Expr::Some(e) => node("some", vec![("e", expr(e))]),
        Expr::None(t) => node("none", vec![("ty", ty_value(t))]),
        Expr::Match(e, x, a, b) => node("match", vec![("e", expr(e)), ("sym", int(*x)), ("some", expr(a)), ("none", expr(b))]),
        Expr::If(c, a, b) => node("ife", vec![("c", expr(c)), ("then", expr(a)), ("else", expr(b))]),
        Expr::Op(op, es) => node("op", vec![("op", txt(op.name())), ("args", list(expr, es))]),
        Expr::Cmp(op, a, b) => node("cmp", vec![("op", txt(op.name())), ("l", expr(a)), ("r", expr(b))]),
        Expr::Call(n, es) => node("call", vec![("fn", txt(n)), ("args", list(expr, es))]),
        Expr::Std(f, es) => node("std", vec![("fn", txt(f.show())), ("args", list(expr, es))]),
        Expr::Map(xs, x, b) => node("map", vec![("in", expr(xs)), ("sym", int(*x)), ("body", expr(b))]),
        Expr::Filter(xs, x, b) => node("filter", vec![("in", expr(xs)), ("sym", int(*x)), ("body", expr(b))]),
        Expr::Any(xs, x, b) => node("any", vec![("in", expr(xs)), ("sym", int(*x)), ("body", expr(b))]),
        Expr::All(xs, x, b) => node("all", vec![("in", expr(xs)), ("sym", int(*x)), ("body", expr(b))]),
        Expr::SortBy(xs, x, k) => node("sort_by", vec![("in", expr(xs)), ("sym", int(*x)), ("key", expr(k))]),
        Expr::Fold(xs, z, acc, x, b) => node(
            "fold",
            vec![
                ("in", expr(xs)),
                ("init", expr(z)),
                ("acc", int(*acc)),
                ("sym", int(*x)),
                ("body", expr(b)),
            ],
        ),
        Expr::Select(p) => node("select", vec![("plan", plan(p))]),
        Expr::Get(t, ks) => node("get", vec![("table", txt(t)), ("key", list(expr, ks))]),
        Expr::Exists(t, ks) => node("exists", vec![("table", txt(t)), ("key", list(expr, ks))]),
    }
}

fn dir(d: &Dir) -> Value {
    txt(if *d == Dir::Asc { "asc" } else { "desc" })
}

/// §1.8 A plan: v3's keys — `table`, `filter`, `order`, `limit`, `related`
/// — and the v4 ones only when present, so a v3-shaped plan encodes as it
/// did: `group` (the `by` columns; `table` is the grouped table), `row`,
/// `members`, `lookups`, `having`, `project`.
fn plan(p: &Plan) -> Value {
    let mut fields = vec![
        ("table", txt(p.table())),
        ("filter", p.filter.as_ref().map(pred).unwrap_or(Value::Null)),
        (
            "order",
            list(
                |(k, d)| match k {
                    Key::Column(c) => node("by", vec![("column", txt(c)), ("dir", dir(d))]),
                    Key::Expr(e) => node("by", vec![("expr", expr(e)), ("dir", dir(d))]),
                },
                &p.order,
            ),
        ),
        ("limit", p.limit.map(int).unwrap_or(Value::Null)),
        ("related", list(related, &p.related)),
    ];
    if let Source::Group { by, .. } = &p.source {
        fields.push(("group", list(|c| txt(c), by)));
    }
    if let Some(r) = p.row {
        fields.push(("row", int(r)));
    }
    if let Some(m) = p.members {
        fields.push(("members", int(m)));
    }
    if !p.lookups.is_empty() {
        fields.push(("lookups", list(lookup, &p.lookups)));
    }
    if let Some(h) = &p.having {
        fields.push(("having", expr(h)));
    }
    if let Some(e) = &p.project {
        fields.push(("project", expr(e)));
    }
    node("plan", fields)
}

fn lookup(l: &Lookup) -> Value {
    node(
        "lookup",
        vec![
            ("name", txt(&l.name)),
            ("sym", int(l.sym)),
            ("table", txt(&l.table)),
            ("key", list(expr, &l.key)),
        ],
    )
}

/// A related plan is always the `on` form: `[column, expr]` pairs.
fn related(r: &Related) -> Value {
    node(
        "related",
        vec![
            ("name", txt(&r.name)),
            ("sym", int(r.sym)),
            ("on", list(|(c, e)| Value::List(vec![txt(c), expr(e)]), &r.on)),
            ("plan", plan(&r.plan)),
        ],
    )
}

fn pred(p: &Pred) -> Value {
    match p {
        Pred::Cmp(c, op, e) => node("pcmp", vec![("column", txt(c)), ("op", txt(op.name())), ("e", expr(e))]),
        Pred::In(c, es) => node("pin", vec![("column", txt(c)), ("items", list(expr, es))]),
        Pred::All(ps) => node("pall", vec![("items", list(pred, ps))]),
        Pred::Any(ps) => node("pany", vec![("items", list(pred, ps))]),
        Pred::Not(q) => node("pnot", vec![("e", pred(q))]),
    }
}

/// The names of the helpers a function calls directly — in its checks,
/// its refinements and its body — sorted and without repeats
/// (`Ark.Encode.calls`). Middleware is not a call; see [`reaches`].
pub fn calls(f: &Function) -> Vec<String> {
    let mut acc = std::collections::BTreeSet::new();
    for (_, fd) in &f.input {
        for c in &fd.checks {
            if let Check::Refine(e, _) = c {
                expr_calls(e, &mut acc);
            }
        }
    }
    for (e, _) in &f.refine {
        expr_calls(e, &mut acc);
    }
    for s in &f.body {
        stmt_calls(s, &mut acc);
    }
    if let Some(p) = &f.plan {
        plan_calls(p, &mut acc);
    }
    acc.into_iter().collect()
}

/// Everything a function reaches directly by name: the helpers it calls and
/// the middleware it uses, sorted and without repeats. What its hash
/// depends on and what its closure carries.
pub fn reaches(f: &Function) -> Vec<String> {
    let mut acc: std::collections::BTreeSet<String> = calls(f).into_iter().collect();
    acc.extend(f.uses.iter().cloned());
    acc.into_iter().collect()
}

fn stmt_calls(s: &Stmt, acc: &mut std::collections::BTreeSet<String>) {
    match s {
        Stmt::Let(_, e) | Stmt::Insert(_, e, _) | Stmt::Upsert(_, e, _) | Stmt::Refuse(e) => expr_calls(e, acc),
        Stmt::Update(_, ks, _, e) => {
            ks.iter().for_each(|k| expr_calls(k, acc));
            expr_calls(e, acc);
        }
        Stmt::If(c, a, b) => {
            expr_calls(c, acc);
            a.iter().chain(b.iter()).for_each(|s| stmt_calls(s, acc));
        }
        Stmt::For(_, xs, b) => {
            expr_calls(xs, acc);
            b.iter().for_each(|s| stmt_calls(s, acc));
        }
        Stmt::Delete(_, ks) => ks.iter().for_each(|e| expr_calls(e, acc)),
        Stmt::Return(me) => {
            if let Some(e) = me {
                expr_calls(e, acc)
            }
        }
    }
}

fn expr_calls(e: &Expr, acc: &mut std::collections::BTreeSet<String>) {
    match e {
        Expr::Call(n, es) => {
            acc.insert(n.clone());
            es.iter().for_each(|e| expr_calls(e, acc));
        }
        Expr::Field(e, _) | Expr::Some(e) => expr_calls(e, acc),
        Expr::Struct(fs) => fs.values().for_each(|e| expr_calls(e, acc)),
        Expr::List(es) | Expr::Op(_, es) | Expr::Std(_, es) | Expr::Get(_, es) | Expr::Exists(_, es) => es.iter().for_each(|e| expr_calls(e, acc)),
        Expr::Match(e, _, a, b) | Expr::If(e, a, b) => [e, a, b].iter().for_each(|e| expr_calls(e, acc)),
        Expr::Cmp(_, a, b) => {
            expr_calls(a, acc);
            expr_calls(b, acc);
        }
        Expr::Map(xs, _, b) | Expr::Filter(xs, _, b) | Expr::Any(xs, _, b) | Expr::All(xs, _, b) | Expr::SortBy(xs, _, b) => {
            expr_calls(xs, acc);
            expr_calls(b, acc);
        }
        Expr::Fold(xs, z, _, _, b) => [xs, z, b].iter().for_each(|e| expr_calls(e, acc)),
        Expr::Select(p) => plan_calls(p, acc),
        Expr::Lit(_) | Expr::Arg(_) | Expr::Auto(_) | Expr::Var(_) | Expr::CtxUser | Expr::CtxSession | Expr::Provided(_) | Expr::None(_) => {}
    }
}

fn plan_calls(p: &Plan, acc: &mut std::collections::BTreeSet<String>) {
    if let Some(f) = &p.filter {
        pred_calls(f, acc);
    }
    p.lookups.iter().flat_map(|l| &l.key).for_each(|e| expr_calls(e, acc));
    for r in &p.related {
        r.on.iter().for_each(|(_, e)| expr_calls(e, acc));
        plan_calls(&r.plan, acc);
    }
    p.having.iter().chain(&p.project).for_each(|e| expr_calls(e, acc));
    for (k, _) in &p.order {
        if let Key::Expr(e) = k {
            expr_calls(e, acc);
        }
    }
}

fn pred_calls(p: &Pred, acc: &mut std::collections::BTreeSet<String>) {
    match p {
        Pred::Cmp(_, _, e) => expr_calls(e, acc),
        Pred::In(_, es) => es.iter().for_each(|e| expr_calls(e, acc)),
        Pred::All(ps) | Pred::Any(ps) => ps.iter().for_each(|p| pred_calls(p, acc)),
        Pred::Not(q) => pred_calls(q, acc),
    }
}
