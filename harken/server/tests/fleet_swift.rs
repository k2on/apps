//! The fleet, mixed across runtimes, with Swift peers (`docs/plan-phones.md`
//! round 7, `docs/plan-fleet.md` §6): the six scenarios of
//! `tests/support/mixed.rs` — the same bodies `tests/fleet_kotlin.rs` runs
//! with Kotlin peers — each with the Swift client's `harken-peer-swift`
//! beside Rust peers. What is the Swift peer's own here is how each scenario
//! was falsified for it.
//!
//! The Swift peer is `FLEET_SWIFT_PEER` ([`SWIFT`]), which
//! `checks.fleet-swift` builds and sets; without it every scenario says it
//! is skipped and passes, so `cargo test` on a laptop is unchanged. The
//! version shapes want `HARKEN_OLD_<n>_*` too ([`olds`]), as
//! `checks.versions` does.
//!
//! `cargo test -p harken-server --test fleet_swift -- --nocapture` prints
//! the timings.

mod support {
    pub mod fleet;
    pub mod mixed;
    pub mod proxy;
}

use support::fleet::*;
use support::mixed;

/// **S1** ([`mixed::partitions_and_restarts`]).
///
/// Falsified with a Swift client that did not push what was pending when a
/// connection opened (`Client.connected` without its `push`): the ten pushed
/// into the black hole were never pushed again, and `converged` timed out
/// after 60 s with the Swift peer holding them.
#[test]
fn s1_two_rust_peers_and_a_swift_one_through_partitions_and_restarts() {
    if let Some(rt) = SWIFT.or_skip("s1 mixed partitions and restarts") {
        mixed::partitions_and_restarts(&rt);
    }
}

/// **S2** ([`mixed::alone_then_joining`]).
///
/// Falsified with a Swift `Peer.join` that re-queued nothing (its
/// `forkBackTo` skipped): the `join` without a restart answered pending 0,
/// cursor 49, where three were owed — as the Kotlin falsification did. The
/// first run of this scenario found a real bug, in the program: `quit` was
/// answered and `harken-peer-swift` never exited, its stdin reader holding
/// stdin's `FILE` lock in `readLine` while `exit` flushed under it.
#[test]
fn s2_a_swift_peer_alone_then_joining() {
    if let Some(rt) = SWIFT.or_skip("s2 alone, then joining") {
        mixed::alone_then_joining(&rt);
    }
}

/// **S3** ([`mixed::server_restart_and_lost_log`]).
///
/// Falsified with a Swift client that ignored every snapshot
/// (`Client.recvSnapshot` returning at once): after the lost log its intents
/// were never confirmed, and `converged` timed out after 60 s.
#[test]
fn s3_a_swift_peer_through_a_server_restart_and_a_lost_log() {
    if let Some(rt) = SWIFT.or_skip("s3 server restart and lost log") {
        mixed::server_restart_and_lost_log(&rt);
    }
}

/// **S4** ([`mixed::new_peer_old_server`]).
///
/// Falsified with a Swift client that took every `reject` as a verdict (the
/// unknown function not read as a hold): `held` stayed 0 and pending 0 — the
/// three refused outright — and the scenario failed waiting for three held.
#[test]
fn s4_new_swift_peer_old_server() {
    if let Some(rt) = SWIFT.or_skip("s4 new Swift peer, old server") {
        mixed::new_peer_old_server(&rt);
    }
}

/// **S5** ([`mixed::old_peer_new_server`]).
///
/// Falsified with a `harken-peer-swift` that ignored `--module` and ran the
/// harken.ark installed beside it: it was not behind (`Some(false)` where
/// `Some(true)` was owed).
#[test]
fn s5_old_swift_peer_new_server() {
    if let Some(rt) = SWIFT.or_skip("s5 old Swift peer, new server") {
        mixed::old_peer_new_server(&rt);
    }
}

/// **S6** ([`mixed::partial_under_a_scope`]).
///
/// Falsified with a Swift client that opened a partial snapshot whole (the
/// device schema and the union dropped in `Client.recvSnapshot`): the Swift
/// peer stayed at the head with three intents pending for good, and the
/// scenario said it had not converged.
#[test]
fn s6_a_swift_peer_partial_under_a_scoped_module() {
    if let Some(rt) = SWIFT.or_skip("s6 partial under a scope") {
        mixed::partial_under_a_scope(&rt);
    }
}
