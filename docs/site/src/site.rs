//! The site, written out: an overview, a page per component, the pictures
//! they show, a stylesheet whose colours are arkui's own, and a few lines of
//! script (`web/site.js`) for what HTML cannot do alone — remembering the
//! theme and the phone, and fetching the phones' latest captures.
//!
//! Every page is whole HTML, so a load is that page and nothing it does not
//! show: the stylesheet and the script are shared and cached, and a picture
//! is fetched only in the theme and at the width it is shown at.
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use arkui::{glyphs, style, theme};
use iced::{Color, Theme};

use crate::catalogue::{self, ENTRIES};
use crate::components::{Component, Kind, Mobile, Section, Shot, COMPONENTS};
use crate::examples::{self, Canvas};
use crate::render::Painter;

/// Where the source is read, for a module's link.
const SOURCE: &str = "https://github.com/k2on/apps/blob/main/";

/// The two widths a picture is drawn at, in logical pixels: the page's
/// measure, and a phone's less its gutters.
pub const WIDE: f32 = 640.0;
pub const NARROW: f32 = 320.0;
/// Below this the page is a phone's: the narrow pictures, and the sidebar
/// a dropdown. In `web/style.css` too.
const PHONE_WIDTH: u32 = 700;

/// Where the phones' captures are read from: the `screens` branch on
/// GitHub, or wherever `DOCS_SCREENS` said when this was built (a local
/// directory served for trying the page, say).
pub fn screens() -> &'static str {
    option_env!("DOCS_SCREENS").unwrap_or("https://raw.githubusercontent.com/k2on/apps/screens/")
}

/// Write the site into `out`; with `pictures` false, every page but none of
/// the pictures (the tests' check of the pages).
pub fn write(out: &Path, pictures: bool) -> io::Result<()> {
    fs::create_dir_all(out.join("pictures"))?;
    fs::create_dir_all(out.join("glyphs"))?;
    let css = format!("{}\n{}", colours(), include_str!("../web/style.css"));
    let js = include_str!("../web/site.js");
    let v = Version {
        css: format!("{:08x}", arkui::art::hash(&css)),
        js: format!("{:08x}", arkui::art::hash(js)),
    };
    fs::write(out.join("style.css"), &css)?;
    fs::write(out.join("site.js"), js)?;
    fs::write(out.join("favicon.svg"), include_str!("../web/favicon.svg"))?;
    fs::write(out.join("catalogue.json"), catalogue::json())?;
    fs::write(out.join("glyphs/overview.svg"), glyphs::HOME)?;
    fs::write(out.join("glyphs/down.svg"), glyphs::DOWN)?;
    for c in COMPONENTS {
        fs::write(out.join(format!("glyphs/{}.svg", c.id)), c.glyph)?;
    }

    let mut painter = pictures.then(Painter::new);
    let mut sizes = Sizes::default();
    for c in COMPONENTS {
        for e in examples::pictures(c.id) {
            for (tone, theme) in [("light", Theme::Light), ("dark", Theme::Dark)] {
                for (wide, width) in [("wide", WIDE), ("narrow", NARROW)] {
                    let size = match &mut painter {
                        Some(p) => {
                            let picture = (e.draw)(Canvas::new(p, &theme, width));
                            fs::write(out.join(format!("pictures/{}-{tone}-{wide}.webp", e.name)), picture.webp())?;
                            picture.logical()
                        }
                        None => (width as u32, 0),
                    };
                    sizes.0.push((format!("{}-{tone}-{wide}", e.name), size));
                }
            }
        }
    }

    fs::write(out.join("index.html"), page(&v, None, "arkui", &overview()))?;
    for c in COMPONENTS {
        fs::write(
            out.join(format!("{}.html", c.id)),
            page(&v, Some(c), &format!("{} · arkui", c.name), &component(c, &sizes)),
        )?;
    }
    Ok(())
}

struct Version {
    css: String,
    js: String,
}

/// Each picture's logical size, by its file's stem.
#[derive(Default)]
struct Sizes(Vec<(String, (u32, u32))>);

