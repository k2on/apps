//! SwiftUI, composition only: a view struct reading its record from its
//! holder, each part a call to the ArkUI part of the same name
//! (`Part::call`), `ForEach`, `if`, `if let`, and actions sending messages.
//! Every part is hand-written in ui/swift.

use std::fmt::Debug;

use ark_app::shell::CommandMenu;
use ark_app::view::{Call, Given};

use crate::*;

/// The Swift file for `view`, whose record is called `root`.
pub fn emit<M: Debug>(view: &View<M>, s: &Screen) -> String {
    let root = s.root;
    let mut o = Out::default();
    for l in header("//", s).lines() {
        o.line(l);
    }
    o.line("import ArkUI");
    o.line("import SwiftUI");
    o.line("");
    o.block(format!("struct {}: View {{", s.name), "}", |o| {
        o.line(format!("@EnvironmentObject private var screen: {}", s.holder));
        o.line("");
        o.block("var body: some View {", "}", |o| {
            o.line(format!("let {root} = screen.page"));
            node(o, view);
        });
    });
    o.text()
}

fn node<M: Debug>(o: &mut Out, v: &View<M>) {
    match v {
        View::Empty => {}
        View::List(vs) => vs.iter().for_each(|v| node(o, v)),
        View::Each { list, item, body } => o.block(format!("ForEach({}, id: \\.self) {{ {item} in", path(list)), "}", |o| node(o, body)),
        View::When { cond, yes, no } => {
            o.block(format!("if {} {{", path(cond)), "}", |o| node(o, yes));
            if !matches!(**no, View::Empty) {
                o.reopen("} else {", |o| node(o, no));
            }
        }
        View::WhenSome { opt, item, yes, no } => {
            o.block(format!("if let {item} = {} {{", path(opt)), "}", |o| node(o, yes));
            if !matches!(**no, View::Empty) {
                o.reopen("} else {", |o| node(o, no));
            }
        }
        View::Part(p) => call(o, &p.call()),
    }
}

fn args<M: Debug>(c: &Call<'_, M>) -> Vec<String> {
    let mut args = vec![];
    if let Some(f) = &c.first {
        args.push(given(f));
    }
    args.extend(c.args.iter().map(|(k, g)| format!("{k}: {}", given(g))));
    args
}

/// `Name(first, label: value, …) { content } slot: { … }`.
fn call<M: Debug>(o: &mut Out, c: &Call<'_, M>) {
    let args = args(c);
    let head = match args.is_empty() && !c.slots.is_empty() {
        true => c.name.to_string(),
        false => format!("{}({})", c.name, args.join(", ")),
    };
    match c.slots.split_first() {
        None => o.line(head),
        Some(((_, content), rest)) => {
            o.block(format!("{head} {{"), "}", |o| node(o, content));
            for (name, v) in rest {
                o.reopen(&format!("}} {name}: {{"), |o| node(o, v));
            }
        }
    }
}

fn given<M: Debug>(g: &Given<'_, M>) -> String {
    match g {
        Given::Val(v) => value(v, "nil"),
        Given::Text(t) => quoted(t),
        Given::Act(m) => format!("{{ screen.send({}) }}", msg(&Msg::of(m))),
        Given::Typed(i) => format!("{{ screen.send({}) }}", msg(&Msg::typed(i))),
        Given::Switched(i) => format!("{{ screen.send({}) }}", msg(&Msg::switched(i))),
        Given::Counted(i) => format!("{{ screen.send({}) }}", msg(&Msg::counted(i))),
        Given::Items(cs) => {
            let items: Vec<String> = cs.iter().map(|c| format!("{}({})", c.name, args(c).join(", "))).collect();
            format!("[{}]", items.join(", "))
        }
        Given::Case { case, .. } => format!(".{case}"),
    }
}

/// The Swift file for an app's commands: a `Commands` whose menus are
/// ArkUI's `ArkCommands` (each where its [`MenuPlace`] puts it) and whose
/// items are `CommandButton`s sending to the shell's holder.
pub fn emit_commands<M: Debug>(menus: &[CommandMenu<M>], s: &Screen) -> String {
    let mut o = Out::default();
    for l in header("//", s).lines() {
        o.line(l);
    }
    o.line("import ArkUI");
    o.line("import SwiftUI");
    o.line("");
    o.block(format!("struct {}: Commands {{", s.name), "}", |o| {
        o.line(format!("let screen: {}", s.holder));
        o.line("");
        o.block("var body: some Commands {", "}", |o| {
            for m in menus {
                o.block(format!("ArkCommands(.{}) {{", m.place.case()), "}", |o| {
                    for c in &m.items {
                        let keys = match c.shortcut {
                            Some(k) => format!(", key: {}, shift: {}, option: {}", quoted(k.key), k.shift, k.option),
                            None => String::new(),
                        };
                        o.line(format!(
                            "CommandButton({}{keys}) {{ screen.send({}) }}",
                            quoted(c.title),
                            msg(&Msg::of(&c.run))
                        ));
                    }
                });
            }
        });
    });
    o.text()
}

/// A message as UniFFI's Swift enum spells it: `.openAccount(id: a.id)`.
pub fn msg(m: &Msg) -> String {
    let case = {
        let mut c = m.case.chars();
        c.next().map(|f| f.to_lowercase().chain(c).collect::<String>()).unwrap_or_default()
    };
    if m.fields.is_empty() {
        return format!(".{case}");
    }
    let args: Vec<String> = m.fields.iter().map(|(k, v)| format!("{}: {}", camel(k), arg(v, INPUT, "nil"))).collect();
    format!(".{case}({})", args.join(", "))
}
