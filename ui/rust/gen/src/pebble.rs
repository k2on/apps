//! Pebble, written at build time: a screen's layers and what each shows,
//! as no_std Rust over ark-pebble's toolkit (`ark_pebble::ui`), from what
//! a `#[view]` says when it is recorded — as SwiftUI and Compose are
//! written from it (ui/docs/pebble.md). The watch runs no view machinery:
//! it runs what this writes.
//!
//! What it writes, for a view `now_playing(p: &Face) -> View<Act>`:
//!
//! - `GLYPHS`: every glyph the view names, and those its paths may hold
//!   ([`Screen::glyphs`]), arkui's Lucide drawings flattened into line
//!   segments here, painted on the watch at the size they are drawn;
//! - `build(ui)`: each layer made once, at a frame worked out from the
//!   room the window gives and ui/theme.toml's tokens (`ark_pebble::tokens`,
//!   named, never a number): a text layer, a meter, a glyph's bitmap layer,
//!   a list (`MenuLayer`), a rectangle behind, and the action bar;
//! - `BINDINGS`: what each layer shows, as data: for each a source
//!   function of the screen's (the module [`Screen::sources`] names), by
//!   the path the view read, `p.title` the source `title`, a `for t in
//!   p.next`'s `t.title` the source `next_title`; and each `if`'s branches
//!   shown while its source holds. The toolkit reads them through the
//!   node, as PebbleOS draws, each layer's reads recorded against it, and
//!   draws a layer again when what it read changed: no code is written per
//!   layer, and no record exists between the node and the screen;
//! - `PRESS`: the message each of the action bar's buttons sends.
//!
//! What it lays out: a vertical stack's content one under another, what is
//! left shared by its spacers and lists; a horizontal one's glyphs at their
//! side and its words and spacers sharing the rest, words after a spacer at
//! the trailing end; a depth stack's content in one frame; padding;
//! a background. Words are one line of their style unless `.lines(n)`
//! says more, and keep their line while empty. A view it cannot lay out
//! (a scroll, a picture, a grid, a `for` outside a list, a part with no
//! Pebble twin) panics as it is written, saying what: the watch's build
//! fails, never its screen.

use std::collections::BTreeSet;
use std::fmt::Debug;

use ark_app::view::{Align, Axis, Edges, Ink, Layout, Modifier, Part, Prim, Section, Space, Surface, TextStyle, Val, View};

use crate::{Arg, Msg};

/// What the emitter needs to know of a watch's screen besides its view.
#[derive(Clone, Copy, Debug)]
pub struct Screen {
    /// The Rust file it is written from, for the header.
    pub source: &'static str,
    /// The sources' name in the view function (its parameter).
    pub root: &'static str,
    /// The module of the screen's source functions, as the generated code
    /// names it (`crate::sources`): one for each path the view reads.
    pub sources: &'static str,
    /// What the generated code brings into scope: the messages' path
    /// (`now_playing::sources::Act`).
    pub uses: &'static [&'static str],
    /// The app's accent (ui/theme.toml's `[accent.<name>]`).
    pub accent: &'static str,
    /// The glyphs a path the view reads may name (a play glyph that is
    /// `pause` while playing), painted beside those it names itself.
    pub glyphs: &'static [&'static str],
}

/// The Rust module for `view`.
pub fn emit<M: Debug>(view: &View<M>, s: &Screen) -> String {
    let mut e = Em {
        s,
        build: vec![],
        b: Tables::default(),
        items: vec![],
        press: vec![],
        next: 0,
        fills: 0,
        glyphs: s.glyphs.iter().map(|g| g.to_string()).collect(),
        ty: std::any::type_name::<M>().rsplit("::").next().unwrap_or("Msg").to_string(),
    };
    let room = Rect {
        x: "0".into(),
        y: "0".into(),
        w: "w".into(),
        h: "h".into(),
    };
    match view {
        View::Part(Part::ActionBar { items, content }) => {
            e.build.push("ui.action_bar(CONTAINER_HIGH, INK, ACTION_GLYPH);".into());
            e.build.push("let (w, h) = ui.room();".into());
            for i in items {
                let button = format!("Button::{}", pascal(i.button.case()));
                match &i.glyph {
                    Val::Is(g) => {
                        e.glyphs.insert(g.clone());
                        e.build.push(format!("ui.set_action({button}, {});", bytes_lit(g)));
                    }
                    Val::At(p) => {
                        let f = e.src(p);
                        e.b.actions.push(format!("({button}, {f})"));
                    }
                }
                e.press.push((button, e.msg(&i.tap)));
            }
            e.place(content, &room, Align::Start);
        }
        v => {
            e.build.push("let (w, h) = ui.room();".into());
            e.place(v, &room, Align::Start);
        }
    }
    e.text()
}

/// A frame as expressions of the room and the tokens.
#[derive(Clone, Debug)]
struct Rect {
    x: String,
    y: String,
    w: String,
    h: String,
}

