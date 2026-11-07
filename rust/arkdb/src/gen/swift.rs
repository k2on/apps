//! A domain in Swift: a struct per record and row, an enum per sum, each
//! read from its bytes in the value format (`init(_ r: Reader)`) or off the
//! JSON tree `JSONSerialization` parses (`init(json:)`), and written as the
//! kernel's JSON (`json`); the stubs over the bridge's module (`ark_ffi`).

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
    read: |n, r| format!("{n}({r})"),
    of: |n, e| format!("{n}(json: {e})"),
    write: |e| format!("{e}.json"),
    param: |t| format!("_ input: {t}"),
    mutator: |Hole { m, p, name, arg, .. }| {
        format!("/// `{name}`: its intent's id.\n    @discardableResult public func {m}({p}) throws -> String {{ try peer.mutate(name: \"{name}\", input: {arg}) }}")
    },
    live: |Hole { m, p, name, arg, .. }| {
        format!("/// `{name}`, on the live space.\n    public func {m}({p}) throws {{ _ = try peer.mutate(name: \"{name}\", input: {arg}) }}")
    },
    view: |Hole { m, p, name, arg, t, rd, .. }| {
        format!("/// `{name}`, kept.\n    public func {m}({p}) throws -> LiveView<{t}> {{ LiveView(try peer.view(name: \"{name}\", input: {arg})) {{ r in {rd} }} }}")
    },
    live_table: |table, row, m| {
        format!("/// The live table `{table}`, as this device holds it.\n    public func {m}() -> [{row}] {{ peer.liveRows(table: \"{table}\").map {{ {row}(Reader($0)) }} }}")
    },
};

fn ty(t: &Type) -> String {
    LANG.ty(t, &|n| n.to_string())
}

fn rd(t: &Type) -> String {
    LANG.rd(t, "r", &|n| n.to_string(), 0)
}

/// The JSON object of `fields`, each its variable.
fn object(fields: &[(String, Type)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("\"\\\"{n}\\\":\" + {}", LANG.wr(t, &LANG.ident(n), 0)))
        .collect();
    format!("J.obj([{}])", parts.join(", "))
}

/// Each field read from `r`, in order, by its label.
fn labelled(fields: &[(String, Type)]) -> String {
    let arg = |(n, t): &(String, Type)| format!("{}: {}", LANG.ident(n), rd(t));
    fields.iter().map(arg).collect::<Vec<_>>().join(", ")
}

/// Each field read off the JSON object `o`, by its label.
fn json_labelled(fields: &[(String, Type)], o: &str) -> String {
    let rj = |t: &Type, e: &str| LANG.rj(t, e, &|n| n.to_string(), 0);
    let arg = |(n, t): &(String, Type)| format!("{}: {}", LANG.ident(n), rj(t, &format!("{o}[\"{n}\"]")));
    fields.iter().map(arg).collect::<Vec<_>>().join(", ")
}

fn record(out: &mut String, n: &str, fields: &[(String, Type)]) {
    let _ = writeln!(out, "\n/// The record `{n}`.\npublic struct {n}: Hashable {{");
    for (f, t) in fields {
        let _ = writeln!(out, "    public var {}: {}", LANG.ident(f), ty(t));
    }
    // Trailing optional fields default to nil, as a missing one reads
    // (`super::optional_from`).
    let from = super::optional_from(fields);
    let ps: Vec<String> = fields
        .iter()
        .enumerate()
        .map(|(i, (f, t))| format!("{}: {}{}", LANG.ident(f), ty(t), if i >= from { " = nil" } else { "" }))
        .collect();
    let sets: String = fields.iter().map(|(f, _)| format!(" self.{0} = {0};", LANG.ident(f))).collect();
    let _ = writeln!(out, "\n    public init({}) {{{sets} }}\n", ps.join(", "));
    let _ = writeln!(out, "    public init(_ r: Reader) {{ self.init({}) }}\n", labelled(fields));
    let _ = writeln!(
        out,
        "    public init(json j: Any?) {{\n        let o = j as! [String: Any]\n        self.init({})\n    }}\n",
        json_labelled(fields, "o")
    );
    let _ = writeln!(out, "    public var json: String {{ {} }}\n}}", object(fields));
}

fn sum(out: &mut String, n: &str, variants: &[(String, Vec<(String, Type)>)]) {
    let _ = writeln!(out, "\n/// The sum `{n}`.\npublic indirect enum {n}: Hashable {{");
    let case = |v: &str| LANG.ident(&camel(v, false));
    for (v, fs) in variants {
        let ps: Vec<String> = fs.iter().map(|(f, t)| format!("{}: {}", LANG.ident(f), ty(t))).collect();
        let _ = writeln!(
            out,
            "    case {}{}",
            case(v),
            if fs.is_empty() { String::new() } else { format!("({})", ps.join(", ")) }
        );
    }
    let _ = writeln!(out, "\n    public init(_ r: Reader) {{\n        switch r.variant() {{");
    for (i, (v, fs)) in variants.iter().enumerate() {
        let made = if fs.is_empty() { String::new() } else { format!("({})", labelled(fs)) };
        let _ = writeln!(out, "        case {i}: self = .{}{made}", case(v));
    }
    let _ = writeln!(
        out,
        "        case let v: fatalError(\"{n}: no variant \\(v)\")\n        }}\n    }}\n\n    public init(json j: Any?) {{\n        let (t, o) = J.sum(j)\n        switch t {{"
    );
    for (v, fs) in variants {
        let made = if fs.is_empty() {
            String::new()
        } else {
            format!("({})", json_labelled(fs, "o!"))
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
            Type::Record(_, fs) => record(&mut out, n, fs),
            Type::Sum(_, vs) => sum(&mut out, n, vs),
            _ => unreachable!("named"),
        }
    }
    let _ = writeln!(out, "\n/// The domain's functions, each called by its name through `peer`.\npublic struct {} {{\n    public let peer: Peer\n\n    public init(peer: Peer) {{ self.peer = peer }}\n", w.name);
    LANG.stubs(out, w)
}
