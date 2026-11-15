//! The site as an Ark app: its part of its host ([`Docs`], an
//! `ark_app::host::HostedApp` drawn in a window, `ark_app::window::Windowed`)
//! — which page is shown, what is searched, which phone — the shell's view
//! ([`crate::views`]) over it, and what the shell hosts said in the views
//! every toolkit draws. It has no data of its own (`Nothing`), no server
//! and no first run: the browser's host runs it as the page alone, and its
//! export writes every page (ui/docs/app.md, "Export").
//!
//! **Paths are pages.** A page's route is [`crate::routes::Route`], its tag
//! alone (`/`, `/all/`, `/menu/`), and every one is written
//! (`Windowed::paths`); the search (`?q=`) and the phone (`?phone=`) are
//! the reader's, never written, taken up by the module once it has taken
//! the page over.
use std::rc::Rc;

use ark_app::host::{Cx, HostedApp, Link, Nothing, Place, Running, Session};
use ark_app::shell::specimen;
use ark_app::view::{Part, View};
use ark_app::window::Windowed;
use ark_app::writing::{self, writing};
use ark_router::Address;

use crate::components::{Kind, Section, Shot, COMPONENTS};
use crate::pages::{self, Built, Page, ALL, OVERVIEW};
use crate::routes::Route;
use crate::views::{doc_page, docs_shell, Hit, PageCard, ShellPage};

/// Below this the page is a phone's, and the pictures the narrow ones
/// (`[layout]`'s compact width, as theme.css has it).
pub const PHONE_WIDTH: u32 = 700;

/// Where the phones' captures are read from: the `screens` branch on
/// GitHub (public/head.html says the same to docs.js).
pub const SCREENS: &str = "https://raw.githubusercontent.com/k2on/apps/screens/";

/// The components whose page shows arkui-web's own specimen of them, live
/// (`ark_app::shell::specimen`): what "In the Browser" is over.
pub const LIVE: [&str; 14] = [
    "menu",
    "dialog",
    "command",
    "picker",
    "keys",
    "lists",
    "cards",
    "headers",
    "navigation",
    "style",
    "springs",
    "account",
    "text",
    "writing",
];

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Go {
        tag: String,
    },
    Search {
        text: String,
    },
    /// The phone shown: `android`, `ios`, `both` side by side, or none
    /// chosen (each in turn).
    Phone {
        phone: String,
    },
    /// System, light, dark, in turn: the reader's.
    Appearance,
    /// The page after the one shown in reading order (`1`), or before.
    Step(isize),
}

/// The site, at a page.
pub struct Docs {
    pub base: String,
    pub page: String,
    pub search: String,
    pub phone: String,
    /// The appearance chosen over the system's.
    pub tone: Option<&'static str>,
    pub built: Rc<Built>,
    pages: Rc<Vec<Page>>,
}

type Seen<'a> = &'a dyn Place<Nothing, Nothing>;

