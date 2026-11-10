//! The site, written out: every page of the app as static HTML
//! (`arkui_web::prerender`, every band of width at once), the desktop
//! pictures drawn by arkui itself, sitemap.xml, catalogue.json, and the
//! old addresses (`menu.html`) sent on to the new (`menu/`).
use std::fs;
use std::io;
use std::path::Path;

use arkui_web::page::{Mode, Route};
use arkui_web::{document, prerender, WebApp, Written};
use iced::Theme;

use crate::app::{screens, DocsWeb};
use crate::catalogue;
use crate::components::COMPONENTS;
use crate::examples::{self, Canvas};
use crate::pages::{Answer, Built, BuiltComponent, Pic, VARIANTS};
use crate::render::Painter;

/// The two widths a picture is drawn at, in logical pixels: the page's
/// measure, and a phone's less its gutters.
pub const WIDE: f32 = 640.0;
pub const NARROW: f32 = 320.0;

/// The docs' own styles, after ArkUI's and its writing's.
pub const DOCS_CSS: &str = include_str!("../web/docs.css");
pub const DOCS_JS: &str = include_str!("../web/docs.js");

/// How the site is written.
pub struct Options {
    /// Where it is served from: `/`, or `/apps/docs/`.
    pub base: String,
    /// Where it is published, for the sitemap: `https://k2on.github.io`.
    pub site: String,
    /// With the pictures (false: the pages alone, the tests').
    pub pictures: bool,
    /// The module's `?v=`: the build's own hash, `dev` by hand.
    pub version: String,
}

/// What the native build works out for the browser: each component's
/// pictures (drawn into `out/pictures` when `painter` is given), the tables
/// of what its functions answer, and its notes.
fn built(out: &Path, mut painter: Option<&mut Painter>) -> io::Result<Built> {
    let mut built = Built::default();
    for c in COMPONENTS {
        let mut pictures = vec![];
        for e in examples::pictures(c.id) {
            let mut sizes = vec![];
            for v in VARIANTS {
                let theme = if v.starts_with("light") { Theme::Light } else { Theme::Dark };
                let width = if v.ends_with("wide") { WIDE } else { NARROW };
                let size = match painter.as_deref_mut() {
                    Some(p) => {
                        let picture = (e.draw)(Canvas::new(p, &theme, width));
                        fs::write(out.join(format!("pictures/{}-{v}.webp", e.name)), picture.webp())?;
                        picture.logical()
                    }
                    None => (width as u32, 0),
                };
                sizes.push(size);
            }
            pictures.push(Pic {
                name: e.name.into(),
                caption: e.caption.into(),
                sizes,
            });
        }
        built.components.push(BuiltComponent {
            id: c.id.into(),
            pictures,
            answers: examples::answers(c.id)
                .into_iter()
                .map(|a| Answer {
                    caption: a.caption.into(),
                    rows: a.rows,
                })
                .collect(),
            notes: examples::notes(c.id).iter().map(|n| n.to_string()).collect(),
        });
    }
    Ok(built)
}

/// `s` safe in an HTML attribute or text.
fn esc(s: &str) -> String {
    arkui_web::page::escape(s)
}

