//! Compose, composition only: a `@Composable` reading its record from its
//! holder, each part a call to the ArkUI part of the same name
//! (`Part::call`) with named arguments, `for`, `if`, `?.let`, and actions
//! sending messages. Every part is hand-written in ui/kotlin.

use std::fmt::Debug;

use ark_app::shell::CommandMenu;
use ark_app::view::{Align, Axis, Call, Corner, Edges, Given, Ink, Layout, Modifier, Prim, Space, Surface, Val};

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
    if has_layout(view) {
        imports.extend(LAYOUT_IMPORTS.iter().map(|i| i.to_string()));
    }
    imports.sort();
    imports.dedup();
    for i in imports {
        o.line(format!("import {i}"));
    }
    o.line("");
    o.line("@Composable");
    o.block(format!("fun {}(screen: {}) {{", s.name, s.holder), "}", |o| {
        o.line(format!("val {root} by screen.page.collectAsStateWithLifecycle()"));
        node(o, view, &ty, None);
    });
    o.text()
}

/// What a file that writes primitives out uses of Compose's own layout.
const LAYOUT_IMPORTS: &[&str] = &[
    "androidx.compose.foundation.background",
    "androidx.compose.foundation.horizontalScroll",
    "androidx.compose.foundation.layout.Arrangement",
    "androidx.compose.foundation.layout.Box",
    "androidx.compose.foundation.layout.Column",
    "androidx.compose.foundation.layout.Row",
    "androidx.compose.foundation.layout.Spacer",
    "androidx.compose.foundation.layout.fillMaxHeight",
    "androidx.compose.foundation.layout.fillMaxWidth",
    "androidx.compose.foundation.layout.height",
    "androidx.compose.foundation.layout.heightIn",
    "androidx.compose.foundation.layout.padding",
    "androidx.compose.foundation.layout.widthIn",
    "androidx.compose.foundation.layout.wrapContentWidth",
    "androidx.compose.foundation.rememberScrollState",
    "androidx.compose.foundation.shape.RoundedCornerShape",
    "androidx.compose.foundation.verticalScroll",
    "androidx.compose.material3.Text",
    "androidx.compose.ui.Alignment",
    "androidx.compose.ui.Modifier",
    "androidx.compose.ui.draw.clip",
    "androidx.compose.ui.semantics.semantics",
    "androidx.compose.ui.text.style.TextAlign",
    "androidx.compose.ui.text.style.TextOverflow",
];

/// Whether `v` holds a primitive anywhere.
fn has_layout<M>(v: &View<M>) -> bool {
    match v {
        View::Empty => false,
        View::List(vs) => vs.iter().any(has_layout),
        View::Each { body, .. } => has_layout(body),
        View::When { yes, no, .. } | View::WhenSome { yes, no, .. } => has_layout(yes) || has_layout(no),
        View::Part(p) => p.call().slots.iter().any(|(_, s)| has_layout(s)),
        View::Layout(_) => true,
    }
}

/// `v`, written in a part's slot (`axis` none) or in a primitive's stack
/// along `axis` (where a spacer takes its weight).
fn node<M: Debug>(o: &mut Out, v: &View<M>, ty: &str, axis: Option<Axis>) {
    match v {
        View::Empty => {}
        View::List(vs) => vs.iter().for_each(|v| node(o, v, ty, axis)),
        View::Each { list, item, body } => o.block(format!("for ({item} in {}) {{", path(list)), "}", |o| node(o, body, ty, axis)),
        View::When { cond, yes, no } => {
            o.block(format!("if ({}) {{", path(cond)), "}", |o| node(o, yes, ty, axis));
            if !matches!(**no, View::Empty) {
                o.reopen("} else {", |o| node(o, no, ty, axis));
            }
        }
        View::WhenSome { opt, item, yes, no } => {
            o.block(format!("{}?.let {{ {item} ->", path(opt)), "}", |o| node(o, yes, ty, axis));
            if !matches!(**no, View::Empty) {
                o.block(format!("if ({} == null) {{", path(opt)), "}", |o| node(o, no, ty, axis));
            }
        }
        View::Part(p) => call(o, &p.call(), ty),
        View::Layout(l) => layout(o, l, ty, axis),
    }
}

// * The primitives (ui/docs/primitives.md): Compose's own layout

/// `l` as Compose's `Column`, `Row`, `Box`, `Spacer`, a scrolling column,
/// `Text`, a glyph or a photo; its modifiers a `Modifier` chain, outermost
/// first (SwiftUI's order reversed).
fn layout<M: Debug>(o: &mut Out, l: &Layout<M>, ty: &str, axis: Option<Axis>) {
    if let Prim::Text(w) = &l.prim {
        if w.if_any {
            match &w.text {
                Val::Is(t) if t.is_empty() => return,
                Val::At(p) => return o.block(format!("if ({}.isNotEmpty()) {{", path(p)), "}", |o| prim(o, l, ty, axis)),
                Val::Is(_) => {}
            }
        }
    }
    prim(o, l, ty, axis)
}