impl Docs {
    /// The site served under `base` (`/`, `/apps/docs/`), with what its
    /// export worked out.
    pub fn new(base: &str, built: Built) -> Docs {
        let base = if base.is_empty() { "/" } else { base };
        Docs {
            base: base.to_string(),
            page: OVERVIEW.into(),
            search: String::new(),
            phone: String::new(),
            tone: None,
            built: Rc::new(built),
            pages: Rc::new(pages::pages(base)),
        }
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The page shown.
    pub fn shown(&self) -> &Page {
        self.pages.iter().find(|p| p.tag == self.page).unwrap_or(&self.pages[0])
    }

    /// A page's address: its route's (`/menu/`), the overview the root.
    pub fn route_of(tag: &str) -> Address {
        Route::of(tag).address()
    }

    fn card(p: &Page) -> PageCard {
        PageCard {
            title: p.title.clone(),
            glyph: p.glyph.clone(),
            tag: p.tag.clone(),
            intro: p.intro.clone(),
            reference: p.reference.clone(),
        }
    }

    /// The page tagged `tag` shown, or the overview for a tag of none.
    fn go(&mut self, tag: &str) {
        self.page = match self.pages.iter().any(|p| p.tag == tag) {
            true => tag.to_string(),
            false => OVERVIEW.into(),
        };
        self.search.clear();
    }

    /// What the search finds: every page with each of its words.
    pub fn found(&self) -> Vec<Hit> {
        let words: Vec<String> = self.search.to_lowercase().split_whitespace().map(String::from).collect();
        self.pages
            .iter()
            .filter(|p| !words.is_empty() && words.iter().all(|w| p.words.contains(w.as_str())))
            .map(|p| Hit {
                title: p.title.clone(),
                said: p.description.clone(),
                tag: p.tag.clone(),
            })
            .collect()
    }

    pub fn record(&self) -> ShellPage {
        let of = |s: Section| self.pages.iter().filter(|p| p.section == Some(s)).map(Self::card).collect();
        let own = |s: Section| self.pages.iter().filter(|p| p.index == Some(s)).map(Self::card).collect();
        ShellPage {
            page: self.page.clone(),
            search: self.search.clone(),
            searching: !self.search.trim().is_empty(),
            results: self.found(),
            top: self.pages.iter().filter(|p| p.tag == OVERVIEW).map(Self::card).collect(),
            more: self.pages.iter().filter(|p| p.tag == ALL).map(Self::card).collect(),
            foundations_page: own(Section::Foundations),
            foundations: of(Section::Foundations),
            components_page: own(Section::Components),
            components: of(Section::Components),
            overlays_page: own(Section::Overlays),
            overlays: of(Section::Overlays),
            motion_page: own(Section::Motion),
            motion: of(Section::Motion),
            behaviour_page: own(Section::Behaviour),
            behaviour: of(Section::Behaviour),
        }
    }

    /// The page shown's neighbours in reading order: before, after.
    pub fn neighbours(&self) -> (Option<&Page>, Option<&Page>) {
        let order = pages::reading(&self.pages);
        match order.iter().position(|p| p.tag == self.page) {
            Some(i) => (i.checked_sub(1).map(|j| order[j]), order.get(i + 1).copied()),
            None => (None, None),
        }
    }

    /// The page shown (`views::doc_page`), a tab's scrolling page, each
    /// part it names (`hosted(…)`) in its place.
    fn page_view(&self) -> View<Msg> {
        let card = Self::card(self.shown());
        let mut parts = vec![];
        let mut flat = vec![doc_page::view(&card)];
        while let Some(v) = flat.pop() {
            match v {
                View::List(vs) => flat.extend(vs.into_iter().rev()),
                View::Empty => {}
                View::Part(Part::Hosted { name }) => parts.push(self.part(name)),
                v => parts.push(v),
            }
        }
        ark_app::view::tab_page("", View::List(parts)).into()
    }

    /// A part of the page shown, by its name in `doc_page`.
    fn part(&self, name: &str) -> View<Msg> {
        match name {
            "pager" => self.pager(),
            "index" => self.index(),
            "live" => self.live(),
            "desktop" => self.desktop(),
            "mobile" => self.mobile(),
            _ => View::Empty,
        }
    }

    /// The pages before and after in reading order, as links.
    fn pager(&self) -> View<Msg> {
        let (before, after) = self.neighbours();
        let glyph = |name: &str| {
            String::from_utf8_lossy(arkui_glyphs::named(name).unwrap_or(arkui_glyphs::NOTE))
                .replace("<svg ", "<svg width=\"12\" height=\"12\" aria-hidden=\"true\" ")
        };
        let one = |p: &Page, next: bool| {
            let (class, rel, said) = match next {
                true => (
                    "ark-pager-next",
                    "next",
                    format!("<span>Next</span><span class=\"ark-glyph\">{}</span>", glyph("chevron-right")),
                ),
                false => (
                    "ark-pager-previous",
                    "prev",
                    format!("<span class=\"ark-glyph\">{}</span><span>Previous</span>", glyph("chevron-left")),
                ),
            };
            format!(
                "<a class=\"{class}\" href=\"{href}\" rel=\"{rel}\"><span class=\"ark-pager-said\">{said}</span><span class=\"ark-pager-title\">{title}</span></a>",
                href = esc(&pages::href(&self.base, &p.tag)),
                title = esc(&p.title),
            )
        };
        let links: String = [before.map(|p| one(p, false)), after.map(|p| one(p, true))]
            .into_iter()
            .flatten()
            .collect();
        writing(format!("<nav class=\"ark-pager\" aria-label=\"Pages\">{links}</nav>"))
    }

    /// A section's own page: a row for each of its pages.
    fn index(&self) -> View<Msg> {
        let Some(s) = self.shown().index else { return View::Empty };
        let rows: Vec<View<Msg>> = self
            .pages
            .iter()
            .filter(|p| p.section == Some(s))
            .map(|p| {
                ark_app::view::list_row(p.title.clone())
                    .subtitle(p.description.clone())
                    .glyph(pages::glyph(&p.tag))
                    .pushes()
                    .on_tap(Msg::Go { tag: p.tag.clone() })
                    .into()
            })
            .collect();
        ark_app::view::card_section(rows).into()
    }

    /// arkui-web's own specimen of the component shown, live.
    fn live(&self) -> View<Msg> {
        let Some(id) = LIVE.iter().copied().find(|id| *id == self.page) else {
            return View::Empty;
        };
        View::List(vec![
            writing("## In the Browser\n\narkui-web's own parts, live: the same `ark_app::view` parts an app's screen is made of. What opens over a page is drawn open in its frame, so it reads without script; with script, each part answers."),
            specimen(id),
        ])
    }

    /// The component's pictures, drawn by arkui itself when the site was
    /// built, and what its functions answered.
    fn desktop(&self) -> View<Msg> {
        let Some(b) = self.built.of(&self.page) else { return View::Empty };
        if b.pictures.is_empty() && b.answers.is_empty() && b.notes.is_empty() {
            return View::Empty;
        }
        let base = &self.base;
        let mut md = String::from("## On the Desktop\n\nEach picture is the arkui component itself, drawn by iced's own renderer when the site was built, in the page's theme and at its width.\n\n");
        for p in &b.pictures {
            let size = |i: usize| p.sizes.get(i).copied().unwrap_or((0, 0));
            let alt = esc(&writing::plain(&writing::inline(&p.caption)));
            let one = |tone: &str, wide: usize, narrow: usize| {
                format!(
                    "<picture class=\"{tone}\"><source media=\"(max-width: {PHONE_WIDTH}px)\" srcset=\"{base}pictures/{name}-{tone}-narrow.webp\" width=\"{nw}\" height=\"{nh}\">\
                     <img src=\"{base}pictures/{name}-{tone}-wide.webp\" width=\"{ww}\" height=\"{wh}\" alt=\"{alt}\" loading=\"lazy\" decoding=\"async\"></picture>",
                    name = p.name,
                    nw = size(narrow).0,
                    nh = size(narrow).1,
                    ww = size(wide).0,
                    wh = size(wide).1,
                )
            };
            md.push_str(&format!(
                "<figure class=\"docs-picture\">{}{}<figcaption>{}</figcaption></figure>\n\n",
                one("light", 0, 1),
                one("dark", 2, 3),
                inline_html(&p.caption),
            ));
        }
        for a in &b.answers {
            md.push_str(&format!("| {} | Answers |\n| --- | --- |\n", cell(&a.caption, false)));
            for (call, said) in &a.rows {
                md.push_str(&format!("| {} | {} |\n", cell(call, true), cell(said, true)));
            }
            md.push('\n');
        }
        for n in &b.notes {
            md.push_str(n);
            md.push_str("\n\n");
        }
        writing(md)
    }

    /// The phones: Android's and iOS's halves, each with its latest
    /// captures, which `docs.js` fetches from the screens branch.
    fn mobile(&self) -> View<Msg> {
        let Some(c) = COMPONENTS.iter().find(|c| c.id == self.page) else {
            return View::Empty;
        };
        let both: Vec<&Shot> = match (&c.android, &c.ios) {
            (Some(_), Some(_)) => c
                .shots
                .iter()
                .filter(|s| s.phones.contains(&"android") && s.phones.contains(&"ios"))
                .collect(),
            _ => vec![],
        };
        let mut chips = vec![("android", "Android", "smartphone"), ("ios", "iOS", "smartphone")];
        if !both.is_empty() {
            chips.push(("both", "Side by Side", "copy"));
        }
        let chips = ark_app::view::chip_row(
            chips
                .into_iter()
                .map(|(key, name, g)| {
                    let next = if self.phone == key { String::new() } else { key.to_string() };
                    ark_app::view::chip(name, g, self.phone == key, Msg::Phone { phone: next })
                })
                .collect::<Vec<_>>(),
        );
        let mut out = vec![
            writing("## On the Phones\n\nThe shared Compose and SwiftUI, where each is and how to call it, with the latest capture from a real emulator and simulator."),
            chips,
        ];
        let shown = |key: &str| self.phone.is_empty() || self.phone == key;
        for (key, name, half) in [("android", "Android", c.android.as_ref()), ("ios", "iOS", c.ios.as_ref())] {
            if !shown(key) {
                continue;
            }
            match half {
                None => out.push(writing(format!("### {name}\n\nNothing shared for this on {name} yet."))),
                Some(half) => {
                    let shots: String = c
                        .shots
                        .iter()
                        .filter(|s| s.phones.contains(&key))
                        .map(|s| capture(s, key, None))
                        .collect();
                    let captures = match shots.is_empty() {
                        true => String::new(),
                        false => format!("<div class=\"docs-captures\">{shots}</div>\n\n"),
                    };
                    out.push(writing(format!("### {name}\n\n{captures}{}", pages::mobile(half))));
                }
            }
        }
        if self.phone == "both" {
            let pairs: String = both
                .iter()
                .map(|s| {
                    format!(
                        "<div class=\"docs-captures docs-pair\">{}{}</div>",
                        capture(s, "android", Some("Android")),
                        capture(s, "ios", Some("iOS"))
                    )
                })
                .collect();
            out.push(writing(pairs));
        }
        View::List(out)
    }
}

/// `s` safe in an HTML attribute or text.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A caption's inline Markdown (`code`, *emphasis*) as HTML, for a
/// figure's caption.
fn inline_html(md: &str) -> String {
    use writing::Inline;
    fn one(i: &Inline, out: &mut String) {
        match i {
            Inline::Text(t) => out.push_str(&esc(t)),
            Inline::Code(c) => out.push_str(&format!("<code>{}</code>", esc(c))),
            Inline::Emphasis(xs) => {
                out.push_str("<em>");
                xs.iter().for_each(|x| one(x, out));
                out.push_str("</em>");
            }
            Inline::Strong(xs) => {
                out.push_str("<strong>");
                xs.iter().for_each(|x| one(x, out));
                out.push_str("</strong>");
            }
            Inline::Link { href, text } => {
                out.push_str(&format!("<a href=\"{}\">", esc(href)));
                text.iter().for_each(|x| one(x, out));
                out.push_str("</a>");
            }
            Inline::Break => out.push_str("<br>"),
        }
    }
    let mut out = String::new();
    for i in writing::inline(md) {
        one(&i, &mut out);
    }
    out
}

/// A table's cell in Markdown: the words, or code (a call and what it
/// answered), its bars and backticks kept out of the table's way.
fn cell(s: &str, code: bool) -> String {
    let s = s.replace('|', "\u{2223}");
    match code {
        true => format!("`{}`", s.replace('`', "'")),
        false => s,
    }
}

/// One phone's capture of a shot, in a frame of that device, which
/// docs.js fills from the screens branch's manifest once script runs.
fn capture(s: &Shot, key: &str, phone: Option<&str>) -> String {
    let (kind, word) = match s.kind {
        Kind::Screenshot => ("screenshot", "Screenshot"),
        Kind::Recording => ("recording", "Recording"),
    };
    format!(
        "<figure class=\"capture\" data-phone=\"{key}\" data-shot=\"{id}\" data-kind=\"{kind}\"><div class=\"device\"><div class=\"screen\">\
         <p class=\"ark-js\">Asking for the latest capture…</p><p class=\"ark-nojs\">The latest capture is fetched by script from the <a href=\"{SCREENS}manifest.json\">screens branch</a>.</p>\
         </div></div><figcaption>{phone}{word} · {what}</figcaption><small></small></figure>",
        id = s.id,
        phone = phone.map(|p| format!("{p} · ")).unwrap_or_default(),
        what = esc(s.what),
    )
}

impl HostedApp for Docs {
    type Key = Nothing;
    type Change = Nothing;
    type Route = Route;
    type Request = ();

