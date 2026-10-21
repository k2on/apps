//! §1.15 Typed bindings, generated from a module: types only. [`rust`]
//! writes a host's Rust for a module — a struct per record and per table's
//! row, an enum per sum, an alias per refinement or other name, and per
//! bound function, in a module per dotted prefix, a stub of its
//! parameters' types that encodes them, calls the function by its binding
//! through a [`Call`] and decodes what it answers. No body is generated:
//! the module is the program, the stubs only name it. A generated file
//! carries its [`Shape`], what it was made from, and [`fits`] holds a
//! loaded module to it: the schema may have grown (§1.2), and every type
//! and function the bindings name must be of the structure it was.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::marker::PhantomData;
use std::path::Path;

use crate::check::Types;
use crate::codec::Reader;
use crate::hash::{hex, unhex};
use crate::ir::{Decl, Effect, Function};
use crate::log::EntryId;
use crate::module::Domain;
use crate::peer::Ctx;
use crate::schema::Schema;
use crate::store::{Change, Store};
use crate::ty::Ty;
use crate::Refusal;

pub use crate::ir::Node;
pub use ark_macros::Node;

/// An id of a row of `R`'s table: sixteen bytes, `Id<t>`.
pub struct Id<R>(pub [u8; 16], PhantomData<fn() -> R>);

impl<R> Id<R> {
    pub fn new(b: [u8; 16]) -> Id<R> {
        Id(b, PhantomData)
    }
}
impl<R> Clone for Id<R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R> Copy for Id<R> {}
impl<R> Default for Id<R> {
    fn default() -> Self {
        Id::new([0; 16])
    }
}
impl<R> PartialEq for Id<R> {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<R> Eq for Id<R> {}
impl<R> PartialOrd for Id<R> {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl<R> Ord for Id<R> {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.cmp(&o.0)
    }
}
impl<R> std::hash::Hash for Id<R> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.0.hash(h)
    }
}
impl<R> std::fmt::Debug for Id<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Id({})", hex(&self.0))
    }
}
impl<R> Node for Id<R> {
    fn ty() -> Ty {
        Ty::Bytes
    }
    fn put(&self, out: &mut Vec<u8>) {
        crate::codec::put_bytes(out, &self.0);
    }
    fn take(r: &mut Reader) -> Result<Self, Refusal> {
        r.fixed::<16>().map(Id::new)
    }
}

/// What a stub calls a function through, by its binding: a replica, a
/// hub, the domain alone.
pub trait Call {
    /// What authoring an intent gives back.
    type Authored;
    /// A mutator or a live transition, authored with `input`.
    fn author(&mut self, name: &str, input: Vec<u8>) -> Self::Authored;
    /// A query's or a helper's answer to `input`, as a reader of `want`
    /// reads it ([`answer`]).
    fn ask(&self, name: &str, input: Vec<u8>, want: &Ty) -> Result<Vec<u8>, String>;
}

/// The domain alone, over an empty store, for nobody at time 0: a
/// helper's answer on the host, a mutator's changes.
impl Call for Domain {
    type Authored = Result<Vec<Change>, String>;
    fn author(&mut self, name: &str, input: Vec<u8>) -> Self::Authored {
        self.run(&Store::new(self.schema.clone()), &nobody(), name, &input).map_err(|r| r.0)
    }
    fn ask(&self, name: &str, input: Vec<u8>, want: &Ty) -> Result<Vec<u8>, String> {
        let b = Domain::ask(self, &Store::new(self.schema.clone()), &nobody(), name, &input).map_err(|r| r.0)?;
        answer(self, name, &b, want).map_err(|r| r.0)
    }
}

fn nobody() -> Ctx {
    Ctx {
        draws: Default::default(),
        id: EntryId::of("host", 0),
        actor: String::new(),
        session: String::new(),
        roles: vec![],
        now: 0,
    }
}

