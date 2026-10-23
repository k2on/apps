//! ArkDB v5's development tools (`docs/plan-v5.md` D1): the live demo
//! ([`demo`]), the simulator over the real hub and clients ([`sim`]), the
//! trace recorder and player ([`trace`]), the fuzzer ([`fuzz`]), the
//! vector writer and runner ([`vectors`]), the `views/` vectors' reference
//! domains ([`library`]) and the Kotlin and Swift of a domain's types
//! and stubs for an app over the bridge ([`gen`]).

pub mod demo;
pub mod events;
pub mod fuzz;
pub mod gen;
pub mod library;
pub mod sim;
pub mod trace;
pub mod vectors;