    fn encode(change: &Nothing) -> (&'static str, Vec<u8>) {
        match *change {}
    }

    /// The site under the session's base, with what its export worked out
    /// (`Session::prepared`: the pictures' sizes, the answers' tables).
    fn open(session: &Session, _cx: &mut Cx<'_, Docs>) -> Docs {
        Docs::new(&session.base, serde_json::from_str(&session.prepared).unwrap_or_default())
    }

    fn screens(&mut self) -> Vec<&mut dyn Running<Docs>> {
        vec![]
    }

    fn screens_ref(&self) -> Vec<&dyn Running<Docs>> {
        vec![]
    }

    fn route(&mut self, route: Route, _cx: &mut Cx<'_, Docs>) {
        self.go(route.tag())
    }

    fn request(&mut self, _request: (), _cx: &mut Cx<'_, Docs>) {}

    /// A page's address: its path the page, its query the reader's search
    /// (`q`) and phone (`phone`); a launch's appearance.
    fn link(&mut self, link: Link, _cx: &mut Cx<'_, Docs>) {
        match link {
            Link::Address(url) => {
                let a = Address::of_url(&url, "/");
                let tag = Route::from_address(&a).map(|r| r.tag().to_string()).unwrap_or_else(|| OVERVIEW.into());
                self.go(&tag);
                self.search = a.get("q").unwrap_or_default().to_string();
                self.phone = a
                    .get("phone")
                    .filter(|p| ["android", "ios", "both"].contains(p))
                    .unwrap_or_default()
                    .to_string();
            }
            Link::Launch(args) => {
                for pair in args.chunks(2) {
                    if let [k, v] = pair {
                        if k == "-appearance" {
                            self.tone = tone(v);
                        }
                    }
                }
            }
        }
    }

    fn launched(&mut self, args: Vec<String>, cx: &mut Cx<'_, Docs>) {
        self.link(Link::Launch(args), cx)
    }
}

/// An appearance by its word.
fn tone(word: &str) -> Option<&'static str> {
    match word {
        "light" => Some("light"),
        "dark" => Some("dark"),
        _ => None,
    }
}

impl Windowed for Docs {
    type Msg = Msg;

