//! A domain read from its source (`docs/plan-v5.md`, Landed G1): the
//! crate's files parsed (`syn`), never compiled, into what the compiled
//! domain's [`Catalog`] says of it — its schema and every function by name,
//! kind, input, answer and refinements — and the twins its types ask UniFFI
//! for (`#[ark(ffi)]`, `#[ark(ffi_view)]`, [`Twin`]). The derives' rules
//! (`ark-macros/src/domain.rs`, `ffi.rs`) are mirrored here, rule for rule:
//! `Row`, `Input`, `Describe`, `Schema`, `Effect`, and `#[mutator]`,
//! `#[live]`, `#[query]`, `#[view]`, `#[scope]` registered through the
//! domain's `register!(Domain::new::<S>(..); paths)` and its `.with(..)`s.
//!
//! Names resolve as rustc resolves them within the crate — its modules, each
//! one's items, `use`s and globs, `crate::`, `self::`, `super::`, type
//! aliases (generic ones too) — and outside it to what is known here:
//! `ark`'s (`Id`, `Job`, `Plan`, …, its prelude), the standard library's
//! (`Option`, `Vec`, `Box`, `String`, `Result`, the integers, `bool`) and
//! `ark-auth`'s, read as it is built (its rows and inputs, and
//! `ark_auth::register`). Anything else is refused with its file and line:
//! what this cannot read, it never reads differently.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ark::domain::Kind;
use ark::row::{Describe, Row as _, Type};
use ark::schema::{Column, Index, Ref, Schema, Space, Table};
use ark_explorer::data::Described;
use ark_explorer::Catalog;
use proc_macro2::Span;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Ident, Item, Token};

/// What a domain's source says: its catalog, as the compiled domain's, and
/// its twins, in the order the crate declares them.
pub struct Read {
    pub catalog: Catalog,
    pub twins: Vec<Twin>,
    /// Whether the crate sets up UniFFI's scaffolding
    /// (`uniffi::setup_scaffolding!()`), under the features read.
    pub scaffolding: bool,
}

/// A type with its UniFFI twin: its name, the module it is declared in
/// (`harken_domain::types`, UniFFI's `module_path!()`), whether it is a
/// view's row (`ffi_view`: its view object too), and its shape, each field
/// as the type it crosses as.
pub struct Twin {
    pub name: String,
    pub module: String,
    pub view: bool,
    pub shape: Shape,
}

pub enum Shape {
    Record(Vec<(String, Crosses)>),
    Enum(Vec<(String, Vec<(String, Crosses)>)>),
}

/// What a field crosses UniFFI as (`ark_ffi::Twin`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Crosses {
    I64,
    U64,
    I32,
    U32,
    U8,
    Bool,
    String,
    /// A row id: the bridge's `RowId`.
    Id,
    Option(Box<Crosses>),
    Vec(Box<Crosses>),
    /// Another twin: a record or an enum, and the module it is declared in.
    Record(String, String),
    Enum(String, String),
}

/// What is read: the crate's directory (its `src/lib.rs` the root), its
/// name as Rust spells it, and the features on (`cfg(feature = ..)`,
/// `cfg_attr(feature = .., ..)`).
pub struct Options {
    pub dir: PathBuf,
    pub crate_name: String,
    pub features: BTreeSet<String>,
}

/// A refusal: where, and why.
fn at(file: &Path, span: Span, msg: impl std::fmt::Display) -> String {
    let s = span.start();
    format!("{}:{}:{}: {msg}", file.display(), s.line, s.column + 1)
}

/// The domain's source under `o`, read.
pub fn read(o: &Options) -> Result<Read, String> {
    let mut c = Crate {
        o,
        modules: vec![],
        defs: vec![],
        aliases: vec![],
        fns: vec![],
        scaffolding: false,
        looked: Default::default(),
        cycles: Default::default(),
    };
    c.load(vec![], o.dir.join("src/lib.rs"), None)?;
    c.finish()
}

// * The crate's items, by module

struct Module {
    path: Vec<String>,
    file: PathBuf,
    parent: Option<usize>,
    /// The type namespace's: types, aliases, child modules.
    types: BTreeMap<String, Ent>,
    fns: BTreeMap<String, usize>,
    uses: Vec<(String, Vec<String>, Span)>,
    globs: Vec<(Vec<String>, Span)>,
}

#[derive(Clone, Copy)]
enum Ent {
    Def(usize),
    Alias(usize),
    Module(usize),
}

/// A struct or an enum the derives read.
struct Def {
    input: DeriveInput,
    /// Its attributes with `cfg_attr` applied.
    attrs: Vec<Attribute>,
    derives: BTreeSet<String>,
    module: usize,
}

struct Alias {
    params: Vec<String>,
    ty: syn::Type,
    module: usize,
}

struct Func {
    /// Its signature and attributes; its body, as tokens, is `body`.
    item: syn::ItemFn,
    body: proc_macro2::TokenStream,
    module: usize,
    /// Its registering attribute: the kind and the name.
    kind: Option<(Kind, String)>,
}

struct Crate<'o> {
    o: &'o Options,
    modules: Vec<Module>,
    defs: Vec<Def>,
    aliases: Vec<Alias>,
    fns: Vec<Func>,
    scaffolding: bool,
    /// Each name looked up, by module and namespace: its meaning, or none
    /// while it is being looked up (a glob's cycle).
    looked: std::cell::RefCell<std::collections::HashMap<(usize, String, bool), Option<Res>>>,
    /// How often a lookup met one in progress: what was looked up meanwhile
    /// is not kept, since a cycle cut short is not an answer.
    cycles: std::cell::Cell<usize>,
}

/// A `cfg` predicate under the features on (and not under test).
fn holds(meta: &syn::Meta, features: &BTreeSet<String>, file: &Path) -> Result<bool, String> {
    match meta {
        syn::Meta::Path(p) if p.is_ident("test") => Ok(false),
        syn::Meta::NameValue(nv) if nv.path.is_ident("feature") => match &nv.value {
            Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Ok(features.contains(&s.value())),
            e => Err(at(file, syn::spanned::Spanned::span(e), "feature = \"name\"")),
        },
        syn::Meta::List(l) if ["all", "any", "not"].iter().any(|k| l.path.is_ident(k)) => {
            let inner = Punctuated::<syn::Meta, Token![,]>::parse_terminated
                .parse2(l.tokens.clone())
                .map_err(|e| at(file, e.span(), e))?;
            let vs = inner.iter().map(|m| holds(m, features, file)).collect::<Result<Vec<_>, _>>()?;
            Ok(match l.path.get_ident().unwrap().to_string().as_str() {
                "all" => vs.iter().all(|v| *v),
                "any" => vs.iter().any(|v| *v),
                _ => vs.len() == 1 && !vs[0],
            })
        }
        m => Err(at(
            file,
            syn::spanned::Spanned::span(m),
            "a cfg this reader does not read: feature, test, all, any, not",
        )),
    }
}

