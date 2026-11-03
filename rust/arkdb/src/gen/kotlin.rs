//! A domain in Kotlin: a data class per record and row, a sealed class per
//! sum, each read from the JSON tree Android's `org.json` parses (`of`) and
//! written as the kernel's JSON (`json`); the stubs over `dev.arkdb.ffi`.
//! Of a type UniFFI declares, its JSON as extensions, and its views typed.

use std::fmt::Write;

use ark::row::Type;

use super::{Hole, Lang, Walked};

/// How a named type is spelt: bare, or qualified by its package (where a
/// variant's name could hide it) — the generated package's, or UniFFI's
/// for a type it declares.
#[derive(Clone, Copy)]
struct Q<'a> {
    w: &'a Walked,
    package: &'a str,
    qualified: bool,
}

impl Q<'_> {
    fn name(&self, n: &str) -> String {
        match (self.qualified, self.w.typed.contains(n)) {
            (false, _) => n.into(),
            (true, true) => format!("{}.{n}", self.w.typed_package),
            (true, false) => format!("{}.{n}", self.package),
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
    read: |n, e| format!("{n}.of({e})"),
    write: |e| format!("{e}.json()"),
    param: |t| format!("input: {t}"),
    mutator: |Hole { m, p, name, arg, .. }| format!("/** `{name}`: its intent's id. */\n    fun {m}({p}): String = peer.mutate(\"{name}\", {arg})"),
    live: |Hole { m, p, name, arg, .. }| format!("/** `{name}`, on the live space. */\n    fun {m}({p}) = peer.call(\"{name}\", {arg})"),
    typed_view: |Hole { m, p, name, arg, r, .. }| {
        format!("/** `{name}`, kept, its rows UniFFI's. */\n    fun {m}({p}): View<{r}> = typed({r}View(peer, \"{name}\", {arg}))")
    },
    view: |Hole { m, p, name, arg, t, rd, .. }| {
        format!("/** `{name}`, kept. */\n    fun {m}({p}): View<{t}> = View(peer.view(\"{name}\", {arg})) {{ j -> {rd} }}")
    },
    query: |Hole { m, p, name, t, rd, .. }| format!("/** `{name}`. */\n    fun {m}({p}): {t} = {rd}"),
    query_call: |name, arg| format!("J.parse(peer.query(\"{name}\", {arg}))"),
    typed: |r| format!("private fun typed(v: {r}View): View<{r}> = View.typed(v::rows, {{ v.take().let {{ it.update to it.rows }} }}, v::close)"),
    live_table: |table, row, m| {
        format!("/** The live table `{table}`, as this device holds it. */\n    fun {m}(): List<{row}> = peer.liveRows(\"{table}\").map {{ {row}.of(J.parse(it)) }}")
    },
};

fn ty(t: &Type, q: Q) -> String {
    LANG.ty(t, &|n| q.name(n))
}

fn rd(t: &Type, e: &str, q: Q, d: usize) -> String {
    LANG.rd(t, e, &|n| q.name(n), d)
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

fn args(fields: &[(String, Type)], o: &str, q: Q) -> String {
    fields
        .iter()
        .map(|(n, t)| rd(t, &format!("{o}.opt(\"{n}\")"), q, 0))
        .collect::<Vec<_>>()
        .join(", ")
}

fn record(out: &mut String, n: &str, fields: &[(String, Type)], q: Q) {
    if q.w.typed.contains(n) {
        let _ = writeln!(out, "\n/** The record `{n}` (UniFFI's): its JSON, for the functions called by name. */");
        let _ = writeln!(out, "fun {n}.json(): String = {}\n", object(fields));
        let of = args(fields, "o", q.bare());
        let _ = writeln!(out, "fun {n}.Companion.of(j: Any?): {n} = (j as JSONObject).let {{ o -> {n}({of}) }}");
        return;
    }
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
    let _ = writeln!(
        out,
        "        fun of(j: Any?): {n} = (j as JSONObject).let {{ o -> {n}({}) }}\n    }}\n}}",
        args(fields, "o", q.bare())
    );
}

/// A variant as UniFFI's Kotlin names an `enum class` entry: `LastMonth`
/// is `LAST_MONTH`.
fn shouty(v: &str) -> String {
    let mut s = String::new();
    let mut prev_lower = false;
    for c in v.chars() {
        if c.is_uppercase() && prev_lower {
            s.push('_');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        s.extend(c.to_uppercase());
    }
    s
}

/// A sum UniFFI declares: its JSON, as extensions. One whose variants carry
/// nothing is a Kotlin `enum class` there, its entries shouted; any other a
/// sealed class with a class per variant.
fn sum_json(out: &mut String, n: &str, variants: &[(String, Vec<(String, Type)>)], q: Q) {
    if variants.iter().all(|(_, fs)| fs.is_empty()) {
        let _ = writeln!(out, "\n/** The sum `{n}` (UniFFI's enum): its JSON, for the functions called by name. */");
        let _ = writeln!(out, "fun {n}.json(): String =\n    when (this) {{");
        for (v, _) in variants {
            let _ = writeln!(out, "        {n}.{} -> \"\\\"{v}\\\"\"", shouty(v));
        }
        let _ = writeln!(out, "    }}\n\nfun {n}.Companion.of(j: Any?): {n} = J.sum(j) {{ t, _ ->\n    when (t) {{");
        for (v, _) in variants {
            let _ = writeln!(out, "        \"{v}\" -> {n}.{}", shouty(v));
        }
        let _ = writeln!(out, "        else -> throw IllegalArgumentException(\"{n}: no variant $t\")\n    }}\n}}");
        return;
    }
    let _ = writeln!(out, "\n/** The sum `{n}` (UniFFI's): its JSON, for the functions called by name. */");
    let _ = writeln!(out, "fun {n}.json(): String =\n    when (val v = this) {{");
    for (v, fs) in variants {
        match fs.is_empty() {
            true => drop(writeln!(out, "        is {n}.{v} -> \"\\\"{v}\\\"\"")),
            false => drop(writeln!(
                out,
                "        is {n}.{v} -> with(v) {{ \"{{\\\"{v}\\\":\" + {} + \"}}\" }}",
                object(fs)
            )),
        }
    }
    let _ = writeln!(out, "    }}\n\nfun {n}.Companion.of(j: Any?): {n} = J.sum(j) {{ t, o ->\n    when (t) {{");
    for (v, fs) in variants {
        match fs.is_empty() {
            true => drop(writeln!(out, "        \"{v}\" -> {n}.{v}")),
            false => drop(writeln!(out, "        \"{v}\" -> {n}.{v}({})", args(fs, "o!!", q))),
        }
    }
    let _ = writeln!(out, "        else -> throw IllegalArgumentException(\"{n}: no variant $t\")\n    }}\n}}");
}

fn sum(out: &mut String, n: &str, variants: &[(String, Vec<(String, Type)>)], q: Q) {
    if q.w.typed.contains(n) {
        return sum_json(out, n, variants, q);
    }
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
    let mut out = format!("// Generated by `arkdb gen kotlin` from the domain {}: its types and its functions by name, over\n// dev.arkdb.ffi. Never edit; regenerate.\npackage {package}\n\n", w.name);
    out += "import dev.arkdb.ffi.Id\nimport dev.arkdb.ffi.J\nimport dev.arkdb.ffi.Peer\nimport dev.arkdb.ffi.View\nimport org.json.JSONObject\n";
    let views = w.typed_views();
    for n in &w.typed {
        let _ = writeln!(out, "import {}.{n}", w.typed_package);
    }
    for n in &views {
        let _ = writeln!(out, "import {}.{n}View", w.typed_package);
    }
    let q = Q { w, package, qualified: true };
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
