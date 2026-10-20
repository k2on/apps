//! §1.15 Ark text: one canonical spelling of a module, and its compiler.
//!
//! [`build`] compiles text to a module and its functions, each checked;
//! [`print`] writes a module as text. One printer, so `build(print(m))` is
//! `m` and `print(build(t))` is `t` for canonical text. The grammar is
//! `spec/AUTHORING.md` §1; in short:
//!
//! ```text
//! schema { [volatile] table t { col: Type [key] [refs t] [default x"…"]  index [unique] (a, b) } }
//! type Name = Text where trim, non_empty "a playlist needs a name"
//! mutator demo.rename(id: Id<playlist>, name: Name) = playlist.update(id, p => { name: name, ..p })
//! ```
//!
//! What the module's bytes do not hold the text names canonically: a
//! binder from its type, a row's field by its table's column, a refined
//! record field by its declaration (`AddSong.title`), anything else by
//! position (`x.2`, `#1(a, b)`). A function's declarations are those its
//! stored types name, closed over their structures, and its callees'; its
//! effect is its kind's bound, or the least its body has for a `helper`.
//!
//! Both front ends — this one and the builders (`crate::author`) — build
//! with variables as **levels** (`Var(MARK + level)`) and a match arm as a
//! `Lambda` of its fields, and [`close`] turns that into De Bruijn once.

use std::collections::{BTreeMap, BTreeSet};

use crate::author::Domain;
use crate::check::{type_of, Env, Types};
use crate::codec::{bytes_enc, int_enc, Reader};
use crate::hash::{hex, FnHash};
use crate::ir::{CtxField, Decl, Effect, Expr, Function, Node, Op, Refine, Rule, Std, Un};
use crate::module::{Lib, Module, SPEC};
use crate::schema::{Column, Index, Ref, Schema, Space, Table};
use crate::store::Dir;
use crate::ty::Ty;
use crate::Refusal;

type R<T> = Result<T, Refusal>;

/// A variable at a level, before [`close`].
pub const MARK: u32 = 1 << 31;

fn bx(e: Expr) -> Box<Expr> {
    Box::new(e)
}

/// `e` with each expression directly under it through `f`, told how
/// many binders `e` puts it under.
fn kids(e: Expr, f: &mut dyn FnMut(Expr, u32) -> Expr) -> Expr {
    use Expr::*;
    let mut b = |x: Box<Expr>, n: u32| bx(f(*x, n));
    match e {
        Let(v, x) => Let(b(v, 0), b(x, 1)),
        Lambda(n, x) => Lambda(n, b(x, n)),
        Field(a, i) => Field(b(a, 0), i),
        Op(o, x, y) => Op(o, b(x, 0), b(y, 0)),
        Un(o, a) => Un(o, b(a, 0)),
        Lookup(t, k) => Lookup(t, b(k, 0)),
        Filter(x, y) => Filter(b(x, 0), b(y, 0)),
        Map(x, y) => Map(b(x, 0), b(y, 0)),
        Sort(x, y, ds) => Sort(b(x, 0), b(y, 0), ds),
        Take(x, y) => Take(b(x, 0), b(y, 0)),
        Insert(t, a) => Insert(t, b(a, 0)),
        Update(t, x, y) => Update(t, b(x, 0), b(y, 0)),
        Delete(t, a) => Delete(t, b(a, 0)),
        Refuse(a) => Refuse(b(a, 0)),
        If(x, y, z) => If(b(x, 0), b(y, 0), b(z, 0)),
        Join(x, y, z) => Join(b(x, 0), b(y, 0), b(z, 0)),
        Group(x, y, z) => Group(b(x, 0), b(y, 0), b(z, 0)),
        Match(x, es) => Match(b(x, 0), es.into_iter().map(|e| f(e, 0)).collect()),
        Tuple(es) => Tuple(es.into_iter().map(|e| f(e, 0)).collect()),
        Variant(t, i, es) => Variant(t, i, es.into_iter().map(|e| f(e, 0)).collect()),
        List(t, es) => List(t, es.into_iter().map(|e| f(e, 0)).collect()),
        Std(g, es) => Std(g, es.into_iter().map(|e| f(e, 0)).collect()),
        Call(h, es) => Call(h, es.into_iter().map(|e| f(e, 0)).collect()),
        Seq(es) => Seq(es.into_iter().map(|e| f(e, 0)).collect()),
        e => e,
    }
}

/// Levels to De Bruijn indices at `d` binders, and every match arm built
/// as a `Lambda` of its fields back to its body.
pub fn close(e: Expr, d: u32) -> Expr {
    match e {
        Expr::Var(v) if v >= MARK => Expr::Var(d.checked_sub(v - MARK + 1).expect("a variable used outside its binder")),
        Expr::Match(s, arms) => {
            let arm = |a| match a {
                Expr::Lambda(k, b) => close(*b, d + k),
                a => close(a, d),
            };
            Expr::Match(bx(close(*s, d)), arms.into_iter().map(arm).collect())
        }
        e => kids(e, &mut |x, n| close(x, d + n)),
    }
}

/// Every variable at a level through `f`.
fn levels(e: Expr, f: &dyn Fn(u32) -> Expr) -> Expr {
    match e {
        Expr::Var(v) if v >= MARK => f(v - MARK),
        e => kids(e, &mut |x, _| levels(x, f)),
    }
}

/// The standard functions as methods, by name, in `Std`'s order (`len`
/// and `concat` each name two, told apart by receiver and arity).
const STD: &str = "trim is_empty concat lower is_alnum chars len starts_with split_once to_text hex min max clamp abs fnv1a64 \
    sha256 id_of_text text_of_id nil_id utf8 first last len contains reverse concat insert remove_at move_at slice is_some \
    unwrap_or unwrap sum";

/// The operators, in `Op`'s order, and each one's precedence.
const OPS: &str = "+ - * / % == != < <= > >= and or";
const LEVEL: [u8; 13] = [5, 5, 6, 6, 6, 4, 4, 4, 4, 4, 4, 2, 1];

const KEYWORDS: &str = "schema table volatile key refs default index unique type where mutator query scope guard helper live let \
    if else match refuse ctx not and or desc asc true false none some lit nil_id";

/// The `i`th of an enum the codec reads by its variant's index.
fn nth<X: Node>(i: usize) -> X {
    X::from_bytes(&[i as u8]).unwrap()
}

/// An operator's spelling and precedence.
fn op(o: Op) -> (&'static str, u8) {
    let i = o.to_bytes()[0] as usize;
    (OPS.split(' ').nth(i).unwrap(), LEVEL[i])
}

fn keyword(n: &str) -> bool {
    KEYWORDS.split_whitespace().any(|k| k == n)
}

/// A type's canonical spelling, which is also a declaration's name.
pub fn spell(t: &Ty) -> String {
    spell_in(t, &[])
}

