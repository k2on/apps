//! §1.15 ArkLang, the common text form of a module: S-expressions.
//!
//! **One reader rule**, `(head args…)`. Atoms are symbols, integers,
//! strings `"…"` (`\"` and `\\` their only escapes) and bytes `0x…`; `;`
//! comments run to the end of a line. A symbol may hold `.`: `input.title`
//! is a field path when `input` is bound, else a declared name
//! (`playlists.items`, `Clock.Paused`). [`read`] reads forms; [`layout`]
//! prints one on a line when it ends by column 100, else its head on the
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
//! a row's — and `(as T …)` gives a `none`, `some` or `list` a type no
//! place does. Variables are levels until a function is built
//! ([`crate::author::close`]).

use std::collections::BTreeMap;
use std::fmt::Display;
use std::mem::take;

use crate::author::{close, levels, Domain, MARK};
use crate::check::{type_of, Env, Types};
use crate::codec::{bytes_enc, int_enc, Reader};
use crate::hash::{hex, unhex, FnHash};
use crate::interp::walk;
use crate::ir::{CtxField, Decl, Effect, Expr, Function, Node, Refine, Rule, Std as StdFn, Un as UnOp};
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

fn fail<T>(at: At, what: impl Display) -> R<T> {
    Err(Refusal(format!("{}:{}: {what}", at.0, at.1)))
}
/// An error said at a place.
trait Here<T> {
    fn at(self, p: At) -> R<T>;
}
impl<T, E: Display> Here<T> for Result<T, E> {
    fn at(self, p: At) -> R<T> {
        self.or_else(|e| fail(p, e))
    }
}
fn sx(k: K) -> Sx {
    Sx { at: (0, 0), k }
}
fn y(n: impl Into<String>) -> Sx {
    sx(K::Sym(n.into()))
}
fn l(xs: Vec<Sx>) -> Sx {
    sx(K::List(xs))
}
fn h(head: &str, xs: impl IntoIterator<Item = Sx>) -> Sx {
    l(std::iter::once(y(head)).chain(xs).collect())
}
fn bx(e: Expr) -> Box<Expr> {
    Box::new(e)
}

/// The forms of a file.
pub fn read(src: &str) -> R<Vec<Sx>> {
    let (b, mut i, mut line, mut start) = (src.as_bytes(), 0, 1, 0);
    let mut open: Vec<(At, Vec<Sx>)> = vec![((1, 1), vec![])];
    while i < b.len() {
        let at = (line, i - start + 1);
        i += 1;
        match b[i - 1] {
            b'\n' => (line, start) = (line + 1, i),
            b';' => i += b[i..].iter().position(|c| *c == b'\n').unwrap_or(b.len() - i),
            c if c.is_ascii_whitespace() => {}
            b'(' => open.push((at, vec![])),
            b')' if open.len() == 1 => return fail(at, "a `)` without its `(`"),
            b')' => {
                let (at, xs) = open.pop().unwrap();
                open.last_mut().unwrap().1.push(Sx { at, k: K::List(xs) });
            }
            b'"' => {
                let mut s = vec![];
                while b.get(i) != Some(&b'"') {
                    match (b.get(i), b.get(i + 1)) {
                        (None, _) => return fail(at, "a string without its end"),
                        (Some(b'\\'), Some(c @ (b'"' | b'\\'))) => (s.push(*c), i += 1).0,
                        (Some(b'\\'), _) => return fail((line, i - start + 1), "only `\\\"` and `\\\\` escape"),
                        (Some(b'\n'), _) => (s.push(b'\n'), (line, start) = (line + 1, i + 1)).0,
                        (Some(c), _) => s.push(*c),
                    }
                    i += 1;
                }
                i += 1;
                let k = K::Str(String::from_utf8(s).expect("text between ASCII quotes"));
                open.last_mut().unwrap().1.push(Sx { at, k });
            }
            _ => {
                let n = b[i - 1..]
                    .iter()
                    .position(|c| c.is_ascii_whitespace() || b"()\";".contains(c))
                    .unwrap_or(b.len() - i + 1);
                let t = &src[i - 1..i - 1 + n];
                i += n - 1;
                let k = match (t.strip_prefix("0x").map(unhex), t.parse()) {
                    (Some(Some(h)), _) => K::Bytes(h),
                    (_, Ok(n)) => K::Int(n),
                    (Some(None), _) => return fail(at, format!("`{t}` is no bytes")),
                    _ if t.trim_start_matches('-').starts_with(|c: char| c.is_ascii_digit()) => return fail(at, format!("`{t}` is no integer")),
                    _ => K::Sym(t.into()),
                };
                open.last_mut().unwrap().1.push(Sx { at, k });
            }
        }
    }
    match open.pop() {
        Some((_, xs)) if open.is_empty() => Ok(xs),
        Some((at, _)) => fail(at, "a `(` without its `)`"),
        None => unreachable!(),
    }
}

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
/// two spaces past the form's `(`.
pub fn layout(x: &Sx, col: usize) -> String {
    match &x.k {
        K::List(xs) if !xs.is_empty() && col + flat(x).chars().count() > 100 => {
            let rest: String = xs[1..]
                .iter()
                .map(|a| format!("\n{}{}", " ".repeat(col + 2), layout(a, col + 2)))
                .collect();
            format!("({}{rest})", layout(&xs[0], col + 1))
        }
        _ => flat(x),
    }
}

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
fn names(x: &Sx) -> R<Vec<String>> {
    items(x)?.iter().map(|n| name(n).map(String::from)).collect()
}
fn pair(x: &Sx) -> R<(&Sx, &Sx)> {
    match items(x)? {
        [a, b] => Ok((a, b)),
        _ => fail(x.at, "a pair expected"),
    }
}

/// The kinds, each the effect bound it says: a mutator's and a live
/// function's `Write`, a query's `Read`, a scope's `Pure`; a helper's and a
/// guard's the least its body has.
const KINDS: [(&str, Option<Effect>); 6] = [
    ("mutator", Some(Effect::Write)),
    ("live", Some(Effect::Write)),
    ("query", Some(Effect::Read)),
    ("scope", Some(Effect::Pure)),
    ("helper", None),
    ("guard", None),
];
/// The standard functions, in `Std`'s order (`len`, `concat` name two each).
const STD: &str = "trim is-empty concat lower is-alnum chars len starts-with split-once to-text hex min max clamp abs fnv1a64 \
    sha256 id-of-text text-of-id nil-id utf8 first last len contains reverse concat insert-at remove-at move-at slice some? ?? unwrap sum";
const OPS: &str = "+ - * / % == != < <= > >= and or";
/// The atoms no binder is named; the context's in `CtxField`'s order.
const ATOMS: [&str; 8] = ["true", "false", "none", "ctx", "ctx.user", "ctx.session", "ctx.roles", "ctx.now"];
const RULES: [(&str, Rule); 3] = [("utf8", Rule::Utf8), ("trim", Rule::Trim), ("non-empty", Rule::NonEmpty)];

