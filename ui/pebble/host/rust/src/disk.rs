//! The node's store on the watch's persistent storage: what a watch keeps
//! across a restart, paged into the 256-byte values PebbleOS stores.
//!
//! The watch has no file API, 1 MiB of storage per app and at most 256
//! bytes per value, and each write takes about 13 ms (measured in the
//! Time 2's emulator, `docs/north-star.md`, "Open Questions"). So the rows
//! live in memory ([`crate::Rows`]) and the disk holds a copy, written in
//! batches ([`Disk::flush`]) that rewrite only the values that changed.
//!
//! ```text
//! key 0        the superblock: magic, format, origin, made (a lease),
//!              cursor, actor, the directory's length and its keys
//! directory    page keys, pending keys, the pending records' length,
//!              in 256-byte chunks
//! pages        the rows, in path order ([space, table (4 bytes), key]),
//!              whole rows per page, each page a value
//! pending      the pending records' bytes, in 256-byte chunks
//! ```
//!
//! **Pages.** Each page holds the rows from its first path up to the next
//! page's, so a row written lands in one known page. A flush walks every
//! row, packs each page's rows (splitting a page that outgrows a value,
//! dropping an empty one, joining two small neighbours) and writes only the
//! pages whose bytes changed, compared by a hash kept from the last write.
//! A row is at most a value (its path and value 250 bytes or so); a larger
//! one is not kept, and counted (`Stats::too_big`).
//!
//! **Copy on write.** A changed page, chunk or directory is written to a
//! free key, never over the one in use, and the superblock, written last
//! and alone in place, commits the flush: a watch that dies mid-flush comes
//! back to the flush before. The keys let go are free for the next flush
//! (left as they are: deleting is a write too).
//!
//! **Entry ids.** The kernel's ids are never renumbered, so the count of
//! entries this origin made must never go back, even after a crash between
//! a push and a flush. The superblock keeps a lease past the count, and the
//! host flushes before the count reaches it; a restart takes the lease as
//! its count, leaving a gap the authority does not mind.

use alloc::vec::Vec;

use ark_kernel::codec::{put_bytes, put_varint, Reader};
use ark_kernel::Decode;

/// The largest value PebbleOS stores.
pub const VALUE: usize = 256;
/// The keys the store may use, from 0: 4000 values of 256 bytes, within
/// the 1 MiB an app gets, leaving the rest for the app's own.
pub const KEYS: u32 = 4000;
const SUPER: u32 = 0;
const MAGIC: &[u8; 4] = b"arkP";
const FORMAT: u8 = 1;
/// How far past the count of entries made the lease runs.
const LEASE: u64 = 32;
/// Two neighbouring pages are joined when both fit in this.
const JOIN: usize = VALUE / 2;

/// Values by number: the watch's persistent storage, or a test's map.
pub trait Persist {
    /// The value at `key` read into `buf`: its length, or `None` if absent.
    fn read(&mut self, key: u32, buf: &mut [u8; VALUE]) -> Option<usize>;
    /// Writes a value of at most [`VALUE`] bytes: whether it was written.
    fn write(&mut self, key: u32, data: &[u8]) -> bool;
}

/// What the host keeps of its node besides rows and pending records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub origin: u64,
    /// The entries this origin has made; on load, the lease.
    pub made: u64,
    pub cursor: u64,
    pub actor: u64,
}

/// What a start found on the disk.
pub struct Loaded {
    pub meta: Meta,
    /// The pending records' bytes, as the host flushed them.
    pub pending: Vec<u8>,
}

/// What the disk has done since it opened.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub flushes: u32,
    /// Values written, and their bytes.
    pub writes: u32,
    pub bytes: u32,
    /// Values the last flush wrote.
    pub last: u32,
    /// Rows too large for a page, not kept.
    pub too_big: u32,
    /// Flushes refused by the storage (full), kept in memory.
    pub failed: u32,
    /// Values in use: pages, chunks, directory and superblock.
    pub used: u32,
}

/// A page of rows: its first path, where it is, and its bytes' hash.
struct Page {
    first: Vec<u8>,
    key: u32,
    hash: u32,
}

/// Called with each row's path and value.
pub type Visit<'a> = dyn FnMut(&[u8], &[u8]) + 'a;
/// Walks every row to keep, in path order, calling the visitor with each.
pub type Walk<'a> = dyn Fn(&mut Visit<'_>) + 'a;