impl Crate<'_> {
    /// Whether an item's `#[cfg(..)]`s hold.
    fn on(&self, attrs: &[Attribute], file: &Path) -> Result<bool, String> {
        for a in attrs.iter().filter(|a| a.path().is_ident("cfg")) {
            let m: syn::Meta = a.parse_args().map_err(|e| at(file, e.span(), e))?;
            if !holds(&m, &self.o.features, file)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `attrs` with every `cfg_attr` that holds made its attributes.
    fn flat(&self, attrs: &[Attribute], file: &Path) -> Result<Vec<Attribute>, String> {
        let mut out = vec![];
        for a in attrs {
            if !a.path().is_ident("cfg_attr") {
                out.push(a.clone());
                continue;
            }
            let parts = a
                .parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
                .map_err(|e| at(file, e.span(), e))?;
            let mut it = parts.into_iter();
            let pred = it.next().ok_or_else(|| at(file, a.span(), "cfg_attr(predicate, attributes)"))?;
            if holds(&pred, &self.o.features, file)? {
                for m in it {
                    let made: Attribute = syn::parse_quote!(#[#m]);
                    out.extend(self.flat(&[made], file)?);
                }
            }
        }
        Ok(out)
    }

    fn load(&mut self, path: Vec<String>, file: PathBuf, parent: Option<usize>) -> Result<usize, String> {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let parsed = light.parse_str(&text).map_err(|e| at(&file, e.span(), e))?;
        // A module's own directory: `src/` for the root and a `mod.rs`,
        // `src/a/` for `src/a.rs`.
        let dir = match (parent, file.file_name().and_then(|n| n.to_str())) {
            (None, _) | (_, Some("mod.rs")) => file.parent().unwrap().to_path_buf(),
            _ => file.with_extension(""),
        };
        self.module(path, file, parent, parsed, dir)
    }

    fn module(&mut self, path: Vec<String>, file: PathBuf, parent: Option<usize>, items: Vec<Light>, dir: PathBuf) -> Result<usize, String> {
        let m = self.modules.len();
        self.modules.push(Module {
            path: path.clone(),
            file: file.clone(),
            parent,
            types: BTreeMap::new(),
            fns: BTreeMap::new(),
            uses: vec![],
            globs: vec![],
        });
        for item in items {
            self.item(m, &path, &file, &dir, item)?;
        }
        Ok(m)
    }

    fn named(&mut self, m: usize, name: String, e: Ent, file: &Path, span: Span) -> Result<(), String> {
        match self.modules[m].types.insert(name.clone(), e) {
            Some(_) => Err(at(file, span, format!("{name} is declared twice"))),
            None => Ok(()),
        }
    }

    fn item(&mut self, m: usize, path: &[String], file: &Path, dir: &Path, light: Light) -> Result<(), String> {
        let (item, body, content) = match light {
            Light::Item(i) => (i, None, None),
            Light::Fn(f, b) => (Item::Fn(f), Some(b), None),
            Light::Mod(md, c) => (Item::Mod(md), None, c),
        };
        let attrs: &[Attribute] = match &item {
            Item::Struct(i) => &i.attrs,
            Item::Enum(i) => &i.attrs,
            Item::Type(i) => &i.attrs,
            Item::Fn(i) => &i.attrs,
            Item::Mod(i) => &i.attrs,
            Item::Use(i) => &i.attrs,
            Item::Macro(i) => &i.attrs,
            _ => &[],
        };
        if !self.on(attrs, file)? {
            return Ok(());
        }
        match item {
            Item::Struct(_) | Item::Enum(_) => {
                let input: DeriveInput = match item {
                    Item::Struct(s) => s.into(),
                    Item::Enum(e) => e.into(),
                    _ => unreachable!(),
                };
                let attrs = self.flat(&input.attrs, file)?;
                let mut derives = BTreeSet::new();
                for a in attrs.iter().filter(|a| a.path().is_ident("derive")) {
                    let ps = a
                        .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                        .map_err(|e| at(file, e.span(), e))?;
                    derives.extend(ps.iter().map(|p| p.segments.last().unwrap().ident.to_string()));
                }
                let (name, span) = (input.ident.to_string(), input.ident.span());
                self.named(m, name, Ent::Def(self.defs.len()), file, span)?;
                self.defs.push(Def {
                    input,
                    attrs,
                    derives,
                    module: m,
                });
            }
            Item::Type(t) => {
                let params = t
                    .generics
                    .params
                    .iter()
                    .filter_map(|p| match p {
                        syn::GenericParam::Type(t) => Some(t.ident.to_string()),
                        _ => None,
                    })
                    .collect();
                self.named(m, t.ident.to_string(), Ent::Alias(self.aliases.len()), file, t.ident.span())?;
                self.aliases.push(Alias {
                    params,
                    ty: *t.ty,
                    module: m,
                });
            }
            Item::Fn(f) => {
                let mut kind = None;
                for a in &f.attrs {
                    let k = match a.path().segments.last().map(|s| s.ident.to_string()).as_deref() {
                        Some("mutator") => Kind::Mutator,
                        Some("live") => Kind::Live,
                        Some("query") => Kind::Query,
                        Some("view") => Kind::View,
                        Some("scope") => Kind::Scope,
                        _ => continue,
                    };
                    let name: syn::LitStr = a.parse_args().map_err(|e| at(file, e.span(), e))?;
                    kind = Some((k, name.value()));
                }
                self.modules[m].fns.insert(f.sig.ident.to_string(), self.fns.len());
                let body = body.unwrap_or_default();
                self.fns.push(Func {
                    item: f,
                    body,
                    module: m,
                    kind,
                });
            }
            Item::Mod(md) => {
                let mut sub = path.to_vec();
                sub.push(md.ident.to_string());
                let child = match content {
                    Some(items) => self.module(sub, file.to_path_buf(), Some(m), items, dir.join(md.ident.to_string()))?,
                    None => {
                        let (a, b) = (dir.join(format!("{}.rs", md.ident)), dir.join(md.ident.to_string()).join("mod.rs"));
                        let f = if a.exists() { a } else { b };
                        self.load(sub, f, Some(m))?
                    }
                };
                self.named(m, md.ident.to_string(), Ent::Module(child), file, md.ident.span())?;
            }
            Item::Use(u) => self.uses(m, vec![], &u.tree)?,
            Item::Macro(mac) => {
                let p = mac.mac.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("::");
                match p.as_str() {
                    "uniffi::setup_scaffolding" => self.scaffolding = true,
                    "ark_auth::install" | "macro_rules" => {}
                    _ => {
                        return Err(at(
                            file,
                            mac.mac.path.segments[0].ident.span(),
                            format!("{p}!: an item macro this reader does not know what it declares"),
                        ))
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn uses(&mut self, m: usize, mut prefix: Vec<String>, tree: &syn::UseTree) -> Result<(), String> {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                self.uses(m, prefix, &p.tree)
            }
            syn::UseTree::Name(n) => {
                let name = n.ident.to_string();
                if name == "self" {
                    let last = prefix.last().cloned().unwrap_or_default();
                    self.modules[m].uses.push((last, prefix, n.ident.span()));
                } else {
                    prefix.push(name.clone());
                    self.modules[m].uses.push((name, prefix, n.ident.span()));
                }
                Ok(())
            }
            syn::UseTree::Rename(r) => {
                prefix.push(r.ident.to_string());
                self.modules[m].uses.push((r.rename.to_string(), prefix, r.rename.span()));
                Ok(())
            }
            syn::UseTree::Glob(g) => {
                self.modules[m].globs.push((prefix, g.star_token.span));
                Ok(())
            }
            syn::UseTree::Group(g) => g.items.iter().try_for_each(|t| self.uses(m, prefix.clone(), t)),
        }
    }
}

// * Parsing, function bodies left as tokens

/// An item as read: a function's body is kept as its tokens (only the
/// domain's builder is parsed), an `impl`'s or a trait's skipped, and an
/// inline module's items read so too.
enum Light {
    Item(Item),
    Fn(syn::ItemFn, proc_macro2::TokenStream),
    Mod(syn::ItemMod, Option<Vec<Light>>),
}

fn light(input: ParseStream) -> syn::Result<Vec<Light>> {
    let mut out = vec![];
    input.call(Attribute::parse_inner)?;
    while !input.is_empty() {
        let ahead = input.fork();
        ahead.call(Attribute::parse_outer)?;
        ahead.parse::<syn::Visibility>()?;
        let skip = ahead.peek(Token![impl])
            || ahead.peek(Token![trait])
            || (ahead.peek(Token![unsafe]) && (ahead.peek2(Token![impl]) || ahead.peek2(Token![trait])));
        if skip {
            // Up to and with its body, the first brace group.
            input.step(|sc| {
                let mut c = *sc;
                while let Some((tt, next)) = c.token_tree() {
                    c = next;
                    if matches!(&tt, proc_macro2::TokenTree::Group(g) if g.delimiter() == proc_macro2::Delimiter::Brace) {
                        return Ok(((), c));
                    }
                }
                Err(sc.error("an impl without a body"))
            })?;
            continue;
        }
        if ahead.peek(Token![mod]) {
            let attrs = input.call(Attribute::parse_outer)?;
            let vis: syn::Visibility = input.parse()?;
            let mod_token: Token![mod] = input.parse()?;
            let ident: Ident = input.parse()?;
            let (content, semi) = match input.peek(Token![;]) {
                true => (None, Some(input.parse::<Token![;]>()?)),
                false => {
                    let inner;
                    syn::braced!(inner in input);
                    (Some(light(&inner)?), None)
                }
            };
            let md = syn::ItemMod {
                attrs,
                vis,
                unsafety: None,
                mod_token,
                ident,
                content: None,
                semi,
            };
            out.push(Light::Mod(md, content));
            continue;
        }
        let sig = ahead.fork();
        if sig.parse::<syn::Signature>().is_ok() && sig.peek(syn::token::Brace) {
            let attrs = input.call(Attribute::parse_outer)?;
            let vis: syn::Visibility = input.parse()?;
            let sig: syn::Signature = input.parse()?;
            let inner;
            let brace_token = syn::braced!(inner in input);
            let body: proc_macro2::TokenStream = inner.parse()?;
            let block = Box::new(syn::Block { brace_token, stmts: vec![] });
            out.push(Light::Fn(syn::ItemFn { attrs, vis, sig, block }, body));
            continue;
        }
        out.push(Light::Item(input.parse()?));
    }
    Ok(out)
}

// * Names

/// What a name means.
#[derive(Clone, Debug, PartialEq)]
enum Res {
    Def(usize),
    Alias(usize),
    Module(usize),
    Fn(usize),
    /// A path outside the crate: its crate and the rest.
    Extern(String, Vec<String>),
}

/// The crates a domain names that this reader knows of.
const CRATES: &[&str] = &["ark", "ark_auth", "std", "core", "alloc", "serde", "uniffi", "ark_ffi"];
/// What `ark::prelude::*` brings.
const ARK_PRELUDE: &[&str] = &[
    "Db",
    "Tables",
    "Volatile",
    "Domain",
    "Scope",
    "Effect",
    "Job",
    "Encode",
    "Ctx",
    "scan",
    "Desc",
    "Plan",
    "Describe",
    "Id",
    "Index",
    "Input",
    "Row",
    "Refusal",
    "live",
    "mutator",
    "query",
    "scope",
    "view",
    "Schema",
    "Deserialize",
    "Serialize",
];
const STD_PRELUDE: &[&str] = &["Option", "Vec", "String", "Box", "Result"];
const PRIMS: &[&str] = &[
    "i64", "u64", "i32", "u32", "u8", "bool", "str", "i8", "i16", "u16", "f32", "f64", "usize", "isize", "char",
];

#[derive(Clone, Copy, PartialEq)]
enum Ns {
    Type,
    Value,
}

impl Crate<'_> {
    fn lookup(&self, m: usize, name: &str, ns: Ns, depth: usize) -> Option<Res> {
        let key = (m, name.to_string(), ns == Ns::Type);
        if let Some(r) = self.looked.borrow().get(&key) {
            if r.is_none() {
                self.cycles.set(self.cycles.get() + 1);
            }
            return r.clone();
        }
        if depth > 32 {
            return None;
        }
        let before = self.cycles.get();
        self.looked.borrow_mut().insert(key.clone(), None);
        let r = self.lookup_now(m, name, ns, depth);
        match (self.cycles.get() == before, &r) {
            (true, _) | (false, Some(_)) => drop(self.looked.borrow_mut().insert(key, r.clone())),
            (false, None) => drop(self.looked.borrow_mut().remove(&key)),
        }
        r
    }

    fn lookup_now(&self, m: usize, name: &str, ns: Ns, depth: usize) -> Option<Res> {
        let md = &self.modules[m];
        let local = match ns {
            Ns::Type => md.types.get(name).map(|e| match *e {
                Ent::Def(i) => Res::Def(i),
                Ent::Alias(i) => Res::Alias(i),
                Ent::Module(i) => Res::Module(i),
            }),
            Ns::Value => md.fns.get(name).map(|i| Res::Fn(*i)),
        };
        if local.is_some() {
            return local;
        }
        for (alias, path, _) in &md.uses {
            if alias == name {
                if let Some(r) = self.path(m, path, ns, depth + 1) {
                    return Some(r);
                }
            }
        }
        for (path, _) in &md.globs {
            match self.path(m, path, Ns::Type, depth + 1) {
                Some(Res::Module(g)) if g != m => {
                    if let Some(r) = self.lookup(g, name, ns, depth + 1) {
                        return Some(r);
                    }
                }
                Some(Res::Extern(k, rest)) if k == "ark" && rest == ["prelude"] && ARK_PRELUDE.contains(&name) => {
                    return Some(Res::Extern(k, vec!["prelude".into(), name.into()]));
                }
                _ => {}
            }
        }
        if ns == Ns::Type {
            if CRATES.contains(&name) {
                return Some(Res::Extern(name.into(), vec![]));
            }
            if STD_PRELUDE.contains(&name) || PRIMS.contains(&name) {
                return Some(Res::Extern("std".into(), vec![name.into()]));
            }
        }
        None
    }

    /// A path's meaning, from module `m`.
    fn path(&self, m: usize, segs: &[String], ns: Ns, depth: usize) -> Option<Res> {
        let (first, rest) = segs.split_first()?;
        if rest.is_empty() {
            return match first.as_str() {
                "crate" => Some(Res::Module(0)),
                "self" => Some(Res::Module(m)),
                "super" => self.modules[m].parent.map(Res::Module),
                _ => self.lookup(m, first, ns, depth),
            };
        }
        let mut at = match first.as_str() {
            "crate" => Res::Module(0),
            "self" => Res::Module(m),
            "super" => Res::Module(self.modules[m].parent?),
            _ => self.lookup(m, first, Ns::Type, depth)?,
        };
        for (i, s) in rest.iter().enumerate() {
            let last = i + 1 == rest.len();
            at = match at {
                Res::Module(g) if s == "super" => Res::Module(self.modules[g].parent?),
                Res::Module(g) => self.lookup(g, s, if last { ns } else { Ns::Type }, depth + 1)?,
                Res::Extern(k, mut r) => {
                    r.push(s.clone());
                    Res::Extern(k, r)
                }
                _ => return None,
            };
        }
        Some(at)
    }
}

// * Types

/// A type as the derives see it.
#[derive(Clone, Debug, PartialEq)]
enum R {
    Prim(String),
    Unit,
    Text,
    Option(Box<R>),
    Vec(Box<R>),
    Box(Box<R>),
    Tuple(Vec<R>),
    Id(Box<R>),
    Job(Box<R>),
    /// `Plan<S, T>`: its rows' `T`.
    Plan(Box<R>),
    /// `Result<T, E>`: its `T`.
    Result(Box<R>),
    Def(usize),
    /// One of `ark-auth`'s, by name.
    Auth(String),
    /// What a signature names beside a function's input (`Ctx`, `Db`, …).
    Other(String),
}

/// `ark-auth`'s types, as it is built: what each describes as and, of a
/// row, its table.
fn auth(name: &str) -> Option<(Type, Option<Table>)> {
    use ark_auth::{Grant, Revoke, Role, SignIn};
    Some(match name {
        "SignIn" => (SignIn::describe(), Some(SignIn::table())),
        "Role" => (Role::describe(), Some(Role::table())),
        "Revoke" => (Revoke::describe(), None),
        "Grant" => (Grant::describe(), None),
        _ => return None,
    })
}

/// A schema holding `ark-auth`'s two tables, for its functions' descriptions.
#[allow(dead_code)]
#[derive(ark::prelude::Schema)]
struct AuthOnly {
    sign_in: ark_auth::SignIn,
    role: ark_auth::Role,
}
ark_auth::install!(AuthOnly);

/// `ark_auth::register`'s functions, as it registers them.
fn auth_functions() -> Vec<Described> {
    let d = ark_auth::register::<AuthOnly>(ark::domain::Domain::new::<AuthOnly>("auth", ""));
    Catalog::of(&d).functions
}

impl Crate<'_> {
    fn file(&self, m: usize) -> &Path {
        &self.modules[m].file
    }

    fn ty(&self, t: &syn::Type, m: usize, subst: &BTreeMap<String, R>, depth: usize) -> Result<R, String> {
        let file = self.file(m);
        let span = syn::spanned::Spanned::span(t);
        if depth > 32 {
            return Err(at(file, span, "a type alias without end"));
        }
        let p = match t {
            syn::Type::Tuple(tt) if tt.elems.is_empty() => return Ok(R::Unit),
            syn::Type::Tuple(tt) => {
                return tt
                    .elems
                    .iter()
                    .map(|e| self.ty(e, m, subst, depth + 1))
                    .collect::<Result<_, _>>()
                    .map(R::Tuple)
            }
            syn::Type::Paren(p) => return self.ty(&p.elem, m, subst, depth + 1),
            syn::Type::Group(g) => return self.ty(&g.elem, m, subst, depth + 1),
            syn::Type::Reference(_) => return Ok(R::Other("&".into())),
            syn::Type::Path(p) if p.qself.is_none() => &p.path,
            _ => return Err(at(file, span, format!("a type this reader does not read: {}", t.to_token_stream()))),
        };
        if let (Some(id), 1) = (p.get_ident(), p.segments.len()) {
            if let Some(r) = subst.get(&id.to_string()) {
                return Ok(r.clone());
            }
        }
        let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
        let args: Vec<&syn::Type> = match &p.segments.last().unwrap().arguments {
            syn::PathArguments::AngleBracketed(a) => a
                .args
                .iter()
                .filter_map(|g| match g {
                    syn::GenericArgument::Type(t) => Some(t),
                    _ => None,
                })
                .collect(),
            syn::PathArguments::None => vec![],
            syn::PathArguments::Parenthesized(_) => return Err(at(file, span, "a function type")),
        };
        let arg = |i: usize| -> Result<R, String> {
            let a = args.get(i).ok_or_else(|| at(file, span, format!("{} takes a type", segs.join("::"))))?;
            self.ty(a, m, subst, depth + 1)
        };
        let res = self
            .path(m, &segs, Ns::Type, 0)
            .ok_or_else(|| at(file, span, format!("{}: not a type this reader can find", segs.join("::"))))?;
        Ok(match res {
            Res::Def(i) => {
                if !self.defs[i].input.generics.params.is_empty() {
                    return Err(at(file, span, format!("{}: a generic type is not read", segs.join("::"))));
                }
                R::Def(i)
            }
            Res::Alias(i) => {
                let a = &self.aliases[i];
                if a.params.len() != args.len() {
                    return Err(at(file, span, format!("{} takes {} types", segs.join("::"), a.params.len())));
                }
                let mut s = BTreeMap::new();
                for (n, t) in a.params.iter().zip(&args) {
                    s.insert(n.clone(), self.ty(t, m, subst, depth + 1)?);
                }
                self.ty(&a.ty, a.module, &s, depth + 1)?
            }
            Res::Extern(k, rest) => {
                let last = rest.last().map(String::as_str).unwrap_or("");
                match (k.as_str(), last) {
                    ("std" | "core" | "alloc", "Option") => R::Option(Box::new(arg(0)?)),
                    ("std" | "core" | "alloc", "Vec") => R::Vec(Box::new(arg(0)?)),
                    ("std" | "core" | "alloc", "Box") => R::Box(Box::new(arg(0)?)),
                    ("std" | "core" | "alloc", "String") => R::Text,
                    ("std" | "core" | "alloc", "Result") => R::Result(Box::new(arg(0)?)),
                    ("std", p) if PRIMS.contains(&p) => R::Prim(p.into()),
                    ("ark", "Id") => R::Id(Box::new(arg(0)?)),
                    ("ark", "Job") => R::Job(Box::new(arg(0)?)),
                    ("ark", "Plan") => R::Plan(Box::new(arg(1)?)),
                    ("ark", "Ctx" | "Db" | "Refusal" | "Volatile" | "Scope" | "Domain") => R::Other(last.into()),
                    ("ark_auth", n) if auth(n).is_some() => R::Auth(n.into()),
                    _ => {
                        return Err(at(
                            file,
                            span,
                            format!("{}: a type outside the crate this reader does not know", segs.join("::")),
                        ))
                    }
                }
            }
            _ => return Err(at(file, span, format!("{}: not a type", segs.join("::")))),
        })
    }

    fn def_file(&self, i: usize) -> &Path {
        self.file(self.defs[i].module)
    }

    /// A row's table: its `#[ark(table = ..)]`, or its name in snake case.
    fn table_name(&self, r: &R, file: &Path, span: Span) -> Result<String, String> {
        match r {
            R::Def(i) if self.defs[*i].derives.contains("Row") => self.table_of(*i),
            R::Auth(n) => auth(n)
                .and_then(|(_, t)| t)
                .map(|t| t.name)
                .ok_or_else(|| at(file, span, format!("{n} is not a row"))),
            _ => Err(at(file, span, "an id is of a row")),
        }
    }

    /// A row's `TABLE`: its `#[ark(table = ..)]`, or its name in snake case.
    fn table_of(&self, i: usize) -> Result<String, String> {
        let def = &self.defs[i];
        let mut table = snake(&def.input.ident.to_string());
        for it in items(&def.attrs, self.def_file(i))?.into_iter().flatten() {
            if let Item_::Is(n, Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. })) = it {
                if n == "table" {
                    table = s.value();
                }
            }
        }
        Ok(table)
    }

    /// `<r as Describe>::describe()`.
    fn describe(&self, r: &R, file: &Path, span: Span, depth: usize) -> Result<Type, String> {
        if depth > 64 {
            return Err(at(file, span, "a type that describes itself without end"));
        }
        let d = |r: &R| self.describe(r, file, span, depth + 1);
        Ok(match r {
            R::Prim(p) if p == "i64" => Type::Int,
            R::Prim(p) if p == "bool" => Type::Bool,
            R::Unit => Type::Unit,
            R::Text => Type::Text,
            R::Option(x) => Type::Option(Box::new(d(x)?)),
            R::Vec(x) => Type::List(Box::new(d(x)?)),
            R::Box(x) => d(x)?,
            R::Tuple(xs) if xs.len() <= 4 => Type::Tuple(xs.iter().map(d).collect::<Result<_, _>>()?),
            R::Id(x) => Type::Id(self.table_name(x, file, span)?),
            R::Job(t) => {
                let t = d(t)?;
                let f = |fs: &[(&str, Type)]| fs.iter().map(|(n, t)| (n.to_string(), t.clone())).collect::<Vec<_>>();
                let name = format!("Job{}", t.decl().replace(|c: char| !c.is_alphanumeric(), ""));
                let vs = [
                    ("Pending", f(&[("due_ms", Type::Int)])),
                    ("Claimed", f(&[("by", Type::Text), ("at_ms", Type::Int)])),
                    ("Done", f(&[("value", t)])),
                    ("Failed", f(&[("why", Type::Text), ("retry_ms", Type::Int)])),
                    ("Cancelled", vec![]),
                ];
                Type::Sum(name, vs.into_iter().map(|(n, fs)| (n.to_string(), fs)).collect())
            }
            R::Def(i) => self.described(*i, depth + 1)?,
            R::Auth(n) => auth(n).unwrap().0,
            other => return Err(at(file, span, format!("{other:?} is not Describe"))),
        })
    }

    fn field_r(&self, f: &syn::Field, m: usize) -> Result<R, String> {
        self.ty(&f.ty, m, &BTreeMap::new(), 0)
    }

    /// A struct's or an enum's description (`describe` in `domain.rs`).
    fn described(&self, i: usize, depth: usize) -> Result<Type, String> {
        let def = &self.defs[i];
        let (file, span) = (self.def_file(i), def.input.ident.span());
        if !["Row", "Input", "Describe"].iter().any(|k| def.derives.contains(*k)) {
            return Err(at(file, span, format!("{} derives none of Row, Input, Describe", def.input.ident)));
        }
        let record = |fields: &Fields| -> Result<Vec<(String, Type)>, String> {
            fields
                .iter()
                .enumerate()
                .map(|(n, f)| {
                    let name = f.ident.as_ref().map_or(n.to_string(), |n| n.to_string());
                    let r = self.field_r(f, def.module)?;
                    Ok((name, self.describe(&r, file, syn::spanned::Spanned::span(&f.ty), depth + 1)?))
                })
                .collect()
        };
        let label = def.input.ident.to_string();
        Ok(match &def.input.data {
            Data::Struct(s) => Type::Record(label, record(&s.fields)?),
            Data::Enum(e) => Type::Sum(
                label,
                e.variants
                    .iter()
                    .map(|v| Ok((v.ident.to_string(), record(&v.fields)?)))
                    .collect::<Result<_, String>>()?,
            ),
            Data::Union(_) => return Err(at(file, span, "a union describes nothing")),
        })
    }

    fn named_fields(&self, i: usize) -> Result<&syn::FieldsNamed, String> {
        let def = &self.defs[i];
        match &def.input.data {
            Data::Struct(syn::DataStruct {
                fields: Fields::Named(n), ..
            }) => Ok(n),
            _ => Err(at(self.def_file(i), def.input.ident.span(), "a struct with named fields")),
        }
    }

    /// `<Row>::table()` (`row` in `domain.rs`).
    fn row(&self, i: usize) -> Result<Table, String> {
        let def = &self.defs[i];
        let (file, span) = (self.def_file(i), def.input.ident.span());
        let fields = self.named_fields(i)?;
        let (mut table, mut volatile, mut indexes) = (self.table_of(i)?, false, vec![]);
        for it in items(&def.attrs, file)?.into_iter().flatten() {
            match it {
                Item_::Flag(f) if f == "volatile" => volatile = true,
                Item_::Flag(f) if f == "ffi" || f == "ffi_view" => {}
                Item_::Is(n, Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. })) if n == "table" => table = s.value(),
                Item_::Cols(k, cols) if k == "index" || k == "unique" => indexes.push((k == "unique", cols)),
                _ => {
                    return Err(at(
                        file,
                        span,
                        "#[ark(..)] on a row: table = \"..\", volatile, index(..), unique(..), ffi, ffi_view",
                    ))
                }
            }
        }
        let names: Vec<String> = fields.named.iter().map(|f| f.ident.as_ref().unwrap().to_string()).collect();
        let (mut keys, mut refs, mut columns) = (vec![], vec![], vec![]);
        for (n, f) in fields.named.iter().enumerate() {
            for it in items(&f.attrs, file)?.into_iter().flatten() {
                match it {
                    Item_::Flag(k) if k == "key" => keys.push(n),
                    Item_::Refs(t) => {
                        let r = self.ty(&t, def.module, &BTreeMap::new(), 0)?;
                        refs.push(Ref {
                            column: n,
                            table: self.table_name(&r, file, syn::spanned::Spanned::span(&t))?,
                        });
                    }
                    _ => return Err(at(file, f.ident.as_ref().unwrap().span(), "#[ark(..)] on a column: key, refs(Row)")),
                }
            }
            let r = self.field_r(f, def.module)?;
            let fspan = syn::spanned::Spanned::span(&f.ty);
            let ty = self.describe(&r, file, fspan, 0)?;
            let default = match serde_default(&f.attrs, file)? {
                false => None,
                true => Some(default_json(&r).ok_or_else(|| {
                    at(
                        file,
                        fspan,
                        "#[serde(default)] on a column of a type whose default this reader does not know",
                    )
                })?),
            };
            columns.push(Column {
                name: names[n].clone(),
                ty,
                default,
            });
        }
        serde_default(&def.attrs, file)?;
        if keys.is_empty() || keys.iter().enumerate().any(|(n, k)| n != *k) {
            return Err(at(file, span, "a row's key is its leading fields, each #[ark(key)], at least one"));
        }
        let indexes = indexes
            .into_iter()
            .map(|(unique, cols)| {
                let columns = cols
                    .iter()
                    .map(|c| {
                        names
                            .iter()
                            .position(|x| c == x)
                            .ok_or_else(|| at(file, c.span(), "an index over a column of this row"))
                    })
                    .collect::<Result<_, _>>()?;
                Ok(Index { columns, unique })
            })
            .collect::<Result<_, String>>()?;
        Ok(Table {
            name: table,
            columns,
            key: keys.len(),
            indexes,
            refs,
            space: if volatile { Space::Volatile } else { Space::Durable },
        })
    }

    /// `<I as Input>::checks()` (`said` in `domain.rs`): a row's and `()`'s are none.
    fn checks(&self, r: &R, file: &Path, span: Span) -> Result<Vec<(String, Vec<String>)>, String> {
        let i = match r {
            R::Unit => return Ok(vec![]),
            R::Def(i) if self.defs[*i].derives.contains("Input") => *i,
            R::Def(i) if self.defs[*i].derives.contains("Row") => return Ok(vec![]),
            _ => return Err(at(file, span, "a function's input is a Row, an Input or ()")),
        };
        let mut out = vec![];
        for f in &self.named_fields(i)?.named {
            let s = said(f, self.def_file(i))?;
            if !s.is_empty() {
                out.push((f.ident.as_ref().unwrap().to_string(), s));
            }
        }
        Ok(out)
    }
}

