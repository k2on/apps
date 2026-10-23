//! The fuzzer, a slice of it on every `cargo test`: random scripts over
//! the demo domain, and every check — convergence,
//! the partial client its union, replay from facts, roles as logged, every
//! frame read back, every view its patches and a fresh evaluation, every
//! client's trace replayed. `arkdb fuzz 10000` is the full run.

/// Two hundred sessions find nothing. Falsified by `connected` pushing a
/// joined peer's pending intents one by one rather than handing its chain
/// over again: seeds 29 and 31 did not settle, a gap holding the rest.
#[test]
fn two_hundred_sessions_find_nothing() {
    let mut failed = vec![];
    for seed in 1..=200 {
        if let Err(e) = arkdb::fuzz::session(seed, true) {
            failed.push(format!("seed {seed}: {e}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The scripts do things over the demo: sessions log entries and are
/// refused some — a blank name, a playlist nobody has. Forty sessions log
/// 421 entries and are told 532 refusals. Falsified by drawing every
/// add's playlist as one nobody has: the entries fell to 238.
#[test]
fn sessions_do_things() {
    let (mut entries, mut refused) = (0, 0);
    for seed in 1..=40 {
        let (_, f) = arkdb::fuzz::session(seed, false).unwrap();
        (entries, refused) = (entries + f.entries, refused + f.refused);
    }
    assert!(entries > 300 && refused > 300, "{entries} entries, {refused} refused");
}
