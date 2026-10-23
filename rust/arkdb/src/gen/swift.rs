//! A domain in Swift: a struct per record and row, an enum per sum, each
//! read from the JSON tree `JSONSerialization` parses (`init(json:)`) and
//! written as the kernel's JSON (`json`); the stubs over the bridge's
//! module (`ark_ffi`). Of a type UniFFI declares, its JSON as an extension,
//! and its views typed.

use std::fmt::Write;

use ark::row::Type;

use super::{camel, Hole, Lang, Walked};

/// Swift's spelling of the shared walk.
const LANG: Lang = Lang {
    keywords: "as case class default defer do else enum extension false for func guard if import in init is let nil operator protocol repeat return self static struct subscript super switch throw true try var where while",
    int: "Int64",
    bool: "Bool",
    unit: "Void",
    unit_value: "()",
    arrow: "in",
    list: |t| format!("[{t}]"),
    show: |e| format!("String({e})"),
    read: |n, e| format!("{n}(json: {e})"),
    write: |e| format!("{e}.json"),
    param: |t| format!("_ input: {t}"),
    mutator: |Hole { m, p, name, arg, .. }| {
        format!("/// `{name}`: its intent's id.\n    @discardableResult public func {m}({p}) throws -> String {{ try peer.mutate(name: \"{name}\", input: {arg}) }}")
    },
    live: |Hole { m, p, name, arg, .. }| {
        format!("/// `{name}`, on the live space.\n    public func {m}({p}) throws {{ try peer.call(name: \"{name}\", input: {arg}) }}")
    },
    typed_view: |Hole { m, p, name, arg, r, .. }| {
        format!("/// `{name}`, kept, its rows UniFFI's.\n    public func {m}({p}) throws -> LiveView<{r}> {{ typed(try {r}View(peer: peer, name: \"{name}\", input: {arg})) }}")
    },
    view: |Hole { m, p, name, arg, t, rd, .. }| {
        format!("/// `{name}`, kept.\n    public func {m}({p}) throws -> LiveView<{t}> {{ LiveView(try peer.view(name: \"{name}\", input: {arg})) {{ j in {rd} }} }}")
    },
    query: |Hole { m, p, name, t, rd, .. }| format!("/// `{name}`.\n    public func {m}({p}) throws -> {t} {{ {rd} }}"),
    query_call: |name, arg| format!("J.parse(try peer.query(name: \"{name}\", input: {arg}))"),
    typed: |r| format!("private func typed(_ v: {r}View) -> LiveView<{r}> {{ LiveView.typed(rows: v.rows, take: {{ let t = v.take(); return (t.update, t.rows) }}) }}"),
    live_table: |table, row, m| {
        format!("/// The live table `{table}`, as this device holds it.\n    public func {m}() -> [{row}] {{ peer.liveRows(table: \"{table}\").map {{ {row}(json: J.parse($0)) }} }}")
    },
};

fn ty(t: &Type) -> String {
    LANG.ty(t, &|n| n.to_string())
}

fn rd(t: &Type, e: &str, d: usize) -> String {
    LANG.rd(t, e, &|n| n.to_string(), d)
}

/// The JSON object of `fields`, each its variable.
fn object(fields: &[(String, Type)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("\"\\\"{n}\\\":\" + {}", LANG.wr(t, &LANG.ident(n), 0)))
        .collect();
    format!("J.obj([{}])", parts.join(", "))
}

fn labelled(fields: &[(String, Type)], o: &str) -> String {
    let arg = |(n, t): &(String, Type)| format!("{}: {}", LANG.ident(n), rd(t, &format!("{o}[\"{n}\"]"), 0));
    fields.iter().map(arg).collect::<Vec<_>>().join(", ")
}

/// A type's opening: its declaration, or — for one UniFFI declares — an
/// extension of it with its JSON, for the functions called by name.
fn opening(out: &mut String, what: &str, n: &str, decl: &str, typed: bool) {
    match typed {
        true => drop(writeln!(
            out,
            "\n/// The {what} `{n}` (UniFFI's): its JSON, for the functions called by name.\nextension {n} {{"
        )),
        false => drop(writeln!(out, "\n/// The {what} `{n}`.\n{decl} {{")),
    }
}

