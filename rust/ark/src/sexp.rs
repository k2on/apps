//! §1.15 ArkLang, the common text form of a module: S-expressions.
//!
//! **One reader rule**, `(head args…)`. Atoms are symbols, integers,
//! strings `"…"` (`\"` and `\\` their only escapes) and bytes `0x…`; `;`
//! comments run to the end of a line. A symbol may hold `.`: `input.title`
//! is a field path when `input` is bound, else a declared name
//! (`playlists.items`, `Clock.Paused`). [`read`] reads forms and [`layout`]
//! prints one: on one line when it fits 100 columns, else its head on the
//! first line and each argument on its own, two spaces further in.
//!
//! [`build`] lowers a file through the checker to a module and its
//! functions, callees first; [`print`] writes one back — its schema, its
//! declarations, its functions by name, a blank line apart. So
//! `build(print(m)) == m` for every module and `print(build(t)) == t` for
//! text as the printer writes it. `spec/AUTHORING.md` §1 is the grammar.
//!
//! What the bytes leave out the printer names: a binder from its type (a
//! row by its table's initials, `playlist_id`, `n`) or its variant's field,
//! a field by its declaration's names or its table's columns, a tuple's by
//! position (`pair.0`). A `none` and a `(record …)` take the type their
//! place wants — a function's result, a branch's, a field's, an argument's,
//! a row's — and `(as T …)` gives a `none`, a `some` or a `list` a type
//! nothing else does. Variables are levels until a function is built
//! ([`crate::author::close`]).

use std::collections::BTreeMap;
use std::mem::take;

use crate::author::{close, levels, Domain, MARK};
use crate::check::{type_of, Env, Types};
use crate::codec::{bytes_enc, int_enc, Reader};
use crate::hash::{hex, unhex, FnHash};
use crate::interp::walk;
use crate::ir::{CtxField, Decl, Effect, Expr, Function, Node, Refine, Rule};
use crate::module::{Lib, Module, SPEC};
use crate::schema::{Column, Index, Ref, Schema, Space, Table};
use crate::store::Dir;
use crate::ty::Ty;
use crate::Refusal;

type R<T> = Result<T, Refusal>;
type At = (usize, usize);
type Want<'a> = Option<&'a Ty>;

/// A form: where it was read (line, column; `(0, 0)` when printed) and what.
#[derive(Clone, Debug)]
pub struct Sx {
    pub at: At,
    pub k: K,
}

#[derive(Clone, Debug, PartialEq)]
pub enum K {
    Sym(String),
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
    List(Vec<Sx>),
}

impl PartialEq for Sx {
    fn eq(&self, o: &Sx) -> bool {
        self.k == o.k
    }
}

fn fail<T>(at: At, what: impl std::fmt::Display) -> R<T> {
    Err(Refusal(format!("{}:{}: {what}", at.0, at.1)))
}
fn y(n: impl Into<String>) -> Sx {
    Sx {
        at: (0, 0),
        k: K::Sym(n.into()),
    }
}
fn l(xs: Vec<Sx>) -> Sx {
    Sx { at: (0, 0), k: K::List(xs) }
}
fn bx(e: Expr) -> Box<Expr> {
    Box::new(e)
}

// * The reader

/// The forms of a file.
pub fn read(src: &str) -> R<Vec<Sx>> {
    let (b, mut i, mut line, mut start) = (src.as_bytes(), 0, 1, 0);
    let mut open: Vec<(At, Vec<Sx>)> = vec![((1, 1), vec![])];
    while i < b.len() {
        let at = (line, i - start + 1);
        let k = match b[i] {
            b'\n' => {
                (line, start, i) = (line + 1, i + 1, i + 1);
                continue;
            }
            c if c.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b';' => {
                i += b[i..].iter().position(|c| *c == b'\n').unwrap_or(b.len() - i);
                continue;
            }
            b'(' => {
                open.push((at, vec![]));
                i += 1;
                continue;
            }
            b')' if open.len() == 1 => return fail(at, "a `)` without its `(`"),
            b')' => {
                i += 1;
                let (at, xs) = open.pop().unwrap();
                open.last_mut().unwrap().1.push(Sx { at, k: K::List(xs) });
                continue;
            }
            b'"' => {
                let mut s = vec![];
                i += 1;
                while b.get(i) != Some(&b'"') {
                    match (b.get(i), b.get(i + 1)) {
                        (None, _) => return fail(at, "a string without its end"),
                        (Some(b'\\'), Some(c @ (b'"' | b'\\'))) => (s.push(*c), i += 2).0,
                        (Some(b'\\'), _) => return fail((line, i - start + 1), "only `\\\"` and `\\\\` escape"),
                        (Some(c), _) => {
                            if *c == b'\n' {
                                (line, start) = (line + 1, i + 1);
                            }
                            (s.push(*c), i += 1).0
                        }
                    }
                }
                i += 1;
                K::Str(String::from_utf8(s).expect("text between ASCII quotes"))
            }
            _ => {
                let n = b[i..]
                    .iter()
                    .position(|c| c.is_ascii_whitespace() || b"()\";".contains(c))
                    .unwrap_or(b.len() - i);
                let t = &src[i..i + n];
                i += n;
                let digit = t.trim_start_matches('-').starts_with(|c: char| c.is_ascii_digit());
                match (t.strip_prefix("0x").map(unhex), t.parse()) {
                    (Some(Some(h)), _) => K::Bytes(h),
                    (Some(None), _) => return fail(at, format!("`{t}` is no bytes")),
                    (_, Ok(n)) => K::Int(n),
                    _ if digit => return fail(at, format!("`{t}` is no integer")),
                    _ => K::Sym(t.into()),
                }
            }
        };
        open.last_mut().unwrap().1.push(Sx { at, k });
    }
    match open.pop() {
        Some((_, xs)) if open.is_empty() => Ok(xs),
        Some((at, _)) => fail(at, "a `(` without its `)`"),
        None => unreachable!(),
    }
}

// * The printer

fn flat(x: &Sx) -> String {
    match &x.k {
        K::Sym(n) => n.clone(),
        K::Int(n) => n.to_string(),
        K::Str(t) => format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\"")),
        K::Bytes(b) => format!("0x{}", hex(b)),
        K::List(xs) => format!("({})", xs.iter().map(flat).collect::<Vec<_>>().join(" ")),
    }
}

/// A form laid out from column `col`: on one line when it ends by column
/// 100, else its head on the first line and each argument on its own,
/// indented two spaces past the form's `(`.
pub fn layout(x: &Sx, col: usize) -> String {
    let f = flat(x);
    match &x.k {
        K::List(xs) if !xs.is_empty() && col + f.chars().count() > 100 => {
            let rest: String = xs[1..]
                .iter()
                .map(|a| format!("\n{}{}", " ".repeat(col + 2), layout(a, col + 2)))
                .collect();
            format!("({}{rest})", layout(&xs[0], col + 1))
        }
        _ => f,
    }
}

// * Shared by the lowering and the printer

/// `(head args…)` with a symbol for its head.
fn form(x: &Sx) -> Option<(&str, &[Sx])> {
    match &x.k {
        K::List(xs) => match xs.split_first() {
            Some((Sx { k: K::Sym(h), .. }, a)) => Some((h, a)),
            _ => None,
        },
        _ => None,
    }
}
fn name(x: &Sx) -> R<&str> {
    match &x.k {
        K::Sym(n) => Ok(n),
        _ => fail(x.at, "a name expected"),
    }
}
fn items(x: &Sx) -> R<&[Sx]> {
    match &x.k {
        K::List(xs) => Ok(xs),
        _ => fail(x.at, "a list expected"),
    }
}
fn pair(x: &Sx) -> R<(&Sx, &Sx)> {
    match items(x)? {
        [a, b] => Ok((a, b)),
        _ => fail(x.at, "a pair expected"),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Mutator,
    Live,
    Query,
    Scope,
    Helper,
}
const KINDS: [(&str, Kind); 6] = [
    ("mutator", Kind::Mutator),
    ("live", Kind::Live),
    ("query", Kind::Query),
    ("scope", Kind::Scope),
    ("helper", Kind::Helper),
    ("guard", Kind::Helper),
];

/// The standard functions, in `Std`'s order (`len`, `concat` name two each).
const STD: &str = "trim is-empty concat lower is-alnum chars len starts-with split-once to-text hex min max clamp abs fnv1a64 \
    sha256 id-of-text text-of-id nil-id utf8 first last len contains reverse concat insert-at remove-at move-at slice some? ?? unwrap sum";
const OPS: &str = "+ - * / % == != < <= > >= and or";
const ATOMS: [&str; 8] = ["true", "false", "none", "ctx", "ctx.user", "ctx.session", "ctx.now", "ctx.roles"];

fn nth<X: Node>(i: usize) -> X {
    X::from_bytes(&[i as u8]).unwrap()
}
fn ord<X: Node>(x: &X) -> usize {
    x.to_bytes()[0] as usize
}

fn camel(s: &str) -> String {
    s.split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect()
}
fn snake(s: &str) -> String {
    s.chars()
        .enumerate()
        .flat_map(|(i, c)| (c.is_uppercase() && i > 0).then_some('_').into_iter().chain(c.to_lowercase()))
        .collect()
}

fn id_decl() -> Decl {
    Decl {
        ty: Ty::Bytes,
        refine: vec![Refine {
            rule: Rule::Len(16),
            why: None,
        }],
        names: vec![],
    }
}
fn option_decl(t: Ty) -> Decl {
    Decl {
        ty: Ty::option(t),
        refine: vec![],
        names: vec![],
    }
}

/// A sum's variants by name, each with its fields' names.
fn variants(d: &Decl) -> Option<Vec<(String, Vec<String>)>> {
    let Ty::Sum(vs) = &d.ty else { return None };
    let mut at = 0;
    let out = vs.iter().map(|v| {
        at += v.len() + 1;
        (d.names[at - v.len() - 1].clone(), d.names[at - v.len()..at].to_vec())
    });
    (!d.names.is_empty()).then(|| out.collect())
}

/// The one table whose rows are of type `t`, if one is.
fn table_of<'a>(rows: &'a [(Table, Ty)], t: &Ty) -> Option<&'a Table> {
    let mut it = rows.iter().filter(|r| r.1 == *t);
    match (it.next(), it.next()) {
        (Some(r), None) => Some(&r.0),
        _ => None,
    }
}

