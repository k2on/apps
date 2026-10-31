//! A domain's types and stubs in Kotlin and Swift (§1.16, N3): what an app
//! over the bridge (`ark-ffi`) calls its domain with, **types only** — a
//! data class or struct per record and row, a sealed class or enum per sum,
//! each read from and written as the JSON the kernel's serde reads and
//! writes (struct fields in declaration order, sums externally tagged), and
//! a stub per function calling it by name: a mutator's `mutate`, a live
//! transition's `call`, a query's answer read, a view's handle typed.
//! Nothing of a function's body is ever generated. What the generated code
//! stands on — `Id`, `View` and JSON — is the bridge's own typed half,
//! written once (`rust/ark-ffi/kotlin/Typed.kt`, `rust/ark-ffi/swift/Typed.swift`).
//!
//! A type UniFFI declares — a view's row with its twin (`#[ark(ffi_view)]`,
//! `ark_ffi::typed`) — is not declared again: only its JSON is written, as
//! extensions, and a view of its rows is read typed, through its view
//! object (`<T>View`), with no JSON per row ([`Walked::typed`]).
//!
//! A domain is walked as `ark::domain::Catalog` describes it: its
//! schema — every table a row type, named as the table — and every
//! function's name, kind, input and answer.

pub mod kotlin;
pub mod swift;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use ark::domain::{Catalog, Described, Kind};
use ark::row::Type;
use ark::schema::Space;

/// What a generator writes from: the domain's name, every named type by
/// name, its functions, and its volatile tables with their row types.
pub struct Walked {
    pub name: String,
    pub types: BTreeMap<String, Type>,
    pub fns: Vec<Stub>,
    pub live: Vec<(String, String)>,
    /// The types UniFFI declares, and its Kotlin package.
    pub typed: BTreeSet<String>,
    pub typed_package: String,
}

impl Walked {
    /// `names` declared by UniFFI in the Kotlin package `package`, each
    /// that is a view's row with its view object.
    pub fn typed(mut self, package: &str, names: &[&str]) -> Walked {
        self.typed = names.iter().map(|n| n.to_string()).collect();
        self.typed_package = package.into();
        self
    }

