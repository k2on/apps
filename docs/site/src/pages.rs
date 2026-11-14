//! The pages, as words: each one's title, description and writing (as
//! Markdown, drawn by `ark_app::writing`), made from the catalogue, and
//! what the native build worked out that the browser cannot ([`Built`]:
//! the pictures' sizes, the tables of what arkui's functions answer).
use serde::{Deserialize, Serialize};

use crate::catalogue::{self, ENTRIES};
use crate::components::{Component, Mobile, Section, COMPONENTS};

/// Where the source is read, for a module's link.
pub const SOURCE: &str = "https://github.com/k2on/apps/blob/main/";

/// The overview's tag and the index of every page's.
pub const OVERVIEW: &str = "overview";
pub const ALL: &str = "all";

/// What the native build worked out, carried in each written page for the
/// module to draw the same page from (`#docs-built`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Built {
    pub components: Vec<BuiltComponent>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BuiltComponent {
    pub id: String,
    pub pictures: Vec<Pic>,
    pub answers: Vec<Answer>,
    pub notes: Vec<String>,
}

/// A picture of a component, drawn four times: its logical sizes, by
/// [`VARIANTS`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Pic {
    pub name: String,
    pub caption: String,
    pub sizes: Vec<(u32, u32)>,
}

/// Each picture's four drawings, in [`Pic::sizes`]' order.
pub const VARIANTS: [&str; 4] = ["light-wide", "light-narrow", "dark-wide", "dark-narrow"];

/// What a component's functions answered when the site was built.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Answer {
    pub caption: String,
    pub rows: Vec<(String, String)>,
}

impl Built {
    pub fn of(&self, id: &str) -> Option<&BuiltComponent> {
        self.components.iter().find(|c| c.id == id)
    }
}

/// A page: what the sidebar, the toolbar, the `<head>` and the search
/// know of it, and its writing.
#[derive(Clone, Debug)]
pub struct Page {
    pub tag: String,
    pub title: String,
    pub glyph: String,
    pub section: Option<Section>,
    /// The section this page is the index of: its own page, a row for
    /// each of its pages.
    pub index: Option<Section>,
    /// A sentence for `<meta name="description">` and a search's result.
    pub description: String,
    /// The writing above the examples.
    pub intro: String,
    /// The writing under the pictures: each module's API and rules.
    pub reference: String,
    /// Every word of it, lowercase, for the search.
    pub words: String,
}

/// The sidebar's glyph for a component, of ArkUI's shared names.
pub fn glyph(id: &str) -> &'static str {
    match id {
        "theme" => "droplet",
        "style" => "pencil",
        "icons" => "badge-check",
        "text" | "writing" => "list",
        "art" => "layers",
        "navigation" => "panel-right",
        "lists" => "list-filter",
        "reorder" => "arrow-left-right",
        "cards" => "credit-card",
        "headers" => "landmark",
        "empty" => "inbox",
        "account" => "users",
        "slider" => "minus",
        "menu" => "ellipsis-vertical",
        "command" => "search",
        "picker" => "check",
        "dialog" => "mail",
        "expanding-card" => "plus",
        "glass" => "cloud",
        "pages" => "copy",
        "springs" => "zap",
        "scroll" => "chevron-down",
        "keys" => "wrench",
        "routes" => "link",
        "waking" => "clock",
        _ => "circle",
    }
}

/// `s` with its backticks gone: a description is plain words.
fn bare(s: &str) -> String {
    s.replace('`', "")
}

/// Where a page is, under `base`: its route's URL there.
pub fn href(base: &str, tag: &str) -> String {
    crate::routes::Route::of(tag).address().url_under(base)
}

/// Every page, in the sidebar's order: the overview, the index, then each
/// component.
pub fn pages(base: &str) -> Vec<Page> {
    let mut out = vec![overview(base), all(base)];
    for section in Section::ALL {
        out.extend(COMPONENTS.iter().filter(|c| c.section == section).map(component));
    }
    out.extend(Section::ALL.into_iter().map(index));
    for p in &mut out {
        p.words = format!("{} {} {} {}", p.title, p.description, p.intro, p.reference).to_lowercase();
    }
    out
}

/// The pages in reading order, for each page's way to the one before and
/// the one after: the overview, then each section's own page and its
/// pages. The index of every page is the search's, not a page to read.
pub fn reading(pages: &[Page]) -> Vec<&Page> {
    let mut out: Vec<&Page> = pages.iter().filter(|p| p.tag == OVERVIEW).collect();
    for s in Section::ALL {
        out.extend(pages.iter().filter(|p| p.index == Some(s)));
        out.extend(pages.iter().filter(|p| p.section == Some(s)));
    }
    out
}

/// A section's own page: what it is, and a row for each of its pages
/// (`DocsWeb`'s `index`, hosted under the words).
fn index(s: Section) -> Page {
    Page {
        tag: s.tag().into(),
        title: s.heading().into(),
        glyph: s.glyph().into(),
        section: None,
        index: Some(s),
        description: s.about().into(),
        intro: format!("{}\n", s.about()),
        reference: String::new(),
        words: String::new(),
    }
}

