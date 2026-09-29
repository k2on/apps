//! Where a peer's replica is kept between runs.
//!
//! What is durable about the log is the confirmed store at its cursor and
//! the intents still pending — exactly what `Replica::open` takes, and
//! nothing optimistic. It is one canonical-CBOR record, the Swift client's
//! `ReplicaFile` shape without the `scope` spec v3 removed:
//!
//! ```text
//! { t: "replica", mode: "server" | "alone", cursor,
//!   confirmed: { table: [row…] }, pending: [entry…],
//!   user, session }
//! ```
//!
//! `user` and `session` are the login this peer last authored as — both
//! empty while nobody has signed in on it — so a peer reopened signed out
//! goes on authoring as whoever it was. A file written before they existed
//! reads as nobody's.
//!
//! Three places to keep it: a directory natively ([`Dir`], written to a
//! temporary name and renamed), the browser's `localStorage` in wasm
//! ([`Local`], base64 under a key), and memory ([`Memory`], for tests and
//! the demo — cloneable, so a test can "reopen" from what a peer left
//! behind).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ark::canon;
use ark::log::{Entry, Seq};
use ark::protocol::{entry_from_value, entry_value};
use ark::schema::Schema;
use ark::store::{Change, MemoryStore, Store};
use ark::value::Value;

use crate::Error;

/// A key-value place for a peer's files. `key` is a short file name such as
/// `replica`.
pub trait Storage {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, Error>;
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), Error>;
    fn remove(&mut self, key: &str) -> Result<(), Error>;
}

/// A storage a peer can own: `Send` natively, so a peer can move to a
/// thread; the browser's has no threads to move to.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxStorage = Box<dyn Storage + Send>;
#[cfg(target_arch = "wasm32")]
pub type BoxStorage = Box<dyn Storage>;

/// In memory. Clones share the same map.
#[derive(Clone, Debug, Default)]
pub struct Memory(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);

impl Memory {
    pub fn new() -> Memory {
        Memory::default()
    }

    fn map(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every key held, for a test to look at.
    pub fn keys(&self) -> Vec<String> {
        self.map().keys().cloned().collect()
    }
}

impl Storage for Memory {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.map().get(key).cloned())
    }
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), Error> {
        self.map().insert(key.into(), bytes.to_vec());
        Ok(())
    }
    fn remove(&mut self, key: &str) -> Result<(), Error> {
        self.map().remove(key);
        Ok(())
    }
}

/// A directory, one file per key, each written whole to a temporary name
/// beside it and renamed over the old one: a crash leaves either.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct Dir(pub std::path::PathBuf);

#[cfg(not(target_arch = "wasm32"))]
impl Storage for Dir {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = self.0.join(key);
        match std::fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Storage(format!("{}: {e}", path.display()))),
        }
    }
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), Error> {
        let io = |e: std::io::Error, p: &std::path::Path| Error::Storage(format!("{}: {e}", p.display()));
        std::fs::create_dir_all(&self.0).map_err(|e| io(e, &self.0))?;
        let path = self.0.join(key);
        let tmp = self.0.join(format!(".{key}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| io(e, &tmp))?;
        std::fs::rename(&tmp, &path).map_err(|e| io(e, &path))
    }
    fn remove(&mut self, key: &str) -> Result<(), Error> {
        let path = self.0.join(key);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Storage(format!("{}: {e}", path.display()))),
            _ => Ok(()),
        }
    }
}

/// The browser's `localStorage`: base64 under `"{prefix}{key}"`.
///
/// Synchronous, which is why it is this and not IndexedDB: a peer opens
/// inside iced's `boot`, which cannot wait for a promise. The cost is the
/// origin's quota — about five megabytes of UTF-16 in every browser, so
/// about 3.7 MB of replica — which a library of a few thousand tracks fits
/// and one of fifty thousand does not. An app that outgrows it implements
/// [`Storage`] over IndexedDB, loading every key before `open`.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug)]
pub struct Local {
    pub prefix: String,
}

#[cfg(target_arch = "wasm32")]
impl Local {
    fn storage() -> Result<web_sys::Storage, Error> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or_else(|| Error::Storage("this page has no localStorage".into()))
    }
}

#[cfg(target_arch = "wasm32")]
impl Storage for Local {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        let item = Self::storage()?
            .get_item(&format!("{}{key}", self.prefix))
            .map_err(|e| Error::Storage(format!("{e:?}")))?;
        match item {
            None => Ok(None),
            Some(s) => base64_decode(&s).map(Some).ok_or_else(|| Error::Corrupt(format!("{key} is not base64"))),
        }
    }
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), Error> {
        Self::storage()?
            .set_item(&format!("{}{key}", self.prefix), &base64_encode(bytes))
            .map_err(|e| Error::Storage(format!("writing {key}: {e:?} (over the origin's quota?)")))
    }
    fn remove(&mut self, key: &str) -> Result<(), Error> {
        Self::storage()?
            .remove_item(&format!("{}{key}", self.prefix))
            .map_err(|e| Error::Storage(format!("{e:?}")))
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The other way; `None` for anything that is not standard base64.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.as_bytes().chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= (B64.iter().position(|b| b == c)? as u32) << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// What is durable about the log, in two records: `replica` — the mode,
/// the cursor, the confirmed store, the login — written when the cursor or
/// the login moves, and `pending` — the intents not yet answered — written
/// when they do. Authoring an intent costs writing the intents, not the
/// store; the store is written once per batch that lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplicaFile {
    /// `"server"` for a replica of an authority elsewhere, `"alone"` for one
    /// that is its own authority: the sequences mean different things.
    pub mode: String,
    pub cursor: Seq,
    pub confirmed: MemoryStore,
    pub pending: Vec<Entry>,
    /// The login last authored as; empty for nobody (`Ctx::nobody`).
    pub user: String,
    pub session: String,
}