impl Rect {
    fn frame(&self) -> String {
        format!("Frame {{ x: {}, y: {}, w: {}, h: {} }}", self.x, self.y, self.w, self.h)
    }

    /// Inset by `s` on `edges`.
    fn inset(&self, edges: Edges, s: &str) -> Rect {
        let (left, right, top, bottom) = match edges {
            Edges::All => (true, true, true, true),
            Edges::Horizontal => (true, true, false, false),
            Edges::Vertical => (false, false, true, true),
            Edges::Top => (false, false, true, false),
            Edges::Bottom => (false, false, false, true),
            Edges::Leading => (true, false, false, false),
            Edges::Trailing => (false, true, false, false),
        };
        let less = |v: &str, n: usize| (0..n).fold(v.to_string(), |v, _| minus(&v, s));
        Rect {
            x: if left { plus(&self.x, s) } else { self.x.clone() },
            y: if top { plus(&self.y, s) } else { self.y.clone() },
            w: less(&self.w, left as usize + right as usize),
            h: less(&self.h, top as usize + bottom as usize),
        }
    }
}

/// The binding tables, each entry as the generated code writes it.
#[derive(Default)]
struct Tables {
    words: Vec<String>,
    meters: Vec<String>,
    glyphs: Vec<String>,
    actions: Vec<String>,
    lists: Vec<String>,
    shown: Vec<String>,
}

struct Em<'s> {
    s: &'s Screen,
    build: Vec<String>,
    b: Tables,
    /// The `for`s' and `if let`s' items in scope, each with the source
    /// name of what it is an item of.
    items: Vec<(String, String)>,
    press: Vec<(String, String)>,
    next: u8,
    fills: usize,
    glyphs: BTreeSet<String>,
    ty: String,
}

