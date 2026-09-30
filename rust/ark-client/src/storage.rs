//! Where a peer's replica is kept between runs.
//!
//! What is durable about the log is the confirmed store at its cursor and
//! the intents still pending — exactly what `Replica::open` takes, and
//! nothing optimistic. It is kept as canonical-CBOR records, each written
//! whole or not at all ([`Storage::save`] is atomic per record):
//!
//! ```text
//! replica   { t: "replica", mode: "server" | "alone", cursor,
//!             confirmed: { table: [row…] }, user, session }    the snapshot
//! facts.1   { t: "facts", from, to, facts: [[change…], …] }    the journal:
//! facts.2   …                                                  one page per
//!                                                              write, dense
//! pending   { t: "pending", gen, entries: [entry…] }       the intents'
//! pending.1 { t: "pending-ops", gen, add: [entry…],          snapshot, and
//!             drop: [id…] }                                  their pages
//! pending.2 …
//! who       { t: "who", user, session }
//! ```
//!
//! **A snapshot and a journal**, because a peer alone moves its cursor on
//! every mutation and writing the whole store each time made every tap cost
//! the library (`docs/plan-v4.md`, "Landed"). The snapshot is the confirmed
//! store at some cursor, in the Swift client's `ReplicaFile` shape without
//! the `scope` spec v3 removed. Each page after it holds the changes the
//! confirmed store moved by for a contiguous run of sequences — `from` to
//! `to`, one list of changes per sequence, in the form a `FactsFor` frame
//! carries them (§12) — starting where the previous page, or the snapshot,
//! ended. What a write costs is the changes since the last one, and not
//! the store.
//!
//! **Compaction** writes a fresh snapshot at the current cursor and removes
//! the pages, once their total encoded size exceeds the snapshot's. That is
//! the rule that keeps a mutation's cost independent of the store: a
//! snapshot of *S* bytes is written only after at least *S* bytes of pages,
//! so every byte of journal pays for at most one byte of snapshot and the
//! bytes written stay within twice the journal's, whatever the library's
//! size — where compacting every *n* pages would have every *n*th write pay
//! for the whole store, and the average grow with it. What it costs is room:
//! a storage holds up to about twice a snapshot, and a directory up to as
//! many small files as it takes to reach one.
//!
//! **What a stop leaves behind is safe, by the order of the writes.** A
//! page is written after the intents it confirmed have left `pending`, so a
//! stop between leaves an intent to be sent again rather than one applied
//! twice. A compaction writes the snapshot before it removes a page, and
//! removes them newest first. So `open` reads the snapshot, then `facts.1`,
//! `facts.2`, … until one is absent (a [`Storage`] cannot list its keys):
//! a page wholly at or below the snapshot's cursor is one a compaction
//! already holds, and is skipped; the first page that does not start at the
//! next sequence — or does not decode, being a write the platform tore — is
//! where the journal ends, and it and everything after it are dropped,
//! never applied out of order. Anything skipped or dropped is cleaned up by
//! a compaction as the peer opens. A torn tail costs the sequences in it,
//! which a server sends again and a peer alone had not yet written down.
//!
//! **The pending intents are a snapshot and pages too**
//! (`docs/plan-perf.md` §R3), for the same reason: rewriting every intent on
//! every `mutate` made a tap cost the backlog — 21 ms each at eight thousand
//! pending, which is where the scanner is after authoring a directory
//! offline. `pending` is the snapshot; each page after it says what moved
//! since, as the ids that left (`drop`: acknowledged, or refused) and the
//! intents that joined (`add`), and the list it stands for is the one
//! before it without the dropped, then the added, in order. `mutate` writes
//! one page holding its one intent before it returns, alone — the
//! durability of a local write (`spec/README.md`) is that page — and a pump
//! whose answers moved the list writes one page of what they moved.
//! Compaction writes a fresh snapshot and removes the pages once they
//! outgrow it, or once nothing is pending (then the snapshot is a few bytes
//! and is written in place of the page). It happens on a pump, never inside
//! `mutate`, so what a tap writes is its page and nothing else.
//!
//! **`gen` is what makes a stop inside a compaction safe.** Pages are not
//! idempotent — replaying an old `add` after a snapshot that no longer
//! holds the intent would resurrect one the server has already answered —
//! so each snapshot carries a generation, one more than the last, and each
//! page the generation of the snapshot it extends. A compaction writes the
//! snapshot and then removes the pages, newest first; what a stop between
//! leaves is pages of an older generation, which `open` skips. A page of
//! the right generation that does not decode — a write the platform tore —
//! or adds an intent already held ends the pages: it and everything after
//! it are dropped, never applied out of order, and a compaction as the peer
//! opens cleans up whatever was skipped or dropped. A torn page costs the
//! one intent it held: `mutate` had not returned. A `pending` record
//! written before pages existed has no `gen`, reads as generation 0, and
//! opens unchanged.
//!
//! **The login is a record of its own** (`who`): the one this peer last
//! authored as, both empty while nobody has signed in on it, so a peer
//! reopened signed out goes on authoring as whoever it was. It moves on a
//! sign-in, which is no reason to write the store. The snapshot carries it
//! too, as it always has; `who` wins where both are.
//!
//! A storage written before the journal existed — a `replica` record, no
//! pages, no `who` — is a snapshot with nothing after it, and opens as it
//! did. A file written before `user` and `session` existed reads as
//! nobody's; one written before the intents had a record of their own
//! carries them inside, and the record wins where both are.
//!
//! Three places to keep it: a directory natively ([`Dir`], written to a
//! temporary name, synced, and renamed), the browser's `localStorage` in
//! wasm ([`Local`], base64 under a key), and memory ([`Memory`], for tests
//! and the demo — cloneable, so a test can "reopen" from what a peer left
//! behind).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use ark::canon;
use ark::log::{Entry, Facts, Seq};
use ark::protocol::{change_from_value, change_value, entry_from_value, entry_value};
use ark::schema::Schema;
use ark::store::{Change, MemoryStore, Store};
use ark::value::{Id, Value};

