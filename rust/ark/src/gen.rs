//! The prelude generated code imports: `use ark::gen::*;`.
//!
//! Exactly what GENERATED.md names, in Rust's spelling — snake case for
//! every function, `?` after every call that may fault, and nothing else
//! different:
//!
//! - [`Value`] with `Value::null()`, `bool`, `int`, `text`, `bytes`,
//!   `bytes_hex`, `id`, `id_hex`, `opt`, `list`, `record`; and `is_null`,
//!   `as_bool`, `as_int`, `as_text`, `as_list`, `field`, fatal on a mismatch.
//!   `record` takes `Vec<(String, Value)>` (any `Into<String>` key);
//!   `as_list` hands back owned `Value`s so `for v in xs.as_list()` binds a
//!   `Value`; `as_text` is a `&str`.
//! - [`Id`], the runtime's 16-byte id, with `Value::id(Id)`.
//! - [`Fault`] with `Fault::refuse(text)` and `Fault::bug(text)`; a fault
//!   is `Err(Fault)`.
//! - [`Ctx`] is `Ctx { user: String, session: String }`; generated code
//!   spells `Value::text(ctx.user.clone())`.
//! - [`Args`] is `BTreeMap<String, Value>`, so `Args::from([("k".to_string(),
//!   v)])` builds one; `Ops::arg(&args, "k")` reads one, cloned.
//! - [`Db`] is `db`: `db.get(table, vec![key…])` and `db.exists(...)` return a
//!   `Value`, `db.select(&plan)` a `Value::list`, `db.put(table, row)?` and
//!   `db.delete(table, vec![key…])?` fault with the store's refusal. Tables
//!   are `&str`, keys `Vec<Value>`.
//! - [`Plan`] and [`Pred`] builders, [`Dir`], [`CmpOp`].
//! - [`Ops`]: `add`, `sub`, `mul`, `div`, `r#mod` (`mod` is a keyword), `neg`
//!   each `Result<Value, Fault>`; `cmp(CmpOp, a, b)` and `not(a)` return a
//!   `Value`; `arg(&args, name)` a `Value`; `match_opt(opt, |v| ..., || ...)`,
//!   `map`, `filter`, `any`, `all`, `sort_by`, `fold` take closures returning
//!   `Result<Value, Fault>`.
//! - [`Std`]: one associated function per standard function, snake case,
//!   each `Result<Value, Fault>`.
//! - [`Store`], the trait a runtime's backend implements, for the signature
//!   of `apply`/`query` and for `run_mutator`.

pub use crate::db::{run_mutator, Db};
pub use crate::eval::{Args, Ctx};
pub use crate::fault::Fault;
pub use crate::ir::{CmpOp, Plan, Pred};
pub use crate::ops::Ops;
pub use crate::schema::Dir;
pub use crate::stdlib::Std;
pub use crate::store::Store;
pub use crate::value::{Id, Value};