/// The storage refused a write: it is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// A chunk of a blob: where it is, and its bytes' hash.
type Chunk = (u32, u32);

pub struct Disk<P> {
    p: P,
    pages: Vec<Page>,
    pending: Vec<Chunk>,
    dir: Vec<Chunk>,
    /// The superblock as last written.
    sup: Vec<u8>,
    /// A bit per key in use; on the heap, as the watch's stack is small.
    used: Vec<u32>,
    lease: u64,
    pub stats: Stats,
}

/// FNV-1a over the bytes and their length.
fn hash(b: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &x in b.iter().chain(&(b.len() as u32).to_le_bytes()) {
        h = (h ^ u32::from(x)).wrapping_mul(0x0100_0193);
    }
    h
}

fn mark(used: &mut [u32], key: u32, on: bool) {
    let (w, b) = ((key / 32) as usize, key % 32);
    if on {
        used[w] |= 1 << b;
    } else {
        used[w] &= !(1 << b);
    }
}

/// Writes `data` at the lowest free key, noted in `fresh` so that a flush
/// that fails can free it again.
fn put<P: Persist>(p: &mut P, used: &mut [u32], fresh: &mut Vec<u32>, data: &[u8]) -> Result<u32, Full> {
    let key = (1..KEYS).find(|&k| used[(k / 32) as usize] & (1 << (k % 32)) == 0).ok_or(Full)?;
    mark(used, key, true);
    fresh.push(key);
    if p.write(key, data) {
        Ok(key)
    } else {
        Err(Full)
    }
}

/// A blob's chunks: each kept where it is if its bytes are the same as
/// `old`'s at its place, else written afresh.
fn chunks<P: Persist>(p: &mut P, used: &mut [u32], fresh: &mut Vec<u32>, data: &[u8], old: &[Chunk]) -> Result<Vec<Chunk>, Full> {
    let mut out = Vec::new();
    for (j, c) in data.chunks(VALUE).enumerate() {
        let h = hash(c);
        let key = match old.get(j) {
            Some(&(key, oh)) if oh == h => key,
            _ => put(p, used, fresh, c)?,
        };
        out.push((key, h));
    }
    Ok(out)
}

fn keys(chunks: &[Chunk]) -> Vec<u32> {
    chunks.iter().map(|c| c.0).collect()
}

impl<P: Persist> Disk<P> {
    /// Opens the disk, calling `rows` with each row's path and value, in
    /// path order, and answers what else it held; `None` for a disk with
    /// nothing on it, or one it cannot read (then whatever `rows` was given
    /// is to be dropped).
    pub fn open(p: P, rows: &mut Visit<'_>) -> (Disk<P>, Option<Loaded>) {
        let mut d = Disk {
            p,
            pages: Vec::new(),
            pending: Vec::new(),
            dir: Vec::new(),
            sup: Vec::new(),
            used: alloc::vec![0; KEYS.div_ceil(32) as usize],
            lease: 0,
            stats: Stats::default(),
        };
        let loaded = d.load(rows);
        if loaded.is_none() {
            d.pages.clear();
            d.pending.clear();
            d.dir.clear();
            d.sup.clear();
            d.used = alloc::vec![0; KEYS.div_ceil(32) as usize];
        }
        mark(&mut d.used, SUPER, true);
        d.count();
        (d, loaded)
    }

    /// The persistent storage, for what else the app keeps.
    pub fn persist(&mut self) -> &mut P {
        &mut self.p
    }

    /// The count of entries made the next flush must come before.
    pub fn lease(&self) -> u64 {
        self.lease
    }

    /// Reads a blob of `len` bytes from its chunks, noting each in use.
    fn blob(&mut self, keys: &[u32], len: usize) -> Option<(Vec<u8>, Vec<Chunk>)> {
        let mut buf = [0u8; VALUE];
        let mut out = Vec::with_capacity(len);
        let mut chunks = Vec::new();
        for &key in keys {
            let n = self.p.read(key, &mut buf)?;
            out.extend_from_slice(&buf[..n]);
            chunks.push((key, hash(&buf[..n])));
            mark(&mut self.used, key, true);
        }
        (out.len() == len).then_some((out, chunks))
    }