/// Bytes of type `have` as a reader of `want` reads them: each tuple cut
/// to the fields `want` knows — a row of a table that grew (§1.2's
/// prefix rule) — and the rest as they are. `None` when `want` cannot.
pub fn project(want: &Ty, have: &Ty, bytes: &[u8]) -> Option<Vec<u8>> {
    fn go(w: &Ty, h: &Ty, b: &[u8], r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
        let at = r.at;
        match (w, h) {
            (Ty::Tuple(ws), Ty::Tuple(hs)) if ws.len() <= hs.len() => hs.iter().enumerate().try_for_each(|(i, h)| match ws.get(i) {
                Some(w) => go(w, h, b, r, out),
                None => crate::codec::skip(h, r).ok(),
            })?,
            (Ty::List(w), Ty::List(h)) => {
                while r.more().ok()? {
                    out.push(1);
                    go(w, h, b, r, out)?;
                }
                out.push(0);
            }
            (Ty::Sum(ws), Ty::Sum(hs)) if ws.len() == hs.len() => {
                let i = r.variant(hs.len()).ok()?;
                out.push(i);
                go(&Ty::Tuple(ws[i as usize].clone()), &Ty::Tuple(hs[i as usize].clone()), b, r, out)?;
            }
            _ if w == h => {
                crate::codec::skip(h, r).ok()?;
                out.extend_from_slice(&b[at..r.at]);
            }
            _ => return None,
        }
        Some(())
    }
    let (mut r, mut out) = (Reader::new(bytes), vec![]);
    go(want, have, bytes, &mut r, &mut out)?;
    r.end().ok().map(|_| out)
}

/// Whether a reader of `want` can read a value of `have` ([`project`]).
fn fit(want: &Ty, have: &Ty) -> bool {
    match (want, have) {
        (Ty::Tuple(ws), Ty::Tuple(hs)) => ws.len() <= hs.len() && ws.iter().zip(hs).all(|(w, h)| fit(w, h)),
        (Ty::List(w), Ty::List(h)) => fit(w, h),
        (Ty::Sum(ws), Ty::Sum(hs)) => ws.len() == hs.len() && ws.iter().zip(hs).all(|(w, h)| fit(&Ty::Tuple(w.clone()), &Ty::Tuple(h.clone()))),
        _ => want == have,
    }
}

/// A function's answer, `bytes`, as a reader of `want` reads it: the
/// bindings' result read from the loaded module's ([`project`]).
pub fn answer(d: &Domain, name: &str, bytes: &[u8], want: &Ty) -> Result<Vec<u8>, Refusal> {
    let f = d
        .lib
        .get(&d.module.get(name).ok_or_else(|| Refusal(format!("no function {name}")))?)
        .expect("a bound function");
    let have = Types::new(&f.f.types).deep(&f.f.ret);
    project(want, &have, bytes).ok_or_else(|| Refusal(format!("{name}: an answer the bindings cannot read")))
}

/// Every row of `table` in `store`, each as a reader of `T` reads it.
pub fn rows<T: Node>(store: &dyn crate::store::Read, table: &str) -> Vec<T> {
    let have = store
        .schema()
        .table(table)
        .map(|(_, t)| Ty::Tuple(t.columns.iter().map(|c| c.ty.clone()).collect()));
    let read = |b: Vec<u8>| project(&T::ty(), have.as_ref()?, &b).and_then(|b| T::from_bytes(&b).ok());
    store
        .scan(table, &crate::store::Scan::all())
        .into_iter()
        .map(|b| read(b).expect("a row the bindings read"))
        .collect()
}

/// What bindings were generated from: the schema, the declarations, and
/// each bound function's parameters and result.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Shape {
    pub schema: Schema,
    pub types: Vec<(String, Decl)>,
    pub fns: Vec<(String, (Vec<Ty>, Ty))>,
}

impl Shape {
    pub fn of(d: &Domain) -> Shape {
        let f = |h| d.lib.get(h).expect("a bound function").f.clone();
        let fns = d.module.bindings.iter().map(|(n, h)| (n.clone(), (f(h).params, f(h).ret))).collect();
        Shape {
            schema: d.module.schema.clone(),
            types: d.module.types.clone(),
            fns,
        }
    }
}

