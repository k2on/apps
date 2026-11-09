//! Compose, composition only: a `@Composable` reading its record from its
//! holder, each part a call to the ArkUI part of the same name
//! (`Part::call`) with named arguments, `for`, `if`, `?.let`, and actions
//! sending messages. Every part is hand-written in ui/kotlin.

use std::fmt::Debug;

use ark_app::view::{Call, Given};

use crate::*;

/// The Kotlin file for `view`, whose record is called `root`.
pub fn emit<M: Debug>(view: &View<M>, s: &Screen) -> String {
    let root = s.root;
    let ty = std::any::type_name::<M>().rsplit("::").next().unwrap_or("Msg").to_string();
    let mut o = Out::default();
    for l in header("//", s).lines() {
        o.line(l);
    }
    o.line(format!("package {}", s.package));
    o.line("");
    let mut imports: Vec<String> = [
        "androidx.compose.runtime.Composable",
        "androidx.compose.runtime.getValue",
        "androidx.lifecycle.compose.collectAsStateWithLifecycle",
        "dev.arkui.*",
    ]
    .iter()
    .map(|i| i.to_string())
    .collect();
    imports.extend(s.imports.iter().map(|i| i.to_string()));
    imports.sort();
    for i in imports {
        o.line(format!("import {i}"));
    }
    o.line("");
    o.line("@Composable");
    o.block(format!("fun {}(screen: {}) {{", s.name, s.holder), "}", |o| {
        o.line(format!("val {root} by screen.page.collectAsStateWithLifecycle()"));
        node(o, view, &ty);
    });
    o.text()
}

fn node<M: Debug>(o: &mut Out, v: &View<M>, ty: &str) {
    match v {
        View::Empty => {}
        View::List(vs) => vs.iter().for_each(|v| node(o, v, ty)),
        View::Each { list, item, body } => o.block(format!("for ({item} in {}) {{", path(list)), "}", |o| node(o, body, ty)),
        View::When { cond, yes, no } => {
            o.block(format!("if ({}) {{", path(cond)), "}", |o| node(o, yes, ty));
            if !matches!(**no, View::Empty) {
                o.reopen("} else {", |o| node(o, no, ty));
            }
        }
        View::WhenSome { opt, item, yes, no } => {
            o.block(format!("{}?.let {{ {item} ->", path(opt)), "}", |o| node(o, yes, ty));
            if !matches!(**no, View::Empty) {
                o.block(format!("if ({} == null) {{", path(opt)), "}", |o| node(o, no, ty));
            }
        }
        View::Part(p) => call(o, &p.call(), ty),
    }
}

/// `Name(first, label = value, …, slot = { … }) { content }`.
fn call<M: Debug>(o: &mut Out, c: &Call<'_, M>, ty: &str) {
    let mut args = vec![];
    if let Some(f) = &c.first {
        args.push(given(f, ty));
    }
    args.extend(c.args.iter().map(|(k, g)| format!("{k} = {}", given(g, ty))));
    match c.slots.split_first() {
        None => o.line(format!("{}({})", c.name, args.join(", "))),
        Some(((_, content), rest)) => {
            let head = match args.is_empty() && rest.is_empty() {
                true => c.name.to_string(),
                false => format!("{}({}", c.name, args.join(", ")),
            };
            if rest.is_empty() {
                let open = if args.is_empty() { format!("{head} {{") } else { format!("{head}) {{") };
                o.block(open, "}", |o| node(o, content, ty));
            } else {
                o.line(format!("{head},"));
                for (name, v) in rest {
                    o.indent += 1;
                    o.block(format!("{name} = {{"), "},", |o| node(o, v, ty));
                    o.indent -= 1;
                }
                o.block(") {", "}", |o| node(o, content, ty));
            }
        }
    }
}

fn given<M: Debug>(g: &Given<'_, M>, ty: &str) -> String {
    match g {
        Given::Val(v) => value(v, "null"),
        Given::Text(t) => quoted(t),
        Given::Act(m) => format!("{{ screen.send({}) }}", msg(&Msg::of(m), ty)),
        Given::Typed(i) => format!("{{ screen.send({}) }}", msg(&Msg::typed(i), ty)),
        Given::Switched(i) => format!("{{ screen.send({}) }}", msg(&Msg::switched(i), ty)),
        Given::Items(cs) => {
            let items: Vec<String> = cs
                .iter()
                .map(|c| {
                    let mut args = vec![];
                    if let Some(f) = &c.first {
                        args.push(given(f, ty));
                    }
                    args.extend(c.args.iter().map(|(k, g)| format!("{k} = {}", given(g, ty))));
                    format!("{}({})", c.name, args.join(", "))
                })
                .collect();
            format!("listOf({})", items.join(", "))
        }
    }
}

/// A message as UniFFI's Kotlin sealed class spells it: `AccountsMsg.OpenAccount(id = a.id)`.
pub fn msg(m: &Msg, ty: &str) -> String {
    if m.fields.is_empty() {
        return format!("{ty}.{}", m.case);
    }
    let args: Vec<String> = m.fields.iter().map(|(k, v)| format!("{} = {}", camel(k), arg(v, "it", "null"))).collect();
    format!("{ty}.{}({})", m.case, args.join(", "))
}
