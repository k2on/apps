//! SwiftUI, composition only: a view struct reading its record from its
//! holder, each part a call to the ArkUI part of the same name
//! (`Part::call`), `ForEach`, `if`, `if let`, and actions sending messages.
//! Every part is hand-written in ui/swift.

use std::fmt::Debug;

use ark_app::shell::CommandMenu;
use ark_app::view::{Align, Axis, Call, Corner, Edges, Given, Ink, Layout, Modifier, Prim, Space, Surface, Val};

use crate::*;

/// The Swift file for `view`, whose record is called `root`.
pub fn emit<M: Debug>(view: &View<M>, s: &Screen) -> String {
    let root = s.root;
    let mut o = Out {
        screen: s.screen,
        ..Out::default()
    };
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
            node(o, view, Ctx::Row);
        });
    });
    o.text()
}

/// Where a view is written: straight into a part's slot (a row of a
/// `List` on the iPhone, whose insets a primitive replaces with its own
/// padding), or inside a primitive's stack along an axis.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Row,
    In(Axis),
}

fn node<M: Debug>(o: &mut Out, v: &View<M>, ctx: Ctx) {
    match v {
        View::Empty => {}
        View::List(vs) => vs.iter().for_each(|v| node(o, v, ctx)),
        View::Each { list, item, body } => o.block(format!("ForEach({}, id: \\.self) {{ {item} in", path(list)), "}", |o| node(o, body, ctx)),
        View::When { cond, yes, no } => {
            o.block(format!("if {} {{", path(cond)), "}", |o| node(o, yes, ctx));
            if !matches!(**no, View::Empty) {
                o.reopen("} else {", |o| node(o, no, ctx));
            }
        }
        View::WhenSome { opt, item, yes, no } => {
            o.block(format!("if let {item} = {} {{", path(opt)), "}", |o| node(o, yes, ctx));
            if !matches!(**no, View::Empty) {
                o.reopen("} else {", |o| node(o, no, ctx));
            }
        }
        // A page's sheets, dialogs and alerts are not rows of its list: a
        // row off the screen is never made, and a question in one never
        // came up (app-screens 43429292874: Auto-Assign's dialog, the last
        // row of a long budget, held the iPhone's walk for a minute). They
        // go behind the list, always there.
        View::Part(p @ Part::Page { content, .. }) if !overs(content).1.is_empty() => {
            let (body, over) = overs(content);
            let c = p.call();
            let args = args(&c);
            o.block(format!("{}({}) {{", c.name, args.join(", ")), "}", |o| {
                body.into_iter().for_each(|v| node(o, v, Ctx::Row))
            });
            for (name, v) in c.slots.iter().skip(1) {
                o.reopen(&format!("}} {name}: {{"), |o| node(o, v, Ctx::Row));
            }
            o.indent += 1;
            o.block(".background {", "}", |o| over.into_iter().for_each(|v| node(o, v, Ctx::Row)));
            o.indent -= 1;
        }
        View::Part(p) => {
            call(o, &p.call());
            // Its walk id, which nothing reads aloud.
            if let Some(id) = walk_id(o.screen, p, "ArkId.slug") {
                o.indent += 1;
                o.line(format!(".accessibilityIdentifier({id})"));
                o.indent -= 1;
            }
        }
        View::Layout(l) => layout(o, l, ctx),
    }
}

/// A page's content, its lists opened out: what is laid out in its list,
/// and what comes up over it — a sheet, a prompt, a confirmation dialog,
/// an alert, or a `for`/`if` of nothing else.
fn overs<M>(v: &View<M>) -> (Vec<&View<M>>, Vec<&View<M>>) {
    fn over<M>(v: &View<M>) -> bool {
        match v {
            View::Part(Part::Sheet { .. } | Part::Prompt { .. } | Part::Confirm { .. } | Part::Alert { .. }) => true,
            View::List(vs) => !vs.is_empty() && vs.iter().all(over),
            View::When { yes, no, .. } | View::WhenSome { yes, no, .. } => over(yes) && (matches!(**no, View::Empty) || over(no)),
            View::Each { body, .. } => over(body),
            _ => false,
        }
    }
    fn walk<'a, M>(v: &'a View<M>, body: &mut Vec<&'a View<M>>, out: &mut Vec<&'a View<M>>) {
        match v {
            View::List(vs) if !over(v) => vs.iter().for_each(|v| walk(v, body, out)),
            View::Empty => {}
            v if over(v) => out.push(v),
            v => body.push(v),
        }
    }
    let (mut body, mut out) = (vec![], vec![]);
    walk(v, &mut body, &mut out);
    (body, out)
}

// * The primitives (ui/docs/primitives.md): SwiftUI's own layout

/// `l` as SwiftUI's stack, spacer, scroll view, text, glyph or image, its
/// modifiers after it in their order.
fn layout<M: Debug>(o: &mut Out, l: &Layout<M>, ctx: Ctx) {
    // Words that are nothing while empty: a literal decides now, a path
    // when drawn.
    if let Prim::Text(w) = &l.prim {
        if w.if_any {
            match &w.text {
                Val::Is(t) if t.is_empty() => return,
                Val::At(p) => return o.block(format!("if !{}.isEmpty {{", path(p)), "}", |o| prim(o, l, ctx)),
                Val::Is(_) => {}
            }
        }
    }
    prim(o, l, ctx)
}

