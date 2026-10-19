//! §1.4 The one interpreter, and §1.1's boundary.
//!
//! [`parse`] is the one door bytes enter by from outside — an input, a
//! form: canonical bytes of the type, every declaration's refinements
//! held, in order, `trim` normalising before any later one. Its refusal is
//! what a form shows: the refinement's own words, or the default's
//! (`at least 1 character`, `exactly 16 bytes`). [`check_form`] is the
//! same walk reporting every field that fails, by its path of positions.
//!
//! [`Interp`] is a function as a [`Procedure`]: it parses the input, then
//! evaluates the body to the list of changes it makes (**effects are
//! values**: the store is not written). A body that reads what it wrote
//! reads through its own list so far ([`Machine::get`]). Arithmetic is
//! checked, and an overflow, a division by zero, an index out of range is
//! a refusal. `Ctx(now)` and the rest are the entry's; `NewId` is the
//! entry's `n`-th draw, `n` counted from 0 in evaluation order.
//!
//! While it runs, a value is [`V`]: the tree the codec reads under the
//! type the checker gave it — no tag, no null, no order of its own. Two
//! are compared by their bytes, which is the type's order (§1.1).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::check::Types;
use crate::codec::{put_bytes, put_int, Reader, Typed};
use crate::hash::FnHash;
use crate::ir::{CtxField, Decl, Expr, Function, Op, Rule, Un};
use crate::peer::{Ctx, Procedure};
use crate::store::{Change, Dir, Read, Scan};
use crate::ty::Ty;
use crate::Refusal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum V {
    Int(i64),
    Bytes(Vec<u8>),
    Tup(Vec<V>),
    Sum(u8, Vec<V>),
    List(Vec<V>),
}

impl V {
    pub fn bool(b: bool) -> V {
        V::Sum(b as u8, vec![])
    }
    pub fn none() -> V {
        V::Sum(0, vec![])
    }
    pub fn some(v: V) -> V {
        V::Sum(1, vec![v])
    }
    pub fn int(&self) -> i64 {
        match self {
            V::Int(n) => *n,
            v => panic!("{v:?} is not an int"),
        }
    }
    pub fn bytes(&self) -> &[u8] {
        match self {
            V::Bytes(b) => b,
            v => panic!("{v:?} is not bytes"),
        }
    }
    pub fn list(&self) -> &[V] {
        match self {
            V::List(xs) => xs,
            v => panic!("{v:?} is not a list"),
        }
    }
    pub fn variant(&self) -> (u8, &[V]) {
        match self {
            V::Sum(i, fs) => (*i, fs),
            v => panic!("{v:?} is not a sum"),
        }
    }
    pub fn truth(&self) -> bool {
        self.variant().0 == 1
    }
    pub fn put(&self, out: &mut Vec<u8>) {
        match self {
            V::Int(n) => put_int(out, *n),
            V::Bytes(b) => put_bytes(out, b),
            V::Tup(fs) => fs.iter().for_each(|f| f.put(out)),
            V::Sum(i, fs) => {
                out.push(*i);
                fs.iter().for_each(|f| f.put(out));
            }
            V::List(xs) => {
                for x in xs {
                    out.push(1);
                    x.put(out);
                }
                out.push(0);
            }
        }
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![];
        self.put(&mut out);
        out
    }
}

/// Reads a value of a type; with `refine`, holds it to every refinement
/// on the way and keeps what fails, by path.
struct Reading<'a> {
    cx: &'a Types,
    refine: bool,
    path: Vec<u32>,
    fails: Vec<(Vec<u32>, String)>,
}

