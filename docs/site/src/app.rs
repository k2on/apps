//! The site as an app (`arkui_web::WebApp`): which page is shown, what is
//! searched, which phone, and the live parts' state; the shell's view
//! ([`crate::views`]) over it, and what the shell hosts drawn here.
//!
//! **Paths are pages.** A page's route is its tag alone (`/`, `/all/`,
//! `/menu/`), and every one is written ([`DocsWeb::static_paths`]); the
//! search (`?q=`) and the phone are the reader's, never written, taken up
//! by the module once it has taken the page over.
use std::rc::Rc;

use ark_app::writing::{self, Block, Inline};
use arkui_web::page::Route;
use arkui_web::{AnyView, WebApp};
use leptos::html;
use leptos::prelude::*;

use crate::components::{Kind, Section, Shot, COMPONENTS};
use crate::live::{self, Live, LiveMsg};
use crate::pages::{self, Built, Page, ALL, OVERVIEW};
use crate::views::{doc_page, docs_shell, Hit, PageCard, ShellPage};
use ark_app::view::{Part, View};

/// Below this the page is a phone's, and the pictures the narrow ones
/// (`[layout]`'s compact width, as theme.css has it).
pub const PHONE_WIDTH: u32 = 700;

/// Where the phones' captures are read from: the `screens` branch on
/// GitHub, or wherever `DOCS_SCREENS` said when this was built.
pub fn screens() -> &'static str {
    option_env!("DOCS_SCREENS").unwrap_or("https://raw.githubusercontent.com/k2on/apps/screens/")
}

#[derive(Clone, Debug)]
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
    /// System, light, dark, in turn: the reader's, kept by the browser.
    Appearance,
    Live(LiveMsg),
}

pub struct DocsWeb {
    pub base: String,
    pub page: String,
    pub search: String,
    pub phone: String,
    pub live: Live,
    pub built: Rc<Built>,
    pages: Rc<Vec<Page>>,
}

