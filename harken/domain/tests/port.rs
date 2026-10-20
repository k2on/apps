//! harken's domain, ported by hand to ArkLang (`harken/domain/*.ark`),
//! held to the pin: every function at the hash the attribute macros built
//! it at, once its declarations' and its binders' names (§1.15, which the
//! macros did not keep) are taken out and its callees named by their
//! pinned hashes. So the names are the only thing A3 and A4 moved.
//! Falsified by numbering a playlist "Favorites [1]": `playlists.numbered`
//! and the three bound functions that reach it were wrong; once names were
//! kept, by comparing the functions as built: 108 of 127 were wrong (every
//! one declaring a type: a declaration's bytes gained its names); and in
//! the text, by writing `create_playlist`'s `(?? last.pos 0)` as `(?? last.pos 1)`.

use std::collections::BTreeMap;

use ark::author::Node;
use ark::codec::bytes_enc;
use ark::hash::FnHash;
use ark::ir::{Effect, Expr, Function, Refine};
use ark::ty::Ty;

/// A declaration as the macros encoded it: no names.
#[derive(Node)]
struct MacroDecl {
    ty: Ty,
    refine: Vec<Refine>,
}

#[derive(Node)]
struct MacroFunction {
    types: Vec<(String, MacroDecl)>,
    params: Vec<Ty>,
    ret: Ty,
    effect: Effect,
    body: Expr,
}

/// `f`'s hash as the macros would have built it: no names, and each callee
/// by the hash `old` gives it.
fn as_the_macros(f: &Function, old: &BTreeMap<FnHash, FnHash>) -> FnHash {
    let types = f
        .types
        .iter()
        .map(|(n, d)| {
            (
                n.clone(),
                MacroDecl {
                    ty: d.ty.clone(),
                    refine: d.refine.clone(),
                },
            )
        })
        .collect();
    let m = MacroFunction {
        types,
        params: f.params.clone(),
        ret: f.ret.clone(),
        effect: f.effect,
        body: f.body.clone(),
    };
    let mut b = m.to_bytes();
    for (new, was) in old.iter().filter(|(n, w)| n != w) {
        let (from, to) = (bytes_enc(&new.0), bytes_enc(&was.0));
        let mut at = 0;
        while let Some(i) = b[at..].windows(from.len()).position(|w| w == from) {
            b.splice(at + i..at + i + from.len(), to.iter().copied());
            at += i + to.len();
        }
    }
    FnHash::of(&b)
}

#[test]
fn every_function_is_the_macros() {
    let pinned: BTreeMap<&str, &str> = include_str!("fixtures/functions.txt")
        .lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(n, _)| *n != "module")
        .collect();
    let d = harken_domain::text();
    let old: BTreeMap<FnHash, FnHash> = d
        .module
        .bindings
        .iter()
        .filter_map(|(n, h)| Some((*h, FnHash(ark::hash::unhex(pinned.get(n.as_str())?)?.try_into().ok()?))))
        .collect();
    let mut wrong = vec![];
    for (n, h) in &d.module.bindings {
        let f = &d.lib.get(h).unwrap().f;
        match pinned.get(n.as_str()) {
            Some(p) if *p == as_the_macros(f, &old).to_string() => {}
            Some(_) => wrong.push(format!(
                "{n}\n  types {:?}\n  body {:?}",
                f.types.iter().map(|t| &t.0).collect::<Vec<_>>(),
                f.body
            )),
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