/// What `(some x)` and `(list x …)` declare by: `x`'s type with the
/// refinements on top dropped, an `Id<t>` kept — as a builder's Rust type is.
fn infer(types: &Types, t: &Ty) -> Ty {
    match t {
        Ty::Named(n) if !n.starts_with("Id<") && n != "Text" => match types.decl(n) {
            Some(d) if !d.refine.is_empty() => infer(types, &d.ty),
            _ => t.clone(),
        },
        t => t.clone(),
    }
}

/// Whether every table `e` and its callees touch is volatile; `None` when
/// it touches none.
fn volatile(schema: &Schema, lib: &Lib, e: &Expr) -> Option<bool> {
    let mut seen = None;
    walk(e, None, &mut |x, _| {
        let v = match x {
            Expr::Scan(t) | Expr::Lookup(t, _) | Expr::Insert(t, _) | Expr::Update(t, ..) | Expr::Delete(t, _) => {
                Some(schema.table(t).is_some_and(|(_, t)| t.space == Space::Volatile))
            }
            Expr::Call(h, _) => lib.get(h).and_then(|c| volatile(schema, lib, &c.f.body)),
            _ => None,
        };
        if let Some(v) = v {
            seen = Some(seen.unwrap_or(true) && v);
        }
    });
    seen
}

/// A module as it is lowered or printed: its schema, every declaration —
/// written, or made by an `Id<t>` or `Option<T>` spelled — its functions'
/// forms, those built, and while one is: the names in scope, the types it
/// stores and its callees' declarations.
#[derive(Default)]
struct Lo {
    schema: Schema,
    decls: BTreeMap<String, Decl>,
    rows: Option<Vec<(Table, Ty)>>,
    fns: BTreeMap<String, Sx>,
    built: BTreeMap<String, Option<Function>>,
    lib: Lib,
    names: BTreeMap<FnHash, String>,
    scope: Vec<(String, Ty)>,
    stored: Vec<Ty>,
    callees: BTreeMap<String, Decl>,
}

impl Lo {
    fn types(&self) -> Types {
        Types::new(&self.decls.clone().into_iter().collect::<Vec<_>>())
    }

    /// The type and effect of `e` (levels or indices) with the scope bound.
    fn type_of(&self, e: &Expr) -> R<(Ty, Effect)> {
        let stack: Vec<Ty> = self.scope.iter().map(|s| s.1.clone()).collect();
        self.type_at(&stack, &close(e.clone(), stack.len() as u32))
    }

    fn type_at(&self, stack: &[Ty], e: &Expr) -> R<(Ty, Effect)> {
        let callee = |h: &FnHash| self.lib.get(h);
        let env = Env {
            schema: &self.schema,
            callee: &callee,
        };
        type_of(&env, &self.decls.clone().into_iter().collect::<Vec<_>>(), stack, e)
    }

    fn structure(&self, t: Want) -> Option<Ty> {
        t.map(|t| self.types().structure(t))
    }
    fn elem(&self, t: Want) -> Option<Ty> {
        match self.structure(t)? {
            Ty::List(e) => Some(*e),
            _ => None,
        }
    }
    fn inner(&self, t: Want) -> Option<Ty> {
        match self.structure(t)? {
            Ty::Sum(mut vs) if vs.len() == 2 && vs[0].is_empty() && vs[1].len() == 1 => vs.remove(1).pop(),
            _ => None,
        }
    }

    fn auto(&mut self, n: String, d: Decl) -> Ty {
        if !self.decls.contains_key(&n) {
            self.decls.insert(n.clone(), d);
            self.rows = None;
        }
        Ty::Named(n)
    }

    fn option(&mut self, t: Ty) -> Ty {
        let n = format!("Option<{}>", self.spell(&t));
        self.auto(n, option_decl(t))
    }

    /// Every table and its row as the checker types it under every
    /// declaration: a column its declaration's name, else its structure.
    fn rows(&mut self) -> Vec<(Table, Ty)> {
        if let Some(r) = &self.rows {
            return r.clone();
        }
        self.rows = Some(vec![]);
        let mut out = vec![];
        for t in self.schema.tables.clone() {
            let cols = t.columns.iter().map(|c| self.dty(&c.decl).unwrap_or(Ty::unit())).collect::<Vec<_>>();
            let types = self.types();
            let cols = cols.into_iter().map(|c| if matches!(c, Ty::Named(_)) { c } else { types.deep(&c) });
            out.push((t, Ty::Tuple(cols.collect())));
        }
        self.rows = Some(out.clone());
        out
    }

    fn row(&mut self, t: &str, at: At) -> R<(Table, Ty)> {
        match self.rows().into_iter().find(|r| r.0.name == t) {
            Some(r) => Ok(r),
            None => fail(at, format!("no table {t}")),
        }
    }

    /// A type's name as the builders spell a declaration: `Option<Item>`.
    fn spell(&mut self, t: &Ty) -> String {
        let all = |me: &mut Self, ts: &[Ty]| ts.iter().map(|t| me.spell(t)).collect::<Vec<_>>().join(", ");
        let row = self.rows().into_iter().find(|r| r.1 == *t).map(|r| camel(&r.0.name));
        match t {
            Ty::Int => "Int".into(),
            Ty::Bytes => "Bytes".into(),
            Ty::Named(n) => n.clone(),
            Ty::List(e) => format!("List<{}>", self.spell(e)),
            _ if row.is_some() => row.unwrap(),
            Ty::Tuple(ts) if ts.len() == 1 => format!("({},)", self.spell(&ts[0])),
            Ty::Tuple(ts) => format!("({})", all(self, ts)),
            Ty::Sum(vs) if vs.is_empty() => "Never".into(),
            t if *t == Ty::bool() => "Bool".into(),
            Ty::Sum(vs) => format!("Sum<{}>", all(self, &vs.iter().map(|v| Ty::Tuple(v.clone())).collect::<Vec<_>>())),
        }
    }

    /// A declaration's name as a type: `Id<t>`, `Option<T>`, `List<T>`,
    /// `(A, B)`, a leaf or a declared name.
    fn dty(&mut self, s: &str) -> Result<Ty, String> {
        let inner = |p: &str| s.strip_prefix(p).and_then(|x| x.strip_suffix('>'));
        if let Some(t) = inner("Id<") {
            return Ok(t).map(|_| self.auto(s.into(), id_decl()));
        }
        if let Some(x) = inner("Option<") {
            let t = self.dty(x)?;
            return Ok(self.option(t));
        }
        if let Some(x) = inner("List<") {
            return Ok(Ty::list(self.dty(x)?));
        }
        if let Some(x) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
            let (mut parts, mut depth, mut from) = (vec![], 0, 0);
            for (i, c) in x.char_indices() {
                match c {
                    '<' | '(' => depth += 1,
                    '>' | ')' => depth -= 1,
                    ',' if depth == 0 => (parts.push(&x[from..i]), from = i + 1).0,
                    _ => {}
                }
            }
            parts.push(&x[from..]);
            let parts = parts.into_iter().map(str::trim).filter(|p| !p.is_empty());
            return Ok(Ty::Tuple(parts.map(|p| self.dty(p)).collect::<Result<_, _>>()?));
        }
        Ok(match s {
            "Int" => Ty::Int,
            "Bytes" => Ty::Bytes,
            "Bool" => Ty::bool(),
            "Never" => Ty::Sum(vec![]),
            n if n == "Text" || self.decls.contains_key(n) => Ty::Named(n.into()),
            n => return Err(format!("no type {n}")),
        })
    }

    /// The declarations `t` names, closed over their structures.
    fn declare(&self, t: &Ty, out: &mut BTreeMap<String, Decl>) {
        match t {
            Ty::Named(n) if n != "Text" && !out.contains_key(n) => {
                if let Some(d) = self.decls.get(n) {
                    out.insert(n.clone(), d.clone());
                    self.declare(&d.ty, out);
                }
            }
            Ty::List(e) => self.declare(e, out),
            Ty::Tuple(ts) => ts.iter().for_each(|t| self.declare(t, out)),
            Ty::Sum(vs) => vs.iter().flatten().for_each(|t| self.declare(t, out)),
            _ => {}
        }
    }

    fn store(&mut self, t: Ty) -> Ty {
        self.stored.push(t.clone());
        t
    }

    /// A record's or a row's field names: its declaration's, its table's columns.
    fn names_of(&mut self, t: Want) -> Option<Vec<String>> {
        let t = t?;
        if let Ty::Named(n) = t {
            return self
                .decls
                .get(n)
                .filter(|d| matches!(d.ty, Ty::Tuple(_)) && !d.names.is_empty())
                .map(|d| d.names.clone());
        }
        Some(table_of(&self.rows(), t)?.columns.iter().map(|c| c.name.clone()).collect())
    }

    /// Field `f` of a value of type `t`: a position, or a name.
    fn field(&mut self, t: &Ty, f: &str) -> Option<u32> {
        f.parse()
            .ok()
            .or_else(|| self.names_of(Some(t))?.iter().position(|n| n == f).map(|i| i as u32))
    }

    /// The arms' labels of a match on `t`: its declaration's variants,
    /// `none some`, `false true`, else positions.
    fn labels(&self, t: &Ty, n: usize) -> Vec<String> {
        let named = match t {
            Ty::Named(m) => self.decls.get(m).and_then(variants),
            _ => None,
        };
        let s = self.types().structure(t);
        match named {
            Some(vs) => vs.into_iter().map(|v| v.0).collect(),
            None if s.is_option() => vec!["none".into(), "some".into()],
            None if s == Ty::bool() => vec!["false".into(), "true".into()],
            None => (0..n).map(|i| i.to_string()).collect(),
        }
    }

    /// `T.V`: variant `V` of the declared sum `T`, and its fields' types.
    fn variant(&self, h: &str) -> Option<(Ty, u8, Vec<Ty>)> {
        let (t, v) = h.rsplit_once('.')?;
        let d = self.decls.get(t)?;
        let i = variants(d)?.iter().position(|x| x.0 == v)?;
        let Ty::Sum(vs) = &d.ty else { return None };
        Some((Ty::Named(t.into()), i as u8, vs[i].clone()))
    }
}

