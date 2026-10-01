//! §9 Verification, at spec version 4 (`docs/plan-v4.md` §1.10).
//!
//! What a module must satisfy before anything runs it or hashes it. A
//! builder ([`crate::authoring`]) makes most of these hard to write; the
//! verifier refuses them anyway, because a module may arrive from anywhere.
//! [`verify`] returns the form a function's hash is taken of — orders
//! completed, symbols renumbered — so that the hash is of the program as it
//! will be run.
//!
//! The rules, beyond v1's (a well-formed schema; unique function names; a
//! well-typed body with no option of an option; a read only as the whole
//! right-hand side of a `let` and never in a helper; a helper called only
//! by functions declared after it; every plan's order made total):
//!
//! - routers: names unique, `uses` names middleware; a procedure is on a
//!   router and its `uses` is a subsequence of the router's
//!   (`UsesNotOnRouter`);
//! - middleware: no router, no uses, no autos, input without
//!   checks; every procedure using it has its input fields at its types;
//!   declared before any procedure using it;
//! - any function but a helper may read any table; only a mutator writes;
//!   a refusal anywhere but a helper;
//! - input: checks fit their field's type; `exists` names an id of a table
//!   that exists; refinements are Bool;
//! - `insert`/`upsert` `on` is empty or a declared unique index (`OnNotUnique`);
//! - `provided` names a `Provide` the procedure uses;
//! - v4: a query has a plan and an empty body, and returns a list of its
//!   plan's node type (`QueryWithoutPlan`, `QueryWithBody`); nothing else
//!   has a plan (`PlanOutsideQuery`), and a read in any other body is
//!   v3-shaped — no group, lookup, having, projection or expression order
//!   key, and a related plan only as a schema reference
//!   (`PlanFeatureOutsideQuery`); every node of a plan is typed under its
//!   own binders alone (scope is flat), a having is a `Bool`, a lookup's
//!   key fits its table's key (a part may be an option of it), an `on`
//!   pairs a column with a value of its type (either side may be the
//!   option), an order column is the source's (a group's `by`), and no
//!   expression in a plan reads (`ReadInPlan`) or draws an auto
//!   (`AutoInPlan`); a plan that binds anything has a `row`
//!   (`NoRowBinder`), and only a group has `members` (`MembersWithoutGroup`).

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::normalize::normalize;
use crate::ir::{Auto, Block, Check, Expr, FnKind, Function, Key, Module, Op, Plan, Pred, Source, StdFn, Stmt, Sym, SPEC_VERSION};
use crate::schema::{check_schema, Schema, SchemaError, Table, Ty};
use crate::value::{FieldName, TableName, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    BadSpecVersion(i64),
    BadSchema(SchemaError),
    DuplicateFunction(String),
    DuplicateRouter(String),
    /// Router, then the complaint.
    InRouter(String, RouterComplaint),
    /// Function, then the complaint.
    In(String, Complaint),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for VerifyError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouterComplaint {
    /// A name in `uses` that is no guard or provide.
    NotMiddleware(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Complaint {
    // v1
    AutosOnNonMutator,
    ReturnTypeOnMutator,
    NoReturnType,
    DuplicateName(String),
    UnknownAutoTable(TableName),
    NestedOption,
    /// Where, expected, actual.
    TypeMismatch(String, Ty, Ty),
    NotAStruct(FieldName),
    NoSuchField(FieldName),
    UnknownArg(String),
    UnknownAuto(String),
    UnboundSymbol(Sym),
    ReadNotBound,
    ReadInHelper,
    WriteOutsideMutator,
    RefuseInHelper,
    UnknownTable(TableName),
    UnknownColumn(TableName, FieldName),
    UnknownHelper(String),
    HelperNotYetDeclared(String),
    NotAHelper(String),
    Arity(String, usize, usize),
    BadOp(String),
    MayNotReturn,
    NeedsAnnotation(String),
    BadRelation(TableName, TableName),
    KeyArity(TableName, usize, usize),
    StdMisuse(String),
    // v2
    /// A procedure with no router, or a router that does not exist.
    UnknownRouter(Option<String>),
    /// A router on something that is not a procedure.
    RouterOnNonProcedure,
    /// Uses on something that is not a procedure.
    UsesOnNonProcedure,
    /// A procedure's uses are not a subsequence of its router's.
    UsesNotOnRouter(Vec<String>),
    NotMiddleware(String),
    MiddlewareNotYetDeclared(String),
    /// Middleware, field: a field it reads that this procedure's input
    /// does not have at that type.
    MiddlewareInput(String, String),
    ChecksOnMiddleware,
    /// Field, check: a check that does not fit the field's type.
    CheckOnWrongType(String, String),
    /// Table, columns: an `on` that is not a declared unique index.
    OnNotUnique(TableName, Vec<FieldName>),
    /// `provided` of a middleware the procedure does not use as a provide.
    NotProvided(String),
    /// A value returned from a guard or a mutator, or refinements on
    /// something that takes no input.
    RefineOnNonProcedure,
    // v4
    /// A query with no plan.
    QueryWithoutPlan,
    /// A query whose body is not empty: a query is its plan.
    QueryWithBody,
    /// A plan on something that is not a query.
    PlanOutsideQuery,
    /// A read in a body that is not v3-shaped, naming what it has.
    PlanFeatureOutsideQuery(String),
    /// A `Select`, `Get` or `Exists` inside a plan's expression: a read is
    /// a lookup or a related plan.
    ReadInPlan,
    /// An `Auto` inside a plan: a query draws none.
    AutoInPlan,
    /// A plan that binds something (a lookup, a related plan, a having, a
    /// projection, an expression order key, a group) with no row binder.
    NoRowBinder,
    /// `members` on a plan whose source is not a group.
    MembersWithoutGroup,
}

/// Verify a module. On success, the module as it is to be hashed and run:
/// orders completed and every function normalised.
pub fn verify(m0: &Module) -> Result<Module, Vec<VerifyError>> {
    let m = complete_orders(m0);
    if m.spec != SPEC_VERSION {
        return Err(vec![VerifyError::BadSpecVersion(m.spec)]);
    }
    let schema_errors = check_schema(&m.schema);
    if !schema_errors.is_empty() {
        return Err(schema_errors.into_iter().map(VerifyError::BadSchema).collect());
    }
    let dups = duplicates(m.functions.iter().map(|f| f.name.clone()));
    if !dups.is_empty() {
        return Err(dups.into_iter().map(VerifyError::DuplicateFunction).collect());
    }
    let rdups = duplicates(m.routers.iter().map(|r| r.name.clone()));
    if !rdups.is_empty() {
        return Err(rdups.into_iter().map(VerifyError::DuplicateRouter).collect());
    }
    let mut errs = Vec::new();
    for r in &m.routers {
        for u in &r.uses {
            let ok = m.lookup_function(u).is_some_and(|f| f.kind.is_middleware());
            if !ok {
                errs.push(VerifyError::InRouter(r.name.clone(), RouterComplaint::NotMiddleware(u.clone())));
            }
        }
    }
    for (i, f) in m.functions.iter().enumerate() {
        if let Err(cs) = verify_function(&m, i, f) {
            errs.extend(cs.into_iter().map(|c| VerifyError::In(f.name.clone(), c)));
        }
    }
    if errs.is_empty() {
        Ok(Module {
            functions: m.functions.iter().map(normalize).collect(),
            ..m
        })
    } else {
        Err(errs)
    }
}

fn duplicates(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for n in names {
        if !seen.insert(n.clone()) && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

type Check_<T> = Result<T, Vec<Complaint>>;

fn err<T>(c: Complaint) -> Check_<T> {
    Err(vec![c])
}

fn is_subsequence(xs: &[String], ys: &[String]) -> bool {
    let mut it = ys.iter();
    xs.iter().all(|x| it.any(|y| y == x))
}

/// Verify the `i`th function of a module (its index decides which helpers
/// and middleware it may reach).
pub fn verify_function(m: &Module, i: usize, f: &Function) -> Result<(), Vec<Complaint>> {
    let sch = &m.schema;
    match f.kind {
        FnKind::Mutator | FnKind::Query => {
            let Some(rn) = &f.router else {
                return err(Complaint::UnknownRouter(None));
            };
            let Some(r) = m.lookup_router(rn) else {
                return err(Complaint::UnknownRouter(Some(rn.clone())));
            };
            if !is_subsequence(&f.uses, &r.uses) {
                return err(Complaint::UsesNotOnRouter(f.uses.clone()));
            }
            for u in &f.uses {
                let pos = m.functions.iter().position(|g| g.name == *u);
                match pos.map(|j| (j, &m.functions[j])) {
                    Some((j, g)) if g.kind.is_middleware() => {
                        if j >= i {
                            return err(Complaint::MiddlewareNotYetDeclared(u.clone()));
                        }
                        for (n, fd) in &g.input {
                            let here = f.input.iter().find(|(k, _)| k == n).map(|(_, x)| &x.ty);
                            if here != Some(&fd.ty) {
                                return err(Complaint::MiddlewareInput(u.clone(), n.clone()));
                            }
                        }
                    }
                    _ => return err(Complaint::NotMiddleware(u.clone())),
                }
            }
            match (f.kind, &f.ret) {
                (FnKind::Mutator, Some(_)) => return err(Complaint::ReturnTypeOnMutator),
                (FnKind::Query, None) => return err(Complaint::NoReturnType),
                _ => {}
            }
            if f.kind == FnKind::Query && !f.autos.is_empty() {
                return err(Complaint::AutosOnNonMutator);
            }
            if f.kind == FnKind::Query {
                if f.plan.is_none() {
                    return err(Complaint::QueryWithoutPlan);
                }
                if !f.body.is_empty() {
                    return err(Complaint::QueryWithBody);
                }
            }
        }
        FnKind::Guard | FnKind::Provide => {
            if f.router.is_some() {
                return err(Complaint::RouterOnNonProcedure);
            }
            if !f.uses.is_empty() {
                return err(Complaint::UsesOnNonProcedure);
            }
            if !f.autos.is_empty() {
                return err(Complaint::AutosOnNonMutator);
            }
            if f.input.iter().any(|(_, fd)| !fd.checks.is_empty()) || !f.refine.is_empty() {
                return err(Complaint::ChecksOnMiddleware);
            }
            match (f.kind, &f.ret) {
                (FnKind::Guard, Some(_)) => return err(Complaint::ReturnTypeOnMutator),
                (FnKind::Provide, None) => return err(Complaint::NoReturnType),
                _ => {}
            }
        }
        FnKind::Helper => {
            if f.router.is_some() {
                return err(Complaint::RouterOnNonProcedure);
            }
            if !f.uses.is_empty() {
                return err(Complaint::UsesOnNonProcedure);
            }
            if !f.autos.is_empty() {
                return err(Complaint::AutosOnNonMutator);
            }
            if f.ret.is_none() {
                return err(Complaint::NoReturnType);
            }
            if f.input.iter().any(|(_, fd)| !fd.checks.is_empty()) || !f.refine.is_empty() {
                return err(Complaint::RefineOnNonProcedure);
            }
        }
    }
    let names: Vec<String> = f
        .input
        .iter()
        .map(|(n, _)| n.clone())
        .chain(f.autos.iter().map(|(n, _)| n.clone()))
        .collect();
    if let Some(d) = duplicates(names.into_iter()).into_iter().next() {
        return err(Complaint::DuplicateName(d));
    }
    for (_, fd) in &f.input {
        no_nested_option(&fd.ty)?;
    }
    for (_, a) in &f.autos {
        if let Auto::NewId(t) = a {
            if sch.lookup_table(t).is_none() {
                return err(Complaint::UnknownAutoTable(t.clone()));
            }
        }
    }
    if let Some(t) = &f.ret {
        no_nested_option(t)?;
    }
    let args: BTreeMap<String, Ty> = f.input.iter().map(|(n, fd)| (n.clone(), fd.ty.clone())).collect();
    let provided: BTreeMap<String, Ty> = f
        .uses
        .iter()
        .filter_map(|u| m.lookup_function(u))
        .filter(|g| g.kind == FnKind::Provide)
        .filter_map(|g| g.ret.clone().map(|t| (g.name.clone(), t)))
        .collect();
    let g = G {
        m,
        index: i,
        f,
        kind: f.kind,
        args: &args,
        provided: &provided,
        locals: BTreeMap::new(),
    };
    // The input's checks, each over its own field as it stands.
    for (n, fd) in &f.input {
        let inner = match &fd.ty {
            Ty::Option(t) => (**t).clone(),
            t => t.clone(),
        };
        for c in &fd.checks {
            let fits = match c {
                Check::Trim | Check::MinLen(..) | Check::MaxLen(..) => inner == Ty::Text,
                Check::Range(..) => inner == Ty::Int,
                Check::NonEmpty(_) => matches!(inner, Ty::List(_)),
                Check::Exists(_) => match &inner {
                    Ty::Id(t) => {
                        if sch.lookup_table(t).is_none() {
                            return err(Complaint::UnknownTable(t.clone()));
                        }
                        true
                    }
                    _ => false,
                },
                Check::Refine(e, _) => {
                    let mut own = args.clone();
                    own.insert(n.clone(), inner.clone());
                    let gc = G {
                        args: &own,
                        kind: FnKind::Helper,
                        provided: &BTreeMap::new(),
                        ..g.clone()
                    };
                    expect(&gc, "refine", &Ty::Bool, e)?;
                    true
                }
            };
            if !fits {
                return err(Complaint::CheckOnWrongType(n.clone(), format!("{c:?}")));
            }
        }
    }
    for (e, _) in &f.refine {
        let gc = G {
            kind: FnKind::Helper,
            provided: &BTreeMap::new(),
            ..g.clone()
        };
        expect(&gc, "refine", &Ty::Bool, e)?;
    }
    match (&f.plan, f.kind) {
        (Some(p), FnKind::Query) => {
            let node = query_plan(&g, p)?;
            let got = Ty::List(Box::new(node));
            if !f.ret.as_ref().is_some_and(|r| same(&g.m.schema, r, &got)) {
                let want = f.ret.clone().unwrap_or(Ty::Bool);
                return err(Complaint::TypeMismatch("query result".into(), want, got));
            }
            return Ok(());
        }
        (Some(_), _) => return err(Complaint::PlanOutsideQuery),
        (None, _) => {}
    }
    block(&g, &f.body)?;
    let must_return = matches!(f.kind, FnKind::Query | FnKind::Helper | FnKind::Provide);
    if must_return && !returns(&f.body) {
        return err(Complaint::MayNotReturn);
    }
    Ok(())
}

#[derive(Clone)]
struct G<'a> {
    m: &'a Module,
    index: usize,
    f: &'a Function,
    kind: FnKind,
    args: &'a BTreeMap<String, Ty>,
    provided: &'a BTreeMap<String, Ty>,
    locals: BTreeMap<Sym, Ty>,
}

impl G<'_> {
    fn schema(&self) -> &Schema {
        &self.m.schema
    }

    fn bind(&self, x: Sym, t: Ty) -> Self {
        let mut g = self.clone();
        g.locals.insert(x, t);
        g
    }
}

fn no_nested_option(t: &Ty) -> Check_<()> {
    match t {
        Ty::Option(inner) => match &**inner {
            Ty::Option(_) => err(Complaint::NestedOption),
            other => no_nested_option(other),
        },
        Ty::List(t) => no_nested_option(t),
        Ty::Struct(fs) => fs.values().try_for_each(no_nested_option),
        _ => Ok(()),
    }
}

// A block definitely returns when its last statement does, or is an `if`
// both of whose branches do.
fn returns(b: &Block) -> bool {
    match b.last() {
        Some(Stmt::Return(_)) | Some(Stmt::Refuse(_)) => true,
        Some(Stmt::If(_, a, c)) => returns(a) && returns(c),
        _ => false,
    }
}

fn block(g: &G, b: &Block) -> Check_<()> {
    let mut g2 = g.clone();
    for s in b {
        g2 = stmt(&g2, s)?;
    }
    Ok(())
}

fn stmt<'a>(g: &G<'a>, s: &Stmt) -> Check_<G<'a>> {
    let read_ok = || {
        if g.kind == FnKind::Helper {
            err(Complaint::ReadInHelper)
        } else {
            Ok(())
        }
    };
    let mutating = || {
        if g.kind != FnKind::Mutator {
            err(Complaint::WriteOutsideMutator)
        } else {
            Ok(())
        }
    };
    match s {
        Stmt::Let(x, e) => {
            let t = match e {
                Expr::Select(p) => {
                    read_ok()?;
                    plan_ty(g, p)?
                }
                Expr::Get(t, ks) => {
                    read_ok()?;
                    keyed(g, t, ks)?;
                    Ty::Option(Box::new(table(g, t)?.row_ty()))
                }
                Expr::Exists(t, ks) => {
                    read_ok()?;
                    keyed(g, t, ks)?;
                    Ty::Bool
                }
                _ => infer(g, None, e)?,
            };
            Ok(g.bind(*x, t))
        }
        Stmt::If(c, a, b) => {
            expect(g, "if condition", &Ty::Bool, c)?;
            block(g, a)?;
            block(g, b)?;
            Ok(g.clone())
        }
        Stmt::For(x, xs, body) => {
            let t = elem_of("for", infer(g, None, xs)?)?;
            block(&g.bind(*x, t), body)?;
            Ok(g.clone())
        }
        Stmt::Insert(t, e, on) | Stmt::Upsert(t, e, on) => {
            mutating()?;
            let tbl = table(g, t)?;
            row_fits(g, tbl, e)?;
            if !on.is_empty() {
                let want: BTreeSet<&String> = on.iter().collect();
                let unique = tbl
                    .indexes
                    .iter()
                    .any(|ix| ix.unique && ix.columns.iter().collect::<BTreeSet<_>>() == want && ix.columns.len() == on.len());
                if !unique {
                    return err(Complaint::OnNotUnique(t.clone(), on.clone()));
                }
            }
            Ok(g.clone())
        }
        Stmt::Update(t, ks, x, e) => {
            mutating()?;
            keyed(g, t, ks)?;
            let tbl = table(g, t)?;
            row_fits(&g.bind(*x, tbl.row_ty()), tbl, e)?;
            Ok(g.clone())
        }
        Stmt::Delete(t, ks) => {
            mutating()?;
            keyed(g, t, ks)?;
            Ok(g.clone())
        }
        Stmt::Refuse(e) => {
            if g.kind == FnKind::Helper {
                return err(Complaint::RefuseInHelper);
            }
            expect(g, "refuse", &Ty::Text, e)?;
            Ok(g.clone())
        }
        Stmt::Return(me) => {
            match (&g.f.ret, me) {
                (None, None) => {}
                (None, Some(_)) => return err(Complaint::ReturnTypeOnMutator),
                (Some(_), None) => return err(Complaint::NoReturnType),
                (Some(want), Some(e)) => expect(g, "return", want, e)?,
            }
            Ok(g.clone())
        }
    }
}

// A written row may leave nullable columns out (they are written as None),
// so the struct is checked field by field against the row type: every
// field it has must be a column of the right type, and every non-nullable
// column must be there.
fn row_fits(g: &G, tbl: &Table, e: &Expr) -> Check_<()> {
    let row = tbl.row_ty();
    let got = infer(g, Some(&row), e)?;
    let site = format!("write {}", tbl.name);
    match (&row, &got) {
        (Ty::Struct(want), Ty::Struct(have)) => {
            for (k, ty) in have {
                match want.get(k) {
                    None => return err(Complaint::UnknownColumn(tbl.name.clone(), k.clone())),
                    Some(w) if w != ty => return err(Complaint::TypeMismatch(format!("{site}.{k}"), w.clone(), ty.clone())),
                    _ => {}
                }
            }
            for c in &tbl.columns {
                if !c.nullable && !have.contains_key(&c.name) {
                    return err(Complaint::TypeMismatch(site, row.clone(), got.clone()));
                }
            }
            Ok(())
        }
        _ => err(Complaint::TypeMismatch(site, row.clone(), got.clone())),
    }
}

fn table<'a>(g: &G<'a>, t: &str) -> Check_<&'a Table> {
    g.m.schema.lookup_table(t).map_or_else(|| err(Complaint::UnknownTable(t.into())), Ok)
}

