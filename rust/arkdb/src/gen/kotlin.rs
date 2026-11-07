//! A domain in Kotlin: a data class per record and row, a sealed class per
//! sum, each read from its bytes in the value format (`read`, over the
//! bridge's `Reader`) or off the JSON tree Android's `org.json` parses
//! (`of`), and written as the kernel's JSON (`json`); the stubs over
//! `dev.arkdb.ffi`.

use std::fmt::Write;

use ark::row::Type;

use super::{Hole, Lang, Walked};

/// How a named type is spelt: bare, or qualified by its package, where a
/// variant's name could hide it.
#[derive(Clone, Copy)]
struct Q<'a> {
    package: &'a str,
    qualified: bool,
}

impl Q<'_> {
    fn name(&self, n: &str) -> String {
        match self.qualified {
            false => n.into(),
            true => format!("{}.{n}", self.package),
        }
    }

    fn bare(self) -> Self {
        Q { qualified: false, ..self }
    }
}

/// Kotlin's spelling of the shared walk.
const LANG: Lang = Lang {
    keywords: "as break class continue do else false for fun if in interface is null object package return super this throw true try typealias typeof val var when while",
    int: "Long",
    bool: "Boolean",
    unit: "Unit",
    unit_value: "Unit",
    arrow: "->",
    list: |t| format!("List<{t}>"),
    show: |e| format!("{e}.toString()"),
    read: |n, r| format!("{n}.read({r})"),
    of: |n, e| format!("{n}.of({e})"),
    write: |e| format!("{e}.json()"),
    param: |t| format!("input: {t}"),
    mutator: |Hole { m, p, name, arg, .. }| format!("/** `{name}`: its intent's id. */\n    fun {m}({p}): String = peer.mutate(\"{name}\", {arg})"),
    live: |Hole { m, p, name, arg, .. }| format!("/** `{name}`, on the live space. */\n    fun {m}({p}) {{ peer.mutate(\"{name}\", {arg}) }}"),
    view: |Hole { m, p, name, arg, t, rd, .. }| {
        format!("/** `{name}`, kept. */\n    fun {m}({p}): View<{t}> = View(peer.view(\"{name}\", {arg})) {{ r -> {rd} }}")
    },
    live_table: |table, row, m| {
        format!("/** The live table `{table}`, as this device holds it. */\n    fun {m}(): List<{row}> = peer.liveRows(\"{table}\").map {{ {row}.read(Reader(it)) }}")
    },
};

fn ty(t: &Type, q: Q) -> String {
    LANG.ty(t, &|n| q.name(n))
}

fn rd(t: &Type, q: Q) -> String {
    LANG.rd(t, "r", &|n| q.name(n), 0)
}

/// The JSON object of `fields`, each read off `this`.
fn object(fields: &[(String, Type)]) -> String {
    if fields.is_empty() {
        return "\"{}\"".into();
    }
    let parts: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("\"\\\"{n}\\\":\" + {}", LANG.wr(t, &LANG.ident(n), 0)))
        .collect();
    format!("\"{{\" + {} + \"}}\"", parts.join(" + \",\" + "))
}