// * Lowering

impl Lo {
    fn ty(&mut self, x: &Sx) -> R<Ty> {
        if let K::Sym(n) = &x.k {
            return match n.contains(['<', '(', ',']) {
                false => self.dty(n).or_else(|e| fail(x.at, e)),
                true => fail(x.at, format!("`{n}` is no type")),
            };
        }
        Ok(match form(x) {
            Some(("id", [t])) => self.auto(format!("Id<{}>", name(t)?), id_decl()),
            Some(("option", [t])) => {
                let t = self.ty(t)?;
                self.option(t)
            }
            Some(("list", [t])) => Ty::list(self.ty(t)?),
            Some(("tuple", ts)) => Ty::Tuple(ts.iter().map(|t| self.ty(t)).collect::<R<_>>()?),
            Some(("row", [t])) => self.row(name(t)?, t.at)?.1,
            Some(("record" | "sum", _)) => self.decl("", x)?.ty,
            _ => return fail(x.at, "a type expected"),
        })
    }

    /// `(type NAME TY)`'s declaration: a refinement, a record (a refined
    /// field declared as `NAME.field`), a sum, or a name for a type.
    fn decl(&mut self, n: &str, x: &Sx) -> R<Decl> {
        let (mut names, mut refine) = (vec![], vec![]);
        let ty = match form(x) {
            Some(("where", [base, rules @ ..])) => {
                let ty = self.ty(base)?;
                for r in rules {
                    if let K::Str(why) = &r.k {
                        match refine.last_mut() {
                            Some(Refine { why: w @ None, .. }) => *w = Some(why.clone()),
                            _ => return fail(r.at, "words follow the rule they give"),
                        }
                        continue;
                    }
                    let rule = match form(r) {
                        Some(("utf8", [])) => Rule::Utf8,
                        Some(("trim", [])) => Rule::Trim,
                        Some(("non-empty", [])) => Rule::NonEmpty,
                        Some(("len", [Sx { k: K::Int(a), .. }])) => Rule::Len(*a),
                        Some(("len", [Sx { k: K::Int(a), .. }, b])) => match &b.k {
                            K::Int(b) => Rule::LenRange(*a, *b),
                            K::Sym(d) if d == ".." => Rule::LenRange(*a, i64::MAX),
                            _ => return fail(b.at, "a length's bound is an integer or `..`"),
                        },
                        Some(("fn", [ps, body])) => {
                            let v = name(items(ps)?.first().unwrap_or(ps))?.to_string();
                            self.scope = vec![(v, ty.clone())];
                            let e = self.ex(body, None);
                            self.scope.clear();
                            Rule::Refine(close(e?, 1))
                        }
                        _ => return fail(r.at, "a rule expected: (utf8) (trim) (non-empty) (len …) (fn (x) …)"),
                    };
                    refine.push(Refine { rule, why: None });
                }
                ty
            }
            Some(("record", fs)) => {
                let mut ts = vec![];
                for f in fs {
                    let (fname, ft) = pair(f)?;
                    names.push(name(fname)?.to_string());
                    ts.push(match form(ft) {
                        Some(("where", _)) if !n.is_empty() => {
                            let d = self.decl("", ft)?;
                            self.auto(format!("{n}.{}", name(fname)?), d)
                        }
                        _ => self.ty(ft)?,
                    });
                }
                Ty::Tuple(ts)
            }
            Some(("sum", vs)) => {
                let mut out = vec![];
                for v in vs {
                    let (vname, fs) = items(v)?.split_first().ok_or(()).or_else(|_| fail(v.at, "a variant expected"))?;
                    names.push(name(vname)?.to_string());
                    let mut ts = vec![];
                    for f in fs {
                        let (fname, ft) = pair(f)?;
                        names.push(name(fname)?.to_string());
                        ts.push(self.ty(ft)?);
                    }
                    out.push(ts);
                }
                Ty::Sum(out)
            }
            _ => self.ty(x)?,
        };
        Ok(Decl { ty, refine, names })
    }

    /// `(schema (table NAME [volatile] (COL TY [key] [(refs T)] [(default 0x…)])… (index [unique] COL…)…)…)`.
    fn tables(&mut self, ts: &[Sx]) -> R<()> {
        for x in ts {
            let Some(("table", [n, rest @ ..])) = form(x) else {
                return fail(x.at, "a table expected");
            };
            let (mut t, mut keys) = (Table::new(name(n)?), 0);
            for c in rest {
                if let K::Sym(v) = &c.k {
                    match v.as_str() {
                        "volatile" => t.space = Space::Volatile,
                        _ => return fail(c.at, format!("`{v}`: a table is volatile or not")),
                    }
                    continue;
                }
                if let Some(("index", cols)) = form(c) {
                    let unique = cols.first().is_some_and(|u| u.k == K::Sym("unique".into()));
                    let cols = cols[unique as usize..].iter().map(|col| match t.column(name(col)?) {
                        Some(i) => Ok(i),
                        None => fail(col.at, "no such column"),
                    });
                    t.indexes.push(Index {
                        columns: cols.collect::<R<_>>()?,
                        unique,
                    });
                    continue;
                }
                let [cn, ct, opts @ ..] = items(c)? else {
                    return fail(c.at, "a column expected");
                };
                let ty = self.ty(ct)?;
                let mut col = Column {
                    name: name(cn)?.into(),
                    decl: self.spell(&ty),
                    ty: Ty::Int,
                    default: None,
                };
                for o in opts {
                    match (form(o), &o.k) {
                        (_, K::Sym(k)) if k == "key" && keys == t.columns.len() => keys += 1,
                        (Some(("refs", [r])), _) => t.refs.push(Ref {
                            column: t.columns.len(),
                            table: name(r)?.into(),
                        }),
                        (Some(("default", [Sx { k: K::Bytes(b), .. }])), _) => col.default = Some(b.clone()),
                        _ => return fail(o.at, "`key` (on the leading columns), (refs T) or (default 0x…)"),
                    }
                }
                t.columns.push(col);
            }
            t.key = keys.max(1);
            self.schema.tables.push(t);
            self.rows = None;
        }
        Ok(())
    }

    /// Function `n`, built once and installed; a cycle is refused.
    fn function(&mut self, n: &str, at: At) -> R<Function> {
        match self.built.get(n) {
            Some(Some(f)) => return Ok(f.clone()),
            Some(None) => return fail(at, format!("{n} reaches itself, which a hash cannot name")),
            None => {}
        }
        let Some(x) = self.fns.get(n).cloned() else {
            return fail(at, format!("no function {n}"));
        };
        self.built.insert(n.into(), None);
        let saved = (take(&mut self.scope), take(&mut self.stored), take(&mut self.callees));
        let f = self.lower(&x);
        (self.scope, self.stored, self.callees) = saved;
        let (f, kind) = f?;
        if kind == Kind::Live && volatile(&self.schema, &self.lib, &f.body) == Some(false) {
            return fail(x.at, format!("{n}: a live function touches a durable table"));
        }
        self.lib.install(&self.schema, f.clone()).or_else(|r| fail(x.at, format!("{n}: {r}")))?;
        self.built.insert(n.into(), Some(f.clone()));
        Ok(f)
    }