/// How many arguments a form takes, at least and at most.
fn arity(h: &str) -> Option<(usize, usize)> {
    Some(match h {
        "if" => (2, 3),
        "some" | "none?" | "not" | "refuse" | "new-id" | "has-role" => (1, 1),
        "as" | "insert" | "filter" | "map" | "sort" | "take" => (2, 2),
        "join" | "group" => (3, 3),
        "let" | "match" => (1, 99),
        "get" | "delete" | "variant" | "." => (2, 99),
        "update" => (3, 99),
        "-" => (1, 2),
        o if OPS.split(' ').any(|p| p == o) => (2, 2),
        _ => return None,
    })
}

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
    let under = |(i, c): (usize, char)| (c.is_uppercase() && i > 0).then_some('_').into_iter().chain(c.to_lowercase());
    s.chars().enumerate().flat_map(under).collect()
}
fn decl(ty: Ty, refine: Vec<Refine>) -> Decl {
    Decl { ty, refine, names: vec![] }
}
fn id_decl() -> Decl {
    decl(
        Ty::Bytes,
        vec![Refine {
            rule: Rule::Len(16),
            why: None,
        }],
    )
}

/// A sum's variants by name, each with its fields' names.
fn variants(d: &Decl) -> Option<Vec<(String, Vec<String>)>> {
    let (Ty::Sum(vs), false) = (&d.ty, d.names.is_empty()) else {
        return None;
    };
    let mut at = 0;
    Some(
        vs.iter()
            .map(|v| (at += v.len() + 1, (d.names[at - v.len() - 1].clone(), d.names[at - v.len()..at].to_vec())).1)
            .collect(),
    )
}

/// The one table whose rows are of type `t`, if one is.
fn table_of<'a>(rows: &'a [(Table, Ty)], t: &Ty) -> Option<&'a Table> {
    let mut it = rows.iter().filter(|r| r.1 == *t);
    it.next().filter(|_| it.next().is_none()).map(|r| &r.0)
}

/// What `(some x)` and `(list x …)` declare by: `x`'s type, its
/// refinements on top dropped and an `Id<t>` kept, as a builder's Rust is.
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
        seen = v.map_or(seen, |v| Some(seen.unwrap_or(true) && v));
    });
    seen
}