fn prim<M: Debug>(o: &mut Out, l: &Layout<M>, ty: &str, axis: Option<Axis>) {
    let chain = chain(&l.mods);
    let with = |args: &mut Vec<String>| {
        if let Some(c) = &chain {
            args.insert(0, format!("modifier = {c}"));
        }
    };
    match &l.prim {
        Prim::Stack {
            axis: along,
            align,
            spacing,
            content,
        } => {
            let mut args = vec![];
            with(&mut args);
            let (name, arrangement, alignment) = match along {
                Axis::Vertical => ("Column", "verticalArrangement", ("horizontalAlignment", horizontal(*align))),
                Axis::Horizontal => ("Row", "horizontalArrangement", ("verticalAlignment", vertical(*align))),
                Axis::Depth => ("Box", "", ("contentAlignment", depth(*align))),
            };
            if let (Some(s), false) = (spacing, arrangement.is_empty()) {
                args.push(format!("{arrangement} = Arrangement.spacedBy({})", space(*s)));
            }
            args.push(format!("{} = {}", alignment.0, alignment.1));
            o.block(format!("{name}({}) {{", args.join(", ")), "}", |o| node(o, content, ty, Some(*along)));
        }
        Prim::Spacer { min } => {
            // What is left: a weight, in a row's or a column's scope only;
            // at least `min` while there is room (Compose measures a weight
            // after the rest, so unlike SwiftUI it squeezes nothing for it).
            let least = |along: &str| min.map(|m| format!(".{along}In(min = {})", space(m))).unwrap_or_default();
            match axis {
                Some(Axis::Horizontal) => o.line(format!("Spacer(Modifier.weight(1f){})", least("width"))),
                Some(Axis::Vertical) => o.line(format!("Spacer(Modifier.weight(1f){})", least("height"))),
                _ => {
                    if let Some(m) = min {
                        o.line(format!("Spacer(Modifier.height({}))", space(*m)));
                    }
                }
            }
        }
        Prim::Scroll { axis: along, content } => {
            let (name, scroll, inner) = match along {
                Axis::Horizontal => ("Row", "horizontalScroll", Axis::Horizontal),
                _ => ("Column", "verticalScroll", Axis::Vertical),
            };
            let m = match &chain {
                Some(c) => format!("{c}.{scroll}(rememberScrollState())"),
                None => format!("Modifier.{scroll}(rememberScrollState())"),
            };
            o.block(format!("{name}({m}) {{"), "}", |o| node(o, content, ty, Some(inner)));
        }
        Prim::Text(w) => {
            let mut args = vec![
                value(&lit(&w.text), "null"),
                format!("style = ArkType.{}.style", pascal(w.style.case())),
                format!("color = {}", ink(&w.ink)),
            ];
            if let Some(n) = w.lines {
                args.push(format!("maxLines = {n}"));
                args.push("overflow = TextOverflow.Ellipsis".into());
            }
            if w.centred {
                args.push("textAlign = TextAlign.Center".into());
            }
            if let Some(c) = &chain {
                args.push(format!("modifier = {c}"));
            }
            o.line(format!("Text({})", args.join(", ")));
        }
        Prim::Glyph { name, style, ink: i } => {
            let mut args = vec![
                value(&lit(name), "null"),
                format!("ArkType.{}", pascal(style.case())),
                format!("color = {}", ink(i)),
            ];
            if let Some(c) = &chain {
                args.push(format!("modifier = {c}"));
            }
            o.line(format!("ArkGlyph({})", args.join(", ")));
        }
        Prim::Image { url, label } => {
            let photo = format!(
                "Photo({}, label = {}, zoom = false)",
                value(&lit(url), "null"),
                value(&lit(label), "null")
            );
            match &chain {
                Some(c) => o.block(format!("Box({c}) {{"), "}", |o| o.line(photo)),
                None => o.line(photo),
            }
        }
    }
}

/// A value as the recording holds it, for [`value`].
fn lit<T: Debug>(v: &Val<T>) -> String {
    match v {
        Val::At(p) => p.clone(),
        Val::Is(t) => format!("{t:?}"),
    }
}

fn space(s: Space) -> String {
    format!("Space.{}", s.case())
}

fn horizontal(a: Align) -> &'static str {
    match a {
        Align::Start => "Alignment.Start",
        Align::Center => "Alignment.CenterHorizontally",
        Align::End => "Alignment.End",
    }
}

fn vertical(a: Align) -> &'static str {
    match a {
        Align::Start => "Alignment.Top",
        Align::Center => "Alignment.CenterVertically",
        Align::End => "Alignment.Bottom",
    }
}

