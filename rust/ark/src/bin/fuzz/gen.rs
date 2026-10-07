//! Random schemas, and random modules over them: what `arkc fuzz` runs.
//!
//! Built as IR directly rather than through the authoring vocabulary,
//! because the vocabulary is a type system that keeps the author inside
//! what it can say, and the fuzzer's job is to reach what a module from
//! anywhere may say. Every module is then held to [`ark::verify::verify`],
//! which decides what is admissible: a module that does not verify is
//! counted, by the first complaint, and discarded. The generator is typed
//! by construction so that most of what it makes verifies — a fuzzer whose
//! modules mostly fail verification is fuzzing the verifier's first rule.

use std::collections::BTreeMap;

use ark::ir::{
    Auto, Block, Check, CmpOp, Expr, Field, FnKind, Function, Key, Lookup, Module, Op, Plan, Pred, Related, Router, Source, StdFn, Stmt, Sym,
    SPEC_VERSION,
};
use ark::schema::{Column, Dir, Index, Ref, Schema, Table, Ty};
use ark::value::{FieldName, Value};

use super::churn::Rng;

/// Short texts that trim, fold and compare in every interesting way.
pub const TEXTS: [&str; 9] = ["", "a", "b", " a ", "A", "ab", "x y", "\u{fc}", "zz"];

pub const ENUM: [&str; 3] = ["red", "green", "blue"];

/// A random schema: two to four tables, each keyed by an id of its own, a
/// text or an int, or two columns; scalar columns, some nullable; some
/// referencing an earlier table (or, rarely, their own); unique and plain
/// indexes. A column's name says which table it is of — `c10` is the first
/// plain column of `t1` — but for `id`, which is an id of its own table
/// everywhere: the churn generator ties columns of one name together, as a
/// domain's shared names are, and two of one name and two types would have
/// it write a text into an int.
pub fn schema(rng: &mut Rng) -> Schema {
    let n = 2 + rng.below(3);
    let mut tables: Vec<Table> = vec![];
    for i in 0..n {
        let name = format!("t{i}");
        let mut cols: Vec<Column> = vec![];
        let mut refs: Vec<Ref> = vec![];
        // Earlier tables with one key column: what a reference can name.
        let parents: Vec<(String, Ty)> = tables
            .iter()
            .filter(|t| t.key.len() == 1)
            .map(|t| {
                let k = t.column(&t.key[0]).expect("a key column").ty.clone();
                (t.name.clone(), k)
            })
            .collect();
        let key: Vec<FieldName> = match rng.below(8) {
            0..=3 => {
                cols.push(col("id", Ty::Id(name.clone()), false));
                vec!["id".into()]
            }
            4 | 5 => {
                let k = format!("k{i}");
                cols.push(col(&k, if rng.chance(50) { Ty::Text } else { Ty::Int }, false));
                vec![k]
            }
            _ => {
                // Composite: a parent and a position under it, as an item
                // of a playlist is; or two plain columns.
                match rng.pick(&parents).cloned() {
                    Some((p, pk)) if rng.chance(70) => {
                        cols.push(col(&format!("p{i}"), ref_ty(&p, &pk), false));
                        refs.push(Ref {
                            column: format!("p{i}"),
                            table: p,
                        });
                    }
                    _ => cols.push(col(&format!("p{i}"), Ty::Text, false)),
                }
                cols.push(col(&format!("n{i}"), if rng.chance(70) { Ty::Int } else { Ty::Text }, false));
                vec![format!("p{i}"), format!("n{i}")]
            }
        };
        for c in 0..1 + rng.below(4) {
            let ty = match rng.below(10) {
                0..=3 => Ty::Int,
                4..=6 => Ty::Text,
                7 => Ty::Bool,
                8 => Ty::Enum(ENUM.iter().map(|s| s.to_string()).collect()),
                _ => Ty::Bytes,
            };
            cols.push(col(&format!("c{i}{c}"), ty, rng.chance(30)));
        }
        for r in 0..rng.below(3) {
            let target = if rng.chance(8) && key.len() == 1 {
                let k = cols[0].ty.clone();
                Some((name.clone(), k))
            } else {
                rng.pick(&parents).cloned()
            };
            if let Some((p, pk)) = target {
                let c = format!("r{i}{r}");
                let self_ref = p == name;
                cols.push(col(&c, ref_ty(&p, &pk), self_ref || rng.chance(40)));
                refs.push(Ref { column: c, table: p });
            }
        }
        let others: Vec<FieldName> = cols.iter().map(|c| c.name.clone()).filter(|c| !key.contains(c)).collect();
        let mut indexes = vec![];
        for _ in 0..rng.below(3) {
            let mut ix: Vec<FieldName> = vec![];
            for _ in 0..1 + rng.below(2) {
                if let Some(c) = rng.pick(&others).cloned() {
                    if !ix.contains(&c) {
                        ix.push(c);
                    }
                }
            }
            if !ix.is_empty() && !indexes.iter().any(|x: &Index| x.columns == ix) {
                indexes.push(Index {
                    columns: ix,
                    unique: rng.chance(45),
                });
            }
        }
        tables.push(Table::new(name, cols, key, indexes, refs));
    }
    Schema { tables }
}

/// The roles a session may grant a client's login, and its device may
/// believe it holds; a generated module's `holds` guard tests one of them
/// (`docs/plan-guards.md` D1).
pub const ROLES: [&str; 2] = ["r0", "r1"];

/// What a generated `holds` guard refuses with: how a session tells a
/// guard's refusal from every other.
pub const FORBIDDEN: &str = "forbidden";

/// The router whose mutators run `holds`.
pub const GUARDED: &str = "guarded";

fn col(name: &str, ty: Ty, nullable: bool) -> Column {
    Column {
        name: name.into(),
        ty,
        nullable,
    }
}

// The type of a column referencing `p`, whose key is `pk`.
fn ref_ty(p: &str, pk: &Ty) -> Ty {
    match pk {
        Ty::Id(_) => Ty::Id(p.into()),
        other => other.clone(),
    }
}

/// What an expression may name: the input, the autos, the locals by type,
/// the provided values, the helpers declared so far, and whether it is in a
/// mutator (autos, reads) or a plan (neither).
#[derive(Clone, Default)]
struct Env {
    args: Vec<(String, Ty)>,
    autos: Vec<(String, Ty)>,
    vars: Vec<(Sym, Ty)>,
    provided: Vec<(String, Ty)>,
    helpers: Vec<(String, Vec<Ty>, Ty)>,
    user: bool,
}