/// A type's spelling where a tuple that is a table's row is its row's
/// name (`PlaylistItem`), as an `Option` of one is declared.
fn spell_in(t: &Ty, rows: &[(String, Ty)]) -> String {
    let all = |ts: &[Ty]| ts.iter().map(|t| spell_in(t, rows)).collect::<Vec<_>>().join(", ");
    match t {
        Ty::Int => "Int".into(),
        Ty::Bytes => "Bytes".into(),
        Ty::Named(n) => n.clone(),
        Ty::List(e) => format!("List<{}>", spell_in(e, rows)),
        t if rows.iter().any(|r| r.1 == *t) => rows.iter().find(|r| r.1 == *t).unwrap().0.clone(),
        Ty::Tuple(ts) if ts.len() == 1 => format!("({},)", all(ts)),
        Ty::Tuple(ts) => format!("({})", all(ts)),
        Ty::Sum(vs) if vs.is_empty() => "Never".into(),
        t if *t == Ty::bool() => "Bool".into(),
        Ty::Sum(vs) => format!(
            "Sum<{}>",
            vs.iter().map(|v| spell_in(&Ty::Tuple(v.clone()), rows)).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

fn camel(s: &str) -> String {
    s.split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect()
}

/// The declaration an `Id<t>` or `Option<T>` is without one written.
fn auto(name: &str, d: &Ty, rows: &[(String, Ty)]) -> Option<Decl> {
    let opt = match d {
        Ty::Sum(vs) if d.is_option() => Some(vs[1][0].clone()),
        _ => None,
    };
    match opt {
        _ if name.starts_with("Id<") && name.ends_with('>') => Some(Decl {
            ty: Ty::Bytes,
            refine: vec![Refine {
                rule: Rule::Len(16),
                why: None,
            }],
        }),
        Some(t) if name == format!("Option<{}>", spell_in(&t, rows)) => Some(Decl {
            ty: d.clone(),
            refine: vec![],
        }),
        _ => None,
    }
}

/// What `[x]` and `some(x)` declare their element by: `x`'s type with the
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

/// The one table whose rows are of type `t` as the checker types them
/// (`rows`, each table's), if one is.
fn table_of<'a>(rows: &'a [(Table, Ty)], t: &Ty) -> Option<&'a Table> {
    let mut it = rows.iter().filter(|r| r.1 == *t);
    match (it.next(), it.next()) {
        (Some(r), None) => Some(&r.0),
        _ => None,
    }
}

/// Field `name` of a value of type `t`: a position, a refined field's
/// declaration (`R.f`), or the column of the one table rows of `t` are.
fn field(rows: &[(Table, Ty)], types: &Types, t: &Ty, name: &str) -> Option<u32> {
    if let Ok(i) = name.parse() {
        return Some(i);
    }
    if let (Ty::Named(r), Ty::Tuple(ts)) = (t, types.structure(t)) {
        if let Some(i) = ts.iter().position(|x| *x == Ty::Named(format!("{r}.{name}"))) {
            return Some(i as u32);
        }
    }
    table_of(rows, t)?.column(name).map(|i| i as u32)
}

// * Tokens

#[derive(Clone, Debug, PartialEq)]
enum T {
    Id(String),
    Int(i128),
    Str(String),
    Hex(Vec<u8>),
    P(&'static str),
    End,
}

#[derive(Clone, Debug)]
struct Tok {
    t: T,
    line: usize,
    /// First on its line.
    nl: bool,
}

const PUNCT: [&str; 26] = [
    "..=", "=>", "->", "==", "!=", "<=", ">=", "..", "(", ")", "{", "}", "[", "]", "<", ">", ",", ":", ".", "=", "+", "-", "*", "/", "%", "#",
];

fn lex(src: &str) -> R<Vec<Tok>> {
    let (b, mut i, mut line, mut out) = (src.as_bytes(), 0, 1, vec![]);
    let mut nl = true;
    let err = |line, what: &str| Refusal(format!("line {line}: {what}"));
    while i < b.len() {
        let c = b[i] as char;
        if c == '\n' {
            (line, nl, i) = (line + 1, true, i + 1);
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if src[i..].starts_with("//") {
            i += src[i..].find('\n').unwrap_or(src.len() - i);
            continue;
        }
        let t = if c == '"' || (c == 'x' && b.get(i + 1) == Some(&b'"')) {
            i += if c == 'x' { 2 } else { 1 };
            let mut s = String::new();
            let mut it = src[i..].char_indices();
            loop {
                let (k, ch) = it.next().ok_or_else(|| err(line, "a string without its end"))?;
                match ch {
                    '"' => {
                        i += k + 1;
                        break;
                    }
                    '\\' => s.push(match it.next().map(|x| x.1) {
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some('0') => '\0',
                        Some('u') => {
                            let rest: String = it.by_ref().map(|x| x.1).take_while(|&c| c != '}').collect();
                            let n = u32::from_str_radix(rest.trim_start_matches('{'), 16).ok();
                            n.and_then(char::from_u32).ok_or_else(|| err(line, "a bad \\u escape"))?
                        }
                        Some(ch) => ch,
                        None => return Err(err(line, "a string without its end")),
                    }),
                    ch => s.push(ch),
                }
            }
            match c {
                'x' => T::Hex(crate::hash::unhex(&s).ok_or_else(|| err(line, "bad hex"))?),
                _ => T::Str(s),
            }
        } else if c.is_ascii_digit() {
            let n = src[i..].find(|c: char| !c.is_ascii_digit()).unwrap_or(src.len() - i);
            let v = src[i..i + n].parse().map_err(|_| err(line, "an int too long"))?;
            i += n;
            T::Int(v)
        } else if c.is_alphabetic() || c == '_' {
            let n = src[i..].find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(src.len() - i);
            i += n;
            T::Id(src[i - n..i].into())
        } else {
            let p = PUNCT
                .iter()
                .find(|p| src[i..].starts_with(**p))
                .ok_or_else(|| err(line, &format!("`{c}`")))?;
            i += p.len();
            T::P(p)
        };
        out.push(Tok { t, line, nl });
        nl = false;
    }
    out.push(Tok { t: T::End, line, nl: true });
    Ok(out)
}

// * Compiling

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Mutator,
    Live,
    Query,
    Scope,
    Helper,
}

/// The module being compiled: its tokens, schema, declarations (written
/// and automatic) and functions, built on first use so callees come first.
#[derive(Default)]
struct M {
    toks: Vec<Tok>,
    schema: Schema,
    decls: BTreeMap<String, Decl>,
    written: BTreeSet<String>,
    fns: BTreeMap<String, (Kind, usize)>,
    built: BTreeMap<String, Option<Function>>,
    lib: Lib,
    rows: Option<Vec<(String, Ty)>>,
}

/// A cursor over `m`'s tokens, compiling one function's body or a
/// fragment: the names bound by level, the declarations typed under.
struct P<'m> {
    m: &'m mut M,
    i: usize,
    /// Whether a newline ends an expression here: in a block, not inside
    /// brackets.
    lines: Vec<bool>,
    scope: Vec<(String, Ty)>,
    decls: BTreeMap<String, Decl>,
    stored: Vec<Ty>,
    callees: BTreeMap<String, Decl>,
}

impl M {
    /// Over a schema and declarations already made.
    fn of(schema: &Schema, decls: &[(String, Decl)]) -> M {
        M {
            schema: schema.clone(),
            decls: decls.iter().cloned().collect(),
            written: decls.iter().map(|d| d.0.clone()).collect(),
            ..M::default()
        }
    }

    fn err(&self, i: usize, what: impl std::fmt::Display) -> Refusal {
        Refusal(format!("line {}: {what}", self.toks[i.min(self.toks.len() - 1)].line))
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

    /// Every table's row type, by its row's name.
    fn rows(&mut self) -> R<Vec<(String, Ty)>> {
        if let Some(r) = &self.rows {
            return Ok(r.clone());
        }
        self.rows = Some(vec![]);
        let mut rows = vec![];
        for t in self.schema.tables.clone() {
            rows.push((camel(&t.name), self.row(&t)?));
        }
        self.rows = Some(rows.clone());
        Ok(rows)
    }

    /// A table's row: each column as its declared type's spelling reads.
    fn row(&mut self, t: &Table) -> R<Ty> {
        let cols = t
            .columns
            .iter()
            .map(|c| lex(&c.decl).and_then(|toks| self.ty_of(toks)))
            .collect::<R<_>>()?;
        Ok(Ty::Tuple(cols))
    }

    fn ty_of(&mut self, toks: Vec<Tok>) -> R<Ty> {
        let saved = std::mem::replace(&mut self.toks, toks);
        let r = P::new(self, 0).ty();
        self.toks = saved;
        r
    }

    /// Function `name`, built and installed once; a cycle is refused.
    fn function(&mut self, name: &str, at: usize) -> R<Function> {
        match self.built.get(name) {
            Some(Some(f)) => return Ok(f.clone()),
            Some(None) => return Err(self.err(at, format!("{name} reaches itself, which a hash cannot name"))),
            None => {}
        }
        let &(kind, start) = self.fns.get(name).ok_or_else(|| self.err(at, format!("no function {name}")))?;
        self.built.insert(name.into(), None);
        let f = P::new(self, start).function(kind)?;
        if kind == Kind::Live && volatile(&self.schema, &self.lib, &f.body) == Some(false) {
            return Err(self.err(at, format!("{name}: a live function touches a durable table")));
        }
        self.lib.install(&self.schema, f.clone()).map_err(|r| Refusal(format!("{name}: {r}")))?;
        self.built.insert(name.into(), Some(f.clone()));
        Ok(f)
    }
}

impl<'m> P<'m> {
    fn new(m: &'m mut M, i: usize) -> P<'m> {
        P {
            m,
            i,
            lines: vec![false],
            scope: vec![],
            decls: BTreeMap::new(),
            stored: vec![],
            callees: BTreeMap::new(),
        }
    }

    fn tok(&self) -> &T {
        &self.m.toks[self.i].t
    }

    fn peek(&self, k: usize) -> &T {
        &self.m.toks[(self.i + k).min(self.m.toks.len() - 1)].t
    }

    fn err(&self, what: impl std::fmt::Display) -> Refusal {
        self.m.err(self.i, what)
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.tok(), T::P(q) if *q == p) || matches!(self.tok(), T::Id(q) if q == p)
    }

    fn eat(&mut self, p: &str) -> bool {
        let y = self.is(p);
        self.i += y as usize;
        y
    }

    fn want(&mut self, p: &str) -> R<()> {
        match self.eat(p) {
            true => Ok(()),
            false => Err(self.err(format!("`{p}` expected, not {:?}", self.tok()))),
        }
    }

    fn name(&mut self) -> R<String> {
        match self.tok().clone() {
            T::Id(n) if !keyword(&n) => {
                self.i += 1;
                Ok(n)
            }
            t => Err(self.err(format!("a name expected, not {t:?}"))),
        }
    }

    /// Any word, a keyword too: what follows a `.`, a column's name.
    fn word(&mut self) -> R<String> {
        match self.tok().clone() {
            T::Id(n) => {
                self.i += 1;
                Ok(n)
            }
            t => Err(self.err(format!("a name expected, not {t:?}"))),
        }
    }

    /// `a.b.c`.
    fn path(&mut self) -> R<String> {
        let mut n = self.name()?;
        while self.is(".") && matches!(self.peek(1), T::Id(_)) {
            self.i += 1;
            n = format!("{n}.{}", self.word()?);
        }
        Ok(n)
    }

    fn int(&mut self) -> R<i64> {
        let neg = self.eat("-");
        match *self.tok() {
            T::Int(n) => {
                self.i += 1;
                i64::try_from(if neg { -n } else { n }).map_err(|_| self.err("an int beyond i64"))
            }
            _ => Err(self.err("an int expected")),
        }
    }

    /// Breaks before a token first on its line, where newlines end things.
    fn ends(&self) -> bool {
        *self.lines.last().unwrap() && self.m.toks[self.i].nl
    }

    fn inside<X>(&mut self, lines: bool, f: impl FnOnce(&mut Self) -> R<X>) -> R<X> {
        self.lines.push(lines);
        let r = f(self);
        self.lines.pop();
        r
    }

    /// Comma-separated items until `close`.
    fn list<X>(&mut self, close: &str, mut f: impl FnMut(&mut Self) -> R<X>) -> R<Vec<X>> {
        self.inside(false, |p| {
            let mut out = vec![];
            while !p.eat(close) {
                out.push(f(p)?);
                if !p.is(close) {
                    p.want(",")?;
                }
            }
            Ok(out)
        })
    }

    // * Types

    fn ty(&mut self) -> R<Ty> {
        if self.eat("(") {
            let ts = self.list(")", |p| p.ty())?;
            return Ok(match ts.len() {
                1 if !matches!(self.m.toks[self.i - 2].t, T::P(",")) => ts[0].clone(),
                _ => Ty::Tuple(ts),
            });
        }
        let n = self.path()?;
        let arg = |p: &mut Self| -> R<Ty> {
            p.want("<")?;
            let t = p.ty()?;
            p.want(">")?;
            Ok(t)
        };
        Ok(match n.as_str() {
            "Int" => Ty::Int,
            "Bytes" => Ty::Bytes,
            "Bool" => Ty::bool(),
            "Never" => Ty::Sum(vec![]),
            "List" => Ty::list(arg(self)?),
            "Sum" => {
                self.want("<")?;
                let vs = self.inside(false, |p| {
                    let mut vs = vec![];
                    while !p.eat(">") {
                        match p.ty()? {
                            Ty::Tuple(ts) => vs.push(ts),
                            t => vs.push(vec![t]),
                        }
                        p.eat(",");
                    }
                    Ok(vs)
                })?;
                Ty::Sum(vs)
            }
            "Id" | "Option" => {
                let inner = match n.as_str() {
                    "Id" => {
                        self.want("<")?;
                        let t = self.name()?;
                        self.want(">")?;
                        Ty::Named(t)
                    }
                    _ => arg(self)?,
                };
                let name = format!("{n}<{}>", spell_in(&inner, &self.m.rows()?));
                self.auto(&name, Ty::option(inner))?;
                Ty::Named(name)
            }
            _ if n == "Text" || self.m.written.contains(&n) => Ty::Named(n),
            _ => match self.m.schema.tables.iter().find(|t| camel(&t.name) == n).cloned() {
                Some(t) => self.m.row(&t)?,
                None => Ty::Named(n),
            },
        })
    }

    /// A declaration's name: a path, or `Id<t>`, `Option<T>`.
    fn tname(&mut self) -> R<String> {
        match self.tok() {
            T::Id(n) if n == "Id" || n == "Option" => {
                let t = self.ty()?;
                Ok(spell_in(&t, &self.m.rows()?))
            }
            _ => self.path(),
        }
    }

    /// `trim`, `len 1..=80 "words"`, `v => v >= 0`.
    fn rule(&mut self, ty: &Ty) -> R<Refine> {
        let rule = match self.tok().clone() {
            T::Id(n) if matches!(self.peek(1), T::P("=>")) => {
                self.i += 2;
                self.scope.push((n, ty.clone()));
                let e = self.expr();
                self.scope.pop();
                Rule::Refine(close(e?, 1))
            }
            T::Id(n) => {
                self.i += 1;
                match n.as_str() {
                    "utf8" => Rule::Utf8,
                    "trim" => Rule::Trim,
                    "non_empty" => Rule::NonEmpty,
                    "len" => {
                        let lo = if self.is("..=") { 0 } else { self.int()? };
                        match (self.eat(".."), self.eat("..=")) {
                            (true, _) => Rule::LenRange(lo, i64::MAX),
                            (_, true) => Rule::LenRange(lo, self.int()?),
                            _ => Rule::Len(lo),
                        }
                    }
                    _ => return Err(self.err(format!("no rule `{n}`"))),
                }
            }
            t => return Err(self.err(format!("a rule expected, not {t:?}"))),
        };
        let why = match self.tok().clone() {
            T::Str(s) => {
                self.i += 1;
                Some(s)
            }
            _ => None,
        };
        Ok(Refine { rule, why })
    }

    // * Functions

    /// `kind path(params) [-> R] (= expr | { … })`, compiled until its
    /// declarations settle: they are what its names are typed under.
    fn function(&mut self, kind: Kind) -> R<Function> {
        let start = self.i;
        let (mut decls, mut tries) = (BTreeMap::new(), 0);
        loop {
            self.i = start;
            self.path()?;
            self.want("(")?;
            let params = self.list(")", |p| {
                let n = p.name()?;
                p.want(":")?;
                Ok((n, p.ty()?))
            })?;
            let ret = if self.eat("->") { self.ty()? } else { Ty::unit() };
            self.decls = decls.clone();
            self.scope = params.clone();
            self.stored = vec![];
            for t in params.iter().map(|p| p.1.clone()).chain([ret.clone()]) {
                self.store(t);
            }
            self.callees.clear();
            let body = match self.eat("=") {
                true => self.inside(true, |p| p.expr()),
                false => self.want("{").and_then(|_| self.block()),
            };
            // A name typed before a callee's declarations were known may
            // fail to check: again, under what was learnt.
            let body = match body {
                Err(_) if self.decls != decls && tries < 8 => {
                    (decls, tries) = (self.decls.clone(), tries + 1);
                    continue;
                }
                b => close(b?, params.len() as u32),
            };
            let mut now = self.callees.clone();
            for t in &self.stored {
                self.m.declare(t, &mut now);
            }
            if now != decls && tries < 8 {
                (decls, tries) = (now, tries + 1);
                continue;
            }
            let types: Vec<_> = decls.into_iter().collect();
            let pts: Vec<Ty> = params.into_iter().map(|p| p.1).collect();
            let effect = match kind {
                Kind::Mutator | Kind::Live => Effect::Write,
                Kind::Query => Effect::Read,
                Kind::Scope => Effect::Pure,
                Kind::Helper => self.type_of_at(&types, &pts, &body)?.1,
            };
            return Ok(Function {
                types,
                params: pts,
                ret,
                effect,
                body,
            });
        }
    }

    fn type_of_at(&self, decls: &[(String, Decl)], stack: &[Ty], e: &Expr) -> R<(Ty, Effect)> {
        let lib = &self.m.lib;
        let callee = |h: &FnHash| lib.get(h);
        type_of(
            &Env {
                schema: &self.m.schema,
                callee: &callee,
            },
            decls,
            stack,
            e,
        )
        .map_err(|r| self.err(r))
    }

    /// The type of an expression built here, its levels closed.
    fn type_of(&self, e: &Expr) -> R<Ty> {
        let stack: Vec<Ty> = self.scope.iter().map(|s| s.1.clone()).collect();
        Ok(self.type_of_at(&self.decl_list(), &stack, &close(e.clone(), stack.len() as u32))?.0)
    }

    /// Every table, and its row as the checker types it here.
    fn trows(&self) -> R<Vec<(Table, Ty)>> {
        let ts = self.m.schema.tables.clone();
        ts.into_iter()
            .map(|t| Ok((t.clone(), self.elem(&self.type_of(&Expr::Scan(t.name))?)?)))
            .collect()
    }

    fn decl_list(&self) -> Vec<(String, Decl)> {
        self.decls.clone().into_iter().collect()
    }

    fn types(&self) -> Types {
        Types::new(&self.decl_list())
    }

    /// An `Id<t>` or `Option<T>` declared as it is when none is written.
    fn auto(&mut self, name: &str, d: Ty) -> R<()> {
        if !self.m.written.contains(name) {
            let d = auto(name, &d, &self.m.rows()?).unwrap();
            self.m.decls.insert(name.into(), d);
        }
        Ok(())
    }

    /// `t`, stored in the function: what it declares, now and after.
    fn store(&mut self, t: Ty) -> Ty {
        let mut d = std::mem::take(&mut self.decls);
        self.m.declare(&t, &mut d);
        self.decls = d;
        self.stored.push(t.clone());
        t
    }

    fn var(&self, n: &str) -> Option<Expr> {
        self.scope.iter().rposition(|s| s.0 == n).map(|l| Expr::Var(MARK + l as u32))
    }

    /// `body` with `names` bound to `tys` at the next levels.
    fn bind(&mut self, names: Vec<String>, tys: Vec<Ty>, body: impl FnOnce(&mut Self) -> R<Expr>) -> R<Expr> {
        if names.len() != tys.len() {
            return Err(self.err(format!("{} names for {} values", names.len(), tys.len())));
        }
        let depth = self.scope.len();
        self.scope.extend(names.into_iter().zip(tys));
        let r = body(self);
        self.scope.truncate(depth);
        r
    }

    // * Expressions

    fn block(&mut self) -> R<Expr> {
        self.inside(true, |p| {
            let mut first = vec![];
            loop {
                if p.eat("}") {
                    return match first.len() {
                        0 => Err(p.err("an empty block (`()` is nothing)")),
                        1 => Ok(first.pop().unwrap()),
                        _ => Ok(Expr::Seq(first)),
                    };
                }
                if !first.is_empty() && !p.m.toks[p.i].nl {
                    return Err(p.err("one statement to a line"));
                }
                if p.eat("let") {
                    let n = p.name()?;
                    p.want("=")?;
                    let v = p.expr()?;
                    let t = p.type_of(&v)?;
                    let rest = p.bind(vec![n], vec![t], |p| p.block())?;
                    let e = Expr::Let(bx(v), bx(rest));
                    return Ok(match first.is_empty() {
                        true => e,
                        false => Expr::Seq(first.into_iter().chain([e]).collect()),
                    });
                }
                first.push(p.expr()?);
            }
        })
    }

    pub fn expr(&mut self) -> R<Expr> {
        self.binary(1)
    }

    fn binary(&mut self, level: u8) -> R<Expr> {
        if level == 3 {
            return match self.eat("not") {
                true => Ok(Expr::Un(Un::Not, bx(self.binary(3)?))),
                false => self.binary(4),
            };
        }
        if level == 7 {
            return self.unary();
        }
        let mut a = self.binary(level + 1)?;
        while !self.ends() {
            let Some(i) = (0..13).find(|&i| LEVEL[i] == level && self.is(OPS.split(' ').nth(i).unwrap())) else {
                break;
            };
            self.i += 1;
            let b = self.binary(level + 1)?;
            a = Expr::Op(nth(i), bx(a), bx(b));
            if level == 4 {
                break;
            }
        }
        Ok(a)
    }

    fn unary(&mut self) -> R<Expr> {
        if self.is("-") && matches!(self.peek(1), T::Int(_)) {
            let n = self.int()?;
            return self.postfix(Expr::Lit(Ty::Int, int_enc(n).to_vec()));
        }
        if self.eat("-") {
            return Ok(Expr::Un(Un::Neg, bx(self.unary()?)));
        }
        if self.eat("refuse") {
            return Ok(Expr::Refuse(bx(self.unary()?)));
        }
        let e = self.primary()?;
        self.postfix(e)
    }

    fn postfix(&mut self, mut e: Expr) -> R<Expr> {
        while self.is(".") {
            self.i += 1;
            let name = match self.tok().clone() {
                T::Int(n) => {
                    self.i += 1;
                    n.to_string()
                }
                _ => self.word()?,
            };
            let t = self.type_of(&e)?;
            e = match self.is("(") {
                true => self.method(e, &t, &name)?,
                false => {
                    let types = self.types();
                    let i = field(&self.trows()?, &types, &t, &name).ok_or_else(|| self.err(format!("no field {name} of {}", spell(&t))))?;
                    Expr::Field(bx(e), i)
                }
            };
        }
        Ok(e)
    }

    fn elem(&self, t: &Ty) -> R<Ty> {
        match self.types().structure(t) {
            Ty::List(e) => Ok(*e),
            t => Err(self.err(format!("{} is not a list", spell(&t)))),
        }
    }

    /// `x => e`, `(a, b) => e`, its parameters typed `tys`.
    fn lambda(&mut self, tys: Vec<Ty>) -> R<Expr> {
        let names = match self.eat("(") {
            true => self.list(")", |p| p.name())?,
            false => vec![self.name()?],
        };
        self.want("=>")?;
        let n = names.len() as u32;
        Ok(Expr::Lambda(n, bx(self.bind(names, tys, |p| p.expr())?)))
    }

    fn is_lambda(&self) -> bool {
        let mut k = 0;
        if matches!(self.tok(), T::P("(")) {
            k = 1;
            while matches!(self.peek(k), T::Id(_) | T::P(",")) {
                k += 1;
            }
            if !matches!(self.peek(k), T::P(")")) {
                return false;
            }
        }
        matches!(self.peek(k), T::Id(_) | T::P(")")) && matches!(self.peek(k + 1), T::P("=>"))
    }

    fn method(&mut self, recv: Expr, t: &Ty, name: &str) -> R<Expr> {
        self.want("(")?;
        let e = bx(recv);
        let out = self.inside(false, |p| -> R<Expr> {
            Ok(match name {
                "filter" | "map" => {
                    let l = bx(p.lambda(vec![p.elem(t)?])?);
                    if name == "map" {
                        Expr::Map(e, l)
                    } else {
                        Expr::Filter(e, l)
                    }
                }
                "join" => {
                    let o = p.expr()?;
                    p.want(",")?;
                    let tys = vec![p.elem(t)?, p.elem(&p.type_of(&o)?)?];
                    Expr::Join(e, bx(o), bx(p.lambda(tys)?))
                }
                "group" => {
                    let x = p.elem(t)?;
                    let by = p.lambda(vec![x.clone()])?;
                    p.want(",")?;
                    let stack: Vec<Ty> = p.scope.iter().map(|s| s.1.clone()).chain([x.clone()]).collect();
                    let Expr::Lambda(_, b) = &by else { unreachable!() };
                    let k = p.type_of_at(&p.decl_list(), &stack, &close((**b).clone(), stack.len() as u32))?.0;
                    Expr::Group(e, bx(by), bx(p.lambda(vec![k, Ty::list(x)])?))
                }
                "sort" => {
                    let x = p.elem(t)?;
                    let names = vec![p.name()?];
                    p.want("=>")?;
                    let mut dirs = vec![];
                    let body = p.bind(names, vec![x], |p| p.keys(&mut dirs))?;
                    Expr::Sort(e, bx(Expr::Lambda(1, bx(body))), dirs)
                }
                "take" => Expr::Take(e, bx(p.expr()?)),
                _ => {
                    let args = p.rest()?;
                    let s = p.types().structure(t);
                    let f = (STD.split_whitespace().enumerate())
                        .filter(|(_, n)| *n == name)
                        .map(|(i, _)| nth::<Std>(i))
                        .filter(|f| (*f != Std::TextLen || s == Ty::Bytes) && (*f != Std::Len || s != Ty::Bytes))
                        .find(|f| *f != Std::Concat || args.is_empty())
                        .ok_or_else(|| p.err(format!("no method {name}")))?;
                    return Ok(Expr::Std(f, [*e].into_iter().chain(args).collect()));
                }
            })
        })?;
        if !matches!(out, Expr::Std(..)) {
            self.want(")")?;
        }
        Ok(out)
    }

    /// Arguments after `(`, through `)`.
    fn rest(&mut self) -> R<Vec<Expr>> {
        self.list(")", |p| p.expr())
    }

    /// A sort's keys: `desc e`, `asc e`, `e`, or a tuple of those.
    fn keys(&mut self, dirs: &mut Vec<Dir>) -> R<Expr> {
        let dir = |p: &mut Self| match (p.eat("desc"), p.eat("asc")) {
            (true, _) => Some(Dir::Desc),
            (_, true) => Some(Dir::Asc),
            _ => None,
        };
        if let Some(d) = dir(self) {
            dirs.push(d);
            return self.expr();
        }
        if self.is("(") && !self.is_lambda() {
            let save = self.i;
            self.i += 1;
            let ks = self.list(")", |p| {
                let d = dir(p).unwrap_or(Dir::Asc);
                Ok((d, p.expr()?))
            })?;
            if ks.len() > 1 && !self.is(".") {
                dirs.extend(ks.iter().map(|k| k.0));
                return Ok(Expr::Tuple(ks.into_iter().map(|k| k.1).collect()));
            }
            self.i = save;
        }
        dirs.push(Dir::Asc);
        self.expr()
    }

    /// A table's key arguments, `a, b` or `..k`, up to `)` or a lambda.
    fn key(&mut self) -> R<Box<Expr>> {
        if self.eat("..") {
            let k = self.expr()?;
            self.eat(",");
            return Ok(bx(k));
        }
        let mut ks = vec![];
        while !self.is(")") && !self.is_lambda() {
            ks.push(self.expr()?);
            if !self.is(")") {
                self.want(",")?;
            }
        }
        Ok(bx(Expr::Tuple(ks)))
    }

    /// `{ col: e, …, ..base }` as a row of `t`, or any expression.
    fn row(&mut self, t: &str) -> R<Expr> {
        let rec = self.is("{") && (matches!(self.peek(1), T::P("..")) || matches!(self.peek(2), T::P(":")));
        if !rec {
            return self.expr();
        }
        self.i += 1;
        let tab = self.m.schema.table(t).unwrap().1.clone();
        let (mut given, mut base) = (BTreeMap::new(), None);
        self.list("}", |p| {
            if p.eat("..") {
                base = Some(p.expr()?);
                return Ok(());
            }
            let n = p.word()?;
            let i = tab.column(&n).ok_or_else(|| p.err(format!("no column {n} of {t}")))?;
            p.want(":")?;
            given.insert(i, p.expr()?);
            Ok(())
        })?;
        let cols = (0..tab.columns.len()).map(|i| match (given.remove(&i), &base) {
            (Some(e), _) => Ok(e),
            (None, Some(b)) => Ok(Expr::Field(bx(b.clone()), i as u32)),
            (None, None) => Err(self.err(format!("{t}.{} is not given", tab.columns[i].name))),
        });
        Ok(Expr::Tuple(cols.collect::<R<_>>()?))
    }

    fn table_op(&mut self, t: String, op: &str) -> R<Expr> {
        self.i += 4;
        self.inside(false, |p| {
            let e = match op {
                "get" => Expr::Lookup(t, p.key()?),
                "delete" => Expr::Delete(t, p.key()?),
                "insert" => Expr::Insert(t.clone(), bx(p.row(&t)?)),
                _ => {
                    let k = p.key()?;
                    let row = p.type_of(&Expr::Scan(t.clone())).and_then(|r| p.elem(&r))?;
                    let n = p.name()?;
                    p.want("=>")?;
                    let body = p.bind(vec![n], vec![row], |p| p.row(&t))?;
                    Expr::Update(t, k, bx(Expr::Lambda(1, bx(body))))
                }
            };
            p.want(")")?;
            Ok(e)
        })
    }

    fn primary(&mut self) -> R<Expr> {
        let tok = self.tok().clone();
        match tok {
            T::Int(_) => {
                let n = self.int()?;
                return Ok(Expr::Lit(Ty::Int, int_enc(n).to_vec()));
            }
            T::Str(s) => {
                self.i += 1;
                return Ok(Expr::Lit(Types::text(), bytes_enc(s.as_bytes())));
            }
            T::Hex(b) => {
                self.i += 1;
                return Ok(Expr::Lit(Ty::Bytes, bytes_enc(&b)));
            }
            T::P("(") => {
                self.i += 1;
                let es = self.list(")", |p| p.expr())?;
                let comma = matches!(self.m.toks[self.i - 2].t, T::P(","));
                return Ok(match es.len() {
                    0 => Expr::Seq(vec![]),
                    1 if !comma => es.into_iter().next().unwrap(),
                    _ => Expr::Tuple(es),
                });
            }
            T::P("[") => {
                self.i += 1;
                let es = self.list("]", |p| p.expr())?;
                let first = es.first().ok_or_else(|| self.err("`[]` has no type: List<T>[]"))?;
                let t = infer(&self.types(), &self.type_of(first)?);
                return Ok(Expr::List(self.store(t), es));
            }
            T::P("{") => {
                self.i += 1;
                return self.block();
            }
            T::Id(_) => {}
            t => return Err(self.err(format!("an expression expected, not {t:?}"))),
        }
        let T::Id(n) = tok else { unreachable!() };
        if let Some(v) = self.var(&n) {
            self.i += 1;
            return Ok(v);
        }
        let upper = n.starts_with(|c: char| c.is_uppercase());
        match n.as_str() {
            "true" | "false" => {
                self.i += 1;
                return Ok(Expr::Lit(Ty::bool(), vec![(n == "true") as u8]));
            }
            "if" => {
                self.i += 1;
                let c = self.inside(false, |p| p.expr())?;
                self.want("{")?;
                let a = self.block()?;
                let b = match self.eat("else") {
                    true if self.is("if") => self.primary()?,
                    true => {
                        self.want("{")?;
                        self.block()?
                    }
                    false => Expr::Seq(vec![]),
                };
                return Ok(Expr::If(bx(c), bx(a), bx(b)));
            }
            "match" => return self.matching(),
            "ctx" => {
                self.i += 1;
                self.want(".")?;
                let f = self.word()?;
                return Ok(Expr::Ctx(match f.as_str() {
                    "user" => CtxField::User,
                    "session" => CtxField::Session,
                    "roles" => CtxField::Roles,
                    "now" => CtxField::Now,
                    "new_id" => {
                        self.want("(")?;
                        let t = self.name()?;
                        self.want(")")?;
                        self.auto(&format!("Id<{t}>"), Ty::Bytes)?;
                        self.store(Ty::Named(format!("Id<{t}>")));
                        CtxField::NewId(t)
                    }
                    f => return Err(self.err(format!("no context field {f}"))),
                }));
            }
            "some" | "none" => {
                self.i += 1;
                let given = match self.eat("<") {
                    true => {
                        let t = self.ty()?;
                        self.want(">")?;
                        Some(t)
                    }
                    false => None,
                };
                let (i, args) = match n == "some" {
                    true => {
                        self.want("(")?;
                        (1, self.rest()?)
                    }
                    false => (0, vec![]),
                };
                let inner = match (given, args.first()) {
                    (Some(t), _) => t,
                    (None, Some(x)) => infer(&self.types(), &self.type_of(x)?),
                    (None, None) => return Err(self.err("none<T> needs its type")),
                };
                let name = format!("Option<{}>", spell_in(&inner, &self.m.rows()?));
                self.auto(&name, Ty::option(inner))?;
                return Ok(Expr::Variant(self.store(Ty::Named(name)), i, args));
            }
            "lit" => {
                self.i += 1;
                self.want("<")?;
                let t = self.ty()?;
                self.want(">")?;
                self.want("(")?;
                let T::Hex(b) = self.tok().clone() else {
                    return Err(self.err("lit<T>(x\"…\")"));
                };
                self.i += 1;
                self.want(")")?;
                return Ok(Expr::Lit(self.store(t), b));
            }
            "nil_id" => {
                self.i += 1;
                self.want("(")?;
                self.want(")")?;
                return Ok(Expr::Std(Std::NilId, vec![]));
            }
            _ if upper || n == "Sum" => {
                let t = self.ty()?;
                let t = self.store(t);
                if self.eat("[") {
                    let es = self.list("]", |p| p.expr())?;
                    let Ty::List(e) = t else { return Err(self.err("List<T>[…]")) };
                    return Ok(Expr::List(*e, es));
                }
                self.want("#")?;
                let i = self.int()?;
                let args = if self.eat("(") { self.rest()? } else { vec![] };
                return Ok(Expr::Variant(t, i as u8, args));
            }
            _ => {}
        }
        let mut k = 1;
        let mut path = n.clone();
        while let (T::P("."), T::Id(x)) = (self.peek(k), self.peek(k + 1)) {
            path = format!("{path}.{x}");
            k += 2;
        }
        let call = matches!(self.peek(k), T::P("(")) && self.m.fns.contains_key(&path);
        if !call && self.m.schema.table(&n).is_some() {
            if let (T::P("."), T::Id(op), T::P("(")) = (self.peek(1), self.peek(2).clone(), self.peek(3)) {
                if ["get", "insert", "update", "delete"].contains(&op.as_str()) {
                    return self.table_op(n, &op);
                }
            }
            self.i += 1;
            return Ok(Expr::Scan(n));
        }
        let at = self.i;
        let path = self.path()?;
        if !self.is("(") {
            return Err(self.m.err(at, format!("`{path}` is no variable, table or function")));
        }
        self.i += 1;
        let args = self.rest()?;
        let f = self.m.function(&path, at)?;
        self.callees.extend(f.types.iter().cloned());
        self.decls.extend(f.types.iter().cloned());
        Ok(Expr::Call(f.hash(), args))
    }

    /// `match e { pattern => e … }`, one arm per variant in order:
    /// `none`, `some(x)`, `false`, `true`, `#i`, `#i(a, b)`.
    fn matching(&mut self) -> R<Expr> {
        self.i += 1;
        let s = self.inside(false, |p| p.expr())?;
        let t = self.type_of(&s)?;
        let Ty::Sum(vs) = self.types().structure(&t) else {
            return Err(self.err(format!("a match on {}", spell(&t))));
        };
        self.want("{")?;
        let arms = self.inside(true, |p| {
            let mut arms = vec![];
            for (i, fields) in vs.into_iter().enumerate() {
                let label = match p.tok().clone() {
                    T::Id(n) if ["none", "some", "false", "true"].contains(&n.as_str()) => {
                        p.i += 1;
                        ["none", "some", "false", "true"].iter().position(|m| *m == n).unwrap() % 2
                    }
                    _ => {
                        p.want("#")?;
                        p.int()? as usize
                    }
                };
                if label != i {
                    return Err(p.err(format!("arm {label} where variant {i} is due")));
                }
                let names = if p.eat("(") { p.list(")", |p| p.name())? } else { vec![] };
                p.want("=>")?;
                let k = fields.len() as u32;
                let body = p.bind(names, fields, |p| p.expr())?;
                arms.push(Expr::Lambda(k, bx(body)));
            }
            p.want("}")?;
            Ok(arms)
        })?;
        Ok(Expr::Match(bx(s), arms))
    }
}

/// §1.15 A module from its text: the schema, the declarations, every
/// function checked (callees first) and bound by its name unless the name
/// starts with `_`; the module's types are what its bound functions name.
pub fn build(src: &str) -> R<Domain> {
    let mut m = M {
        toks: lex(src)?,
        ..M::default()
    };
    // Items, found by their keyword at the top level.
    let (mut schema, mut types, mut depth) = (None, vec![], 0);
    for i in 0..m.toks.len() {
        match &m.toks[i].t {
            T::P("(" | "{" | "[") => depth += 1,
            T::P(")" | "}" | "]") => depth -= 1,
            T::Id(k) if depth == 0 && m.toks[i].nl => match k.as_str() {
                "schema" => schema = Some(i + 1),
                "type" => types.push(i + 1),
                k => {
                    let kind = match k {
                        "mutator" => Kind::Mutator,
                        "live" => Kind::Live,
                        "query" => Kind::Query,
                        "scope" => Kind::Scope,
                        "helper" | "guard" => Kind::Helper,
                        _ => continue,
                    };
                    let mut p = P::new(&mut m, i + 1);
                    let name = p.path()?;
                    if m.fns.insert(name.clone(), (kind, i + 1)).is_some() {
                        return Err(m.err(i, format!("{name} is defined twice")));
                    }
                }
            },
            _ => {}
        }
    }
    for &i in &types {
        let n = P::new(&mut m, i).tname()?;
        m.written.insert(n);
    }
    if let Some(i) = schema {
        let mut p = P::new(&mut m, i);
        p.want("{")?;
        let mut tables = vec![];
        while !p.eat("}") {
            let space = if p.eat("volatile") { Space::Volatile } else { Space::Durable };
            p.want("table")?;
            let mut t = Table::new(&p.name()?);
            t.space = space;
            let mut keys = 0;
            p.want("{")?;
            while !p.eat("}") {
                if !matches!(p.peek(1), T::P(":")) && p.eat("index") {
                    let unique = p.eat("unique");
                    p.want("(")?;
                    let cols = p.list(")", |p| {
                        let n = p.word()?;
                        t.column(&n).ok_or_else(|| p.err(format!("no column {n}")))
                    })?;
                    t.indexes.push(Index { columns: cols, unique });
                    continue;
                }
                let name = p.word()?;
                p.want(":")?;
                let ct = p.ty()?;
                let decl = spell_in(&ct, &p.m.rows()?);
                if p.eat("key") {
                    if keys != t.columns.len() {
                        return Err(p.err("a key column after one that is not"));
                    }
                    keys += 1;
                }
                if p.eat("refs") {
                    t.refs.push(Ref {
                        column: t.columns.len(),
                        table: p.name()?,
                    });
                }
                let default = match p.eat("default") {
                    true => match p.tok().clone() {
                        T::Hex(b) => {
                            p.i += 1;
                            Some(b)
                        }
                        _ => return Err(p.err("a default is x\"…\"")),
                    },
                    false => None,
                };
                t.columns.push(Column {
                    name,
                    decl,
                    ty: Ty::Int,
                    default,
                });
            }
            t.key = keys.max(1);
            tables.push(t);
        }
        m.schema.tables = tables;
        m.rows = None;
    }
    for &i in &types {
        let mut p = P::new(&mut m, i);
        let n = p.tname()?;
        p.want("=")?;
        let ty = p.ty()?;
        let mut refine = vec![];
        if p.eat("where") {
            refine.push(p.rule(&ty)?);
            while p.eat(",") {
                refine.push(p.rule(&ty)?);
            }
        }
        m.decls.insert(n, Decl { ty, refine });
    }
    // The columns' structures, under every declaration.
    let mut tys = vec![];
    for c in m.schema.tables.clone().iter().flat_map(|t| &t.columns) {
        tys.push(m.ty_of(lex(&c.decl)?)?);
    }
    let all = Types::new(&m.decls.clone().into_iter().collect::<Vec<_>>());
    for (c, t) in m.schema.tables.iter_mut().flat_map(|t| &mut t.columns).zip(tys) {
        c.ty = all.deep(&t);
    }
    let names: Vec<(String, usize)> = m.fns.iter().map(|(n, k)| (n.clone(), k.1)).collect();
    let (mut bindings, mut decls) = (vec![], BTreeMap::new());
    for (n, at) in &names {
        let f = m.function(n, *at)?;
        if !n.starts_with('_') {
            bindings.push((n.clone(), f.hash()));
            decls.extend(f.types.iter().cloned());
        }
    }
    let bodies = m.built.values().flatten().map(|f| (f.hash(), f.to_bytes())).collect();
    let module = Module {
        spec: SPEC,
        schema: m.schema.clone(),
        types: decls.into_iter().collect(),
        bindings,
    };
    Ok(Domain {
        schema: std::sync::Arc::new(m.schema),
        module,
        lib: m.lib,
        bodies,
    })
}

/// A fragment's expression, its type, and the declarations it makes.
pub type Fragment = (Expr, Ty, Vec<(String, Decl)>);

/// A fragment of text as an expression in a builder's function (§1.15):
/// `vars` its free names, by level, with their types; the levels after
/// them its own binders, which begin at `depth`. What it stores is
/// declared through `declare`.
pub fn fragment(src: &str, schema: &Schema, decls: &[(String, Decl)], vars: &[(&str, Expr, Ty)], depth: u32) -> R<Fragment> {
    let mut m = M::of(schema, decls);
    m.toks = lex(src)?;
    let mut p = P::new(&mut m, 0);
    p.decls = decls.iter().cloned().collect();
    p.scope = vars.iter().map(|v| (v.0.to_string(), v.2.clone())).collect();
    let e = p.inside(true, |p| p.expr())?;
    if !matches!(p.tok(), T::End) {
        return Err(p.err("more after the expression"));
    }
    let t = p.type_of(&e)?;
    let k = vars.len() as u32;
    let e = levels(e, &|l| {
        if l < k {
            vars[l as usize].1.clone()
        } else {
            Expr::Var(MARK + depth + l - k)
        }
    });
    let mut d = BTreeMap::new();
    for t in &p.stored {
        p.m.declare(t, &mut d);
    }
    Ok((e, t, d.into_iter().collect()))
}

// * Printing

struct Pr<'a> {
    rows: &'a [(String, Ty)],
    m: &'a Module,
    lib: &'a Lib,
    names: &'a BTreeMap<FnHash, String>,
    f: &'a Function,
    scope: Vec<(String, Ty)>,
    ind: usize,
}

impl Pr<'_> {
    fn ty(&self, e: &Expr) -> R<(Ty, Effect)> {
        let callee = |h: &FnHash| self.lib.get(h);
        let stack: Vec<Ty> = self.scope.iter().map(|s| s.1.clone()).collect();
        type_of(
            &Env {
                schema: &self.m.schema,
                callee: &callee,
            },
            &self.f.types,
            &stack,
            e,
        )
    }

    fn types(&self) -> Types {
        Types::new(&self.f.types)
    }

    /// Every table, and its row as the checker types it here.
    fn trows(&self) -> Vec<(Table, Ty)> {
        let row = |t: &Table| match self.ty(&Expr::Scan(t.name.clone())) {
            Ok((Ty::List(r), _)) => *r,
            _ => Ty::unit(),
        };
        self.m.schema.tables.iter().map(|t| (t.clone(), row(t))).collect()
    }

    /// A fresh name for a value of `t`: from its type, unlike any name in
    /// scope, any table, any function's first segment or keyword.
    fn fresh(&self, t: &Ty) -> String {
        let rows = self.trows();
        let stem = match (t, table_of(&rows, t)) {
            (_, Some(tab)) => tab.name.split('_').map(|w| &w[..1]).collect(),
            (Ty::Named(n), _) if n.starts_with("Id<") => format!("{}_id", &n[3..n.len() - 1]),
            (Ty::Named(n), _) if n.starts_with("Option<") => "opt".into(),
            (Ty::Named(n), _) => snake(n.rsplit('.').next().unwrap()),
            (Ty::Int, _) => "n".into(),
            (Ty::List(_), _) => "xs".into(),
            (t, _) if *t == Ty::bool() => "b".into(),
            _ => "x".into(),
        };
        let taken = |n: &str| {
            keyword(n)
                || n == "_"
                || self.scope.iter().any(|s| s.0 == n)
                || self.m.schema.table(n).is_some()
                || self.names.values().any(|f| f.split('.').next() == Some(n))
        };
        (1..)
            .map(|i| if i == 1 { stem.clone() } else { format!("{stem}{i}") })
            .find(|n| !taken(n))
            .unwrap()
    }

    fn under<X>(&mut self, tys: &[Ty], f: impl FnOnce(&mut Self, Vec<String>) -> R<X>) -> R<X> {
        let depth = self.scope.len();
        let mut names = vec![];
        for t in tys {
            let n = self.fresh(t);
            self.scope.push((n.clone(), t.clone()));
            names.push(n);
        }
        let r = f(self, names);
        self.scope.truncate(depth);
        r
    }

    fn pad(&self) -> String {
        "  ".repeat(self.ind)
    }

    /// `e` as a block: `{`, its statements a line each, `}`.
    fn block(&mut self, e: &Expr) -> R<String> {
        self.ind += 1;
        let mut lines = vec![];
        let r = self.stmts(e, &mut lines);
        self.ind -= 1;
        r?;
        let pad = self.pad();
        Ok(format!(
            "{{\n{}\n{pad}}}",
            lines.iter().map(|l| format!("{pad}  {l}")).collect::<Vec<_>>().join("\n")
        ))
    }

    fn stmts(&mut self, e: &Expr, out: &mut Vec<String>) -> R<()> {
        let nested = |e: &Expr| matches!(e, Expr::Let(..)) || matches!(e, Expr::Seq(es) if es.len() > 1);
        match e {
            Expr::Let(v, b) => {
                let value = self.ex(v, 0)?;
                let t = self.ty(v)?.0;
                self.under(&[t], |p, n| {
                    out.push(format!("let {} = {value}", n[0]));
                    p.stmts(b, out)
                })
            }
            Expr::Seq(es) if es.len() > 1 => {
                for (i, x) in es.iter().enumerate() {
                    match (i + 1 == es.len(), x) {
                        (true, Expr::Let(..)) => self.stmts(x, out)?,
                        _ if nested(x) => out.push(self.block(x)?),
                        _ => out.push(self.ex(x, 0)?),
                    }
                }
                Ok(())
            }
            e => {
                out.push(self.ex(e, 0)?);
                Ok(())
            }
        }
    }

    fn lam(&mut self, l: &Expr, tys: Vec<Ty>, row: Option<&str>) -> R<String> {
        let Expr::Lambda(_, b) = l else {
            return Err(Refusal::new("a lambda expected"));
        };
        self.under(&tys, |p, n| {
            let ps = if n.len() == 1 { n[0].clone() } else { format!("({})", n.join(", ")) };
            let body = match row {
                Some(t) => p.row(t, b)?,
                None => p.ex(b, 0)?,
            };
            Ok(format!("{ps} => {body}"))
        })
    }

    /// A row of table `t`: `{ col: e, …, ..base }` where it is a tuple.
    fn row(&mut self, t: &str, e: &Expr) -> R<String> {
        let tab = self.m.schema.table(t).unwrap().1;
        let Expr::Tuple(es) = e else { return self.ex(e, 0) };
        if es.len() != tab.columns.len() {
            return self.ex(e, 0);
        }
        let row = self.trows().into_iter().find(|r| r.0.name == t).map(|r| r.1);
        let base = es.iter().enumerate().find_map(|(i, x)| match x {
            Expr::Field(b, j) if *j as usize == i && self.ty(b).ok().map(|t| t.0) == row => Some((**b).clone()),
            _ => None,
        });
        let is_base = |i: usize, x: &Expr| matches!((x, &base), (Expr::Field(b, j), Some(c)) if *j as usize == i && **b == *c);
        let mut parts = vec![];
        for (i, x) in es.iter().enumerate() {
            if !is_base(i, x) {
                parts.push(format!("{}: {}", tab.columns[i].name, self.ex(x, 0)?));
            }
        }
        if let Some(b) = &base {
            parts.push(format!("..{}", self.ex(b, 8)?));
        }
        Ok(format!("{{ {} }}", parts.join(", ")))
    }

    fn key(&mut self, k: &Expr) -> R<String> {
        match k {
            Expr::Tuple(es) => Ok(es.iter().map(|e| self.ex(e, 0)).collect::<R<Vec<_>>>()?.join(", ")),
            k => Ok(format!("..{}", self.ex(k, 0)?)),
        }
    }

    fn all(&mut self, es: &[Expr]) -> R<String> {
        Ok(es.iter().map(|e| self.ex(e, 0)).collect::<R<Vec<_>>>()?.join(", "))
    }

    fn elem(&self, e: &Expr) -> R<Ty> {
        match self.types().structure(&self.ty(e)?.0) {
            Ty::List(t) => Ok(*t),
            _ => Err(Refusal::new("a list expected")),
        }
    }

    /// `e` at precedence `at` (`spec/AUTHORING.md` §1): 0 anything, 1 `or`,
    /// 2 `and`, 3 `not`, 4 comparison, 5 `+`, 6 `*`, 7 unary, 8 postfix.
    fn ex(&mut self, e: &Expr, at: u8) -> R<String> {
        use Expr::*;
        let wrap = |s: String, own: u8| if own < at { format!("({s})") } else { s };
        let types = self.types();
        Ok(match e {
            Lit(t, b) => {
                let mut r = Reader::new(b);
                match t {
                    Ty::Int => {
                        let n = r.int()?;
                        wrap(n.to_string(), if n < 0 { 7 } else { 8 })
                    }
                    Ty::Named(n) if n == "Text" => format!("{:?}", r.text()?),
                    Ty::Bytes => format!("x\"{}\"", hex(&r.bytes()?)),
                    t if *t == Ty::bool() => (if b[..] == [1] { "true" } else { "false" }).into(),
                    t => format!("lit<{}>(x\"{}\")", spell_in(t, self.rows), hex(b)),
                }
            }
            Var(i) => self
                .scope
                .iter()
                .rev()
                .nth(*i as usize)
                .map(|s| s.0.clone())
                .ok_or_else(|| Refusal::new("an unbound variable"))?,
            Let(..) => self.block(e)?,
            Seq(es) if es.is_empty() => "()".into(),
            Seq(es) if es.len() == 1 => return Err(Refusal::new("a sequence of one has no spelling")),
            Seq(_) => self.block(e)?,
            If(c, a, b) => {
                let mut s = format!("if {} {}", self.ex(c, 0)?, self.block(a)?);
                match &**b {
                    Seq(es) if es.is_empty() => {}
                    If(..) => s += &format!(" else {}", self.ex(b, 0)?),
                    b => s += &format!(" else {}", self.block(b)?),
                }
                wrap(s, 0)
            }
            Match(s, arms) => {
                let st = self.ty(s)?.0;
                let Ty::Sum(vs) = types.structure(&st) else {
                    return Err(Refusal::new("a match on no sum"));
                };
                let opt = types.structure(&st).is_option();
                let head = format!("match {} {{", self.ex(s, 0)?);
                let bool = types.structure(&st) == Ty::bool();
                self.ind += 1;
                let lines = (|| {
                    let mut lines = vec![];
                    for (i, (fields, arm)) in vs.iter().zip(arms).enumerate() {
                        let label = match (opt, bool) {
                            (true, _) => ["none", "some"][i].to_string(),
                            (_, true) => ["false", "true"][i].to_string(),
                            _ => format!("#{i}"),
                        };
                        let line = self.under(fields, |p, n| {
                            let binds = if n.is_empty() { String::new() } else { format!("({})", n.join(", ")) };
                            Ok(format!("{label}{binds} => {}", p.ex(arm, 0)?))
                        })?;
                        lines.push(format!("{}{line}", self.pad()));
                    }
                    Ok::<_, Refusal>(lines)
                })();
                self.ind -= 1;
                wrap(format!("{head}\n{}\n{}}}", lines?.join("\n"), self.pad()), 0)
            }
            Field(x, i) => {
                let t = self.ty(x)?.0;
                let rows = self.trows();
                let row = table_of(&rows, &t);
                let name = match (&t, types.structure(&t)) {
                    (Ty::Named(r), Ty::Tuple(ts)) => match ts.get(*i as usize) {
                        Some(Ty::Named(f)) if f.starts_with(&format!("{r}.")) => Some(f[r.len() + 1..].to_string()),
                        _ => None,
                    },
                    _ => None,
                };
                let name = name.or_else(|| Some(row?.columns.get(*i as usize)?.name.clone()));
                let name = name.filter(|n| field(&rows, &types, &t, n) == Some(*i)).unwrap_or(i.to_string());
                format!("{}.{name}", self.ex(x, 8)?)
            }
            Tuple(es) if es.len() == 1 => format!("({},)", self.ex(&es[0], 0)?),
            Tuple(es) => format!("({})", self.all(es)?),
            Variant(t, i, es) => match (t, types.decl(&spell(t))) {
                (Ty::Named(n), Some(d)) if n.starts_with("Option<") && auto(n, &d.ty, self.rows).as_ref() == Some(d) => {
                    let Ty::Sum(vs) = &d.ty else { unreachable!() };
                    let inner = &vs[1][0];
                    match (*i, es.first()) {
                        (1, Some(x)) if infer(&types, &self.ty(x)?.0) == *inner => format!("some({})", self.ex(x, 0)?),
                        (1, Some(x)) => format!("some<{}>({})", spell_in(inner, self.rows), self.ex(x, 0)?),
                        _ => format!("none<{}>", spell_in(inner, self.rows)),
                    }
                }
                _ if es.is_empty() => format!("{}#{i}", spell_in(t, self.rows)),
                _ => format!("{}#{i}({})", spell_in(t, self.rows), self.all(es)?),
            },
            List(t, es) => match es.first() {
                Some(x) if infer(&types, &self.ty(x)?.0) == *t => format!("[{}]", self.all(es)?),
                _ => format!("List<{}>[{}]", spell_in(t, self.rows), self.all(es)?),
            },
            Op(o, a, b) => {
                let (s, l) = op(*o);
                wrap(format!("{} {s} {}", self.ex(a, if l == 4 { 5 } else { l })?, self.ex(b, l + 1)?), l)
            }
            Un(crate::ir::Un::Not, a) => wrap(format!("not {}", self.ex(a, 3)?), 3),
            Un(_, a) if matches!(**a, Lit(Ty::Int, _)) => wrap(format!("-({})", self.ex(a, 0)?), 7),
            Un(_, a) => wrap(format!("-{}", self.ex(a, 7)?), 7),
            Std(crate::ir::Std::NilId, _) => "nil_id()".into(),
            Std(f, es) => {
                let name = STD.split_whitespace().nth(f.to_bytes()[0] as usize).unwrap();
                format!("{}.{name}({})", self.ex(&es[0], 8)?, self.all(&es[1..])?)
            }
            Call(h, es) => {
                let n = self.names.get(h).ok_or_else(|| Refusal(format!("no function {h}")))?;
                format!("{n}({})", self.all(es)?)
            }
            Ctx(c) => match c {
                CtxField::NewId(t) => format!("ctx.new_id({t})"),
                c => format!("ctx.{}", snake(&format!("{c:?}"))),
            },
            Lambda(..) => return Err(Refusal::new("a lambda outside an operator")),
            Scan(t) => t.clone(),
            Lookup(t, k) => format!("{t}.get({})", self.key(k)?),
            Delete(t, k) => format!("{t}.delete({})", self.key(k)?),
            Insert(t, r) => format!("{t}.insert({})", self.row(t, r)?),
            Update(t, k, l) => {
                let row = self.elem(&Scan(t.clone()))?;
                let key = self.key(k)?;
                let sep = if key.is_empty() { "" } else { ", " };
                format!("{t}.update({key}{sep}{})", self.lam(l, vec![row], Some(t))?)
            }
            Filter(l, f) | Map(l, f) => {
                let name = if matches!(e, Filter(..)) { "filter" } else { "map" };
                let x = self.elem(l)?;
                format!("{}.{name}({})", self.ex(l, 8)?, self.lam(f, vec![x], None)?)
            }
            Join(a, b, on) => {
                let (x, y) = (self.elem(a)?, self.elem(b)?);
                format!("{}.join({}, {})", self.ex(a, 8)?, self.ex(b, 0)?, self.lam(on, vec![x, y], None)?)
            }
            Group(l, by, agg) => {
                let x = self.elem(l)?;
                let Lambda(_, kb) = &**by else {
                    return Err(Refusal::new("a lambda expected"));
                };
                self.scope.push((String::new(), x.clone()));
                let k = self.ty(kb).map(|t| t.0);
                self.scope.pop();
                let (b, a) = (self.lam(by, vec![x.clone()], None)?, self.lam(agg, vec![k?, Ty::list(x)], None)?);
                format!("{}.group({b}, {a})", self.ex(l, 8)?)
            }
            Sort(l, keys, dirs) => {
                let x = self.elem(l)?;
                let Lambda(_, kb) = &**keys else {
                    return Err(Refusal::new("a lambda expected"));
                };
                let d = |d: &Dir| if *d == Dir::Desc { "desc " } else { "" };
                let body = self.under(&[x], |p, n| {
                    let ks = match (&**kb, &dirs[..]) {
                        (k, [Dir::Desc]) => format!("desc {}", p.ex(k, 0)?),
                        (k @ Tuple(_), [Dir::Asc]) => format!("asc {}", p.ex(k, 0)?),
                        (k, [Dir::Asc]) => p.ex(k, 0)?,
                        (Tuple(ks), ds) if ks.len() == ds.len() => {
                            let each = ks.iter().zip(ds).map(|(k, x)| Ok(format!("{}{}", d(x), p.ex(k, 0)?)));
                            format!("({})", each.collect::<R<Vec<_>>>()?.join(", "))
                        }
                        _ => return Err(Refusal::new("sort keys that are no tuple of their directions")),
                    };
                    Ok(format!("{} => {ks}", n[0]))
                })?;
                format!("{}.sort({body})", self.ex(l, 8)?)
            }
            Take(l, n) => format!("{}.take({})", self.ex(l, 8)?, self.ex(n, 0)?),
            Refuse(m) => wrap(format!("refuse {}", self.ex(m, 7)?), 7),
        })
    }
}

