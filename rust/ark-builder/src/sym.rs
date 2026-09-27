//! Fresh symbols. One counter for the whole process rather than one per
//! function, because an `Expr` method such as [`crate::Expr::map_some`]
//! binds a symbol with no function in reach — and a symbol only has to be
//! distinct from every other binder it could shadow. The encoding
//! alpha-normalises anyway (`Ark.Encode.normalize`), so the numbers a
//! builder chose never reach the file, let alone the hash.

use std::sync::atomic::{AtomicI64, Ordering};

use ark::ir::Sym;

static NEXT: AtomicI64 = AtomicI64::new(0);

/// A symbol no other binder in this process has used.
pub(crate) fn fresh() -> Sym {
    NEXT.fetch_add(1, Ordering::Relaxed)
}
