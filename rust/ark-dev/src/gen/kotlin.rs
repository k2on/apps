//! A domain in Kotlin: a data class per record and row, a sealed class per
//! sum, each read from the JSON tree Android's `org.json` parses (`of`) and
//! written as the kernel's JSON (`json`); the stubs over `dev.arkdb.ffi`.

use std::fmt::Write;

use ark::domain::Kind;
use ark::row::Type;

use super::{camel, Walked};

const KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

fn field(n: &str) -> String {
    let c = camel(n, false);
    if KEYWORDS.contains(&c.as_str()) {
        format!("`{c}`")
    } else {
        c
    }
}

/// `t` as a Kotlin type, a named one under `q` (the package, where a
/// variant's name could hide it).
fn ty(t: &Type, q: &str) -> String {
    match t {
        Type::Int => "Long".into(),
        Type::Text | Type::Bytes => "String".into(),
        Type::Bool => "Boolean".into(),
        Type::Unit => "Unit".into(),
        Type::Id(_) => format!("{q}Id"),
        Type::Option(x) => format!("{}?", ty(x, q)),
        Type::List(x) => format!("List<{}>", ty(x, q)),
        Type::Record(n, _) | Type::Sum(n, _) => format!("{q}{n}"),
        Type::Tuple(_) => unreachable!("walked"),
    }
}

/// An expression reading `e`, a JSON tree, as `t`.
fn rd(t: &Type, e: &str, q: &str, d: usize) -> String {
    match t {
        Type::Int => format!("J.int({e})"),
        Type::Text | Type::Bytes => format!("J.text({e})"),
        Type::Bool => format!("J.bool({e})"),
        Type::Unit => "Unit".into(),
        Type::Id(_) => format!("{q}Id(J.text({e}))"),
        Type::Option(x) => format!("J.opt({e}) {{ v{d} -> {} }}", rd(x, &format!("v{d}"), q, d + 1)),
        Type::List(x) => format!("J.list({e}) {{ v{d} -> {} }}", rd(x, &format!("v{d}"), q, d + 1)),
        Type::Record(n, _) | Type::Sum(n, _) => format!("{q}{n}.of({e})"),
        Type::Tuple(_) => unreachable!("walked"),
    }
}

/// An expression writing `e`, a value of `t`, as its JSON.
fn wr(t: &Type, e: &str, d: usize) -> String {
    match t {
        Type::Int | Type::Bool => format!("{e}.toString()"),
        Type::Text | Type::Bytes => format!("J.q({e})"),
        Type::Unit => "\"null\"".into(),
        Type::Id(_) => format!("J.q({e}.hex)"),
        Type::Option(x) => format!("J.wopt({e}) {{ v{d} -> {} }}", wr(x, &format!("v{d}"), d + 1)),
        Type::List(x) => format!("J.wlist({e}) {{ v{d} -> {} }}", wr(x, &format!("v{d}"), d + 1)),
        Type::Record(..) | Type::Sum(..) => format!("{e}.json()"),
        Type::Tuple(_) => unreachable!("walked"),
    }
}

/// The JSON object of `fields`, each read off `this`.
fn object(fields: &[(String, Type)]) -> String {
    if fields.is_empty() {
        return "\"{}\"".into();
    }
    let parts: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("\"\\\"{n}\\\":\" + {}", wr(t, &field(n), 0)))
        .collect();
    format!("\"{{\" + {} + \"}}\"", parts.join(" + \",\" + "))
}