fn many(n: i64, unit: &str) -> String {
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

impl Reading<'_> {
    fn at<T>(&mut self, i: usize, f: impl FnOnce(&mut Self) -> T) -> T {
        self.path.push(i as u32);
        let x = f(self);
        self.path.pop();
        x
    }

    fn val(&mut self, t: &Ty, r: &mut Reader) -> Result<V, Refusal> {
        Ok(match t {
            Ty::Int => V::Int(r.int()?),
            Ty::Bytes => V::Bytes(r.bytes()?),
            Ty::List(e) => {
                let mut xs = vec![];
                while r.more()? {
                    xs.push(self.at(xs.len(), |s| s.val(e, r))?);
                }
                V::List(xs)
            }
            Ty::Tuple(ts) => V::Tup(ts.iter().enumerate().map(|(i, t)| self.at(i, |s| s.val(t, r))).collect::<Result<_, _>>()?),
            Ty::Sum(vs) => {
                let i = r.variant(vs.len())?;
                let fs = vs[i as usize].iter().enumerate().map(|(k, t)| self.at(k, |s| s.val(t, r)));
                V::Sum(i, fs.collect::<Result<_, _>>()?)
            }
            Ty::Named(n) => {
                let d = self.cx.decl(n).ok_or_else(|| Refusal(format!("{n} is not declared")))?;
                let v = self.val(&d.ty, r)?;
                match self.refine {
                    true => self.held(d, v)?,
                    false => v,
                }
            }
        })
    }

    /// The value held to a declaration's rules, normalised by `trim`.
    fn held(&mut self, d: &Decl, mut v: V) -> Result<V, Refusal> {
        let texty = self.cx.texty(d);
        let unit = match &v {
            V::Bytes(_) if texty => "character",
            V::Bytes(_) => "byte",
            _ => "element",
        };
        for r in &d.refine {
            let len = match &v {
                V::Bytes(b) if texty => String::from_utf8_lossy(b).chars().count() as i64,
                V::Bytes(b) => b.len() as i64,
                V::List(xs) => xs.len() as i64,
                _ => 0,
            };
            let fail = match &r.rule {
                Rule::Utf8 => std::str::from_utf8(v.bytes()).is_err().then(|| "not UTF-8 text".into()),
                Rule::Trim => {
                    v = V::Bytes(crate::std::trim(std::str::from_utf8(v.bytes()).unwrap_or("")).into());
                    None
                }
                Rule::Len(n) => (len != *n).then(|| format!("exactly {}", many(*n, unit))),
                Rule::LenRange(lo, _) if len < *lo => Some(format!("at least {}", many(*lo, unit))),
                Rule::LenRange(_, hi) => (len > *hi).then(|| format!("at most {}", many(*hi, unit))),
                Rule::NonEmpty => (len == 0).then(|| "must not be empty".into()),
                Rule::Refine(p) => {
                    let mut m = Machine::bare(self.cx);
                    m.stack.push(v.clone());
                    (!m.eval(p)?.truth()).then(|| "is not valid".into())
                }
            };
            if let Some(why) = fail {
                self.fails.push((self.path.clone(), r.why.clone().unwrap_or(why)));
                break;
            }
        }
        Ok(v)
    }
}

/// A value of `t` from canonical bytes, names resolved, refinements not
/// asked: what the checker or the store vouched for already.
pub fn decode(cx: &Types, t: &Ty, b: &[u8]) -> Result<V, Refusal> {
    let mut r = Reader::new(b);
    let mut s = Reading { cx, refine: false, path: vec![], fails: vec![] };
    let v = s.val(t, &mut r)?;
    r.end()?;
    Ok(v)
}

/// §1.1 The form validator: the value, normalised, or every refinement
/// that fails, each by its path of positions and its message.
pub fn check_form(cx: &Types, t: &Ty, b: &[u8]) -> Result<V, Vec<(Vec<u32>, String)>> {
    let mut r = Reader::new(b);
    let mut s = Reading { cx, refine: true, path: vec![], fails: vec![] };
    match s.val(t, &mut r).and_then(|v| r.end().map(|_| v)) {
        Err(e) => Err(vec![(vec![], e.0)]),
        Ok(_) if !s.fails.is_empty() => Err(s.fails),
        Ok(v) => Ok(v),
    }
}

/// §1.1 The boundary: bytes of `t` with its refinements held, normalised,
/// or the first refinement's refusal.
pub fn parse(cx: &Types, t: &Ty, b: &[u8]) -> Result<Typed, Refusal> {
    match check_form(cx, t, b) {
        Ok(v) => Ok(Typed(v.to_bytes())),
        Err(mut fails) => Err(Refusal(fails.remove(0).1)),
    }
}

/// A checked function, ready to run, with its declarations resolved.
#[derive(Debug)]
pub struct Loaded {
    pub hash: FnHash,
    pub f: Function,
    pub cx: Types,
}

/// The functions a run may call, by hash.
pub type Callee<'a> = &'a dyn Fn(&FnHash) -> Option<Arc<Loaded>>;

/// One run: the store it reads, the entry, what it has written so far.
pub struct Machine<'a> {
    store: Option<&'a dyn Read>,
    ctx: Option<&'a Ctx>,
    callee: Option<Callee<'a>>,
    cx: &'a Types,
    pub stack: Vec<V>,
    pub changes: Vec<Change>,
    draws: i64,
}