    /// `(KIND NAME ((P TY)…) [RET] BODY…)`: a query, helper or guard says
    /// what it returns, a scope `Bool`, a mutator or live function `()`.
    fn lower(&mut self, x: &Sx) -> R<(Function, Kind)> {
        let Some((k, [_, ps, rest @ ..])) = form(x) else {
            return fail(x.at, "a function expected");
        };
        let kind = KINDS.iter().find(|w| w.0 == k).unwrap().1;
        for p in items(ps)? {
            let (pn, pt) = pair(p)?;
            let t = self.ty(pt)?;
            let t = self.store(t);
            self.scope.push((name(pn)?.into(), t));
        }
        let (ret, body) = match (kind, rest) {
            (Kind::Query | Kind::Helper, [r, body @ ..]) => (self.ty(r)?, body),
            (Kind::Scope, _) => (Ty::bool(), rest),
            _ => (Ty::unit(), rest),
        };
        let ret = self.store(ret);
        let body = close(self.body(body, Some(&ret), x.at)?, self.scope.len() as u32);
        let mut types = take(&mut self.callees);
        for t in take(&mut self.stored) {
            self.declare(&t, &mut types);
        }
        let params: Vec<Ty> = self.scope.iter().map(|p| p.1.clone()).collect();
        let effect = match kind {
            Kind::Mutator | Kind::Live => Effect::Write,
            Kind::Query => Effect::Read,
            Kind::Scope => Effect::Pure,
            Kind::Helper => self.type_at(&params, &body).or_else(|r| fail(x.at, r))?.1,
        };
        let types = types.into_iter().collect();
        Ok((
            Function {
                types,
                params,
                ret,
                effect,
                body,
            },
            kind,
        ))
    }

    /// A body: one form, or several in sequence, the last its value.
    fn body(&mut self, xs: &[Sx], want: Want, at: At) -> R<Expr> {
        match xs {
            [] => fail(at, "a body expected"),
            [x] => self.ex(x, want),
            [first @ .., last] => {
                let mut es = first.iter().map(|x| self.ex(x, None)).collect::<R<Vec<_>>>()?;
                es.push(self.ex(last, want)?);
                Ok(Expr::Seq(es))
            }
        }
    }

    /// `f` with `names` bound at the next levels to `tys`.
    fn under<X>(&mut self, names: Vec<String>, tys: Vec<Ty>, f: impl FnOnce(&mut Self) -> R<X>) -> R<X> {
        let depth = self.scope.len();
        self.scope.extend(names.into_iter().zip(tys));
        let r = f(self);
        self.scope.truncate(depth);
        r
    }

    /// `(fn (X…) BODY…)` over parameters of `tys`.
    fn lam(&mut self, x: &Sx, tys: Vec<Ty>, want: Want) -> R<Expr> {
        let Some(("fn", [ps, body @ ..])) = form(x) else {
            return fail(x.at, "(fn (x …) body) expected");
        };
        let names = items(ps)?.iter().map(|p| name(p).map(String::from)).collect::<R<Vec<_>>>()?;
        if names.len() != tys.len() {
            return fail(x.at, format!("{} names where {} are bound", names.len(), tys.len()));
        }
        let n = names.len() as u32;
        Ok(Expr::Lambda(n, bx(self.under(names, tys, |me| me.body(body, want, x.at))?)))
    }

    fn list_of(&self, e: &Expr, at: At) -> R<Ty> {
        let t = self.type_of(e).or_else(|r| fail(at, r))?.0;
        self.elem(Some(&t)).map_or_else(|| fail(at, "a list expected"), Ok)
    }

    /// A table's key from its arguments: one of the key's type as it is,
    /// else the tuple of them.
    fn key(&mut self, t: &str, ks: &[Sx], at: At) -> R<Expr> {
        let (tab, row) = self.row(t, at)?;
        let Ty::Tuple(cols) = row else { unreachable!() };
        let es = ks.iter().map(|k| self.ex(k, None)).collect::<R<Vec<_>>>()?;
        match &es[..] {
            [e] if self.types().sub(&self.type_of(e)?.0, &Ty::Tuple(cols[..tab.key].to_vec())) => Ok(e.clone()),
            _ => Ok(Expr::Tuple(es)),
        }
    }

    /// A field path: `e.a.b`.
    fn path(&mut self, mut e: Expr, fs: &[&str], at: At) -> R<Expr> {
        for f in fs {
            let t = self.type_of(&e).or_else(|r| fail(at, r))?.0;
            let Some(i) = self.field(&t, f) else {
                return fail(at, format!("no field {f}"));
            };
            e = Expr::Field(bx(e), i);
        }
        Ok(e)
    }

    fn ex(&mut self, x: &Sx, want: Want) -> R<Expr> {
        let at = x.at;
        let n = match &x.k {
            K::Int(n) => return Ok(Expr::Lit(Ty::Int, int_enc(*n).to_vec())),
            K::Str(s) => return Ok(Expr::Lit(Types::text(), bytes_enc(s.as_bytes()))),
            K::Bytes(b) => return Ok(Expr::Lit(Ty::Bytes, bytes_enc(b))),
            K::List(xs) if xs.is_empty() => return Ok(Expr::Seq(vec![])),
            K::List(_) => return self.call(x, want),
            K::Sym(n) => n.as_str(),
        };
        let segs: Vec<&str> = n.split('.').collect();
        if let Some(l) = self.scope.iter().rposition(|s| s.0 == segs[0]) {
            return self.path(Expr::Var(MARK + l as u32), &segs[1..], at);
        }
        Ok(match n {
            "true" | "false" => Expr::Lit(Ty::bool(), vec![(n == "true") as u8]),
            "none" => match want.filter(|w| self.inner(Some(w)).is_some()) {
                Some(w) => Expr::Variant(self.store(w.clone()), 0, vec![]),
                None => return fail(at, "a none no place types: (as (option T) none)"),
            },
            "ctx.user" => Expr::Ctx(CtxField::User),
            "ctx.session" => Expr::Ctx(CtxField::Session),
            "ctx.now" => Expr::Ctx(CtxField::Now),
            "ctx.roles" => Expr::Ctx(CtxField::Roles),
            n if self.schema.table(n).is_some() => Expr::Scan(n.into()),
            n => return fail(at, format!("`{n}` is no variable, table or value")),
        })
    }

