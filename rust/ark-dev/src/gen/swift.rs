//! A domain in Swift: a struct per record and row, an enum per sum, each
//! read from the JSON tree `JSONSerialization` parses (`init(json:)`) and
//! written as the kernel's JSON (`json`); the stubs over the bridge's
//! module (`ark_ffi`).

use std::fmt::Write;

use ark::domain::Kind;
use ark::row::Type;

use super::{camel, ident, Walked};

const KEYWORDS: &str = "as case class default defer do else enum extension false for func guard if import in init is let nil operator protocol repeat return self static struct subscript super switch throw true try var where while";

fn field(n: &str) -> String {
    ident(n, KEYWORDS)
}

fn ty(t: &Type) -> String {
    match t {
        Type::Int => "Int64".into(),
        Type::Text | Type::Bytes => "String".into(),
        Type::Bool => "Bool".into(),
        Type::Unit => "Void".into(),
        Type::Id(_) => "Id".into(),
        Type::Option(x) => format!("{}?", ty(x)),
        Type::List(x) => format!("[{}]", ty(x)),
        Type::Record(n, _) | Type::Sum(n, _) => n.clone(),
        Type::Tuple(_) => unreachable!("walked"),
    }
}

/// An expression reading `e`, a JSON tree, as `t`.
fn rd(t: &Type, e: &str, d: usize) -> String {
    match t {
        Type::Int => format!("J.int({e})"),
        Type::Text | Type::Bytes => format!("J.text({e})"),
        Type::Bool => format!("J.bool({e})"),
        Type::Unit => "()".into(),
        Type::Id(_) => format!("Id(J.text({e}))"),
        Type::Option(x) => format!("J.opt({e}) {{ v{d} in {} }}", rd(x, &format!("v{d}"), d + 1)),
        Type::List(x) => format!("J.list({e}) {{ v{d} in {} }}", rd(x, &format!("v{d}"), d + 1)),
        Type::Record(n, _) | Type::Sum(n, _) => format!("{n}(json: {e})"),
        Type::Tuple(_) => unreachable!("walked"),
    }
}

/// An expression writing `e`, a value of `t`, as its JSON.
fn wr(t: &Type, e: &str, d: usize) -> String {
    match t {
        Type::Int | Type::Bool => format!("String({e})"),
        Type::Text | Type::Bytes => format!("J.q({e})"),
        Type::Unit => "\"null\"".into(),
        Type::Id(_) => format!("J.q({e}.hex)"),
        Type::Option(x) => format!("J.wopt({e}) {{ v{d} in {} }}", wr(x, &format!("v{d}"), d + 1)),
        Type::List(x) => format!("J.wlist({e}) {{ v{d} in {} }}", wr(x, &format!("v{d}"), d + 1)),
        Type::Record(..) | Type::Sum(..) => format!("{e}.json"),
        Type::Tuple(_) => unreachable!("walked"),
    }
}

/// The JSON object of `fields`, each its variable.
fn object(fields: &[(String, Type)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("\"\\\"{n}\\\":\" + {}", wr(t, &field(n), 0)))
        .collect();
    format!("J.obj([{}])", parts.join(", "))
}

fn labelled(fields: &[(String, Type)], o: &str) -> String {
    let arg = |(n, t): &(String, Type)| format!("{}: {}", field(n), rd(t, &format!("{o}[\"{n}\"]"), 0));
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
            let _ = writeln!(out, "    public var {}: {}", field(f), ty(t));
        }
        let ps: Vec<String> = fields.iter().map(|(f, t)| format!("{}: {}", field(f), ty(t))).collect();
        let sets: String = fields.iter().map(|(f, _)| format!(" self.{0} = {0};", field(f))).collect();
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
    let case = |v: &str| field(&camel(v, false));
    for (v, fs) in variants.iter().filter(|_| !typed) {
        let ps: Vec<String> = fs.iter().map(|(f, t)| format!("{}: {}", field(f), ty(t))).collect();
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
                let binds: Vec<String> = fs.iter().map(|(f, _)| field(f)).collect();
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
    let mut out = format!("// Generated by `ark-dev gen swift` from the domain {}: its types and its functions by name, over\n// the bridge (ark_ffi). Never edit; regenerate.\nimport Foundation\n", w.name);
    for (n, t) in &w.types {
        match t {
            Type::Record(_, fs) => record(&mut out, n, fs, w.typed.contains(n)),
            Type::Sum(_, vs) => sum(&mut out, n, vs, w.typed.contains(n)),
            _ => unreachable!("named"),
        }
    }
    let n = &w.name;
    let _ = writeln!(out, "\n/// The domain's functions, each called by its name through `peer`.\npublic struct {n} {{\n    public let peer: Peer\n\n    public init(peer: Peer) {{ self.peer = peer }}\n");
    for f in &w.fns {
        let (p, arg) = match &f.input {
            Type::Unit => (String::new(), "\"null\"".to_string()),
            t => (format!("_ input: {}", ty(t)), wr(t, "input", 0)),
        };
        let (m, name) = (&f.method, &f.name);
        let line = match (f.kind, &f.output) {
            (Kind::Mutator, _) => format!(
                "/// `{name}`: its intent's id.\n    @discardableResult public func {m}({p}) throws -> String {{ try peer.mutate(name: \"{name}\", input: {arg}) }}"
            ),
            (Kind::Live, _) => format!("/// `{name}`, on the live space.\n    public func {m}({p}) throws {{ try peer.call(name: \"{name}\", input: {arg}) }}"),
            (Kind::View, Some(o)) => format!(
                "/// `{name}`, kept.\n    public func {m}({p}) throws -> View<{}> {{ View(try peer.view(name: \"{name}\", input: {arg})) {{ j in {} }} }}",
                ty(o),
                rd(o, "j", 0)
            ),
            (_, Some(o)) => format!(
                "/// `{name}`.\n    public func {m}({p}) throws -> {} {{ {} }}",
                ty(o),
                rd(o, &format!("J.parse(try peer.query(name: \"{name}\", input: {arg}))"), 0)
            ),
            (_, None) => continue,
        };
        let _ = writeln!(out, "    {line}\n");
    }
    for (table, row) in &w.live {
        let m = camel(&format!("live_{table}"), false);
        let _ = writeln!(out, "    /// The live table `{table}`, as this device holds it.\n    public func {m}() -> [{row}] {{ peer.liveRows(table: \"{table}\").map {{ {row}(json: J.parse($0)) }} }}\n");
    }
    out.truncate(out.trim_end().len());
    out + "\n}\n"
}
