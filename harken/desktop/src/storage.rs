//! What a replica needs to be reopened, per scope, as one canonical CBOR
//! file under the data directory: the confirmed store, the cursor, and the
//! pending intents — exactly what `Replica::open` takes, and nothing else.
//! Written whole, to a temporary name, then renamed, after every change;
//! the whole file is a few kilobytes at this scale.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ark::canon;
use ark::log::{Entry, Seq};
use ark::protocol::{entry_from_value, entry_value};
use ark::schema::Schema;
use ark::store::{MemoryStore, Store};
use ark::value::Value;

/// What `Replica::open` takes.
pub struct Durable {
    pub confirmed: MemoryStore,
    pub cursor: Seq,
    pub pending: Vec<Entry>,
}

impl Durable {
    pub fn empty(schema: Schema) -> Durable {
        Durable {
            confirmed: MemoryStore::empty(schema),
            cursor: 0,
            pending: vec![],
        }
    }
}

/// The file a scope is kept in.
pub fn path_of(data: &Path, scope: &str) -> PathBuf {
    data.join(format!("{scope}.cbor"))
}

fn to_value(d: &Durable) -> Value {
    let mut m: BTreeMap<String, Value> = BTreeMap::new();
    m.insert("confirmed".into(), d.confirmed.store_value());
    m.insert("cursor".into(), Value::Int(d.cursor));
    m.insert("pending".into(), Value::List(d.pending.iter().map(entry_value).collect()));
    Value::Struct(m)
}

fn from_value(schema: Schema, v: &Value) -> Result<Durable, String> {
    let Value::Struct(m) = v else {
        return Err("a scope file is a struct".into());
    };
    let confirmed = MemoryStore::from_value(schema, m.get("confirmed").ok_or("no confirmed store")?);
    let cursor = match m.get("cursor") {
        Some(Value::Int(n)) => *n,
        _ => return Err("no cursor".into()),
    };
    let pending = match m.get("pending") {
        Some(Value::List(es)) => es
            .iter()
            .map(entry_from_value)
            .collect::<Result<Vec<Entry>, _>>()
            .map_err(|e| e.to_string())?,
        _ => return Err("no pending".into()),
    };
    Ok(Durable { confirmed, cursor, pending })
}

/// Read a scope's file; an absent file is an empty scope at cursor 0.
pub fn load(data: &Path, schema: Schema, scope: &str) -> Result<Durable, String> {
    let path = path_of(data, scope);
    match fs::read(&path) {
        Ok(bytes) => {
            let v = canon::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            from_value(schema, &v).map_err(|e| format!("{}: {e}", path.display()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Durable::empty(schema)),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write a scope's file: temporary name, then rename over the old one.
pub fn save(data: &Path, scope: &str, d: &Durable) -> Result<(), String> {
    fs::create_dir_all(data).map_err(|e| format!("{}: {e}", data.display()))?;
    let path = path_of(data, scope);
    let tmp = data.join(format!(".{scope}.cbor.tmp"));
    let bytes = canon::encode(&to_value(d));
    fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}