fn params(fields: &[(String, Type)], q: Q) -> String {
    fields
        .iter()
        .map(|(n, t)| format!("val {}: {}", LANG.ident(n), ty(t, q)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Each field read from `r`, in order.
fn args(fields: &[(String, Type)], q: Q) -> String {
    fields.iter().map(|(_, t)| rd(t, q)).collect::<Vec<_>>().join(", ")
}

/// Each field read off the JSON object `o`.
fn json_args(fields: &[(String, Type)], o: &str, q: Q) -> String {
    let rj = |t: &Type, e: &str| LANG.rj(t, e, &|n| q.name(n), 0);
    fields
        .iter()
        .map(|(n, t)| rj(t, &format!("{o}.opt(\"{n}\")")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn record(out: &mut String, n: &str, fields: &[(String, Type)], q: Q) {
    let _ = writeln!(out, "\n/** The record `{n}`. */");
    match fields.is_empty() {
        true => drop(writeln!(out, "class {n} {{")),
        false => {
            // Trailing optional fields default to null, as a missing one
            // reads (`super::optional_from`).
            let ps: Vec<String> = fields
                .iter()
                .enumerate()
                .map(|(i, (f, t))| {
                    let default = if i >= super::optional_from(fields) { " = null" } else { "" };
                    format!("val {}: {}{default}", LANG.ident(f), ty(t, q.bare()))
                })
                .collect();
            let _ = writeln!(out, "data class {n}({}) {{", ps.join(", "));
        }
    }
    let _ = writeln!(out, "    fun json(): String = {}\n", object(fields));
    let _ = writeln!(out, "    companion object {{");
    let _ = writeln!(out, "        fun read(r: Reader): {n} = {n}({})\n", args(fields, q.bare()));
    let of = json_args(fields, "o", q.bare());
    let _ = writeln!(
        out,
        "        fun of(j: Any?): {n} = (j as JSONObject).let {{ o -> {n}({of}) }}\n    }}\n}}"
    );
}

fn sum(out: &mut String, n: &str, variants: &[(String, Vec<(String, Type)>)], q: Q) {
    let _ = writeln!(out, "\n/** The sum `{n}`. */\nsealed class {n} {{\n    abstract fun json(): String");
    // Inside its body a variant named as the sum hides it: the sum there is
    // qualified by its package.
    let me = match variants.iter().any(|(v, _)| v == n) {
        true => q.name(n),
        false => n.to_string(),
    };
    let (mut arms, mut tags) = (vec![], vec![]);
    for (i, (v, fs)) in variants.iter().enumerate() {
        if fs.is_empty() {
            let _ = writeln!(
                out,
                "\n    data object {v} : {me}() {{\n        override fun json(): String = \"\\\"{v}\\\"\"\n    }}"
            );
            arms.push(format!("{i} -> {v}"));
            tags.push(format!("\"{v}\" -> {v}"));
            continue;
        }
        let _ = writeln!(out, "\n    data class {v}({}) : {me}() {{", params(fs, q));
        let _ = writeln!(
            out,
            "        override fun json(): String = \"{{\\\"{v}\\\":\" + {} + \"}}\"\n    }}",
            object(fs)
        );
        arms.push(format!("{i} -> {v}({})", args(fs, q)));
        tags.push(format!("\"{v}\" -> {v}({})", json_args(fs, "o!!", q)));
    }
    let _ = writeln!(
        out,
        "\n    companion object {{\n        fun read(r: Reader): {me} =\n            when (val v = r.variant()) {{"
    );
    for a in arms {
        let _ = writeln!(out, "                {a}");
    }
    let _ = writeln!(
        out,
        "                else -> throw IllegalArgumentException(\"{n}: no variant $v\")\n            }}\n\n        fun of(j: Any?): {me} =\n            J.sum(j) {{ t, o ->\n                when (t) {{"
    );
    for t in tags {
        let _ = writeln!(out, "                    {t}");
    }
    let _ = writeln!(
        out,
        "                    else -> throw IllegalArgumentException(\"{n}: no variant $t\")\n                }}\n            }}\n    }}\n}}"
    );
}

/// The Kotlin of a walked domain, in `package`.
pub fn write(w: &Walked, package: &str) -> String {
    let mut out = format!("// Generated by `arkdb gen kotlin` from the domain {}: its types and its functions by name, over\n// dev.arkdb.ffi. Never edit; regenerate.\npackage {package}\n\n", w.name);
    out += "import dev.arkdb.ffi.Id\nimport dev.arkdb.ffi.J\nimport dev.arkdb.ffi.Peer\nimport dev.arkdb.ffi.Reader\nimport dev.arkdb.ffi.View\nimport org.json.JSONObject\n";
    let q = Q { package, qualified: true };
    for (n, t) in &w.types {
        match t {
            Type::Record(_, fs) => record(&mut out, n, fs, q),
            Type::Sum(_, vs) => sum(&mut out, n, vs, q),
            _ => unreachable!("named"),
        }
    }
    let _ = writeln!(
        out,
        "\n/** The domain's functions, each called by its name through [peer]. */\nclass {}(val peer: Peer) {{",
        w.name
    );
    LANG.stubs(out, w)
}