fn refuse<T>(why: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal(why.into()))
}

impl<'a> Machine<'a> {
    /// A run of `Pure` code alone: a refinement's predicate.
    pub fn bare(cx: &'a Types) -> Machine<'a> {
        Machine { store: None, ctx: None, callee: None, cx, stack: vec![], changes: vec![], draws: 0 }
    }

    pub fn new(store: &'a dyn Read, ctx: &'a Ctx, callee: Callee<'a>, cx: &'a Types) -> Machine<'a> {
        Machine { store: Some(store), ctx: Some(ctx), callee: Some(callee), ..Machine::bare(cx) }
    }

    /// This run's store, entry and functions, under other declarations.
    fn at<'b>(&self, cx: &'b Types) -> Machine<'b>
    where
        'a: 'b,
    {
        Machine { store: self.store, ctx: self.ctx, callee: self.callee, ..Machine::bare(cx) }
    }

    fn store(&self) -> &'a dyn Read {
        self.store.expect("the checker let only a run with a store read")
    }

    fn row_ty(&self, table: &str) -> Result<(Ty, usize), Refusal> {
        let (_, t) = self.store().schema().table(table).ok_or_else(|| Refusal(format!("no table {table}")))?;
        Ok((t.row_ty(), t.key))
    }

    fn key_of(&self, table: &str, row: &[u8]) -> Vec<u8> {
        let t = self.store().schema().table(table).expect("a table").1;
        row[..t.key_len(row).expect("a row")].to_vec()
    }

    /// A row by key, as the store holds it with this run's changes on it.
    pub fn get(&self, table: &str, key: &[u8]) -> Option<Vec<u8>> {
        for c in self.changes.iter().rev().filter(|c| c.table() == table) {
            match c {
                Change::Add { row, .. } | Change::Edit { new: row, .. } if self.key_of(table, row) == key => return Some(row.clone()),
                Change::Remove { row, .. } if self.key_of(table, row) == key => return None,
                _ => {}
            }
        }
        self.store().get(table, key)
    }

    /// A table's rows in key order, likewise.
    fn rows(&self, table: &str) -> Vec<Vec<u8>> {
        let mut m: BTreeMap<Vec<u8>, Vec<u8>> =
            self.store().scan(table, &Scan::all()).into_iter().map(|r| (self.key_of(table, &r), r)).collect();
        for c in self.changes.iter().filter(|c| c.table() == table) {
            match c {
                Change::Add { row, .. } | Change::Edit { new: row, .. } => m.insert(self.key_of(table, row), row.clone()),
                Change::Remove { row, .. } => m.remove(&self.key_of(table, row)),
            };
        }
        m.into_values().collect()
    }

    fn row(&self, table: &str, b: &[u8]) -> Result<V, Refusal> {
        decode(self.cx, &self.row_ty(table)?.0, b)
    }

    /// A lambda applied: its body with `args` bound.
    fn apply(&mut self, lam: &Expr, args: Vec<V>) -> Result<V, Refusal> {
        let Expr::Lambda(_, body) = lam else { return refuse("not a lambda") };
        let depth = self.stack.len();
        self.stack.extend(args);
        let r = self.eval(body);
        self.stack.truncate(depth);
        r
    }

    fn list(&mut self, e: &Expr) -> Result<Vec<V>, Refusal> {
        match self.eval(e)? {
            V::List(xs) => Ok(xs),
            v => refuse(format!("{v:?} is not a list")),
        }
    }

    pub fn eval(&mut self, e: &Expr) -> Result<V, Refusal> {
        let unit = V::Tup(vec![]);
        Ok(match e {
            Expr::Lit(t, b) => decode(self.cx, t, b)?,
            Expr::Var(i) => self.stack[self.stack.len() - 1 - *i as usize].clone(),
            Expr::Let(v, body) => {
                let v = self.eval(v)?;
                self.stack.push(v);
                let r = self.eval(body);
                self.stack.pop();
                r?
            }
            Expr::If(c, a, b) => match self.eval(c)?.truth() {
                true => self.eval(a)?,
                false => self.eval(b)?,
            },
            Expr::Match(s, arms) => {
                let V::Sum(i, fs) = self.eval(s)? else { return refuse("a match on no sum") };
                let depth = self.stack.len();
                self.stack.extend(fs);
                let r = self.eval(&arms[i as usize]);
                self.stack.truncate(depth);
                r?
            }
            Expr::Field(x, i) => match self.eval(x)? {
                V::Tup(mut fs) => fs.swap_remove(*i as usize),
                v => return refuse(format!("no field of {v:?}")),
            },
            Expr::Tuple(es) => V::Tup(es.iter().map(|e| self.eval(e)).collect::<Result<_, _>>()?),
            Expr::Variant(_, i, es) => V::Sum(*i, es.iter().map(|e| self.eval(e)).collect::<Result<_, _>>()?),
            Expr::List(_, es) => V::List(es.iter().map(|e| self.eval(e)).collect::<Result<_, _>>()?),
            Expr::Op(Op::And, a, b) => V::bool(self.eval(a)?.truth() && self.eval(b)?.truth()),
            Expr::Op(Op::Or, a, b) => V::bool(self.eval(a)?.truth() || self.eval(b)?.truth()),
            Expr::Op(op, a, b) => {
                let (x, y) = (self.eval(a)?, self.eval(b)?);
                let int = |f: fn(i64, i64) -> Option<i64>| match f(x.int(), y.int()) {
                    Some(n) => Ok(V::Int(n)),
                    None if y.int() == 0 && matches!(op, Op::Div | Op::Mod) => refuse("division by zero"),
                    None => refuse("integer overflow"),
                };
                let ord = || x.to_bytes().cmp(&y.to_bytes());
                match op {
                    Op::Add => int(i64::checked_add)?,
                    Op::Sub => int(i64::checked_sub)?,
                    Op::Mul => int(i64::checked_mul)?,
                    Op::Div => int(i64::checked_div)?,
                    Op::Mod => int(i64::checked_rem)?,
                    Op::Eq => V::bool(x == y),
                    Op::Ne => V::bool(x != y),
                    Op::Lt => V::bool(ord() == Ordering::Less),
                    Op::Le => V::bool(ord() != Ordering::Greater),
                    Op::Gt => V::bool(ord() == Ordering::Greater),
                    _ => V::bool(ord() != Ordering::Less),
                }
            }
            Expr::Un(Un::Neg, a) => V::Int(self.eval(a)?.int().checked_neg().ok_or_else(|| Refusal::new("integer overflow"))?),
            Expr::Un(Un::Not, a) => V::bool(!self.eval(a)?.truth()),
            Expr::Std(f, es) => crate::std::apply(*f, es.iter().map(|e| self.eval(e)).collect::<Result<_, _>>()?)?,
            Expr::Call(h, es) => {
                let callee = self.callee.expect("the checker let only a run call");
                let c = callee(h).ok_or_else(|| Refusal(format!("no function {h}")))?;
                let args = es.iter().map(|e| self.eval(e)).collect::<Result<Vec<_>, _>>()?;
                // The callee runs under its own declarations, on this run's changes.
                let mut m = Machine { stack: args, changes: std::mem::take(&mut self.changes), draws: self.draws, ..self.at(&c.cx) };
                let r = m.eval(&c.f.body);
                (self.changes, self.draws) = (m.changes, m.draws);
                r?
            }
            Expr::Ctx(f) => {
                let ctx = self.ctx.expect("the checker let only a run read the context");
                match f {
                    CtxField::User => V::Bytes(ctx.actor.clone().into_bytes()),
                    CtxField::Session => V::Bytes(ctx.session.clone().into_bytes()),
                    CtxField::Roles => V::List(ctx.roles.iter().map(|r| V::Bytes(r.clone().into_bytes())).collect()),
                    CtxField::Now => V::Int(ctx.now),
                    CtxField::NewId(_) => {
                        self.draws += 1;
                        V::Bytes(ctx.new_id(self.draws - 1).to_vec())
                    }
                }
            }
            Expr::Lambda(..) => return refuse("a lambda outside an operator"),
            Expr::Scan(t) => V::List(self.rows(t).iter().map(|r| self.row(t, r)).collect::<Result<_, _>>()?),
            Expr::Lookup(t, k) => {
                let key = self.eval(k)?.to_bytes();
                match self.get(t, &key) {
                    Some(r) => V::some(self.row(t, &r)?),
                    None => V::none(),
                }
            }
            Expr::Filter(l, p) => {
                let mut out = vec![];
                for x in self.list(l)? {
                    if self.apply(p, vec![x.clone()])?.truth() {
                        out.push(x);
                    }
                }
                V::List(out)
            }
            Expr::Map(l, f) => V::List(self.list(l)?.into_iter().map(|x| self.apply(f, vec![x])).collect::<Result<_, _>>()?),
            Expr::Join(a, b, on) => {
                let (xs, ys) = (self.list(a)?, self.list(b)?);
                let mut out = vec![];
                for x in &xs {
                    for y in &ys {
                        if self.apply(on, vec![x.clone(), y.clone()])?.truth() {
                            out.push(V::Tup(vec![x.clone(), y.clone()]));
                        }
                    }
                }
                V::List(out)
            }
            Expr::Group(l, by, agg) => {
                let mut groups: BTreeMap<Vec<u8>, (V, Vec<V>)> = BTreeMap::new();
                for x in self.list(l)? {
                    let k = self.apply(by, vec![x.clone()])?;
                    groups.entry(k.to_bytes()).or_insert((k, vec![])).1.push(x);
                }
                let each = groups.into_values().map(|(k, xs)| Ok(V::Tup(vec![k.clone(), self.apply(agg, vec![k, V::List(xs)])?])));
                V::List(each.collect::<Result<_, Refusal>>()?)
            }
            Expr::Sort(l, keys, dirs) => {
                let mut keyed = vec![];
                for x in self.list(l)? {
                    let k = match (self.apply(keys, vec![x.clone()])?, dirs.len()) {
                        (V::Tup(ks), n) if n != 1 => ks.iter().map(V::to_bytes).collect(),
                        (k, _) => vec![k.to_bytes()],
                    };
                    keyed.push((k, x));
                }
                keyed.sort_by(|(a, _), (b, _)| {
                    let each = a.iter().zip(b).zip(dirs).map(|((x, y), d)| if *d == Dir::Asc { x.cmp(y) } else { y.cmp(x) });
                    each.fold(Ordering::Equal, Ordering::then)
                });
                V::List(keyed.into_iter().map(|(_, x)| x).collect())
            }
            Expr::Take(l, n) => {
                let mut xs = self.list(l)?;
                xs.truncate(self.eval(n)?.int().max(0) as usize);
                V::List(xs)
            }
            Expr::Insert(t, r) => {
                let row = self.eval(r)?.to_bytes();
                if self.get(t, &self.key_of(t, &row)).is_some() {
                    return refuse(format!("{t}: a row with this key exists"));
                }
                self.changes.push(Change::Add { table: t.clone(), row });
                unit
            }
            Expr::Update(t, k, f) => {
                let key = self.eval(k)?.to_bytes();
                let Some(old) = self.get(t, &key) else { return refuse(format!("{t}: no row has this key")) };
                let v = self.row(t, &old)?;
                let new = self.apply(f, vec![v])?.to_bytes();
                if new != old {
                    self.changes.push(Change::Edit { table: t.clone(), old, new });
                }
                unit
            }
            Expr::Delete(t, k) => {
                let key = self.eval(k)?.to_bytes();
                if let Some(row) = self.get(t, &key) {
                    self.changes.push(Change::Remove { table: t.clone(), row });
                }
                unit
            }
            Expr::Refuse(m) => return refuse(String::from_utf8_lossy(self.eval(m)?.bytes())),
            Expr::Seq(es) => {
                let mut last = unit;
                for e in es {
                    last = self.eval(e)?;
                }
                last
            }
        })
    }
}

/// A loaded function as the kernel runs it: the input parsed under its
/// parameters (one, or the tuple of several), the body evaluated, the
/// changes returned.
pub struct Interp {
    pub f: Arc<Loaded>,
    pub callee: Arc<dyn Fn(&FnHash) -> Option<Arc<Loaded>> + Send + Sync>,
}

impl Interp {
    /// The input's type: the one parameter, or the tuple of them.
    pub fn input_ty(f: &Function) -> Ty {
        match &f.params[..] {
            [t] => t.clone(),
            ts => Ty::Tuple(ts.to_vec()),
        }
    }
}

impl Procedure for Interp {
    fn run(&self, store: &dyn Read, ctx: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal> {
        let (f, cx) = (&self.f.f, &self.f.cx);
        let ty = Interp::input_ty(f);
        let v = decode(cx, &ty, parse(cx, &ty, input)?.bytes())?;
        let callee = |h: &FnHash| (self.callee)(h);
        let mut m = Machine::new(store, ctx, &callee, cx);
        m.stack = match (v, f.params.len()) {
            (V::Tup(vs), n) if n != 1 => vs,
            (v, _) => vec![v],
        };
        m.eval(&f.body)?;
        Ok(m.changes)
    }
}
