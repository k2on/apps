//! The fuzzer, a slice of it on every `cargo test`: random modules (each
//! through the checker), random scripts, and every check — convergence,
//! the partial client its union, replay from facts, roles as logged, every
//! frame read back, every view its patches and a fresh evaluation, every
//! client's trace replayed. `ark-fuzz 10000` is the full run.

/// Two hundred sessions find nothing. Falsified by `connected` pushing a
/// joined peer's pending intents one by one rather than handing its chain
/// over again: seeds 29 and 31 did not settle, a gap holding the rest.
#[test]
fn two_hundred_sessions_find_nothing() {
    let mut failed = vec![];
    for seed in 1..=200 {
        if let Err(e) = ark_dev::fuzz::session(seed, true) {
            failed.push(format!("seed {seed}: {e}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The generator's modules are what the checker takes, and they vary:
/// sessions log entries and are refused some, over every kind of op.
/// Falsified by the generator giving `bump`'s amount the type `Text`: the
/// checker refused it and the session panicked.
#[test]
fn sessions_do_things() {
    let (mut entries, mut refused) = (0, 0);
    for seed in 1..=40 {
        let (_, f) = ark_dev::fuzz::session(seed, false).unwrap();
        (entries, refused) = (entries + f.entries, refused + f.refused);
    }
    assert!(entries > 200 && refused > 100, "{entries} entries, {refused} refused");
}

/// Every generated module — plain IR with no declarations, a `Bool` built
/// as a variant, rows as positional tuples — printed as ArkLang builds to
/// itself, with the names the printer gave its binders, and prints the
/// same again (§1.15: `build(print(m)) == m` for every module, not only
/// those written in text). Falsified by printing
/// a `Bool` built as a variant as `true`: the rebuilt function held a
/// literal, and the module was another.
#[test]
fn every_generated_module_prints_and_builds_to_itself() {
    use ark::ir::{Function, Node};
    for seed in 1..=200 {
        let g = ark_dev::fuzz::Gen::new(&mut ark_dev::fuzz::Rng(seed));
        let bindings = g.bindings();
        let fns: Vec<Function> = bindings
            .iter()
            .map(|b| Function::from_bytes(g.module.cas.body(&b.1).unwrap()).unwrap())
            .collect();
        let m = ark::module::Module {
            spec: ark::module::SPEC,
            schema: (*g.module.schema).clone(),
            types: vec![],
            bindings,
        };
        let text = ark::sexp::print(&m, &fns).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let d = ark::sexp::build(&text).unwrap_or_else(|e| panic!("seed {seed}: {e}\n{text}"));
        let unnamed = |h: &ark::hash::FnHash| d.lib.get(h).unwrap().f.unnamed();
        let built: Vec<(String, Function)> = d.module.bindings.iter().map(|(n, h)| (n.clone(), unnamed(h))).collect();
        let was: Vec<(String, Function)> = m.bindings.iter().cloned().zip(&fns).map(|((n, _), f)| (n, f.clone())).collect();
        assert_eq!((&d.module.schema, built), (&m.schema, was), "seed {seed}\n{text}");
        let again: Vec<Function> = d.bodies.values().map(|b| Function::from_bytes(b).unwrap()).collect();
        assert_eq!(ark::sexp::print(&d.module, &again).unwrap(), text, "seed {seed}");
    }
}