/// Write the site into `out`; the paths written, each a page's.
pub fn write(out: &Path, o: &Options) -> io::Result<Vec<String>> {
    fs::create_dir_all(out.join("pictures"))?;
    fs::write(out.join("favicon.svg"), include_str!("../web/favicon.svg"))?;
    fs::write(out.join("catalogue.json"), catalogue::json())?;
    let js_v = format!("{:08x}", hash(DOCS_JS));
    fs::write(out.join("docs.js"), DOCS_JS)?;

    let mut painter = o.pictures.then(Painter::new);
    let built = built(out, painter.as_mut())?;
    let data = serde_json::to_string(&built).expect("plain data").replace("</", "<\\/");

    let base = o.base.clone();
    let open = || DocsWeb::open(&base, built.clone());
    let app = open();
    let paths = app.static_paths();
    let titles: Vec<(Route, String, String)> = app
        .pages()
        .iter()
        .map(|p| {
            let title = match p.tag.as_str() {
                crate::pages::OVERVIEW => "ArkUI".to_string(),
                _ => format!("{} · ArkUI", p.title),
            };
            (DocsWeb::route_of(&p.tag), title, p.description.clone())
        })
        .collect();
    let css = format!("{}\n{}", arkui_web::writing::WRITING_CSS, DOCS_CSS);
    let args = vec![("base".to_string(), base.clone())];
    let page = |r: &Route, view: &str| {
        let (_, title, description) = titles.iter().find(|(t, _, _)| t == r).cloned().unwrap_or_default();
        let canonical = format!("{}{}{}", o.site, base.trim_end_matches('/'), r.path_url());
        let head = format!(
            "<title>{title}</title>\n<meta name=\"description\" content=\"{description}\">\n<link rel=\"canonical\" href=\"{canonical}\">\n\
             <meta name=\"screens\" content=\"{screens}\">\n<link rel=\"icon\" href=\"{base}favicon.svg\" type=\"image/svg+xml\">\n\
             <link rel=\"alternate\" type=\"application/json\" href=\"{base}catalogue.json\">\n\
             <script>try{{var t=localStorage.getItem(\"docs-tone\");if(t)document.documentElement.dataset.theme=t}}catch(e){{}}</script>\n\
             <script type=\"application/json\" id=\"docs-built\">{data}</script>\n\
             <script src=\"{base}docs.js?v={js_v}\" defer></script>\n\
             <script type=\"module\">import init from \"{base}pkg/docs-web.js?v={v}\"; init({{ module_or_path: \"{base}pkg/docs-web_bg.wasm?v={v}\" }});</script>",
            title = esc(&title),
            description = esc(&description),
            canonical = esc(&canonical),
            screens = esc(screens()),
            v = o.version,
        );
        document(
            &Written {
                mode: Mode::Static,
                route: r.clone(),
                base: base.clone(),
                args: args.clone(),
                width: None,
                accent: "blue".into(),
                css: css.clone(),
                head,
                ..Written::default()
            },
            view,
        )
    };
    // Every page's path and no other: the reader's search and choices are
    // the module's (ui/docs/app.md, "The Browser's Modes").
    let written = prerender(&open, paths.len(), None, &args, &page).map_err(io::Error::other)?;
    let mut out_paths = vec![];
    for (r, html) in &written {
        let dir = r.path.iter().fold(out.to_path_buf(), |d, s| d.join(s));
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("index.html"), html)?;
        out_paths.push(r.path_url());
    }
    // No link on a written page to a page that is not written.
    let urls: Vec<(String, String)> = written
        .iter()
        .map(|(r, h)| (format!("{}{}", base.trim_end_matches('/'), r.path_url()), h.clone()))
        .collect();
    let dangling = arkui_web::page::dangling(&urls, &base);
    if !dangling.is_empty() {
        return Err(io::Error::other(format!("links to no page: {dangling:?}")));
    }
    // The old addresses, `menu.html`, and their fragments, `#menu`.
    for c in COMPONENTS {
        let to = format!("{base}{}/", c.id);
        fs::write(
            out.join(format!("{}.html", c.id)),
            format!(
                "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>{name} · ArkUI</title>\
                 <link rel=\"canonical\" href=\"{to}\"><meta http-equiv=\"refresh\" content=\"0; url={to}\"></head>\
                 <body><p>This page is now at <a href=\"{to}\">{to}</a>.</p></body></html>\n",
                name = esc(c.name),
            ),
        )?;
    }
    fs::write(
        out.join("sitemap.xml"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{}</urlset>\n",
            out_paths
                .iter()
                .map(|p| format!("  <url><loc>{}{}{p}</loc></url>\n", o.site, base.trim_end_matches('/')))
                .collect::<String>()
        ),
    )?;
    fs::write(
        out.join("robots.txt"),
        format!("User-agent: *\nAllow: /\nSitemap: {}{}sitemap.xml\n", o.site, base),
    )?;
    Ok(out_paths)
}

fn hash(s: &str) -> u32 {
    arkui::art::hash(s)
}
