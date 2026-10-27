//! The catalogue held to the code it describes, and the site to itself.
use std::path::{Path, PathBuf};

use crate::catalogue::{self, ENTRIES};
use crate::components::COMPONENTS;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn arkui_modules() -> Vec<String> {
    read(&root().join("rust/arkui/src/lib.rs"))
        .lines()
        .filter_map(|l| Some(l.strip_prefix("pub mod ")?.strip_suffix(';')?.to_string()))
        .collect()
}

/// Falsified by deleting the `swipe` entry (fails: no entry), and by adding
/// one for a module that does not exist (fails: no module).
#[test]
fn every_module_has_an_entry_and_every_entry_a_module() {
    let modules = arkui_modules();
    assert!(modules.len() > 20, "read arkui's modules: {modules:?}");
    for m in &modules {
        assert!(catalogue::find(m).is_some(), "arkui::{m} has no entry in catalogue.rs");
    }
    for e in ENTRIES {
        assert!(
            modules.iter().any(|m| m == e.module),
            "catalogue.rs has {} but arkui has no such module",
            e.module
        );
        assert!(!e.summary.is_empty() && !e.api.is_empty(), "{} says nothing", e.module);
    }
}

/// Falsified by taking `panel` out of Menu (fails: in no component), and by
/// putting `scroll` in Lists as well (fails: in two).
#[test]
fn every_module_is_in_exactly_one_component() {
    for m in arkui_modules() {
        let holders: Vec<&str> = COMPONENTS.iter().filter(|c| c.desktop.contains(&m.as_str())).map(|c| c.id).collect();
        assert_eq!(holders.len(), 1, "arkui::{m} is in {holders:?}; every module is in exactly one component");
    }
    for c in COMPONENTS {
        for m in c.desktop {
            assert!(catalogue::find(m).is_some(), "{} names arkui::{m}, which has no entry", c.id);
        }
        assert!(
            !c.desktop.is_empty() || c.android.is_some() || c.ios.is_some(),
            "{} is on no platform",
            c.id
        );
    }
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
    let table = read(&root().join("rust/arkui/src/glyphs.rs"));
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

/// Every page is written, and every page, picture, glyph and file a page
/// names is one the site has. Falsified by linking `menus.html`.
#[test]
fn every_link_lands() {
    let out = scratch("links");
    crate::site::write(&out, false).unwrap();
    let pages: Vec<PathBuf> = std::iter::once(out.join("index.html"))
        .chain(COMPONENTS.iter().map(|c| out.join(format!("{}.html", c.id))))
        .collect();
    let pictures: Vec<String> = COMPONENTS
        .iter()
        .flat_map(|c| crate::examples::pictures(c.id).iter().map(|e| e.name))
        .flat_map(|n| ["light-wide", "light-narrow", "dark-wide", "dark-narrow"].map(|v| format!("pictures/{n}-{v}.webp")))
        .collect();
    for page in &pages {
        let html = read(page);
        for attr in ["href=\"", "src=\"", "srcset=\"", "url("] {
            for link in html.split(attr).skip(1) {
                let link = link.split(['"', ')', '?', '#']).next().unwrap();
                if link.is_empty() || link.starts_with("http") {
                    continue;
                }
                let there = out.join(link).exists() || pictures.iter().any(|p| p == link);
                assert!(there, "{} links {link}, which the site does not have", page.display());
            }
        }
    }
    let _ = std::fs::remove_dir_all(&out);
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
