//! harken's natives against the interpreter under the fuzzer's sessions
//! (`arkc fuzz`, `docs/plan-db.md` D2): random fleets over harken's own
//! module — partitions, drops, rebases, late joiners, restarts from the
//! journal or over an emptied one, a moved horizon, re-opens — with the
//! natives held by the server and every even client and the rest
//! interpreting. Every mutation is run both ways over its author's view
//! and must agree (`Procedure::agrees`); a fleet of both kinds must
//! converge with no replay diverging from the facts; every maintained
//! view of every query must be a fresh hydrate. `agreement.rs` holds the
//! same on chosen steps; this holds it on drawn ones.
//!
//! The session generator is `arkc`'s, reached by path: it is a binary's
//! module, not a library's, and harken is the one other domain with
//! natives to hold it to.

#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../rust/ark/src/bin/fuzz/mod.rs"]
mod fuzz;

/// Falsified by giving `create_playlist` a body that reads the host (a
/// static counter added to the position, which the emit saw once and the
/// native sees anew each run): the first session that makes a second
/// playlist fails `natives`, native `pos` 2 against interpreted 1.
#[test]
fn harken_natives_agree_with_the_interpreter_under_random_sessions() {
    let m = harken_domain::module();
    let built = m.build().clone();
    let natives = m.procedures();
    let mut tally = fuzz::session::Tally::default();
    for seed in 0..40u64 {
        let (found, _) = fuzz::session::run(&built, &natives, 0x4a7e_0000 + seed, &[], &mut tally);
        if let Some(f) = found {
            panic!("seed {}: {}: {}", 0x4a7e_0000 + seed, f.check(), f.why());
        }
    }
    assert!(tally.natives_checked >= 500, "too few mutations compared: {tally:?}");
    assert!(tally.entries >= 100, "too few entries sequenced: {tally:?}");
    // `docs/plan-guards.md` D1: `library` is granted and revoked like any
    // role, so the library's guard refuses on devices and at the authority.
    assert!(
        tally.forbidden_local > 0 && tally.forbidden_remote > 0,
        "the library's guard was never met: {tally:?}"
    );
}