impl Em<'_> {
    fn id(&mut self) -> u8 {
        let id = self.next;
        self.next = self.next.checked_add(1).expect("a Pebble screen of more than 255 layers");
        id
    }

    /// The source a path names: its root's fields (`p.title`, `title`),
    /// or an item's under the name of what it is an item of (`t.title` in
    /// `for t in p.next`, `next_title`).
    fn name(&self, path: &str) -> String {
        let (root, rest) = path.split_once('.').unwrap_or((path, ""));
        let base = if root == self.s.root {
            String::new()
        } else {
            match self.items.iter().rev().find(|(i, _)| i == root) {
                Some((_, of)) => of.clone(),
                None => panic!("Pebble binds paths from the sources, `{}`, or an item of theirs: {path}", self.s.root),
            }
        };
        let rest = rest.replace('.', "_");
        match (base.is_empty(), rest.is_empty()) {
            (true, _) => rest,
            (false, true) => base,
            (false, false) => format!("{base}_{rest}"),
        }
    }

    /// The source function a path names, as the generated code calls it.
    fn src(&self, path: &str) -> String {
        format!("{}::{}", self.s.sources, self.name(path))
    }

    /// Words: a source's, or a literal's.
    fn words(&self, v: &Val<String>) -> String {
        match v {
            Val::Is(t) => format!("|_, out| {{ out.s({}); }}", bytes_lit(t)),
            Val::At(p) => self.src(p),
        }
    }

    /// `v` laid out in `f`, its words at `align` across.
    fn place<M: Debug>(&mut self, v: &View<M>, f: &Rect, align: Align) {
        match v {
            View::Empty => {}
            View::List(_) => self.stack(&flat(v), Axis::Vertical, None, f, align),
            View::Layout(l) => self.layout(l, f, align),
            View::When { cond, yes, no } => self.branches(cond, None, yes, no, f, align),
            View::WhenSome { opt, item, yes, no } => self.branches(opt, Some(item), yes, no, f, align),
            View::Each { .. } => panic!("Pebble lays out a `for` only as a list's rows (a card_section of list_rows)"),
            View::Part(p) => self.part(p, f),
        }
    }

    /// An `if`'s two branches in one frame, each shown while it holds.
    /// An `if`'s two branches in one frame, each shown while its source
    /// holds (an `if let`'s: while it is some, its item bound to it).
    fn branches<M: Debug>(&mut self, cond: &str, item: Option<&str>, yes: &View<M>, no: &View<M>, f: &Rect, align: Align) {
        let flag = self.src(cond);
        let of = self.name(cond);
        let arm = |e: &mut Self, v: &View<M>, item: Option<&str>| {
            let from = e.next;
            if let Some(i) = item {
                e.items.push((i.to_string(), of.clone()));
            }
            // Each at its own height, at the top of the frame both share.
            let at = Rect {
                h: height(v).unwrap_or_else(|| f.h.clone()),
                ..f.clone()
            };
            e.place(v, &at, align);
            if item.is_some() {
                e.items.pop();
            }
            (from..e.next).map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
        };
        let yes = arm(self, yes, item);
        let no = arm(self, no, None);
        self.b.shown.push(format!("({flag}, &[{yes}], &[{no}])"));
    }

    fn layout<M: Debug>(&mut self, l: &Layout<M>, f: &Rect, align: Align) {
        // The modifiers from the outermost in: each padding insets what is
        // inside it, each background is drawn at the frame it is given.
        let mut f = f.clone();
        let mut corner = "0".to_string();
        for m in l.mods.iter().rev() {
            match m {
                Modifier::Padding(edges, s) => f = f.inset(*edges, space(*s)),
                Modifier::Corner(c) => {
                    corner = match c {
                        ark_app::view::Corner::Card => "CARD_CORNER".into(),
                        ark_app::view::Corner::Small => "CORNER".into(),
                        ark_app::view::Corner::Circle => format!("{} / 2", f.h),
                    }
                }
                Modifier::Background(s) => {
                    let id = self.id();
                    let color = match s {
                        Surface::Card => "CONTAINER_LOW",
                        Surface::Raised => "CONTAINER_HIGH",
                    };
                    self.build.push(format!("ui.rect({id}, {}, {color}, {corner});", f.frame()));
                }
                // Everything is as wide as it is given; a screen reader is
                // not the watch's.
                Modifier::FillWidth(_) | Modifier::FillHeight | Modifier::Grow | Modifier::Combine => {}
                m => panic!("Pebble has no {m:?} yet (ui/docs/pebble.md)"),
            }
        }
        match &l.prim {
            Prim::Stack {
                axis,
                align: a,
                spacing,
                content,
            } => self.stack(&flat(content), *axis, spacing.map(space), &f, *a),
            Prim::Spacer { .. } => {}
            Prim::Text(w) => {
                let id = self.id();
                let at = if w.centred { Align::Center } else { align };
                let ink = self.ink(&w.ink);
                self.build
                    .push(format!("ui.text({id}, {}, FONT_{}, {ink}, Align::{at:?});", f.frame(), style(w.style)));
                let words = self.words(&w.text);
                self.b.words.push(format!("({id}, {words})"));
            }
            Prim::Glyph { name, style: st, ink } => {
                let id = self.id();
                let side = format!("SIDE_{}", style(*st));
                // The bitmap is centred in its layer: the layer is placed.
                let x = match align {
                    Align::Start => f.x.clone(),
                    Align::Center => plus(&f.x, &format!("({} - {side}) / 2", f.w)),
                    Align::End => plus(&f.x, &minus(&f.w, &side)),
                };
                let frame = Rect {
                    x,
                    y: f.y.clone(),
                    w: side.clone(),
                    h: f.h.clone(),
                };
                let ink = self.ink(ink);
                self.build.push(format!("ui.glyph({id}, {}, {side}, {ink});", frame.frame()));
                match name {
                    Val::Is(g) => {
                        self.glyphs.insert(g.clone());
                        self.build.push(format!("ui.set_glyph({id}, {});", bytes_lit(g)));
                    }
                    Val::At(p) => {
                        let f = self.src(p);
                        self.b.glyphs.push(format!("({id}, {f})"));
                    }
                }
            }
            p => panic!("Pebble has no {} yet (ui/docs/pebble.md)", p.kind()),
        }
    }

    /// `content` one after another along `axis`, `gap` apart.
    fn stack<M: Debug>(&mut self, content: &[&View<M>], axis: Axis, gap: Option<&str>, f: &Rect, align: Align) {
        let gap = gap.unwrap_or("0");
        if axis == Axis::Depth {
            for v in content {
                self.place(v, f, align);
            }
            return;
        }
        let along = |v: &View<M>| match axis {
            Axis::Vertical => height(v),
            _ => width(v),
        };
        let sizes: Vec<Option<String>> = content.iter().map(|v| along(v)).collect();
        let fixed = sizes.iter().flatten().fold("0".to_string(), |a, s| plus(&a, s));
        let gaps = (0..content.len().saturating_sub(1)).fold("0".to_string(), |a, _| plus(&a, gap));
        let length = if axis == Axis::Vertical { &f.h } else { &f.w };
        let flexible = sizes.iter().filter(|s| s.is_none()).count();
        let fill = (flexible > 0).then(|| {
            let name = format!("fill{}", self.fills);
            self.fills += 1;
            let rest = minus(length, &plus(&fixed, &gaps));
            let share = if flexible == 1 { rest } else { format!("({rest}) / {flexible}") };
            self.build.push(format!("let {name} = {share};"));
            name
        });
        let spacers: Vec<usize> = content.iter().enumerate().filter(|(_, v)| is_spacer(v)).map(|(i, _)| i).collect();
        let mut at = if axis == Axis::Vertical { f.y.clone() } else { f.x.clone() };
        for (i, (v, size)) in content.iter().zip(&sizes).enumerate() {
            let size = size.clone().unwrap_or_else(|| fill.clone().unwrap());
            let child = match axis {
                Axis::Vertical => Rect {
                    x: f.x.clone(),
                    y: at.clone(),
                    w: f.w.clone(),
                    h: size.clone(),
                },
                _ => {
                    // Across a row, a thing shorter than it at the row's
                    // alignment.
                    let (y, h) = match height(v) {
                        Some(h) if h != f.h && align != Align::Start => {
                            let off = minus(&f.h, &h);
                            let off = if align == Align::Center { format!("({off}) / 2") } else { off };
                            (plus(&f.y, &off), h)
                        }
                        Some(h) => (f.y.clone(), h),
                        None => (f.y.clone(), f.h.clone()),
                    };
                    Rect {
                        x: at.clone(),
                        y,
                        w: size.clone(),
                        h,
                    }
                }
            };
            // Words in a row: leading before its first spacer, trailing
            // after its last, centred between.
            let words = match axis {
                Axis::Vertical => align,
                _ => match (spacers.first(), spacers.last()) {
                    (Some(&first), _) if i < first => Align::Start,
                    (_, Some(&last)) if i > last => Align::End,
                    (Some(_), _) => Align::Center,
                    _ => Align::Start,
                },
            };
            self.place(v, &child, words);
            at = plus(&plus(&at, &size), gap);
        }
    }

    fn part<M: Debug>(&mut self, p: &Part<M>, f: &Rect) {
        match p {
            Part::Meter { fraction, tint } => {
                let id = self.id();
                let fill = self.tint(tint, "meter");
                self.build.push(format!("ui.meter({id}, {}, {fill}, CONTAINER_HIGH);", f.frame()));
                let f = match fraction {
                    Val::Is(x) => format!("|_| {}", (x.clamp(0.0, 1.0) * 1000.0) as u16),
                    Val::At(p) => self.src(p),
                };
                self.b.meters.push(format!("({id}, {f})"));
            }
            Part::Section(s) => self.section(s, f),
            Part::ActionBar { .. } => panic!("an action bar is a Pebble screen's whole: its root, not inside it"),
            p => panic!("Pebble has no {} yet (ui/docs/pebble.md)", p.call().name),
        }
    }

    /// A card of rows: a `MenuLayer`, its heading and its rows a title over
    /// the line under it.
    fn section<M: Debug>(&mut self, s: &Section<M>, f: &Rect) {
        let id = self.id();
        self.build.push(format!(
            "ui.list({id}, {}, ListStyle {{ title: FONT_HEADLINE, title_line: LINE_HEADLINE, subtitle: FONT_FOOTNOTE, header: FONT_HEADER, ink: INK, quiet: QUIET, ground: BACKGROUND, inset: PAGE_MARGIN }});",
            f.frame()
        ));
        let header = match &s.title {
            None => self.words(&Val::Is(String::new())),
            Some(t) => self.words(t),
        };
        let View::Each { list, item, body } = &*s.rows else {
            panic!("Pebble binds a list to a `for` over a source, its rows list_rows")
        };
        let View::Part(Part::Row(r)) = &**body else {
            panic!("Pebble lists a `for` of list_rows only")
        };
        let of = self.name(list);
        let count = self.src(list);
        self.items.push((item.clone(), of));
        let row = |e: &Self, v: Option<&Val<String>>| match v {
            None => "|_, _, _| {}".to_string(),
            Some(Val::Is(t)) => format!("|_, _, out| {{ out.s({}); }}", bytes_lit(t)),
            Some(Val::At(p)) => e.src(p),
        };
        let (title, line) = (row(self, Some(&r.title)), row(self, r.subtitle.as_ref()));
        self.items.pop();
        self.b.lists.push(format!(
            "({id}, List {{ header: {header}, count: {count}, title: {title}, line: {line} }})"
        ));
    }

    fn ink(&self, i: &Ink) -> String {
        match i {
            Ink::Ink => "INK".into(),
            Ink::Quiet => "QUIET".into(),
            Ink::Accent => format!("{}_ACCENT_TEXT", screaming(self.s.accent)),
            Ink::Tint(t) => self.tint(t, "words"),
            Ink::Figure(t) => self.tint(t, "figure"),
        }
    }

    /// A tint by its name, as words, a figure or a meter's bar take it.
    fn tint(&self, t: &Val<String>, as_: &str) -> String {
        let Val::Is(name) = t else {
            panic!("Pebble takes a tint by its name, not read from the record ({t:?}), yet")
        };
        let accent = screaming(self.s.accent);
        match (name.as_str(), as_) {
            ("positive", _) => "POSITIVE".into(),
            ("caution", _) => "CAUTION".into(),
            ("negative", _) => "ERROR".into(),
            ("accent", "meter") => format!("{accent}_ACCENT"),
            ("accent", _) => format!("{accent}_ACCENT_TEXT"),
            (_, "meter") => "POSITIVE".into(),
            (_, "figure") => "INK".into(),
            _ => "QUIET".into(),
        }
    }

    /// A message as the generated code builds it: `Act::Open { id: t.id.clone() }`.
    fn msg<M: Debug>(&self, m: &M) -> String {
        let m = Msg::of(m);
        let case = format!("{}::{}", self.ty, m.case);
        if m.fields.is_empty() {
            return case;
        }
        let fields: Vec<String> = m
            .fields
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    Arg::Path(p) => format!("{p}.clone()"),
                    Arg::Text(t) => format!("{t:?}.into()"),
                    Arg::Number(n) => n.clone(),
                    Arg::Bool(b) => b.to_string(),
                    Arg::None => "None".into(),
                };
                format!("{k}: {v}")
            })
            .collect();
        format!("{case} {{ {} }}", fields.join(", "))
    }

    fn text(self) -> String {
        let s = self.s;
        let mut out = vec![
            // Plain comments: the module is `include!`d, where an inner
            // doc comment is not allowed.
            format!("// Generated by ark-ui-gen from {} — never edited, never committed.", s.source),
            "// The layout is said once, in Rust; change it there.".to_string(),
            String::new(),
            "use ark_pebble::tokens::*;".to_string(),
            "use ark_pebble::ui::{Align, Bindings, Button, Frame, Glyph, List, ListStyle, Ui};".to_string(),
        ];
        out.extend(s.uses.iter().map(|u| format!("use {u};")));
        out.push(String::new());
        out.push("/// The glyphs it draws: Lucide's, flattened (`ark_pebble::ui::Glyph`).".into());
        out.push("pub static GLYPHS: &[Glyph] = &[".into());
        for g in &self.glyphs {
            let path = flatten(g);
            let bytes: Vec<String> = path.iter().map(|b| b.to_string()).collect();
            out.push(format!("    Glyph {{ name: {}, path: &[{}] }},", bytes_lit(g), bytes.join(", ")));
        }
        out.push("];".into());
        out.push(String::new());
        out.push("/// Its layers, made once, laid out in the room the window gives.".into());
        out.push("#[allow(unused_variables, clippy::identity_op, clippy::redundant_field_names)]".into());
        out.push("pub fn build(ui: &mut Ui) {".into());
        out.extend(self.build.iter().map(|l| format!("    {l}")));
        out.push("}".into());
        out.push(String::new());
        out.push(format!(
            "/// What each layer shows: the screen's sources ({}), read as PebbleOS draws.",
            s.sources
        ));
        out.push("pub static BINDINGS: Bindings = Bindings {".into());
        for (name, rows) in [
            ("words", &self.b.words),
            ("meters", &self.b.meters),
            ("glyphs", &self.b.glyphs),
            ("actions", &self.b.actions),
            ("lists", &self.b.lists),
            ("shown", &self.b.shown),
        ] {
            if rows.is_empty() {
                out.push(format!("    {name}: &[],"));
            } else {
                out.push(format!("    {name}: &["));
                out.extend(rows.iter().map(|r| format!("        {r},")));
                out.push("    ],".into());
            }
        }
        out.push("};".into());
        out.push(String::new());
        out.push("/// What a button sends.".into());
        out.push(format!("pub static PRESS: &[(Button, {})] = &[", self.ty));
        for (b, m) in &self.press {
            out.push(format!("    ({b}, {m}),"));
        }
        out.push("];".into());
        out.join("\n") + "\n"
    }
}