/// A module as it is lowered or printed: its schema, every declaration —
/// written, or made by an `Id<t>` or `Option<T>` spelled — its functions'
/// forms and names, those built; and while one is: the names in scope, the
/// types it stores and its callees' declarations.
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
    fn ty_of(&self, e: &Expr, at: At) -> R<Ty> {
        self.type_of(e).at(at).map(|t| t.0)
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
            Ty::Sum(vs) if Ty::Sum(vs.clone()).is_option() => Some(vs[1][0].clone()),
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
        self.auto(n, decl(Ty::option(t), vec![]))
    }
    /// Whether `n` is an `Id<t>` or `Option<T>` as one is made without a word.
    fn is_auto(&mut self, n: &str) -> bool {
        let d = self.decls.get(n).cloned();
        match self.inner(d.as_ref().map(|d| &d.ty)) {
            Some(i) => n.starts_with("Option<") && *n == format!("Option<{}>", self.spell(&i)) && d == Some(decl(Ty::option(i), vec![])),
            None => n.starts_with("Id<") && d == Some(id_decl()),
        }
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
            let cols: Vec<Ty> = t.columns.iter().map(|c| self.dty(&c.decl).unwrap_or(Ty::unit())).collect();
            let types = self.types();
            out.push((
                t,
                Ty::Tuple(
                    cols.into_iter()
                        .map(|c| if matches!(c, Ty::Named(_)) { c } else { types.deep(&c) })
                        .collect(),
                ),
            ));
        }
        self.rows = Some(out.clone());
        out
    }
    fn row(&mut self, t: &str, at: At) -> R<(Table, Ty)> {
        self.rows()
            .into_iter()
            .find(|r| r.0.name == t)
            .map_or_else(|| fail(at, format!("no table {t}")), Ok)
    }

    /// A type's name as the builders spell a declaration: `Option<Item>`.
    fn spell(&mut self, t: &Ty) -> String {
        let row = self.rows().into_iter().find(|r| r.1 == *t);
        let mut all = |ts: Vec<Ty>| ts.iter().map(|t| self.spell(t)).collect::<Vec<_>>().join(", ");
        match t {
            Ty::Int => "Int".into(),
            Ty::Bytes => "Bytes".into(),
            Ty::Named(n) => n.clone(),
            Ty::List(e) => format!("List<{}>", all(vec![(**e).clone()])),
            _ if row.is_some() => camel(&row.unwrap().0.name),
            Ty::Tuple(ts) if ts.len() == 1 => format!("({},)", all(ts.clone())),
            Ty::Tuple(ts) => format!("({})", all(ts.clone())),
            Ty::Sum(vs) if vs.is_empty() => "Never".into(),
            t if *t == Ty::bool() => "Bool".into(),
            Ty::Sum(vs) => format!("Sum<{}>", all(vs.iter().map(|v| Ty::Tuple(v.clone())).collect())),
        }
    }

    /// A declaration's name as a type: `Id<t>`, `Option<T>`, `List<T>`, a
    /// leaf or a declared name.
    fn dty(&mut self, s: &str) -> Result<Ty, String> {
        let inner = |p: &str| s.strip_prefix(p).and_then(|x| x.strip_suffix('>'));
        Ok(match s {
            _ if inner("Id<").is_some() => self.auto(s.into(), id_decl()),
            _ if inner("Option<").is_some() => {
                let t = self.dty(inner("Option<").unwrap())?;
                self.option(t)
            }
            _ if inner("List<").is_some() => Ty::list(self.dty(inner("List<").unwrap())?),
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
        match t? {
            Ty::Named(n) => self
                .decls
                .get(n)
                .filter(|d| matches!(d.ty, Ty::Tuple(_)) && !d.names.is_empty())
                .map(|d| d.names.clone()),
            t => Some(table_of(&self.rows(), t)?.columns.iter().map(|c| c.name.clone()).collect()),
        }
    }
    /// Field `f` of a value of type `t`: a position, or a name.
    fn field(&mut self, t: &Ty, f: &str) -> Option<u32> {
        f.parse()
            .ok()
            .or_else(|| self.names_of(Some(t))?.iter().position(|n| n == f).map(|i| i as u32))
    }
    /// The labels of a match's arms on `t`: its declaration's variants,
    /// `none some`, `false true`, else positions.
    fn labels(&self, t: &Ty, n: usize) -> Vec<String> {
        let named = if let Ty::Named(m) = t {
            self.decls.get(m).and_then(variants)
        } else {
            None
        };
        let (s, words) = (self.types().structure(t), |w: &str| -> Vec<String> {
            w.split(' ').map(String::from).collect()
        });
        match named {
            Some(vs) => vs.into_iter().map(|v| v.0).collect(),
            None if s.is_option() => words("none some"),
            None if s == Ty::bool() => words("false true"),
            None => (0..n).map(|i| i.to_string()).collect(),
        }
    }
    /// `T.V`: variant `V` of the declared sum `T`, and its fields' types.
    fn variant(&self, h: &str) -> Option<(Ty, u8, Vec<Ty>)> {
        let (t, v) = h.rsplit_once('.')?;
        let d = self.decls.get(t)?;
        let (i, Ty::Sum(vs)) = (variants(d)?.iter().position(|x| x.0 == v)?, &d.ty) else {
            return None;
        };
        Some((Ty::Named(t.into()), i as u8, vs[i].clone()))
    }
}

// * Lowering

impl Lo {
    fn ty(&mut self, x: &Sx) -> R<Ty> {
        Ok(match (&x.k, form(x)) {
            (K::Sym(n), _) if n.contains(['<', '(', ',']) => return fail(x.at, format!("`{n}` is no type")),
            (K::Sym(n), _) => self.dty(n).at(x.at)?,
            (_, Some(("id", [t]))) => self.auto(format!("Id<{}>", name(t)?), id_decl()),
            (_, Some(("option", [t]))) => {
                let t = self.ty(t)?;
                self.option(t)
            }
            (_, Some(("list", [t]))) => Ty::list(self.ty(t)?),
            (_, Some(("tuple", ts))) => Ty::Tuple(ts.iter().map(|t| self.ty(t)).collect::<R<_>>()?),
            (_, Some(("row", [t]))) => self.row(name(t)?, t.at)?.1,
            (_, Some(("record" | "sum", _))) => self.decl("", x)?.ty,
            _ => return fail(x.at, "a type expected"),
        })
    }

    /// `(type NAME TY)`'s declaration: a refinement, a record (a refined
    /// field declared as `NAME.field`), a sum, or a name for a type.
    fn decl(&mut self, n: &str, x: &Sx) -> R<Decl> {
        let mut d = decl(Ty::unit(), vec![]);
        let field = |me: &mut Self, f: &Sx, ns: &mut Vec<String>, top: bool| -> R<Ty> {
            let (fname, ft) = pair(f)?;
            ns.push(name(fname)?.into());
            match form(ft) {
                Some(("where", _)) if top => {
                    let d = me.decl("", ft)?;
                    Ok(me.auto(format!("{n}.{}", name(fname)?), d))
                }
                _ => me.ty(ft),
            }
        };
        d.ty = match form(x) {
            Some(("where", [base, rules @ ..])) => {
                let ty = self.ty(base)?;
                for r in rules {
                    let rule = match (form(r), &r.k) {
                        (_, K::Str(w)) if d.refine.last().is_some_and(|r| r.why.is_none()) => {
                            d.refine.last_mut().unwrap().why = Some(w.clone());
                            continue;
                        }
                        (Some((w, [])), _) if RULES.iter().any(|x| x.0 == w) => RULES.iter().find(|x| x.0 == w).unwrap().1.clone(),
                        (Some(("len", [Sx { k: K::Int(a), .. }])), _) => Rule::Len(*a),
                        (Some(("len", [Sx { k: K::Int(a), .. }, b])), _) => match &b.k {
                            K::Int(b) => Rule::LenRange(*a, *b),
                            K::Sym(d) if d == ".." => Rule::LenRange(*a, i64::MAX),
                            _ => return fail(b.at, "a length's bound is an integer or `..`"),
                        },
                        (Some(("fn", [ps, body])), _) => {
                            self.scope = vec![(names(ps)?.concat(), ty.clone())];
                            let e = self.ex(body, None);
                            self.scope.clear();
                            Rule::Refine(close(e?, 1))
                        }
                        _ => return fail(r.at, "a rule: (utf8) (trim) (non-empty) (len …) (fn (x) …), its words after it"),
                    };
                    d.refine.push(Refine { rule, why: None });
                }
                ty
            }
            Some(("record", fs)) => Ty::Tuple(fs.iter().map(|f| field(self, f, &mut d.names, !n.is_empty())).collect::<R<_>>()?),
            Some(("sum", vs)) => {
                let mut out = vec![];
                for v in vs {
                    let Some((vn, fs)) = items(v)?.split_first() else {
                        return fail(v.at, "a variant expected");
                    };
                    d.names.push(name(vn)?.into());
                    out.push(fs.iter().map(|f| field(self, f, &mut d.names, false)).collect::<R<_>>()?);
                }
                Ty::Sum(out)
            }
            _ => self.ty(x)?,
        };
        Ok(d)
    }

    /// `(table NAME [volatile] (COL TY [key] [(refs T)] [(default 0x…)])… (index [unique] COL…)…)`.
    fn table(&mut self, x: &Sx) -> R<()> {
        let Some(("table", [n, rest @ ..])) = form(x) else {
            return fail(x.at, "a table expected");
        };
        let (mut t, mut keys) = (Table::new(name(n)?), 0);
        for c in rest {
            match (&c.k, form(c)) {
                (K::Sym(v), _) if v == "volatile" => t.space = Space::Volatile,
                (_, Some(("index", cols))) => {
                    let unique = cols.first() == Some(&y("unique"));
                    let cols = cols[unique as usize..]
                        .iter()
                        .map(|c| t.column(name(c)?).map_or_else(|| fail(c.at, "no such column"), Ok));
                    t.indexes.push(Index {
                        columns: cols.collect::<R<_>>()?,
                        unique,
                    });
                }
                (K::List(xs), _) if xs.len() >= 2 => {
                    let ty = self.ty(&xs[1])?;
                    let (cname, decl) = (name(&xs[0])?.into(), self.spell(&ty));
                    let mut col = Column {
                        name: cname,
                        decl,
                        ty: Ty::Int,
                        default: None,
                    };
                    for o in &xs[2..] {
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
                _ => return fail(c.at, "`volatile`, a column or an index expected"),
            }
        }
        t.key = keys.max(1);
        self.schema.tables.push(t);
        self.rows = None;
        Ok(())
    }

    /// Function `n`, built once and installed; a cycle is refused.
    fn function(&mut self, n: &str, at: At) -> R<Function> {
        match (self.built.get(n), self.fns.get(n).cloned()) {
            (Some(Some(f)), _) => Ok(f.clone()),
            (Some(None), _) => fail(at, format!("{n} reaches itself, which a hash cannot name")),
            (None, None) => fail(at, format!("no function {n}")),
            (None, Some(x)) => {
                self.built.insert(n.into(), None);
                let saved = (take(&mut self.scope), take(&mut self.stored), take(&mut self.callees));
                let f = self.lower(&x);
                (self.scope, self.stored, self.callees) = saved;
                let f = f?;
                if form(&x).unwrap().0 == "live" && volatile(&self.schema, &self.lib, &f.body) == Some(false) {
                    return fail(x.at, format!("{n}: a live function touches a durable table"));
                }
                self.lib.install(&self.schema, f.clone()).at(x.at)?;
                self.built.insert(n.into(), Some(f.clone()));
                Ok(f)
            }
        }
    }

    /// `(KIND NAME ((P TY)…) [RET] BODY…)`: a query, helper or guard says
    /// what it returns, a scope `Bool`, a mutator or a live function `()`.
    fn lower(&mut self, x: &Sx) -> R<Function> {
        let Some((k, [_, ps, rest @ ..])) = form(x) else {
            return fail(x.at, "a function expected");
        };
        for p in items(ps)? {
            let (pn, pt) = pair(p)?;
            let t = self.ty(pt).map(|t| self.store(t))?;
            self.scope.push((name(pn)?.into(), t));
        }
        let (ret, body) = match (k, rest) {
            ("query" | "helper" | "guard", [r, body @ ..]) => (self.ty(r)?, body),
            ("scope", _) => (Ty::bool(), rest),
            _ => (Ty::unit(), rest),
        };
        let ret = self.store(ret);
        let body = close(self.body(body, Some(&ret), x.at)?, self.scope.len() as u32);
        let mut types = take(&mut self.callees);
        take(&mut self.stored).iter().for_each(|t| self.declare(t, &mut types));
        let params: Vec<Ty> = self.scope.iter().map(|p| p.1.clone()).collect();
        let effect = match KINDS.iter().find(|w| w.0 == k).unwrap().1 {
            Some(e) => e,
            None => self.type_at(&params, &body).at(x.at)?.1,
        };
        Ok(Function {
            types: types.into_iter().collect(),
            params,
            ret,
            effect,
            body,
        })
    }

    /// A body: one form, or several in sequence, the last its value.
    fn body(&mut self, xs: &[Sx], want: Want, at: At) -> R<Expr> {
        let Some((last, first)) = xs.split_last() else {
            return fail(at, "a body expected");
        };
        let mut es = first.iter().map(|x| self.ex(x, None)).collect::<R<Vec<_>>>()?;
        es.push(self.ex(last, want)?);
        Ok(if es.len() == 1 { es.pop().unwrap() } else { Expr::Seq(es) })
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
        match names(ps)? {
            ns if ns.len() == tys.len() => Ok(Expr::Lambda(ns.len() as u32, bx(self.under(ns, tys, |me| me.body(body, want, x.at))?))),
            ns => fail(x.at, format!("{} names where {} are bound", ns.len(), tys.len())),
        }
    }

    fn list_of(&self, e: &Expr, at: At) -> R<Ty> {
        let t = self.ty_of(e, at)?;
        self.elem(Some(&t)).map_or_else(|| fail(at, "a list expected"), Ok)
    }

    /// A table's key from its arguments: one of the key's type as it is,
    /// else the tuple of them.
    fn key(&mut self, t: &str, ks: &[Sx], at: At) -> R<Expr> {
        let (tab, Ty::Tuple(cols)) = self.row(t, at)? else { unreachable!() };
        let es = ks.iter().map(|k| self.ex(k, None)).collect::<R<Vec<_>>>()?;
        match &es[..] {
            [e] if self.types().sub(&self.ty_of(e, at)?, &Ty::Tuple(cols[..tab.key].to_vec())) => Ok(e.clone()),
            _ => Ok(Expr::Tuple(es)),
        }
    }

    /// A field path: `e.a.b`.
    fn path(&mut self, mut e: Expr, fs: &[&str], at: At) -> R<Expr> {
        for f in fs {
            let t = self.ty_of(&e, at)?;
            e = Expr::Field(bx(e), self.field(&t, f).map_or_else(|| fail(at, format!("no field {f}")), Ok)?);
        }
        Ok(e)
    }

    fn ex(&mut self, x: &Sx, want: Want) -> R<Expr> {
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
            return self.path(Expr::Var(MARK + l as u32), &segs[1..], x.at);
        }
        Ok(match n {
            "true" | "false" => Expr::Lit(Ty::bool(), vec![(n == "true") as u8]),
            "none" if self.inner(want).is_some() => Expr::Variant(self.store(want.unwrap().clone()), 0, vec![]),
            "none" => return fail(x.at, "a none no place types: (as (option T) none)"),
            _ if ATOMS[4..].contains(&n) => Expr::Ctx(nth(ATOMS[4..].iter().position(|a| *a == n).unwrap())),
            n if self.schema.table(n).is_some() => Expr::Scan(n.into()),
            n => return fail(x.at, format!("`{n}` is no variable, table or value")),
        })
    }

    fn call(&mut self, x: &Sx, want: Want) -> R<Expr> {
        use Expr::*;
        let (at, Some((h, a))) = (x.at, form(x)) else {
            return fail(x.at, "a form begins with a name");
        };
        if let Some((lo, hi)) = arity(h).filter(|(lo, hi)| !(lo..=hi).contains(&&a.len())) {
            return fail(at, format!("({h} …) takes {lo} to {hi}"));
        }
        let t0 = || name(&a[0]).map(String::from);
        let e = |me: &mut Self, i: usize| me.ex(&a[i], None);
        Ok(match h {
            "let" => return self.lets(items(&a[0])?, &a[1..], want, at),
            "if" => If(
                bx(e(self, 0)?),
                bx(self.ex(&a[1], want)?),
                bx(a.get(2).map_or(Ok(Seq(vec![])), |b| self.ex(b, want))?),
            ),
            "match" => {
                let s = e(self, 0)?;
                let t = self.ty_of(&s, at)?;
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
                    let (ns, k) = (names(bs)?, fields.len() as u32);
                    if flat(v) != label || ns.len() != fields.len() {
                        return fail(arm.at, format!("({label} ({} names) …) is due", fields.len()));
                    }
                    arms.push(Lambda(k, bx(self.under(ns, fields, |me| me.body(body, want, arm.at))?)));
                }
                Match(bx(s), arms)
            }
            "record" | "tuple" => {
                let ns = self.names_of(want).filter(|ns| ns.len() == a.len());
                if (h == "record") != ns.is_some() {
                    return fail(
                        at,
                        [
                            "a record is written (record (field e) …)",
                            "a record no place types by its names: (tuple …)",
                        ][ns.is_none() as usize],
                    );
                }
                let ts = match self.structure(want) {
                    Some(Ty::Tuple(ts)) if ts.len() == a.len() => ts.into_iter().map(Some).collect(),
                    _ => vec![None; a.len()],
                };
                let mut es = vec![];
                for (i, (f, t)) in a.iter().zip(&ts).enumerate() {
                    let f = match &ns {
                        Some(ns) if name(pair(f)?.0)? != ns[i] => return fail(f.at, format!("{} where the field {} is due", flat(f), ns[i])),
                        Some(_) => pair(f)?.1,
                        None => f,
                    };
                    es.push(self.ex(f, t.as_ref())?);
                }
                Tuple(es)
            }
            "some" => {
                let x = self.ex(&a[0], self.inner(want).as_ref())?;
                let t = infer(&self.types(), &self.ty_of(&x, at)?);
                let t = self.option(t);
                Variant(self.store(t), 1, vec![x])
            }
            "list" => {
                let w = self.elem(want);
                let es = a.iter().map(|e| self.ex(e, w.as_ref())).collect::<R<Vec<_>>>()?;
                let Some(first) = es.first() else {
                    return fail(at, "an empty list no place types: (as (list T) (list))");
                };
                let t = infer(&self.types(), &self.ty_of(first, at)?);
                List(self.store(t), es)
            }
            "as" => {
                let t = self.ty(&a[0])?;
                let (t, i, el) = (self.store(t.clone()), self.inner(Some(&t)), self.elem(Some(&t)));
                match (&a[1].k, form(&a[1]), el) {
                    (K::Sym(n), ..) if n == "none" && i.is_some() => Variant(t, 0, vec![]),
                    (_, Some(("some", [v])), _) if i.is_some() => Variant(t, 1, vec![self.ex(v, i.as_ref())?]),
                    (_, Some(("list", es)), Some(el)) => List(el.clone(), es.iter().map(|x| self.ex(x, Some(&el))).collect::<R<_>>()?),
                    _ => return fail(at, "(as T e) types a none, a (some …) or a (list …) of T"),
                }
            }
            "none?" => Un(UnOp::Not, bx(Std(StdFn::IsSome, vec![e(self, 0)?]))),
            "not" => Un(UnOp::Not, bx(e(self, 0)?)),
            "-" if a.len() == 1 => Un(UnOp::Neg, bx(e(self, 0)?)),
            o if OPS.split(' ').any(|p| p == o) => Op(nth(OPS.split(' ').position(|p| p == o).unwrap()), bx(e(self, 0)?), bx(e(self, 1)?)),
            "refuse" => Refuse(bx(e(self, 0)?)),
            "get" => Lookup(t0()?, bx(self.key(&t0()?, &a[1..], at)?)),
            "delete" => Delete(t0()?, bx(self.key(&t0()?, &a[1..], at)?)),
            "insert" => {
                let row = self.row(&t0()?, at)?.1;
                Insert(t0()?, bx(self.ex(&a[1], Some(&row))?))
            }
            "update" => {
                let row = self.row(&t0()?, at)?.1;
                let k = self.key(&t0()?, &a[1..a.len() - 1], at)?;
                Update(t0()?, bx(k), bx(self.lam(&a[a.len() - 1], vec![row.clone()], Some(&row))?))
            }
            "filter" | "map" | "join" => {
                let ls = a[..a.len() - 1].iter().map(|x| self.ex(x, None)).collect::<R<Vec<_>>>()?;
                let f = bx(self.lam(&a[a.len() - 1], ls.iter().map(|x| self.list_of(x, at)).collect::<R<_>>()?, None)?);
                let mut ls = ls.into_iter().map(bx);
                match h {
                    "filter" => Filter(ls.next().unwrap(), f),
                    "map" => Map(ls.next().unwrap(), f),
                    _ => Join(ls.next().unwrap(), ls.next().unwrap(), f),
                }
            }
            "group" => {
                let g = e(self, 0)?;
                let x = self.list_of(&g, at)?;
                let by = self.lam(&a[1], vec![x.clone()], None)?;
                let (Lambda(_, b), stack) = (&by, self.scope.iter().map(|s| s.1.clone()).chain([x.clone()]).collect::<Vec<_>>()) else {
                    unreachable!()
                };
                let k = self.type_at(&stack, &close((**b).clone(), stack.len() as u32)).at(at)?.0;
                Group(bx(g), bx(by), bx(self.lam(&a[2], vec![k, Ty::list(x)], None)?))
            }
            "sort" => {
                let s = e(self, 0)?;
                let x = self.list_of(&s, at)?;
                let Some(("fn", [ps, kb])) = form(&a[1]) else {
                    return fail(a[1].at, "(fn (x) keys) expected");
                };
                let key = |me: &mut Self, k: &Sx| match form(k) {
                    Some(("desc", [e])) => Ok((Dir::Desc, me.ex(e, None)?)),
                    Some(("asc", [e])) => Ok((Dir::Asc, me.ex(e, None)?)),
                    _ => Ok((Dir::Asc, me.ex(k, None)?)),
                };
                let ks = self.under(names(ps)?, vec![x], |me| match form(kb) {
                    Some(("tuple", ks)) if ks.len() > 1 => ks.iter().map(|k| key(me, k)).collect::<R<Vec<_>>>(),
                    _ => Ok(vec![key(me, kb)?]),
                })?;
                let (dirs, mut ks): (Vec<Dir>, Vec<Expr>) = ks.into_iter().unzip();
                let body = if ks.len() == 1 { ks.pop().unwrap() } else { Tuple(ks) };
                Sort(bx(s), bx(Lambda(1, bx(body))), dirs)
            }
            "take" => Take(bx(e(self, 0)?), bx(e(self, 1)?)),
            "new-id" => {
                let t = self.auto(format!("Id<{}>", t0()?), id_decl());
                (self.store(t), Ctx(CtxField::NewId(t0()?))).1
            }
            "has-role" => Std(StdFn::Contains, vec![Ctx(CtxField::Roles), e(self, 0)?]),
            "lit" | "variant" => match (a, h) {
                ([t, Sx { k: K::Bytes(b), .. }], "lit") => Lit(self.ty(t).map(|t| self.store(t))?, b.clone()),
                ([t, Sx { k: K::Int(i), .. }, es @ ..], "variant") => {
                    let t = self.ty(t).map(|t| self.store(t))?;
                    Variant(t, *i as u8, es.iter().map(|e| self.ex(e, None)).collect::<R<_>>()?)
                }
                _ => return fail(at, "(lit T 0x…), (variant T i e …)"),
            },
            "." => {
                let x = e(self, 0)?;
                self.path(x, &a[1..].iter().map(name).collect::<R<Vec<_>>>()?, at)?
            }
            s if STD.split(' ').any(|n| n == s) => {
                let es = a.iter().map(|e| self.ex(e, None)).collect::<R<Vec<_>>>()?;
                let text = es
                    .first()
                    .map(|e| self.ty_of(e, at))
                    .transpose()?
                    .is_some_and(|t| self.types().structure(&t) == Ty::Bytes);
                let f = match (s, es.len()) {
                    ("len", _) if text => StdFn::TextLen,
                    ("len", _) => StdFn::Len,
                    ("concat", 2) => StdFn::ListConcat,
                    _ => nth(STD.split(' ').position(|n| n == s).unwrap()),
                };
                Std(f, es)
            }
            f if self.fns.contains_key(f) => {
                let c = self.function(f, at)?;
                self.callees.extend(c.types.iter().cloned());
                if c.params.len() != a.len() {
                    return fail(at, format!("{f} takes {}", c.params.len()));
                }
                Call(c.hash(), a.iter().zip(&c.params).map(|(e, p)| self.ex(e, Some(p))).collect::<R<_>>()?)
            }
            v => match self.variant(v) {
                Some((t, i, fts)) if fts.len() == a.len() => {
                    Variant(self.store(t), i, a.iter().zip(&fts).map(|(e, f)| self.ex(e, Some(f))).collect::<R<_>>()?)
                }
                Some((.., fts)) => return fail(at, format!("{v} has {} fields", fts.len())),
                None => return fail(at, format!("({h} …) is no form, function or variant")),
            },
        })
    }

    /// `(let ((x e) …) body …)`: each bound for the rest.
    fn lets(&mut self, bs: &[Sx], body: &[Sx], want: Want, at: At) -> R<Expr> {
        let Some((b, rest)) = bs.split_first() else {
            return self.body(body, want, at);
        };
        let (n, v) = pair(b)?;
        let v = self.ex(v, None)?;
        let t = self.ty_of(&v, b.at)?;
        Ok(Expr::Let(
            bx(v),
            bx(self.under(vec![name(n)?.into()], vec![t], |me| me.lets(rest, body, want, at))?),
        ))
    }
}

// * Printing

impl Lo {
    /// A fresh name for a value of `t`, bound: `prefer`, else from its type;
    /// unlike any name in scope, table, function's first segment or atom.
    fn fresh(&mut self, t: &Ty, prefer: Option<&String>) -> String {
        let stem = match (prefer, t, table_of(&self.rows(), t)) {
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
        let tab = table_of(&self.rows(), t).map(|t| t.name.clone());
        match t {
            Ty::Int => y("Int"),
            Ty::Bytes => y("Bytes"),
            Ty::Named(n) if n.starts_with("Id<") && self.is_auto(n) => h("id", [y(&n[3..n.len() - 1])]),
            Ty::Named(n) if self.is_auto(n) => {
                let i = self.inner(Some(t)).unwrap();
                h("option", [self.pty(&i)])
            }
            Ty::Named(n) => y(n),
            Ty::List(e) => h("list", [self.pty(e)]),
            _ if tab.is_some() => h("row", [y(tab.unwrap())]),
            t if *t == Ty::bool() => y("Bool"),
            Ty::Sum(vs) if vs.is_empty() => y("Never"),
            Ty::Tuple(ts) => h("tuple", ts.iter().map(|t| self.pty(t)).collect::<Vec<_>>()),
            Ty::Sum(vs) => {
                let v = |me: &mut Self, i: usize, fs: &[Ty]| {
                    h(
                        &format!("v{i}"),
                        fs.iter().enumerate().map(|(j, f)| h(&format!("f{j}"), [me.pty(f)])).collect::<Vec<_>>(),
                    )
                };
                h("sum", vs.iter().enumerate().map(|(i, fs)| v(self, i, fs)).collect::<Vec<_>>())
            }
        }
    }

    /// `(type N TY)`'s TY: a refinement, a record (its refined fields
    /// inline), a sum, a name for a type.
    fn pdecl(&mut self, n: &str, d: &Decl) -> R<Sx> {
        let field = |me: &mut Self, f: &String, t: &Ty| -> R<Sx> {
            let inline = me.decls.get(&format!("{n}.{f}")).filter(|_| *t == Ty::Named(format!("{n}.{f}"))).cloned();
            Ok(h(
                f,
                [match inline {
                    Some(d) => me.pdecl("", &d)?,
                    None => me.pty(t),
                }],
            ))
        };
        Ok(match (&d.ty, variants(d)) {
            _ if !d.refine.is_empty() => {
                let mut out = vec![self.pty(&d.ty)];
                for r in &d.refine {
                    let i = |n: i64| sx(K::Int(n));
                    out.push(match &r.rule {
                        Rule::Len(a) => h("len", [i(*a)]),
                        Rule::LenRange(a, b) => h("len", [i(*a), if *b == i64::MAX { y("..") } else { i(*b) }]),
                        Rule::Refine(e) => {
                            let v = self.fresh(&d.ty, None);
                            let b = self.pr(e, None);
                            self.scope.clear();
                            h("fn", [l(vec![y(v)]), b?])
                        }
                        r => h(RULES.iter().find(|x| x.1 == *r).unwrap().0, []),
                    });
                    out.extend(r.why.clone().map(|w| sx(K::Str(w))));
                }
                h("where", out)
            }
            (Ty::Tuple(ts), _) if !d.names.is_empty() => h("record", d.names.iter().zip(ts).map(|(f, t)| field(self, f, t)).collect::<R<Vec<_>>>()?),
            (Ty::Sum(ts), Some(vs)) => {
                let v = |me: &mut Self, (v, fs): (String, Vec<String>), ts: &Vec<Ty>| {
                    h(&v, fs.into_iter().zip(ts).map(|(f, t)| h(&f, [me.pty(t)])).collect::<Vec<_>>())
                };
                h("sum", vs.into_iter().zip(ts).map(|(x, ts)| v(self, x, ts)).collect::<Vec<_>>())
            }
            _ => self.pty(&d.ty),
        })
    }

    /// `e`'s forms as a body: a sequence's each, else itself.
    fn forms(&mut self, e: &Expr, want: Want) -> R<Vec<Sx>> {
        match e {
            Expr::Seq(es) if es.len() > 1 => es
                .iter()
                .enumerate()
                .map(|(i, x)| self.pr(x, want.filter(|_| i + 1 == es.len())))
                .collect(),
            e => Ok(vec![self.pr(e, want)?]),
        }
    }
    /// `(head e…)`, the `i`th wanting `wants[i]`.
    fn prs(&mut self, head: &str, es: &[Expr], wants: &[Ty]) -> R<Sx> {
        Ok(h(
            head,
            es.iter().enumerate().map(|(i, e)| self.pr(e, wants.get(i))).collect::<R<Vec<_>>>()?,
        ))
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
        Ok(h("fn", [l(ns), body?]))
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
        let (depth, at) = (self.scope.len(), (0, 0));
        let out = match e {
            Lit(t, b) => {
                let mut r = Reader::new(b);
                sx(match t {
                    Ty::Int => K::Int(r.int()?),
                    Ty::Named(n) if n == "Text" => K::Str(r.text()?),
                    Ty::Bytes => K::Bytes(r.bytes()?),
                    t if *t == Ty::bool() => K::Sym(ATOMS[1 - b[0] as usize].into()),
                    t => return Ok(h("lit", [self.pty(t), sx(K::Bytes(b.clone()))])),
                })
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
                    let (t, v) = (self.type_of(v)?.0, self.pr(v, None)?);
                    binds.push(h(&self.fresh(&t, None), [v]));
                    e = b;
                }
                h("let", [l(binds)].into_iter().chain(self.forms(e, want)?))
            }
            Seq(es) if es.len() == 1 => return Err(Refusal::new("a sequence of one has no spelling")),
            Seq(es) if es.is_empty() => l(vec![]),
            Seq(_) => h("let", [l(vec![])].into_iter().chain(self.forms(e, want)?)),
            If(c, a, b) => {
                let b = if **b == Seq(vec![]) { None } else { Some(self.pr(b, want)?) };
                h("if", [self.pr(c, None)?, self.pr(a, want)?].into_iter().chain(b))
            }
            Match(s, arms) => {
                let t = self.type_of(s)?.0;
                let Ty::Sum(vs) = self.types().structure(&t) else {
                    return Err(Refusal::new("a match on no sum"));
                };
                let fnames = if let Ty::Named(n) = &t {
                    self.decls.get(n).and_then(variants)
                } else {
                    None
                };
                let fnames = fnames.map_or(vec![vec![]; vs.len()], |v| v.into_iter().map(|v| v.1).collect());
                let mut out = vec![y("match"), self.pr(s, None)?];
                for (((label, fs), arm), ns) in self.labels(&t, vs.len()).into_iter().zip(&vs).zip(arms).zip(fnames) {
                    let label = label.parse().map_or_else(|_| y(&label), |i| sx(K::Int(i)));
                    let ns = fs.iter().enumerate().map(|(j, f)| y(self.fresh(f, ns.get(j)))).collect();
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
                    fs.insert(0, n.filter(|n| self.field(&t, n) == Some(*i)).unwrap_or(i.to_string()));
                    e = x;
                }
                match e {
                    Var(_) => y(format!("{}.{}", flat(&self.pr(e, None)?), fs.join("."))),
                    _ => h(".", [self.pr(e, None)?].into_iter().chain(fs.into_iter().map(y))),
                }
            }
            Tuple(es) => {
                let ts = match self.structure(want) {
                    Some(Ty::Tuple(ts)) if ts.len() == es.len() => ts.into_iter().map(Some).collect(),
                    _ => vec![None; es.len()],
                };
                let fs = es.iter().zip(&ts).map(|(e, t)| self.pr(e, t.as_ref())).collect::<R<Vec<_>>>()?;
                match self.names_of(want).filter(|ns| ns.len() == es.len()) {
                    Some(ns) => h("record", ns.into_iter().zip(fs).map(|(n, f)| h(&n, [f]))),
                    None => h("tuple", fs),
                }
            }
            Variant(t, i, es) => {
                let n = if let Ty::Named(n) = t { n.clone() } else { String::new() };
                let named = self.decls.get(&n).and_then(variants).map(|vs| format!("{n}.{}", vs[*i as usize].0));
                match (n.starts_with("Option<") && self.is_auto(&n), *i, named) {
                    (true, 0, _) if want == Some(t) => y("none"),
                    (true, 0, _) => h("as", [self.pty(t), y("none")]),
                    (true, ..) => {
                        let it = infer(&self.types(), &self.type_of(&es[0])?.0);
                        let natural = Ty::Named(format!("Option<{}>", self.spell(&it))) == *t;
                        let inner: Vec<Ty> = self.inner(if natural { want } else { Some(t) }).into_iter().collect();
                        let some = self.prs("some", es, &inner)?;
                        if natural {
                            some
                        } else {
                            h("as", [self.pty(t), some])
                        }
                    }
                    (_, i, Some(c)) if !self.names.values().any(|f| *f == c) => {
                        let Ty::Sum(fts) = self.types().structure(t) else { unreachable!() };
                        self.prs(&c, es, &fts[i as usize])?
                    }
                    _ => h(
                        "variant",
                        [self.pty(t), sx(K::Int(*i as i64))]
                            .into_iter()
                            .chain(es.iter().map(|e| self.pr(e, None)).collect::<R<Vec<_>>>()?),
                    ),
                }
            }
            List(t, es) => {
                let natural = es.first().map(|x| self.type_of(x).map(|u| infer(&self.types(), &u.0))).transpose()? == Some(t.clone());
                let w = if natural { self.elem(want) } else { Some(t.clone()) };
                let list = self.prs("list", es, &w.map_or(vec![], |w| vec![w; es.len()]))?;
                if natural {
                    list
                } else {
                    h("as", [self.pty(&Ty::list(t.clone())), list])
                }
            }
            Op(o, a, b) => self.prs(OPS.split(' ').nth(ord(o)).unwrap(), &[(**a).clone(), (**b).clone()], &[])?,
            Un(UnOp::Not, a) => match &**a {
                Std(StdFn::IsSome, x) => self.prs("none?", x, &[])?,
                a => self.prs("not", std::slice::from_ref(a), &[])?,
            },
            Un(_, a) => self.prs("-", std::slice::from_ref(&**a), &[])?,
            Std(StdFn::Contains, es) if es.first() == Some(&Ctx(CtxField::Roles)) => self.prs("has-role", &es[1..], &[])?,
            Std(f, es) => self.prs(STD.split(' ').nth(ord(f)).unwrap(), es, &[])?,
            Call(c, es) => {
                let params = self.lib.get(c).ok_or_else(|| Refusal(format!("no function {c}")))?.f.params.clone();
                let n = self.names.get(c).cloned().ok_or_else(|| Refusal(format!("no name for {c}")))?;
                self.prs(&n, es, &params)?
            }
            Ctx(CtxField::NewId(t)) => h("new-id", [y(t)]),
            Ctx(c) => y(ATOMS[4 + ord(c)]),
            Lambda(..) => return Err(Refusal::new("a lambda outside an operator")),
            Scan(t) => y(t),
            Lookup(t, k) | Delete(t, k) => h(
                if matches!(e, Lookup(..)) { "get" } else { "delete" },
                [y(t)].into_iter().chain(self.pkey(t, k)?),
            ),
            Insert(t, r) => {
                let row = self.row(t, at)?.1;
                h("insert", [y(t), self.pr(r, Some(&row))?])
            }
            Update(t, k, f) => {
                let row = self.row(t, at)?.1;
                let f = self.plam(f, std::slice::from_ref(&row), Some(&row))?;
                h("update", [y(t)].into_iter().chain(self.pkey(t, k)?).chain([f]))
            }
            Filter(s, f) | Map(s, f) => {
                let x = self.list_of(s, at)?;
                h(
                    if matches!(e, Filter(..)) { "filter" } else { "map" },
                    [self.pr(s, None)?, self.plam(f, &[x], None)?],
                )
            }
            Join(a, b, on) => {
                let tys = [self.list_of(a, at)?, self.list_of(b, at)?];
                h("join", [self.pr(a, None)?, self.pr(b, None)?, self.plam(on, &tys, None)?])
            }
            Group(g, by, agg) => {
                let (x, Lambda(_, kb)) = (self.list_of(g, at)?, &**by) else {
                    return Err(Refusal::new("a lambda expected"));
                };
                self.scope.push((String::new(), x.clone()));
                let k = self.type_of(kb).map(|t| t.0);
                self.scope.pop();
                let (b, a) = (self.plam(by, std::slice::from_ref(&x), None)?, self.plam(agg, &[k?, Ty::list(x)], None)?);
                h("group", [self.pr(g, None)?, b, a])
            }
            Sort(s, keys, dirs) => {
                let (x, Lambda(_, kb)) = (self.list_of(s, at)?, &**keys) else {
                    return Err(Refusal::new("a lambda expected"));
                };
                let v = self.fresh(&x, None);
                let key = |me: &mut Self, k: &Expr, d: &Dir, bare: bool| -> R<Sx> {
                    let k = me.pr(k, None)?;
                    Ok(if *d == Dir::Desc {
                        h("desc", [k])
                    } else if bare {
                        k
                    } else {
                        h("asc", [k])
                    })
                };
                let body = match (&**kb, &dirs[..]) {
                    (Tuple(ks), ds) if ds.len() > 1 && ks.len() == ds.len() => {
                        h("tuple", ks.iter().zip(ds).map(|(k, d)| key(self, k, d, true)).collect::<R<Vec<_>>>()?)
                    }
                    (k, [d]) => key(self, k, d, !matches!(k, Tuple(_)))?,
                    _ => return Err(Refusal::new("sort keys that are no tuple of their directions")),
                };
                self.scope.truncate(depth);
                h("sort", [self.pr(s, None)?, h("fn", [l(vec![y(v)]), body])])
            }
            Take(s, n) => self.prs("take", &[(**s).clone(), (**n).clone()], &[])?,
            Refuse(m) => self.prs("refuse", std::slice::from_ref(&**m), &[])?,
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
        let ps = f.params.iter().map(|t| h(&self.fresh(t, None), [self.pty(t)])).collect();
        let ret = ["query", "helper"].contains(&kind).then(|| self.pty(&f.ret));
        let out = h(kind, [y(n), l(ps)].into_iter().chain(ret).chain(self.forms(&f.body, Some(&f.ret))?));
        self.scope.clear();
        Ok(out)
    }
}

// * The module

/// §1.15 A module from its text: the schema, the declarations, every
/// function checked (callees first) and bound by its name unless the name
/// starts with `_`; the module's declarations are its bound functions'.
pub fn build(src: &str) -> R<Domain> {
    let (mut lo, mut types, mut tables) = (Lo::default(), vec![], vec![]);
    for x in read(src)? {
        match form(&x) {
            Some(("schema", ts)) => tables.extend(ts.iter().cloned()),
            Some(("type", [n, t])) => types.push((name(n)?.to_string(), t.clone())),
            Some((k, [n, ..])) if KINDS.iter().any(|w| w.0 == k) && !lo.fns.contains_key(name(n)?) => _ = lo.fns.insert(name(n)?.into(), x.clone()),
            _ => return fail(x.at, "a schema, a type or a function (each name once) expected"),
        }
    }
    for (n, _) in &types {
        lo.decls.insert(n.clone(), decl(Ty::unit(), vec![]));
    }
    tables.iter().try_for_each(|t| lo.table(t))?;
    // Twice: a row a declaration names is read under the others.
    for (n, t) in types.iter().chain(&types) {
        let d = lo.decl(n, t)?;
        lo.decls.insert(n.clone(), d);
        lo.rows = None;
    }
    let cols = lo
        .schema
        .tables
        .iter()
        .flat_map(|t| &t.columns)
        .map(|c| c.decl.clone())
        .collect::<Vec<_>>();
    let cols = cols.iter().map(|c| lo.dty(c).map_err(Refusal)).collect::<R<Vec<_>>>()?;
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
    let scope = vars.iter().map(|v| (v.0.to_string(), v.2.clone())).collect();
    let mut lo = Lo {
        schema: schema.clone(),
        decls: decls.iter().cloned().collect(),
        scope,
        ..Lo::default()
    };
    let e = lo.ex(x, None)?;
    let (t, k) = (lo.type_of(&e)?.0, vars.len() as u32);
    let e = levels(e, &|l| {
        if l < k {
            vars[l as usize].1.clone()
        } else {
            Expr::Var(MARK + depth + l - k)
        }
    });
    let mut d = BTreeMap::new();
    take(&mut lo.stored).iter().for_each(|t| lo.declare(t, &mut d));
    Ok((e, t, d.into_iter().collect()))
}

/// A module's printing context: its declarations, its columns' too.
fn printing(m: &Module, lib: Lib) -> Lo {
    let mut lo = Lo {
        schema: m.schema.clone(),
        decls: m.types.iter().cloned().collect(),
        lib,
        ..Lo::default()
    };
    for c in m.schema.tables.iter().flat_map(|t| &t.columns) {
        if lo.dty(&c.decl).is_err() {
            lo.decls.insert(c.decl.clone(), decl(c.ty.clone(), vec![]));
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
    let (mut out, mut tables) = (vec![], vec![]);
    for t in &m.schema.tables {
        let mut x = vec![y(&t.name)];
        x.extend((t.space == Space::Volatile).then(|| y("volatile")));
        for (i, c) in t.columns.iter().enumerate() {
            let ty = lo.dty(&c.decl).map_err(Refusal)?;
            let refs = t.refs.iter().filter(|r| r.column == i).map(|r| h("refs", [y(&r.table)]));
            let def = c.default.clone().map(|d| h("default", [sx(K::Bytes(d))]));
            x.push(l([y(&c.name), lo.pty(&ty)]
                .into_iter()
                .chain((i < t.key).then(|| y("key")))
                .chain(refs)
                .chain(def)
                .collect()));
        }
        let index = |ix: &Index| {
            h(
                "index",
                ix.unique
                    .then(|| y("unique"))
                    .into_iter()
                    .chain(ix.columns.iter().map(|c| y(&t.columns[*c].name))),
            )
        };
        x.extend(t.indexes.iter().map(index));
        tables.push(h("table", x));
    }
    out.extend((!tables.is_empty()).then(|| h("schema", tables)));
    let decls = lo.decls.clone();
    let inline = |n: &str| {
        n.rsplit_once('.')
            .is_some_and(|(r, f)| decls.get(r).is_some_and(|d| d.names.iter().any(|x| x == f)))
    };
    for (n, d) in &decls {
        if !lo.is_auto(n) && !inline(n) {
            out.push(h("type", [y(n), lo.pdecl(n, d)?]));
        }
    }
    let mut fns: Vec<(String, &Function)> = functions.iter().map(|f| (lo.names[&f.hash()].clone(), f)).collect();
    for (n, x) in &m.bindings {
        fns.extend(functions.iter().filter(|f| f.hash() == *x && lo.names[x] != *n).map(|f| (n.clone(), f)));
    }
    fns.sort_by(|a, b| a.0.cmp(&b.0));
    for (n, f) in fns {
        out.push(lo.pfn(&n, f)?);
    }
    Ok(out.iter().map(|x| layout(x, 0)).collect::<Vec<_>>().join("\n\n") + "\n")
}

/// One function of a module as text, for a browser of it: `name` its binding.
pub fn print_function(m: &Module, lib: &Lib, name: &str) -> R<String> {
    let f = lib
        .get(&m.get(name).ok_or_else(|| Refusal(format!("no function {name}")))?)
        .ok_or_else(|| Refusal::new("not loaded"))?;
    Ok(layout(&printing(m, lib.clone()).pfn(name, &f.f)?, 0))
}