    fn call(&mut self, x: &Sx, want: Want) -> R<Expr> {
        use Expr::*;
        let at = x.at;
        let Some((h, a)) = form(x) else {
            return fail(at, "a form begins with a name");
        };
        let arity = |n: usize| {
            if a.len() == n {
                Ok(())
            } else {
                fail(at, format!("({h} …) takes {n}"))
            }
        };
        let e1 = |me: &mut Self, w: Want| -> R<Expr> {
            arity(1)?;
            me.ex(&a[0], w)
        };
        let table = |i: usize| a.get(i).map_or_else(|| fail(at, "a table expected"), |t| name(t).map(String::from));
        Ok(match h {
            "let" => {
                let Some((bs, body)) = a.split_first() else {
                    return fail(at, "(let ((x e) …) body …)");
                };
                return self.lets(items(bs)?, body, want, at);
            }
            "if" if a.len() == 2 || a.len() == 3 => {
                let c = self.ex(&a[0], None)?;
                let b = match a.get(2) {
                    Some(b) => self.ex(b, want)?,
                    None => Seq(vec![]),
                };
                If(bx(c), bx(self.ex(&a[1], want)?), bx(b))
            }
            "match" if !a.is_empty() => {
                let s = self.ex(&a[0], None)?;
                let t = self.type_of(&s).or_else(|r| fail(at, r))?.0;
                let Ty::Sum(vs) = self.types().structure(&t) else {
                    return fail(at, "a match on no sum");
                };
                let labels = self.labels(&t, vs.len());
                if a.len() != vs.len() + 1 {
                    return fail(at, format!("{} arms for {} variants", a.len() - 1, vs.len()));
                }
                let mut arms = vec![];
                for ((fields, label), arm) in vs.into_iter().zip(labels).zip(&a[1..]) {
                    let [v, bs, body @ ..] = items(arm)? else {
                        return fail(arm.at, "(V (x …) body …) expected");
                    };
                    if flat(v) != label {
                        return fail(arm.at, format!("arm {} where {label} is due", flat(v)));
                    }
                    let names = items(bs)?.iter().map(|p| name(p).map(String::from)).collect::<R<Vec<_>>>()?;
                    if names.len() != fields.len() {
                        return fail(arm.at, format!("{label} has {} fields", fields.len()));
                    }
                    let k = fields.len() as u32;
                    arms.push(Lambda(k, bx(self.under(names, fields, |me| me.body(body, want, arm.at))?)));
                }
                Match(bx(s), arms)
            }
            "record" => {
                let Some(ns) = self.names_of(want).filter(|ns| ns.len() == a.len()) else {
                    return fail(at, "a record no place types by its names: (tuple …)");
                };
                let Some(Ty::Tuple(fts)) = self.structure(want) else { unreachable!() };
                let mut es = vec![];
                for ((f, n), ft) in a.iter().zip(ns).zip(fts) {
                    let (fname, fe) = pair(f)?;
                    if name(fname)? != n {
                        return fail(f.at, format!("field {} where {n} is due", name(fname)?));
                    }
                    es.push(self.ex(fe, Some(&ft))?);
                }
                Tuple(es)
            }
            "tuple" => {
                if self.names_of(want).is_some_and(|ns| ns.len() == a.len()) {
                    return fail(at, "a record is written (record (field e) …)");
                }
                let fts = match self.structure(want) {
                    Some(Ty::Tuple(ts)) if ts.len() == a.len() => ts.into_iter().map(Some).collect(),
                    _ => vec![None; a.len()],
                };
                Tuple(a.iter().zip(fts).map(|(e, t)| self.ex(e, t.as_ref())).collect::<R<_>>()?)
            }
            "some" => {
                let x = e1(self, self.inner(want).as_ref())?;
                let t = infer(&self.types(), &self.type_of(&x).or_else(|r| fail(at, r))?.0);
                let t = self.option(t);
                Variant(self.store(t), 1, vec![x])
            }
            "list" => {
                let w = self.elem(want);
                let es = a.iter().map(|e| self.ex(e, w.as_ref())).collect::<R<Vec<_>>>()?;
                let Some(first) = es.first() else {
                    return fail(at, "an empty list no place types: (as (list T) (list))");
                };
                let t = infer(&self.types(), &self.type_of(first).or_else(|r| fail(at, r))?.0);
                List(self.store(t), es)
            }
            "as" if a.len() == 2 => {
                let t = self.ty(&a[0])?;
                let t = self.store(t);
                match (&a[1].k, form(&a[1])) {
                    (K::Sym(n), _) if n == "none" && self.inner(Some(&t)).is_some() => Variant(t, 0, vec![]),
                    (_, Some(("some", [v]))) if self.inner(Some(&t)).is_some() => {
                        Variant(t.clone(), 1, vec![self.ex(v, self.inner(Some(&t)).as_ref())?])
                    }
                    (_, Some(("list", es))) if self.elem(Some(&t)).is_some() => {
                        let e = self.elem(Some(&t)).unwrap();
                        List(e.clone(), es.iter().map(|x| self.ex(x, Some(&e))).collect::<R<_>>()?)
                    }
                    _ => return fail(at, "(as T e) types a none, a (some …) or a (list …) of T"),
                }
            }
            "none?" => Un(crate::ir::Un::Not, bx(Std(crate::ir::Std::IsSome, vec![e1(self, None)?]))),
            "not" => Un(crate::ir::Un::Not, bx(e1(self, None)?)),
            "-" if a.len() == 1 => Un(crate::ir::Un::Neg, bx(e1(self, None)?)),
            o if OPS.split(' ').any(|p| p == o) => {
                arity(2)?;
                let i = OPS.split(' ').position(|p| p == o).unwrap();
                Op(nth(i), bx(self.ex(&a[0], None)?), bx(self.ex(&a[1], None)?))
            }
            "refuse" => Refuse(bx(e1(self, None)?)),
            "get" | "delete" if !a.is_empty() => {
                let (t, k) = (table(0)?, self.key(&table(0)?, &a[1..], at)?);
                if h == "get" {
                    Lookup(t, bx(k))
                } else {
                    Delete(t, bx(k))
                }
            }
            "insert" => {
                arity(2)?;
                let row = self.row(&table(0)?, at)?.1;
                Insert(table(0)?, bx(self.ex(&a[1], Some(&row))?))
            }
            "update" if a.len() >= 2 => {
                let (t, row) = (table(0)?, self.row(&table(0)?, at)?.1);
                let k = self.key(&t, &a[1..a.len() - 1], at)?;
                Update(t, bx(k), bx(self.lam(&a[a.len() - 1], vec![row.clone()], Some(&row))?))
            }
            "filter" | "map" if a.len() == 2 => {
                let l = self.ex(&a[0], None)?;
                let f = self.lam(&a[1], vec![self.list_of(&l, at)?], None)?;
                if h == "map" {
                    Map(bx(l), bx(f))
                } else {
                    Filter(bx(l), bx(f))
                }
            }
            "join" if a.len() == 3 => {
                let (p, q) = (self.ex(&a[0], None)?, self.ex(&a[1], None)?);
                let tys = vec![self.list_of(&p, at)?, self.list_of(&q, at)?];
                Join(bx(p), bx(q), bx(self.lam(&a[2], tys, None)?))
            }
            "group" if a.len() == 3 => {
                let g = self.ex(&a[0], None)?;
                let x = self.list_of(&g, at)?;
                let by = self.lam(&a[1], vec![x.clone()], None)?;
                let Lambda(_, b) = &by else { unreachable!() };
                let stack: Vec<Ty> = self.scope.iter().map(|s| s.1.clone()).chain([x.clone()]).collect();
                let k = self
                    .type_at(&stack, &close((**b).clone(), stack.len() as u32))
                    .or_else(|r| fail(at, r))?
                    .0;
                Group(bx(g), bx(by), bx(self.lam(&a[2], vec![k, Ty::list(x)], None)?))
            }
            "sort" if a.len() == 2 => {
                let s = self.ex(&a[0], None)?;
                let x = self.list_of(&s, at)?;
                let Some(("fn", [ps, kb])) = form(&a[1]) else {
                    return fail(a[1].at, "(fn (x) keys) expected");
                };
                let mut dirs = vec![];
                let v = vec![name(items(ps)?.first().unwrap_or(ps))?.to_string()];
                let body = self.under(v, vec![x], |me| {
                    let key = |me: &mut Self, k: &Sx| match form(k) {
                        Some(("desc", [e])) => Ok((Dir::Desc, me.ex(e, None)?)),
                        Some(("asc", [e])) => Ok((Dir::Asc, me.ex(e, None)?)),
                        _ => Ok((Dir::Asc, me.ex(k, None)?)),
                    };
                    match form(kb) {
                        Some(("tuple", ks)) => {
                            let ks = ks.iter().map(|k| key(me, k)).collect::<R<Vec<_>>>()?;
                            dirs = ks.iter().map(|k| k.0).collect();
                            Ok(Tuple(ks.into_iter().map(|k| k.1).collect()))
                        }
                        _ => key(me, kb).map(|(d, e)| (dirs.push(d), e).1),
                    }
                })?;
                Sort(bx(s), bx(Lambda(1, bx(body))), dirs)
            }
            "take" if a.len() == 2 => Take(bx(self.ex(&a[0], None)?), bx(self.ex(&a[1], None)?)),
            "new-id" => {
                arity(1)?;
                let t = self.auto(format!("Id<{}>", table(0)?), id_decl());
                self.store(t);
                Ctx(CtxField::NewId(table(0)?))
            }
            "has-role" => Std(crate::ir::Std::Contains, vec![Ctx(CtxField::Roles), e1(self, None)?]),
            "lit" => match a {
                [t, Sx { k: K::Bytes(b), .. }] => Lit(self.ty(t).map(|t| self.store(t))?, b.clone()),
                _ => return fail(at, "(lit T 0x…)"),
            },
            "variant" => match a {
                [t, Sx { k: K::Int(i), .. }, es @ ..] => {
                    let t = self.ty(t)?;
                    Variant(self.store(t), *i as u8, es.iter().map(|e| self.ex(e, None)).collect::<R<_>>()?)
                }
                _ => return fail(at, "(variant T i e …)"),
            },
            "." if !a.is_empty() => {
                let e = self.ex(&a[0], None)?;
                let fs = a[1..].iter().map(name).collect::<R<Vec<_>>>()?;
                self.path(e, &fs, at)?
            }
            s if STD.split(' ').any(|n| n == s) => {
                let es = a.iter().map(|e| self.ex(e, None)).collect::<R<Vec<_>>>()?;
                let text = match es.first() {
                    Some(e) => self.types().structure(&self.type_of(e).or_else(|r| fail(at, r))?.0) == Ty::Bytes,
                    None => false,
                };
                let f = match s {
                    "len" if text => crate::ir::Std::TextLen,
                    "len" => crate::ir::Std::Len,
                    "concat" if es.len() == 1 => crate::ir::Std::Concat,
                    "concat" => crate::ir::Std::ListConcat,
                    _ => nth(STD.split(' ').position(|n| n == s).unwrap()),
                };
                Std(f, es)
            }
            f if self.fns.contains_key(f) => {
                let callee = self.function(f, at)?;
                self.callees.extend(callee.types.iter().cloned());
                if callee.params.len() != a.len() {
                    return fail(at, format!("{f} takes {}", callee.params.len()));
                }
                Call(
                    callee.hash(),
                    a.iter().zip(&callee.params).map(|(e, p)| self.ex(e, Some(p))).collect::<R<_>>()?,
                )
            }
            v if self.variant(v).is_some() => {
                let (t, i, fts) = self.variant(v).unwrap();
                if fts.len() != a.len() {
                    return fail(at, format!("{v} has {} fields", fts.len()));
                }
                Variant(self.store(t), i, a.iter().zip(&fts).map(|(e, f)| self.ex(e, Some(f))).collect::<R<_>>()?)
            }
            _ => return fail(at, format!("({h} …) is no form, function or variant")),
        })
    }

    /// `(let ((x e) …) body …)`: each bound for the rest.
    fn lets(&mut self, bs: &[Sx], body: &[Sx], want: Want, at: At) -> R<Expr> {
        let Some((b, rest)) = bs.split_first() else {
            return self.body(body, want, at);
        };
        let (n, v) = pair(b)?;
        let v = self.ex(v, None)?;
        let t = self.type_of(&v).or_else(|r| fail(b.at, r))?.0;
        let inner = self.under(vec![name(n)?.into()], vec![t], |me| me.lets(rest, body, want, at))?;
        Ok(Expr::Let(bx(v), bx(inner)))
    }
}

// * Printing

