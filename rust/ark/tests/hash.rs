//! The `hash/` vectors of v4 re-derived: the same demo rows under the same
//! schema, each its bytes in the value format, hashed by §1.3 — a leaf per row, `sha256(table ‖
//! row)`; a digest per table, the leaves' sum modulo 2^256; the
//! state hash, `sha256(schema_hash ‖ durable digests in schema order)`.
//! Written to `spec/vectors/hash/` when `ARK_WRITE_VECTORS` is set, and
//! otherwise held to what is there.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod json;
}

use std::path::PathBuf;

use ark::hash::{hex, sha256};
use ark::schema::Schema;
use ark::store::Store;
use demo::{item_row, playlist_row};
use support::json::{array, obj, object, quoted, row, schema};

/// `00000000-0000-0000-0000-000000000001`, v4's playlist.
fn favorites() -> Vec<u8> {
    let mut i = vec![0; 16];
    i[15] = 1;
    i
}

fn store_of(items: &[(&str, i64)]) -> Store {
    let p = favorites();
    let rows = vec![
        ("playlist".into(), vec![playlist_row(&p, "Favorites", "alice")]),
        ("item".into(), items.iter().map(|(t, n)| item_row(&p, t, *n)).collect()),
    ];
    Store::from_rows(demo::shared(), &rows).unwrap()
}

/// The store as the dialect writes one: tables by name in code-point
/// order, rows in key order.
fn store_json(sch: &Schema, s: &Store) -> String {
    let mut tables: Vec<_> = sch.tables.iter().collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    let parts: Vec<(&str, String)> = tables.iter().map(|t| (&t.name[..], array(s.rows(&t.name).map(|r| row(t, r))))).collect();
    object(&parts)
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors/hash")
}

fn vector(name: &str, text: String) {
    let path = dir().join(name);
    if std::env::var_os("ARK_WRITE_VECTORS").is_some() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(on_disk, text, "{name} moved; ARK_WRITE_VECTORS=1 writes it again");
}

/// v4's `hash/demo-state`: the playlist and three items. Falsified by
/// hashing the digests without the schema hash: the vector on disk moved.
#[test]
fn demo_state() {
    let sch = demo::schema();
    let s = store_of(&[("t11", 3), ("t7", 1), ("t9", 2)]);
    vector(
        "demo-state.json",
        obj(&[
            ("schema", schema(&sch)),
            ("schema_hash", quoted(&sch.hash().hex())),
            ("store", store_json(&sch, &s)),
            ("hash", quoted(&s.state_hash().hex())),
        ]),
    );
}

/// v4's `hash/leaves-and-digests`: every row's bytes and leaf, every
/// table's digest, and the hash from them — each recomputed here from the
/// definition, not read from the store. Falsified by summing leaves with a
/// carry dropped (`add256` without the carry): the store's digest of
/// `item` was not the sum.
#[test]
fn leaves_and_digests() {
    let sch = demo::schema();
    let s = store_of(&[("t7", 1), ("t9", 2)]);
    let mut digests = vec![];
    let tables = array(sch.tables.iter().map(|t| {
        let mut sum = [0u8; 32];
        let rows = array(s.rows(&t.name).map(|r| {
            let leaf = sha256(&[t.name.as_bytes(), r]);
            assert_eq!(leaf, Store::leaf(&t.name, r));
            let mut carry = 0u16;
            for i in (0..32).rev() {
                let x = sum[i] as u16 + leaf[i] as u16 + carry;
                sum[i] = x as u8;
                carry = x >> 8;
            }
            object(&[("bytes", quoted(&hex(r))), ("leaf", quoted(&hex(&leaf))), ("row", row(t, r))])
        }));
        assert_eq!(Some(sum), s.digest(&t.name), "{}", t.name);
        digests.extend_from_slice(&sum);
        object(&[("digest", quoted(&hex(&sum))), ("rows", rows), ("table", quoted(&t.name))])
    }));
    let hash = sha256(&[&sch.hash().0, &digests]);
    assert_eq!(hex(&hash), s.state_hash().hex());
    vector(
        "leaves-and-digests.json",
        obj(&[
            ("schema", schema(&sch)),
            ("schema_hash", quoted(&sch.hash().hex())),
            ("store", store_json(&sch, &s)),
            ("tables", tables),
            ("hash", quoted(&hex(&hash))),
        ]),
    );
}
