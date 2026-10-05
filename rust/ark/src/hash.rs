//! §8 Hashes, as `Ark.Hash` defines them: two quantities every runtime must
//! reproduce byte for byte, both SHA-256 over canonical CBOR — the state
//! hash (§8.1), which moves with the store, and the function hash (§8.2).
//!
//! # §8.1 The state hash
//!
//! Exactly, with `enc` the canonical encoding (§1.3) and `‖` concatenation:
//!
//! ```text
//! leaf(t, row)  = sha256(enc(Text t) ‖ enc(Struct row))
//! digest(t)     = Σ leaf(t, row) over the rows of t, mod 2^256
//!                 (each leaf read as a big-endian unsigned integer, the
//!                 sum written back as 32 big-endian bytes; a table with
//!                 no rows has 32 zero bytes)
//! state_hash(s) = sha256(enc(List [List [Text t, Bytes digest(t)]
//!                                  for every table t of the schema,
//!                                  in schema order]))
//! ```
//!
//! `Struct row` is the row as §4 has it — every column by name — so a
//! leaf does not depend on how a runtime lays a row out, and `Text t` is
//! the table's name encoded on its own, so the name is delimited and no
//! table's leaf can be read as another's (`docs/plan-db.md` D3).
//!
//! **Why a sum** (`docs/plan-db.md` D3). Until spec version 4 the state hash
//! was SHA-256 over every table's rows in key order: O(rows) per `Verify`,
//! so verifying was a diagnostic rather than a habit. A sum of leaves does
//! not depend on order and moves by one addition and one subtraction per
//! row written, so a store that reports its changes (§4.1) keeps its
//! digests beside its rows ([`crate::store::MemoryStore`]), an overlay
//! derives its own from its writes ([`crate::store::Overlay`]), and a
//! `Verify` costs one SHA-256 over as many pairs as there are tables. A
//! store that keeps none is summed by scanning, which is the definition
//! and therefore always right ([`table_digest`]).
//!
//! What a sum is not: a multiset hash of 256 bits is no defence against
//! somebody *choosing* rows to reach a given digest (Wagner's generalised
//! birthday attack finds such a set far below 2^128 work). It is a check
//! that two replicas which applied the same log hold the same rows, which
//! is the question a `Verify` asks; it is not a commitment a stranger's
//! snapshot can be trusted on without replaying it.

use std::collections::BTreeMap;

use sha2::{Digest as _, Sha256};

use crate::canon::encode;
use crate::ir::encode::{function_value, module_value, reaches};
use crate::ir::normalize::{normalize, normalize_module};
use crate::ir::{Function, Module};
use crate::sha256::sha256;
use crate::store::{Row, Store};
use crate::value::Value;

/// The 32-byte hash an entry names its function by.
pub type FnHash = Vec<u8>;

/// §8.3 A function with the helpers it was verified against: every helper
/// and middleware the function reaches, in the version current when it was
/// hashed, in declaration order — a complete program `apply_closure` runs
/// without consulting any module. A procedure runs its middleware in its
/// own `uses` order, looked up here by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closure {
    pub function: Function,
    pub helpers: Vec<Function>,
}

/// The closure of a function within a module: the function normalised,
/// with the helpers it reaches, normalised, in the module's order
/// (`Ark.Hash.closure`).
pub fn closure(m: &Module, f: &Function) -> Closure {
    let mut seen: Vec<String> = Vec::new();
    let mut todo: Vec<String> = reaches(f);
    todo.reverse();
    while let Some(n) = todo.pop() {
        if seen.contains(&n) {
            continue;
        }
        if let Some(h) = m.lookup_function(&n) {
            seen.push(n);
            let mut more = reaches(h);
            more.reverse();
            todo.extend(more);
        }
    }
    Closure {
        function: normalize(f),
        helpers: m.functions.iter().filter(|h| seen.contains(&h.name)).map(normalize).collect(),
    }
}

/// Every function of a module, by the hash of its closure
/// (`Ark.Hash.closures`).
pub fn closures(m: &Module) -> BTreeMap<FnHash, Closure> {
    m.functions
        .iter()
        .map(|f| {
            let c = closure(m, f);
            (function_hash(&c), c)
        })
        .collect()
}

/// §8.1 A row's leaf: `sha256(enc(Text table) ‖ enc(Struct row))`, read
/// as a 256-bit big-endian integer when it is summed.
pub fn leaf(table: &str, row: &Row) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(encode(&Value::text(table)));
    h.update(encode(&row.to_value()));
    h.finalize().into()
}

