//! The catalogue held to the code it describes.
use std::path::{Path, PathBuf};

use arkui::route::Route;

use crate::catalogue::{self, ENTRIES};
use crate::components::{self, COMPONENTS};
use crate::Place;

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
/// putting `scroll` in Rows as well (fails: in two).
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

/// Falsified by giving Cards' shot Rows' id.
#[test]
fn a_shot_is_one_id_and_a_component_one_page() {
    let mut shots: Vec<&str> = COMPONENTS.iter().flat_map(|c| c.shots.iter().map(|s| s.id)).collect();
    let n = shots.len();
    shots.sort_unstable();
    shots.dedup();
    assert_eq!(shots.len(), n, "two components capture under one id");
    for (i, c) in COMPONENTS.iter().enumerate() {
        assert!(COMPONENTS[..i].iter().all(|d| d.id != c.id), "two components are {}", c.id);
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
fn the_glyph_page_draws_every_glyph() {
    let table = read(&root().join("rust/arkui/src/glyphs.rs"));
    let names: Vec<&str> = table.lines().filter_map(|l| l.strip_prefix("pub const ")?.split(':').next()).collect();
    assert!(names.len() > 30);
    for n in names {
        assert!(
            crate::examples::GLYPHS.iter().any(|(g, _)| *g == n),
            "glyphs::{n} is not on the Icons page"
        );
    }
}

/// Falsified by spelling Menu's fragment `#mneu` in `fragment`.
#[test]
fn every_place_reads_back_from_its_fragment() {
    for place in Place::all() {
        assert_eq!(Place::parse(&place.fragment()), place);
    }
    assert_eq!(Place::parse("#nowhere"), Place::Overview);
    // An arkui module's name lands on its component.
    assert_eq!(Place::parse("#context"), Place::Component("menu"));
    assert_eq!(Place::all().len(), COMPONENTS.len() + 1);
    assert!(components::find("menu").is_some());
}
