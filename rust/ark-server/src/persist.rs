//! The log on disk: `DATA/log.ark-log`, canonical CBOR of
//! [`log_to_value`], rewritten whole after every batch of appends.
//!
//! The shape reuses the protocol's own encoders for an entry and a change,
//! so a log file carries exactly what a `Batch` frame would:
//!
//! ```text
//! { t: "log",
//!   base: { seq: Int, hash: Bytes, rows: { table: [row…] } },
//!   entries: [ { seq: Int, entry: Entry, facts: [Change…] } … ],
//!   ids: [ { id: Id, seq: Int } … ] }
//! ```
//!
//! The base's hash is recomputed on load and held to what was written, and
//! the entries must run without a gap, so a file that has been damaged is
//! refused rather than served.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ark::canon;
use ark::log::{snapshot_of, Log};
use ark::protocol::{change_from_value, change_value, entry_from_value, entry_value};
use ark::schema::Schema;
use ark::store::{MemoryStore, Store};
use ark::value::{FieldName, Value};

/// The file the log is kept in.
pub fn path_of(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Its name.
pub const FILE: &str = "log.ark-log";

/// A log as a value; its file form is `canon::encode` of this.
pub fn log_to_value(log: &Log) -> Value {
    let store = &log.base.store;
    let rows: Vec<(String, Value)> = store
        .table_names()
        .into_iter()
        .map(|t| {
            let rs = store.scan(&t).into_iter().map(Value::Struct).collect();
            (t, Value::list(rs))
        })
        .collect();
    let entries: Vec<Value> = log
        .entries
        .iter()
        .map(|(n, (e, f))| {
            Value::record(vec![
                ("seq", Value::int(*n)),
                ("entry", entry_value(e)),
                ("facts", Value::list(f.iter().map(change_value).collect())),
            ])
        })
        .collect();
    let ids: Vec<Value> = log
        .ids
        .iter()
        .map(|(id, n)| Value::record(vec![("id", Value::Id(*id)), ("seq", Value::int(*n))]))
        .collect();
    Value::record(vec![
        ("t", Value::text("log")),
        (
            "base",
            Value::record(vec![
                ("seq", Value::int(log.base.seq)),
                ("hash", Value::bytes(log.base.hash.clone())),
                ("rows", Value::record(rows)),
            ]),
        ),
        ("entries", Value::list(entries)),
        ("ids", Value::list(ids)),
    ])
}

fn fields(v: &Value) -> Result<&BTreeMap<FieldName, Value>> {
    match v {
        Value::Struct(m) => Ok(m),
        other => bail!("expected a struct, found {other:?}"),
    }
}

fn need<'a>(m: &'a BTreeMap<FieldName, Value>, k: &str) -> Result<&'a Value> {
    m.get(k).with_context(|| format!("missing field {k}"))
}

fn int(v: &Value) -> Result<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        other => bail!("expected an int, found {other:?}"),
    }
}

fn items(v: &Value) -> Result<&[Value]> {
    match v {
        Value::List(xs) => Ok(xs),
        other => bail!("expected a list, found {other:?}"),
    }
}

/// A log from its value, over the module's schema.
pub fn log_from_value(schema: &Schema, v: &Value) -> Result<Log> {
    let m = fields(v)?;
    match need(m, "t")? {
        Value::Text(t) if t == "log" => {}
        other => bail!("not a log file: t = {other:?}"),
    }
    let base = fields(need(m, "base")?)?;
    let store = MemoryStore::from_value(schema.clone(), need(base, "rows")?);
    let snapshot = snapshot_of(int(need(base, "seq")?)?, store);
    match need(base, "hash")? {
        Value::Bytes(h) if *h == snapshot.hash => {}
        _ => bail!("the snapshot's hash does not match its rows"),
    }
    let mut log = Log {
        base: snapshot,
        entries: BTreeMap::new(),
        ids: BTreeMap::new(),
    };
    for item in items(need(m, "entries")?)? {
        let im = fields(item)?;
        let n = int(need(im, "seq")?)?;
        let e = entry_from_value(need(im, "entry")?).with_context(|| format!("entry {n}"))?;
        let facts = items(need(im, "facts")?)?
            .iter()
            .map(change_from_value)
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("facts of {n}"))?;
        log.entries.insert(n, (e, facts));
    }
    for item in items(need(m, "ids")?)? {
        let im = fields(item)?;
        let id = match need(im, "id")? {
            Value::Id(i) => *i,
            other => bail!("expected an id, found {other:?}"),
        };
        log.ids.insert(id, int(need(im, "seq")?)?);
    }
    if !log.contiguous() {
        bail!("the entries do not run without a gap from {} to {}", log.horizon() + 1, log.head_seq());
    }
    Ok(log)
}

/// Write the log: to a temporary file beside it, then into place.
pub fn save(dir: &Path, log: &Log) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let final_path = path_of(dir);
    let tmp = dir.join(format!(".{FILE}.tmp"));
    let bytes = canon::encode(&log_to_value(log));
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &final_path).with_context(|| format!("moving {} into place", tmp.display()))?;
    Ok(())
}

/// Read the log back, if there is one.
pub fn load(dir: &Path, schema: &Schema) -> Result<Option<Log>> {
    let path = path_of(dir);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let v = canon::decode(&bytes).with_context(|| format!("decoding {}", path.display()))?;
    let log = log_from_value(schema, &v).with_context(|| format!("reading {}", path.display()))?;
    Ok(Some(log))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark::eval::Ctx;
    use ark::peer::{Authority, Sequenced};
    use ark_client::{demo, Domain};

    fn author(a: &mut Authority, d: &Domain, id: [u8; 16], name: &str, ctx: &Ctx) {
        let (fh, _) = d.mutator("create_playlist").unwrap();
        let e = ark::log::Entry {
            id,
            actor: ctx.user.clone(),
            session: ctx.session.clone(),
            fn_hash: fh.clone(),
            args: [("name".to_string(), Value::text(name))].into(),
            autos: [("id".to_string(), Value::Id(id))].into(),
        };
        assert!(matches!(a.sequence_entry(&e), Sequenced::Appended(..)), "{name}");
    }

    #[test]
    fn a_log_survives_the_file_and_a_damaged_one_is_refused() {
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let mut a = Authority::new(schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let ctx = Ctx::new("alice", "dev");
        author(&mut a, &d, [1; 16], "Road trip", &ctx);
        author(&mut a, &d, [2; 16], "Focus", &ctx);
        // Compact once so the base carries rows, then append above it.
        assert!(a.compact(1));
        author(&mut a, &d, [3; 16], "Sleep", &ctx);
        assert_eq!(a.log.head_seq(), 3);

        let dir = tempfile::tempdir().unwrap();
        let empty = tempfile::tempdir().unwrap();
        assert!(load(empty.path(), &schema).unwrap().is_none());
        save(dir.path(), &a.log).unwrap();
        let back = load(dir.path(), &schema).unwrap().expect("a file was written");
        assert_eq!(back, a.log);

        // A row changed under the snapshot's hash is a file that is not served.
        let mut v = log_to_value(&a.log);
        if let Value::Struct(m) = &mut v {
            if let Some(Value::Struct(base)) = m.get_mut("base") {
                base.insert("rows".into(), Value::record::<String>(vec![]));
            }
        }
        let err = log_from_value(&schema, &v).unwrap_err();
        assert!(err.to_string().contains("hash"), "{err}");
    }
}