impl ReplicaFile {
    /// The key the confirmed store is kept under.
    pub const KEY: &'static str = "replica";
    /// The key the pending intents are kept under.
    pub const PENDING: &'static str = "pending";

    /// The `replica` record: everything but the pending intents.
    pub fn encode(&self) -> Vec<u8> {
        encode_replica(&self.mode, self.cursor, &self.confirmed, &self.user, &self.session)
    }

    /// The `pending` record.
    pub fn encode_pending(&self) -> Vec<u8> {
        encode_pending(&self.pending)
    }

    /// Both records out of a storage, or `None` where there is no replica.
    /// A replica written before the intents had a record of their own
    /// carries them inside; the record wins where both are there.
    pub fn load(storage: &dyn Storage, schema: &Schema) -> Result<Option<ReplicaFile>, Error> {
        let Some(bytes) = storage.load(ReplicaFile::KEY)? else {
            return Ok(None);
        };
        let mut f = ReplicaFile::decode(&bytes, schema)?;
        if let Some(bytes) = storage.load(ReplicaFile::PENDING)? {
            f.pending = decode_pending(&bytes)?;
        }
        Ok(Some(f))
    }

    pub fn decode(bytes: &[u8], schema: &Schema) -> Result<ReplicaFile, Error> {
        let bad = |w: &str| Error::Corrupt(format!("a replica file: {w}"));
        let v = canon::decode(bytes).map_err(|e| bad(&e.to_string()))?;
        let Value::Struct(m) = &v else { return Err(bad("not a struct")) };
        if m.get("t") != Some(&Value::text("replica")) {
            return Err(bad("not a replica"));
        }
        let text = |k: &str| match m.get(k) {
            Some(Value::Text(t)) => Ok(t.clone()),
            _ => Err(bad(&format!("no {k}"))),
        };
        let cursor = match m.get("cursor") {
            Some(Value::Int(n)) => *n,
            _ => return Err(bad("no cursor")),
        };
        let Some(Value::Struct(tables)) = m.get("confirmed") else {
            return Err(bad("no confirmed store"));
        };
        let mut confirmed = MemoryStore::empty(schema.clone());
        for (t, rows) in tables {
            let Value::List(rs) = rows else {
                return Err(bad(&format!("rows of {t}")));
            };
            for r in rs {
                let Value::Struct(row) = r else {
                    return Err(bad(&format!("a row of {t}")));
                };
                confirmed.apply_change(&Change::Add(t.clone(), row.clone()));
            }
        }
        let pending = match m.get("pending") {
            None => vec![],
            Some(Value::List(ps)) => ps
                .iter()
                .map(entry_from_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| bad(&e.to_string()))?,
            Some(_) => return Err(bad("pending is not a list")),
        };
        let optional = |k: &str| match m.get(k) {
            None => Ok(String::new()),
            Some(Value::Text(t)) => Ok(t.clone()),
            Some(_) => Err(bad(&format!("{k} is not text"))),
        };
        Ok(ReplicaFile {
            mode: text("mode")?,
            cursor,
            confirmed,
            pending,
            user: optional("user")?,
            session: optional("session")?,
        })
    }
}

/// The `replica` record's bytes.
pub fn encode_replica(mode: &str, cursor: Seq, confirmed: &MemoryStore, user: &str, session: &str) -> Vec<u8> {
    canon::encode(&Value::record(vec![
        ("t", Value::text("replica")),
        ("mode", Value::text(mode)),
        ("cursor", Value::Int(cursor)),
        ("confirmed", confirmed.store_value()),
        ("user", Value::text(user)),
        ("session", Value::text(session)),
    ]))
}

/// The `pending` record's bytes.
pub fn encode_pending(pending: &[Entry]) -> Vec<u8> {
    canon::encode(&Value::record(vec![
        ("t", Value::text("pending")),
        ("entries", Value::List(pending.iter().map(entry_value).collect())),
    ]))
}

/// The intents a `pending` record holds.
pub fn decode_pending(bytes: &[u8]) -> Result<Vec<Entry>, Error> {
    let bad = |w: &str| Error::Corrupt(format!("a pending file: {w}"));
    let v = canon::decode(bytes).map_err(|e| bad(&e.to_string()))?;
    let Value::Struct(m) = &v else { return Err(bad("not a struct")) };
    if m.get("t") != Some(&Value::text("pending")) {
        return Err(bad("not pending intents"));
    }
    let Some(Value::List(ps)) = m.get("entries") else {
        return Err(bad("no entries"));
    };
    ps.iter()
        .map(entry_from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| bad(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for len in 0..40 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let enc = base64_encode(&bytes);
            assert_eq!(enc.len() % 4, 0);
            assert_eq!(base64_decode(&enc).unwrap(), bytes, "{len}");
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert!(base64_decode("*").is_none());
    }
}
