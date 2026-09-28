//! harken's domain, written once in the vocabulary of `spec/AUTHORING.md`.
//!
//! Four files: `schema` (the scopes and their tables), `library` (what the
//! scanner authors and every peer reads), `playlists` (what people do), and
//! `module` (the routers, in order). The same text is the module and the
//! code that applies it: `module().emit()` is `harken.ark`, which every
//! runtime verifies and hashes, and `module().procedures()` are the
//! procedures a Rust peer — the server, the desktop — applies entries with
//! natively, held to the interpreter over their own emit by this crate's
//! tests.

// A row's columns are `pub const id: Col<..>` beside its fields, named as
// the fields are: the canonical spelling, not a Rust constant's.
#![allow(non_upper_case_globals)]

pub mod library;
pub mod module;
pub mod playlists;
pub mod schema;

pub use module::module;

/// The scope the tracks live in.
pub const LIBRARY: &str = "library";
/// The scope the playlists live in.
pub const PLAYLISTS: &str = "playlists";