/// Whether `#[serde(..)]` says `default`; any other serde attribute changes
/// the JSON, which this reader would not follow, and is refused.
fn serde_default(attrs: &[Attribute], file: &Path) -> Result<bool, String> {
    let mut default = false;
    for a in attrs.iter().filter(|a| a.path().is_ident("serde")) {
        let text = a.to_token_stream().to_string().replace(' ', "");
        match text == "#[serde(default)]" {
            true => default = true,
            false => return Err(at(file, a.span(), format!("{text}: a serde attribute this reader does not follow"))),
        }
    }
    Ok(default)
}

/// `serde_json::to_value(T::default())`, for the types it is plain for.
fn default_json(r: &R) -> Option<serde_json::Value> {
    use serde_json::json;
    Some(match r {
        R::Prim(p) if p == "i64" => json!(0),
        R::Prim(p) if p == "bool" => json!(false),
        R::Text => json!(""),
        R::Unit | R::Option(_) => serde_json::Value::Null,
        R::Vec(_) => json!([]),
        _ => return None,
    })
}

trait Spanned {
    fn span(&self) -> Span;
}

impl Spanned for Attribute {
    fn span(&self) -> Span {
        self.pound_token.span
    }
}

/// `snake_case` of a type's name (`domain.rs`).
fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

