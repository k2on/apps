#![forbid(unsafe_code)]
//! Authoring Ark IR from Rust.
//!
//! A domain is written as ordinary code — a schema, then mutators, queries
//! and helpers, each a closure over a builder — and comes out as an
//! [`ark::ir::Module`], or as the canonical bytes `arkc verify` accepts
//! and `arkc gen` generates from. Nothing here reimplements the IR: every
//! node is the `ark` crate's, every byte is `ark::canon::encode`'s, and
//! the module hash is `ark::ir::module_hash`'s.
//!
//! ```
//! use ark_builder::*;
//!
//! let mut m = ModuleBuilder::new();
//! m.scope("playlists", |s| {
//!     s.table("playlist", |t| {
//!         t.id("id");
//!         t.text("name");
//!         t.key(&["id"]);
//!     });
//! });
//! m.mutator("create_playlist", "playlists", |f| {
//!     let id = f.new_id("id", "playlist");
//!     let name = f.arg("name", Ty::Text);
//!     let b = f.body();
//!     b.if_(name.trim().is_empty(), |b| b.refuse("a playlist needs a name"));
//!     b.put("playlist", record([("id", id), ("name", name.trim())]));
//! });
//! let module = m.build();
//! assert_eq!(module.functions.len(), 1);
//! ```
//!
//! What the builder holds the IR to by construction: a read (`select`,
//! `exists`, `get`) is always the whole right-hand side of a `let`, a
//! nested block's statements are inside it, a `match` binds a symbol its
//! arm can see, and symbols are never reused. What it does not do is type:
//! the verifier does, and the module is verified before anything runs it.
//!
//! Symbols are numbered by a process-wide counter and the author's names
//! for them are kept on the [`ark::ir::Function`] but never encoded; the
//! encoding alpha-normalises, so neither reaches the file.

mod block;
mod expr;
mod function;
mod module;
mod plan;
mod schema;
mod sym;
mod ty;

pub use ark::ir::CmpOp;
pub use ark::schema::Dir;
pub use ark::value::Value;

pub use block::Block;
pub use expr::{ctx_session, ctx_user, list, none, record, some, Expr, MaybeExpr};
pub use function::FunctionBuilder;
pub use module::ModuleBuilder;
pub use plan::{Plan, Pred};
pub use schema::{ScopeBuilder, TableBuilder};
pub use ty::Ty;