// A key expression list matches the table's key columns in number and type.
fn keyed(g: &G, t: &str, ks: &[Expr]) -> Check_<()> {
    let tbl = table(g, t)?;
    let want = tbl.key_ty();
    if want.len() != ks.len() {
        return err(Complaint::KeyArity(t.into(), want.len(), ks.len()));
    }
    for (w, k) in want.iter().zip(ks) {
        expect(g, &format!("key of {t}"), w, k)?;
    }
    Ok(())
}

/// §9 Two types the same, where a table's row is that table's row whichever
/// schema it was written against (`docs/plan-db.md` D1). There is no row
/// type of its own: a row is `Ty::Struct` of its columns (`Table::row_ty`),
/// and that struct is what a function's `ret`, a provider's result and an
/// `Expr::None` carry and what its hash covers. So a schema that grew a
/// nullable column made every function that names a whole row of the
/// table fail here — the declared struct one field short of the one the
/// plan now produces — though nothing it does had changed, and re-authoring
/// it would have moved its hash. A row is therefore compared as the table's
/// row: two structs that differ only in fields that are nullable columns of
/// one table, carried whole by the wider of the two, are the same type.
/// Anything else is compared exactly, as before.
fn same(sch: &Schema, a: &Ty, b: &Ty) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Ty::Option(x), Ty::Option(y)) | (Ty::List(x), Ty::List(y)) => same(sch, x, y),
        (Ty::Struct(fa), Ty::Struct(fb)) => {
            let (wide, narrow) = if fa.len() >= fb.len() { (fa, fb) } else { (fb, fa) };
            if !narrow.iter().all(|(k, t)| wide.get(k).is_some_and(|w| same(sch, w, t))) {
                return false;
            }
            let extra: Vec<&String> = wide.keys().filter(|k| !narrow.contains_key(*k)).collect();
            if extra.is_empty() {
                return true;
            }
            sch.tables().any(|tbl| {
                tbl.columns.iter().all(|c| wide.get(&c.name) == Some(&c.column_ty()))
                    && extra.iter().all(|k| tbl.column(k).is_some_and(|c| c.nullable))
            })
        }
        _ => false,
    }
}