/// §8.1 A table's digest: the sum of its rows' leaves modulo 2^256, as 32
/// big-endian bytes. The empty table's is zero. Addition is commutative,
/// so the digest is of the table's rows as a set, whatever order they are
/// held or arrive in; subtraction is its inverse, so a row that goes takes
/// exactly what it brought.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    /// The digest of no rows.
    pub const ZERO: Digest = Digest([0; 32]);

    /// This digest with a leaf added, mod 2^256.
    pub fn add(&mut self, leaf: &[u8; 32]) {
        let mut carry = 0u16;
        for i in (0..32).rev() {
            let s = u16::from(self.0[i]) + u16::from(leaf[i]) + carry;
            self.0[i] = s as u8;
            carry = s >> 8;
        }
    }

    /// This digest with a leaf taken away, mod 2^256: the inverse of
    /// [`Digest::add`].
    pub fn sub(&mut self, leaf: &[u8; 32]) {
        let mut borrow = 0i16;
        for i in (0..32).rev() {
            let d = i16::from(self.0[i]) - i16::from(leaf[i]) - borrow;
            self.0[i] = d.rem_euclid(256) as u8;
            borrow = i16::from(d < 0);
        }
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Digest({})", crate::value::hex(&self.0))
    }
}

/// §8.1 The digest of some rows of `table`, summed one by one: the
/// definition, and what [`state_hash`] falls back to for a store that keeps
/// no digest.
pub fn table_digest<'r>(table: &str, rows: impl IntoIterator<Item = &'r Row>) -> Digest {
    let mut d = Digest::ZERO;
    for r in rows {
        d.add(&leaf(table, r));
    }
    d
}

/// §8.1 The state hash from every table's digest, given in schema order:
/// the SHA-256 of the canonical encoding of the list of `[name, digest]`
/// pairs.
pub fn state_hash_of<'t>(digests: impl IntoIterator<Item = (&'t str, Digest)>) -> Vec<u8> {
    let pairs: Vec<Value> = digests
        .into_iter()
        .map(|(t, d)| Value::list(vec![Value::text(t), Value::Bytes(d.0.into())]))
        .collect();
    sha256(&encode(&Value::from(pairs)))
}

/// Which construction of the state hash a stored snapshot's hash is by: 2
/// is §8.1 as above, 1 the one before it ([`state_hash_v1`]). A snapshot
/// written down — the server's `log.ark-log`, a peer alone's `log`, a
/// client's `replica` record — carries it as `hashing`, and one without the
/// field was written before there was a second, so reads as 1. A snapshot of
/// an older construction is checked by that one and hashed again by this
/// one when it is opened, never refused (`docs/plan-db.md` D3, Landed).
pub const HASH_VERSION: i64 = 2;

/// §8.1 as it was until `docs/plan-db.md` D3: SHA-256 of the canonical
/// encoding of the list of every table's `[name, rows]`, schema order, rows
/// in key order. Kept for one purpose — checking the hash of a snapshot
/// written before the change ([`HASH_VERSION`] 1) when it is opened — and
/// O(rows), which is why it is the state hash no longer.
pub fn state_hash_v1(st: &dyn Store) -> Vec<u8> {
    let tables: Vec<Value> = st
        .schema()
        .tables()
        .map(|t| {
            Value::list(vec![
                Value::text(&t.name),
                Value::List(st.scan(&t.name).into_iter().map(Row::into_value).collect()),
            ])
        })
        .collect();
    sha256(&encode(&Value::from(tables)))
}

/// The state hash of a store by the construction `version` names
/// ([`HASH_VERSION`]); `None` for one this build does not know.
pub fn state_hash_by(version: i64, st: &dyn Store) -> Option<Vec<u8>> {
    match version {
        1 => Some(state_hash_v1(st)),
        HASH_VERSION => Some(state_hash(st)),
        _ => None,
    }
}

/// §8.1 The state hash of a store: every table of its schema, in schema
/// order, with the digest the store keeps for it ([`Store::digest`]) — read,
/// not computed, for a [`crate::store::MemoryStore`] — or, for a store that
/// keeps none, the digest of a scan. Tables with no rows contribute their
/// name and a zero digest.
pub fn state_hash(st: &dyn Store) -> Vec<u8> {
    let tables: Vec<&str> = st.schema().tables().map(|t| t.name.as_str()).collect();
    state_hash_of(
        tables
            .into_iter()
            .map(|t| (t, st.digest(t).unwrap_or_else(|| table_digest(t, &st.scan(t))))),
    )
}

