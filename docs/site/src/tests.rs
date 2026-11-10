//! The catalogue held to the code it describes, and the site to itself.
use std::path::{Path, PathBuf};

use crate::catalogue;
use crate::components::{Section, COMPONENTS};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

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
    let committed = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("catalogue.json"));
    assert!(
        committed == catalogue::json(),
        "docs/site/catalogue.json is stale: in docs/site, cargo run -- --json > catalogue.json"
    );
}

/// Falsified by dropping `PULPIT` from `examples::GLYPHS`.
#[test]
fn the_glyph_picture_draws_every_glyph() {
    // The table is arkui-glyphs', which arkui re-exports as `glyphs`.
    let table = read(&root().join("ui/rust/glyphs/src/lib.rs"));
    let names: Vec<&str> = table.lines().filter_map(|l| l.strip_prefix("pub const ")?.split(':').next()).collect();
    assert!(names.len() > 30);
    for n in names {
        assert!(
            crate::examples::GLYPHS.iter().any(|(g, _)| *g == n),
            "glyphs::{n} is not in the Icons picture"
        );
    }
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
    let o = crate::write::Options {
        base: "/docs/".into(),
        site: "https://example.org".into(),
        pictures: false,
        version: "dev".into(),
    };
    let paths = crate::write::write(&out, &o).unwrap();
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
    // What opens over a page is written open in its frame, and takes no
    // keys from the page: the menu with its submenu, the sheet and the
    // question, the picker's menu, the command box. Falsified by drawing
    // the live parts with `render` rather than as a specimen: the sheet is
    // modal and the menu's submenu is not written.
    let page = |p: &str| read(&out.join(p).join("index.html"));
    let menu = page("menu");
    assert!(menu.matches("ark-menu ark-menu-written").count() >= 2, "the menu and its submenu");
    assert!(menu.contains("Add to Collection") && menu.contains("New Collection…"));
    let dialog = page("dialog");
    assert!(dialog.matches("ark-modal-layer ark-in-place").count() >= 2, "the sheet and the question");
    assert!(!dialog.contains("aria-modal"), "a specimen is not modal");
    assert!(page("picker").contains("ark-menu-written"));
    assert!(page("command").contains("ark-command-written"));
    // Short of a sidebar, each section is a tab, and a page of one is
    // pushed over the section's own page, the way back named for it.
    // Falsified by `tab_group` in `docs_shell`: no tab, no way back.
    assert!(menu.contains("ark-bar-button ark-bar-back") && menu.contains("aria-label=\"Overlays\""));
    for s in Section::ALL {
        assert!(
            menu.contains(&format!("<span class=\"ark-bottom-title\">{}</span>", s.heading())),
            "no {} tab",
            s.heading()
        );
    }
    // A section's own page lists its pages.
    let overlays = page("overlays");
    for c in COMPONENTS.iter().filter(|c| c.section == Section::Overlays) {
        assert!(overlays.contains(c.name), "{} is not on Overlays' page", c.name);
    }
    let _ = std::fs::remove_dir_all(&out);
}

/// Every page but the first has the one before it, every page but the
/// last the one after, and a section's own page comes before its pages.
/// Falsified by leaving the sections' pages out of the reading order.
#[test]
fn every_page_leads_to_the_next() {
    use arkui_web::WebApp;
    let mut app = crate::app::DocsWeb::open("/", crate::pages::Built::default());
    let order: Vec<String> = crate::pages::reading(app.pages()).iter().map(|p| p.tag.clone()).collect();
    assert_eq!(order.len(), app.pages().len() - 1, "every page but the index of every page");
    assert_eq!(order[0], "overview");
    assert_eq!(order[1], "foundations");
    let at = |t: &str| order.iter().position(|o| o == t).unwrap();
    assert_eq!(at("overlays") + 1, at("menu"));
    app.update(crate::app::Msg::Go { tag: "menu".into() });
    app.update(crate::app::Msg::Step(-1));
    assert_eq!(app.route().path_url(), "/overlays/");
    app.update(crate::app::Msg::Step(1));
    app.update(crate::app::Msg::Step(1));
    assert_eq!(app.route().path_url(), "/command/");
    app.update(crate::app::Msg::Go {
        tag: order.last().unwrap().clone(),
    });
    app.update(crate::app::Msg::Step(1));
    assert_eq!(app.route().path_url(), format!("/{}/", order.last().unwrap()), "past the last, it stays");
}

/// A page's every way to another page is a link without script, and the
/// app's view of a page is the same written natively as drawn again.
/// Falsified by a route that keeps the search (the reader's) in its path.
#[test]
fn every_page_is_a_path_and_the_search_is_not() {
    use arkui_web::WebApp;
    let mut app = crate::app::DocsWeb::open("/", crate::pages::Built::default());
    app.update(crate::app::Msg::Go { tag: "menu".into() });
    assert_eq!(app.route().path_url(), "/menu/");
    app.update(crate::app::Msg::Search { text: "spring".into() });
    assert_eq!(app.route().path_url(), "/menu/");
    assert_eq!(app.route().get("q"), Some("spring"));
    assert_eq!(app.static_paths().len(), COMPONENTS.len() + 2 + Section::ALL.len());
    assert!(app.static_paths().iter().all(|r| r.query.is_empty()));
    assert!(app.found().iter().any(|h| h.tag == "springs"), "{:?}", app.found());
    assert!(app.found().iter().all(|h| h.tag != "overview" || h.title == "ArkUI"));
    app.update(crate::app::Msg::Go { tag: "nowhere".into() });
    assert_eq!(app.route().path_url(), "/");
}

/// A picture is drawn, cut to its own height and encoded, light and dark
/// differing. Falsified by cutting nothing (fails: as tall as the canvas).
#[test]
fn a_picture_is_the_component_and_only_as_tall_as_it_draws() {
    let mut painter = crate::render::Painter::new();
    let menu = &crate::examples::pictures("lists")[0];
    let light = (menu.draw)(crate::examples::Canvas::new(&mut painter, &iced::Theme::Light, 320.0));
    let dark = (menu.draw)(crate::examples::Canvas::new(&mut painter, &iced::Theme::Dark, 320.0));
    assert_eq!(light.width, 640);
    assert!(light.height > 100 && light.height < 1000, "a table is {}px tall", light.height);
    assert_ne!(light.webp(), dark.webp());
}