fn rules(d: &Decl, pr: &mut Pr) -> R<String> {
    let mut out = vec![];
    for r in &d.refine {
        let s = match &r.rule {
            Rule::Utf8 => "utf8".to_string(),
            Rule::Trim => "trim".into(),
            Rule::NonEmpty => "non_empty".into(),
            Rule::Len(n) => format!("len {n}"),
            Rule::LenRange(a, i64::MAX) => format!("len {a}.."),
            Rule::LenRange(a, b) => format!("len {a}..={b}"),
            Rule::Refine(e) => pr.under(std::slice::from_ref(&d.ty), |p, n| Ok(format!("{} => {}", n[0], p.ex(e, 0)?)))?,
        };
        out.push(match &r.why {
            Some(w) => format!("{s} {w:?}"),
            None => s,
        });
    }
    Ok(if out.is_empty() {
        String::new()
    } else {
        format!(" where {}", out.join(", "))
    })
}

/// §1.15 A module as text: its schema, the declarations no `Id<t>` or
/// `Option<T>` makes alone, then every function — bound ones by name,
/// sorted; others as `_` and their hash.
pub fn print(m: &Module, functions: &[Function]) -> R<String> {
    let lib = Lib::default();
    let mut todo: Vec<&Function> = functions.iter().collect();
    while !todo.is_empty() {
        let before = todo.len();
        todo.retain(|f| lib.install(&m.schema, (*f).clone()).is_err());
        if todo.len() == before {
            return Err(Refusal::new("a function that does not check"));
        }
    }
    let mut names: BTreeMap<FnHash, String> = BTreeMap::new();
    for (n, h) in &m.bindings {
        names.entry(*h).or_insert_with(|| n.clone());
    }
    for f in functions {
        names.entry(f.hash()).or_insert_with(|| format!("_{}", &f.hash().to_string()[..16]));
    }
    let empty = Function {
        types: m.types.clone(),
        params: vec![],
        ret: Ty::unit(),
        effect: Effect::Pure,
        body: Expr::Seq(vec![]),
    };
    let rows = rows_of(m)?;
    let mut pr = Pr {
        rows: &rows,
        m,
        lib: &lib,
        names: &names,
        f: &empty,
        scope: vec![],
        ind: 0,
    };
    let mut out = String::new();
    if !m.schema.tables.is_empty() {
        out += "schema {\n";
        for t in &m.schema.tables {
            let v = if t.space == Space::Volatile { "volatile " } else { "" };
            out += &format!("  {v}table {} {{\n", t.name);
            for (i, c) in t.columns.iter().enumerate() {
                let key = if i < t.key { " key" } else { "" };
                let refs: String = t.refs.iter().filter(|r| r.column == i).map(|r| format!(" refs {}", r.table)).collect();
                let def = c.default.as_ref().map_or(String::new(), |d| format!(" default x\"{}\"", hex(d)));
                out += &format!("    {}: {}{key}{refs}{def}\n", c.name, c.decl);
            }
            for x in &t.indexes {
                let cols: Vec<&str> = x.columns.iter().map(|&c| t.columns[c].name.as_str()).collect();
                out += &format!("    index {}({})\n", if x.unique { "unique " } else { "" }, cols.join(", "));
            }
            out += "  }\n";
        }
        out += "}\n";
    }
    let mut decls: BTreeMap<String, Decl> = m.types.iter().cloned().collect();
    for c in m.schema.tables.iter().flat_map(|t| &t.columns) {
        let built_in = ["Int", "Bytes", "Bool", "Never", "Text"].contains(&c.decl.as_str());
        if !(built_in || ["List<", "(", "Id<", "Option<", "Sum<"].iter().any(|p| c.decl.starts_with(p))) {
            decls.entry(c.decl.clone()).or_insert_with(|| Decl {
                ty: c.ty.clone(),
                refine: vec![],
            });
        }
    }
    let shown: Vec<_> = decls.iter().filter(|(n, d)| auto(n, &d.ty, &rows).as_ref() != Some(d)).collect();
    if !shown.is_empty() {
        out += "\n";
    }
    for (n, d) in shown {
        out += &format!("type {n} = {}{}\n", spell_in(&d.ty, &rows), rules(d, &mut pr)?);
    }
    let mut fns: Vec<(&String, &Function)> = functions.iter().map(|f| (&names[&f.hash()], f)).collect();
    for (n, h) in &m.bindings {
        if names[h] != *n {
            fns.extend(functions.iter().filter(|f| f.hash() == *h).map(|f| (n, f)));
        }
    }
    fns.sort_by_key(|x| x.0);
    for (name, f) in fns {
        out += "\n";
        out += &function(m, &lib, &names, &rows, name, f)?;
    }
    Ok(out)
}