impl Lo {
    /// A fresh name for a value of `t`: `prefer`, else from its type; unlike
    /// any name in scope, any table, any function's first segment, an atom.
    fn fresh(&mut self, t: &Ty, prefer: Option<&String>) -> String {
        let rows = self.rows();
        let stem = match (prefer, t, table_of(&rows, t)) {
            (Some(p), ..) => p.clone(),
            (_, _, Some(tab)) => tab.name.split('_').map(|w| &w[..1]).collect(),
            (_, Ty::Named(n), _) if n.starts_with("Id<") => format!("{}_id", &n[3..n.len() - 1]),
            (_, Ty::Named(n), _) if n.starts_with("Option<") => "opt".into(),
            (_, Ty::Named(n), _) => snake(n.rsplit('.').next().unwrap()),
            (_, Ty::Int, _) => "n".into(),
            (_, Ty::List(_), _) => "xs".into(),
            (_, t, _) if *t == Ty::bool() => "b".into(),
            _ => "x".into(),
        };
        let taken = |n: &str| {
            ATOMS.contains(&n)
                || self.scope.iter().any(|s| s.0 == n)
                || self.schema.table(n).is_some()
                || self.names.values().any(|f| f.split('.').next() == Some(n))
        };
        let n = (1..)
            .map(|i| if i == 1 { stem.clone() } else { format!("{stem}{i}") })
            .find(|n| !taken(n))
            .unwrap();
        self.scope.push((n.clone(), t.clone()));
        n
    }

    fn pty(&mut self, t: &Ty) -> Sx {
        let rows = self.rows();
        match t {
            Ty::Int => y("Int"),
            Ty::Bytes => y("Bytes"),
            Ty::Named(n) if n.starts_with("Id<") && self.decls.get(n) == Some(&id_decl()) => l(vec![y("id"), y(&n[3..n.len() - 1])]),
            Ty::Named(n) => match self.decls.get(n).cloned().and_then(|d| Some((d.clone(), self.inner(Some(&d.ty))?))) {
                Some((d, i)) if *n == format!("Option<{}>", self.spell(&i)) && d == option_decl(i.clone()) => l(vec![y("option"), self.pty(&i)]),
                _ => y(n),
            },
            Ty::List(e) => l(vec![y("list"), self.pty(e)]),
            t if table_of(&rows, t).is_some() => l(vec![y("row"), y(&table_of(&rows, t).unwrap().name)]),
            t if *t == Ty::bool() => y("Bool"),
            Ty::Sum(vs) if vs.is_empty() => y("Never"),
            Ty::Tuple(ts) => l([y("tuple")].into_iter().chain(ts.iter().map(|t| self.pty(t))).collect()),
            Ty::Sum(vs) => {
                let v = |me: &mut Self, (i, fs): (usize, &Vec<Ty>)| {
                    let fs = fs.iter().enumerate().map(|(j, f)| l(vec![y(format!("f{j}")), me.pty(f)]));
                    l([y(format!("v{i}"))].into_iter().chain(fs.collect::<Vec<_>>()).collect())
                };
                l([y("sum")]
                    .into_iter()
                    .chain(vs.iter().enumerate().map(|x| v(self, x)).collect::<Vec<_>>())
                    .collect())
            }
        }
    }

    /// `(type N TY)`: a refinement, a record (its refined fields inline), a
    /// sum, a name for a type.
    fn pdecl(&mut self, n: &str, d: &Decl) -> R<Sx> {
        let mut out = vec![];
        if !d.refine.is_empty() {
            out.extend([y("where"), self.pty(&d.ty)]);
            for r in &d.refine {
                out.push(match &r.rule {
                    Rule::Utf8 => l(vec![y("utf8")]),
                    Rule::Trim => l(vec![y("trim")]),
                    Rule::NonEmpty => l(vec![y("non-empty")]),
                    Rule::Len(a) => l(vec![y("len"), y(a.to_string())]),
                    Rule::LenRange(a, i64::MAX) => l(vec![y("len"), y(a.to_string()), y("..")]),
                    Rule::LenRange(a, b) => l(vec![y("len"), y(a.to_string()), y(b.to_string())]),
                    Rule::Refine(e) => {
                        let v = self.fresh(&d.ty, None);
                        let b = self.pr(e, None);
                        self.scope.clear();
                        l(vec![y("fn"), l(vec![y(v)]), b?])
                    }
                });
                out.extend(r.why.clone().map(|w| Sx { at: (0, 0), k: K::Str(w) }));
            }
        } else if let (Ty::Tuple(ts), false) = (&d.ty, d.names.is_empty()) {
            out.push(y("record"));
            for (f, t) in d.names.iter().zip(ts) {
                let inline = self
                    .decls
                    .get(&format!("{n}.{f}"))
                    .filter(|_| *t == Ty::Named(format!("{n}.{f}")))
                    .cloned();
                let t = match inline {
                    Some(d) => self.pdecl("", &d)?,
                    None => self.pty(t),
                };
                out.push(l(vec![y(f), t]));
            }
        } else if let (Some(vs), Ty::Sum(ts)) = (variants(d), &d.ty) {
            out.push(y("sum"));
            for ((v, fs), ts) in vs.into_iter().zip(ts) {
                let fs = fs.into_iter().zip(ts).map(|(f, t)| l(vec![y(f), self.pty(t)])).collect::<Vec<_>>();
                out.push(l([y(v)].into_iter().chain(fs).collect()));
            }
        } else {
            return Ok(self.pty(&d.ty));
        }
        Ok(l(out))
    }

    /// `e`'s forms as a body: a sequence's each, else itself.
    fn forms(&mut self, e: &Expr, want: Want) -> R<Vec<Sx>> {
        match e {
            Expr::Seq(es) if es.len() > 1 => {
                let mut out = es[..es.len() - 1].iter().map(|x| self.pr(x, None)).collect::<R<Vec<_>>>()?;
                out.push(self.pr(&es[es.len() - 1], want)?);
                Ok(out)
            }
            e => Ok(vec![self.pr(e, want)?]),
        }
    }

    /// `(fn (x …) body)` over `tys`, its names from them.
    fn plam(&mut self, e: &Expr, tys: &[Ty], want: Want) -> R<Sx> {
        let Expr::Lambda(_, b) = e else {
            return Err(Refusal::new("a lambda expected"));
        };
        let depth = self.scope.len();
        let ns = tys.iter().map(|t| y(self.fresh(t, None))).collect();
        let body = self.pr(b, want);
        self.scope.truncate(depth);
        Ok(l(vec![y("fn"), l(ns), body?]))
    }

    fn pkey(&mut self, t: &str, k: &Expr) -> R<Vec<Sx>> {
        let (tab, Ty::Tuple(cols)) = self.row(t, (0, 0))? else { unreachable!() };
        match k {
            Expr::Tuple(es) if es.len() != 1 || !self.types().sub(&self.type_of(&es[0])?.0, &Ty::Tuple(cols[..tab.key].to_vec())) => {
                es.iter().map(|e| self.pr(e, None)).collect()
            }
            k => Ok(vec![self.pr(k, None)?]),
        }
    }

