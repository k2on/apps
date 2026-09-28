#![deny(unsafe_code)]
//! The ArkDB runtime, held to the specification in `spec/` (a Haskell
//! program; `spec/README.md` is the index). Each module here mirrors the
//! spec module it names and says, briefly, what the Haskell says.
//!
//! | module | spec |
//! |---|---|
//! | [`value`] | `Ark.Value`: the eight values and the one total order |
//! | [`canon`] | `Ark.Canon`: deterministic CBOR, strict decoder |
//! | [`schema`] | `Ark.Schema`: scopes, tables, relations, well-formedness |
//! | [`ir`] | `Ark.IR`, `Ark.Encode`, `Ark.Decode`: the IR, as a value and back |
//! | [`store`] | `Ark.Store`: get, scan, put, delete; constraints as refusals |
//! | [`stdlib`] | `Ark.Std`: the standard library over pinned Unicode tables |
//! | [`eval`] | `Ark.Eval`: the interpreter every native procedure must agree with |
//! | [`verify`] | `Ark.Verify`: what a module must satisfy before anything runs it |
//! | [`hash`] | `Ark.Hash`: the state hash, closures, the function hash |
//! | [`log`] | `Ark.Log`: entries, facts, snapshots, the horizon |
//! | [`peer`] | `Ark.Peer`: the replica and the authority |
//! | [`protocol`] | `Ark.Protocol`: the frames, the client and server machines |
//! | [`view`] | `Ark.View`: incremental views and their contract |
//! | [`live`] | `Ark.Live`: rooms per account over opaque frames |
//! | [`sim`] | `Ark.Sim`: the seeded fleet |
//! | [`authoring`] | `spec/AUTHORING.md`: the vocabulary a domain is written in, run `Emit` or `Native` |
//!
//! A domain is written once, in Rust, in the vocabulary of
//! `spec/AUTHORING.md` — the contract, read it first — through
//! [`authoring`]: run under `Emit` it is the module (the `.ark` every
//! runtime verifies and hashes), run under `Native` it applies entries
//! directly, and [`eval`] is what both mean. Nothing is generated into this
//! crate's private shape any more: `GENERATED.md`, `GENERATED-RUST.md` and
//! the prelude they named are retired, and so is `ark-builder`.
//!
//! No async, no sockets: every machine here is sans-io, and a transport is
//! a loop around one. The one `unsafe` is in [`authoring`], and says why.

pub mod authoring;
pub mod canon;
pub mod eval;
pub mod hash;
pub mod ir;
pub mod live;
pub mod log;
pub mod peer;
pub mod plan;
pub mod protocol;
pub mod schema;
pub mod sha256;
pub mod sim;
pub mod stdlib;
pub mod store;
pub mod unicode_tables;
pub mod value;
pub mod verify;
pub mod view;