/// Whether bindings made from `shape` (hex) may call `d`: its schema a
/// prefix-extension of theirs (§1.2), and every type and function they
/// name there of the structure they were made with, but for rows grown
/// by appended columns, which [`answer`] and [`rows`] cut.
pub fn fits(shape: &str, d: &Domain) -> Result<(), Refusal> {
    let want = Shape::from_bytes(&unhex(shape).ok_or_else(|| Refusal::new("a shape is hex"))?)?;
    let had = Shape::of(d);
    crate::schema::compat(&want.schema, &had.schema).map_err(|r| Refusal(format!("the bindings' schema: {r}")))?;
    let (a, b) = (Types::new(&want.types), Types::new(&had.types));
    let same = |x: &Ty, y: &Ty| fit(&a.deep(x), &b.deep(y));
    for (n, d) in &want.types {
        if !had.types.iter().any(|(m, e)| m == n && same(&d.ty, &e.ty)) {
            return Err(Refusal(format!("{n} is not the type the bindings were made with")));
        }
    }
    for (n, (ps, r)) in &want.fns {
        let fit = |(qs, s): &(Vec<Ty>, Ty)| ps.len() == qs.len() && ps.iter().zip(qs).all(|(p, q)| same(p, q)) && same(r, s);
        if !had.fns.iter().any(|(m, f)| m == n && fit(f)) {
            return Err(Refusal(format!("{n} is not the function the bindings call")));
        }
    }
    Ok(())
}

const KEYWORDS: &str = "as async await break const continue crate dyn else enum extern false fn for gen if impl in let loop \
    match mod move mut pub ref return self static struct super trait true try type unsafe use where while yield";

fn ident(n: &str) -> String {
    match KEYWORDS.split(' ').any(|k| k == n) {
        true => format!("r#{n}"),
        false => n.into(),
    }
}

fn camel(s: &str) -> String {
    s.split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect()
}

/// A column's declared name as the type a function sees.
fn column(s: &str) -> Ty {
    match s {
        "Int" => Ty::Int,
        "Bytes" => Ty::Bytes,
        "Bool" => Ty::bool(),
        _ if s.starts_with("List<") => Ty::list(column(&s[5..s.len() - 1])),
        _ => Ty::Named(s.into()),
    }
}

struct Gen<'a> {
    decls: BTreeMap<String, Decl>,
    rows: Vec<(String, Ty)>,
    types: &'a Types,
}