    fn load(&mut self, rows: &mut Visit<'_>) -> Option<Loaded> {
        let mut buf = [0u8; VALUE];
        let n = self.p.read(SUPER, &mut buf)?;
        let sup = buf[..n].to_vec();
        let mut r = Reader::new(&sup);
        if r.take(4).ok()? != MAGIC || r.byte().ok()? != FORMAT {
            return None;
        }
        let meta = Meta {
            origin: u64::decode(&mut r).ok()?,
            made: u64::decode(&mut r).ok()?,
            cursor: u64::decode(&mut r).ok()?,
            actor: u64::decode(&mut r).ok()?,
        };
        let dir_len = u32::decode(&mut r).ok()? as usize;
        let dir_keys = Vec::<u32>::decode(&mut r).ok()?;
        let (dir, dir_chunks) = self.blob(&dir_keys, dir_len)?;
        self.dir = dir_chunks;
        let mut r = Reader::new(&dir);
        let page_keys = Vec::<u32>::decode(&mut r).ok()?;
        let pending_keys = Vec::<u32>::decode(&mut r).ok()?;
        let pending_len = u32::decode(&mut r).ok()? as usize;
        for key in page_keys {
            let n = self.p.read(key, &mut buf)?;
            let page = &buf[..n];
            let mut r = Reader::new(page);
            let mut first = None;
            while !r.is_empty() {
                let (path, value) = (r.bytes().ok()?, r.bytes().ok()?);
                first.get_or_insert_with(|| path.to_vec());
                rows(path, value);
            }
            self.pages.push(Page {
                first: first?,
                key,
                hash: hash(page),
            });
            mark(&mut self.used, key, true);
        }
        let (pending, pending_chunks) = self.blob(&pending_keys, pending_len)?;
        self.pending = pending_chunks;
        self.sup = sup;
        self.lease = meta.made;
        Some(Loaded { meta, pending })
    }

    fn count(&mut self) {
        self.stats.used = self.used.iter().map(|w| w.count_ones()).sum();
    }

    /// Writes what changed since the last flush: the rows `rows` gives
    /// (each path and value, in path order), the pending records' bytes
    /// and `meta`, committed by the superblock. Answers the values it
    /// wrote; on a refusal of the storage nothing is committed, and the
    /// disk holds the flush before.
    pub fn flush(&mut self, meta: Meta, rows: &Walk<'_>, pending: &[u8]) -> Result<u32, Full> {
        let mut fresh = Vec::new();
        let before = self.stats.writes;
        match self.write(meta, rows, pending, &mut fresh) {
            Ok(()) => {
                self.stats.flushes += 1;
                self.stats.last = self.stats.writes - before;
                self.count();
                Ok(self.stats.last)
            }
            Err(Full) => {
                for k in fresh {
                    mark(&mut self.used, k, false);
                }
                self.stats.failed += 1;
                Err(Full)
            }
        }
    }

