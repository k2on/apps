//! `docs/plan-guards.md` D4 The authority's raw writes: two functions in
//! every module by construction, as the standard library is, that write a
//! row or take one away with no domain logic in between — what the
//! explorer's raw switch and an operator's repair are.
//!
//! ```text
//! ark.put_row(table: Text, row: Struct)      the row, judged as `put` is
//! ark.delete_row(table: Text, key: List)     the row under the key, as `delete` is
//! ```
//!
//! **Named by fixed hashes.** An entry names its function by hash
//! ([`crate::log::Entry::fn_hash`]); these two are no module's, so their
//! hashes are not of a closure but of the name: `sha256(enc(Text name))`,
//! the canonical encoding of the name as a text ([`hash_of`]). A closure's
//! hash is of a closure's encoding, a struct, so the two can never be one
//! hash. Nothing emits them into an `.ark` and no client loads them; every
//! peer knows them, because the engine runs them natively here ([`apply`]),
//! judged by the table's constraints exactly as a mutator's `put` and
//! `delete` are — so a confirmed entry naming one replays on every whole
//! peer like any other, and a partial peer takes its facts, filtered to its
//! union, like any other (D2). They are deterministic, and carry no auto.
//!
//! **The authority's alone.** [`crate::peer::Authority::edit`] is the one
//! way to author one, and only a server's authority may
//! ([`crate::peer::Authority::private`], the flag D3 gave it): its actor is
//! the server's own identity. A replica has no path to them
//! ([`crate::peer::Replica::mutate`] refuses), nor does a peer alone, which
//! is a client here; an authority sequencing an intent refuses one
//! ([`crate::peer::Authority::sequence_entry`]); and a server refuses one
//! pushed by any connection with [`Refusal::Forbidden`] before anything else
//! is asked of it ([`crate::protocol::Server`]).

use std::sync::OnceLock;

use crate::canon::encode;
use crate::eval::{Args, EvalError};
use crate::hash::FnHash;
use crate::schema::Schema;
use crate::sha256::sha256;
use crate::store::{self, Change, Refusal, Store};
use crate::value::Value;

/// The actor of every raw write a server makes: the authority's own user,
/// under a login that is the server instance's. No account is it, and no
/// connection is held to it, since none may push one.
pub const AUTHOR: &str = "authority";

/// The name `ark.put_row` is hashed from.
pub const PUT_ROW: &str = "ark.put_row";
/// The name `ark.delete_row` is hashed from.
pub const DELETE_ROW: &str = "ark.delete_row";

/// Which of the two an entry names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raw {
    PutRow,
    DeleteRow,
}

impl Raw {
    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Raw::PutRow => PUT_ROW,
            Raw::DeleteRow => DELETE_ROW,
        }
    }

    /// Its fixed hash.
    pub fn hash(self) -> &'static FnHash {
        let [put, delete] = hashes();
        match self {
            Raw::PutRow => put,
            Raw::DeleteRow => delete,
        }
    }
}

/// `sha256(enc(Text name))`: how a function no module carries is named.
pub fn hash_of(name: &str) -> FnHash {
    sha256(&encode(&Value::text(name)))
}

fn hashes() -> &'static [FnHash; 2] {
    static HASHES: OnceLock<[FnHash; 2]> = OnceLock::new();
    HASHES.get_or_init(|| [hash_of(PUT_ROW), hash_of(DELETE_ROW)])
}

/// Which raw write a hash names, if it names one.
pub fn of(fh: &[u8]) -> Option<Raw> {
    let [put, delete] = hashes();
    if fh == put.as_slice() {
        Some(Raw::PutRow)
    } else if fh == delete.as_slice() {
        Some(Raw::DeleteRow)
    } else {
        None
    }
}

/// Whether a hash names a raw write.
pub fn is_raw(fh: &[u8]) -> bool {
    of(fh).is_some()
}

/// The table an entry of a raw write writes, as its arguments name it; the
/// function's name where they do not. What a refusal of one is about.
pub fn table_of(raw: Raw, args: &Args) -> String {
    match args.get("table") {
        Some(Value::Text(t)) => t.to_string(),
        _ => raw.name().to_string(),
    }
}

/// The raw write that makes `change`, and its arguments: an `Add` or an
/// `Edit` puts the new row, a `Remove` deletes the row's key — under the
/// schema's key for its table, the row's own columns where the table is
/// unknown (and the write is then refused as `NoSuchTable`, as any is).
pub fn call_of(schema: &Schema, change: &Change) -> (Raw, Args) {
    let mut args = Args::new();
    args.insert("table".into(), Value::text(change.table()));
    match change {
        Change::Add(_, row) | Change::Edit(_, _, row) => {
            args.insert("row".into(), row.to_value());
            (Raw::PutRow, args)
        }
        Change::Remove(t, row) => {
            let key = match schema.lookup_table(t) {
                Some(tbl) => tbl.key_of(row),
                None => vec![],
            };
            args.insert("key".into(), Value::List(key.into()));
            (Raw::DeleteRow, args)
        }
    }
}

/// Apply a raw write to a store, as [`crate::eval::apply_closure`] applies a
/// closure: `Ok(Ok)` the changes, already applied; `Ok(Err)` the verdict —
/// the table's constraints, exactly as a mutator's `put` (§4.3) and
/// `delete` (§4.4) are judged — with the store untouched; `Err` a bug,
/// arguments that are not the function's. A put of the row already there is
/// no change; a delete of a key nothing has is none either.
pub fn apply(raw: Raw, args: &Args, store: &mut dyn Store) -> Result<Result<Vec<Change>, Refusal>, EvalError> {
    let table = match args.get("table") {
        Some(Value::Text(t)) => t.to_string(),
        Some(other) => return Err(EvalError::TypeError(format!("{}: table is a text, not {other:?}", raw.name()))),
        None => return Err(EvalError::MissingArg("table".into())),
    };
    let written = match raw {
        Raw::PutRow => {
            let row = match args.get("row") {
                Some(Value::Struct(m)) => (**m).clone(),
                Some(other) => return Err(EvalError::TypeError(format!("{PUT_ROW}: row is a struct, not {other:?}"))),
                None => return Err(EvalError::MissingArg("row".into())),
            };
            let row = store::row_for(store.as_store(), &table, row);
            store.put(&table, row)
        }
        Raw::DeleteRow => {
            let key = match args.get("key") {
                Some(Value::List(k)) => k.clone(),
                Some(other) => return Err(EvalError::TypeError(format!("{DELETE_ROW}: key is a list, not {other:?}"))),
                None => return Err(EvalError::MissingArg("key".into())),
            };
            store.delete(&table, &key)
        }
    };
    Ok(written.map(|ch| ch.into_iter().collect()))
}