fn record(out: &mut String, n: &str, fields: &[(String, Type)], typed: bool) {
    opening(out, "record", n, &format!("public struct {n}: Equatable"), typed);
    if !typed {
        for (f, t) in fields {
            let _ = writeln!(out, "    public var {}: {}", LANG.ident(f), ty(t));
        }
        let ps: Vec<String> = fields.iter().map(|(f, t)| format!("{}: {}", LANG.ident(f), ty(t))).collect();
        let sets: String = fields.iter().map(|(f, _)| format!(" self.{0} = {0};", LANG.ident(f))).collect();
        let _ = writeln!(out, "\n    public init({}) {{{sets} }}\n", ps.join(", "));
    }
    let _ = writeln!(
        out,
        "    public init(json j: Any?) {{\n        let o = j as! [String: Any]\n        self.init({})\n    }}\n",
        labelled(fields, "o")
    );
    let _ = writeln!(out, "    public var json: String {{ {} }}\n}}", object(fields));
}

fn sum(out: &mut String, n: &str, variants: &[(String, Vec<(String, Type)>)], typed: bool) {
    opening(out, "sum", n, &format!("public indirect enum {n}: Equatable"), typed);
    let case = |v: &str| LANG.ident(&camel(v, false));
    for (v, fs) in variants.iter().filter(|_| !typed) {
        let ps: Vec<String> = fs.iter().map(|(f, t)| format!("{}: {}", LANG.ident(f), ty(t))).collect();
        let _ = writeln!(
            out,
            "    case {}{}",
            case(v),
            if fs.is_empty() { String::new() } else { format!("({})", ps.join(", ")) }
        );
    }
    let _ = writeln!(
        out,
        "{}    public init(json j: Any?) {{\n        let (t, o) = J.sum(j)\n        switch t {{",
        if typed { "" } else { "\n" }
    );
    for (v, fs) in variants {
        let made = if fs.is_empty() {
            String::new()
        } else {
            format!("({})", labelled(fs, "o!"))
        };
        let _ = writeln!(out, "        case \"{v}\": self = .{}{made}", case(v));
    }
    let _ = writeln!(
        out,
        "        default: fatalError(\"{n}: no variant \\(t)\")\n        }}\n    }}\n\n    public var json: String {{\n        switch self {{"
    );
    for (v, fs) in variants {
        match fs.is_empty() {
            true => drop(writeln!(out, "        case .{}: return \"\\\"{v}\\\"\"", case(v))),
            false => {
                let binds: Vec<String> = fs.iter().map(|(f, _)| LANG.ident(f)).collect();
                let _ = writeln!(
                    out,
                    "        case let .{}({}): return \"{{\\\"{v}\\\":\" + {} + \"}}\"",
                    case(v),
                    binds.join(", "),
                    object(fs)
                );
            }
        }
    }
    let _ = writeln!(out, "        }}\n    }}\n}}");
}

/// The Swift of a walked domain.
pub fn write(w: &Walked) -> String {
    let mut out = format!("// Generated by `arkdb gen swift` from the domain {}: its types and its functions by name, over\n// the bridge (ark_ffi). Never edit; regenerate.\nimport Foundation\n", w.name);
    for (n, t) in &w.types {
        match t {
            Type::Record(_, fs) => record(&mut out, n, fs, w.typed.contains(n)),
            Type::Sum(_, vs) => sum(&mut out, n, vs, w.typed.contains(n)),
            _ => unreachable!("named"),
        }
    }
    let _ = writeln!(out, "\n/// The domain's functions, each called by its name through `peer`.\npublic struct {} {{\n    public let peer: Peer\n\n    public init(peer: Peer) {{ self.peer = peer }}\n", w.name);
    LANG.stubs(out, w)
}
