//! SwiftUI, as the iPhone app's hand-written screens were: an inset
//! grouped `List` under the tab's `TitleSection`, ArkUI's parts in it, a
//! sheet and an alert as modifiers. The view reads its record from the
//! screen's holder (an `ObservableObject` in the environment, the app's
//! glue over `ark_app::Runtime`) and sends it messages.

use std::fmt::Debug;

use crate::*;

/// The Swift file for `view`.
pub fn emit<M: Debug>(view: &View<M>, s: &Screen) -> String {
    let View::Part(Part::Page { title, refresh, content }) = view else {
        panic!("a screen is a page")
    };
    let laid = lay(content);
    let mut o = Out::default();
    for l in header("//", s).lines() {
        o.line(l);
    }
    o.line("import ArkUI");
    o.line("import SwiftUI");
    o.line("");
    o.block(format!("struct {}: View {{", s.name), "}", |o| {
        o.line(format!("@EnvironmentObject private var screen: {}", s.holder));
        o.line("/// How tall the large title's row is: past it, the inline title comes into the bar.");
        o.line("@State private var fold: CGFloat = 0");
        o.line("");
        o.block("var body: some View {", "}", |o| {
            o.line("let p = screen.page");
            o.block("List {", "}", |o| {
                o.block(
                    format!("TitleSection({}, fold: $fold) {{ AccountButton() }} below: {{", quoted(title)),
                    "}",
                    |o| {
                        for t in &laid.top {
                            node(o, t, Ctx::List);
                        }
                    },
                );
                for b in &laid.body {
                    node(o, b, Ctx::List);
                }
            });
            o.line(".listStyle(.insetGrouped)");
            if let Some(r) = refresh {
                o.line(format!(".refreshable {{ screen.send({}) }}", msg(&Msg::of(r))));
            }
            o.line(format!(".largeTitleBar({}, fold: fold)", quoted(title)));
            for v in &laid.over {
                over(o, v);
            }
        });
    });
    o.text()
}

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    /// A `List`'s content: sections.
    List,
    /// A section's rows, or a form's fields.
    Rows,
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
    let args: Vec<String> = m.fields.iter().map(|(k, v)| format!("{}: {}", camel(k), arg(v))).collect();
    format!(".{case}({})", args.join(", "))
}

fn arg(a: &Arg) -> String {
    match a {
        Arg::Path(p) if p == INPUT => INPUT.into(),
        Arg::Path(p) => path(p),
        Arg::Text(t) => quoted(t),
        Arg::Number(n) => n.clone(),
        Arg::Bool(b) => b.to_string(),
        Arg::None => "nil".into(),
    }
}

fn send<M: Debug>(m: &M) -> String {
    format!("screen.send({})", msg(&Msg::of(m)))
}

fn glyph(name: &str) -> String {
    format!("ArkGlyphs.sf({})", quoted(name))
}

fn node<M: Debug>(o: &mut Out, v: &View<M>, ctx: Ctx) {
    match v {
        View::Empty => {}
        View::List(vs) => vs.iter().for_each(|v| node(o, v, ctx)),
        View::Each { list, item, body } => o.block(format!("ForEach({}, id: \\.self) {{ {item} in", path(list)), "}", |o| node(o, body, ctx)),
        View::When { cond, yes, no } => {
            o.block(format!("if {} {{", path(cond)), "}", |o| node(o, yes, ctx));
            if !matches!(**no, View::Empty) {
                o.block("else {", "}", |o| node(o, no, ctx));
            }
        }
        View::WhenSome { opt, item, yes, no } => {
            o.block(format!("if let {item} = {} {{", path(opt)), "}", |o| node(o, yes, ctx));
            if !matches!(**no, View::Empty) {
                o.block("else {", "}", |o| node(o, no, ctx));
            }
        }
        View::Part(p) => part(o, p, ctx),
    }
}