fn expect(g: &G, site: &str, want: &Ty, e: &Expr) -> Check_<()> {
    let got = infer(g, Some(want), e)?;
    if same(g.schema(), &got, want) {
        Ok(())
    } else {
        err(Complaint::TypeMismatch(site.into(), want.clone(), got))
    }
}

fn elem_of(site: &str, t: Ty) -> Check_<Ty> {
    match t {
        Ty::List(t) => Ok(*t),
        other => err(Complaint::TypeMismatch(site.into(), Ty::List(Box::new(other.clone())), other)),
    }
}

/// §9.1 Typing of expressions. The expected type, when known, is used only
/// where an expression cannot be typed on its own.
fn infer(g: &G, want: Option<&Ty>, e: &Expr) -> Check_<Ty> {
    match e {
        Expr::Lit(v) => lit(want, v),
        Expr::Arg(a) => g.args.get(a).cloned().map_or_else(|| err(Complaint::UnknownArg(a.clone())), Ok),
        Expr::Auto(a) => match g.f.autos.iter().find(|(n, _)| n == a) {
            Some((_, Auto::NewId(t))) if g.kind == FnKind::Mutator => Ok(Ty::Id(t.clone())),
            Some((_, Auto::Now)) if g.kind == FnKind::Mutator => Ok(Ty::Int),
            _ => err(Complaint::UnknownAuto(a.clone())),
        },
        Expr::Var(x) => g.locals.get(x).cloned().map_or_else(|| err(Complaint::UnboundSymbol(*x)), Ok),
        Expr::CtxUser | Expr::CtxSession => Ok(Ty::Text),
        Expr::Provided(n) => g.provided.get(n).cloned().map_or_else(|| err(Complaint::NotProvided(n.clone())), Ok),
        Expr::Field(e, f) => match infer(g, None, e)? {
            Ty::Struct(fs) => fs.get(f).cloned().map_or_else(|| err(Complaint::NoSuchField(f.clone())), Ok),
            _ => err(Complaint::NotAStruct(f.clone())),
        },
        Expr::Struct(fs) => {
            let wf = match want {
                Some(Ty::Struct(ws)) => Some(ws),
                _ => None,
            };
            let mut out = BTreeMap::new();
            for (k, e) in fs {
                out.insert(k.clone(), infer(g, wf.and_then(|w| w.get(k)), e)?);
            }
            Ok(Ty::Struct(out))
        }
        Expr::List(es) => {
            let we = match want {
                Some(Ty::List(t)) => Some(&**t),
                _ => None,
            };
            let mut ts = Vec::with_capacity(es.len());
            for e in es {
                ts.push(infer(g, we, e)?);
            }
            match (ts.first(), we) {
                (None, Some(t)) => Ok(Ty::List(Box::new(t.clone()))),
                (None, None) => err(Complaint::NeedsAnnotation("empty list".into())),
                (Some(t), _) => {
                    for t2 in &ts[1..] {
                        if !same(g.schema(), t2, t) {
                            return err(Complaint::TypeMismatch("list element".into(), t.clone(), t2.clone()));
                        }
                    }
                    Ok(Ty::List(Box::new(t.clone())))
                }
            }
        }
        Expr::Some(e) => {
            let inner = match want {
                Some(Ty::Option(t)) => Some(&**t),
                _ => None,
            };
            match infer(g, inner, e)? {
                Ty::Option(_) => err(Complaint::NestedOption),
                t => Ok(Ty::Option(Box::new(t))),
            }
        }
        Expr::None(t) => {
            let o = Ty::Option(Box::new(t.clone()));
            no_nested_option(&o)?;
            Ok(o)
        }
        Expr::Match(e, x, a, b) => {
            let inner = match infer(g, None, e)? {
                Ty::Option(t) => *t,
                other => return err(Complaint::TypeMismatch("match".into(), Ty::Option(Box::new(other.clone())), other)),
            };
            let ta = infer(&g.bind(*x, inner), want, a)?;
            let tb = infer(g, Some(&ta), b)?;
            if !same(g.schema(), &ta, &tb) {
                return err(Complaint::TypeMismatch("match arms".into(), ta, tb));
            }
            Ok(ta)
        }
        Expr::If(c, a, b) => {
            expect(g, "if", &Ty::Bool, c)?;
            let ta = infer(g, want, a)?;
            let tb = infer(g, Some(&ta), b)?;
            if !same(g.schema(), &ta, &tb) {
                return err(Complaint::TypeMismatch("if arms".into(), ta, tb));
            }
            Ok(ta)
        }
        Expr::Op(op, es) => {
            let all = |t: Ty| -> Check_<Ty> {
                for e in es {
                    expect(g, if t == Ty::Bool { "boolean operator" } else { "arithmetic" }, &t, e)?;
                }
                Ok(t)
            };
            match (op, es.len()) {
                (Op::And | Op::Or, 2) | (Op::Not, 1) => all(Ty::Bool),
                (Op::Neg, 1) | (Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod, 2) => all(Ty::Int),
                _ => err(Complaint::BadOp(op.show().into())),
            }
        }
        Expr::Cmp(_, a, b) => {
            let ta = infer(g, None, a)?;
            expect(g, "comparison", &ta, b)?;
            Ok(Ty::Bool)
        }
        Expr::Call(name, es) => {
            let Some(j) = g.m.functions.iter().position(|f| f.name == *name) else {
                return err(Complaint::UnknownHelper(name.clone()));
            };
            let f = &g.m.functions[j];
            if f.kind != FnKind::Helper {
                return err(Complaint::NotAHelper(name.clone()));
            }
            if j >= g.index {
                return err(Complaint::HelperNotYetDeclared(name.clone()));
            }
            if es.len() != f.input.len() {
                return err(Complaint::Arity(name.clone(), f.input.len(), es.len()));
            }
            for ((_, fd), e) in f.input.iter().zip(es) {
                expect(g, &format!("argument of {name}"), &fd.ty, e)?;
            }
            f.ret.clone().map_or_else(|| err(Complaint::NotAHelper(name.clone())), Ok)
        }
        Expr::Std(f, es) => {
            let mut ts = Vec::with_capacity(es.len());
            for e in es {
                ts.push(infer(g, None, e)?);
            }
            std_ty(*f, &ts, want)
        }
        Expr::Map(xs, x, b) => {
            let t = elem_of("map", infer(g, None, xs)?)?;
            let wu = match want {
                Some(Ty::List(u)) => Some(&**u),
                _ => None,
            };
            let u = infer(&g.bind(*x, t), wu, b)?;
            Ok(Ty::List(Box::new(u)))
        }
        Expr::Filter(xs, x, b) => {
            let t = elem_of("filter", infer(g, None, xs)?)?;
            expect(&g.bind(*x, t.clone()), "filter body", &Ty::Bool, b)?;
            Ok(Ty::List(Box::new(t)))
        }
        Expr::Any(xs, x, b) | Expr::All(xs, x, b) => {
            let t = elem_of("any/all", infer(g, None, xs)?)?;
            expect(&g.bind(*x, t), "any/all body", &Ty::Bool, b)?;
            Ok(Ty::Bool)
        }
        Expr::SortBy(xs, x, k) => {
            let t = elem_of("sort_by", infer(g, None, xs)?)?;
            infer(&g.bind(*x, t.clone()), None, k)?;
            Ok(Ty::List(Box::new(t)))
        }
        Expr::Fold(xs, z, acc, x, b) => {
            let t = elem_of("fold", infer(g, None, xs)?)?;
            let a = infer(g, want, z)?;
            expect(&g.bind(*x, t).bind(*acc, a.clone()), "fold body", &a, b)?;
            Ok(a)
        }
        Expr::Select(_) | Expr::Get(_, _) | Expr::Exists(_, _) => err(Complaint::ReadNotBound),
    }
}

