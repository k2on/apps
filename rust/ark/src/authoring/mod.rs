//! Authoring: the vocabulary of `spec/AUTHORING.md`, in Rust.
//!
//! A domain is written once against this module — its tables are a struct
//! of [`Table`]s, a row a struct with its [`Row::columns`], an input a struct
//! with its [`Input::schema`], and every procedure a closure on a
//! [`Router`] — and the same text runs two ways:
//!
//! - **Emit** ([`Module::emit`], [`Module::build`]): every closure runs once
//!   with fresh symbols, a read becomes an `SLet`, a write a statement, an
//!   auto is registered by name; the result is the module, verified, whose
//!   bytes are what every runtime hashes. The lowering of each spelling is
//!   §6 of the contract, exactly.
//! - **Native** ([`Module::procedures`], [`Procedure::apply`]): every value
//!   is a value, reads run at once against the transaction, a `when` runs
//!   its closure only when it holds, `ctx.now(..)` is the entry's frozen
//!   auto. A peer holding a procedure applies entries through it
//!   ([`crate::peer::Replica::hold`]); one that does not replays the
//!   emitted closure through [`crate::eval`], and the two are held to each
//!   other by the tests on every procedure.
//!
//! A body cannot tell which: every value of the vocabulary is a handle into
//! the run in progress, an expression under one and a value under the
//! other.
//!
//! Where Rust differs from the contract's spelling, and why:
//!
//! - `.min(n)` / `.max(n)` and the rest take no message; `.why("…")` after a
//!   check gives it one (Rust has no overloading by arity, so
//!   `.min(n, "why")` beside `.min(n)` cannot be one method).
//! - The tables are said in [`Tables::open`], one [`table`] per field:
//!   the order written there is the schema's order.
//! - Natively, a method's operands are values before the call: `a.and(b)`,
//!   `pick(c, a, b)` and `opt.map_or(d, f)` have computed `b` or `d`
//!   already, where the IR evaluates only what it takes. The two differ
//!   only when the operand not taken would itself fault.

mod control;
mod cx;
mod helper;
mod input;
mod raw;
mod router;
mod schema;
mod values;

pub use control::{for_each, if_else, refuse, unless, when};
pub use cx::H;
pub use helper::{helper, Body, Params};
pub use input::{bool_, bytes, enum_, id, int, object, opt, text, FieldB, Input, Object, Variants};
pub use router::{evaluate, router, Applied, Module, Proc, Procedure, Route, Router, Routers, Routes};
pub use schema::{
    col, columns, fields, rel, table, table_of, Col, Cols, ColumnOf, Columns, Effect, Fields, IntoEffect, Key, Order, Orders, Pred, Query, Record,
    Rel, Row, Table, Tables, Write,
};
pub use values::{concat, ctx, id_of_text, list, nil_id, none, pick, some, Bool, Bytes, Ctx, Data, Id, Int, List, ListOf, Opt, Text};