/// What `v` holds one after another: its lists opened out.
fn flat<M>(v: &View<M>) -> Vec<&View<M>> {
    match v {
        View::List(vs) => vs.iter().flat_map(flat).collect(),
        View::Empty => vec![],
        v => vec![v],
    }
}

fn is_spacer<M>(v: &View<M>) -> bool {
    matches!(v, View::Layout(l) if matches!(l.prim, Prim::Spacer { .. }))
}

/// How tall `v` is of itself, or none: it takes what is left.
fn height<M>(v: &View<M>) -> Option<String> {
    match v {
        View::Empty => Some("0".into()),
        View::List(_) => sum(&flat(v), height),
        View::When { yes, no, .. } | View::WhenSome { yes, no, .. } => Some(max(&height(yes)?, &height(no)?)),
        View::Each { .. } => None,
        View::Part(Part::Meter { .. }) => Some("PROGRESS_HEIGHT".into()),
        View::Part(_) => None,
        View::Layout(l) => {
            let inner = match &l.prim {
                Prim::Text(w) => Some(match w.lines {
                    Some(n) if n > 1 => format!("{n} * LINE_{}", style(w.style)),
                    _ => format!("LINE_{}", style(w.style)),
                }),
                Prim::Glyph { style: s, .. } => Some(format!("SIDE_{}", style(*s))),
                Prim::Spacer { .. } => None,
                Prim::Stack {
                    axis: Axis::Vertical,
                    spacing,
                    content,
                    ..
                } => {
                    let c = flat(content);
                    let gaps = (0..c.len().saturating_sub(1)).fold("0".to_string(), |a, _| plus(&a, spacing.map(space).unwrap_or("0")));
                    sum(&c, height).map(|h| plus(&h, &gaps))
                }
                Prim::Stack { content, .. } => Some(
                    flat(content)
                        .into_iter()
                        .filter_map(height)
                        .reduce(|a, b| max(&a, &b))
                        .unwrap_or_else(|| "0".into()),
                ),
                _ => None,
            };
            modified(inner, &l.mods, true)
        }
    }
}