fn lit(want: Option<&Ty>, v: &Value) -> Check_<Ty> {
    match v {
        Value::Null => err(Complaint::NeedsAnnotation("null literal; use None".into())),
        Value::Bool(_) => Ok(Ty::Bool),
        Value::Int(_) => Ok(Ty::Int),
        Value::Text(_) => match want {
            Some(t @ Ty::Enum(_)) => Ok(t.clone()),
            _ => Ok(Ty::Text),
        },
        Value::Bytes(_) => Ok(Ty::Bytes),
        Value::Id(_) => match want {
            Some(t @ Ty::Id(_)) => Ok(t.clone()),
            _ => err(Complaint::NeedsAnnotation("id literal".into())),
        },
        Value::List(_) => err(Complaint::NeedsAnnotation("list literal; use a list expression".into())),
        Value::Struct(_) => err(Complaint::NeedsAnnotation("struct literal; use a struct expression".into())),
    }
}

/// §9.2 The type of a read in a body: the table's columns, plus a list
/// field per relationship read beneath. The plan must be v3-shaped (§1.4):
/// only a query's plan may look up, group, have, project or sort by an
/// expression, and a related plan here is a schema reference.
fn plan_ty(g: &G, p: &Plan) -> Check_<Ty> {
    v3_shaped(g, p)?;
    let t = table(g, p.table())?;
    if let Some(f) = &p.filter {
        pred_ok(g, t, f)?;
    }
    for (k, _) in &p.order {
        if let Key::Column(c) = k {
            if t.column(c).is_none() {
                return err(Complaint::UnknownColumn(t.name.clone(), c.clone()));
            }
        }
    }
    let Ty::Struct(mut fields) = t.row_ty() else {
        unreachable!("a row type is a struct")
    };
    for r in &p.related {
        let ct = plan_ty(g, &r.plan)?;
        fields.insert(r.name.clone(), ct);
    }
    Ok(Ty::List(Box::new(Ty::Struct(fields))))
}

