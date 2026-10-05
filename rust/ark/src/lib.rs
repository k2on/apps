#![deny(unsafe_code)]
//! The ArkDB runtime, held to the specification in `spec/` (a Haskell
//! program; `spec/README.md` is the index). Each module here mirrors the
//! spec module it names and says, briefly, what the Haskell says.
//!
//! | module | spec |
//! |---|---|
//! | [`value`] | `Ark.Value`: the eight values and the one total order |
//! | [`canon`] | `Ark.Canon`: deterministic CBOR, strict decoder |
//! | [`schema`] | `Ark.Schema`: tables, relations, well-formedness |
//! | [`ir`] | `Ark.IR`, `Ark.Encode`, `Ark.Decode`: the IR, as a value and back |
//! | [`store`] | `Ark.Store`: get, scan, put, delete; constraints as refusals |
//! | [`stdlib`] | `Ark.Std`: the standard library over pinned Unicode tables |
//! | [`eval`] | `Ark.Eval`: the interpreter every native mutator must agree with |
//! | [`verify`] | `Ark.Verify`: what a module must satisfy before anything runs it |
//! | [`hash`] | `Ark.Hash`: the state hash, closures, the function hash |
//! | [`log`] | `Ark.Log`: entries, facts, snapshots, the horizon |
//! | [`journal`] | a log on a key/value storage: a snapshot and pages of records, the server's and a peer alone's (`docs/plan-alone.md` §2) |
//! | [`peer`] | `Ark.Peer`: the replica and the authority |
//! | [`protocol`] | `Ark.Protocol`: the frames, the client and server machines |
//! | [`view`] | `Ark.View`: [`view::pull`], the one evaluator of plans (spec v4), and incremental views with their contract |
//! | [`live`] | `Ark.Live`: rooms per account over opaque frames |
//! | [`sim`] | `Ark.Sim`: the seeded fleet |
//! | [`json`] | the vectors' JSON dialect (`spec/README.md`), printed and read back |
//! | [`authoring`] | `spec/AUTHORING.md`: the vocabulary a domain is written in; a mutator run `Emit` or `Native`, a query described under `Emit` as its plan |
//!
//! A domain is written once, in Rust, in the vocabulary of
//! `spec/AUTHORING.md` — the contract, read it first — through
//! [`authoring`]: run under `Emit` it is the module (the `.ark` every
//! runtime verifies and hashes), run under `Native` its mutators apply
//! entries directly, and [`eval`] is what both mean. A query is a plan
//! (`docs/plan-v4.md`), and [`view::pull`] is what one means. Nothing is
//! generated into this crate's private shape any more: `GENERATED.md`,
//! `GENERATED-RUST.md` and the prelude they named are retired, and so is
//! `ark-builder`.
//!
//! No async, no sockets: every machine here is sans-io, and a transport is
//! a loop around one. The one `unsafe` is in [`authoring`], and says why.

pub mod authoring;
pub mod canon;
pub mod compat;
pub mod eval;
pub mod hash;
pub mod ir;
pub mod journal;
pub mod json;
pub mod live;
pub mod log;
pub mod peer;
pub mod plan;
pub mod protocol;
pub mod retention;
pub mod rules;
pub mod schema;
pub mod sha256;
pub mod sim;
pub mod stdlib;
pub mod store;
pub mod unicode_tables;
pub mod value;
pub mod verify;
pub mod view;