    fn pr(&mut self, e: &Expr, want: Want) -> R<Sx> {
        use Expr::*;
        let all =
            |me: &mut Self, h: Sx, es: &[Expr]| -> R<Sx> { Ok(l([Ok(h)].into_iter().chain(es.iter().map(|e| me.pr(e, None))).collect::<R<_>>()?)) };
        let depth = self.scope.len();
        let out = match e {
            Lit(t, b) => {
                let mut r = Reader::new(b);
                Sx {
                    at: (0, 0),
                    k: match t {
                        Ty::Int => K::Int(r.int()?),
                        Ty::Named(n) if n == "Text" => K::Str(r.text()?),
                        Ty::Bytes => K::Bytes(r.bytes()?),
                        t if *t == Ty::bool() => K::Sym(["false", "true"][b[0] as usize].into()),
                        t => {
                            return Ok(l(vec![
                                y("lit"),
                                self.pty(t),
                                Sx {
                                    at: (0, 0),
                                    k: K::Bytes(b.clone()),
                                },
                            ]))
                        }
                    },
                }
            }
            Var(i) => y(&self
                .scope
                .iter()
                .rev()
                .nth(*i as usize)
                .ok_or_else(|| Refusal::new("an unbound variable"))?
                .0),
            Let(..) => {
                let (mut binds, mut e) = (vec![], e);
                while let Let(v, b) = e {
                    let t = self.type_of(v)?.0;
                    let v = self.pr(v, None)?;
                    binds.push(l(vec![y(self.fresh(&t, None)), v]));
                    e = b;
                }
                l([y("let"), l(binds)].into_iter().chain(self.forms(e, want)?).collect())
            }
            Seq(es) if es.is_empty() => l(vec![]),
            Seq(es) if es.len() == 1 => return Err(Refusal::new("a sequence of one has no spelling")),
            Seq(_) => l([y("let"), l(vec![])].into_iter().chain(self.forms(e, want)?).collect()),
            If(c, a, b) => {
                let mut out = vec![y("if"), self.pr(c, None)?, self.pr(a, want)?];
                if **b != Seq(vec![]) {
                    out.push(self.pr(b, want)?);
                }
                l(out)
            }
            Match(s, arms) => {
                let t = self.type_of(s)?.0;
                let Ty::Sum(vs) = self.types().structure(&t) else {
                    return Err(Refusal::new("a match on no sum"));
                };
                let fnames = match &t {
                    Ty::Named(n) => self.decls.get(n).and_then(variants).map(|v| v.into_iter().map(|v| v.1).collect()),
                    _ => None,
                }
                .unwrap_or_else(|| vec![vec![]; vs.len()]);
                let mut out = vec![y("match"), self.pr(s, None)?];
                for (((label, fs), arm), names) in self.labels(&t, vs.len()).into_iter().zip(&vs).zip(arms).zip(fnames) {
                    let label = label.parse().map_or_else(|_| y(&label), |i| Sx { at: (0, 0), k: K::Int(i) });
                    let ns = fs.iter().enumerate().map(|(j, f)| y(self.fresh(f, names.get(j)))).collect();
                    out.push(l([label, l(ns)].into_iter().chain(self.forms(arm, want)?).collect()));
                    self.scope.truncate(depth);
                }
                l(out)
            }
            Field(..) => {
                let (mut fs, mut e) = (vec![], e);
                while let Field(x, i) = e {
                    let t = self.type_of(x)?.0;
                    let n = self.names_of(Some(&t)).and_then(|ns| ns.get(*i as usize).cloned());
                    fs.push(n.filter(|n| self.field(&t, n) == Some(*i)).unwrap_or(i.to_string()));
                    e = x;
                }
                fs.reverse();
                match e {
                    Var(_) => y(format!("{}.{}", flat(&self.pr(e, None)?), fs.join("."))),
                    _ => l([y("."), self.pr(e, None)?].into_iter().chain(fs.into_iter().map(y)).collect()),
                }
            }
            Tuple(es) => match (self.names_of(want).filter(|ns| ns.len() == es.len()), self.structure(want)) {
                (Some(ns), Some(Ty::Tuple(ts))) => {
                    let fs = ns.into_iter().zip(es).zip(ts).map(|((n, e), t)| Ok(l(vec![y(n), self.pr(e, Some(&t))?])));
                    l([Ok(y("record"))].into_iter().chain(fs).collect::<R<_>>()?)
                }
                (_, Some(Ty::Tuple(ts))) if ts.len() == es.len() => l([Ok(y("tuple"))]
                    .into_iter()
                    .chain(es.iter().zip(&ts).map(|(e, t)| self.pr(e, Some(t))))
                    .collect::<R<_>>()?),
                _ => all(self, y("tuple"), es)?,
            },
            Variant(t, i, es) => {
                let n = match t {
                    Ty::Named(n) => n.clone(),
                    _ => String::new(),
                };
                let opt = self.decls.get(&n).zip(self.inner(Some(t))).is_some_and(|(d, i)| *d == option_decl(i));
                let named = self.decls.get(&n).and_then(variants);
                match (opt, *i, named) {
                    (true, 0, _) if want == Some(t) => y("none"),
                    (true, 0, _) => l(vec![y("as"), self.pty(t), y("none")]),
                    (true, ..) => {
                        let it = infer(&self.types(), &self.type_of(&es[0])?.0);
                        match Ty::Named(format!("Option<{}>", self.spell(&it))) == *t {
                            true => l(vec![y("some"), self.pr(&es[0], self.inner(want).as_ref())?]),
                            false => l(vec![
                                y("as"),
                                self.pty(t),
                                l(vec![y("some"), self.pr(&es[0], self.inner(Some(t)).as_ref())?]),
                            ]),
                        }
                    }
                    (_, i, Some(vs)) if !self.names.values().any(|f| *f == format!("{n}.{}", vs[i as usize].0)) => {
                        let Ty::Sum(fts) = self.types().structure(t) else { unreachable!() };
                        let head = y(format!("{n}.{}", vs[i as usize].0));
                        l([Ok(head)]
                            .into_iter()
                            .chain(es.iter().zip(&fts[i as usize]).map(|(e, f)| self.pr(e, Some(f))))
                            .collect::<R<_>>()?)
                    }
                    _ => {
                        let head = vec![
                            y("variant"),
                            self.pty(t),
                            Sx {
                                at: (0, 0),
                                k: K::Int(*i as i64),
                            },
                        ];
                        l(head.into_iter().map(Ok).chain(es.iter().map(|e| self.pr(e, None))).collect::<R<_>>()?)
                    }
                }
            }
            List(t, es) => {
                let natural = match es.first() {
                    Some(x) => Some(infer(&self.types(), &self.type_of(x)?.0)),
                    None => None,
                };
                let w = if natural.as_ref() == Some(t) { self.elem(want) } else { Some(t.clone()) };
                let list = l([Ok(y("list"))]
                    .into_iter()
                    .chain(es.iter().map(|e| self.pr(e, w.as_ref())))
                    .collect::<R<_>>()?);
                match natural.as_ref() == Some(t) {
                    true => list,
                    false => l(vec![y("as"), self.pty(&Ty::list(t.clone())), list]),
                }
            }
            Op(o, a, b) => all(self, y(OPS.split(' ').nth(ord(o)).unwrap()), &[(**a).clone(), (**b).clone()])?,
            Un(crate::ir::Un::Not, a) => match &**a {
                Std(crate::ir::Std::IsSome, x) => all(self, y("none?"), x)?,
                a => all(self, y("not"), std::slice::from_ref(a))?,
            },
            Un(_, a) => all(self, y("-"), std::slice::from_ref(&**a))?,
            Std(crate::ir::Std::Contains, es) if es.first() == Some(&Ctx(CtxField::Roles)) => all(self, y("has-role"), &es[1..])?,
            Std(f, es) => all(self, y(STD.split(' ').nth(ord(f)).unwrap()), es)?,
            Call(h, es) => {
                let params = self.lib.get(h).ok_or_else(|| Refusal(format!("no function {h}")))?.f.params.clone();
                let n = self.names.get(h).cloned().ok_or_else(|| Refusal(format!("no name for {h}")))?;
                l([Ok(y(n))]
                    .into_iter()
                    .chain(es.iter().zip(&params).map(|(e, p)| self.pr(e, Some(p))))
                    .collect::<R<_>>()?)
            }
            Ctx(CtxField::NewId(t)) => l(vec![y("new-id"), y(t)]),
            Ctx(c) => y(format!("ctx.{}", snake(&format!("{c:?}")))),
            Lambda(..) => return Err(Refusal::new("a lambda outside an operator")),
            Scan(t) => y(t),
            Lookup(t, k) | Delete(t, k) => {
                let h = y(if matches!(e, Lookup(..)) { "get" } else { "delete" });
                l([h, y(t)].into_iter().chain(self.pkey(t, k)?).collect())
            }
            Insert(t, r) => {
                let row = self.row(t, (0, 0))?.1;
                l(vec![y("insert"), y(t), self.pr(r, Some(&row))?])
            }
            Update(t, k, f) => {
                let row = self.row(t, (0, 0))?.1;
                let f = self.plam(f, std::slice::from_ref(&row), Some(&row))?;
                l([y("update"), y(t)].into_iter().chain(self.pkey(t, k)?).chain([f]).collect())
            }
            Filter(s, f) | Map(s, f) => {
                let x = self.list_of(s, (0, 0))?;
                let h = if matches!(e, Filter(..)) { "filter" } else { "map" };
                l(vec![y(h), self.pr(s, None)?, self.plam(f, &[x], None)?])
            }
            Join(a, b, on) => {
                let (x, z) = (self.list_of(a, (0, 0))?, self.list_of(b, (0, 0))?);
                l(vec![y("join"), self.pr(a, None)?, self.pr(b, None)?, self.plam(on, &[x, z], None)?])
            }
            Group(g, by, agg) => {
                let x = self.list_of(g, (0, 0))?;
                let Lambda(_, kb) = &**by else {
                    return Err(Refusal::new("a lambda expected"));
                };
                self.scope.push((String::new(), x.clone()));
                let k = self.type_of(kb).map(|t| t.0);
                self.scope.pop();
                let (b, a) = (self.plam(by, std::slice::from_ref(&x), None)?, self.plam(agg, &[k?, Ty::list(x)], None)?);
                l(vec![y("group"), self.pr(g, None)?, b, a])
            }
            Sort(s, keys, dirs) => {
                let x = self.list_of(s, (0, 0))?;
                let Lambda(_, kb) = &**keys else {
                    return Err(Refusal::new("a lambda expected"));
                };
                let v = self.fresh(&x, None);
                let key = |me: &mut Self, k: &Expr, d: &Dir, bare: bool| -> R<Sx> {
                    let k = me.pr(k, None)?;
                    Ok(match (d, bare) {
                        (Dir::Desc, _) => l(vec![y("desc"), k]),
                        (_, false) => l(vec![y("asc"), k]),
                        _ => k,
                    })
                };
                let body = match (&**kb, &dirs[..]) {
                    (Tuple(ks), ds) if ds.len() > 1 && ks.len() == ds.len() => l([Ok(y("tuple"))]
                        .into_iter()
                        .chain(ks.iter().zip(ds).map(|(k, d)| key(self, k, d, true)))
                        .collect::<R<_>>()?),
                    (k, [d]) => key(self, k, d, !matches!(k, Tuple(_)))?,
                    _ => return Err(Refusal::new("sort keys that are no tuple of their directions")),
                };
                self.scope.truncate(depth);
                l(vec![y("sort"), self.pr(s, None)?, l(vec![y("fn"), l(vec![y(v)]), body])])
            }
            Take(s, n) => all(self, y("take"), &[(**s).clone(), (**n).clone()])?,
            Refuse(m) => all(self, y("refuse"), std::slice::from_ref(&**m))?,
        };
        self.scope.truncate(depth);
        Ok(out)
    }

