//! The catalogue held to the code it describes, and the site's links,
//! routes and reading order to themselves.
use std::path::{Path, PathBuf};

use crate::catalogue;
use crate::components::{Section, COMPONENTS};

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Falsified by giving Cards' shot Lists' id, or Menu's picture Panel's name.
#[test]
fn a_shot_a_picture_and_a_page_are_each_one_name() {
    let mut shots: Vec<&str> = COMPONENTS.iter().flat_map(|c| c.shots.iter().map(|s| s.id)).collect();
    let n = shots.len();
    shots.sort_unstable();
    shots.dedup();
    assert_eq!(shots.len(), n, "two components capture under one id");
    let mut pictures: Vec<&str> = COMPONENTS
        .iter()
        .flat_map(|c| crate::examples::pictures(c.id).iter().map(|e| e.name))
        .collect();
    let n = pictures.len();
    pictures.sort_unstable();
    pictures.dedup();
    assert_eq!(pictures.len(), n, "two pictures under one name");
    for (i, c) in COMPONENTS.iter().enumerate() {
        assert!(COMPONENTS[..i].iter().all(|d| d.id != c.id), "two components are {}", c.id);
        assert_ne!(c.id, "index", "a component's page would be the overview");
    }
}

/// Falsified by changing a summary without regenerating (fails: stale).
#[test]
fn catalogue_json_is_what_the_catalogue_prints() {
    let at = Path::new(env!("CARGO_MANIFEST_DIR")).join("catalogue.json");
    if std::env::var_os("DOCS_WRITE_CATALOGUE").is_some() {
        std::fs::write(&at, catalogue::json()).unwrap();
    }
    assert!(
        read(&at) == catalogue::json(),
        "docs/site/catalogue.json is stale: in docs/site, DOCS_WRITE_CATALOGUE=1 cargo test catalogue_json"
    );
}

/// The site as its generated entry point says it (ui/prebuild.py's `web`,
/// from app.toml), its pictures not drawn: what the tests export and drive.
struct Site;

impl arkui_web::WebHosted for Site {
    type App = crate::app::Docs;
    const ID: &'static str = "docs";
    const NAME: &'static str = "ArkUI";
    const SCHEME: &'static str = "docs";
    const VERSION: &'static str = "0.1";
    const ACCENT: &'static str = "blue";
    const MARK: &'static str = include_str!("../icon.svg");
    const CSS: &'static str = include_str!("../public/docs.css");
    const HEAD: &'static str = include_str!("../public/head.html");
    const FILES: &'static [(&'static str, &'static [u8])] = &[("docs.js", include_bytes!("../public/docs.js"))];
    const STATIC: bool = true;

    fn prepare(out: &Path) -> std::io::Result<String> {
        crate::export::unpainted(out)
    }
}

/// The site at its overview, as a page in the browser opens it.
fn site() -> arkui_web::WebHost<Site> {
    let here = arkui_web::host::Here {
        today: "2026-11-10".into(),
        base: "/".into(),
        prepared: String::new(),
    };
    arkui_web::WebHost::over(None, here, None)
}

