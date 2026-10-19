//! §1.4 The one checker: types, effects, and refinements by signature.
//!
//! A function is checked once, against the schema it will run over and
//! the functions it calls, and its hash then covers the result (§1.5).
//! Every expression has a type and an effect, `Pure < Read < Write`; a
//! function's body must have a type its `ret` admits at an effect its
//! `effect` admits.
//!
//! **Refinements are types.** [`Types`] resolves a function's declarations
//! (and `Text = Bytes where utf8`, which every function has). A declared
//! type is a subtype of what it is declared over — `Name ≤ Text ≤ Bytes`
//! — and nothing becomes refined inside the language except by a
//! signature that says so ([`crate::std::sig`]) or a literal, which is
//! parsed here under its type once. A declaration without refinements is
//! a name for its structure, both ways; that is how a type recurs. Two
//! values are compared only when they are of one structure, refinements
//! aside, and the empty sum is the type of `Refuse`, which every type
//! admits.
//!
//! **Total by construction.** A call is by hash, and the callee is checked
//! before the caller can name it, so the call graph is a DAG and every
//! function ends: there is no termination rule. A `Lambda` stands only as
//! an operator's argument. A refinement's predicate is `Pure`, of the
//! value alone: no context, no call.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::hash::FnHash;
use crate::interp::{parse, Loaded};
use crate::ir::{text_decl, CtxField, Decl, Effect, Expr, Function, Op, Rule, Un};
use crate::schema::{Schema, Table};
use crate::std::sig;
use crate::ty::Ty;
use crate::Refusal;

/// A function's declarations, by name, `Text` among them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Types(BTreeMap<String, Decl>);

impl Types {
    pub fn new(decls: &[(String, Decl)]) -> Types {
        let mut m: BTreeMap<_, _> = decls.iter().cloned().collect();
        m.entry("Text".into()).or_insert_with(text_decl);
        Types(m)
    }

    pub fn text() -> Ty {
        Ty::Named("Text".into())
    }

    pub fn decl(&self, n: &str) -> Option<&Decl> {
        self.0.get(n)
    }

    /// The constructor a type is at its top, names unfolded.
    pub fn structure(&self, t: &Ty) -> Ty {
        let mut t = t.clone();
        for _ in 0..64 {
            match t {
                Ty::Named(n) => t = self.decl(&n).map_or(Ty::Tuple(vec![]), |d| d.ty.clone()),
                t => return t,
            }
        }
        t
    }

    /// Every name unfolded, to a depth: a recursive type stays named there.
    pub fn deep(&self, t: &Ty) -> Ty {
        self.deep_to(t, 16)
    }

    fn deep_to(&self, t: &Ty, n: u32) -> Ty {
        if n == 0 {
            return t.clone();
        }
        let each = |ts: &[Ty]| ts.iter().map(|t| self.deep_to(t, n - 1)).collect();
        match self.structure(t) {
            Ty::List(e) => Ty::list(self.deep_to(&e, n - 1)),
            Ty::Tuple(ts) => Ty::Tuple(each(&ts)),
            Ty::Sum(vs) => Ty::Sum(vs.iter().map(|v| each(v)).collect()),
            t => t,
        }
    }

    /// Whether a declaration is text: `utf8` in it or under it.
    pub fn texty(&self, d: &Decl) -> bool {
        d.refine.iter().any(|r| r.rule == Rule::Utf8) || matches!(&d.ty, Ty::Named(m) if self.decl(m).is_some_and(|d| self.texty(d)))
    }

    /// Whether every value of `a` is a value of `b`.
    pub fn sub(&self, a: &Ty, b: &Ty) -> bool {
        self.sub_in(a, b, &mut vec![])
    }