// §1.4 What a read outside a query may be, or the first thing it has that
// it may not.
fn v3_shaped(g: &G, p: &Plan) -> Check_<()> {
    let feature = |what: &str| err(Complaint::PlanFeatureOutsideQuery(what.into()));
    if matches!(p.source, Source::Group { .. }) {
        return feature("a group source");
    }
    if p.members.is_some() {
        return err(Complaint::MembersWithoutGroup);
    }
    if !p.lookups.is_empty() {
        return feature("a lookup");
    }
    if p.having.is_some() {
        return feature("a having");
    }
    if p.project.is_some() {
        return feature("a projection");
    }
    if p.order.iter().any(|(k, _)| matches!(k, Key::Expr(_))) {
        return feature("an expression order key");
    }
    if !p.related.is_empty() && p.row.is_none() {
        return err(Complaint::NoRowBinder);
    }
    for r in &p.related {
        // The one related form a body may read: `child.col == row.key`
        // where `child.col` references this table's single key column.
        let rel = match (&r.on[..], p.row) {
            ([(col, Expr::Field(e, key))], Some(row)) if **e == Expr::Var(row) => g
                .schema()
                .children_of(p.table())
                .into_iter()
                .find(|rel| rel.child == *r.plan.table() && rel.column == *col)
                .filter(|_| g.schema().lookup_table(p.table()).is_some_and(|t| t.key == [key.clone()])),
            _ => None,
        };
        if rel.is_none() {
            return feature("a related plan that is not a reference");
        }
    }
    Ok(())
}

