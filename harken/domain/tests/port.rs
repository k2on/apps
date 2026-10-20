//! The builder port (`harken_domain::built`) held to the pin: every
//! function at the hash the attribute macros built it at. Falsified by
//! numbering a playlist "Favorites [1]": `playlists.numbered` and the
//! three bound functions that reach it were wrong.

use std::collections::BTreeMap;

use ark::ir::{Function, Node};

#[test]
fn every_function_built_is_the_macros() {
    let pinned: BTreeMap<&str, &str> = include_str!("fixtures/functions.txt")
        .lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(n, _)| *n != "module")
        .collect();
    let file = include_bytes!("../harken.ark");
    let (_, bodies) = <(ark::module::Module, Vec<Vec<u8>>)>::from_bytes(file).unwrap();
    let old: BTreeMap<String, Function> = bodies
        .iter()
        .map(|b| Function::from_bytes(b).unwrap())
        .map(|f| (f.hash().to_string(), f))
        .collect();
    let d = harken_domain::built::domain();
    let mut wrong = vec![];
    for (n, h) in &d.module.bindings {
        match pinned.get(n.as_str()) {
            Some(p) if *p == h.to_string() => {}
            Some(p) => {
                let new = d.lib.get(h).unwrap().f.clone();
                wrong.push(format!(
                    "{n}\n  pinned {:?}\n  built  {:?}\n  types pinned {:?}\n  types built  {:?}",
                    old[*p].body,
                    new.body,
                    old[*p].types.iter().map(|t| &t.0).collect::<Vec<_>>(),
                    new.types.iter().map(|t| &t.0).collect::<Vec<_>>()
                ));
            }
            None => wrong.push(format!("{n}: not pinned")),
        }
    }
    let missing: Vec<_> = pinned.keys().filter(|n| d.module.get(n).is_none()).collect();
    assert!(
        wrong.is_empty() && missing.is_empty(),
        "{} wrong, missing {missing:?}\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