impl Gen<'_> {
    /// A type's Rust spelling.
    fn rust(&self, t: &Ty) -> String {
        let row = self.rows.iter().find(|r| r.1 == *t);
        match t {
            Ty::Int => "i64".into(),
            Ty::Bytes => "Vec<u8>".into(),
            Ty::List(e) => format!("Vec<{}>", self.rust(e)),
            Ty::Named(n) if n == "Text" => "String".into(),
            Ty::Named(n) if n.starts_with("Id<") => format!("Id<{}>", camel(&n[3..n.len() - 1])),
            Ty::Named(n) if n.starts_with("Option<") || n.contains('.') => self.rust(&self.decls[n].ty),
            Ty::Named(n) => n.clone(),
            _ if row.is_some() => row.unwrap().0.clone(),
            t if *t == Ty::bool() => "bool".into(),
            Ty::Sum(vs) if Ty::Sum(vs.clone()).is_option() => format!("Option<{}>", self.rust(&vs[1][0])),
            Ty::Tuple(ts) => format!("({})", ts.iter().map(|t| self.rust(t) + ",").collect::<String>()),
            Ty::Sum(_) => panic!("a sum with no name has no Rust type: {t:?}"),
        }
    }

    /// Whether a type's Rust has a `Default`: every field's has.
    fn plain(&self, t: &Ty, n: u32) -> bool {
        match self.types.structure(t) {
            _ if n == 0 => false,
            Ty::Tuple(ts) => ts.iter().all(|t| self.plain(t, n - 1)),
            Ty::Sum(vs) => Ty::Sum(vs.clone()).is_option() || vs == [vec![], vec![]],
            _ => true,
        }
    }

    /// Whether a type's Rust is `Copy`: no text, bytes or list in it.
    fn copy(&self, t: &Ty, n: u32) -> bool {
        match t {
            _ if n == 0 => false,
            Ty::Named(m) if m == "Text" => false,
            Ty::Named(m) if m.starts_with("Id<") => true,
            Ty::Named(m) => self.decls.get(m).is_some_and(|d| self.copy(&d.ty, n - 1)),
            Ty::Int => true,
            Ty::Tuple(ts) => ts.iter().all(|t| self.copy(t, n - 1)),
            Ty::Sum(vs) => vs.iter().flatten().all(|t| self.copy(t, n - 1)),
            _ => false,
        }
    }

    /// What a declared type's Rust derives.
    fn derives(&self, t: &Ty) -> String {
        let (copy, default) = (self.copy(t, 8), matches!(t, Ty::Tuple(_)) && self.plain(t, 8));
        format!(
            "#[derive(Clone, {}Debug, PartialEq, Eq, {}::ark::gen::Node)]",
            if copy { "Copy, " } else { "" },
            if default { "Default, " } else { "" }
        )
    }

    fn record(&self, out: &mut String, name: &str, fields: &[(String, Ty)]) {
        let derives = self.derives(&Ty::Tuple(fields.iter().map(|f| f.1.clone()).collect()));
        let _ = writeln!(out, "{derives}\npub struct {name} {{");
        for (f, t) in fields {
            let _ = writeln!(out, "    pub {}: {},", ident(f), self.rust(t));
        }
        out.push_str("}\n\n");
    }

    fn stub(&self, out: &mut String, at: &str, full: &str, f: &Function) {
        let mut ps = vec![];
        for (i, t) in f.params.iter().enumerate() {
            let n = f.names.get(i).filter(|n| !n.is_empty() && f.names[..i].iter().all(|m| m != *n));
            let n = n.map_or(format!("p{i}"), |n| ident(n));
            ps.push(match t {
                Ty::Named(x) if x == "Text" => (format!(", {n}: &str"), format!("::ark::codec::put_bytes(&mut b, {n}.as_bytes());")),
                Ty::List(e) => (format!(", {n}: &[{}]", self.rust(e)), format!("::ark::ir::Node::put_list({n}, &mut b);")),
                t => (format!(", {n}: &{}", self.rust(t)), format!("::ark::ir::Node::put({n}, &mut b);")),
            });
        }
        let (sig, enc): (String, Vec<String>) = ps.into_iter().unzip();
        let input = match &enc[..] {
            [] => "Vec::new()".to_string(),
            enc => format!(
                "{{\n{at}        let mut b = Vec::new();\n{at}        {}\n{at}        b\n{at}    }}",
                enc.join(&format!("\n{at}        "))
            ),
        };
        let name = ident(full.rsplit('.').next().unwrap());
        let _ = if f.effect == Effect::Write && f.ret == Ty::unit() {
            writeln!(
                out,
                "{at}/// `{full}`\n{at}pub fn {name}<C: Call>(peer: &mut C{sig}) -> C::Authored {{\n{at}    peer.author(\"{full}\", {input})\n{at}}}"
            )
        } else {
            let r = self.rust(&f.ret);
            writeln!(
                out,
                "{at}/// `{full}`\n{at}pub fn {name}<C: Call>(peer: &C{sig}) -> Result<{r}, String> {{\n{at}    \
                 let b = peer.ask(\"{full}\", {input}, &<{r} as ::ark::ir::Node>::ty())?;\n{at}    <{r} as ::ark::ir::Node>::from_bytes(&b).map_err(|r| r.0)\n{at}}}"
            )
        };
    }
}

