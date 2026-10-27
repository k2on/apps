//! The catalogue held to the code it describes.
use std::path::{Path, PathBuf};

use arkui::route::Route;

use crate::catalogue::{self, ENTRIES, HARKEN_ONLY};
use crate::Place;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every `.rs` under `dir`, read.
fn sources(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(sources(&path));
        } else if path.extension().is_some_and(|x| x == "rs") {
            let text = read(&path);
            out.push((path, text));
        }
    }
    out
}

/// The modules `src` reaches in arkui: each `arkui::m`, and each name in a
/// `use arkui::{…}` list.
fn arkui_modules(src: &str) -> Vec<String> {
    let mut found = Vec::new();
    let ident = |s: &str| s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>();
    for (at, _) in src.match_indices("arkui::") {
        let rest = &src[at + "arkui::".len()..];
        if let Some(list) = rest.strip_prefix('{') {
            let body = &list[..list.find('}').unwrap_or(list.len())];
            found.extend(body.split(',').map(|n| ident(n.trim())).filter(|n| !n.is_empty()));
        } else {
            found.push(ident(rest));
        }
    }
    found
}

/// Falsified by deleting the `swipe` entry (fails: no entry), and by adding
/// one for a module that does not exist (fails: no module).
#[test]
fn every_module_has_an_entry_and_every_entry_a_module() {
    let lib = read(&root().join("rust/arkui/src/lib.rs"));
    let modules: Vec<&str> = lib.lines().filter_map(|l| l.strip_prefix("pub mod ")?.strip_suffix(';')).collect();
    assert!(modules.len() > 20, "read arkui's modules: {modules:?}");
    for m in &modules {
        assert!(catalogue::find(m).is_some(), "arkui::{m} has no entry in catalogue.rs");
    }
    for e in ENTRIES {
        assert!(modules.contains(&e.module), "catalogue.rs has {} but arkui has no such module", e.module);
        assert!(!e.summary.is_empty() && !e.api.is_empty(), "{} says nothing", e.module);
    }
}

/// Falsified by emptying `cards`' harken list (fails: harken uses it), and
/// by giving `url`… a module harken does not use a line.
#[test]
fn an_entry_says_harken_uses_it_exactly_when_harken_does() {
    let used: Vec<String> = sources(&root().join("harken/iced/src"))
        .iter()
        .flat_map(|(_, s)| arkui_modules(s))
        .collect();
    for e in ENTRIES {
        let uses = used.iter().any(|m| m == e.module);
        assert_eq!(
            !e.harken.is_empty(),
            uses,
            "catalogue.rs says harken {} arkui::{}; harken/iced/src says otherwise",
            if e.harken.is_empty() { "does not use" } else { "uses" },
            e.module
        );
    }
}

/// Falsified by changing a summary without regenerating (fails: stale).
#[test]
fn catalogue_json_is_what_the_catalogue_prints() {
    let committed = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("catalogue.json"));
    assert!(
        committed == catalogue::json(),
        "rust/docs/catalogue.json is stale: cargo run -p docs -- --json > rust/docs/catalogue.json"
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
            "glyphs::{n} is not on the Glyphs page"
        );
    }
}

/// Falsified by spelling `harken`'s fragment `#harkn` in `fragment`.
#[test]
fn every_place_reads_back_from_its_fragment() {
    for place in Place::all() {
        assert_eq!(Place::parse(&place.fragment()), place);
    }
    assert_eq!(Place::parse("#nowhere"), Place::Overview);
    assert_eq!(Place::all().len(), ENTRIES.len() + 2);
}

/// Falsified by naming a module in `built_on` that arkui does not have.
#[test]
fn what_harken_builds_on_is_in_arkui() {
    for h in HARKEN_ONLY {
        for m in h.built_on {
            assert!(catalogue::find(m).is_some(), "{} is built on arkui::{m}, which has no entry", h.name);
        }
    }
}