/// §1.10 A query's plan: every node typed under its own binders, the node
/// type as the answer. `g` is the function's scope, which the filter and
/// every child plan are checked in (scope is flat per node).
fn query_plan(g: &G, p: &Plan) -> Check_<Ty> {
    plan_reads_nothing(p)?;
    node_ty(g, p)
}

fn node_ty(g: &G, p: &Plan) -> Check_<Ty> {
    let t = table(g, p.table())?;
    if let Some(f) = &p.filter {
        pred_ok(g, t, f)?;
    }
    let row_ty = match &p.source {
        Source::Table(_) => t.row_ty(),
        Source::Group { by, .. } => {
            let mut fs = BTreeMap::new();
            for c in by {
                let col = t.column(c).map_or_else(|| err(Complaint::UnknownColumn(t.name.clone(), c.clone())), Ok)?;
                fs.insert(c.clone(), col.column_ty());
            }
            Ty::Struct(fs)
        }
    };
    let binds = !p.is_bare();
    if binds && p.row.is_none() {
        return err(Complaint::NoRowBinder);
    }
    if p.members.is_some() && !matches!(p.source, Source::Group { .. }) {
        return err(Complaint::MembersWithoutGroup);
    }
    let mut node = G {
        locals: BTreeMap::new(),
        ..g.clone()
    };
    if let Some(x) = p.row {
        node = node.bind(x, row_ty.clone());
    }
    if let Some(x) = p.members {
        node = node.bind(x, Ty::List(Box::new(t.row_ty())));
    }
    for l in &p.lookups {
        let lt = table(g, &l.table)?;
        let want = lt.key_ty();
        if want.len() != l.key.len() {
            return err(Complaint::KeyArity(l.table.clone(), want.len(), l.key.len()));
        }
        for (w, k) in want.iter().zip(&l.key) {
            let got = infer(&node, Some(w), k)?;
            if got != *w && got != Ty::Option(Box::new(w.clone())) {
                return err(Complaint::TypeMismatch(format!("key of {}", l.table), w.clone(), got));
            }
        }
        node = node.bind(l.sym, Ty::Option(Box::new(lt.row_ty())));
    }
    let Ty::Struct(mut fields) = row_ty.clone() else {
        unreachable!("a row type is a struct")
    };
    for r in &p.related {
        let ct = table(g, r.plan.table())?;
        for (c, e) in &r.on {
            let col = ct
                .column(c)
                .map_or_else(|| err(Complaint::UnknownColumn(ct.name.clone(), c.clone())), Ok)?;
            let want = col.column_ty();
            let got = infer(&node, Some(&want), e)?;
            let fits = got == want || strip(&got) == strip(&want);
            if !fits {
                return err(Complaint::TypeMismatch(format!("on {}.{c}", ct.name), want, got));
            }
        }
        let child = Ty::List(Box::new(node_ty(g, &r.plan)?));
        fields.insert(r.name.clone(), child.clone());
        node = node.bind(r.sym, child);
    }
    if let Some(h) = &p.having {
        expect(&node, "having", &Ty::Bool, h)?;
    }
    for (k, _) in &p.order {
        match k {
            Key::Column(c) => {
                if !matches!(&row_ty, Ty::Struct(fs) if fs.contains_key(c)) {
                    return err(Complaint::UnknownColumn(t.name.clone(), c.clone()));
                }
            }
            Key::Expr(e) => {
                infer(&node, None, e)?;
            }
        }
    }
    match &p.project {
        Some(e) => infer(&node, None, e),
        None => Ok(Ty::Struct(fields)),
    }
}