impl DocsWeb {
    /// The site served under `base` (`/`, `/apps/docs/`), with what the
    /// native build worked out.
    pub fn open(base: &str, built: Built) -> DocsWeb {
        DocsWeb {
            base: base.to_string(),
            page: OVERVIEW.into(),
            search: String::new(),
            phone: String::new(),
            live: Live::default(),
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

    /// A page's route: its tag, the overview none.
    pub fn route_of(tag: &str) -> Route {
        match tag {
            OVERVIEW => Route::default(),
            t => Route::at([t]),
        }
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
        ShellPage {
            page: self.page.clone(),
            search: self.search.clone(),
            searching: !self.search.trim().is_empty(),
            results: self.found(),
            top: self.pages.iter().filter(|p| p.section.is_none()).map(Self::card).collect(),
            foundations: of(Section::Foundations),
            components: of(Section::Components),
            overlays: of(Section::Overlays),
            motion: of(Section::Motion),
            behaviour: of(Section::Behaviour),
        }
    }

    /// The page shown (`views::doc_page`), a scrolling column as a tab's
    /// page is, each part it hosts drawn in its place.
    fn page_view(&self) -> AnyView {
        let card = Self::card(self.shown());
        let mut parts = vec![];
        let mut flat = vec![doc_page::view(&card)];
        while let Some(v) = flat.pop() {
            match v {
                View::List(vs) => flat.extend(vs.into_iter().rev()),
                View::Empty => {}
                View::Part(Part::Hosted { name }) => parts.push(self.host(name)),
                v => parts.push(arkui_web::render(v)),
            }
        }
        html::div()
            .class("ark-page")
            .child(html::div().class("ark-page-column docs-page").child(parts))
            .into_any()
    }

    /// What the shell hosts on the page shown.
    fn host(&self, name: &str) -> AnyView {
        if name == "page" {
            return self.page_view();
        }
        let Some(c) = COMPONENTS.iter().find(|c| c.id == self.page) else {
            return ().into_any();
        };
        match name {
            "live" => match live::view(c.id, &self.live) {
                Some(v) => html::section()
                    .class("docs-live")
                    .child((
                        arkui_web::writing::markdown("## In the Browser\n\narkui-web's own parts, live: the same `ark_app::view` parts an app's screen is made of. Without script they are drawn as they open; with it, they answer."),
                        html::div().class("docs-specimen").child(arkui_web::render(v)),
                        (c.id == "writing").then(|| arkui_web::writing::code("md", live::WRITING_SAMPLE)),
                    ))
                    .into_any(),
                None => ().into_any(),
            },
            "desktop" => self.desktop(c.id),
            "mobile" => self.mobile(c),
            _ => ().into_any(),
        }
    }

    /// A component's pictures, drawn by arkui itself, and what its
    /// functions answered.
    fn desktop(&self, id: &str) -> AnyView {
        let Some(b) = self.built.of(id) else { return ().into_any() };
        if b.pictures.is_empty() && b.answers.is_empty() && b.notes.is_empty() {
            return ().into_any();
        }
        let base = &self.base;
        let pictures: Vec<AnyView> = b
            .pictures
            .iter()
            .map(|p| {
                let size = |i: usize| p.sizes.get(i).copied().unwrap_or((0, 0));
                let one = |tone: &str, wide: usize, narrow: usize| {
                    html::picture().class(tone.to_string()).child((
                        html::source()
                            .attr("media", format!("(max-width: {PHONE_WIDTH}px)"))
                            .attr("srcset", format!("{base}pictures/{}-{tone}-narrow.webp", p.name))
                            .attr("width", size(narrow).0.to_string())
                            .attr("height", size(narrow).1.to_string()),
                        html::img()
                            .attr("src", format!("{base}pictures/{}-{tone}-wide.webp", p.name))
                            .attr("width", size(wide).0.to_string())
                            .attr("height", size(wide).1.to_string())
                            .attr("alt", writing::plain(&writing::inline(&p.caption)))
                            .attr("loading", "lazy")
                            .attr("decoding", "async"),
                    ))
                };
                html::figure()
                    .class("docs-picture")
                    .child((
                        one("light", 0, 1),
                        one("dark", 2, 3),
                        html::figcaption().child(arkui_web::writing::inlines(&writing::inline(&p.caption))),
                    ))
                    .into_any()
            })
            .collect();
        let answers: Vec<Block> = b
            .answers
            .iter()
            .map(|a| Block::Table {
                head: vec![vec![Inline::Text(a.caption.clone())], vec![Inline::Text("Answers".into())]],
                rows: a
                    .rows
                    .iter()
                    .map(|(call, said)| vec![vec![Inline::Code(call.clone())], vec![Inline::Code(said.clone())]])
                    .collect(),
            })
            .collect();
        let notes: String = b.notes.iter().map(|n| format!("{n}\n\n")).collect();
        html::section()
            .class("docs-desktop")
            .child((
                arkui_web::writing::markdown("## On the Desktop\n\nEach picture is the arkui component itself, drawn by iced's own renderer when the site was built, in the page's theme and at its width."),
                pictures,
                arkui_web::writing::blocks(&answers),
                arkui_web::writing::markdown(&notes),
            ))
            .into_any()
    }

    /// The phones: Android's and iOS's halves, each with its latest
    /// captures, which `docs.js` fetches from the screens branch.
    fn mobile(&self, c: &crate::components::Component) -> AnyView {
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
        let shown = |key: &str| self.phone.is_empty() || self.phone == key;
        let mut panes: Vec<AnyView> = vec![];
        for (key, name, half) in [("android", "Android", c.android.as_ref()), ("ios", "iOS", c.ios.as_ref())] {
            if !shown(key) {
                continue;
            }
            let body = match half {
                None => arkui_web::writing::markdown(&format!("### {name}\n\nNothing shared for this on {name} yet.")),
                Some(half) => {
                    let shots: Vec<AnyView> = c
                        .shots
                        .iter()
                        .filter(|s| s.phones.contains(&key))
                        .map(|s| capture(s, key, None))
                        .collect();
                    (
                        arkui_web::writing::markdown(&format!("### {name}")),
                        (!shots.is_empty()).then(|| html::div().class("docs-captures").child(shots)),
                        arkui_web::writing::markdown(&pages::mobile(half)),
                    )
                        .into_any()
                }
            };
            panes.push(html::div().class("docs-pane").attr("data-phone", key).child(body).into_any());
        }
        if self.phone == "both" {
            for s in &both {
                panes.push(
                    html::div()
                        .class("docs-captures docs-pair")
                        .child((capture(s, "android", Some("Android")), capture(s, "ios", Some("iOS"))))
                        .into_any(),
                );
            }
        }
        html::section()
            .class("docs-mobile")
            .child((
                arkui_web::writing::markdown("## On the Phones\n\nThe shared Compose and SwiftUI, where each is and how to call it, with the latest capture from a real emulator and simulator."),
                html::div().class("docs-phones").child(arkui_web::render(chips)),
                panes,
            ))
            .into_any()
    }
}

/// One phone's capture of a shot, in a frame of that device, which
/// docs.js fills from the screens branch's manifest once script runs.
fn capture(s: &Shot, key: &str, phone: Option<&str>) -> AnyView {
    let (kind, word) = match s.kind {
        Kind::Screenshot => ("screenshot", "Screenshot"),
        Kind::Recording => ("recording", "Recording"),
    };
    let screen = format!(
        "<p class=\"ark-js\">Asking for the latest capture…</p><p class=\"ark-nojs\">The latest capture is fetched by script from the <a href=\"{}manifest.json\">screens branch</a>.</p>",
        screens()
    );
    html::figure()
        .class("capture")
        .attr("data-phone", key.to_string())
        .attr("data-shot", s.id)
        .attr("data-kind", kind)
        .child((
            html::div().class("device").child(html::div().class("screen").inner_html(screen)),
            html::figcaption().child(format!("{}{word} · {}", phone.map(|p| format!("{p} · ")).unwrap_or_default(), s.what)),
            html::small(),
        ))
        .into_any()
}

impl WebApp for DocsWeb {
    type Msg = Msg;

    fn view(&self) -> AnyView {
        let record = self.record();
        arkui_web::shell(docs_shell::view(&record), &|name| self.host(name))
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Go { tag } => {
                let tag = if self.pages.iter().any(|p| p.tag == tag) { tag } else { OVERVIEW.into() };
                let moved = tag != self.page || !self.search.is_empty();
                self.page = tag;
                self.search.clear();
                if moved {
                    browser::went(&pages::href(&self.base, &self.page));
                }
            }
            Msg::Search { text } => {
                self.search = text;
                browser::searched(&self.search);
            }
            Msg::Phone { phone } => {
                self.phone = phone;
                browser::keep("phone", &self.phone);
            }
            Msg::Appearance => browser::next_appearance(),
            Msg::Live(m) => self.live.update(m),
        }
    }

    /// The page is the path; the search is the query's `q`, the reader's.
    fn arrive(&mut self, route: &Route) -> Vec<Msg> {
        let tag = route.seg(0).unwrap_or(OVERVIEW).to_string();
        self.page = if self.pages.iter().any(|p| p.tag == tag) { tag } else { OVERVIEW.into() };
        match route.get("q").filter(|q| !q.is_empty()) {
            Some(q) => vec![Msg::Search { text: q.to_string() }],
            None => vec![],
        }
    }

    fn route(&self) -> Route {
        match self.search.is_empty() {
            true => Self::route_of(&self.page),
            false => Self::route_of(&self.page).with("q", self.search.clone()),
        }
    }

    /// Every page's path: each is written, and no other (the reader's
    /// search is a query, never a page).
    fn static_paths(&self) -> Vec<Route> {
        self.pages.iter().map(|p| Self::route_of(&p.tag)).collect()
    }

    fn rewind(&mut self) -> bool {
        self.page = OVERVIEW.into();
        self.search.clear();
        self.phone.clear();
        self.live = Live::default();
        true
    }
}

/// What the browser keeps of the reader: the address as pages are walked
/// and searched, and the reader's choices. Nothing, written natively.
pub mod browser {
    #[cfg(target_arch = "wasm32")]
    fn window() -> Option<web_sys::Window> {
        web_sys::window()
    }

