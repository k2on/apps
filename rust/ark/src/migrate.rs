//! §1.2's other half: a schema that does not grow from the one rows were
//! written under — a table rekeyed, a column renamed or retyped — reads them
//! through the domain's migrations, steps over the rows as JSON.
//!
//! A step names the schema it reads (`from`, pinned as JSON beside the
//! domain, as it stood when the step was written) and is a function over
//! every row of every table, as objects of their columns. Its rows are
//! written under the next step's `from`, or the domain's schema after the
//! last. Rows of a schema that grows into a step's `from` are read under it
//! as any rows are ([`Table::fit`](crate::schema::Table::fit)), so a step
//! covers every schema before it that grows into it; a table its next
//! schema lacks is dropped.
//!
//! Only rows are migrated, never a log's entries: an entry is facts under
//! the schema it was logged under. What migrates rows folds the log into
//! them first, and what reads them after stands on them alone.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Map, Value as Json};

use crate::dynamic::{row, row_bytes};
use crate::log::Rows;
use crate::schema::{compat, Schema, Space};
use crate::store::Store;
use crate::Refusal;

/// Every row of every table, as objects of their columns, by table.
pub type Tables = BTreeMap<String, Vec<Map<String, Json>>>;

/// One step: the schema it reads and what it makes of the rows.
#[derive(Clone)]
pub struct Migration {
    pub name: String,
    pub from: Arc<Schema>,
    pub run: fn(&mut Tables) -> Result<(), Refusal>,
}

impl std::fmt::Debug for Migration {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Migration({}, from {:?})", self.name, self.from.hash())
    }
}

impl Migration {
    /// A step reading the schema `from` (its JSON, as `ARK_WRITE_GEN`
    /// writes one).
    pub fn new(name: &str, from: &str, run: fn(&mut Tables) -> Result<(), Refusal>) -> Migration {
        let from = serde_json::from_str(from).unwrap_or_else(|e| panic!("{name}: its schema does not read: {e}"));
        Migration {
            name: name.into(),
            from: Arc::new(from),
            run,
        }
    }
}

/// Rows written under `was`, as `to` reads them: `None` when it reads them
/// as they are (the same schema, or one grown from it), the steps' rows
/// when one does, and a refusal when no step reads them.
pub fn migrate(was: &Schema, rows: &Rows, to: &Arc<Schema>, steps: &[Migration]) -> Result<Option<Rows>, Refusal> {
    if compat(was, to).is_ok() {
        return Ok(None);
    }
    let first = steps
        .iter()
        .position(|m| compat(was, &m.from).is_ok())
        .ok_or_else(|| Refusal(format!("no migration reads rows of schema {}", was.hash().hex())))?;
    let mut rows = rows.clone();
    for (i, m) in steps.iter().enumerate().skip(first) {
        let next = steps.get(i + 1).map_or(to, |n| &n.from);
        let mut tables = read(&m.from, &rows).map_err(|e| Refusal(format!("{}: {}", m.name, e.0)))?;
        (m.run)(&mut tables).map_err(|e| Refusal(format!("{}: {}", m.name, e.0)))?;
        rows = written(next, &tables).map_err(|e| Refusal(format!("{}: {}", m.name, e.0)))?;
    }
    Ok(Some(rows))
}

/// Rows of a schema that grows into `schema`, as objects under it.
fn read(schema: &Arc<Schema>, rows: &Rows) -> Result<Tables, Refusal> {
    let (store, _) = Store::from_rows(schema.clone(), Space::Durable, rows)?;
    let mut out = Tables::new();
    for (name, rs) in store.snapshot_rows() {
        let (_, t) = schema.table(&name).expect("a store's table is its schema's");
        let objects = rs.iter().map(|r| match row(t, r)? {
            Json::Object(o) => Ok(o),
            _ => Err(Refusal(format!("a row of {name} is not an object"))),
        });
        let objects = objects.collect::<Result<_, _>>()?;
        out.insert(name, objects);
    }
    Ok(out)
}

/// Objects under `schema` as its rows; a table it lacks dropped, a column
/// it lacks left out, every row judged as the store judges them.
fn written(schema: &Arc<Schema>, tables: &Tables) -> Result<Rows, Refusal> {
    let mut rows = vec![];
    for t in schema.tables.iter().filter(|t| t.space == Space::Durable) {
        let objects = tables.get(&t.name).map_or(&[][..], |v| &v[..]);
        let bytes = objects
            .iter()
            .map(|o| {
                let kept = t.columns.iter().filter_map(|c| o.get(&c.name).map(|v| (c.name.clone(), v.clone())));
                row_bytes(t, &Json::Object(kept.collect()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows.push((t.name.clone(), bytes));
    }
    Store::from_rows(schema.clone(), Space::Durable, &rows)?;
    Ok(rows)
}