// A type with one option taken off, for an `on` that joins `Opt<T>` to `T`.
fn strip(t: &Ty) -> &Ty {
    match t {
        Ty::Option(inner) => inner,
        other => other,
    }
}

// §1.3 No expression in a plan reads or draws an auto, at any depth.
fn plan_reads_nothing(p: &Plan) -> Check_<()> {
    let mut es: Vec<&Expr> = vec![];
    if let Some(f) = &p.filter {
        pred_exprs(f, &mut es);
    }
    es.extend(p.lookups.iter().flat_map(|l| &l.key));
    es.extend(p.related.iter().flat_map(|r| r.on.iter().map(|(_, e)| e)));
    es.extend(p.having.iter().chain(&p.project));
    es.extend(p.order.iter().filter_map(|(k, _)| match k {
        Key::Expr(e) => Some(e),
        Key::Column(_) => None,
    }));
    for e in es {
        if let Some(c) = forbidden(e) {
            return err(c);
        }
    }
    p.related.iter().try_for_each(|r| plan_reads_nothing(&r.plan))
}

fn pred_exprs<'a>(p: &'a Pred, out: &mut Vec<&'a Expr>) {
    match p {
        Pred::Cmp(_, _, e) => out.push(e),
        Pred::In(_, es) => out.extend(es),
        Pred::All(ps) | Pred::Any(ps) => ps.iter().for_each(|q| pred_exprs(q, out)),
        Pred::Not(q) => pred_exprs(q, out),
        Pred::Has(_, e) => out.push(e),
    }
}

// The first read or auto inside an expression.
fn forbidden(e: &Expr) -> Option<Complaint> {
    fn any<'a>(mut es: impl Iterator<Item = &'a Expr>) -> Option<Complaint> {
        es.find_map(forbidden)
    }
    match e {
        Expr::Select(_) | Expr::Get(..) | Expr::Exists(..) => Some(Complaint::ReadInPlan),
        Expr::Auto(_) => Some(Complaint::AutoInPlan),
        Expr::Field(e, _) | Expr::Some(e) => forbidden(e),
        Expr::Struct(fs) => any(fs.values()),
        Expr::List(es) | Expr::Op(_, es) | Expr::Call(_, es) | Expr::Std(_, es) => any(es.iter()),
        Expr::Match(a, _, b, c) | Expr::If(a, b, c) => any([&**a, &**b, &**c].into_iter()),
        Expr::Cmp(_, a, b) | Expr::Map(a, _, b) | Expr::Filter(a, _, b) | Expr::Any(a, _, b) | Expr::All(a, _, b) | Expr::SortBy(a, _, b) => {
            any([&**a, &**b].into_iter())
        }
        Expr::Fold(a, b, _, _, c) => any([&**a, &**b, &**c].into_iter()),
        Expr::Lit(_) | Expr::Arg(_) | Expr::Var(_) | Expr::CtxUser | Expr::CtxSession | Expr::Provided(_) | Expr::None(_) => None,
    }
}

fn pred_ok(g: &G, t: &Table, p: &Pred) -> Check_<()> {
    let col = |c: &str| t.column(c).map_or_else(|| err(Complaint::UnknownColumn(t.name.clone(), c.into())), Ok);
    match p {
        Pred::Cmp(c, _, e) => expect(g, &format!("filter on {c}"), &col(c)?.column_ty(), e),
        Pred::In(c, es) => {
            let ty = col(c)?.column_ty();
            es.iter().try_for_each(|e| expect(g, &format!("filter on {c}"), &ty, e))
        }
        Pred::All(ps) | Pred::Any(ps) => ps.iter().try_for_each(|q| pred_ok(g, t, q)),
        Pred::Not(q) => pred_ok(g, t, q),
        // D4 A substring of a text column, the needle text: on any other
        // column it would fold something that is not text.
        Pred::Has(c, e) => {
            let ty = col(c)?.column_ty();
            if !matches!(&ty, Ty::Text) && ty != Ty::Option(Box::new(Ty::Text)) {
                return err(Complaint::TypeMismatch(format!("has on {c}"), Ty::Text, ty));
            }
            expect(g, &format!("has on {c}"), &Ty::Text, e)
        }
    }
}

