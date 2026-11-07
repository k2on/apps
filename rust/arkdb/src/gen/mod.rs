//! A domain's types and stubs in Kotlin and Swift (§1.16, N3): what an app
//! over the bridge (`ark-ffi`) calls its domain with, **types only** — a
//! data class or struct per record and row, a sealed class or enum per sum,
//! each read from its bytes in the value format (`ark::format`: fields in
//! declaration order, a sum its variant's index), which is how a view's
//! rows cross, and written as the JSON the kernel's serde writes, which is
//! how an input crosses (and read from it, which is how an app's own JSON
//! is read); and a stub per function calling it by name: a
//! mutator's `mutate` (giving its intent's id, or nothing on the live
//! space), a view's handle typed. Nothing of a function's body is ever
//! generated. What the generated code stands on — `Id`, `View`, the
//! value format's `Reader` and JSON — is the bridge's own typed
//! half, written once (`rust/ark-ffi/kotlin/Typed.kt`,
//! `rust/ark-ffi/swift/Typed.swift`).
//!
//! A domain is walked as it registers itself: its schema — every table a
//! row type, named as the table — and every function's name, kind, space,
//! input and answer.

pub mod kotlin;
pub mod swift;

use std::collections::BTreeMap;
use std::fmt::Write;

use ark::domain::{Domain, Function, Kind};
use ark::row::Type;
use ark::schema::Space;

/// What a generator writes from: the domain's name, every named type by
/// name, its functions, and its volatile tables with their row types.
pub struct Walked {
    pub name: String,
    pub types: BTreeMap<String, Type>,
    pub fns: Vec<Stub>,
    pub live: Vec<(String, String)>,
}

/// One function, as a stub calls it: its name, the method's, its kind,
/// input and answer.
pub struct Stub {
    pub name: String,
    pub method: String,
    pub kind: Kind,
    pub space: Space,
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
/// `name`, input as JSON `arg`, answer's type `t` and a row read from `r`,
/// `rd`.
pub struct Hole<'a> {
    pub m: &'a str,
    pub p: &'a str,
    pub name: &'a str,
    pub arg: &'a str,
    pub t: &'a str,
    pub rd: &'a str,
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
    /// from a reader, and off a JSON tree; a record or sum as JSON; a
    /// stub's parameter of a type.
    pub list: fn(&str) -> String,
    pub show: fn(&str) -> String,
    pub read: fn(&str, &str) -> String,
    pub of: fn(&str, &str) -> String,
    pub write: fn(&str) -> String,
    pub param: fn(&str) -> String,
    /// The stubs, by kind (a mutator of the live space's `live`).
    pub mutator: fn(&Hole) -> String,
    pub live: fn(&Hole) -> String,
    pub view: fn(&Hole) -> String,
    /// A live table's rows (`table`, `row`, the method).
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

    /// An expression reading a `t` from `r`, a reader of the value format.
    pub fn rd(&self, t: &Type, r: &str, name: &dyn Fn(&str) -> String, d: usize) -> String {
        let a = self.arrow;
        match t {
            Type::Int => format!("{r}.int()"),
            Type::Text => format!("{r}.text()"),
            Type::Bytes => format!("{r}.hex()"),
            Type::Bool => format!("{r}.bool()"),
            Type::Unit => self.unit_value.into(),
            Type::Id(_) => format!("{r}.id()"),
            Type::Option(x) => format!("{r}.opt {{ r{d} {a} {} }}", self.rd(x, &format!("r{d}"), name, d + 1)),
            Type::List(x) => format!("{r}.list {{ r{d} {a} {} }}", self.rd(x, &format!("r{d}"), name, d + 1)),
            Type::Record(n, _) | Type::Sum(n, _) => (self.read)(&name(n), r),
            Type::Tuple(_) => unreachable!("walked"),
        }
    }

    /// An expression reading `e`, a JSON tree, as `t`: what the app's own
    /// JSON (harken's listening session, its player's) is read with.
    pub fn rj(&self, t: &Type, e: &str, name: &dyn Fn(&str) -> String, d: usize) -> String {
        let a = self.arrow;
        match t {
            Type::Int => format!("J.int({e})"),
            Type::Text | Type::Bytes => format!("J.text({e})"),
            Type::Bool => format!("J.bool({e})"),
            Type::Unit => self.unit_value.into(),
            Type::Id(_) => format!("Id(J.text({e}))"),
            Type::Option(x) => format!("J.opt({e}) {{ v{d} {a} {} }}", self.rj(x, &format!("v{d}"), name, d + 1)),
            Type::List(x) => format!("J.list({e}) {{ v{d} {a} {} }}", self.rj(x, &format!("v{d}"), name, d + 1)),
            Type::Record(n, _) | Type::Sum(n, _) => (self.of)(&name(n), e),
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
    /// live tables, after `out`; the class or struct they are in closed.
    pub fn stubs(&self, mut out: String, w: &Walked) -> String {
        let bare = |n: &str| n.to_string();
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
            };
            let line = match (f.kind, &f.output) {
                (Kind::Mutator, _) if f.space == Space::Volatile => (self.live)(&h),
                (Kind::Mutator, _) => (self.mutator)(&h),
                (Kind::View, Some(o)) => (self.view)(&Hole {
                    t: &self.ty(o, &bare),
                    rd: &self.rd(o, "r", &bare, 0),
                    ..h
                }),
                _ => continue,
            };
            let _ = writeln!(out, "    {line}\n");
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

/// The domain `d`, called `name`, walked.
pub fn walk(name: &str, d: &Domain) -> Walked {
    let (mut types, mut live) = (BTreeMap::new(), vec![]);
    for t in &d.schema.tables {
        let row = Type::Record(camel(&t.name, true), t.columns.iter().map(|c| (c.name.clone(), c.ty.clone())).collect());
        if t.space == Space::Volatile {
            live.push((t.name.clone(), camel(&t.name, true)));
        }
        named(&row, &mut types);
    }
    let fns: Vec<&Function> = d.fns.values().filter(|f| f.kind != Kind::Scope).collect();
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
                space: f.space,
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
    }
}

/// `arkdb gen kotlin|swift DOMAIN [NAME [PACKAGE]]`: DOMAIN is one of
/// this crate's (`demo`, `room`; an app's crate walks its own, as
/// `harken/ffi/tests/gen.rs` does); the source to stdout.
pub fn main(args: &[String]) -> Result<String, String> {
    let [lang, domain, rest @ ..] = args else {
        return Err("arkdb gen kotlin|swift DOMAIN [NAME [PACKAGE]]".into());
    };
    let d = crate::demo::named(domain).ok_or_else(|| format!("no domain {domain} here: demo or room"))?;
    let w = walk(rest.first().unwrap_or(domain), &d);
    match lang.as_str() {
        "kotlin" => Ok(kotlin::write(&w, rest.get(1).map_or("dev.arkdb.gen", String::as_str))),
        "swift" => Ok(swift::write(&w)),
        other => Err(format!("no generator for {other}")),
    }
}