fn part<M: Debug>(o: &mut Out, p: &Part<M>, ctx: Ctx) {
    match p {
        Part::Hero { label, cents, note } => o.line(format!(
            "HeroFigure({}, cents: {}, tint: .neutral, note: {})",
            quoted(label),
            val(cents),
            quoted(note)
        )),
        Part::Section(s) => {
            o.block("Section {", "}", |o| node(o, &s.rows, Ctx::Rows));
            if let Some(t) = &s.title {
                o.reopen("} header: {", |o| {
                    o.block("HStack {", "}", |o| {
                        o.line(format!("Text({})", val(t)));
                        if let Some(a) = &s.amount {
                            o.line("Spacer()");
                            let fig = format!("CentsText({a}).foregroundStyle({a} < 0 ? Color.error : Color.quiet)", a = val(a));
                            o.line("// Below nothing in the error's red, as a group's total in the budget is.");
                            match &s.shows {
                                Some(c) => o.block(format!("if {} {{", val(c)), "}", |o| o.line(&fig)),
                                None => o.line(&fig),
                            }
                        }
                    });
                });
            }
            if let Some(f) = &s.footer {
                o.reopen("} footer: {", |o| {
                    o.block(format!("if let note = {} {{", val(f)), "}", |o| o.line("Text(note)"));
                });
            }
        }
        Part::Row(r) => row(o, r),
        Part::NameField {
            label,
            value,
            problem,
            typed,
        } => o.line(format!(
            "NameField(name: Binding(get: {{ {} }}, set: {{ screen.send({}) }}), problem: {}, label: {})",
            val(value),
            msg(&Msg::typed(typed)),
            val(problem),
            quoted(label)
        )),
        Part::AmountField { label, typed, problem, on } => {
            o.block("VStack(alignment: .leading, spacing: Space.xs) {", "}", |o| {
                o.line(format!(
                    "AmountField({}, text: Binding(get: {{ {} }}, set: {{ screen.send({}) }}))",
                    val(label),
                    val(typed),
                    msg(&Msg::typed(on))
                ));
                o.block(format!("if let problem = {} {{", val(problem)), "}", |o| {
                    o.line("Text(problem).font(.caption).foregroundStyle(Color.error)")
                });
            });
        }
        Part::Chips(c) => o.block("ScrollView(.horizontal, showsIndicators: false) {", "}", |o| {
            o.block("HStack(spacing: Space.s) {", "}", |o| node(o, c, Ctx::Rows));
        }),
        Part::Chip { label, selected, tap, .. } => o.line(format!("Chip({}, selected: {}) {{ {} }}", val(label), val(selected), send(tap))),
        Part::Toggle { title, note, on, change } => {
            o.block(
                format!(
                    "Toggle(isOn: Binding(get: {{ {} }}, set: {{ screen.send({}) }})) {{",
                    val(on),
                    msg(&Msg::switched(change))
                ),
                "}",
                |o| {
                    o.line(format!("Text({})", quoted(title)));
                    o.line(format!("Text({}).font(.caption).foregroundStyle(.quiet)", val(note)));
                },
            );
        }
        Part::Button { title, enabled, tap } => {
            o.line(format!("Button({}) {{ {} }}", quoted(title), send(tap)));
            o.line(format!("    .disabled(!{})", val(enabled)));
        }
        Part::Page { .. } | Part::Sheet { .. } | Part::Prompt { .. } => {
            let _ = ctx;
            panic!("a page, a sheet or a prompt is a screen's own, not inside another part")
        }
    }
}