/// `m` taken by the site's window.
fn tell(app: &mut arkui_web::WebHost<Site>, m: crate::app::Msg) {
    app.update(arkui_web::HostMsg::App(m));
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Every page is written, with its title and description, and every page,
/// picture and file a page names is one the site has; the sitemap lists
/// every page and no other. Falsified by linking `menus/`.
#[test]
fn every_link_lands() {
    let out = scratch("links");
    let o = arkui_web::export::Options {
        base: "/docs/".into(),
        site: Some("https://example.org".into()),
        ..arkui_web::export::Options::default()
    };
    let paths = arkui_web::export::export::<Site>(&out, &o).unwrap();
    assert_eq!(paths.len(), COMPONENTS.len() + 2 + Section::ALL.len(), "{paths:?}");
    let sitemap = read(&out.join("sitemap.xml"));
    for p in &paths {
        assert!(
            sitemap.contains(&format!("<loc>https://example.org/docs{p}</loc>")),
            "{p} is not in the sitemap"
        );
    }
    let pictures: Vec<String> = COMPONENTS
        .iter()
        .flat_map(|c| crate::examples::pictures(c.id).iter().map(|e| e.name))
        .flat_map(|n| crate::pages::VARIANTS.map(|v| format!("pictures/{n}-{v}.webp")))
        .collect();
    for p in &paths {
        let page = p
            .split('/')
            .filter(|s| !s.is_empty())
            .fold(out.clone(), |d, s| d.join(s))
            .join("index.html");
        let html = read(&page);
        assert!(
            html.contains("<title>") && html.contains("<meta name=\"description\" content=\""),
            "{p} has no title or description"
        );
        for attr in ["href=\"", "src=\"", "srcset=\"", "formaction=\""] {
            for link in html.split(attr).skip(1) {
                let link = link.split(['"', '?', '#']).next().unwrap();
                if link.is_empty() || link.starts_with("http") || link.starts_with("mailto:") {
                    continue;
                }
                let Some(rel) = link.strip_prefix("/docs/") else {
                    panic!("{p} links {link}, outside the site");
                };
                let there = out.join(rel).exists()
                    || out.join(rel).join("index.html").exists()
                    || pictures.iter().any(|x| x == rel)
                    || rel.starts_with("pkg/");
                assert!(there, "{p} links {link}, which the site does not have");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&out);
}

/// Every page but the first has the one before it, every page but the
/// last the one after, and a section's own page comes before its pages.
/// Falsified by leaving the sections' pages out of the reading order.
#[test]
fn every_page_leads_to_the_next() {
    let mut app = site();
    let pages = app.app().pages().to_vec();
    let order: Vec<String> = crate::pages::reading(&pages).iter().map(|p| p.tag.clone()).collect();
    assert_eq!(order.len(), pages.len() - 1, "every page but the index of every page");
    assert_eq!(order[0], "overview");
    assert_eq!(order[1], "foundations");
    let at = |t: &str| order.iter().position(|o| o == t).unwrap();
    assert_eq!(at("overlays") + 1, at("menu"));
    tell(&mut app, crate::app::Msg::Go { tag: "menu".into() });
    tell(&mut app, crate::app::Msg::Step(-1));
    assert_eq!(app.route().path_url(), "/overlays/");
    tell(&mut app, crate::app::Msg::Step(1));
    tell(&mut app, crate::app::Msg::Step(1));
    assert_eq!(app.route().path_url(), "/command/");
    tell(
        &mut app,
        crate::app::Msg::Go {
            tag: order.last().unwrap().clone(),
        },
    );
    tell(&mut app, crate::app::Msg::Step(1));
    assert_eq!(app.route().path_url(), format!("/{}/", order.last().unwrap()), "past the last, it stays");
}

/// A page's every way to another page is a link without script, and the
/// app's view of a page is the same written natively as drawn again.
/// Falsified by a route that keeps the search (the reader's) in its path.
#[test]
fn every_page_is_a_path_and_the_search_is_not() {
    let mut app = site();
    tell(&mut app, crate::app::Msg::Go { tag: "menu".into() });
    assert_eq!(app.route().path_url(), "/menu/");
    tell(&mut app, crate::app::Msg::Search { text: "spring".into() });
    assert_eq!(app.route().path_url(), "/menu/");
    assert_eq!(app.route().get("q"), Some("spring"));
    assert_eq!(app.paths().len(), COMPONENTS.len() + 2 + Section::ALL.len());
    assert!(app.paths().iter().all(|r| r.query.is_empty()));
    let found = app.app().found();
    assert!(found.iter().any(|h| h.tag == "springs"), "{found:?}");
    assert!(found.iter().all(|h| h.tag != "overview" || h.title == "ArkUI"));
    tell(&mut app, crate::app::Msg::Phone { phone: "ios".into() });
    assert_eq!(app.route().get("phone"), Some("ios"), "the phone is the reader's, in the query");
    tell(&mut app, crate::app::Msg::Go { tag: "nowhere".into() });
    assert_eq!(app.route().path_url(), "/");
}
