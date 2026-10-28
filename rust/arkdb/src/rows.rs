//! `arkdb rows TABLE [COLUMN=VALUE]… [--sort [-]COLUMN] [-n N] [--from DIR]`:
//! a server's rows, read from its data directory as it keeps them — its
//! log (`DIR/log`) under the schema it keeps beside it
//! (`ark_peer::disk::SCHEMA`) — a JSON object a line.
//!
//! The rows are the table's whose columns equal the values given, sorted
//! by a column (`-` before it, last first) or else by key, the first `N`:
//! the store's one query over rows without a Rust type
//! ([`ark::dynamic::Browse`]). A value reads as JSON where it does
//! (`pos=3`, `on=true`), and as a string where not (`kind=sermon`).
//!
//! Nothing is written: the log is copied into memory and read there, so a
//! server writing it meanwhile is undisturbed. A record it was half way
//! through appending is cut from the copy, as an open cuts a torn tail.

use std::path::Path;
use std::sync::Arc;

use ark::dynamic::{row, Browse};
use ark::schema::Schema;
use ark_peer::disk::SCHEMA;
use ark_peer::{Instance, Memory};
use serde_json::Value as Json;

const USAGE: &str = "arkdb rows TABLE [COLUMN=VALUE]... [--sort [-]COLUMN] [-n N] [--from DIR]";

/// The rows asked for, each as its line.
pub fn main(args: &[String]) -> Result<String, String> {
    let (mut browse, mut dir) = (None::<Browse>, ".".to_string());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| it.next().cloned().ok_or(format!("{flag} wants a value\n{USAGE}"));
        match a.as_str() {
            "--from" => dir = value("--from")?,
            "-n" => {
                let n = value("-n")?.parse().map_err(|_| format!("-n wants a number\n{USAGE}"))?;
                browse = browse.map(|b| b.take(n));
            }
            "--sort" => {
                let c = value("--sort")?;
                let (column, desc) = c.strip_prefix('-').map_or((c.as_str(), false), |c| (c, true));
                browse = browse.map(|b| b.sort(column, desc));
            }
            _ => match (browse.take(), a.split_once('=')) {
                (None, None) => browse = Some(Browse::table(a)),
                (Some(b), Some((column, v))) => {
                    let v = serde_json::from_str(v).unwrap_or_else(|_| Json::String(v.into()));
                    browse = Some(b.eq(column, v));
                }
                _ => return Err(USAGE.into()),
            },
        }
    }
    let browse = browse.ok_or(USAGE)?;
    let store = open(Path::new(&dir))?;
    let (_, table) = store.schema_arc().table(&browse.table).ok_or(format!("no table {}", browse.table))?;
    let mut out = String::new();
    for b in browse.rows(&store).map_err(|e| e.to_string())? {
        out += &row(table, &b).map_err(|e| e.to_string())?.to_string();
        out.push('\n');
    }
    Ok(out)
}

/// The store a data directory's log keeps, opened in memory.
fn open(dir: &Path) -> Result<ark::store::Store, String> {
    let log = dir.join("log");
    let read = |name: &str| match std::fs::read(log.join(name)) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", log.join(name).display())),
    };
    let schema = read(SCHEMA)?.ok_or(format!("{}: no {SCHEMA}: a server keeps its schema there as it starts", log.display()))?;
    let schema: Schema = serde_json::from_slice(&schema).map_err(|e| format!("{SCHEMA}: {e}"))?;
    // The rows whole and the entries after them, as one: the rows read
    // again after the entries, and the two again if a fold rewrote them
    // meanwhile.
    let (kv, logged) = loop {
        let (kv, logged) = (read("kv")?, read("log")?);
        if read("kv")? == kv {
            break (kv, logged);
        }
    };
    let mut disk = Memory::default();
    disk.0.extend(kv.map(|b| ("kv".to_string(), b)));
    disk.0.extend(logged.map(|b| ("log".to_string(), b)));
    let i = Instance::open_under(Arc::new(schema), Box::new(disk));
    if let Some(why) = &i.failure {
        return Err(format!("{}: {why}", log.display()));
    }
    Ok(i.store.clone())
}