// * `#[ark(..)]`, as `domain.rs` parses it

enum Item_ {
    Flag(Ident),
    Is(Ident, Expr),
    Cols(Ident, Vec<Ident>),
    Refs(syn::Type),
}

impl Parse for Item_ {
    fn parse(input: ParseStream) -> syn::Result<Item_> {
        let name: Ident = input.parse()?;
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            return Ok(Item_::Is(name, input.parse()?));
        }
        if input.peek(syn::token::Paren) {
            let inner;
            syn::parenthesized!(inner in input);
            if name == "refs" {
                return Ok(Item_::Refs(inner.parse()?));
            }
            let cols = Punctuated::<Ident, Token![,]>::parse_terminated(&inner)?.into_iter().collect();
            if input.peek(Token![as]) {
                input.parse::<Token![as]>()?;
                input.parse::<Ident>()?;
            }
            return Ok(Item_::Cols(name, cols));
        }
        Ok(Item_::Flag(name))
    }
}

fn items(attrs: &[Attribute], file: &Path) -> Result<Vec<Vec<Item_>>, String> {
    let mut out = vec![];
    for a in attrs.iter().filter(|a| a.path().is_ident("ark")) {
        let list = a.meta.require_list().map_err(|e| at(file, e.span(), e))?;
        let parsed = Punctuated::<Item_, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|e| at(file, e.span(), e))?;
        out.push(parsed.into_iter().collect());
    }
    Ok(out)
}

