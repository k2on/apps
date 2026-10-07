//! `rebase/fleet-fuzz-*`: sessions `arkc fuzz` found a bug with, kept as
//! the fuzzer wrote them (`docs/plan-db.md` D2). The generator does not
//! make these; it carries them, byte for byte, from `fuzzed/`, and writes
//! one only if it now holds — the fleet converges and the log replays
//! ([`ark::sim::run_script`], which is what `rebase/` runs it through) —
//! so a fix that regresses fails the generator as well as the runner.

use ark::hash::closures;
use ark::ir::module_from_value;

use super::{claim, Out};

/// Each session, by the name it is published under.
const SESSIONS: [(&str, &str); 4] = [
    (
        "fleet-fuzz-an-ack-at-or-below-the-cursor.json",
        include_str!("fuzzed/fleet-fuzz-an-ack-at-or-below-the-cursor.json"),
    ),
    (
        "fleet-fuzz-an-ack-names-no-log.json",
        include_str!("fuzzed/fleet-fuzz-an-ack-names-no-log.json"),
    ),
    (
        "fleet-fuzz-an-ack-names-no-log-and-the-replay-refuses.json",
        include_str!("fuzzed/fleet-fuzz-an-ack-names-no-log-and-the-replay-refuses.json"),
    ),
    // `docs/plan-guards.md` D2: a client holding a union, re-opened from
    // what it made durable, kept laying out every later snapshot from the
    // device's schema instead of the module's.
    (
        "fleet-fuzz-a-union-reopened-keeps-the-modules-schema.json",
        include_str!("fuzzed/fleet-fuzz-a-union-reopened-keeps-the-modules-schema.json"),
    ),
];

pub fn fuzzed(out: &Out) {
    out.dir("rebase/ (found by arkc fuzz)");
    for (name, text) in SESSIONS {
        let v = ark::json::decode(text).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let m = module_from_value(&v.field("module")).unwrap_or_else(|e| panic!("{name}: {e}"));
        let script = v.field("script").as_list();
        let ran = ark::sim::run_script(
            m.schema.clone(),
            closures(&m),
            v.field("clients").as_int(),
            v.field("seed").as_int() as u64,
            &script,
        )
        .map(|_| ());
        claim(&format!("{name}: {ran:?}"), ran.is_ok());
        out.write(&format!("rebase/{name}"), text);
    }
}
