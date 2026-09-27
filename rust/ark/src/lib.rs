#![forbid(unsafe_code)]
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
//! | [`eval`] | `Ark.Eval`: the interpreter generated code must agree with |
//! | [`hash`] | `Ark.Hash`: the state hash, closures, the function hash |
//! | [`log`] | `Ark.Log`: entries, facts, snapshots, the horizon |
//! | [`peer`] | `Ark.Peer`: the replica and the authority |
//! | [`protocol`] | `Ark.Protocol`: the frames, the client and server machines |
//! | [`view`] | `Ark.View`: incremental views and their contract |
//! | [`live`] | `Ark.Live`: rooms per account over opaque frames |
//! | [`sim`] | `Ark.Sim`: the seeded fleet |
//! | [`gen`] | the prelude generated code imports (GENERATED.md) |
//!
//! No async, no sockets: every machine here is sans-io, and a transport is
//! a loop around one.

pub mod canon;
pub mod db;
pub mod eval;
pub mod fault;
pub mod gen;
pub mod hash;
pub mod ir;
pub mod live;
pub mod log;
pub mod ops;
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
pub mod view;
