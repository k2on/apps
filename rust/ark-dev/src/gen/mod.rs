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
//! A domain is walked as the admin page describes it (`Catalog`): its
//! schema — every table a row type, named as the table — and every
//! function's name, kind, input and answer.

pub mod kotlin;
pub mod swift;

use std::collections::{BTreeMap, BTreeSet};

use ark::domain::Kind;
use ark::row::Type;
use ark::schema::Space;
use ark_explorer::Catalog;

/// What a generator writes from: the domain's name, every named type by
/// name, its functions, and its volatile tables with their row types.
pub struct Walked {
    pub name: String,
    pub types: BTreeMap<String, Type>,
    pub fns: Vec<Stub>,
    pub live: Vec<(String, String)>,
    /// The types UniFFI declares (the typed path, `ark_ffi::typed`), and
    /// its Kotlin package: of these only their JSON is written, as
    /// extensions, for the functions still called by name.
    pub typed: BTreeSet<String>,
    pub typed_package: String,
}

impl Walked {
    /// `names` declared by UniFFI in the Kotlin package `package`.
    pub fn typed(mut self, package: &str, names: &[&str]) -> Walked {
        self.typed = names.iter().map(|n| n.to_string()).collect();
        self.typed_package = package.into();
        self
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

/// A field's or a case's name: `camelCase`, quoted in backticks when it is
/// one of `keywords`.
pub fn ident(n: &str, keywords: &str) -> String {
    let c = camel(n, false);
    match keywords.split(' ').any(|k| k == c) {
        true => format!("`{c}`"),
        false => c,
    }
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
    let fns: Vec<&ark_explorer::Described> = catalog.functions.iter().filter(|f| f.kind != Kind::Scope).collect();
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

/// `ark-dev gen kotlin|swift DOMAIN [NAME [PACKAGE]]`: DOMAIN is one of
/// this crate's (`demo`, `room`) or a file of the catalog the admin page
/// serves (`/admin/api/state`, or its `catalog`); the source to stdout.
pub fn main(args: &[String]) -> Result<String, String> {
    let [lang, domain, rest @ ..] = args else {
        return Err("ark-dev gen kotlin|swift DOMAIN [NAME [PACKAGE]]".into());
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