use crate::Error;

/// A key-value place for a peer's files. `key` is a short file name such as
/// `replica`.
///
/// A `save` replaces its record whole or not at all: what the layout's
/// crash rules rest on (module docs). There is no listing; the journal's
/// pages are found by trying `facts.1`, `facts.2`, … in turn.
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
/// beside it, synced, and renamed over the old one, and the directory
/// synced after: a crash — of the program or of the machine — leaves the
/// old record or the new one, never part of either, and a record `save`
/// returned from is there after a restart (the spec's durability of a
/// local write, `spec/README.md`).
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
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp).map_err(|e| io(e, &tmp))?;
            f.write_all(bytes).map_err(|e| io(e, &tmp))?;
            // Without it a machine that stops after the rename can come back
            // with the new name over an empty or partial file.
            f.sync_all().map_err(|e| io(e, &tmp))?;
        }
        std::fs::rename(&tmp, &path).map_err(|e| io(e, &path))?;
        // …and without this, with the old record, or none.
        sync_dir(&self.0).map_err(|e| io(e, &self.0))
    }
    fn remove(&mut self, key: &str) -> Result<(), Error> {
        let path = self.0.join(key);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Storage(format!("{}: {e}", path.display()))),
            _ => Ok(()),
        }
    }
}

// A directory's entries are made durable by syncing the directory itself,
// where the platform lets one be opened to do it.
#[cfg(all(not(target_arch = "wasm32"), unix))]
fn sync_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}
#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn sync_dir(_: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

/// The browser's `localStorage`: base64 under `"{prefix}{key}"`. One
/// `setItem` replaces one value whole, which is the atomicity a record
/// needs.
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

/// What is durable about the log, as `open` reads it back: the snapshot
/// with every page after it applied — the mode, the cursor, the confirmed
/// store, the login — and the intents not yet answered. See the module
/// docs for the records it is kept in.
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
    /// The key the snapshot is kept under.
    pub const KEY: &'static str = "replica";
    /// The key the pending intents are kept under.
    pub const PENDING: &'static str = "pending";
    /// The key the login is kept under.
    pub const WHO: &'static str = "who";

    /// The key of the journal's `n`th page, counting from 1.
    pub fn page_key(n: usize) -> String {
        format!("facts.{n}")
    }

    /// The key of the pending intents' `n`th page, counting from 1.
    pub fn pending_page_key(n: usize) -> String {
        format!("pending.{n}")
    }

    /// The `replica` record: a snapshot of everything but the pending
    /// intents.
    pub fn encode(&self) -> Vec<u8> {
        encode_replica(&self.mode, self.cursor, &self.confirmed, &self.user, &self.session)
    }

    /// The `pending` record.
    pub fn encode_pending(&self) -> Vec<u8> {
        encode_pending(&self.pending)
    }

    /// Everything durable out of a storage, or `None` where there is no
    /// replica: [`Stored::load`], without what it says about the pages.
    pub fn load(storage: &dyn Storage, schema: &Schema) -> Result<Option<ReplicaFile>, Error> {
        Ok(Stored::load(storage, schema)?.map(|s| s.file))
    }

    /// The `replica` record alone: a snapshot, with the intents it carried
    /// if it was written before they had a record of their own.
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