/// One function as text, its kind from its effect: `mutator`, or `live`
/// when every table it and its callees touch is volatile; `query`; `helper`.
fn function(m: &Module, lib: &Lib, names: &BTreeMap<FnHash, String>, rows: &[(String, Ty)], name: &str, f: &Function) -> R<String> {
    let mut pr = Pr {
        rows,
        m,
        lib,
        names,
        f,
        scope: vec![],
        ind: 0,
    };
    let kind = match f.effect {
        Effect::Write if volatile(&m.schema, lib, &f.body) == Some(true) => "live",
        Effect::Write => "mutator",
        Effect::Read => "query",
        Effect::Pure => "helper",
    };
    pr.under(&f.params, |p, ns| {
        let params: Vec<String> = ns.iter().zip(&f.params).map(|(n, t)| format!("{n}: {}", spell_in(t, p.rows))).collect();
        let ret = if f.ret == Ty::unit() {
            String::new()
        } else {
            format!(" -> {}", spell_in(&f.ret, p.rows))
        };
        let body = match &f.body {
            Expr::Let(..) | Expr::Seq(_) if !matches!(&f.body, Expr::Seq(es) if es.is_empty()) => format!(" {}", p.block(&f.body)?),
            b => format!(" = {}", p.ex(b, 0)?),
        };
        Ok(format!("{kind} {name}({}){ret}{body}\n", params.join(", ")))
    })
}