    /// The address moved to `href`, a page walked to.
    pub fn went(href: &str) {
        #[cfg(target_arch = "wasm32")]
        if let Some(h) = window().and_then(|w| w.history().ok()) {
            let _ = h.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(href));
            if let Some(d) = window().and_then(|w| w.document()) {
                d.set_title(&super::title_of(href));
            }
        }
        let _ = href;
    }

    /// The address's `?q=` kept to what is searched.
    pub fn searched(q: &str) {
        #[cfg(target_arch = "wasm32")]
        if let (Some(h), Some(l)) = (window().and_then(|w| w.history().ok()), window().map(|w| w.location())) {
            let path = l.pathname().unwrap_or_default();
            let to = match q.is_empty() {
                true => path,
                false => format!("{path}?q={}", super::encode(q)),
            };
            let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&to));
        }
        let _ = q;
    }

    /// `key` kept as `value` for the next page.
    pub fn keep(key: &str, value: &str) {
        #[cfg(target_arch = "wasm32")]
        if let Some(s) = window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item(&format!("docs-{key}"), value);
        }
        let _ = (key, value);
    }

    /// What was kept under `key`.
    pub fn kept(key: &str) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            return window()
                .and_then(|w| w.local_storage().ok().flatten())
                .and_then(|s| s.get_item(&format!("docs-{key}")).ok().flatten())
                .filter(|v| !v.is_empty());
        }
        #[allow(unreachable_code)]
        {
            let _ = key;
            None
        }
    }

    /// The system's appearance, then light, then dark, then the system's.
    pub fn next_appearance() {
        #[cfg(target_arch = "wasm32")]
        if let Some(root) = window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
            let next = match root.get_attribute("data-theme").as_deref() {
                None => "light",
                Some("light") => "dark",
                _ => "",
            };
            match next {
                "" => {
                    let _ = root.remove_attribute("data-theme");
                }
                t => {
                    let _ = root.set_attribute("data-theme", t);
                }
            }
            keep("tone", next);
        }
    }
}

/// The `<title>` of the page at `href`, as the written page has it.
pub fn title_of(href: &str) -> String {
    let tag = href.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let name = COMPONENTS
        .iter()
        .find(|c| c.id == tag)
        .map(|c| c.name)
        .or((tag == ALL).then_some("All Pages"));
    match name {
        Some(n) => format!("{n} · ArkUI"),
        None => "ArkUI".into(),
    }
}

/// `s` as a query's value.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
