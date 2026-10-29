//! A disk's rows and the schema they were written under (§1.2): every disk
//! keeps its schema beside its log ([`SCHEMA`]), so a peer opening it under
//! another knows which.
//!
//! Under a schema grown from it, rows are read as they are. Under one that
//! is not, an authority — a server, or a device alone since its fork —
//! migrates them ([`migrate`]): its log folded into its rows under the
//! schema it was written under, the rows read through the domain's
//! migrations ([`ark::migrate`]), and written whole as a snapshot one past
//! the head, so every replica, wherever it stands, is behind it and is sent
//! it whole; each origin's last `local` among the entries it folds is its
//! journal's, as a horizon moving keeps it. What it stood on before is kept beside it, named by that
//! schema's hash (`kv.<hash>`, `log.<hash>`). A replica's rows are its
//! authority's to send again: it starts empty instead ([`kept`]), its
//! pending intents and its origin kept. A disk with rows and no schema
//! beside them was written before disks kept one: an authority reads it as
//! its own schema's, as it always did, and a replica starts empty.

use std::io;
use std::sync::Arc;

use ark::format::Encode;
use ark::hash::EntryHash;
use ark::log::{Record, Snapshot};
use ark::migrate::Migration;
use ark::schema::{compat, Schema, Space};
use ark::store::Store;

use crate::disk::SCHEMA;
use crate::{Disk, Instance, Memory};

/// The schema `disk` says its rows were written under, if it says.
fn stored(disk: &mut dyn Disk) -> io::Result<Option<Schema>> {
    let Some(b) = disk.get(SCHEMA)? else {
        return Ok(None);
    };
    serde_json::from_slice(&b)
        .map(Some)
        .map_err(|e| io::Error::other(format!("{SCHEMA}: {e}")))
}

/// `schema` beside `disk`'s rows, as what they were written under: what
/// writes a disk's rows itself, rather than a peer, says it too.
pub fn put_schema(disk: &mut dyn Disk, schema: &Schema) -> io::Result<()> {
    disk.put(SCHEMA, &serde_json::to_vec_pretty(schema).map_err(io::Error::other)?)
}

/// Whether a replica's rows read under `schema` as they are: written under
/// it, or one it grows from. `schema` is the disk's from then on.
pub fn kept(disk: &mut dyn Disk, schema: &Schema) -> io::Result<bool> {
    let had = disk.get("kv")?.is_some() || disk.get("log")?.is_some_and(|l| !l.is_empty());
    let kept = match stored(disk)? {
        Some(was) => compat(&was, schema).is_ok(),
        None => !had,
    };
    put_schema(disk, schema)?;
    Ok(kept)
}

/// An authority's rows, as `to` reads them: migrated by `steps` when they
/// were written under a schema it does not grow from, and what they were
/// kept beside them. Whether they were; an error, the disk as it was, when
/// no step reads them.
pub fn migrate(disk: &mut dyn Disk, to: &Arc<Schema>, steps: &[Migration]) -> io::Result<bool> {
    let was = match stored(disk)? {
        Some(was) if compat(&was, to).is_err() => was,
        _ => {
            put_schema(disk, to)?;
            return Ok(false);
        }
    };
    let (kv, log) = (disk.get("kv")?, disk.get("log")?);
    let mut copy = Memory::default();
    copy.0.extend(kv.clone().map(|b| ("kv".to_string(), b)));
    copy.0.extend(log.clone().map(|b| ("log".to_string(), b)));
    let old = Instance::open_under(Arc::new(was.clone()), Box::new(copy));
    if let Some(why) = &old.failure {
        return Err(io::Error::other(format!(
            "the log does not read under the schema it was written under: {why}"
        )));
    }
    let (rows, (seq, hash)) = (old.store.snapshot_rows(), old.log().head());
    let rows = ark::migrate::migrate(&was, &rows, to, steps)
        .map_err(|e| io::Error::other(e.0))?
        .expect("a schema not grown from is migrated");
    let (store, _) = Store::from_rows(to.clone(), Space::Durable, &rows).map_err(|e| io::Error::other(e.0))?;
    let snapshot = Snapshot {
        seq: seq + 1,
        hash: EntryHash::of(&[&hash.0[..], b"migrated to", &to.hash().0].concat()),
        state: store.state_hash(),
        rows,
    };
    // The journal rebuilt each origin's last from the entries; the snapshot
    // holds none, so the journal keeps them, or every device's next intent
    // is past a gap.
    let last = old.log().entries().into_iter().map(|l| (l.entry.origin, l.entry.actor, l.entry.local));
    crate::local::passed(disk, last)?;
    let before = &was.hash().hex()[..12];
    for (name, bytes) in [("kv", kv), ("log", log)] {
        let name = format!("{name}.{before}");
        if let (Some(b), None) = (bytes, disk.get(&name)?) {
            disk.put(&name, &b)?;
        }
    }
    disk.record(&Record::Rows(snapshot.kv_rows()).to_bytes())?;
    put_schema(disk, to)?;
    Ok(true)
}