/// Whether every table `e` and its callees touch is volatile; `None` when
/// it touches none.
fn volatile(schema: &Schema, lib: &Lib, e: &Expr) -> Option<bool> {
    let mut seen = None;
    crate::interp::walk(e, None, &mut |x, _| {
        let t = match x {
            Expr::Scan(t) | Expr::Lookup(t, _) | Expr::Insert(t, _) | Expr::Update(t, ..) | Expr::Delete(t, _) => Some(t),
            _ => None,
        };
        if let Some(t) = t {
            let v = schema.table(t).is_some_and(|(_, t)| t.space == Space::Volatile);
            seen = Some(seen.unwrap_or(true) && v);
        }
        if let Expr::Call(h, _) = x {
            if let Some(c) = lib.get(h).and_then(|c| volatile(schema, lib, &c.f.body)) {
                seen = Some(seen.unwrap_or(true) && c);
            }
        }
    });
    seen
}

/// One function of a module as text, for a browser of it: `name` its binding.
pub fn print_function(m: &Module, lib: &Lib, name: &str) -> R<String> {
    let mut names: BTreeMap<FnHash, String> = BTreeMap::new();
    for (n, h) in &m.bindings {
        names.entry(*h).or_insert_with(|| n.clone());
    }
    let f = lib
        .get(&m.get(name).ok_or_else(|| Refusal(format!("no function {name}")))?)
        .ok_or_else(|| Refusal::new("not loaded"))?;
    function(m, lib, &names, &rows_of(m)?, name, &f.f)
}

/// The module's rows, as the text names them.
fn rows_of(m: &Module) -> R<Vec<(String, Ty)>> {
    M::of(&m.schema, &m.types).rows()
}