    fn sub_in(&self, a: &Ty, b: &Ty, seen: &mut Vec<(Ty, Ty)>) -> bool {
        if a == b || *a == Ty::Sum(vec![]) || seen.iter().any(|(x, y)| x == a && y == b) {
            return true;
        }
        seen.push((a.clone(), b.clone()));
        let all = |xs: &[Ty], ys: &[Ty], seen: &mut Vec<_>| xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| self.sub_in(x, y, seen));
        match (a, b) {
            (Ty::Named(n), _) => self.decl(n).is_some_and(|d| self.sub_in(&d.ty.clone(), b, seen)),
            (_, Ty::Named(m)) => self.decl(m).is_some_and(|d| d.refine.is_empty() && self.sub_in(a, &d.ty.clone(), seen)),
            (Ty::List(x), Ty::List(y)) => self.sub_in(x, y, seen),
            (Ty::Tuple(xs), Ty::Tuple(ys)) => all(xs, ys, seen),
            (Ty::Sum(xs), Ty::Sum(ys)) => xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| all(x, y, seen)),
            _ => false,
        }
    }

    /// A type with its refinements dropped where they are on top.
    fn base(&self, t: &Ty) -> Ty {
        match t {
            Ty::Named(n) => match self.decl(n) {
                Some(d) if !d.refine.is_empty() => self.base(&d.ty),
                _ => t.clone(),
            },
            t => t.clone(),
        }
    }

    /// The least type admitting both, refinements dropped if need be.
    fn join(&self, a: &Ty, b: &Ty) -> Option<Ty> {
        let pick = |a: &Ty, b: &Ty| match (self.sub(a, b), self.sub(b, a)) {
            (true, _) => Some(b.clone()),
            (_, true) => Some(a.clone()),
            _ => None,
        };
        pick(a, b).or_else(|| pick(&self.base(a), &self.base(b)))
    }
}

/// A table's row as the checker types it: each column its declaration
/// when the function has one of that name and structure, else the
/// structure. The tuple of the key columns is the key.
fn row(cx: &Types, t: &Table, key: bool) -> Ty {
    let cols = &t.columns[..if key { t.key } else { t.columns.len() }];
    Ty::Tuple(
        cols.iter()
            .map(|c| match cx.decl(&c.decl) {
                Some(_) if cx.deep(&Ty::Named(c.decl.clone())) == c.ty => Ty::Named(c.decl.clone()),
                _ => c.ty.clone(),
            })
            .collect(),
    )
}

/// What a check reads besides the function: the schema, and the
/// functions it may call, checked already.
pub struct Env<'a> {
    pub schema: &'a Schema,
    pub callee: &'a dyn Fn(&FnHash) -> Option<Arc<Loaded>>,
}

struct Cx<'a> {
    env: &'a Env<'a>,
    types: &'a Types,
    decls: &'a [(String, Decl)],
    stack: Vec<Ty>,
    /// In a refinement's predicate: no context, no call.
    bare: bool,
}

type Out = Result<(Ty, Effect), String>;

fn want(ok: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(why())
    }
}

