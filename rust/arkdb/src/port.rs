//! `arkdb export [--from DIR]` and `arkdb import FILE SCHEMA [--to DIR]`:
//! a server's rows in their human form (`ark::dynamic`), out and in. The
//! export is `{"schema": …, "tables": {table: [row, …]}}`, read from the
//! data directory's log (`DIR/log`) under the schema it keeps beside it,
//! without disturbing a server writing it; the import makes a transformed
//! export's tables the rows of an empty data directory under `SCHEMA` (the
//! new build's, as JSON, which grows from the export's), every row judged,
//! the log starting afresh. Data moves within a schema that only grows by
//! the one, a transform of the tables, and the other.
//! The files rows name (`DIR/log/files`) are their bytes' names, and are
//! copied as they are.

use std::path::Path;
use std::sync::Arc;

use ark_peer::{disk, Files};
use serde_json::{json, Value as Json};

const USAGE: &str = "arkdb export [--from DIR]\narkdb import FILE SCHEMA [--to DIR]";

fn dir(args: &[String], flag: &str) -> Result<(Vec<String>, String), String> {
    let mut rest = vec![];
    let (mut it, mut dir) = (args.iter(), ".".to_string());
    while let Some(a) = it.next() {
        match a.as_str() {
            f if f == flag => dir = it.next().cloned().ok_or(USAGE)?,
            _ => rest.push(a.clone()),
        }
    }
    Ok((rest, dir))
}

fn log(dir: &str) -> Result<Files, String> {
    Files::new(Path::new(dir).join("log")).map_err(|e| format!("{dir}: {e}"))
}

/// Every row of the data directory, as JSON.
pub fn export(args: &[String]) -> Result<String, String> {
    let (rest, dir) = dir(args, "--from")?;
    if !rest.is_empty() {
        return Err(USAGE.into());
    }
    let (schema, tables) = disk::export(&mut log(&dir)?).map_err(|e| e.to_string())?;
    Ok(format!("{:#}\n", json!({"schema": schema, "tables": tables})))
}

/// A transformed export's tables, the rows of an empty data directory.
pub fn import(args: &[String]) -> Result<String, String> {
    let (rest, dir) = dir(args, "--to")?;
    let [file, schema] = &rest[..] else { return Err(USAGE.into()) };
    let read = |f: &str| std::fs::read(f).map_err(|e| format!("{f}: {e}"));
    let rows: Json = serde_json::from_slice(&read(file)?).map_err(|e| format!("{file}: {e}"))?;
    let tables = serde_json::from_value(rows["tables"].clone()).map_err(|e| format!("{file}: tables: {e}"))?;
    let from = serde_json::from_value(rows["schema"].clone()).map_err(|e| format!("{file}: schema: {e}"))?;
    let schema = serde_json::from_slice(&read(schema)?).map_err(|e| format!("{schema}: {e}"))?;
    disk::import(&mut log(&dir)?, &from, Arc::new(schema), &tables).map_err(|e| e.to_string())?;
    Ok(String::new())
}