fn row<M: Debug>(o: &mut Out, r: &ark_app::view::ListRow<M>) {
    let open = match &r.tap {
        Some(t) => format!("Button {{ {} }} label: {{", send(t)),
        None => "Group {".into(),
    };
    o.block(open, "}", |o| {
        o.block("HStack(spacing: Space.m) {", "}", |o| {
            match &r.picture {
                Picture::None => {}
                Picture::Tile(g) => o.line(format!("Tile(symbol: {}, color: .quiet)", glyph(g))),
                Picture::Mark { kind, .. } => o.line(format!("AccountMark(kind: {})", val(kind))),
                Picture::Glyph(g) => o.line(format!("Image(systemName: {}).foregroundStyle(Color.accent)", glyph(g))),
            }
            let title = format!("Text({}).lineLimit(1)", val(&r.title));
            match &r.subtitle {
                Some(s) => o.block("VStack(alignment: .leading, spacing: 2) {", "}", |o| {
                    o.line(&title);
                    o.line(format!("Text({}).font(.caption).foregroundStyle(.quiet).lineLimit(1)", val(s)));
                }),
                None => o.line(&title),
            }
            o.line("Spacer(minLength: Space.s)");
            if let Some(b) = &r.badge {
                o.block(format!("if let count = {} {{", val(b)), "}", |o| {
                    o.line("Badge(Int(count), said: \"\\(count) to review\")")
                });
            }
            if let Some(a) = &r.amount {
                let a = val(a);
                match r.quiet {
                    true => o.line(format!("CentsText({a}).foregroundStyle(Color.quiet)")),
                    false => o.line(format!("CentsText({a}).foregroundStyle({a} < 0 ? Color.error : Color.ink)")),
                }
            }
        });
        if r.said.is_some() {
            o.line(".accessibilityElement(children: .combine)");
        }
    });
    o.line(format!(".foregroundStyle({})", if r.accent { "Color.accent" } else { "Color.ink" }));
    if let Some(s) = &r.said {
        o.line(format!(".accessibilityLabel({})", val(s)));
    }
    if !r.menu.is_empty() {
        o.block(".contextMenu {", "}", |o| {
            for m in &r.menu {
                let role = match &m.destructive {
                    Val::Is(false) => String::new(),
                    d => format!("(role: {} ? .destructive : nil)", val(d)),
                };
                o.line(format!(
                    "Button{role} {{ {} }} label: {{ Label({}, systemImage: {}) }}",
                    send(&m.tap),
                    val(&m.title),
                    glyph(m.glyph)
                ));
            }
        });
    }
}

/// A sheet or a prompt, as the page's modifier, shown while its record is.
fn over<M: Debug>(o: &mut Out, v: &View<M>) {
    let View::WhenSome { opt, item, yes, .. } = v else { return };
    let opt = path(opt);
    match &**yes {
        View::Part(Part::Sheet { title, dismiss, content }) => {
            o.block(
                format!(
                    ".sheet(isPresented: Binding(get: {{ screen.page.{} != nil }}, set: {{ if !$0 {{ {} }} }})) {{",
                    opt.trim_start_matches("p."),
                    send(dismiss)
                ),
                "}",
                |o| {
                    o.block(format!("if let {item} = screen.page.{} {{", opt.trim_start_matches("p.")), "}", |o| {
                        o.block("NavigationStack {", "}", |o| {
                            o.block("Form {", "}", |o| node(o, content, Ctx::Rows));
                            o.line(format!(".navigationTitle({})", quoted(title)));
                            o.line(".navigationBarTitleDisplayMode(.inline)");
                        });
                    });
                },
            );
        }
        View::Part(Part::Prompt {
            title,
            text,
            typed,
            confirm,
            ok,
            cancel,
        }) => {
            o.block(
                format!(
                    ".alert({}, isPresented: Binding(get: {{ screen.page.{} != nil }}, set: {{ if !$0 {{ {} }} }})) {{",
                    quoted(title),
                    opt.trim_start_matches("p."),
                    send(cancel)
                ),
                "}",
                |o| {
                    o.block(format!("if let {item} = screen.page.{} {{", opt.trim_start_matches("p.")), "}", |o| {
                        o.line(format!(
                            "TextField(\"Name\", text: Binding(get: {{ {} }}, set: {{ screen.send({}) }}))",
                            val(text),
                            msg(&Msg::typed(typed))
                        ));
                    });
                    o.line(format!("Button(\"Cancel\", role: .cancel) {{ {} }}", send(cancel)));
                    o.line(format!("Button({}) {{ {} }}", quoted(confirm), send(ok)));
                },
            );
        }
        _ => {}
    }
}

impl Out {
    /// The last line's closing brace joined to what reopens it (`} header: {`),
    /// `body` in it, and closed again.
    fn reopen(&mut self, s: &str, body: impl FnOnce(&mut Out)) {
        let trimmed = self.text.trim_end_matches('\n').len();
        self.text.truncate(trimmed - 1);
        self.text.push_str(s);
        self.text.push('\n');
        self.indent += 1;
        body(self);
        self.indent -= 1;
        self.line("}");
    }
}
