//! A server's rows as a build from before the restructure (main at the
//! commit `migrate.sh` pins) reads them, in the form `arkdb import` takes:
//! `{"schema": …, "tables": {table: [row, …]}}`. Compiled inside a checkout
//! of that commit, against its own crates, so the new build keeps no reader
//! of the old disk format. Reads a copy: the data directory is not touched.
//!
//! `export-main DIR [SCHEMA]`: DIR is the server's data directory (its rows
//! in `DIR/log`); SCHEMA the schema the server ran, as JSON, for a disk
//! that keeps none beside its log.

use std::sync::Arc;

use ark_peer::{Disk, Files, Instance, Memory};
use serde_json::{json, Map, Value as Json};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, schema) = match &args[..] {
        [d] => (d, None),
        [d, s] => (d, Some(s)),
        _ => fail("usage: export-main DIR [SCHEMA]"),
    };
    let mut disk = Files::new(std::path::Path::new(dir).join("log")).unwrap_or_else(|e| fail(&format!("{dir}: {e}")));
    let read = |b: Vec<u8>, what: &str| -> ark::schema::Schema {
        serde_json::from_slice(&b).unwrap_or_else(|e| fail(&format!("{what}: {e}")))
    };
    let schema = match (disk.get("schema.json").unwrap(), schema) {
        (Some(b), _) => read(b, "log/schema.json"),
        (None, Some(s)) => read(std::fs::read(s).unwrap_or_else(|e| fail(&format!("{s}: {e}"))), s),
        (None, None) => fail("no log/schema.json: say the schema the server ran (export-main DIR SCHEMA)"),
    };
    let mut copy = Memory::default();
    for name in ["kv", "log"] {
        if let Some(b) = disk.get(name).unwrap() {
            copy.put(name, &b).unwrap();
        }
    }
    let i = Instance::open_under(Arc::new(schema.clone()), Box::new(copy));
    if let Some(why) = &i.failure {
        fail(why);
    }
    let mut tables = Map::new();
    for (name, rows) in i.store.snapshot_rows() {
        let (_, t) = schema.table(&name).expect("a store's table is its schema's");
        let rows = rows.iter().map(|r| ark::dynamic::row(t, r).unwrap_or_else(|e| fail(&e.0)));
        tables.insert(name, Json::Array(rows.collect()));
    }
    println!("{:#}", json!({"schema": schema, "tables": tables}));
}

fn fail(why: &str) -> ! {
    eprintln!("export-main: {why}");
    std::process::exit(1)
}