fn prim<M: Debug>(o: &mut Out, l: &Layout<M>, ctx: Ctx) {
    match &l.prim {
        Prim::Stack {
            axis,
            align,
            spacing,
            content,
        } => {
            let spacing = spacing.map(space).unwrap_or_else(|| "0".into());
            let head = match axis {
                Axis::Vertical => format!("VStack(alignment: {}, spacing: {spacing}) {{", horizontal(*align)),
                Axis::Horizontal => format!("HStack(alignment: {}, spacing: {spacing}) {{", vertical(*align)),
                Axis::Depth => format!("ZStack(alignment: {}) {{", horizontal(*align)),
            };
            o.block(head, "}", |o| node(o, content, Ctx::In(*axis)));
        }
        Prim::Spacer { min } => o.line(format!("Spacer(minLength: {})", min.map(space).unwrap_or_else(|| "0".into()))),
        Prim::Scroll { axis, content } => {
            let head = match axis {
                Axis::Horizontal => "ScrollView(.horizontal, showsIndicators: false) {",
                _ => "ScrollView {",
            };
            let along = if *axis == Axis::Horizontal { Axis::Horizontal } else { Axis::Vertical };
            // A scroll view lays its content out as a stack along it.
            let stack = if along == Axis::Horizontal {
                "HStack(spacing: 0) {"
            } else {
                "VStack(spacing: 0) {"
            };
            o.block(head, "}", |o| o.block(stack, "}", |o| node(o, content, Ctx::In(along))));
        }
        Prim::Text(w) => {
            o.line(format!("Text({})", value(&lit(&w.text), "nil")));
            o.indent += 1;
            o.line(format!(".arkType(.{})", w.style.case()));
            o.line(format!(".foregroundStyle({})", ink(&w.ink)));
            if let Some(n) = w.lines {
                o.line(format!(".lineLimit({n})"));
            }
            if w.centred {
                o.line(".multilineTextAlignment(.center)");
            }
            o.indent -= 1;
        }
        Prim::Glyph { name, style, ink: i } => {
            o.line(format!("Image(systemName: ArkGlyphs.sf({}))", value(&lit(name), "nil")));
            o.indent += 1;
            o.line(format!(".arkType(.{})", style.case()));
            o.line(format!(".foregroundStyle({})", ink(i)));
            // Decorative: the words beside it say what it is.
            o.line(".accessibilityHidden(true)");
            o.indent -= 1;
        }
        Prim::Image { url, label } => o.line(format!(
            "Photo({}, label: {}, zoom: false)",
            value(&lit(url), "nil"),
            value(&lit(label), "nil")
        )),
    }
    o.indent += 1;
    for m in &l.mods {
        o.line(modifier(m));
    }
    // A row of the iPhone's `List`: its own padding is its only inset, as
    // Compose's `GroupCard` gives none.
    if ctx == Ctx::Row && !matches!(l.prim, Prim::Spacer { .. }) {
        o.line(".listRowInsets(EdgeInsets())");
    }
    o.indent -= 1;
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
        Align::Start => ".leading",
        Align::Center => ".center",
        Align::End => ".trailing",
    }
}

fn vertical(a: Align) -> &'static str {
    match a {
        Align::Start => ".top",
        Align::Center => ".center",
        Align::End => ".bottom",
    }
}

fn ink(i: &Ink) -> String {
    match i {
        Ink::Ink => "Color.ink".into(),
        Ink::Quiet => "Color.quiet".into(),
        Ink::Accent => "Color.accentText".into(),
        Ink::Tint(t) => format!("Tint(named: {}).color", value(&lit(t), "nil")),
        Ink::Figure(t) => format!("Tint(named: {}).figure", value(&lit(t), "nil")),
    }
}

fn modifier(m: &Modifier) -> String {
    match m {
        Modifier::Padding(Edges::All, s) => format!(".padding({})", space(*s)),
        Modifier::Padding(e, s) => {
            let edges = match e {
                Edges::Horizontal => ".horizontal",
                Edges::Vertical => ".vertical",
                Edges::Top => ".top",
                Edges::Bottom => ".bottom",
                Edges::Leading => ".leading",
                Edges::Trailing => ".trailing",
                Edges::All => unreachable!(),
            };
            format!(".padding({edges}, {})", space(*s))
        }
        Modifier::FillWidth(a) => format!(".frame(maxWidth: .infinity, alignment: {})", horizontal(*a)),
        Modifier::FillHeight => ".frame(maxHeight: .infinity)".into(),
        Modifier::Background(Surface::Card) => ".background(ArkSurface.card)".into(),
        Modifier::Background(Surface::Raised) => ".background(ArkSurface.raised)".into(),
        Modifier::Corner(c) => format!(
            ".clipShape(RoundedRectangle(cornerRadius: {}, style: .continuous))",
            match c {
                Corner::Card => "ArkCorner.card",
                Corner::Small => "ArkCorner.small",
            }
        ),
        Modifier::Combine => ".accessibilityElement(children: .combine)".into(),
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
            o.block(format!("{head} {{"), "}", |o| node(o, content, Ctx::Row));
            for (name, v) in rest {
                o.reopen(&format!("}} {name}: {{"), |o| node(o, v, Ctx::Row));
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