/// How wide `v` is of itself across a row, or none: it shares what is left.
fn width<M>(v: &View<M>) -> Option<String> {
    match v {
        View::Layout(l) => {
            let inner = match &l.prim {
                Prim::Glyph { style: s, .. } => Some(format!("SIDE_{}", style(*s))),
                _ => None,
            };
            modified(inner, &l.mods, false)
        }
        _ => None,
    }
}

/// A size and what its padding adds along one axis.
fn modified(inner: Option<String>, mods: &[Modifier], vertical: bool) -> Option<String> {
    let mut size = inner?;
    for m in mods {
        match m {
            Modifier::Padding(e, s) => {
                let n = match (e, vertical) {
                    (Edges::All, _) | (Edges::Vertical, true) | (Edges::Horizontal, false) => 2,
                    (Edges::Top | Edges::Bottom, true) | (Edges::Leading | Edges::Trailing, false) => 1,
                    _ => 0,
                };
                for _ in 0..n {
                    size = plus(&size, space(*s));
                }
            }
            Modifier::FillHeight if vertical => return None,
            _ => {}
        }
    }
    Some(size)
}

fn sum<M>(vs: &[&View<M>], of: fn(&View<M>) -> Option<String>) -> Option<String> {
    vs.iter().try_fold("0".to_string(), |a, v| Some(plus(&a, &of(v)?)))
}