impl Sizes {
    fn of(&self, stem: &str) -> (u32, u32) {
        self.0.iter().find(|(s, _)| s == stem).map(|(_, size)| *size).unwrap_or((0, 0))
    }
}

/// The stylesheet's colours, from arkui's palette in iced's two themes, so
/// the page around a picture is the ground it was drawn on.
fn colours() -> String {
    let hex = |c: Color| {
        let [r, g, b, a] = c.into_rgba8();
        match a {
            255 => format!("#{r:02x}{g:02x}{b:02x}"),
            _ => format!("rgba({r},{g},{b},{:.2})", a as f32 / 255.0),
        }
    };
    let vars = |t: &Theme| {
        let p = theme::of(t);
        let text = p.background.base.text;
        [
            ("bg", p.background.base.color),
            ("weak", p.background.weak.color),
            ("strong", p.background.strong.color),
            ("surface", style::surface_color(t)),
            ("edge", style::edge_color(t)),
            ("text", text),
            ("dim", style::dim(t).color.unwrap_or(text)),
            ("faint", style::faint(0.6)(t).color.unwrap_or(text)),
            ("accent", style::accent(t).color.unwrap_or(p.primary.base.color)),
            ("primary", p.primary.base.color),
            ("on-primary", p.primary.base.text),
        ]
        .iter()
        .map(|(name, c)| format!("--{name}:{};", hex(*c)))
        .collect::<String>()
    };
    let (light, dark) = (vars(&Theme::Light), vars(&Theme::Dark));
    format!(
        "/* arkui's palette, written by docs/site/src/site.rs. */\n\
         :root{{color-scheme:light;{light}}}\n\
         @media (prefers-color-scheme:dark){{:root:not([data-theme=light]){{color-scheme:dark;{dark}}}}}\n\
         :root[data-theme=dark]{{color-scheme:dark;{dark}}}\n"
    )
}

/// `s` safe in HTML text and attributes.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Prose, with `backticked` spans as code.
fn prose(s: &str) -> String {
    esc(s)
        .split('`')
        .enumerate()
        .map(|(i, part)| match i % 2 {
            1 => format!("<code>{part}</code>"),
            _ => part.to_string(),
        })
        .collect()
}

fn page(v: &Version, here: Option<&Component>, title: &str, main: &str) -> String {
    let mut nav = String::new();
    let link = |nav: &mut String, href: &str, glyph: &str, name: &str, on: bool| {
        let current = if on { " aria-current=\"page\"" } else { "" };
        let _ = write!(
            nav,
            "<a href=\"{href}\"{current}><i style=\"--g:url(glyphs/{glyph}.svg)\"></i>{}</a>",
            esc(name)
        );
    };
    nav.push_str("<h2>arkui</h2>");
    link(&mut nav, "index.html", "overview", "Overview", here.is_none());
    for section in Section::ALL {
        let _ = write!(nav, "<h2>{}</h2>", section.heading());
        for c in COMPONENTS.iter().filter(|c| c.section == section) {
            link(&mut nav, &format!("{}.html", c.id), c.id, c.name, here.is_some_and(|h| h.id == c.id));
        }
    }
    let place = here.map(|c| c.name).unwrap_or("Overview");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="screens" content="{screens}">
<link rel="icon" href="favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="style.css?v={css}">
<link rel="alternate" type="application/json" href="catalogue.json">
<script>try{{var t=localStorage.getItem("tone");if(t)document.documentElement.dataset.theme=t}}catch(e){{}}</script>
<script src="site.js?v={js}" defer></script>
</head>
<body>
<input type="checkbox" id="places" hidden>
<header>
<label for="places" class="places"><i style="--g:url(glyphs/{glyph}.svg)"></i>{place}<i class="down"></i></label>
<a class="brand" href="index.html">arkui</a>
<select id="tone" aria-label="Theme"><option value="">Theme: System</option><option value="light">Theme: Light</option><option value="dark">Theme: Dark</option></select>
</header>
<nav>{nav}</nav>
<main>
{main}
</main>
</body>
</html>
"#,
        title = esc(title),
        screens = esc(screens()),
        css = v.css,
        js = v.js,
        glyph = here.map(|c| c.id).unwrap_or("overview"),
        place = esc(place),
    )
}

