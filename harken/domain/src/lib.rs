//! harken's domain, written once in the vocabulary of `spec/AUTHORING.md`.
//!
//! Four files are the domain: `schema` (the tables), `library` (what the
//! scanner authors and every peer reads, and the helpers that derive the
//! keys), `playlists` (what people do), and `module` (the routers, in
//! order). The same text is the module and the code that applies it:
//! `module().emit()` is `harken.ark`, which every runtime verifies and
//! hashes, and `module().procedures()` are the procedures a Rust peer — the
//! server, the desktop — applies entries with natively, held to the
//! interpreter over their own emit by this crate's tests.
//!
//! Beside them, and not the domain: [`listening`] is the realtime protocol
//! of one account's audio session (never the log), [`view`] is what a
//! client maintains, and [`keys`] computes the derived keys on the host
//! with the helpers' own definitions.

// A row's columns are `pub const id: Col<..>` beside its fields, named as
// the fields are: the canonical spelling, not a Rust constant's.
#![allow(non_upper_case_globals)]
// A write mid-body is a statement: it is made when it is dropped at the
// `;`. `Write` asks to be used so that `.on(..)` is said before that, and
// the domain's bodies write several rows each.
#![allow(unused_must_use)]

pub mod keys;
pub mod library;
pub mod listening;
pub mod module;
pub mod playlists;
pub mod schema;
pub mod view;

pub use module::module;

/// The one scope every table is in, while the engine still has scopes.
pub const SCOPE: &str = "harken";