/// What `open` found in a storage: the replica, and what the journal
/// looked like — which decides whether the next write is a page or a
/// snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    /// The snapshot with every good page applied, the intents and the
    /// login.
    pub file: ReplicaFile,
    /// The snapshot's cursor and encoded size.
    pub snapshot_cursor: Seq,
    pub snapshot_bytes: usize,
    /// The pages applied, and their total encoded size.
    pub pages: usize,
    pub page_bytes: usize,
    /// The highest `n` with a `facts.<n>` record, good or not.
    pub found: usize,
    /// Every page found was applied, in place: the journal can be written
    /// on from here. `false` when a page was skipped as one the snapshot
    /// already holds, or dropped as a torn or out-of-order tail — which a
    /// compaction then cleans up.
    pub clean: bool,
    /// What the pending intents' records looked like.
    pub pending: PendingStored,
}

impl Stored {
    /// The snapshot, the journal after it — skipping pages at or below the
    /// snapshot's cursor, stopping at the first that does not decode or does
    /// not start at the next sequence — the login and the intents; `None`
    /// where there is no snapshot. Reads, never writes.
    pub fn load(storage: &dyn Storage, schema: &Schema) -> Result<Option<Stored>, Error> {
        let Some(bytes) = storage.load(ReplicaFile::KEY)? else {
            return Ok(None);
        };
        let snapshot_bytes = bytes.len();
        let mut file = ReplicaFile::decode(&bytes, schema)?;
        let snapshot_cursor = file.cursor;
        let (mut pages, mut page_bytes, mut found, mut clean, mut torn) = (0, 0, 0, true, false);
        while let Some(bytes) = storage.load(&ReplicaFile::page_key(found + 1))? {
            found += 1;
            if torn {
                continue;
            }
            match decode_page(&bytes) {
                Ok(p) if p.to <= snapshot_cursor => clean = false,
                Ok(p) if p.from == file.cursor + 1 => {
                    for f in &p.facts {
                        file.confirmed.apply_changes(f);
                    }
                    file.cursor = p.to;
                    pages += 1;
                    page_bytes += bytes.len();
                }
                _ => {
                    torn = true;
                    clean = false;
                }
            }
        }
        if let Some(bytes) = storage.load(ReplicaFile::WHO)? {
            (file.user, file.session) = decode_who(&bytes)?;
        }
        let (entries, pending) = load_pending(storage)?;
        if let Some(entries) = entries {
            file.pending = entries;
        }
        Ok(Some(Stored {
            file,
            snapshot_cursor,
            snapshot_bytes,
            pages,
            page_bytes,
            found,
            clean,
            pending,
        }))
    }
}

/// How many `facts.<n>` records a storage holds, counting from 1 to the
/// first absent: what a storage with no snapshot has to clear before its
/// first one, since a page found then would be read against it.
pub fn count_pages(storage: &dyn Storage) -> Result<usize, Error> {
    let mut n = 0;
    while storage.load(&ReplicaFile::page_key(n + 1))?.is_some() {
        n += 1;
    }
    Ok(n)
}

/// One page of the journal: the changes the confirmed store moved by at
/// each sequence from `from` to `to`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalPage {
    pub from: Seq,
    pub to: Seq,
    /// One list per sequence, `from` first.
    pub facts: Vec<Facts>,
}

/// A page's bytes, for a contiguous run of sequences, oldest first (what
/// `Replica::take_confirmed` hands over). Empty is not a page.
pub fn encode_page(run: &[(Seq, Facts)]) -> Vec<u8> {
    let from = run.first().map_or(0, |(n, _)| *n);
    let to = run.last().map_or(0, |(n, _)| *n);
    debug_assert!(
        run.iter().enumerate().all(|(i, (n, _))| *n == from + i as Seq),
        "a page is a contiguous run of sequences"
    );
    canon::encode(&Value::record(vec![
        ("t", Value::text("facts")),
        ("from", Value::Int(from)),
        ("to", Value::Int(to)),
        (
            "facts",
            Value::List(run.iter().map(|(_, f)| Value::List(f.iter().map(change_value).collect())).collect()),
        ),
    ]))
}