fn plus(a: &str, b: &str) -> String {
    match (a, b) {
        ("0", b) => b.into(),
        (a, "0") => a.into(),
        (a, b) => format!("{a} + {b}"),
    }
}

fn minus(a: &str, b: &str) -> String {
    match b {
        "0" => a.into(),
        b if a == b => "0".into(),
        b if b.contains(['+', '-']) => format!("{a} - ({b})"),
        b => format!("{a} - {b}"),
    }
}

fn max(a: &str, b: &str) -> String {
    if a == b {
        a.into()
    } else {
        format!("({a}).max({b})")
    }
}

/// As `ark_pebble::tokens` names a step.
fn space(s: Space) -> &'static str {
    match s {
        Space::Xs => "SPACE_XS",
        Space::S => "SPACE_S",
        Space::M => "SPACE_M",
        Space::L => "SPACE_L",
        Space::Xl => "SPACE_XL",
        Space::Xxl => "SPACE_XXL",
        Space::Page => "PAGE_MARGIN",
        Space::Section => "SECTION_GAP",
    }
}

/// As `ark_pebble::tokens` names a style: `FONT_<this>`, `LINE_<this>`.
fn style(s: TextStyle) -> String {
    screaming(s.token().trim_start_matches("type-"))
}

fn screaming(s: &str) -> String {
    s.to_uppercase().replace('-', "_")
}

fn pascal(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Words as a byte string literal.
fn bytes_lit(s: &str) -> String {
    format!("{s:?}.as_bytes()")
}

// * Glyphs: Lucide's drawings as line segments

/// `name`'s drawing (arkui-glyphs) as `ark_pebble::ui::Glyph::path`: each
/// subpath its point count, 1 if it is closed, then its points in eighths
/// of the drawing's 24 units.
pub fn flatten(name: &str) -> Vec<u8> {
    let svg = arkui_glyphs::named(name).unwrap_or_else(|| panic!("arkui has no glyph called {name:?}"));
    let svg = std::str::from_utf8(svg).expect("a glyph's drawing is UTF-8");
    let mut out = vec![];
    for (tag, attrs) in elements(svg) {
        let a = |k: &str| attr(attrs, k).map(|v| v.parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0);
        let shapes: Vec<(Vec<(f64, f64)>, bool)> = match tag {
            "path" => path(attr(attrs, "d").unwrap_or("")),
            "rect" => {
                let (x, y, w, h) = (a("x"), a("y"), a("width"), a("height"));
                vec![(vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true)]
            }
            "circle" | "ellipse" => {
                let (cx, cy) = (a("cx"), a("cy"));
                let (rx, ry) = if tag == "circle" { (a("r"), a("r")) } else { (a("rx"), a("ry")) };
                let n = 24;
                let pts = (0..n)
                    .map(|i| {
                        let t = i as f64 * std::f64::consts::TAU / n as f64;
                        (cx + rx * t.cos(), cy + ry * t.sin())
                    })
                    .collect();
                vec![(pts, true)]
            }
            "line" => vec![(vec![(a("x1"), a("y1")), (a("x2"), a("y2"))], false)],
            "polyline" | "polygon" => {
                let n = numbers(attr(attrs, "points").unwrap_or(""));
                vec![(n.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect(), tag == "polygon")]
            }
            _ => vec![],
        };
        for (pts, closed) in shapes {
            let mut q: Vec<(u8, u8)> = vec![];
            for (x, y) in pts {
                let p = (eighths(x), eighths(y));
                if q.last() != Some(&p) {
                    q.push(p);
                }
            }
            if closed && q.len() > 1 && q.first() == q.last() {
                q.pop();
            }
            for chunk in q.chunks(255) {
                out.push(chunk.len() as u8);
                out.push(closed as u8);
                for (x, y) in chunk {
                    out.extend([*x, *y]);
                }
            }
        }
    }
    out
}

fn eighths(v: f64) -> u8 {
    (v * 8.0).round().clamp(0.0, 255.0) as u8
}

/// Each element's tag and its attributes' text.
fn elements(svg: &str) -> Vec<(&str, &str)> {
    svg.split('<')
        .skip(1)
        .filter(|e| !e.starts_with('/') && !e.starts_with("svg"))
        .map(|e| {
            let e = e.split('>').next().unwrap_or("").trim_end_matches('/');
            let (tag, attrs) = e.split_once(char::is_whitespace).unwrap_or((e, ""));
            (tag, attrs)
        })
        .collect()
}

fn attr<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("{key}=\"");
    let mut from = 0;
    while let Some(i) = attrs[from..].find(&pat) {
        let i = from + i;
        // A whole attribute's name, not the end of another's (`x` of `cx`).
        if i == 0 || attrs.as_bytes()[i - 1].is_ascii_whitespace() {
            let start = i + pat.len();
            let end = start + attrs[start..].find('"')?;
            return Some(&attrs[start..end]);
        }
        from = i + pat.len();
    }
    None
}