/// `"1..=80"`, `"0..120"`, `"1.."` as inclusive bounds (`domain.rs`).
fn bounds(s: &str) -> Option<(usize, usize)> {
    let (lo, hi) = s.split_once("..")?;
    let lo: usize = lo.trim().parse().ok()?;
    let hi = hi.trim();
    let hi = if hi.is_empty() {
        usize::MAX
    } else if let Some(h) = hi.strip_prefix('=') {
        h.trim().parse().ok()?
    } else {
        hi.parse::<usize>().ok()?.checked_sub(1)?
    };
    Some((lo, hi))
}

/// What a field's refinements are, as a form says them (`said` in `domain.rs`).
fn said(f: &syn::Field, file: &Path) -> Result<Vec<String>, String> {
    let mut out = vec![];
    for group in items(&f.attrs, file)? {
        let why = group.iter().find_map(|i| match i {
            Item_::Is(n, Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. })) if n == "why" => Some(s.value()),
            _ => None,
        });
        let quoted = why.as_ref().map_or(String::new(), |w| format!(" (\u{201c}{w}\u{201d})"));
        for it in &group {
            let lit = |e: &Expr| match e {
                Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => s.value(),
                _ => String::new(),
            };
            out.push(match it {
                Item_::Flag(k) if k == "trim" => "trimmed".into(),
                Item_::Flag(k) if k == "non_empty" => format!("non-empty{quoted}"),
                Item_::Is(k, e) if k == "len" => {
                    let chars = |n: usize| if n == 1 { "char" } else { "chars" };
                    match bounds(&lit(e)) {
                        Some((lo, usize::MAX)) => format!("min {lo} {}{quoted}", chars(lo)),
                        Some((0, hi)) => format!("max {hi} {}{quoted}", chars(hi)),
                        Some((lo, hi)) => format!("{lo} to {hi} {}{quoted}", chars(hi)),
                        None => return Err(at(file, k.span(), "len = \"lo..=hi\", \"lo..hi\" or \"lo..\"")),
                    }
                }
                Item_::Is(k, e) if k == "range" => why.clone().unwrap_or_else(|| format!("in {}", lit(e))),
                Item_::Is(k, _) if k == "check" => why.clone().unwrap_or_else(|| "refined".into()),
                Item_::Is(k, _) if k == "why" => continue,
                _ => {
                    return Err(at(
                        file,
                        f.ident.as_ref().map_or(Span::call_site(), |i| i.span()),
                        "#[ark(..)] on an input's field: trim, non_empty, len, range, check, why",
                    ))
                }
            });
        }
    }
    Ok(out)
}