fn params(fields: &[(String, Type)], q: &str) -> String {
    fields
        .iter()
        .map(|(n, t)| format!("val {}: {}", field(n), ty(t, q)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn args(fields: &[(String, Type)], o: &str, q: &str) -> String {
    fields
        .iter()
        .map(|(n, t)| rd(t, &format!("{o}.opt(\"{n}\")"), q, 0))
        .collect::<Vec<_>>()
        .join(", ")
}

fn record(out: &mut String, n: &str, fields: &[(String, Type)]) {
    let _ = writeln!(out, "\n/** The record `{n}`. */");
    match fields.is_empty() {
        true => drop(writeln!(out, "class {n} {{")),
        false => drop(writeln!(out, "data class {n}({}) {{", params(fields, ""))),
    }
    let _ = writeln!(out, "    fun json(): String = {}\n", object(fields));
    let _ = writeln!(out, "    companion object {{");
    let _ = writeln!(
        out,
        "        fun of(j: Any?): {n} = (j as JSONObject).let {{ o -> {n}({}) }}\n    }}\n}}",
        args(fields, "o", "")
    );
}

fn sum(out: &mut String, n: &str, variants: &[(String, Vec<(String, Type)>)], q: &str) {
    let _ = writeln!(out, "\n/** The sum `{n}`. */\nsealed class {n} {{\n    abstract fun json(): String");
    let mut arms = vec![];
    for (v, fs) in variants {
        if fs.is_empty() {
            let _ = writeln!(
                out,
                "\n    data object {v} : {n}() {{\n        override fun json(): String = \"\\\"{v}\\\"\"\n    }}"
            );
            arms.push(format!("\"{v}\" -> {v}"));
            continue;
        }
        let _ = writeln!(out, "\n    data class {v}({}) : {n}() {{", params(fs, q));
        let _ = writeln!(
            out,
            "        override fun json(): String = \"{{\\\"{v}\\\":\" + {} + \"}}\"\n    }}",
            object(fs)
        );
        arms.push(format!("\"{v}\" -> {v}({})", args(fs, "o!!", q)));
    }
    let _ = writeln!(
        out,
        "\n    companion object {{\n        fun of(j: Any?): {n} = J.sum(j) {{ t, o ->\n            when (t) {{"
    );
    for a in arms {
        let _ = writeln!(out, "                {a}");
    }
    let _ = writeln!(
        out,
        "                else -> throw IllegalArgumentException(\"{n}: no variant $t\")\n            }}\n        }}\n    }}\n}}"
    );
}

/// The Kotlin of a walked domain, in `package`.
pub fn write(w: &Walked, package: &str) -> String {
    let mut out = format!("// Generated by `ark-dev gen kotlin` from the domain {}: its types and its functions by name, over\n// dev.arkdb.ffi. Never edit; regenerate.\npackage {package}\n\n", w.name);
    out += "import dev.arkdb.ffi.Id\nimport dev.arkdb.ffi.J\nimport dev.arkdb.ffi.Peer\nimport dev.arkdb.ffi.View\nimport org.json.JSONObject\n";
    let q = format!("{package}.");
    for (n, t) in &w.types {
        match t {
            Type::Record(_, fs) => record(&mut out, n, fs),
            Type::Sum(_, vs) => sum(&mut out, n, vs, &q),
            _ => unreachable!("named"),
        }
    }
    let _ = writeln!(
        out,
        "\n/** The domain's functions, each called by its name through [peer]. */\nclass {}(val peer: Peer) {{",
        w.name
    );
    for f in &w.fns {
        let (p, arg) = match &f.input {
            Type::Unit => (String::new(), "\"null\"".to_string()),
            t => (format!("input: {}", ty(t, "")), wr(t, "input", 0)),
        };
        let (m, name) = (&f.method, &f.name);
        let line = match (f.kind, &f.output) {
            (Kind::Mutator, _) => format!("/** `{name}`: its intent's id. */\n    fun {m}({p}): String = peer.mutate(\"{name}\", {arg})"),
            (Kind::Live, _) => format!("/** `{name}`, on the live space. */\n    fun {m}({p}) = peer.call(\"{name}\", {arg})"),
            (Kind::View, Some(o)) => format!(
                "/** `{name}`, kept. */\n    fun {m}({p}): View<{}> = View(peer.view(\"{name}\", {arg})) {{ j -> {} }}",
                ty(o, ""),
                rd(o, "j", "", 0)
            ),
            (_, Some(o)) => format!(
                "/** `{name}`. */\n    fun {m}({p}): {} = {}",
                ty(o, ""),
                rd(o, &format!("J.parse(peer.query(\"{name}\", {arg}))"), "", 0)
            ),
            (_, None) => continue,
        };
        let _ = writeln!(out, "    {line}\n");
    }
    for (table, row) in &w.live {
        let _ = writeln!(
            out,
            "    /** The live table `{table}`, as this device holds it. */\n    fun {}(): List<{row}> = peer.liveRows(\"{table}\").map {{ {row}.of(J.parse(it)) }}\n",
            camel(&format!("live_{table}"), false)
        );
    }
    out.truncate(out.trim_end().len());
    out + "\n}\n"
}