/// The numbers in `s`, as SVG writes them: `.709-1.528` is two.
fn numbers(s: &str) -> Vec<f64> {
    let mut out = vec![];
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut dot = c == b'.';
            while i < b.len() {
                let c = b[i];
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == b'.' && !dot {
                    dot = true;
                    i += 1;
                } else if (c == b'e' || c == b'E') && i + 1 < b.len() {
                    i += 1;
                    if b[i] == b'-' || b[i] == b'+' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            out.push(s[start..i].parse().unwrap_or(0.0));
        } else {
            i += 1;
        }
    }
    out
}

/// A path's subpaths as points, each closed or not: its curves and arcs
/// flattened.
fn path(d: &str) -> Vec<(Vec<(f64, f64)>, bool)> {
    let mut out: Vec<(Vec<(f64, f64)>, bool)> = vec![];
    let mut cur: Vec<(f64, f64)> = vec![];
    let (mut x, mut y) = (0.0f64, 0.0f64);
    let (mut sx, mut sy) = (0.0, 0.0);
    // The last control point, for a smooth curve's reflection.
    let mut last_c: Option<(f64, f64)> = None;
    let mut last_q: Option<(f64, f64)> = None;
    let b = d.as_bytes();
    let mut i = 0;
    let mut cmd = b'M';
    let take = |i: &mut usize, n: usize| -> Option<Vec<f64>> {
        let mut vals = vec![];
        while vals.len() < n {
            while *i < b.len() && (b[*i] == b' ' || b[*i] == b',') {
                *i += 1;
            }
            if *i >= b.len() || b[*i].is_ascii_alphabetic() {
                return None;
            }
            let start = *i;
            *i += 1;
            let mut dot = b[start] == b'.';
            while *i < b.len() {
                let c = b[*i];
                if c.is_ascii_digit() {
                    *i += 1;
                } else if c == b'.' && !dot {
                    dot = true;
                    *i += 1;
                } else {
                    break;
                }
            }
            vals.push(d[start..*i].parse().ok()?);
        }
        Some(vals)
    };
    // An arc's two flags are one digit each, and may touch.
    let flag = |i: &mut usize| -> Option<f64> {
        while *i < b.len() && (b[*i] == b' ' || b[*i] == b',') {
            *i += 1;
        }
        let c = *b.get(*i)?;
        *i += 1;
        (c == b'0' || c == b'1').then_some(f64::from(c - b'0'))
    };
    let end = |cur: &mut Vec<(f64, f64)>, out: &mut Vec<(Vec<(f64, f64)>, bool)>, closed: bool| {
        if !cur.is_empty() {
            out.push((std::mem::take(cur), closed));
        }
    };
    loop {
        while i < b.len() && (b[i] == b' ' || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        if b[i].is_ascii_alphabetic() {
            cmd = b[i];
            i += 1;
            if cmd == b'Z' || cmd == b'z' {
                end(&mut cur, &mut out, true);
                x = sx;
                y = sy;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        let before = i;
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let Some(v) = take(&mut i, 2) else { break };
                end(&mut cur, &mut out, false);
                x = ox + v[0];
                y = oy + v[1];
                sx = x;
                sy = y;
                cur.push((x, y));
                // What follows a move is a line.
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                let Some(v) = take(&mut i, 2) else { break };
                x = ox + v[0];
                y = oy + v[1];
                cur.push((x, y));
            }
            b'H' => {
                let Some(v) = take(&mut i, 1) else { break };
                x = ox + v[0];
                cur.push((x, y));
            }
            b'V' => {
                let Some(v) = take(&mut i, 1) else { break };
                y = oy + v[0];
                cur.push((x, y));
            }
            b'C' | b'S' => {
                let smooth = cmd.eq_ignore_ascii_case(&b'S');
                let Some(v) = take(&mut i, if smooth { 4 } else { 6 }) else { break };
                let c1 = if smooth {
                    last_c.map(|(cx, cy)| (2.0 * x - cx, 2.0 * y - cy)).unwrap_or((x, y))
                } else {
                    (ox + v[0], oy + v[1])
                };
                let k = if smooth { 0 } else { 2 };
                let c2 = (ox + v[k], oy + v[k + 1]);
                let p = (ox + v[k + 2], oy + v[k + 3]);
                for s in 1..=8 {
                    let t = s as f64 / 8.0;
                    let u = 1.0 - t;
                    cur.push((
                        u * u * u * x + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * p.0,
                        u * u * u * y + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * p.1,
                    ));
                }
                last_c = Some(c2);
                (x, y) = p;
            }
            b'Q' | b'T' => {
                let smooth = cmd.eq_ignore_ascii_case(&b'T');
                let Some(v) = take(&mut i, if smooth { 2 } else { 4 }) else { break };
                let c = if smooth {
                    last_q.map(|(cx, cy)| (2.0 * x - cx, 2.0 * y - cy)).unwrap_or((x, y))
                } else {
                    (ox + v[0], oy + v[1])
                };
                let k = if smooth { 0 } else { 2 };
                let p = (ox + v[k], oy + v[k + 1]);
                for s in 1..=6 {
                    let t = s as f64 / 6.0;
                    let u = 1.0 - t;
                    cur.push((u * u * x + 2.0 * u * t * c.0 + t * t * p.0, u * u * y + 2.0 * u * t * c.1 + t * t * p.1));
                }
                last_q = Some(c);
                (x, y) = p;
            }
            b'A' => {
                let Some(r) = take(&mut i, 3) else { break };
                let (Some(large), Some(sweep)) = (flag(&mut i), flag(&mut i)) else {
                    break;
                };
                let Some(v) = take(&mut i, 2) else { break };
                let p = (ox + v[0], oy + v[1]);
                cur.extend(arc((x, y), p, r[0], r[1], r[2], large != 0.0, sweep != 0.0));
                (x, y) = p;
            }
            _ => break,
        }
        if !matches!(cmd.to_ascii_uppercase(), b'C' | b'S') {
            last_c = None;
        }
        if !matches!(cmd.to_ascii_uppercase(), b'Q' | b'T') {
            last_q = None;
        }
        if i == before {
            break;
        }
    }
    end(&mut cur, &mut out, false);
    out
}

/// An SVG arc from `a` to `b` as points after `a` (SVG 1.1's F.6.5).
fn arc(a: (f64, f64), b: (f64, f64), rx: f64, ry: f64, rot: f64, large: bool, sweep: bool) -> Vec<(f64, f64)> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 || a == b {
        return vec![b];
    }
    let phi = rot.to_radians();
    let (sin, cos) = phi.sin_cos();
    let (dx, dy) = ((a.0 - b.0) / 2.0, (a.1 - b.1) / 2.0);
    let (x1, y1) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (cos * cx1 - sin * cy1 + (a.0 + b.0) / 2.0, sin * cx1 + cos * cy1 + (a.1 + b.1) / 2.0);
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f64::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f64::consts::TAU;
    }
    let n = ((dt.abs() / (std::f64::consts::PI / 8.0)).ceil() as usize).max(1);
    (1..=n)
        .map(|s| {
            let t = t1 + dt * s as f64 / n as f64;
            let (ex, ey) = (rx * t.cos(), ry * t.sin());
            (cos * ex - sin * ey + cx, sin * ex + cos * ey + cy)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_flatten_to_eighths() {
        // `pause`: two rectangles, closed.
        assert_eq!(
            flatten("pause"),
            vec![4, 1, 112, 24, 152, 24, 152, 168, 112, 168, 4, 1, 40, 24, 80, 24, 80, 168, 40, 168]
        );
        // `skip-forward`: a line, open, and a rounded triangle, closed.
        let skip = flatten("skip-forward");
        assert_eq!(&skip[..6], &[2, 0, 168, 32, 168, 160]);
        assert_eq!(skip[7], 1);
    }

    #[test]
    fn numbers_read_as_svg_writes_them() {
        assert_eq!(numbers(".709-1.528 2,3"), vec![0.709, -1.528, 2.0, 3.0]);
        assert_eq!(numbers("1.7.7"), vec![1.7, 0.7]);
    }
}