fn overview() -> String {
    let captured: usize = COMPONENTS.iter().map(|c| c.shots.len()).sum();
    let mut out = String::new();
    let _ = write!(
        out,
        "<h1>arkui</h1>\
         <p class=\"lede\">The ui every app here shares, by component: iced components, the vim keyboard, theming, the glyph table, a picture cache and the address bar — and nothing of any app.</p>\
         <p>Each page says how to use a component on the desktop, where every picture is the arkui component itself, drawn by the renderer the apps use, and on a phone, Android or iOS, with the latest capture from a real emulator and simulator in a frame of that device.</p>\
         <p>This is the one place the components are described. An agent working on an app reads the page for the part it needs; one that changes a component changes its entry in <code>docs/site/src/components.rs</code> (and the module's in <code>catalogue.rs</code>, its pictures in <code>examples.rs</code>) in the same commit, and the tests fail until it does. Other sites pull <a href=\"catalogue.json\">catalogue.json</a>, published beside this page.</p>\
         <p class=\"faint\">{} components over {} arkui modules, and {captured} captures from the phones. j and k walk the pages.</p>\
         <h2>Components</h2>\
         <table class=\"list\"><thead><tr><th>Component</th><th class=\"wide\">Section</th><th>Desktop</th><th class=\"wide\">Android</th><th class=\"wide\">iOS</th></tr></thead><tbody>",
        COMPONENTS.len(),
        ENTRIES.len()
    );
    let yes = |on: bool| if on { "yes" } else { "<span class=\"faint\">—</span>" };
    for c in COMPONENTS {
        let desktop = match c.desktop.is_empty() {
            true => "<span class=\"faint\">—</span>".to_string(),
            false => c.desktop.iter().map(|m| format!("arkui::{m}")).collect::<Vec<_>>().join(", "),
        };
        let _ = write!(
            out,
            "<tr><td><a href=\"{id}.html\">{name}</a></td><td class=\"wide dim\">{section}</td><td class=\"dim\">{desktop}</td><td class=\"wide\">{android}</td><td class=\"wide\">{ios}</td></tr>",
            id = c.id,
            name = esc(c.name),
            section = c.section.heading(),
            android = yes(c.android.is_some()),
            ios = yes(c.ios.is_some()),
        );
    }
    out.push_str("</tbody></table>");
    // An old address, `#menu` or a module's `#context`, lands on its page.
    let mut names: Vec<(&str, &str)> = COMPONENTS.iter().map(|c| (c.id, c.id)).collect();
    names.extend(COMPONENTS.iter().flat_map(|c| c.desktop.iter().map(move |m| (*m, c.id))));
    let map = names
        .iter()
        .map(|(from, to)| format!("\"{from}\":\"{to}\""))
        .collect::<Vec<_>>()
        .join(",");
    let _ = write!(
        out,
        "<script>var to={{{map}}}[location.hash.slice(1)];if(to)location.replace(to+\".html\")</script>"
    );
    out
}

fn list(lines: &[&str], code: bool) -> String {
    let items: String = lines
        .iter()
        .map(|l| match code {
            true => format!("<li><code>{}</code></li>", esc(l)),
            false => format!("<li>{}</li>", prose(l)),
        })
        .collect();
    format!("<ul>{items}</ul>")
}