    /// `(KIND NAME ((P TY)…) [RET] BODY…)`, its kind by its effect: `live`
    /// when every table it reaches is volatile, `scope` when it is a
    /// predicate of a row of any table, `helper` for what else returns.
    fn pfn(&mut self, n: &str, f: &Function) -> R<Sx> {
        let rows: Vec<Vec<Ty>> = self.rows().into_iter().map(|r| vec![r.1]).collect();
        let scoped = f.params.len() == 1 && self.types().structure(&f.params[0]) == Ty::Sum(rows) && f.ret == Ty::bool();
        let kind = match f.effect {
            Effect::Write if f.ret == Ty::unit() && volatile(&self.schema, &self.lib, &f.body) == Some(true) => "live",
            Effect::Write if f.ret == Ty::unit() => "mutator",
            Effect::Read => "query",
            Effect::Pure if scoped => "scope",
            _ => "helper",
        };
        self.scope.clear();
        let ps = f.params.iter().map(|t| l(vec![y(self.fresh(t, None)), self.pty(t)])).collect();
        let mut out = vec![y(kind), y(n), l(ps)];
        if ["query", "helper"].contains(&kind) {
            out.push(self.pty(&f.ret));
        }
        out.extend(self.forms(&f.body, Some(&f.ret))?);
        self.scope.clear();
        Ok(l(out))
    }
}

// * The module

/// §1.15 A module from its text: the schema, the declarations, every
/// function checked (callees first) and bound by its name unless the name
/// starts with `_`; the module's declarations are its bound functions'.
pub fn build(src: &str) -> R<Domain> {
    let mut lo = Lo::default();
    let (mut tables, mut types) = (vec![], vec![]);
    for x in read(src)? {
        match form(&x) {
            Some(("schema", ts)) => tables.extend(ts.iter().cloned()),
            Some(("type", [n, _])) => {
                lo.decls.insert(
                    name(n)?.into(),
                    Decl {
                        ty: Ty::unit(),
                        refine: vec![],
                        names: vec![],
                    },
                );
                types.push(x.clone());
            }
            Some((k, [n, ..])) if KINDS.iter().any(|w| w.0 == k) => {
                let n = name(n)?.to_string();
                if lo.fns.insert(n.clone(), x.clone()).is_some() {
                    return fail(x.at, format!("{n} is defined twice"));
                }
            }
            _ => return fail(x.at, "a schema, a type or a function expected"),
        }
    }
    lo.tables(&tables)?;
    // Twice: a row a declaration names is read under the others.
    for _ in 0..2 {
        for x in &types {
            let Some((_, [n, t])) = form(x) else { unreachable!() };
            let d = lo.decl(name(n)?, t)?;
            lo.decls.insert(name(n)?.into(), d);
            lo.rows = None;
        }
    }
    let mut cols = vec![];
    for c in lo
        .schema
        .tables
        .iter()
        .flat_map(|t| &t.columns)
        .map(|c| c.decl.clone())
        .collect::<Vec<_>>()
    {
        cols.push(lo.dty(&c).map_err(Refusal)?);
    }
    let all = lo.types();
    for (c, t) in lo.schema.tables.iter_mut().flat_map(|t| &mut t.columns).zip(cols) {
        c.ty = all.deep(&t);
    }
    lo.rows = None;
    let (mut bindings, mut decls) = (vec![], BTreeMap::new());
    for (n, x) in lo.fns.clone() {
        let f = lo.function(&n, x.at)?;
        if !n.starts_with('_') {
            bindings.push((n.clone(), f.hash()));
            decls.extend(f.types.iter().cloned());
        }
    }
    let bodies = lo.built.values().flatten().map(|f| (f.hash(), f.to_bytes())).collect();
    let module = Module {
        spec: SPEC,
        schema: lo.schema.clone(),
        types: decls.into_iter().collect(),
        bindings,
    };
    Ok(Domain {
        schema: std::sync::Arc::new(lo.schema),
        module,
        lib: lo.lib,
        bodies,
    })
}

/// A fragment's expression, its type, and the declarations it makes.
pub type Fragment = (Expr, Ty, Vec<(String, Decl)>);

/// One form as an expression in a builder's function (§1.15): `vars` its
/// free names, by level, with their types; the levels after them its own
/// binders, which begin at `depth`.
pub fn fragment(src: &str, schema: &Schema, decls: &[(String, Decl)], vars: &[(&str, Expr, Ty)], depth: u32) -> R<Fragment> {
    let [x] = &read(src)?[..] else {
        return Err(Refusal::new("a fragment is one form"));
    };
    let mut lo = Lo {
        schema: schema.clone(),
        decls: decls.iter().cloned().collect(),
        scope: vars.iter().map(|v| (v.0.to_string(), v.2.clone())).collect(),
        ..Lo::default()
    };
    let e = lo.ex(x, None)?;
    let t = lo.type_of(&e)?.0;
    let k = vars.len() as u32;
    let e = levels(e, &|l| {
        if l < k {
            vars[l as usize].1.clone()
        } else {
            Expr::Var(MARK + depth + l - k)
        }
    });
    let mut d = BTreeMap::new();
    for t in take(&mut lo.stored) {
        lo.declare(&t, &mut d);
    }
    Ok((e, t, d.into_iter().collect()))
}

/// A module's printing context: its declarations, the columns' too.
fn printing(m: &Module, lib: Lib) -> Lo {
    let mut lo = Lo {
        schema: m.schema.clone(),
        decls: m.types.iter().cloned().collect(),
        lib,
        ..Lo::default()
    };
    for c in m.schema.tables.iter().flat_map(|t| &t.columns) {
        if lo.dty(&c.decl).is_err() {
            lo.decls.insert(
                c.decl.clone(),
                Decl {
                    ty: c.ty.clone(),
                    refine: vec![],
                    names: vec![],
                },
            );
        }
    }
    for (n, h) in &m.bindings {
        lo.names.entry(*h).or_insert_with(|| n.clone());
    }
    lo
}

/// §1.15 A module as text: its schema, the declarations no `Id<t>` or
/// `Option<T>` makes alone, then every function — bound ones by name,
/// others as `_` and their hash — sorted.
pub fn print(m: &Module, functions: &[Function]) -> R<String> {
    let mut lo = printing(m, Lib::default());
    let mut todo: Vec<&Function> = functions.iter().collect();
    while !todo.is_empty() {
        let before = todo.len();
        todo.retain(|f| lo.lib.install(&m.schema, (*f).clone()).is_err());
        if todo.len() == before {
            return Err(Refusal::new("a function that does not check"));
        }
    }
    for f in functions {
        lo.names.entry(f.hash()).or_insert_with(|| format!("_{}", &f.hash().to_string()[..16]));
    }
    let mut out = vec![];
    if !m.schema.tables.is_empty() {
        let mut tables = vec![y("schema")];
        for t in &m.schema.tables {
            let mut x = vec![y("table"), y(&t.name)];
            x.extend((t.space == Space::Volatile).then(|| y("volatile")));
            for (i, c) in t.columns.iter().enumerate() {
                let ty = lo.dty(&c.decl).map_err(Refusal)?;
                let mut col = vec![y(&c.name), lo.pty(&ty)];
                col.extend((i < t.key).then(|| y("key")));
                col.extend(t.refs.iter().filter(|r| r.column == i).map(|r| l(vec![y("refs"), y(&r.table)])));
                col.extend(c.default.clone().map(|d| l(vec![y("default"), Sx { at: (0, 0), k: K::Bytes(d) }])));
                x.push(l(col));
            }
            for ix in &t.indexes {
                let mut i = vec![y("index")];
                i.extend(ix.unique.then(|| y("unique")));
                i.extend(ix.columns.iter().map(|c| y(&t.columns[*c].name)));
                x.push(l(i));
            }
            tables.push(l(x));
        }
        out.push(l(tables));
    }
    let decls = lo.decls.clone();
    let inline = |n: &str| {
        n.rsplit_once('.')
            .is_some_and(|(r, f)| decls.get(r).is_some_and(|d| d.names.iter().any(|x| x == f)))
    };
    for (n, d) in &decls {
        let auto = (n.starts_with("Id<") && *d == id_decl()) || (n.starts_with("Option<") && lo.pty(&Ty::Named(n.clone())) != y(n));
        if !auto && !inline(n) {
            out.push(l(vec![y("type"), y(n), lo.pdecl(n, d)?]));
        }
    }
    let mut fns: Vec<(String, &Function)> = functions.iter().map(|f| (lo.names[&f.hash()].clone(), f)).collect();
    for (n, h) in &m.bindings {
        if lo.names[h] != *n {
            fns.extend(functions.iter().filter(|f| f.hash() == *h).map(|f| (n.clone(), f)));
        }
    }
    fns.sort_by(|a, b| a.0.cmp(&b.0));
    for (n, f) in fns {
        out.push(lo.pfn(&n, f)?);
    }
    Ok(out.iter().map(|x| layout(x, 0)).collect::<Vec<_>>().join("\n\n") + "\n")
}

/// One function of a module as text, for a browser of it: `name` its binding.
pub fn print_function(m: &Module, lib: &Lib, name: &str) -> R<String> {
    let mut lo = printing(m, lib.clone());
    let h = m.get(name).ok_or_else(|| Refusal(format!("no function {name}")))?;
    let f = lib.get(&h).ok_or_else(|| Refusal::new("not loaded"))?;
    Ok(layout(&lo.pfn(name, &f.f)?, 0))
}