    /// The typed row types some view reads.
    fn typed_views(&self) -> BTreeSet<&str> {
        self.fns
            .iter()
            .filter_map(|f| match (&f.kind, &f.output) {
                (Kind::View, Some(Type::Record(n, _))) if self.typed.contains(n) => Some(n.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// One function, as a stub calls it: its name, the method's, its kind,
/// input and answer.
pub struct Stub {
    pub name: String,
    pub method: String,
    pub kind: Kind,
    pub input: Type,
    pub output: Option<Type>,
}

/// `snake_case` as `camelCase`, or `CamelCase` with `upper`.
pub fn camel(s: &str, upper: bool) -> String {
    let mut out = String::new();
    for (i, w) in s.split(['_', '.']).filter(|w| !w.is_empty()).enumerate() {
        let mut cs = w.chars();
        let first = cs.next().expect("a word");
        match i == 0 && !upper {
            true => out.extend(first.to_lowercase()),
            false => out.extend(first.to_uppercase()),
        }
        out.extend(cs);
    }
    out
}

/// What a stub is written from: its method `m`, parameter `p`, function
/// `name`, input as JSON `arg`, answer's type `t` and the answer read `rd`,
/// a typed view's row type `r`.
pub struct Hole<'a> {
    pub m: &'a str,
    pub p: &'a str,
    pub name: &'a str,
    pub arg: &'a str,
    pub t: &'a str,
    pub rd: &'a str,
    pub r: &'a str,
}

/// What the two generators spell differently, as a table: the walk over
/// types and stubs is one ([`Lang::ty`], [`Lang::rd`], [`Lang::wr`],
/// [`Lang::stubs`]).
pub struct Lang {
    pub keywords: &'static str,
    /// `Int`, `Bool` and `Unit` as types; `()` as a value; a closure's arrow.
    pub int: &'static str,
    pub bool: &'static str,
    pub unit: &'static str,
    pub unit_value: &'static str,
    pub arrow: &'static str,
    /// A list of a type; an `Int` or a `Bool` as JSON; a named type read
    /// off a tree; a record or sum as JSON; a stub's parameter of a type.
    pub list: fn(&str) -> String,
    pub show: fn(&str) -> String,
    pub read: fn(&str, &str) -> String,
    pub write: fn(&str) -> String,
    pub param: fn(&str) -> String,
    /// The stubs, by kind, and a query's call (`name`, `arg`).
    pub mutator: fn(&Hole) -> String,
    pub live: fn(&Hole) -> String,
    pub typed_view: fn(&Hole) -> String,
    pub view: fn(&Hole) -> String,
    pub query: fn(&Hole) -> String,
    pub query_call: fn(&str, &str) -> String,
    /// The adapter of a typed view of a row type; a live table's rows
    /// (`table`, `row`, the method).
    pub typed: fn(&str) -> String,
    pub live_table: fn(&str, &str, &str) -> String,
}

impl Lang {
    /// A field's or a case's name: `camelCase`, quoted in backticks when it
    /// is a keyword.
    pub fn ident(&self, n: &str) -> String {
        let c = camel(n, false);
        match self.keywords.split(' ').any(|k| k == c) {
            true => format!("`{c}`"),
            false => c,
        }
    }

    /// `t` as a type, a named one as `name` spells it.
    pub fn ty(&self, t: &Type, name: &dyn Fn(&str) -> String) -> String {
        match t {
            Type::Int => self.int.into(),
            Type::Text | Type::Bytes => "String".into(),
            Type::Bool => self.bool.into(),
            Type::Unit => self.unit.into(),
            Type::Id(_) => "Id".into(),
            Type::Option(x) => format!("{}?", self.ty(x, name)),
            Type::List(x) => (self.list)(&self.ty(x, name)),
            Type::Record(n, _) | Type::Sum(n, _) => name(n),
            Type::Tuple(_) => unreachable!("walked"),
        }
    }

    /// An expression reading `e`, a JSON tree, as `t`.
    pub fn rd(&self, t: &Type, e: &str, name: &dyn Fn(&str) -> String, d: usize) -> String {
        let a = self.arrow;
        match t {
            Type::Int => format!("J.int({e})"),
            Type::Text | Type::Bytes => format!("J.text({e})"),
            Type::Bool => format!("J.bool({e})"),
            Type::Unit => self.unit_value.into(),
            Type::Id(_) => format!("Id(J.text({e}))"),
            Type::Option(x) => format!("J.opt({e}) {{ v{d} {a} {} }}", self.rd(x, &format!("v{d}"), name, d + 1)),
            Type::List(x) => format!("J.list({e}) {{ v{d} {a} {} }}", self.rd(x, &format!("v{d}"), name, d + 1)),
            Type::Record(n, _) | Type::Sum(n, _) => (self.read)(&name(n), e),
            Type::Tuple(_) => unreachable!("walked"),
        }
    }

    /// An expression writing `e`, a value of `t`, as its JSON.
    pub fn wr(&self, t: &Type, e: &str, d: usize) -> String {
        let a = self.arrow;
        match t {
            Type::Int | Type::Bool => (self.show)(e),
            Type::Text | Type::Bytes => format!("J.q({e})"),
            Type::Unit => "\"null\"".into(),
            Type::Id(_) => format!("J.q({e}.hex)"),
            Type::Option(x) => format!("J.wopt({e}) {{ v{d} {a} {} }}", self.wr(x, &format!("v{d}"), d + 1)),
            Type::List(x) => format!("J.wlist({e}) {{ v{d} {a} {} }}", self.wr(x, &format!("v{d}"), d + 1)),
            Type::Record(..) | Type::Sum(..) => (self.write)(e),
            Type::Tuple(_) => unreachable!("walked"),
        }
    }

    /// The domain's functions, each a stub calling it by name, then the
    /// typed views' adapters and the live tables, after `out`; the class or
    /// struct they are in closed.
    pub fn stubs(&self, mut out: String, w: &Walked) -> String {
        let bare = |n: &str| n.to_string();
        let views = w.typed_views();
        for f in &w.fns {
            let (p, arg) = match &f.input {
                Type::Unit => (String::new(), "\"null\"".to_string()),
                t => ((self.param)(&self.ty(t, &bare)), self.wr(t, "input", 0)),
            };
            // A method named as a keyword (`banks.import`) is quoted as a
            // field would be.
            let (m, name) = (self.ident(&f.method), f.name.as_str());
            let h = Hole {
                m: &m,
                p: &p,
                name,
                arg: &arg,
                t: "",
                rd: "",
                r: "",
            };
            let line = match (f.kind, &f.output) {
                (Kind::Mutator, _) => (self.mutator)(&h),
                (Kind::Live, _) => (self.live)(&h),
                (Kind::View, Some(Type::Record(r, _))) if views.contains(r.as_str()) => (self.typed_view)(&Hole { r, ..h }),
                (Kind::View, Some(o)) => (self.view)(&Hole {
                    t: &self.ty(o, &bare),
                    rd: &self.rd(o, "j", &bare, 0),
                    ..h
                }),
                (_, Some(o)) => {
                    let rd = self.rd(o, &(self.query_call)(name, &arg), &bare, 0);
                    (self.query)(&Hole {
                        t: &self.ty(o, &bare),
                        rd: &rd,
                        ..h
                    })
                }
                (_, None) => continue,
            };
            let _ = writeln!(out, "    {line}\n");
        }
        for r in &views {
            let _ = writeln!(out, "    {}\n", (self.typed)(r));
        }
        for (table, row) in &w.live {
            let _ = writeln!(out, "    {}\n", (self.live_table)(table, row, &camel(&format!("live_{table}"), false)));
        }
        out.truncate(out.trim_end().len());
        out + "\n}\n"
    }
}

/// Where a record's trailing optional fields begin: those default to
/// nothing in its constructor, so that an optional field added at a
/// record's end (as serde reads a missing one) leaves the code that makes
/// the record as it was.
pub fn optional_from(fields: &[(String, Type)]) -> usize {
    fields.len() - fields.iter().rev().take_while(|(_, t)| matches!(t, Type::Option(_))).count()
}

/// Every record and sum `t` names, by name; one name meaning two types is
/// refused.
fn named(t: &Type, into: &mut BTreeMap<String, Type>) {
    let fields: Vec<&Type> = match t {
        Type::Option(x) | Type::List(x) => vec![x],
        Type::Tuple(_) => panic!("a tuple has no type in Kotlin or Swift: name it as a record"),
        Type::Record(_, fs) => fs.iter().map(|(_, t)| t).collect(),
        Type::Sum(_, vs) => vs.iter().flat_map(|(_, fs)| fs.iter().map(|(_, t)| t)).collect(),
        _ => vec![],
    };
    if let Type::Record(n, _) | Type::Sum(n, _) = t {
        match into.get(n) {
            Some(had) => assert!(had == t, "{n} names two types"),
            None => drop(into.insert(n.clone(), t.clone())),
        }
    }
    fields.into_iter().for_each(|f| named(f, into));
}

/// The domain `name` as `catalog` describes it, walked.
pub fn walk(name: &str, catalog: &Catalog) -> Walked {
    let (mut types, mut live) = (BTreeMap::new(), vec![]);
    for t in &catalog.schema.tables {
        let row = Type::Record(camel(&t.name, true), t.columns.iter().map(|c| (c.name.clone(), c.ty.clone())).collect());
        if t.space == Space::Volatile {
            live.push((t.name.clone(), camel(&t.name, true)));
        }
        named(&row, &mut types);
    }
    let fns: Vec<&Described> = catalog.functions.iter().filter(|f| f.kind != Kind::Scope).collect();
    let last = |n: &str| n.rsplit('.').next().unwrap_or(n).to_string();
    let stubs = fns
        .iter()
        .map(|f| {
            named(&f.input, &mut types);
            f.output.iter().for_each(|o| named(o, &mut types));
            let alone = fns.iter().filter(|g| last(&g.name) == last(&f.name)).count() == 1;
            Stub {
                name: f.name.clone(),
                method: camel(if alone { last(&f.name) } else { f.name.clone() }.as_str(), false),
                kind: f.kind,
                input: f.input.clone(),
                output: f.output.clone(),
            }
        })
        .collect();
    Walked {
        name: camel(name, true),
        types,
        fns: stubs,
        live,
        typed: BTreeSet::new(),
        typed_package: String::new(),
    }
}

/// `arkdb gen kotlin|swift DOMAIN [NAME [PACKAGE]]`: DOMAIN is one of
/// this crate's (`demo`, `room`) or a file holding a domain's `Catalog` as
/// JSON (`{"catalog": …}` or the catalog alone); the source to stdout.
pub fn main(args: &[String]) -> Result<String, String> {
    let [lang, domain, rest @ ..] = args else {
        return Err("arkdb gen kotlin|swift DOMAIN [NAME [PACKAGE]]".into());
    };
    let catalog = match crate::demo::named(domain) {
        Some(d) => Catalog::of(&d),
        None => {
            let text = std::fs::read_to_string(domain).map_err(|e| format!("{domain}: {e}"))?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{domain}: {e}"))?;
            serde_json::from_value(v.get("catalog").unwrap_or(&v).clone()).map_err(|e| format!("{domain}: {e}"))?
        }
    };
    let stem = std::path::Path::new(domain)
        .file_stem()
        .map_or(domain.clone(), |s| s.to_string_lossy().into());
    let w = walk(rest.first().unwrap_or(&stem), &catalog);
    match lang.as_str() {
        "kotlin" => Ok(kotlin::write(&w, rest.get(1).map_or("dev.arkdb.gen", String::as_str))),
        "swift" => Ok(swift::write(&w)),
        other => Err(format!("no generator for {other}")),
    }
}