/// §9.3 Signatures of the standard library.
fn std_ty(f: StdFn, ts: &[Ty], want: Option<&Ty>) -> Check_<Ty> {
    use StdFn::*;
    let option = |t: &Ty| match t {
        Ty::Option(_) => err(Complaint::NestedOption),
        t => Ok(Ty::Option(Box::new(t.clone()))),
    };
    match (f, ts) {
        (Trim | Lower, [Ty::Text]) => Ok(Ty::Text),
        (IsEmpty | IsAlnum, [Ty::Text]) => Ok(Ty::Bool),
        (Concat, [Ty::List(t)]) if **t == Ty::Text => Ok(Ty::Text),
        (Chars, [Ty::Text]) => Ok(Ty::List(Box::new(Ty::Text))),
        (TextLen, [Ty::Text]) => Ok(Ty::Int),
        (StartsWith, [Ty::Text, Ty::Text]) => Ok(Ty::Bool),
        (SplitOnce, [Ty::Text, Ty::Text]) => Ok(Ty::Option(Box::new(Ty::Struct(
            [("before".to_string(), Ty::Text), ("after".to_string(), Ty::Text)].into_iter().collect(),
        )))),
        (TextOfInt, [Ty::Int]) => Ok(Ty::Text),
        (Hex, [Ty::Bytes]) => Ok(Ty::Text),
        (Min | Max, [Ty::Int, Ty::Int]) => Ok(Ty::Int),
        (Clamp, [Ty::Int, Ty::Int, Ty::Int]) => Ok(Ty::Int),
        (Abs, [Ty::Int]) => Ok(Ty::Int),
        (Fnv1a64, [Ty::Text]) => Ok(Ty::Int),
        (Sha256, [Ty::Bytes]) => Ok(Ty::Bytes),
        (IdOfText, [Ty::Text]) => match want {
            Some(t @ Ty::Option(inner)) if matches!(**inner, Ty::Id(_)) => Ok(t.clone()),
            _ => err(Complaint::NeedsAnnotation("id_of_text needs its table from context".into())),
        },
        (TextOfId, [Ty::Id(_)]) => Ok(Ty::Text),
        (NilId, []) => match want {
            Some(t @ Ty::Id(_)) => Ok(t.clone()),
            _ => err(Complaint::NeedsAnnotation("nil id needs its table from context".into())),
        },
        (Utf8, [Ty::Text]) => Ok(Ty::Bytes),
        (First | Last, [Ty::List(t)]) => option(t),
        (Len, [Ty::List(_)]) => Ok(Ty::Int),
        (Contains, [Ty::List(t), t2]) if **t == *t2 => Ok(Ty::Bool),
        (Reverse, [Ty::List(t)]) => Ok(Ty::List(t.clone())),
        (IsSome, [Ty::Option(_)]) => Ok(Ty::Bool),
        (UnwrapOr, [Ty::Option(t), t2]) if **t == *t2 => Ok(t2.clone()),
        (Unwrap, [Ty::Option(t)]) => Ok((**t).clone()),
        _ => err(Complaint::StdMisuse(format!("{} applied to {ts:?}", f.show()))),
    }
}

/// §9.4 Make every order total: append the table's key columns, ascending,
/// after whatever the author ordered by, omitting any already present — in
/// every plan of every function, related plans included.
pub fn complete_orders(m: &Module) -> Module {
    let sch = m.schema.clone();
    let mut out = m.clone();
    for f in &mut out.functions {
        for s in &mut f.body {
            complete_stmt(&sch, s);
        }
        if let Some(p) = &mut f.plan {
            complete_plan(&sch, p);
        }
    }
    out
}

fn complete_stmt(sch: &Schema, s: &mut Stmt) {
    match s {
        Stmt::Let(_, e) | Stmt::Insert(_, e, _) | Stmt::Upsert(_, e, _) | Stmt::Refuse(e) | Stmt::Return(Some(e)) => complete_expr(sch, e),
        Stmt::Update(_, ks, _, e) => {
            ks.iter_mut().for_each(|k| complete_expr(sch, k));
            complete_expr(sch, e);
        }
        Stmt::If(c, a, b) => {
            complete_expr(sch, c);
            a.iter_mut().chain(b.iter_mut()).for_each(|s| complete_stmt(sch, s));
        }
        Stmt::For(_, xs, b) => {
            complete_expr(sch, xs);
            b.iter_mut().for_each(|s| complete_stmt(sch, s));
        }
        Stmt::Delete(_, ks) => ks.iter_mut().for_each(|e| complete_expr(sch, e)),
        Stmt::Return(None) => {}
    }
}

fn complete_expr(sch: &Schema, e: &mut Expr) {
    match e {
        Expr::Select(p) => complete_plan(sch, p),
        Expr::Field(e, _) | Expr::Some(e) => complete_expr(sch, e),
        Expr::Struct(fs) => fs.values_mut().for_each(|e| complete_expr(sch, e)),
        Expr::List(es) | Expr::Op(_, es) | Expr::Call(_, es) | Expr::Std(_, es) | Expr::Get(_, es) | Expr::Exists(_, es) => {
            es.iter_mut().for_each(|e| complete_expr(sch, e))
        }
        Expr::Match(a, _, b, c) | Expr::If(a, b, c) => {
            complete_expr(sch, a);
            complete_expr(sch, b);
            complete_expr(sch, c);
        }
        Expr::Cmp(_, a, b) | Expr::Map(a, _, b) | Expr::Filter(a, _, b) | Expr::Any(a, _, b) | Expr::All(a, _, b) | Expr::SortBy(a, _, b) => {
            complete_expr(sch, a);
            complete_expr(sch, b);
        }
        Expr::Fold(xs, z, _, _, b) => {
            complete_expr(sch, xs);
            complete_expr(sch, z);
            complete_expr(sch, b);
        }
        Expr::Lit(_) | Expr::Arg(_) | Expr::Auto(_) | Expr::Var(_) | Expr::CtxUser | Expr::CtxSession | Expr::Provided(_) | Expr::None(_) => {}
    }
}

fn complete_plan(sch: &Schema, p: &mut Plan) {
    crate::eval::complete_order(sch, p);
}
