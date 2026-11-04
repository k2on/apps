//! §1.1, §1.16 A store's human form: every row a JSON object of its
//! columns, read under the types the schema gives them ([`row`]) and
//! written back as its row type's bytes in the value format
//! ([`row_bytes`]). What has no Rust type of a table — a vector's rows, a
//! host speaking JSON, an export — holds rows so.
//!
//! **A store whole** in its human form is its [`Tables`]: what an export
//! is ([`tables`]), and what an import makes a store of again, every row
//! judged as the store judges any ([`store`]). Data moves by an export, a
//! transform of the tables — a program's or a person's — and an import
//! under a schema that grows from the one exported (§1.2).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Map, Value as Json};

use crate::json::{typed_bytes, typed_value};
use crate::schema::{Schema, Space, Table};
use crate::store::Store;
use crate::Refusal;

/// Every row of every table, as objects of their columns, by table.
pub type Tables = BTreeMap<String, Vec<Map<String, Json>>>;

/// A table's row as JSON: an object of its columns.
pub fn row(table: &Table, bytes: &[u8]) -> Result<Json, Refusal> {
    typed_value(&table.row_type(), bytes).map_err(|e| Refusal(format!("a row of {}: {e}", table.name)))
}

/// A row's bytes from its JSON object, a column missing read as null: the
/// row's one encoding, as its Rust type's would be.
pub fn row_bytes(table: &Table, v: &Json) -> Result<Vec<u8>, Refusal> {
    typed_bytes(&table.row_type(), v)
}

/// A store's rows in their human form, table by table in key order.
pub fn tables(store: &Store) -> Result<Tables, Refusal> {
    let mut out = Tables::new();
    for (name, rs) in store.snapshot_rows() {
        let (_, t) = store.schema_arc().table(&name).expect("a store's table is its schema's");
        let objects = rs.iter().map(|r| match row(t, r)? {
            Json::Object(o) => Ok(o),
            _ => Err(Refusal(format!("a row of {name} is not an object"))),
        });
        let objects = objects.collect::<Result<_, _>>()?;
        out.insert(name, objects);
    }
    Ok(out)
}

/// A store of `schema`'s `space` holding exactly these rows, each judged:
/// a table it lacks is refused, a column it lacks left out, one a row
/// lacks its default (or null).
pub fn store(schema: Arc<Schema>, space: Space, tables: &Tables) -> Result<Store, Refusal> {
    if let Some(t) = tables.keys().find(|t| schema.table(t).is_none()) {
        return Err(Refusal(format!("no table {t}")));
    }
    let mut rows = vec![];
    for t in schema.tables.iter().filter(|t| t.space == space) {
        let objects = tables.get(&t.name).map_or(&[][..], |v| &v[..]);
        let bytes = objects.iter().map(|o| {
            let kept = t
                .columns
                .iter()
                .filter_map(|c| o.get(&c.name).or(c.default.as_ref()).map(|v| (c.name.clone(), v.clone())));
            row_bytes(t, &Json::Object(kept.collect()))
        });
        rows.push((t.name.clone(), bytes.collect::<Result<Vec<_>, _>>()?));
    }
    Store::from_rows(schema, space, &rows)
}