fn component(c: &Component, sizes: &Sizes) -> String {
    let mut out = String::new();
    let _ = write!(out, "<h1>{}</h1><p class=\"lede\">{}</p>", esc(c.name), prose(c.summary));
    out.push_str("<h2>Desktop</h2>");
    if c.desktop.is_empty() {
        let _ = write!(out, "<p class=\"faint\">arkui has no {} yet.</p>", esc(&c.name.to_lowercase()));
    } else {
        let _ = write!(
            out,
            "<p class=\"modules\">{}</p>",
            c.desktop
                .iter()
                .map(|m| format!("<code>arkui::{m}</code>"))
                .collect::<Vec<_>>()
                .join(" · ")
        );
        for e in examples::pictures(c.id) {
            out.push_str(&figure(e, sizes));
        }
        for a in examples::answers(c.id) {
            let _ = write!(out, "<table class=\"answers\"><caption>{}</caption><tbody>", esc(a.caption));
            for (call, answer) in &a.rows {
                let _ = write!(out, "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>", esc(call), esc(answer));
            }
            out.push_str("</tbody></table>");
        }
        for n in examples::notes(c.id) {
            let _ = write!(out, "<p class=\"dim\">{}</p>", prose(n));
        }
        for m in c.desktop {
            let e = catalogue::find(m).expect("a component's module is in the catalogue");
            let path = e.source();
            let _ = write!(
                out,
                "<section class=\"module\"><h3>arkui::{module}</h3><p class=\"dim\">{summary}</p>{api}",
                module = e.module,
                summary = prose(e.summary),
                api = list(e.api, true),
            );
            if !e.rules.is_empty() {
                let _ = write!(out, "<h4>Rules</h4>{}", list(e.rules, false));
            }
            let _ = write!(out, "<p class=\"source\"><a href=\"{SOURCE}{path}\">{path}</a></p></section>");
        }
    }
    out.push_str(
        "<section class=\"mobile\"><div class=\"mobile-head\"><h2>Mobile</h2><div class=\"switch\">\
         <input type=\"radio\" name=\"phone\" id=\"android\" value=\"android\" checked><label for=\"android\">Android</label>\
         <input type=\"radio\" name=\"phone\" id=\"ios\" value=\"ios\"><label for=\"ios\">iOS</label></div></div>",
    );
    for (key, name, half) in [("android", "Android", c.android.as_ref()), ("ios", "iOS", c.ios.as_ref())] {
        let _ = write!(out, "<div class=\"pane {key}\">");
        out.push_str(&phone(c, key, name, half));
        out.push_str("</div>");
    }
    out.push_str("</section>");
    out
}

/// One picture of a component, in the page's theme and at its width.
fn figure(e: &examples::Example, sizes: &Sizes) -> String {
    let mut out = String::from("<figure class=\"example\">");
    for tone in ["light", "dark"] {
        let (nw, nh) = sizes.of(&format!("{}-{tone}-narrow", e.name));
        let (ww, wh) = sizes.of(&format!("{}-{tone}-wide", e.name));
        let _ = write!(
            out,
            "<picture class=\"{tone}\"><source media=\"(max-width: {PHONE_WIDTH}px)\" srcset=\"pictures/{name}-{tone}-narrow.webp\" width=\"{nw}\" height=\"{nh}\">\
             <img src=\"pictures/{name}-{tone}-wide.webp\" width=\"{ww}\" height=\"{wh}\" alt=\"{alt}\" loading=\"lazy\" decoding=\"async\"></picture>",
            name = e.name,
            alt = esc(e.caption),
        );
    }
    let _ = write!(out, "<figcaption>{}</figcaption></figure>", prose(e.caption));
    out
}

fn phone(c: &Component, key: &str, name: &str, half: Option<&Mobile>) -> String {
    let Some(half) = half else {
        return format!("<p class=\"faint\">Nothing shared for this on {name} yet.</p>");
    };
    let mut out = String::new();
    let shots: Vec<&Shot> = c.shots.iter().filter(|s| s.phones.contains(&key)).collect();
    if !shots.is_empty() {
        out.push_str("<div class=\"captures\">");
        for s in shots {
            let kind = match s.kind {
                Kind::Screenshot => ("screenshot", "Screenshot"),
                Kind::Recording => ("recording", "Recording"),
            };
            let _ = write!(
                out,
                "<figure class=\"capture\" data-phone=\"{key}\" data-shot=\"{id}\" data-kind=\"{k}\">\
                 <div class=\"device\"><div class=\"screen\"><p>Asking for the latest capture…</p></div></div>\
                 <figcaption>{kind} · {what}</figcaption><small></small></figure>",
                id = s.id,
                k = kind.0,
                kind = kind.1,
                what = esc(s.what),
            );
        }
        out.push_str("</div>");
    }
    let _ = write!(out, "<h3>Where</h3>{}<h3>How</h3>{}", list(half.at, true), list(half.how, false));
    out
}