/// §8.2 The hash of a function: over its normalised canonical form, names
/// excluded, together with the hashes of the helpers it calls and the
/// middleware it uses directly — which cover theirs in turn, so that
/// editing a middleware re-hashes every procedure that runs it.
pub fn function_hash(c: &Closure) -> FnHash {
    let mut deps = BTreeMap::new();
    for n in reaches(&c.function) {
        if let Some(h) = c.helpers.iter().find(|h| h.name == n) {
            deps.insert(
                n,
                Value::bytes(function_hash(&Closure {
                    function: h.clone(),
                    helpers: c.helpers.clone(),
                })),
            );
        }
    }
    sha256(&encode(&function_value(&deps, &c.function)))
}

/// The hash of a whole module, normalised.
pub fn module_hash(m: &Module) -> Vec<u8> {
    sha256(&encode(&module_value(&normalize_module(m))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Column, Schema, Table, Ty};
    use crate::store::{MemoryStore, Overlay, Store};

    fn n(x: u8) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[31] = x;
        b
    }

    /// Addition carries across every byte and wraps at 2^256; subtraction
    /// borrows and is its inverse.
    #[test]
    fn a_digest_is_arithmetic_modulo_two_to_the_256() {
        let mut d = Digest([0xff; 32]);
        d.add(&n(1));
        assert_eq!(d, Digest::ZERO, "all ones plus one wraps to zero");
        d.sub(&n(1));
        assert_eq!(d, Digest([0xff; 32]), "zero minus one wraps to all ones");
        let mut d = Digest(n(0xff));
        d.add(&n(1));
        let mut want = [0u8; 32];
        want[30] = 1;
        assert_eq!(d, Digest(want), "the carry moves up a byte");
        let (a, b) = (leaf("t", &row(1, "a")), leaf("t", &row(2, "b")));
        let mut x = Digest::ZERO;
        x.add(&a);
        x.add(&b);
        let mut y = Digest::ZERO;
        y.add(&b);
        y.add(&a);
        assert_eq!(x, y, "the order rows are summed in is no part of the digest");
        x.sub(&a);
        assert_eq!(x, Digest(b), "a row taken away takes exactly its leaf");
    }

    fn schema() -> Schema {
        let col = |n: &str, ty: Ty, nullable: bool| Column {
            name: n.into(),
            ty,
            nullable,
        };
        Schema {
            tables: vec![Table::new(
                "t",
                vec![col("id", Ty::Int, false), col("name", Ty::Text, true)],
                vec!["id".into()],
                vec![],
                vec![],
            )],
        }
    }

    fn row(id: i64, name: &str) -> Row {
        Row::of(
            &schema().tables[0],
            [("id".to_string(), Value::Int(id)), ("name".to_string(), Value::text(name))],
        )
    }

    /// The leaf is `sha256(enc(Text t) ‖ enc(Struct row))`, written out
    /// here from its parts rather than through [`leaf`].
    #[test]
    fn a_leaf_is_the_table_then_the_row() {
        let r = row(1, "a");
        let mut bytes = encode(&Value::text("t"));
        bytes.extend(encode(&r.to_value()));
        assert_eq!(leaf("t", &r).to_vec(), sha256(&bytes));
        assert_ne!(leaf("t", &r), leaf("u", &r), "the table is in the leaf");
    }

    /// The store keeps what a scan would sum, through puts, edits, deletes
    /// and back to empty; an overlay's delta over it does the same; and the
    /// state hash reads them, matching the pairs written out by hand.
    #[test]
    fn the_kept_digest_is_the_scanned_one() {
        let sch = schema();
        let mut st = MemoryStore::empty(sch.clone());
        let scanned = |st: &dyn Store| table_digest("t", &st.scan("t"));
        assert_eq!(st.digest("t"), Some(Digest::ZERO));
        st.put("t", row(1, "a")).unwrap();
        st.put("t", row(2, "b")).unwrap();
        st.put("t", row(1, "c")).unwrap();
        assert_eq!(st.digest("t"), Some(scanned(&st)));
        assert_eq!(state_hash(&st), state_hash_of([("t", scanned(&st))]));
        {
            let mut o = Overlay::new(&st);
            o.put("t", row(3, "d")).unwrap();
            o.put("t", row(2, "e")).unwrap();
            o.delete("t", &[Value::Int(1)]).unwrap();
            assert_eq!(o.digest("t"), Some(scanned(&o)), "the overlay's delta over the base");
        }
        st.delete("t", &[Value::Int(1)]).unwrap();
        st.delete("t", &[Value::Int(2)]).unwrap();
        assert_eq!(st.digest("t"), Some(Digest::ZERO), "every row gone is zero again");
        assert_eq!(st, MemoryStore::empty(sch), "and the store is the empty one");
        assert_eq!(st.digest("nope"), None, "a table the schema lacks has no digest");
    }
}