/// A module over a schema: helpers, middleware, mutators and queries on one
/// router. Not yet verified; [`module`] is.
pub struct ModuleGen<'a> {
    rng: &'a mut Rng,
    sch: &'a Schema,
    sym: Sym,
    helpers: Vec<(String, Vec<Ty>, Ty)>,
}

/// Generate a module over `sch` and verify it: `Ok` the verified module,
/// `Err` the first complaint, for the tally of misses.
pub fn module(rng: &mut Rng, sch: &Schema) -> Result<Module, String> {
    let mut g = ModuleGen {
        rng,
        sch,
        sym: 0,
        helpers: vec![],
    };
    let m = g.build();
    verify_patching_queries(m)
}

// The verifier names a query's result type when it is not the one
// declared: the plan decides it, so the generator writes a placeholder and
// takes the verifier's answer, once per query.
fn verify_patching_queries(mut m: Module) -> Result<Module, String> {
    for _ in 0..m.functions.len() + 1 {
        match ark::verify::verify(&m) {
            Ok(v) => return Ok(v),
            Err(es) => {
                let mut patched = false;
                for e in &es {
                    if let ark::verify::VerifyError::In(f, ark::verify::Complaint::TypeMismatch(site, _, got)) = e {
                        if site == "query result" {
                            if let Some(func) = m.functions.iter_mut().find(|g| g.name == *f) {
                                func.ret = Some(got.clone());
                                patched = true;
                            }
                        }
                    }
                }
                if !patched {
                    return Err(complaint_kind(&es[0]));
                }
            }
        }
    }
    Err("query result types did not settle".into())
}

/// A complaint without its particulars: what the tally counts.
pub fn complaint_kind(e: &ark::verify::VerifyError) -> String {
    let s = format!("{e:?}");
    // `In("f", TypeMismatch("site", …))` → `TypeMismatch site`.
    if let ark::verify::VerifyError::In(_, c) = e {
        let c = format!("{c:?}");
        let head: String = c.chars().take_while(|ch| ch.is_alphanumeric()).collect();
        if head == "TypeMismatch" {
            let site: String = c.split('"').nth(1).unwrap_or("").chars().take_while(|ch| !ch.is_ascii_digit()).collect();
            return format!("TypeMismatch {site}");
        }
        return head;
    }
    s.chars().take_while(|ch| ch.is_alphanumeric()).collect()
}

fn lit_text(s: &str) -> Expr {
    Expr::Lit(Value::text(s))
}

fn int(n: i64) -> Expr {
    Expr::Lit(Value::Int(n))
}

fn arg(n: &str) -> Expr {
    Expr::Arg(n.into())
}

fn var(x: Sym) -> Expr {
    Expr::Var(x)
}

fn field(e: Expr, f: &str) -> Expr {
    Expr::Field(Box::new(e), f.into())
}

fn std1(f: StdFn, a: Expr) -> Expr {
    Expr::Std(f, vec![a])
}

fn cmp(op: CmpOp, a: Expr, b: Expr) -> Expr {
    Expr::Cmp(op, Box::new(a), Box::new(b))
}

fn bare(table: &str) -> Plan {
    Plan {
        source: Source::Table(table.into()),
        filter: None,
        row: None,
        members: None,
        lookups: vec![],
        related: vec![],
        having: None,
        project: None,
        order: vec![],
        limit: None,
    }
}

fn opt_inner(t: &Ty) -> &Ty {
    match t {
        Ty::Option(i) => i,
        other => other,
    }
}