impl Cx<'_> {
    /// A type every name of which is declared.
    fn wf(&self, t: &Ty) -> Result<(), String> {
        match t {
            Ty::Named(n) => want(self.types.decl(n).is_some(), || format!("{n} is not declared")),
            Ty::List(e) => self.wf(e),
            Ty::Tuple(ts) => ts.iter().try_for_each(|t| self.wf(t)),
            Ty::Sum(vs) => vs.iter().flatten().try_for_each(|t| self.wf(t)),
            _ => Ok(()),
        }
    }

    fn is(&self, a: &Ty, b: &Ty, what: &str) -> Result<(), String> {
        want(self.types.sub(a, b), || format!("{what} is {a:?}, which is not {b:?}"))
    }

    fn table(&self, t: &str) -> Result<&Table, String> {
        self.env.schema.table(t).map(|(_, t)| t).ok_or_else(|| format!("no table {t}"))
    }

    fn list(&self, t: &Ty) -> Result<Ty, String> {
        match self.types.structure(t) {
            Ty::List(e) => Ok(*e),
            t => Err(format!("{t:?} is not a list")),
        }
    }

    /// A lambda over `params`: its body's type and effect.
    fn lam(&mut self, e: &Expr, params: Vec<Ty>) -> Out {
        let Expr::Lambda(n, body) = e else { return Err(format!("{e:?} is not a lambda")) };
        want(*n as usize == params.len(), || format!("a lambda of {n} where {} are bound", params.len()))?;
        let depth = self.stack.len();
        self.stack.extend(params);
        let r = self.infer(body);
        self.stack.truncate(depth);
        r
    }

    fn infer(&mut self, e: &Expr) -> Out {
        use Effect::*;
        let unit = Ty::unit();
        Ok(match e {
            Expr::Lit(t, b) => {
                self.wf(t)?;
                let p = parse(self.types, t, b).map_err(|r| format!("a literal of {t:?}: {r}"))?;
                want(p.bytes() == &b[..], || format!("a literal of {t:?} is not in normal form"))?;
                (t.clone(), Pure)
            }
            Expr::Var(i) => {
                let n = self.stack.len();
                let t = (*i as usize).lt(&n).then(|| self.stack[n - 1 - *i as usize].clone());
                (t.ok_or_else(|| format!("variable {i} is not bound"))?, Pure)
            }
            Expr::Let(v, body) => {
                let (t, f) = self.infer(v)?;
                self.stack.push(t);
                let (u, g) = self.infer(body)?;
                self.stack.pop();
                (u, f.max(g))
            }
            Expr::If(c, a, b) => {
                let (t, f) = self.infer(c)?;
                self.is(&t, &Ty::bool(), "a condition")?;
                let ((x, g), (y, h)) = (self.infer(a)?, self.infer(b)?);
                let j = self.types.join(&x, &y).ok_or_else(|| format!("the branches are {x:?} and {y:?}"))?;
                (j, f.max(g).max(h))
            }
            Expr::Match(s, arms) => {
                let (t, mut f) = self.infer(s)?;
                let Ty::Sum(vs) = self.types.structure(&t) else { return Err(format!("a match on {t:?}")) };
                want(vs.len() == arms.len(), || format!("{} arms for {} variants", arms.len(), vs.len()))?;
                let mut out = Ty::Sum(vec![]);
                for (v, arm) in vs.into_iter().zip(arms) {
                    let depth = self.stack.len();
                    self.stack.extend(v);
                    let (u, g) = self.infer(arm)?;
                    self.stack.truncate(depth);
                    out = self.types.join(&out, &u).ok_or_else(|| format!("the arms are {out:?} and {u:?}"))?;
                    f = f.max(g);
                }
                (out, f)
            }
            Expr::Field(x, i) => {
                let (t, f) = self.infer(x)?;
                match self.types.structure(&t) {
                    Ty::Tuple(ts) if (*i as usize) < ts.len() => (ts[*i as usize].clone(), f),
                    t => return Err(format!("no field {i} of {t:?}")),
                }
            }
            Expr::Tuple(es) => {
                let mut f = Pure;
                let ts = es.iter().map(|e| self.infer(e).map(|(t, g)| (t, f = f.max(g)).0)).collect::<Result<_, _>>()?;
                (Ty::Tuple(ts), f)
            }
            Expr::Variant(t, i, es) => {
                self.wf(t)?;
                let Ty::Sum(vs) = self.types.structure(t) else { return Err(format!("{t:?} is not a sum")) };
                let fields = vs.get(*i as usize).ok_or_else(|| format!("no variant {i} of {t:?}"))?;
                want(fields.len() == es.len(), || format!("variant {i} of {t:?} has {} fields", fields.len()))?;
                let mut f = Pure;
                for (e, w) in es.iter().zip(fields) {
                    let (u, g) = self.infer(e)?;
                    self.is(&u, w, "a field")?;
                    f = f.max(g);
                }
                (t.clone(), f)
            }
            Expr::List(t, es) => {
                self.wf(t)?;
                let mut f = Pure;
                for e in es {
                    let (u, g) = self.infer(e)?;
                    self.is(&u, t, "an element")?;
                    f = f.max(g);
                }
                (Ty::list(t.clone()), f)
            }
            Expr::Op(op, a, b) => {
                let ((x, f), (y, g)) = (self.infer(a)?, self.infer(b)?);
                let t = match op {
                    Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod => {
                        self.is(&x, &Ty::Int, "an operand")?;
                        self.is(&y, &Ty::Int, "an operand")?;
                        Ty::Int
                    }
                    Op::And | Op::Or => {
                        self.is(&x, &Ty::bool(), "an operand")?;
                        self.is(&y, &Ty::bool(), "an operand")?;
                        Ty::bool()
                    }
                    _ => {
                        self.types.join(&x, &y).ok_or_else(|| format!("{x:?} compared with {y:?}"))?;
                        Ty::bool()
                    }
                };
                (t, f.max(g))
            }
            Expr::Un(op, a) => {
                let (x, f) = self.infer(a)?;
                let t = if *op == Un::Neg { Ty::Int } else { Ty::bool() };
                self.is(&x, &t, "an operand")?;
                (t, f)
            }
            Expr::Std(s, es) => {
                let mut f = Pure;
                let ts: Vec<Ty> = es.iter().map(|e| self.infer(e).map(|(t, g)| (t, f = f.max(g)).0)).collect::<Result<_, _>>()?;
                (sig(self.types, *s, &ts)?, f)
            }
            Expr::Call(h, es) => {
                want(!self.bare, || "a call in a refinement".into())?;
                let c = (self.env.callee)(h).ok_or_else(|| format!("no function {h}"))?;
                for (n, d) in &c.f.types {
                    want(self.decls.iter().any(|(m, e)| m == n && e == d), || format!("{h} declares {n}, which this function does not"))?;
                }
                want(c.f.params.len() == es.len(), || format!("{h} takes {} arguments", c.f.params.len()))?;
                let mut f = c.f.effect;
                for (e, p) in es.iter().zip(&c.f.params) {
                    let (t, g) = self.infer(e)?;
                    self.is(&t, p, "an argument")?;
                    f = f.max(g);
                }
                (c.f.ret.clone(), f)
            }
            Expr::Ctx(c) => {
                want(!self.bare, || "the context in a refinement".into())?;
                match c {
                    CtxField::User | CtxField::Session => (Types::text(), Pure),
                    CtxField::Roles => (Ty::list(Types::text()), Pure),
                    CtxField::Now => (Ty::Int, Pure),
                    CtxField::NewId(t) => {
                        self.table(t)?;
                        let id = format!("Id<{t}>");
                        (if self.types.decl(&id).is_some() { Ty::Named(id) } else { Ty::Bytes }, Write)
                    }
                }
            }
            Expr::Lambda(..) => return Err("a lambda outside an operator".into()),
            Expr::Scan(t) => (Ty::list(row(self.types, self.table(t)?, false)), Read),
            Expr::Lookup(t, k) => {
                let tab = self.table(t)?;
                let (r, key) = (row(self.types, tab, false), row(self.types, tab, true));
                let (u, f) = self.infer(k)?;
                self.is(&u, &key, "a key")?;
                (Ty::option(r), f.max(Read))
            }
            Expr::Filter(l, p) | Expr::Map(l, p) => {
                let (t, f) = self.infer(l)?;
                let (u, g) = self.lam(p, vec![self.list(&t)?])?;
                if let Expr::Filter(..) = e {
                    self.is(&u, &Ty::bool(), "a filter")?;
                    (Ty::list(self.list(&t)?), f.max(g))
                } else {
                    (Ty::list(u), f.max(g))
                }
            }
            Expr::Join(a, b, on) => {
                let ((x, f), (y, g)) = (self.infer(a)?, self.infer(b)?);
                let (x, y) = (self.list(&x)?, self.list(&y)?);
                let (u, h) = self.lam(on, vec![x.clone(), y.clone()])?;
                self.is(&u, &Ty::bool(), "a join's condition")?;
                (Ty::list(Ty::Tuple(vec![x, y])), f.max(g).max(h))
            }
            Expr::Group(l, by, agg) => {
                let (t, f) = self.infer(l)?;
                let x = self.list(&t)?;
                let (k, g) = self.lam(by, vec![x.clone()])?;
                let (a, h) = self.lam(agg, vec![k.clone(), Ty::list(x)])?;
                (Ty::list(Ty::Tuple(vec![k, a])), f.max(g).max(h))
            }
            Expr::Sort(l, keys, dirs) => {
                let (t, f) = self.infer(l)?;
                let (k, g) = self.lam(keys, vec![self.list(&t)?])?;
                let n = match self.types.structure(&k) {
                    Ty::Tuple(ts) if dirs.len() != 1 => ts.len(),
                    _ => 1,
                };
                want(n == dirs.len(), || format!("{} directions for {n} keys", dirs.len()))?;
                (Ty::list(self.list(&t)?), f.max(g))
            }
            Expr::Take(l, n) => {
                let ((t, f), (u, g)) = (self.infer(l)?, self.infer(n)?);
                self.is(&u, &Ty::Int, "a count")?;
                (Ty::list(self.list(&t)?), f.max(g))
            }
            Expr::Insert(t, r) => {
                let want_row = row(self.types, self.table(t)?, false);
                let (u, f) = self.infer(r)?;
                self.is(&u, &want_row, "a row")?;
                (unit, f.max(Write))
            }
            Expr::Update(t, k, lam) => {
                let tab = self.table(t)?;
                let (r, key) = (row(self.types, tab, false), row(self.types, tab, true));
                let (u, f) = self.infer(k)?;
                self.is(&u, &key, "a key")?;
                let (v, g) = self.lam(lam, vec![r.clone()])?;
                self.is(&v, &r, "an updated row")?;
                (unit, f.max(g).max(Write))
            }
            Expr::Delete(t, k) => {
                let key = row(self.types, self.table(t)?, true);
                let (u, f) = self.infer(k)?;
                self.is(&u, &key, "a key")?;
                (unit, f.max(Write))
            }
            Expr::Refuse(m) => {
                let (t, f) = self.infer(m)?;
                self.is(&t, &Types::text(), "a refusal")?;
                (Ty::Sum(vec![]), f)
            }
            Expr::Seq(es) => {
                let mut out = (unit, Pure);
                for e in es {
                    let (t, f) = self.infer(e)?;
                    out = (t, out.1.max(f));
                }
                out
            }
        })
    }

    /// A declaration's structure: declared names, a recursion only through
    /// a list or a sum, and refinements that fit it.
    fn decl(&mut self, n: &str, d: &Decl) -> Result<(), String> {
        self.wf(&d.ty)?;
        let (mut todo, mut seen) = (vec![d.ty.clone()], vec![]);
        while let Some(t) = todo.pop() {
            match t {
                Ty::Named(m) if m == n => return Err(format!("{n} contains itself outside a list or a sum")),
                Ty::Named(m) if !seen.contains(&m) => {
                    todo.extend(self.types.decl(&m).map(|d| d.ty.clone()));
                    seen.push(m);
                }
                Ty::Tuple(ts) => todo.extend(ts),
                _ => {}
            }
        }
        let s = self.types.structure(&d.ty);
        for (i, r) in d.refine.iter().enumerate() {
            let before = Decl { ty: d.ty.clone(), refine: d.refine[..i].to_vec() };
            let ok = match &r.rule {
                Rule::Utf8 => s == Ty::Bytes,
                Rule::Trim => self.types.texty(&before),
                Rule::Len(_) | Rule::LenRange(..) | Rule::NonEmpty => matches!(s, Ty::Bytes | Ty::List(_)),
                Rule::Refine(p) => {
                    self.bare = true;
                    let (t, f) = self.lam(&Expr::Lambda(1, Box::new(p.clone())), vec![d.ty.clone()])?;
                    self.bare = false;
                    self.is(&t, &Ty::bool(), "a refinement")?;
                    f == Effect::Pure
                }
            };
            want(ok, || format!("{n}: {:?} does not fit {:?}", r.rule, d.ty))?;
        }
        Ok(())
    }
}

/// §1.4 Whether `f` is well-typed over `env`: its declarations sorted,
/// each once, `Text` as it is; its body of a type `ret` admits, at an
/// effect `effect` admits.
pub fn check(f: &Function, env: &Env) -> Result<Types, Refusal> {
    let types = Types::new(&f.types);
    let mut cx = Cx {
        env,
        types: &types,
        decls: &f.types,
        stack: vec![],
        bare: false,
    };
    let r = (|| {
        want(f.types.windows(2).all(|w| w[0].0 < w[1].0), || "the declarations are not sorted, each once".into())?;
        want(types.decl("Text") == Some(&text_decl()), || "Text is Bytes where utf8".into())?;
        for (n, d) in &f.types {
            cx.decl(n, d)?;
        }
        f.params.iter().chain([&f.ret]).try_for_each(|t| cx.wf(t))?;
        cx.stack = f.params.clone();
        let (t, e) = cx.infer(&f.body)?;
        cx.is(&t, &f.ret, "the body")?;
        want(e <= f.effect, || format!("the body's effect is {e:?}, above {:?}", f.effect))
    })();
    r.map_err(Refusal)?;
    Ok(types)
}