fn depth(a: Align) -> &'static str {
    match a {
        Align::Start => "Alignment.CenterStart",
        Align::Center => "Alignment.Center",
        Align::End => "Alignment.CenterEnd",
    }
}

fn ink(i: &Ink) -> String {
    match i {
        Ink::Ink => "ArkInk.ink".into(),
        Ink::Quiet => "ArkInk.quiet".into(),
        Ink::Accent => "ArkInk.accent".into(),
        Ink::Tint(t) => format!("ArkInk.tint({})", value(&lit(t), "null")),
        Ink::Figure(t) => format!("ArkInk.figure({})", value(&lit(t), "null")),
    }
}

/// The modifiers as one `Modifier` chain, outermost first: SwiftUI wraps a
/// view in each of its modifiers in turn, so its last is Compose's first.
fn chain(mods: &[Modifier]) -> Option<String> {
    if mods.is_empty() {
        return None;
    }
    let links: Vec<String> = mods
        .iter()
        .rev()
        .map(|m| match m {
            Modifier::Padding(e, s) => {
                let s = space(*s);
                match e {
                    Edges::All => format!("padding({s})"),
                    Edges::Horizontal => format!("padding(horizontal = {s})"),
                    Edges::Vertical => format!("padding(vertical = {s})"),
                    Edges::Top => format!("padding(top = {s})"),
                    Edges::Bottom => format!("padding(bottom = {s})"),
                    Edges::Leading => format!("padding(start = {s})"),
                    Edges::Trailing => format!("padding(end = {s})"),
                }
            }
            Modifier::FillWidth(a) => format!("fillMaxWidth().wrapContentWidth({})", horizontal(*a)),
            Modifier::FillHeight => "fillMaxHeight()".into(),
            Modifier::Background(Surface::Card) => "background(ArkSurface.card)".into(),
            Modifier::Background(Surface::Raised) => "background(ArkSurface.raised)".into(),
            Modifier::Corner(Corner::Card) => "clip(RoundedCornerShape(ArkCorner.card))".into(),
            Modifier::Corner(Corner::Small) => "clip(RoundedCornerShape(ArkCorner.small))".into(),
            Modifier::Combine => "semantics(mergeDescendants = true) {}".into(),
        })
        .collect();
    Some(format!("Modifier.{}", links.join(".")))
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
                o.block(open, "}", |o| node(o, content, ty, None));
            } else {
                o.line(format!("{head},"));
                for (name, v) in rest {
                    o.indent += 1;
                    o.block(format!("{name} = {{"), "},", |o| node(o, v, ty, None));
                    o.indent -= 1;
                }
                o.block(") {", "}", |o| node(o, content, ty, None));
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
        Given::Counted(i) => format!("{{ screen.send({}) }}", msg(&Msg::counted(i), ty)),
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
        Given::Case { of, case } => format!("{of}.{}", pascal(case)),
    }
}

fn pascal(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The Kotlin file for an app's commands: a function of the shell's holder
/// giving ArkUI's `ShellMenu`s, each a list of `ShellCommand`s, which the
/// shell binds to the hardware keyboard and puts in the app bar's menu.
pub fn emit_commands<M: Debug>(menus: &[CommandMenu<M>], s: &Screen) -> String {
    let ty = std::any::type_name::<M>().rsplit("::").next().unwrap_or("Msg").to_string();
    let mut o = Out::default();
    for l in header("//", s).lines() {
        o.line(l);
    }
    o.line(format!("package {}", s.package));
    o.line("");
    let mut imports: Vec<String> = vec!["dev.arkui.*".into()];
    imports.extend(s.imports.iter().map(|i| i.to_string()));
    imports.sort();
    for i in imports {
        o.line(format!("import {i}"));
    }
    o.line("");
    o.block(format!("fun {}(screen: {}): List<ShellMenu> = listOf(", s.name, s.holder), ")", |o| {
        for m in menus {
            o.block(format!("ShellMenu(MenuPlace.{}, listOf(", pascal(m.place.case())), ")),", |o| {
                for c in &m.items {
                    let keys = match c.shortcut {
                        Some(k) => format!(", key = {}, shift = {}, option = {}", quoted(k.key), k.shift, k.option),
                        None => String::new(),
                    };
                    o.line(format!(
                        "ShellCommand({}{keys}) {{ screen.send({}) }},",
                        quoted(c.title),
                        msg(&Msg::of(&c.run), &ty)
                    ));
                }
            });
        }
    });
    o.text()
}

/// A message as UniFFI's Kotlin sealed class spells it: `AccountsMsg.OpenAccount(id = a.id)`.
pub fn msg(m: &Msg, ty: &str) -> String {
    if m.fields.is_empty() {
        return format!("{ty}.{}", m.case);
    }
    let args: Vec<String> = m.fields.iter().map(|(k, v)| format!("{} = {}", camel(k), arg(v, "it", "null"))).collect();
    format!("{ty}.{}({})", m.case, args.join(", "))
}
