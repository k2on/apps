//! The fleet, mixed across runtimes, with Kotlin peers (`docs/plan-phones.md`
//! round 4, `docs/plan-fleet.md` §6): the six scenarios of
//! `tests/support/mixed.rs`, each with the Kotlin client's `harken-peer-kt`
//! beside Rust peers — and a seventh, the listening session across the two
//! runtimes (`docs/plan-live.md` phase 3), which the Swift client's peer
//! does not speak yet. The scenarios are one body for every runtime
//! (`tests/fleet_swift.rs` runs the same six with the Swift client's); what
//! is the Kotlin peer's own here is how each scenario was falsified for it.
//!
//! The Kotlin peer is `FLEET_KOTLIN_PEER` ([`KOTLIN`]), which
//! `checks.fleet-kotlin` builds and sets; without it every scenario says it
//! is skipped and passes, so `cargo test` on a laptop is unchanged. The
//! version shapes want `HARKEN_OLD_<n>_*` too ([`olds`]), as
//! `checks.versions` does.
//!
//! `cargo test -p harken-server --test fleet_kotlin -- --nocapture` prints
//! the timings.

mod support {
    pub mod fleet;
    pub mod mixed;
    pub mod proxy;
}

use support::fleet::*;
use support::mixed;

/// **K1** ([`mixed::partitions_and_restarts`]).
///
/// Falsified with a Kotlin client that did not push what was pending when
/// a connection opened (`Client.connected` without its `Push`): the ten
/// pushed into the black hole were never pushed again, and `converged`
/// timed out with the Kotlin peer holding them. (A client that ignored
/// every `ack` was tried first and passed — every phase here reconnects,
/// and a page of its own entries confirms them as an ack would.)
#[test]
fn k1_two_rust_peers_and_a_kotlin_one_through_partitions_and_restarts() {
    if let Some(rt) = KOTLIN.or_skip("k1 mixed partitions and restarts") {
        mixed::partitions_and_restarts(&rt);
    }
}

/// **K2** ([`mixed::alone_then_joining`]).
///
/// Falsified with a Kotlin `Peer.join` that re-queued nothing (its
/// `forkBackTo` skipped): the `join` without a restart answered pending 0,
/// cursor 49, where three were owed. (The join by restart opens through
/// `Peer.open`, which re-queues on its own, and passed.)
#[test]
fn k2_a_kotlin_peer_alone_then_joining() {
    if let Some(rt) = KOTLIN.or_skip("k2 alone, then joining") {
        mixed::alone_then_joining(&rt);
    }
}

/// **K3** ([`mixed::server_restart_and_lost_log`]).
///
/// Falsified with a Kotlin client that ignored every snapshot
/// (`Client.recvSnapshot` returning at once): after the lost log it stayed
/// at cursor 25, past a head of 9, its intents never confirmed, and
/// `converged` timed out.
#[test]
fn k3_a_kotlin_peer_through_a_server_restart_and_a_lost_log() {
    if let Some(rt) = KOTLIN.or_skip("k3 server restart and lost log") {
        mixed::server_restart_and_lost_log(&rt);
    }
}

/// **K4** ([`mixed::new_peer_old_server`]).
///
/// Falsified with a Kotlin client that took every `reject` as a verdict
/// (the unknown function not read as a hold): `held` never reached 3 and
/// the scenario failed waiting for it.
#[test]
fn k4_new_kotlin_peer_old_server() {
    if let Some(rt) = KOTLIN.or_skip("k4 new Kotlin peer, old server") {
        mixed::new_peer_old_server(&rt);
    }
}

/// **K5** ([`mixed::old_peer_new_server`]).
///
/// Falsified with a `harken-peer-kt` that ignored `--module` and ran the
/// harken.ark it carries: it was not behind, and its songs were at this
/// build's hash.
#[test]
fn k5_old_kotlin_peer_new_server() {
    if let Some(rt) = KOTLIN.or_skip("k5 old Kotlin peer, new server") {
        mixed::old_peer_new_server(&rt);
    }
}

/// **K6** ([`mixed::partial_under_a_scope`]).
///
/// Falsified with a Kotlin client that opened a partial snapshot whole (its
/// `held` dropped in `Client.recvSnapshot`): every partial page after it
/// was refused as another kind of replica's, and the Kotlin peer stayed at
/// the head with an intent pending for good.
#[test]
fn k6_a_kotlin_peer_partial_under_a_scoped_module() {
    if let Some(rt) = KOTLIN.or_skip("k6 partial under a scope") {
        mixed::partial_under_a_scope(&rt);
    }
}

/// **K7** ([`mixed::listening_across_runtimes`]).
///
/// Falsified with a `harken-peer-kt` that declared no device (`--device`
/// read and dropped): the Kotlin peer held no live state, its `live`
/// answered null and unsynced, and the scenario failed at its first step,
/// waiting for both devices to be here. And with a Kotlin client that did
/// not say its pending live calls again on a new connection
/// (`Watch.connected`): the play pressed into the black hole never landed,
/// and the laptop saw the phone back with the sound, paused, for good.
#[test]
fn k7_the_listening_session_across_runtimes() {
    if let Some(rt) = KOTLIN.or_skip("k7 the listening session across runtimes") {
        mixed::listening_across_runtimes(&rt);
    }
}