/// A page back, whole: a run that does not add up — not one list of
/// changes per sequence from `from` to `to` — is as corrupt as bytes that
/// do not decode.
pub fn decode_page(bytes: &[u8]) -> Result<JournalPage, Error> {
    let bad = |w: &str| Error::Corrupt(format!("a journal page: {w}"));
    let v = canon::decode(bytes).map_err(|e| bad(&e.to_string()))?;
    let Value::Struct(m) = &v else { return Err(bad("not a struct")) };
    if m.get("t") != Some(&Value::text("facts")) {
        return Err(bad("not a page"));
    }
    let int = |k: &str| match m.get(k) {
        Some(Value::Int(n)) => Ok(*n),
        _ => Err(bad(&format!("no {k}"))),
    };
    let (from, to) = (int("from")?, int("to")?);
    let Some(Value::List(runs)) = m.get("facts") else {
        return Err(bad("no facts"));
    };
    let facts = runs
        .iter()
        .map(|f| match f {
            Value::List(cs) => cs
                .iter()
                .map(change_from_value)
                .collect::<Result<Facts, _>>()
                .map_err(|e| bad(&e.to_string())),
            _ => Err(bad("facts are not lists")),
        })
        .collect::<Result<Vec<Facts>, Error>>()?;
    if from < 1 || to < from || facts.len() as i64 != to - from + 1 {
        return Err(bad(&format!("{} lists of facts for {from}..{to}", facts.len())));
    }
    Ok(JournalPage { from, to, facts })
}

/// The `who` record's bytes.
pub fn encode_who(user: &str, session: &str) -> Vec<u8> {
    canon::encode(&Value::record(vec![
        ("t", Value::text("who")),
        ("user", Value::text(user)),
        ("session", Value::text(session)),
    ]))
}

/// The login a `who` record holds: user, session.
pub fn decode_who(bytes: &[u8]) -> Result<(String, String), Error> {
    let bad = |w: &str| Error::Corrupt(format!("a who file: {w}"));
    let v = canon::decode(bytes).map_err(|e| bad(&e.to_string()))?;
    let Value::Struct(m) = &v else { return Err(bad("not a struct")) };
    if m.get("t") != Some(&Value::text("who")) {
        return Err(bad("not a login"));
    }
    let text = |k: &str| match m.get(k) {
        Some(Value::Text(t)) => Ok(t.clone()),
        _ => Err(bad(&format!("no {k}"))),
    };
    Ok((text("user")?, text("session")?))
}

/// The `replica` record's bytes: a snapshot.
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

/// The `pending` record's bytes, as a storage written before pages had
/// it: generation 0.
pub fn encode_pending(pending: &[Entry]) -> Vec<u8> {
    canon::encode(&Value::record(vec![
        ("t", Value::text("pending")),
        ("entries", Value::List(pending.iter().map(entry_value).collect())),
    ]))
}

/// The `pending` record's bytes as a snapshot of generation `gen`: the
/// intents, and which pages extend it.
pub fn encode_pending_snapshot(gen: i64, pending: &[Entry]) -> Vec<u8> {
    canon::encode(&Value::record(vec![
        ("t", Value::text("pending")),
        ("gen", Value::Int(gen)),
        ("entries", Value::List(pending.iter().map(entry_value).collect())),
    ]))
}

/// The intents a `pending` record holds.
pub fn decode_pending(bytes: &[u8]) -> Result<Vec<Entry>, Error> {
    decode_pending_snapshot(bytes).map(|(_, entries)| entries)
}

/// A `pending` record: its generation (0 where it has none) and its
/// intents.
pub fn decode_pending_snapshot(bytes: &[u8]) -> Result<(i64, Vec<Entry>), Error> {
    let bad = |w: &str| Error::Corrupt(format!("a pending file: {w}"));
    let v = canon::decode(bytes).map_err(|e| bad(&e.to_string()))?;
    let Value::Struct(m) = &v else { return Err(bad("not a struct")) };
    if m.get("t") != Some(&Value::text("pending")) {
        return Err(bad("not pending intents"));
    }
    let gen = match m.get("gen") {
        None => 0,
        Some(Value::Int(n)) => *n,
        Some(_) => return Err(bad("gen is not an int")),
    };
    let Some(Value::List(ps)) = m.get("entries") else {
        return Err(bad("no entries"));
    };
    let entries = ps
        .iter()
        .map(entry_from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| bad(&e.to_string()))?;
    Ok((gen, entries))
}