    fn take(&mut self, msg: Msg, _cx: &mut Cx<'_, Docs>) {
        match msg {
            Msg::Go { tag } => self.go(&tag),
            Msg::Search { text } => self.search = text,
            Msg::Phone { phone } => self.phone = phone,
            Msg::Appearance => {
                self.tone = match self.tone {
                    None => Some("light"),
                    Some("light") => Some("dark"),
                    _ => None,
                }
            }
            Msg::Step(by) => {
                let (before, after) = self.neighbours();
                if let Some(tag) = (if by < 0 { before } else { after }).map(|p| p.tag.clone()) {
                    self.go(&tag);
                }
            }
        }
    }

    fn window(&self, _place: Seen<'_>, _now_ms: i64) -> View<Msg> {
        docs_shell::view(&self.record())
    }

    /// The page shown, the shell's one hosted name.
    fn hosted(&self, name: &str, _place: Seen<'_>, _now_ms: i64) -> View<Msg> {
        match name {
            "page" => self.page_view(),
            _ => View::Empty,
        }
    }

    /// The command box's (`:`, ⌘K): the pages before and after, every
    /// page, the appearance.
    fn commands(&self) -> Vec<ark_app::shell::CommandMenu<Msg>> {
        use ark_app::shell::{command_menu, command_unbound, MenuPlace};
        vec![command_menu(
            MenuPlace::View,
            vec![
                command_unbound("Next Page", Msg::Step(1)),
                command_unbound("Previous Page", Msg::Step(-1)),
                command_unbound("All Pages", Msg::Go { tag: ALL.into() }),
                command_unbound("Appearance", Msg::Appearance),
            ],
        )]
    }

    /// The page is the path; the search (`q`) and the phone are the
    /// query's, the reader's.
    fn address(&self, _place: Seen<'_>, _now_ms: i64) -> String {
        Self::route_of(&self.page)
            .with("q", self.search.clone())
            .with("phone", self.phone.clone())
            .url()
    }

    fn appearance(&self) -> Option<&'static str> {
        self.tone
    }

    /// The page's `<title>`: its title and the site's, the overview the
    /// site's alone.
    fn title(&self, _place: Seen<'_>, _now_ms: i64) -> Option<String> {
        Some(match self.page.as_str() {
            OVERVIEW => "ArkUI".to_string(),
            _ => format!("{} · ArkUI", self.shown().title),
        })
    }

    fn description(&self, _place: Seen<'_>, _now_ms: i64) -> Option<String> {
        Some(self.shown().description.clone())
    }

    /// Every page's path: each is written, and no other (the reader's
    /// search is a query, never a page).
    fn paths(&self, _place: Seen<'_>) -> Vec<String> {
        self.pages.iter().map(|p| Self::route_of(&p.tag).url()).collect()
    }
}