impl ModuleGen<'_> {
    fn fresh(&mut self) -> Sym {
        self.sym += 1;
        self.sym
    }

    fn table(&mut self) -> Table {
        let ts = self.sch.tables.clone();
        self.rng.pick(&ts).cloned().expect("a table")
    }

    fn build(&mut self) -> Module {
        let mut functions: Vec<Function> = vec![];
        // Helpers first: everything after may call them.
        if self.rng.chance(60) {
            functions.push(self.helper_int());
        }
        if self.rng.chance(50) {
            functions.push(self.helper_text());
        }
        // Middleware, before any procedure that uses it.
        let mut mw: Vec<String> = vec![];
        let mut provided: Vec<(String, Ty)> = vec![];
        if self.rng.chance(40) {
            let t = self.table();
            let limit = 3 + self.rng.below(10) as i64;
            let s = self.fresh();
            functions.push(Function {
                name: "full".into(),
                kind: FnKind::Guard,
                router: None,
                uses: vec![],
                autos: vec![],
                input: vec![],
                refine: vec![],
                ret: None,
                body: vec![
                    Stmt::Let(s, Expr::Select(Box::new(bare(&t.name)))),
                    Stmt::If(
                        cmp(CmpOp::Gt, std1(StdFn::Len, var(s)), int(limit)),
                        vec![Stmt::Refuse(lit_text("full"))],
                        vec![],
                    ),
                ],
                plan: None,
                names: BTreeMap::new(),
            });
            mw.push("full".into());
        }
        if self.rng.chance(40) {
            let t = self.table();
            let s = self.fresh();
            functions.push(Function {
                name: "count".into(),
                kind: FnKind::Provide,
                router: None,
                uses: vec![],
                autos: vec![],
                input: vec![],
                refine: vec![],
                ret: Some(Ty::Int),
                body: vec![
                    Stmt::Let(s, Expr::Select(Box::new(bare(&t.name)))),
                    Stmt::Return(Some(std1(StdFn::Len, var(s)))),
                ],
                plan: None,
                names: BTreeMap::new(),
            });
            mw.push("count".into());
            provided.push(("count".into(), Ty::Int));
        }
        if self.rng.chance(25) {
            functions.push(Function {
                name: "who".into(),
                kind: FnKind::Provide,
                router: None,
                uses: vec![],
                autos: vec![],
                input: vec![],
                refine: vec![],
                ret: Some(Ty::Text),
                body: vec![Stmt::Return(Some(std1(StdFn::Concat, Expr::List(vec![Expr::CtxUser, lit_text("!")]))))],
                plan: None,
                names: BTreeMap::new(),
            });
            mw.push("who".into());
            provided.push(("who".into(), Ty::Text));
        }
        // `docs/plan-guards.md` D1: a second router whose one guard tests a
        // role — that the author holds it, or one time in four that they do
        // not — so that some writes are forbidden to some logins, and a
        // device that believes wrongly meets the authority's stamp.
        let guarded = self.rng.chance(60);
        if guarded {
            let role = Expr::HasRole(ROLES[self.rng.below(ROLES.len())].into());
            let refuse_when = if self.rng.chance(25) { role } else { Expr::Op(Op::Not, vec![role]) };
            functions.push(Function {
                name: "holds".into(),
                kind: FnKind::Guard,
                router: None,
                uses: vec![],
                autos: vec![],
                input: vec![],
                refine: vec![],
                ret: None,
                body: vec![Stmt::If(refuse_when, vec![Stmt::Refuse(lit_text(FORBIDDEN))], vec![])],
                plan: None,
                names: BTreeMap::new(),
            });
        }
        let mut names: Vec<String> = vec![];
        let n_mut = 3 + self.rng.below(6);
        for k in 0..n_mut {
            let on_guarded = guarded && self.rng.chance(40);
            let uses: Vec<String> = if on_guarded {
                vec!["holds".into()]
            } else {
                mw.iter().filter(|_| self.rng.chance(40)).cloned().collect()
            };
            let prov: Vec<(String, Ty)> = provided.iter().filter(|(n, _)| uses.contains(n)).cloned().collect();
            let mut f = self.mutator(k, &prov);
            if names.contains(&f.name) {
                f.name = format!("{}_{k}", f.name);
            }
            names.push(f.name.clone());
            f.uses = uses;
            if on_guarded {
                f.router = Some(GUARDED.into());
            }
            functions.push(f);
        }
        let n_q = 1 + self.rng.below(4);
        for k in 0..n_q {
            let uses: Vec<String> = mw.iter().filter(|_| self.rng.chance(20)).cloned().collect();
            let mut f = self.query(k);
            f.uses = uses;
            functions.push(f);
        }
        Module {
            spec: SPEC_VERSION,
            schema: self.sch.clone(),
            functions,
            routers: [
                Router {
                    name: "api".into(),
                    uses: mw,
                },
                Router {
                    name: GUARDED.into(),
                    uses: vec!["holds".into()],
                },
            ]
            .into_iter()
            .filter(|r| r.name == "api" || guarded)
            .collect(),
            live: vec![],
        }
    }

    fn helper_int(&mut self) -> Function {
        let body = match self.rng.below(4) {
            0 => Expr::Std(StdFn::Clamp, vec![Expr::Op(Op::Add, vec![arg("a"), arg("b")]), int(-1000), int(1000)]),
            1 => Expr::Std(StdFn::Max, vec![arg("a"), Expr::Op(Op::Mul, vec![arg("b"), int(2)])]),
            2 => Expr::Op(Op::Mod, vec![arg("a"), arg("b")]),
            _ => Expr::If(
                Box::new(cmp(CmpOp::Lt, arg("a"), arg("b"))),
                Box::new(arg("b")),
                Box::new(Expr::Op(Op::Sub, vec![arg("a"), int(1)])),
            ),
        };
        self.helpers.push(("hi".into(), vec![Ty::Int, Ty::Int], Ty::Int));
        Function {
            name: "hi".into(),
            kind: FnKind::Helper,
            router: None,
            uses: vec![],
            autos: vec![],
            input: vec![("a".into(), Field::plain(Ty::Int)), ("b".into(), Field::plain(Ty::Int))],
            refine: vec![],
            ret: Some(Ty::Int),
            body: vec![Stmt::Return(Some(body))],
            plan: None,
            names: BTreeMap::new(),
        }
    }

    fn helper_text(&mut self) -> Function {
        let body = match self.rng.below(3) {
            0 => std1(
                StdFn::Concat,
                Expr::List(vec![std1(StdFn::Lower, std1(StdFn::Trim, arg("s"))), lit_text("-")]),
            ),
            1 => {
                let x = self.fresh();
                Expr::Match(
                    Box::new(Expr::Std(StdFn::SplitOnce, vec![arg("s"), lit_text(" ")])),
                    x,
                    Box::new(field(var(x), "after")),
                    Box::new(arg("s")),
                )
            }
            _ => std1(StdFn::TextOfInt, std1(StdFn::TextLen, arg("s"))),
        };
        self.helpers.push(("ht".into(), vec![Ty::Text], Ty::Text));
        Function {
            name: "ht".into(),
            kind: FnKind::Helper,
            router: None,
            uses: vec![],
            autos: vec![],
            input: vec![("s".into(), Field::plain(Ty::Text))],
            refine: vec![],
            ret: Some(Ty::Text),
            body: vec![Stmt::Return(Some(body))],
            plan: None,
            names: BTreeMap::new(),
        }
    }

    // ------------------------------------------------------------------
    // Values of a type

    /// A literal of `ty`. Ids and enum names need their type from context,
    /// which every caller of this one gives.
    fn lit(&mut self, ty: &Ty) -> Expr {
        match ty {
            Ty::Bool => Expr::Lit(Value::Bool(self.rng.chance(50))),
            Ty::Int => int(self.rng.below(10) as i64 - 2),
            Ty::Text => lit_text(TEXTS[self.rng.below(TEXTS.len())]),
            Ty::Bytes => Expr::Lit(Value::Bytes((0..self.rng.below(3)).map(|i| i as u8).collect())),
            Ty::Id(_) => {
                let mut id = [0u8; 16];
                id[15] = 1 + self.rng.below(3) as u8;
                Expr::Lit(Value::Id(id))
            }
            Ty::Enum(vs) => lit_text(&vs[self.rng.below(vs.len())]),
            Ty::Option(t) => {
                if self.rng.chance(40) {
                    Expr::None((**t).clone())
                } else {
                    let inner = self.lit(t);
                    Expr::Some(Box::new(inner))
                }
            }
            Ty::List(t) => Expr::List(vec![self.lit(t)]),
            Ty::Struct(fs) => {
                let fs = fs.clone();
                Expr::Struct(fs.iter().map(|(k, t)| (k.clone(), self.lit(t))).collect())
            }
        }
    }

    // Everything in `env` of type `ty`, as expressions: arguments, autos,
    // locals, a local's fields, provided values.
    fn named(&self, env: &Env, ty: &Ty) -> Vec<Expr> {
        let mut out = vec![];
        for (n, t) in &env.args {
            if t == ty {
                out.push(arg(n));
            }
        }
        for (n, t) in &env.autos {
            if t == ty {
                out.push(Expr::Auto(n.clone()));
            }
        }
        for (x, t) in &env.vars {
            if t == ty {
                out.push(var(*x));
            }
            if let Ty::Struct(fs) = t {
                for (f, ft) in fs {
                    if ft == ty {
                        out.push(field(var(*x), f));
                    }
                }
            }
        }
        for (n, t) in &env.provided {
            if t == ty {
                out.push(Expr::Provided(n.clone()));
            }
        }
        if *ty == Ty::Text && env.user {
            out.push(Expr::CtxUser);
        }
        out
    }

    /// An expression of `ty` in `env`. `known` says whether the context
    /// hands the expression its type (a struct field of a row, an argument
    /// of a helper): only then may it be a literal that needs one.
    fn expr(&mut self, ty: &Ty, env: &Env, depth: u32, known: bool) -> Expr {
        let named = self.named(env, ty);
        if !named.is_empty() && self.rng.chance(55) {
            return self.rng.pick(&named).cloned().expect("named");
        }
        let needs_type = matches!(ty, Ty::Id(_) | Ty::Enum(_));
        if depth == 0 || self.rng.chance(30) {
            if !needs_type || known {
                return self.lit(ty);
            }
            if let Some(e) = self.rng.pick(&named).cloned() {
                return e;
            }
        }
        let d = depth.saturating_sub(1);
        match ty {
            Ty::Int => match self.rng.below(9) {
                0 => Expr::Op(Op::Add, vec![self.expr(ty, env, d, true), self.expr(ty, env, d, true)]),
                1 => Expr::Op(Op::Sub, vec![self.expr(ty, env, d, true), int(1)]),
                2 => Expr::Op(Op::Mul, vec![self.expr(ty, env, d, true), self.expr(ty, env, d, true)]),
                3 => Expr::Std(StdFn::Clamp, vec![self.expr(ty, env, d, false), int(-5), int(50)]),
                4 => std1(StdFn::TextLen, self.expr(&Ty::Text, env, d, false)),
                5 => match self.helpers.iter().find(|(_, _, r)| *r == Ty::Int).cloned() {
                    Some((n, ps, _)) => Expr::Call(n, ps.iter().map(|p| self.expr(p, env, d, true)).collect()),
                    None => Expr::Std(StdFn::Abs, vec![self.expr(ty, env, d, false)]),
                },
                6 => {
                    let c = self.expr(&Ty::Bool, env, d, false);
                    Expr::If(Box::new(c), Box::new(self.expr(ty, env, d, true)), Box::new(self.expr(ty, env, d, true)))
                }
                7 => Expr::Op(Op::Div, vec![self.expr(ty, env, d, true), self.expr(ty, env, d, true)]),
                _ => match self.opt_named(env, ty) {
                    Some(o) => Expr::Std(StdFn::UnwrapOr, vec![o, int(0)]),
                    None => Expr::Op(Op::Neg, vec![self.expr(ty, env, d, true)]),
                },
            },
            Ty::Text => match self.rng.below(6) {
                0 => std1(StdFn::Trim, self.expr(ty, env, d, false)),
                1 => std1(StdFn::Lower, self.expr(ty, env, d, false)),
                2 => std1(StdFn::Concat, Expr::List(vec![self.expr(ty, env, d, true), self.expr(ty, env, d, true)])),
                3 => std1(StdFn::TextOfInt, self.expr(&Ty::Int, env, d, false)),
                4 => match self.helpers.iter().find(|(_, _, r)| *r == Ty::Text).cloned() {
                    Some((n, ps, _)) => Expr::Call(n, ps.iter().map(|p| self.expr(p, env, d, true)).collect()),
                    None => self.lit(ty),
                },
                _ => match self.opt_named(env, ty) {
                    Some(o) => Expr::Std(StdFn::UnwrapOr, vec![o, lit_text("none")]),
                    None => self.lit(ty),
                },
            },
            Ty::Bool => match self.rng.below(6) {
                0 => cmp(self.cmp_op(), self.expr(&Ty::Int, env, d, false), self.expr(&Ty::Int, env, d, true)),
                1 => cmp(self.cmp_op(), self.expr(&Ty::Text, env, d, false), self.expr(&Ty::Text, env, d, true)),
                2 => Expr::Op(Op::And, vec![self.expr(ty, env, d, true), self.expr(ty, env, d, true)]),
                3 => Expr::Op(Op::Not, vec![self.expr(ty, env, d, true)]),
                4 => std1(StdFn::IsEmpty, self.expr(&Ty::Text, env, d, false)),
                _ => Expr::Std(
                    StdFn::StartsWith,
                    vec![self.expr(&Ty::Text, env, d, false), self.expr(&Ty::Text, env, d, false)],
                ),
            },
            Ty::Bytes => match self.rng.below(3) {
                0 => std1(StdFn::Utf8, self.expr(&Ty::Text, env, d, false)),
                1 => std1(StdFn::Sha256, self.expr(ty, env, d, false)),
                _ => self.lit(ty),
            },
            Ty::Option(t) => match self.rng.below(3) {
                0 => Expr::None((**t).clone()),
                _ => {
                    let inner = self.expr(t, env, d, known);
                    Expr::Some(Box::new(inner))
                }
            },
            // An id or an enum name from nowhere: a literal, typed by the
            // `if` it is an arm of when the context does not type it.
            _ => {
                if known {
                    self.lit(ty)
                } else {
                    let a = self.rng.pick(&named).cloned();
                    match a {
                        Some(a) => {
                            let c = self.expr(&Ty::Bool, env, d, false);
                            let b = self.lit(ty);
                            Expr::If(Box::new(c), Box::new(a), Box::new(b))
                        }
                        None => self.lit(ty),
                    }
                }
            }
        }
    }

    // Something of `Option<ty>` in reach, if anything is.
    fn opt_named(&mut self, env: &Env, ty: &Ty) -> Option<Expr> {
        let named = self.named(env, &Ty::Option(Box::new(ty.clone())));
        self.rng.pick(&named).cloned()
    }

    fn cmp_op(&mut self) -> CmpOp {
        [CmpOp::Eq, CmpOp::Ne, CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge][self.rng.below(6)]
    }

    // ------------------------------------------------------------------
    // Input

    /// An input field for a column, at the column's type: checks that fit
    /// it, now and then.
    fn field_for(&mut self, tbl: &Table, c: &Column) -> Field {
        let ty = c.column_ty();
        let mut checks = vec![];
        match &c.ty {
            Ty::Text => {
                if self.rng.chance(30) {
                    checks.push(Check::Trim);
                }
                if self.rng.chance(30) {
                    checks.push(Check::MinLen(1, None));
                }
                if self.rng.chance(10) {
                    checks.push(Check::MaxLen(3, Some("too long".into())));
                }
            }
            Ty::Int => {
                if self.rng.chance(25) {
                    checks.push(Check::Range(Some(-1), if self.rng.chance(50) { Some(20) } else { None }, None));
                }
            }
            Ty::Id(t) if (tbl.refs.iter().any(|r| r.column == c.name) || *t != tbl.name) && self.rng.chance(40) => {
                checks.push(Check::Exists(None));
            }
            _ => {}
        }
        if self.rng.chance(5) {
            let x = Expr::Arg(c.name.clone());
            if *opt_inner(&ty) == Ty::Int {
                checks.push(Check::Refine(cmp(CmpOp::Ne, x, int(3)), Some("not three".into())));
            }
        }
        Field { ty, checks }
    }

    // ------------------------------------------------------------------
    // Mutators

    fn mutator(&mut self, k: usize, provided: &[(String, Ty)]) -> Function {
        self.sym = 0;
        let t = self.table();
        let mut env = Env {
            helpers: self.helpers.clone(),
            provided: provided.to_vec(),
            user: true,
            ..Env::default()
        };
        let mut f = Function {
            name: String::new(),
            kind: FnKind::Mutator,
            router: Some("api".into()),
            uses: vec![],
            autos: vec![],
            input: vec![],
            refine: vec![],
            ret: None,
            body: vec![],
            plan: None,
            names: BTreeMap::new(),
        };
        let kind = self.rng.below(12);
        let (name, body) = match kind {
            0..=2 => ("insert", self.write_row(&t, &mut f, &mut env, false)),
            3 => ("upsert", self.write_row(&t, &mut f, &mut env, true)),
            4 | 5 => ("update", self.update(&t, &mut f, &mut env)),
            6 => ("delete", self.delete(&t, &mut f, &mut env)),
            7 | 8 => ("append", self.append(&t, &mut f, &mut env)),
            9 => ("bulk", self.bulk(&t, &mut f, &mut env)),
            _ => ("touch", self.touch(&t, &mut f, &mut env)),
        };
        f.name = format!("{name}_{}_{k}", t.name);
        let mut guard = vec![];
        if self.rng.chance(20) {
            if let Some((n, _)) = f.input.iter().find(|(_, fd)| fd.ty == Ty::Int).cloned() {
                guard.push(Stmt::If(
                    cmp(CmpOp::Lt, arg(&n), int(0)),
                    vec![Stmt::Refuse(lit_text("negative"))],
                    vec![],
                ));
            }
        }
        if self.rng.chance(5) {
            guard.push(Stmt::If(
                cmp(CmpOp::Eq, Expr::CtxUser, lit_text("peer-1")),
                vec![Stmt::Refuse(lit_text("not peer-1"))],
                vec![],
            ));
        }
        if self.rng.chance(5) && !f.input.is_empty() {
            // A refinement sees the input and nothing a body binds.
            let whole = Env {
                args: env.args.clone(),
                helpers: env.helpers.clone(),
                user: true,
                ..Env::default()
            };
            let e = self.expr(&Ty::Bool, &whole, 2, false);
            f.refine.push((e, None));
        }
        guard.extend(body);
        f.body = guard;
        f
    }

    // The input fields for a table's key: an auto for an id of its own, a
    // field each otherwise. The key expressions, in key order.
    fn key_input(&mut self, t: &Table, f: &mut Function, env: &mut Env, auto: bool) -> Vec<Expr> {
        let mut ks = vec![];
        for k in &t.key {
            let c = t.column(k).expect("a key column").clone();
            if auto && c.ty == Ty::Id(t.name.clone()) && !t.refs.iter().any(|r| r.column == c.name) {
                f.autos.push((k.clone(), Auto::NewId(t.name.clone())));
                env.autos.push((k.clone(), c.ty.clone()));
                ks.push(Expr::Auto(k.clone()));
            } else {
                let fd = Field::plain(c.column_ty());
                env.args.push((k.clone(), fd.ty.clone()));
                f.input.push((k.clone(), fd));
                ks.push(arg(k));
            }
        }
        ks
    }

    // A row of `t` written whole: the key from autos or input, the rest
    // from input or computed.
    fn row_struct(&mut self, t: &Table, f: &mut Function, env: &mut Env, keys: &[Expr]) -> Expr {
        let mut fields: BTreeMap<FieldName, Expr> = BTreeMap::new();
        for (k, e) in t.key.iter().zip(keys) {
            fields.insert(k.clone(), e.clone());
        }
        if self.rng.chance(20) {
            if let Some(c) = t.columns.iter().find(|c| c.ty == Ty::Int && !c.nullable && !t.key.contains(&c.name)) {
                f.autos.push(("now".into(), Auto::Now));
                env.autos.push(("now".into(), Ty::Int));
                fields.insert(c.name.clone(), Expr::Auto("now".into()));
            }
        }
        let cols: Vec<Column> = t.columns.iter().filter(|c| !t.key.contains(&c.name)).cloned().collect();
        for c in &cols {
            if fields.contains_key(&c.name) {
                continue;
            }
            if f.input.iter().any(|(n, _)| *n == c.name) {
                fields.insert(c.name.clone(), arg(&c.name));
                continue;
            }
            if c.nullable && self.rng.chance(15) {
                continue; // left out: written as None
            }
            let e = if self.rng.chance(80) {
                let fd = self.field_for(t, c);
                env.args.push((c.name.clone(), fd.ty.clone()));
                f.input.push((c.name.clone(), fd));
                if self.rng.chance(70) {
                    arg(&c.name)
                } else {
                    let ty = c.column_ty();
                    self.expr(&ty, env, 2, true)
                }
            } else {
                let ty = c.column_ty();
                self.expr(&ty, env, 2, true)
            };
            fields.insert(c.name.clone(), e);
        }
        Expr::Struct(fields)
    }

    fn unique_on(&mut self, t: &Table) -> Vec<FieldName> {
        let uniques: Vec<Vec<FieldName>> = t.indexes.iter().filter(|i| i.unique).map(|i| i.columns.clone()).collect();
        if self.rng.chance(60) {
            self.rng.pick(&uniques).cloned().unwrap_or_default()
        } else {
            vec![]
        }
    }

    fn write_row(&mut self, t: &Table, f: &mut Function, env: &mut Env, upsert: bool) -> Block {
        let keys = self.key_input(t, f, env, true);
        let mut body = vec![];
        // A parent checked by hand, now and then, before the store would.
        if self.rng.chance(25) {
            if let Some(r) = t
                .refs
                .iter()
                .find(|r| t.key.contains(&r.column) || !t.column(&r.column).is_some_and(|c| c.nullable))
                .cloned()
            {
                if f.input.iter().any(|(n, _)| *n == r.column) || t.key.contains(&r.column) {
                    let x = self.fresh();
                    body.push(Stmt::Let(x, Expr::Get(r.table.clone(), vec![arg(&r.column)])));
                    body.push(Stmt::If(
                        Expr::Op(Op::Not, vec![std1(StdFn::IsSome, var(x))]),
                        vec![Stmt::Refuse(lit_text("no parent"))],
                        vec![],
                    ));
                }
            }
        }
        if self.rng.chance(15) {
            let x = self.fresh();
            body.push(Stmt::Let(x, Expr::Exists(t.name.clone(), keys.clone())));
            body.push(Stmt::If(var(x), vec![Stmt::Refuse(lit_text("exists"))], vec![]));
        }
        let row = self.row_struct(t, f, env, &keys);
        let on = self.unique_on(t);
        body.push(if upsert {
            Stmt::Upsert(t.name.clone(), row, on)
        } else {
            Stmt::Insert(t.name.clone(), row, on)
        });
        body
    }

    fn update(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Block {
        let keys = self.key_input(t, f, env, false);
        let x = self.fresh();
        let mut inner = env.clone();
        inner.vars.push((x, t.row_ty()));
        let mut fields: BTreeMap<FieldName, Expr> = BTreeMap::new();
        let others: Vec<Column> = t.columns.iter().filter(|c| !t.key.contains(&c.name)).cloned().collect();
        let changing: Vec<String> = others.iter().filter(|_| self.rng.chance(50)).map(|c| c.name.clone()).collect();
        for c in &t.columns {
            let e = if changing.contains(&c.name) {
                let ty = c.column_ty();
                if self.rng.chance(60) {
                    let n = format!("new_{}", c.name);
                    let fd = Field::plain(ty.clone());
                    inner.args.push((n.clone(), ty.clone()));
                    env.args.push((n.clone(), ty.clone()));
                    f.input.push((n.clone(), fd));
                    arg(&n)
                } else if c.ty == Ty::Int && !c.nullable {
                    Expr::Op(Op::Add, vec![field(var(x), &c.name), int(1)])
                } else {
                    self.expr(&ty, &inner, 2, true)
                }
            } else if c.nullable && self.rng.chance(10) {
                continue;
            } else {
                field(var(x), &c.name)
            };
            fields.insert(c.name.clone(), e);
        }
        let mut body = vec![];
        if self.rng.chance(30) {
            let g = self.fresh();
            body.push(Stmt::Let(g, Expr::Get(t.name.clone(), keys.clone())));
            body.push(Stmt::If(
                std1(StdFn::IsSome, var(g)),
                vec![Stmt::Update(t.name.clone(), keys, x, Expr::Struct(fields))],
                vec![Stmt::Refuse(lit_text("missing"))],
            ));
        } else {
            body.push(Stmt::Update(t.name.clone(), keys, x, Expr::Struct(fields)));
        }
        body
    }

    fn delete(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Block {
        let keys = self.key_input(t, f, env, false);
        let mut body = vec![];
        // Children first, now and then: a cascade by hand.
        if t.key.len() == 1 && self.rng.chance(50) {
            for rel in self.sch.children_of(&t.name) {
                let Some(child) = self.sch.lookup_table(&rel.child).cloned() else {
                    continue;
                };
                if child.name == t.name {
                    continue;
                }
                let s = self.fresh();
                let x = self.fresh();
                let col = child.column(&rel.column).expect("a reference column");
                let rhs = if col.nullable {
                    Expr::Some(Box::new(keys[0].clone()))
                } else {
                    keys[0].clone()
                };
                let mut p = bare(&child.name);
                p.filter = Some(Pred::Cmp(rel.column.clone(), CmpOp::Eq, rhs));
                body.push(Stmt::Let(s, Expr::Select(Box::new(p))));
                let ks = child.key.iter().map(|k| field(var(x), k)).collect();
                body.push(Stmt::For(x, var(s), vec![Stmt::Delete(child.name.clone(), ks)]));
            }
        }
        body.push(Stmt::Delete(t.name.clone(), keys));
        body
    }

    // A row at the end of a list: its position one past the last under the
    // same value of a column, read in the body — the shape whose answer a
    // rebase changes.
    fn append(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Block {
        let pos = t
            .columns
            .iter()
            .find(|c| c.ty == Ty::Int && !c.nullable && !t.key.contains(&c.name))
            .cloned();
        let Some(pos) = pos else { return self.write_row(t, f, env, false) };
        let by = t
            .columns
            .iter()
            .find(|c| c.name != pos.name && !t.key.contains(&c.name) && !matches!(c.ty, Ty::Bool | Ty::Bytes))
            .cloned();
        let keys = self.key_input(t, f, env, true);
        let s = self.fresh();
        let r = self.fresh();
        let mut p = bare(&t.name);
        if let Some(by) = &by {
            let fd = Field::plain(by.column_ty());
            env.args.push((by.name.clone(), fd.ty.clone()));
            f.input.push((by.name.clone(), fd));
            p.filter = Some(Pred::Cmp(by.name.clone(), CmpOp::Eq, arg(&by.name)));
        }
        p.order = vec![(Key::Column(pos.name.clone()), Dir::Desc)];
        p.limit = Some(1);
        let next = Expr::Match(
            Box::new(std1(StdFn::First, var(s))),
            r,
            Box::new(Expr::Op(Op::Add, vec![field(var(r), &pos.name), int(1)])),
            Box::new(int(1)),
        );
        let mut row = self.row_struct(t, f, env, &keys);
        if let Expr::Struct(fs) = &mut row {
            fs.insert(pos.name.clone(), next);
            if let Some(by) = &by {
                fs.insert(by.name.clone(), arg(&by.name));
            }
        }
        // `row_struct` may have asked for the position or the column as
        // input too; a field the body no longer reads is still input.
        let on = self.unique_on(t);
        vec![Stmt::Let(s, Expr::Select(Box::new(p))), Stmt::Insert(t.name.clone(), row, on)]
    }

    // Every row a filter admits, each changed or deleted.
    fn bulk(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Block {
        let s = self.fresh();
        let x = self.fresh();
        let y = self.fresh();
        let mut p = bare(&t.name);
        let filt: Vec<Column> = t.columns.iter().filter(|c| !matches!(c.ty, Ty::Bytes)).cloned().collect();
        if let Some(c) = self.rng.pick(&filt).cloned() {
            let fd = Field::plain(c.column_ty());
            let n = format!("w_{}", c.name);
            env.args.push((n.clone(), fd.ty.clone()));
            f.input.push((n.clone(), fd));
            let op = self.cmp_op();
            p.filter = Some(Pred::Cmp(c.name.clone(), op, arg(&n)));
        }
        if self.rng.chance(40) {
            p.limit = Some(1 + self.rng.below(3) as i64);
            let oc = self.rng.pick(&t.columns).cloned().expect("a column");
            p.order = vec![(Key::Column(oc.name), if self.rng.chance(50) { Dir::Asc } else { Dir::Desc })];
        }
        let ks: Vec<Expr> = t.key.iter().map(|k| field(var(x), k)).collect();
        let each = if self.rng.chance(35) {
            Stmt::Delete(t.name.clone(), ks)
        } else {
            let mut inner = env.clone();
            inner.vars.push((y, t.row_ty()));
            let mut fields: BTreeMap<FieldName, Expr> = BTreeMap::new();
            for c in &t.columns {
                let e = if !t.key.contains(&c.name) && self.rng.chance(40) {
                    if c.ty == Ty::Int && !c.nullable {
                        Expr::Op(Op::Add, vec![field(var(y), &c.name), int(1)])
                    } else {
                        let ty = c.column_ty();
                        self.expr(&ty, &inner, 1, true)
                    }
                } else {
                    field(var(y), &c.name)
                };
                fields.insert(c.name.clone(), e);
            }
            Stmt::Update(t.name.clone(), ks, y, Expr::Struct(fields))
        };
        vec![Stmt::Let(s, Expr::Select(Box::new(p))), Stmt::For(x, var(s), vec![each])]
    }

    // Insert when absent, update when present, read by key.
    fn touch(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Block {
        let keys = self.key_input(t, f, env, false);
        let g = self.fresh();
        let x = self.fresh();
        let row = self.row_struct(t, f, env, &keys);
        let mut inner = env.clone();
        inner.vars.push((x, t.row_ty()));
        let mut fields: BTreeMap<FieldName, Expr> = BTreeMap::new();
        for c in &t.columns {
            let e = if c.ty == Ty::Int && !c.nullable && !t.key.contains(&c.name) {
                Expr::Op(Op::Add, vec![field(var(x), &c.name), int(1)])
            } else {
                field(var(x), &c.name)
            };
            fields.insert(c.name.clone(), e);
        }
        vec![
            Stmt::Let(g, Expr::Get(t.name.clone(), keys.clone())),
            Stmt::If(
                std1(StdFn::IsSome, var(g)),
                vec![Stmt::Update(t.name.clone(), keys, x, Expr::Struct(fields))],
                vec![Stmt::Insert(t.name.clone(), row, vec![])],
            ),
        ]
    }

    // ------------------------------------------------------------------
    // Queries

    fn query(&mut self, k: usize) -> Function {
        self.sym = 0;
        let t = self.table();
        let mut f = Function {
            name: String::new(),
            kind: FnKind::Query,
            router: Some("api".into()),
            uses: vec![],
            autos: vec![],
            input: vec![],
            refine: vec![],
            // The verifier names the real one (`verify_patching_queries`).
            ret: Some(Ty::Bool),
            body: vec![],
            plan: None,
            names: BTreeMap::new(),
        };
        let mut env = Env {
            helpers: self.helpers.clone(),
            user: true,
            ..Env::default()
        };
        let (name, plan) = match self.rng.below(8) {
            0 | 1 => ("list", self.list_plan(&t, &mut f, &mut env, true)),
            2 | 3 => ("tree", self.tree_plan(&t, &mut f, &mut env)),
            4 => ("up", self.lookup_plan(&t, &mut f, &mut env)),
            5 | 6 => ("groups", self.group_plan(&t, &mut f, &mut env)),
            _ => ("ranked", self.ranked_plan(&t, &mut f, &mut env)),
        };
        f.name = format!("{name}_{}_{k}", t.name);
        f.plan = Some(plan);
        f
    }

    // A filter over a table's columns: comparisons against input or
    // literals, sometimes combined.
    fn pred(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Option<Pred> {
        let cols: Vec<Column> = t.columns.iter().filter(|c| !matches!(c.ty, Ty::Bytes)).cloned().collect();
        let mut one = |g: &mut Self| -> Option<Pred> {
            let c = g.rng.pick(&cols).cloned()?;
            let ty = c.column_ty();
            let rhs = if g.rng.chance(60) {
                let n = format!("f_{}", c.name);
                if !f.input.iter().any(|(m, _)| *m == n) {
                    env.args.push((n.clone(), ty.clone()));
                    f.input.push((n.clone(), Field::plain(ty.clone())));
                }
                arg(&n)
            } else {
                g.lit(&ty)
            };
            Some(match g.rng.below(6) {
                0 => Pred::In(c.name.clone(), vec![rhs, g.lit(&ty)]),
                _ => Pred::Cmp(c.name.clone(), g.cmp_op(), rhs),
            })
        };
        match self.rng.below(10) {
            0..=2 => None,
            3..=6 => one(self),
            7 => Some(Pred::All([one(self), one(self)].into_iter().flatten().collect())),
            8 => Some(Pred::Any([one(self), one(self)].into_iter().flatten().collect())),
            _ => one(self).map(|p| Pred::Not(Box::new(p))),
        }
    }

    fn order(&mut self, t: &Table) -> Vec<(Key, Dir)> {
        let mut out = vec![];
        for _ in 0..self.rng.below(3) {
            let c = self.rng.pick(&t.columns).cloned().expect("a column");
            if !out.iter().any(|(k, _)| *k == Key::Column(c.name.clone())) {
                out.push((Key::Column(c.name), if self.rng.chance(50) { Dir::Asc } else { Dir::Desc }));
            }
        }
        out
    }

    fn limit(&mut self) -> Option<i64> {
        if self.rng.chance(35) {
            Some(1 + self.rng.below(4) as i64)
        } else {
            None
        }
    }

    fn list_plan(&mut self, t: &Table, f: &mut Function, env: &mut Env, top: bool) -> Plan {
        let mut p = bare(&t.name);
        p.filter = self.pred(t, f, env);
        p.order = self.order(t);
        p.limit = self.limit();
        if top && self.rng.chance(30) {
            // A projection: a column and something computed from the row.
            let r = self.fresh();
            p.row = Some(r);
            let mut node = env.clone();
            node.vars.push((r, t.row_ty()));
            node.args.clear();
            let c = self.rng.pick(&t.columns).cloned().expect("a column");
            let ty = Ty::Int;
            let extra = self.expr(&ty, &node, 2, false);
            p.project = Some(Expr::Struct(BTreeMap::from([
                ("a".to_string(), field(var(r), &c.name)),
                ("b".to_string(), extra),
            ])));
        }
        p
    }

    // A parent with its children beneath it, on the reference.
    fn tree_plan(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Plan {
        let kids = self.sch.children_of(&t.name);
        let Some(rel) = self.rng.pick(&kids).cloned() else {
            return self.list_plan(t, f, env, true);
        };
        let child = self.sch.lookup_table(&rel.child).expect("the child").clone();
        let mut p = bare(&t.name);
        let r = self.fresh();
        let k = self.fresh();
        p.row = Some(r);
        p.filter = self.pred(t, f, env);
        p.order = self.order(t);
        p.limit = self.limit();
        let mut c = self.list_plan(&child, f, env, false);
        if self.rng.chance(30) {
            // Grandchildren, when the child has any.
            let gk = self.sch.children_of(&child.name);
            if let Some(g) = self.rng.pick(&gk).cloned() {
                if child.key.len() == 1 {
                    let gt = self.sch.lookup_table(&g.child).expect("the grandchild").clone();
                    let cr = self.fresh();
                    let cs = self.fresh();
                    c.row = Some(cr);
                    let gp = self.list_plan(&gt, f, env, false);
                    c.related.push(Related {
                        name: "kids".into(),
                        sym: cs,
                        on: vec![(g.column.clone(), field(var(cr), &child.key[0]))],
                        plan: gp,
                    });
                }
            }
        }
        p.related.push(Related {
            name: "kids".into(),
            sym: k,
            on: vec![(rel.column.clone(), field(var(r), &t.key[0]))],
            plan: c,
        });
        if self.rng.chance(40) {
            p.having = Some(cmp(CmpOp::Gt, std1(StdFn::Len, var(k)), int(self.rng.below(2) as i64)));
        }
        if self.rng.chance(30) {
            let c0 = self.rng.pick(&t.columns).cloned().expect("a column");
            p.project = Some(Expr::Struct(BTreeMap::from([
                ("a".to_string(), field(var(r), &c0.name)),
                ("n".to_string(), std1(StdFn::Len, var(k))),
            ])));
        }
        if self.rng.chance(20) {
            p.order.insert(0, (Key::Expr(std1(StdFn::Len, var(k))), Dir::Desc));
        }
        p
    }

    // A child with its parent looked up by the reference.
    fn lookup_plan(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Plan {
        let ups = self.sch.parent_of(&t.name);
        let Some(rel) = self.rng.pick(&ups).cloned() else {
            return self.list_plan(t, f, env, true);
        };
        let parent = self.sch.lookup_table(&rel.parent).expect("the parent").clone();
        let mut p = bare(&t.name);
        let r = self.fresh();
        let u = self.fresh();
        let z = self.fresh();
        p.row = Some(r);
        p.filter = self.pred(t, f, env);
        p.order = self.order(t);
        p.limit = self.limit();
        p.lookups.push(Lookup {
            name: "up".into(),
            sym: u,
            table: parent.name.clone(),
            key: vec![field(var(r), &rel.column)],
        });
        let pc = self.rng.pick(&parent.columns).cloned().expect("a column");
        let pty = pc.column_ty();
        let dflt = self.lit(&pty);
        p.project = Some(Expr::Struct(BTreeMap::from([
            ("x".to_string(), field(var(r), &t.columns[0].name)),
            (
                "y".to_string(),
                Expr::Match(Box::new(var(u)), z, Box::new(field(var(z), &pc.name)), Box::new(dflt)),
            ),
        ])));
        if self.rng.chance(30) {
            p.having = Some(std1(StdFn::IsSome, var(u)));
        }
        p
    }

    // Rows grouped by a column: how many, and a sum, per group.
    fn group_plan(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Plan {
        let by = self.rng.pick(&t.columns).cloned().expect("a column");
        let mut p = bare(&t.name);
        let g = self.fresh();
        let m = self.fresh();
        let acc = self.fresh();
        let x = self.fresh();
        p.source = Source::Group {
            table: t.name.clone(),
            by: vec![by.name.clone()],
        };
        p.row = Some(g);
        p.members = Some(m);
        p.filter = self.pred(t, f, env);
        let mut fields = BTreeMap::from([("key".to_string(), field(var(g), &by.name)), ("n".to_string(), std1(StdFn::Len, var(m)))]);
        if let Some(c) = t.columns.iter().find(|c| c.ty == Ty::Int) {
            let v = if c.nullable {
                Expr::Std(StdFn::UnwrapOr, vec![field(var(x), &c.name), int(0)])
            } else {
                field(var(x), &c.name)
            };
            fields.insert(
                "s".into(),
                Expr::Fold(Box::new(var(m)), Box::new(int(0)), acc, x, Box::new(Expr::Op(Op::Add, vec![var(acc), v]))),
            );
        }
        p.project = Some(Expr::Struct(fields));
        if self.rng.chance(40) {
            p.having = Some(cmp(CmpOp::Ge, std1(StdFn::Len, var(m)), int(1 + self.rng.below(2) as i64)));
        }
        p.order = if self.rng.chance(40) {
            vec![(Key::Expr(std1(StdFn::Len, var(m))), Dir::Desc)]
        } else {
            vec![(Key::Column(by.name.clone()), if self.rng.chance(50) { Dir::Asc } else { Dir::Desc })]
        };
        p.limit = self.limit();
        p
    }

    // Ordered by an expression over the row, under a limit: the window's
    // edge moves with every edit.
    fn ranked_plan(&mut self, t: &Table, f: &mut Function, env: &mut Env) -> Plan {
        let mut p = bare(&t.name);
        let r = self.fresh();
        p.row = Some(r);
        p.filter = self.pred(t, f, env);
        let mut node = Env {
            helpers: self.helpers.clone(),
            ..Env::default()
        };
        node.vars.push((r, t.row_ty()));
        let key = self.expr(&Ty::Int, &node, 2, false);
        p.order = vec![(Key::Expr(key), if self.rng.chance(50) { Dir::Asc } else { Dir::Desc })];
        p.limit = Some(1 + self.rng.below(3) as i64);
        p
    }
}