/// One page of the pending intents: what moved since the page before, or
/// since the snapshot of generation `gen`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPage {
    pub gen: i64,
    /// Intents that joined, in the order they were authored.
    pub add: Vec<Entry>,
    /// Ids that left: acknowledged, or refused.
    pub drop: Vec<Id>,
}

/// A pending page's bytes.
pub fn encode_pending_page(gen: i64, add: &[&Entry], drop: &[Id]) -> Vec<u8> {
    canon::encode(&Value::record(vec![
        ("t", Value::text("pending-ops")),
        ("gen", Value::Int(gen)),
        ("add", Value::List(add.iter().map(|e| entry_value(e)).collect())),
        ("drop", Value::List(drop.iter().map(|i| Value::Id(*i)).collect())),
    ]))
}

/// A pending page back, whole.
pub fn decode_pending_page(bytes: &[u8]) -> Result<PendingPage, Error> {
    let bad = |w: &str| Error::Corrupt(format!("a pending page: {w}"));
    let v = canon::decode(bytes).map_err(|e| bad(&e.to_string()))?;
    let Value::Struct(m) = &v else { return Err(bad("not a struct")) };
    if m.get("t") != Some(&Value::text("pending-ops")) {
        return Err(bad("not a pending page"));
    }
    let Some(Value::Int(gen)) = m.get("gen") else {
        return Err(bad("no gen"));
    };
    let (Some(Value::List(add)), Some(Value::List(drop))) = (m.get("add"), m.get("drop")) else {
        return Err(bad("no add or drop"));
    };
    let add = add
        .iter()
        .map(entry_from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| bad(&e.to_string()))?;
    let drop = drop
        .iter()
        .map(|v| match v {
            Value::Id(i) => Ok(*i),
            _ => Err(bad("a drop is not an id")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PendingPage { gen: *gen, add, drop })
}

/// What the pending intents' records looked like when they were read:
/// which decides whether the next write is a page or a snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingStored {
    /// The snapshot's generation and encoded size; 0 and 0 where there is
    /// no `pending` record.
    pub gen: i64,
    pub snapshot_bytes: usize,
    /// The pages applied, and their total encoded size.
    pub pages: usize,
    pub page_bytes: usize,
    /// The highest `n` with a `pending.<n>` record, good or not.
    pub found: usize,
    /// There is a snapshot and every page found was applied: pages can be
    /// written on from here. `false` otherwise — which a compaction then
    /// cleans up.
    pub clean: bool,
}

/// The pending intents: the snapshot, then its pages in order — skipping
/// pages of an older generation, stopping at the first that does not decode
/// or adds an intent already held (the module docs). `None` for the intents
/// where there is no `pending` record. Reads, never writes.
pub fn load_pending(storage: &dyn Storage) -> Result<(Option<Vec<Entry>>, PendingStored), Error> {
    let mut st = PendingStored::default();
    let (gen, mut entries) = match storage.load(ReplicaFile::PENDING)? {
        Some(bytes) => {
            st.snapshot_bytes = bytes.len();
            let (gen, entries) = decode_pending_snapshot(&bytes)?;
            (gen, Some(entries))
        }
        None => (0, None),
    };
    st.gen = gen;
    st.clean = entries.is_some();
    let mut torn = entries.is_none();
    let mut held: BTreeSet<Id> = entries.iter().flatten().map(|e| e.id).collect();
    while let Some(bytes) = storage.load(&ReplicaFile::pending_page_key(st.found + 1))? {
        st.found += 1;
        if torn {
            continue;
        }
        let list = entries.as_mut().expect("not torn, so a snapshot");
        match decode_pending_page(&bytes) {
            Ok(p) if p.gen < gen => st.clean = false,
            Ok(p) if p.gen == gen && p.add.iter().all(|a| !held.contains(&a.id) || p.drop.contains(&a.id)) => {
                let gone: BTreeSet<Id> = p.drop.iter().copied().collect();
                if !gone.is_empty() {
                    list.retain(|e| !gone.contains(&e.id));
                    held.retain(|i| !gone.contains(i));
                }
                held.extend(p.add.iter().map(|e| e.id));
                list.extend(p.add);
                st.pages += 1;
                st.page_bytes += bytes.len();
            }
            _ => {
                torn = true;
                st.clean = false;
            }
        }
    }
    Ok((entries, st))
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