// * The domain: its schema and its functions, as it registers them

impl Crate<'_> {
    /// The one function building the domain: `register!(Domain::new::<S>(..);
    /// paths)` or `Domain::new::<S>(..)`, then any `.with(..)`s.
    fn domain_fn(&self) -> Result<(usize, Expr), String> {
        let mut found = vec![];
        for (i, f) in self.fns.iter().enumerate() {
            let text = f.body.to_string();
            if !text.contains("Domain") {
                continue;
            }
            let stmts = syn::Block::parse_within
                .parse2(f.body.clone())
                .map_err(|e| at(self.file(f.module), e.span(), e))?;
            if let Some(syn::Stmt::Expr(e, None)) = stmts.last() {
                let mut base = e;
                while let Expr::MethodCall(mc) = base {
                    base = &mc.receiver;
                }
                let is = match base {
                    Expr::Macro(m) => m.mac.path.segments.last().is_some_and(|s| s.ident == "register"),
                    Expr::Call(c) => {
                        matches!(&*c.func, Expr::Path(p) if p.path.segments.len() >= 2 && p.path.segments.iter().rev().nth(1).is_some_and(|s| s.ident == "Domain") && p.path.segments.last().unwrap().ident == "new")
                    }
                    _ => false,
                };
                if is {
                    found.push((i, e.clone()));
                }
            }
        }
        match found.len() {
            1 => Ok(found.pop().unwrap()),
            0 => Err(format!(
                "{}: no function builds the domain (`register!(Domain::new::<S>(..); ..)`)",
                self.o.dir.display()
            )),
            _ => Err(format!("{}: more than one function builds a domain", self.o.dir.display())),
        }
    }

    fn finish(self) -> Result<Read, String> {
        let (di, body) = self.domain_fn()?;
        let dm = self.fns[di].module;
        let file = self.file(dm).to_path_buf();
        let mut withs: Vec<Expr> = vec![];
        let mut base = &body;
        while let Expr::MethodCall(mc) = base {
            if mc.method != "with" || mc.args.len() != 1 {
                return Err(at(
                    &file,
                    mc.method.span(),
                    format!(".{}(..): the domain is built with register! and .with(..) alone", mc.method),
                ));
            }
            withs.push(mc.args[0].clone());
            base = &mc.receiver;
        }
        withs.reverse();
        let mut regs: Vec<(syn::Path, bool)> = vec![];
        let new = match base {
            Expr::Macro(m) => {
                let parser = |input: ParseStream| -> syn::Result<(Expr, Vec<syn::Path>)> {
                    let e: Expr = input.parse()?;
                    input.parse::<Token![;]>()?;
                    let ps = Punctuated::<syn::Path, Token![,]>::parse_terminated(input)?;
                    Ok((e, ps.into_iter().collect()))
                };
                let (e, ps) = parser.parse2(m.mac.tokens.clone()).map_err(|e| at(&file, e.span(), e))?;
                regs.extend(ps.into_iter().map(|p| (p, true)));
                e
            }
            e => e.clone(),
        };
        // `Domain::new::<S>(..)`: S, the schema.
        let schema_ty = match &new {
            Expr::Call(c) => match &*c.func {
                Expr::Path(p) => match &p.path.segments.last().unwrap().arguments {
                    syn::PathArguments::AngleBracketed(a) => match a.args.first() {
                        Some(syn::GenericArgument::Type(t)) => t.clone(),
                        _ => return Err(at(&file, Span::call_site(), "Domain::new::<S>")),
                    },
                    _ => return Err(at(&file, Span::call_site(), "Domain::new::<S>")),
                },
                _ => unreachable!(),
            },
            e => return Err(at(&file, syn::spanned::Spanned::span(e), "the domain is Domain::new::<S>(..)")),
        };
        let schema = match self.ty(&schema_ty, dm, &BTreeMap::new(), 0)? {
            R::Def(i) if self.defs[i].derives.contains("Schema") => self.schema(i)?,
            _ => return Err(at(&file, syn::spanned::Spanned::span(&schema_ty), "Domain::new's S derives Schema")),
        };
        let mut functions = vec![];
        for (p, _) in &regs {
            functions.push(self.registered(dm, p, &file)?);
        }
        for w in &withs {
            functions.extend(self.with(dm, w, &file)?);
        }
        let mut seen = BTreeSet::new();
        for f in &functions {
            if !seen.insert(f.name.clone()) {
                return Err(format!("{}: {} is registered twice", file.display(), f.name));
            }
        }
        functions.sort_by(|a, b| a.name.cmp(&b.name));
        let twins = self.twins()?;
        Ok(Read {
            catalog: Catalog { schema, functions },
            twins,
            scaffolding: self.scaffolding,
        })
    }

    /// `#[derive(Schema)]`'s schema: each field's row's table, in order.
    fn schema(&self, i: usize) -> Result<Schema, String> {
        let file = self.def_file(i);
        let mut tables = vec![];
        for f in &self.named_fields(i)?.named {
            let span = syn::spanned::Spanned::span(&f.ty);
            tables.push(match self.field_r(f, self.defs[i].module)? {
                R::Def(r) if self.defs[r].derives.contains("Row") => self.row(r)?,
                R::Auth(n) => auth(&n).and_then(|(_, t)| t).ok_or_else(|| at(file, span, format!("{n} is not a row")))?,
                _ => return Err(at(file, span, "a schema's field is a row")),
            });
        }
        Ok(Schema { tables })
    }

    /// A function registered by its path (`library::add_song`): as its
    /// attribute registers it.
    fn registered(&self, m: usize, p: &syn::Path, file: &Path) -> Result<Described, String> {
        let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
        let span = p.segments[0].ident.span();
        let i = match self.path(m, &segs, Ns::Value, 0) {
            Some(Res::Fn(i)) => i,
            _ => return Err(at(file, span, format!("{}: not a function of the crate", segs.join("::")))),
        };
        let f = &self.fns[i];
        let ffile = self.file(f.module);
        let fspan = f.item.sig.ident.span();
        let Some((kind, name)) = f.kind.clone() else {
            return Err(at(
                ffile,
                fspan,
                "a registered function is #[mutator], #[live], #[query], #[view] or #[scope]",
            ));
        };
        if kind == Kind::Scope {
            return Ok(Described {
                name,
                kind,
                input: Type::Unit,
                output: None,
                checks: vec![],
            });
        }
        let args: Vec<&syn::Type> = f
            .item
            .sig
            .inputs
            .iter()
            .filter_map(|a| match a {
                syn::FnArg::Typed(t) => Some(&*t.ty),
                _ => None,
            })
            .collect();
        let [_, _, input] = args[..] else {
            return Err(at(ffile, fspan, "a registered function takes (&Ctx, the Db, its input)"));
        };
        let ispan = syn::spanned::Spanned::span(input);
        let ir = self.ty(input, f.module, &BTreeMap::new(), 0)?;
        let checks = self.checks(&ir, ffile, ispan)?;
        let input_t = self.describe(&ir, ffile, ispan, 0)?;
        let syn::ReturnType::Type(_, ret) = &f.item.sig.output else {
            return Err(at(ffile, fspan, "a registered function answers Result<_, Refusal>"));
        };
        let rspan = syn::spanned::Spanned::span(&**ret);
        let R::Result(ok) = self.ty(ret, f.module, &BTreeMap::new(), 0)? else {
            return Err(at(ffile, rspan, "a registered function answers Result<_, Refusal>"));
        };
        let output = match (kind, *ok) {
            (Kind::Mutator | Kind::Live, R::Unit) => None,
            (Kind::Mutator | Kind::Live, _) => return Err(at(ffile, rspan, "a mutator answers Result<(), Refusal>")),
            (Kind::View, R::Plan(t)) => Some(self.describe(&t, ffile, rspan, 0)?),
            (Kind::View, _) => return Err(at(ffile, rspan, "a view answers Result<Plan<S, T>, Refusal>")),
            (_, t) => Some(self.describe(&t, ffile, rspan, 0)?),
        };
        Ok(Described {
            name,
            kind,
            input: input_t,
            output,
            checks,
        })
    }

    /// `.with(..)`: a function's `register`, `ark_auth::register::<S>`, or a
    /// job's `<R as Effect>::register::<S>`.
    fn with(&self, m: usize, e: &Expr, file: &Path) -> Result<Vec<Described>, String> {
        let span = syn::spanned::Spanned::span(e);
        let Expr::Path(p) = e else {
            return Err(at(file, span, ".with(..) of a path"));
        };
        let segs: Vec<String> = p.path.segments.iter().map(|s| s.ident.to_string()).collect();
        if let Some(q) = &p.qself {
            let r = self.ty(&q.ty, m, &BTreeMap::new(), 0)?;
            let tr = &segs[..q.position];
            let is_effect =
                matches!(self.path(m, tr, Ns::Type, 0), Some(Res::Extern(k, rest)) if k == "ark" && rest.last().is_some_and(|l| l == "Effect"));
            return match (r, is_effect, segs.last().map(String::as_str)) {
                (R::Def(i), true, Some("register")) if self.defs[i].derives.contains("Effect") => self.effect(i),
                _ => Err(at(file, span, "<R as Effect>::register::<S>, R deriving Effect")),
            };
        }
        match self.path(m, &segs[..segs.len() - 1], Ns::Type, 0) {
            Some(Res::Extern(k, rest)) if k == "ark_auth" && rest.is_empty() && segs.last().unwrap() == "register" => Ok(auth_functions()),
            Some(Res::Module(_)) | None if segs.last().unwrap() == "register" && segs.len() >= 2 => {
                let fp = syn::Path {
                    leading_colon: None,
                    segments: p.path.segments.iter().take(segs.len() - 1).cloned().collect(),
                };
                Ok(vec![self.registered(m, &fp, file)?])
            }
            _ => Err(at(
                file,
                span,
                ".with(..) of a function's register, ark_auth::register or <R as Effect>::register",
            )),
        }
    }

    /// `Effect::register`'s four functions for the job row `i` (`effect.rs`).
    fn effect(&self, i: usize) -> Result<Vec<Described>, String> {
        let def = &self.defs[i];
        let (file, span) = (self.def_file(i), def.input.ident.span());
        let table = self.row(i)?;
        let fields = &self.named_fields(i)?.named;
        let state = fields.iter().find(|f| f.ident.as_ref().is_some_and(|n| n == "state"));
        let outcome = match state.map(|f| self.field_r(f, def.module)).transpose()? {
            Some(R::Job(t)) => self.describe(&t, file, span, 0)?,
            _ => return Err(at(file, span, "a job is a row with a `state: Job<T>` field")),
        };
        if table.key != 1 {
            return Err(at(file, span, "a job is keyed by one column"));
        }
        let id = table.columns[0].ty.clone();
        let row = self.described(i, 0)?;
        let camel: String = table
            .name
            .split('_')
            .flat_map(|w| w[..1].to_uppercase().chars().chain(w[1..].chars()).collect::<Vec<_>>())
            .collect();
        let named = |what: &str, fs: Vec<(&str, Type)>| Type::Record(format!("{camel}{what}"), fs.into_iter().map(|(n, t)| (n.into(), t)).collect());
        let outcome = Type::Sum(
            format!("{camel}Outcome"),
            vec![
                ("Ok".into(), vec![("value".into(), outcome)]),
                ("Err".into(), vec![("why".into(), Type::Text)]),
            ],
        );
        let f = |what: &str, kind, input, output| Described {
            name: format!("{}.{what}", table.name),
            kind,
            input,
            output,
            checks: vec![],
        };
        Ok(vec![
            f("claim", Kind::Mutator, named("Claim", vec![("id", id.clone()), ("by", Type::Text)]), None),
            f(
                "due",
                Kind::Query,
                Type::Unit,
                Some(named(
                    "Due",
                    vec![("rows", Type::List(Box::new(row))), ("next_ms", Type::Option(Box::new(Type::Int)))],
                )),
            ),
            f(
                "finish",
                Kind::Mutator,
                named("Finish", vec![("id", id.clone()), ("by", Type::Text), ("outcome", outcome)]),
                None,
            ),
            f("cancel", Kind::Mutator, named("Cancel", vec![("id", id)]), None),
        ])
    }

    // * The twins (`ffi.rs`)

    fn twins(&self) -> Result<Vec<Twin>, String> {
        let mut out = vec![];
        for (i, def) in self.defs.iter().enumerate() {
            let file = self.def_file(i);
            if !["Row", "Input", "Describe"].iter().any(|k| def.derives.contains(*k)) {
                continue;
            }
            let flags: Vec<String> = items(&def.attrs, file)?
                .into_iter()
                .flatten()
                .filter_map(|i| match i {
                    Item_::Flag(f) => Some(f.to_string()),
                    _ => None,
                })
                .collect();
            let view = flags.iter().any(|f| f == "ffi_view");
            if !view && !flags.iter().any(|f| f == "ffi") {
                continue;
            }
            let span = def.input.ident.span();
            if !def.input.generics.params.is_empty() {
                return Err(at(file, span, "a twin is of a type with no parameters"));
            }
            if !self.scaffolding {
                return Err(at(file, span, "a twin needs the crate's uniffi::setup_scaffolding!()"));
            }
            let fields = |fs: &Fields| -> Result<Vec<(String, Crosses)>, String> {
                let named = match fs {
                    Fields::Named(n) => n.named.iter().map(|f| (f.ident.as_ref().unwrap().to_string(), f)).collect(),
                    Fields::Unit => vec![],
                    Fields::Unnamed(u) if u.unnamed.len() == 1 => vec![("value".to_string(), &u.unnamed[0])],
                    Fields::Unnamed(_) => return Err(at(file, span, "a twin names its fields: one unnamed field at most")),
                };
                named
                    .into_iter()
                    .map(|(n, f)| Ok((n, self.crosses(&self.field_r(f, def.module)?, file, syn::spanned::Spanned::span(&f.ty))?)))
                    .collect()
            };
            let shape = match &def.input.data {
                Data::Struct(s) => Shape::Record(fields(&s.fields)?),
                Data::Enum(e) => Shape::Enum(
                    e.variants
                        .iter()
                        .map(|v| Ok((v.ident.to_string(), fields(&v.fields)?)))
                        .collect::<Result<_, String>>()?,
                ),
                Data::Union(_) => return Err(at(file, span, "a union has no twin")),
            };
            out.push(Twin {
                name: def.input.ident.to_string(),
                module: self.module_path(def.module),
                view,
                shape,
            });
        }
        Ok(out)
    }

    fn module_path(&self, m: usize) -> String {
        std::iter::once(self.o.crate_name.clone())
            .chain(self.modules[m].path.iter().cloned())
            .collect::<Vec<_>>()
            .join("::")
    }

    /// `<t as ark_ffi::Twin>::Ffi` (`typed.rs`).
    fn crosses(&self, r: &R, file: &Path, span: Span) -> Result<Crosses, String> {
        Ok(match r {
            R::Prim(p) => match p.as_str() {
                "i64" => Crosses::I64,
                "u64" => Crosses::U64,
                "i32" => Crosses::I32,
                "u32" => Crosses::U32,
                "u8" => Crosses::U8,
                "bool" => Crosses::Bool,
                _ => return Err(at(file, span, format!("{p} has no twin"))),
            },
            R::Text => Crosses::String,
            R::Id(_) => Crosses::Id,
            R::Option(x) => Crosses::Option(Box::new(self.crosses(x, file, span)?)),
            R::Vec(x) => Crosses::Vec(Box::new(self.crosses(x, file, span)?)),
            R::Def(i) => {
                let d = &self.defs[*i];
                let (wanted, _) = {
                    let flags: Vec<String> = items(&d.attrs, self.def_file(*i))?
                        .into_iter()
                        .flatten()
                        .filter_map(|i| match i {
                            Item_::Flag(f) => Some(f.to_string()),
                            _ => None,
                        })
                        .collect();
                    (flags.iter().any(|f| f == "ffi" || f == "ffi_view"), ())
                };
                let name = d.input.ident.to_string();
                if !wanted {
                    return Err(at(file, span, format!("{name} has no twin: #[ark(ffi)] on it")));
                }
                match d.input.data {
                    Data::Enum(_) => Crosses::Enum(self.module_path(d.module), name),
                    _ => Crosses::Record(self.module_path(d.module), name),
                }
            }
            other => return Err(at(file, span, format!("{other:?} has no twin"))),
        })
    }
}