/// §1.15 A module's bindings as Rust ([the module's doc](self)): its hash
/// `HASH`, its shape `SHAPE` and `fits`, the types, then the stubs.
pub fn rust(d: &Domain) -> String {
    let types = Types::new(&d.module.types);
    let col = |c: &crate::schema::Column| match column(&c.decl) {
        t @ Ty::Named(_) => t,
        t => types.deep(&t),
    };
    let rows = d
        .module
        .schema
        .tables
        .iter()
        .map(|t| (camel(&t.name), Ty::Tuple(t.columns.iter().map(col).collect())));
    let g = Gen {
        decls: d.module.types.iter().cloned().collect(),
        rows: rows.collect(),
        types: &types,
    };
    let mut out = format!(
        "// The bindings of module {}, written by `ark::gen::rust`: types and stubs only.\n\n\
         pub const HASH: &str = \"{}\";\npub const SHAPE: &str = \"{}\";\n\
         pub use ::ark::gen::{{Call, Id}};\n\n\
         /// Whether a loaded module is one these bindings may call.\n\
         pub fn fits(d: &::ark::Domain) -> Result<(), ::ark::Refusal> {{\n    ::ark::gen::fits(SHAPE, d)\n}}\n\n",
        d.module.hash(),
        d.module.hash(),
        hex(&Shape::of(d).to_bytes())
    );
    for t in &d.module.schema.tables {
        g.record(
            &mut out,
            &camel(&t.name),
            &t.columns.iter().map(|c| (c.name.clone(), column(&c.decl))).collect::<Vec<_>>(),
        );
    }
    for (n, decl) in &d.module.types {
        if n.contains(['<', '.']) || g.rows.iter().any(|r| r.0 == *n) {
            continue;
        }
        let names = |ts: &[Ty], at: usize| decl.names[at..at + ts.len()].iter().cloned().zip(ts.iter().cloned()).collect::<Vec<_>>();
        match (&decl.ty, decl.names.is_empty() || !decl.refine.is_empty()) {
            (Ty::Tuple(ts), false) => g.record(&mut out, n, &names(ts, 0)),
            (Ty::Sum(vs), false) => {
                let _ = writeln!(out, "{}\npub enum {n} {{", g.derives(&decl.ty));
                let mut at = 0;
                for v in vs {
                    let fs: Vec<String> = names(v, at + 1).iter().map(|(f, t)| format!("{}: {}", ident(f), g.rust(t))).collect();
                    let _ = match fs.is_empty() {
                        true => writeln!(out, "    {},", decl.names[at]),
                        false => writeln!(out, "    {} {{ {} }},", decl.names[at], fs.join(", ")),
                    };
                    at += v.len() + 1;
                }
                out.push_str("}\n\n");
            }
            (t, _) => _ = writeln!(out, "pub type {n} = {};\n", g.rust(t)),
        }
    }
    let mut open: Vec<String> = vec![];
    for (full, h) in &d.module.bindings {
        let path: Vec<&str> = full.split('.').collect();
        let path = &path[..path.len() - 1];
        while !path.starts_with(&open.iter().map(String::as_str).collect::<Vec<_>>()) {
            open.pop();
            let _ = writeln!(out, "{}}}\n", "    ".repeat(open.len()));
        }
        for p in &path[open.len()..] {
            let _ = writeln!(
                out,
                "{}pub mod {} {{\n{}    use super::*;\n",
                "    ".repeat(open.len()),
                ident(p),
                "    ".repeat(open.len())
            );
            open.push(p.to_string());
        }
        g.stub(&mut out, &"    ".repeat(open.len()), full, &d.lib.get(h).expect("a bound function").f);
        out.push('\n');
    }
    while open.pop().is_some() {
        let _ = writeln!(out, "{}}}\n", "    ".repeat(open.len()));
    }
    out
}

/// For a `build.rs`: the domain at `dir` (§1.15's directory) built, its
/// bindings written to `OUT_DIR/domain.rs` with `MODULE`, the module file
/// at `OUT_DIR/module.bin`; built again when anything under `dir` changes.
pub fn build_script(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let d = crate::sexp::build_dir(dir).unwrap_or_else(|r| panic!("{}: {r}", dir.display()));
    let out = Path::new(&std::env::var_os("OUT_DIR").expect("OUT_DIR, as cargo sets it")).to_path_buf();
    let module = "pub const MODULE: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/module.bin\"));\n";
    std::fs::write(out.join("module.bin"), d.to_file()).expect("writing module.bin");
    std::fs::write(out.join("domain.rs"), rust(&d) + module).expect("writing domain.rs");
}