fn overview(base: &str) -> Page {
    let captured: usize = COMPONENTS.iter().map(|c| c.shots.len()).sum();
    let mut md = format!(
        "The UI every app here shares, by component: iced components, the vim keyboard, theming, the glyph table, a picture cache and the address bar — and nothing of any app.\n\n\
         Each page says how to use a component on the desktop, where every picture is the arkui component itself, drawn by the renderer the apps use; in the browser, where the examples are arkui-web's own parts, live; and on a phone, Android or iOS, with the latest capture from a real emulator and simulator in a frame of that device.\n\n\
         This is the one place the components are described. An agent working on an app reads the page for the part it needs; one that changes a component changes its entry in `docs/site/src/components.rs` (and the module's in `catalogue.rs`, its pictures in `examples.rs`) in the same commit, and the tests fail until it does. Other sites pull [catalogue.json]({base}catalogue.json), published beside this page.\n\n\
         {} components over {} arkui modules, and {captured} captures from the phones. Every page is plain HTML that works without script; with it, the page is an ArkUI app.\n\n\
         ## Components\n\n\
         | Component | Section | Desktop | Android | iOS |\n|---|---|---|---|---|\n",
        COMPONENTS.len(),
        ENTRIES.len()
    );
    let yes = |on: bool| if on { "Yes" } else { "—" };
    for c in COMPONENTS {
        let desktop = match c.desktop.is_empty() {
            true => "—".to_string(),
            false => c.desktop.iter().map(|m| format!("`arkui::{m}`")).collect::<Vec<_>>().join(", "),
        };
        md.push_str(&format!(
            "| [{}]({}) | {} | {desktop} | {} | {} |\n",
            c.name,
            href(base, c.id),
            c.section.heading(),
            yes(c.android.is_some()),
            yes(c.ios.is_some()),
        ));
    }
    Page {
        tag: OVERVIEW.into(),
        title: "ArkUI".into(),
        glyph: "house".into(),
        section: None,
        index: None,
        description:
            "The components every Ark app shares, a page each: arkui on the desktop, arkui-web in the browser, Compose and SwiftUI on the phones."
                .into(),
        intro: md,
        reference: String::new(),
        words: String::new(),
    }
}

/// Every page, by section: the index a search without script leads to.
fn all(base: &str) -> Page {
    let mut md = String::from("Every page of the docs, by section. With script, the search field finds words on any of them.\n\n");
    md.push_str(&format!("- [Overview]({base})\n"));
    for section in Section::ALL {
        md.push_str(&format!("\n## [{}]({})\n\n", section.heading(), href(base, section.tag())));
        for c in COMPONENTS.iter().filter(|c| c.section == section) {
            md.push_str(&format!("- [{}]({}): {}\n", c.name, href(base, c.id), c.summary));
        }
    }
    Page {
        tag: ALL.into(),
        title: "All Pages".into(),
        glyph: "list".into(),
        section: None,
        index: None,
        description: "Every page of the ArkUI docs, by section.".into(),
        intro: md,
        reference: String::new(),
        words: String::new(),
    }
}

fn component(c: &Component) -> Page {
    let mut intro = format!("{}\n\n", c.summary);
    match c.desktop.is_empty() {
        true => intro.push_str(&format!("arkui has no {} on the desktop yet.\n", c.name.to_lowercase())),
        false => intro.push_str(&format!(
            "On the desktop: {}.\n",
            c.desktop.iter().map(|m| format!("`arkui::{m}`")).collect::<Vec<_>>().join(", ")
        )),
    }
    let mut reference = String::new();
    if !c.desktop.is_empty() {
        reference.push_str("## Modules\n\n");
    }
    for m in c.desktop {
        let e = catalogue::find(m).expect("a component's module is in the catalogue");
        reference.push_str(&format!("### arkui::{}\n\n{}\n\n", e.module, e.summary));
        for a in e.api {
            reference.push_str(&format!("- `{}`\n", a.replace('`', "")));
        }
        if !e.rules.is_empty() {
            reference.push_str("\n#### Rules\n\n");
            for r in e.rules {
                reference.push_str(&format!("- {r}\n"));
            }
        }
        let path = e.source();
        reference.push_str(&format!("\nSource: [{path}]({SOURCE}{path})\n\n"));
    }
    Page {
        tag: c.id.into(),
        title: c.name.into(),
        glyph: glyph(c.id).into(),
        section: Some(c.section),
        index: None,
        description: bare(c.summary),
        intro,
        reference,
        words: String::new(),
    }
}

/// A phone's half of a component as writing: where it is, and how.
pub fn mobile(half: &Mobile) -> String {
    let mut md = String::from("### Where\n\n");
    for a in half.at {
        md.push_str(&format!("- `{}`\n", a.replace('`', "")));
    }
    md.push_str("\n### How\n\n");
    for h in half.how {
        md.push_str(&format!("- {h}\n"));
    }
    md
}