    fn write(&mut self, meta: Meta, rows: &Walk<'_>, pending: &[u8], fresh: &mut Vec<u32>) -> Result<(), Full> {
        // The rows, packed into their pages, a page split where it
        // outgrows a value.
        let mut packed: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut group = usize::MAX;
        let mut too_big = 0;
        let pages = &self.pages;
        rows(&mut |path, value| {
            let mut rec = Vec::with_capacity(path.len() + value.len() + 4);
            put_bytes(&mut rec, path);
            put_bytes(&mut rec, value);
            if rec.len() > VALUE {
                too_big += 1;
                return;
            }
            let g = pages.partition_point(|p| p.first.as_slice() <= path).saturating_sub(1);
            match packed.last_mut() {
                Some((_, bytes)) if g == group && bytes.len() + rec.len() <= VALUE => bytes.extend_from_slice(&rec),
                _ => {
                    // A page keeps its first path, so its neighbours' rows
                    // stay where they are.
                    let first = match pages.get(g) {
                        Some(p) if g != group && p.first.as_slice() <= path => p.first.clone(),
                        _ => path.to_vec(),
                    };
                    packed.push((first, rec));
                }
            }
            group = g;
        });
        self.stats.too_big = too_big;
        let mut joined: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for (first, bytes) in packed {
            match joined.last_mut() {
                Some((_, b)) if b.len() + bytes.len() <= JOIN => b.extend_from_slice(&bytes),
                _ => joined.push((first, bytes)),
            }
        }

        let writes = fresh.len();
        let mut pages = Vec::with_capacity(joined.len());
        for (first, bytes) in joined {
            let h = hash(&bytes);
            let kept = match self.pages.binary_search_by(|p| p.first.cmp(&first)) {
                Ok(i) if self.pages[i].hash == h => Some(self.pages[i].key),
                _ => None,
            };
            let key = match kept {
                Some(key) => key,
                None => put(&mut self.p, &mut self.used, fresh, &bytes)?,
            };
            pages.push(Page { first, key, hash: h });
        }
        let pend = chunks(&mut self.p, &mut self.used, fresh, pending, &self.pending)?;
        let mut dir = Vec::new();
        let page_keys: Vec<u32> = pages.iter().map(|p| p.key).collect();
        ark_kernel::Encode::encode(&page_keys, &mut dir);
        ark_kernel::Encode::encode(&keys(&pend), &mut dir);
        put_varint(&mut dir, pending.len() as u64);
        let dir_chunks = chunks(&mut self.p, &mut self.used, fresh, &dir, &self.dir)?;

        let lease = if meta.made + 8 > self.lease { meta.made + LEASE } else { self.lease };
        let mut sup = Vec::with_capacity(64);
        sup.extend_from_slice(MAGIC);
        sup.push(FORMAT);
        for v in [meta.origin, lease, meta.cursor, meta.actor, dir.len() as u64] {
            put_varint(&mut sup, v);
        }
        ark_kernel::Encode::encode(&keys(&dir_chunks), &mut sup);
        if sup.len() > VALUE {
            return Err(Full);
        }
        let mut wrote = (fresh.len() - writes) as u32;
        let mut bytes: u32 = fresh[writes..].len() as u32 * VALUE as u32;
        if sup != self.sup {
            if !self.p.write(SUPER, &sup) {
                return Err(Full);
            }
            wrote += 1;
            bytes += sup.len() as u32;
        }

        // Committed: the keys no longer in use are free.
        let old: Vec<u32> = self
            .pages
            .iter()
            .map(|p| p.key)
            .chain(keys(&self.pending))
            .chain(keys(&self.dir))
            .collect();
        let new: Vec<u32> = page_keys.iter().copied().chain(keys(&pend)).chain(keys(&dir_chunks)).collect();
        for k in old {
            if !new.contains(&k) {
                mark(&mut self.used, k, false);
            }
        }
        self.pages = pages;
        self.pending = pend;
        self.dir = dir_chunks;
        self.sup = sup;
        self.lease = lease;
        self.stats.writes += wrote;
        self.stats.bytes += bytes;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::vec;

    /// Storage in memory that counts its writes, and can be made to refuse
    /// them.
    #[derive(Default, Clone)]
    struct Map {
        values: BTreeMap<u32, Vec<u8>>,
        writes: u32,
        refuse: bool,
    }

    impl Persist for &mut Map {
        fn read(&mut self, key: u32, buf: &mut [u8; VALUE]) -> Option<usize> {
            let v = self.values.get(&key)?;
            buf[..v.len()].copy_from_slice(v);
            Some(v.len())
        }

        fn write(&mut self, key: u32, data: &[u8]) -> bool {
            assert!(data.len() <= VALUE);
            if self.refuse {
                return false;
            }
            self.writes += 1;
            self.values.insert(key, data.to_vec());
            true
        }
    }

    type Rowset = BTreeMap<Vec<u8>, Vec<u8>>;

    fn each(rows: &Rowset) -> impl Fn(&mut Visit<'_>) + '_ {
        move |f| {
            for (k, v) in rows {
                f(k, v);
            }
        }
    }

    fn open(map: &mut Map) -> (Disk<&mut Map>, Rowset, Option<Loaded>) {
        let mut rows = Rowset::new();
        let (d, loaded) = Disk::open(map, &mut |k, v| {
            rows.insert(k.to_vec(), v.to_vec());
        });
        (d, rows, loaded)
    }

    fn row(i: u32) -> (Vec<u8>, Vec<u8>) {
        let mut k = vec![3, 0, 0, 0, 1];
        k.extend_from_slice(&i.to_be_bytes());
        (k, vec![i as u8; 40])
    }

    #[test]
    fn a_disk_comes_back_as_it_was_flushed() {
        let mut map = Map::default();
        let (mut d, rows, loaded) = open(&mut map);
        assert!(loaded.is_none() && rows.is_empty());
        let rows: Rowset = (0..40).map(row).collect();
        let meta = Meta {
            origin: 7,
            made: 3,
            cursor: 9,
            actor: 1,
        };
        let pending = vec![5u8; 600];
        let wrote = d.flush(meta, &each(&rows), &pending).unwrap();
        // 40 rows of 51 bytes, five a page: 8 pages, 3 chunks, a directory
        // and the superblock.
        assert_eq!(wrote, 13);
        // Nothing changed: nothing written.
        assert_eq!(d.flush(meta, &each(&rows), &pending).unwrap(), 0);
        drop(d);
        let (d, back, loaded) = open(&mut map);
        let loaded = loaded.unwrap();
        assert_eq!(back, rows);
        assert_eq!(loaded.pending, pending);
        assert_eq!(loaded.meta, Meta { made: 3 + LEASE, ..meta });
        assert_eq!(d.stats.used, 13);
    }

    /// A row changed rewrites its page alone; one inserted or removed, its
    /// page and its neighbours at most.
    #[test]
    fn a_flush_writes_what_changed() {
        let mut map = Map::default();
        let (mut d, _, _) = open(&mut map);
        let mut rows: Rowset = (0..100).step_by(2).map(row).collect();
        let meta = Meta::default();
        d.flush(meta, &each(&rows), &[]).unwrap();
        let r = row(40);
        rows.insert(r.0, vec![9; 40]);
        // The page, the directory, and the superblock (the directory's key).
        assert_eq!(d.flush(meta, &each(&rows), &[]).unwrap(), 3);
        let r = row(41);
        rows.insert(r.0, r.1);
        assert!(d.flush(meta, &each(&rows), &[]).unwrap() <= 4);
        rows.remove(&row(80).0);
        assert!(d.flush(meta, &each(&rows), &[]).unwrap() <= 4);
        let used = d.stats.used;
        drop(d);
        let (d, back, _) = open(&mut map);
        assert_eq!(back, rows);
        assert_eq!(d.stats.used, used);
    }

    /// A flush the storage refuses commits nothing: the disk reads as the
    /// flush before, and the next flush writes it all.
    #[test]
    fn a_refused_flush_leaves_the_last() {
        let mut map = Map::default();
        let (mut d, _, _) = open(&mut map);
        let rows: Rowset = (0..10).map(row).collect();
        d.flush(Meta::default(), &each(&rows), b"one").unwrap();
        let mut more = rows.clone();
        more.insert(row(50).0, row(50).1);
        d.persist().refuse = true;
        assert!(d
            .flush(
                Meta {
                    cursor: 4,
                    ..Meta::default()
                },
                &each(&more),
                b"two"
            )
            .is_err());
        d.persist().refuse = false;
        drop(d);
        let (mut d, back, loaded) = open(&mut map);
        assert_eq!((back, loaded.unwrap().pending), (rows, b"one".to_vec()));
        d.flush(Meta::default(), &each(&more), b"two").unwrap();
        drop(d);
        let (_, back, _) = open(&mut map);
        assert_eq!(back, more);
    }

    /// At random: rows put and removed, flushed now and then, always come
    /// back as flushed, and the values in use stay near what they hold.
    #[test]
    fn random_flushes_come_back() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rand = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut map = Map::default();
        let mut rows = Rowset::new();
        for round in 0..300 {
            let (mut d, back, _) = open(&mut map);
            assert_eq!(back, rows, "round {round}");
            for _ in 0..rand(6) {
                for _ in 0..rand(8) {
                    let mut k = vec![[1, 3][rand(2) as usize], 0, 0, 0, rand(3) as u8];
                    k.extend((0..rand(4)).map(|_| rand(256) as u8));
                    if rand(3) == 0 {
                        rows.remove(&k);
                    } else {
                        rows.insert(k, vec![rand(256) as u8; rand(120) as usize]);
                    }
                }
                let pending = vec![rand(256) as u8; rand(700) as usize];
                d.flush(Meta::default(), &each(&rows), &pending).unwrap();
            }
            let bytes: usize = rows.iter().map(|(k, v)| k.len() + v.len() + 2).sum();
            assert!(
                (d.stats.used as usize) < 12 + 4 * bytes.div_ceil(VALUE),
                "{} values for {bytes} bytes",
                d.stats.used
            );
        }
    }
}
