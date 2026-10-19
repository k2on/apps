//! The fuzzer, a slice of it on every `cargo test`: random modules (each
//! through the checker), random scripts, and every check — convergence,
//! the partial client its union, replay from facts, roles as logged, every
//! frame read back, every view its patches and a fresh evaluation, every
//! client's trace replayed. `ark5-fuzz 10000` is the full run.

/// Two hundred sessions find nothing. Falsified by `connected` pushing a
/// joined peer's pending intents one by one rather than handing its chain
/// over again: seeds 29 and 31 did not settle, a gap holding the rest.
#[test]
fn two_hundred_sessions_find_nothing() {
    let mut failed = vec![];
    for seed in 1..=200 {
        if let Err(e) = ark5_dev::fuzz::session(seed, true) {
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
        let (_, f) = ark5_dev::fuzz::session(seed, false).unwrap();
        (entries, refused) = (entries + f.entries, refused + f.refused);
    }
    assert!(entries > 200 && refused > 100, "{entries} entries, {refused} refused");
}
