//! §1.2 and §1.3 held: constraints refuse, every change inverts exactly,
//! the incremental state hash is the hash of the rows, the schema is in
//! it, a volatile table is not, and the compat check is a prefix check.

#[path = "demo.rs"]
mod demo;

use std::sync::Arc;

use ark::format::put_key;
use ark::hash::hex;
use ark::row::{Id, Type};
use ark::schema::{compat, Schema, Table};
use ark::store::{Change, Dir, Read, Scan, Store};
use demo::{item_of, item_row, playlist_row};

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) % n.max(1)
    }
}

fn id(n: u8) -> Vec<u8> {
    let mut i = vec![0; 16];
    i[15] = n;
    i
}

/// A key's bytes.
fn key<T: serde::Serialize>(k: T) -> Vec<u8> {
    let mut out = vec![];
    put_key(&k, &mut out);
    out
}

fn pid(n: u8) -> Id<()> {
    Id::new(id(n).try_into().unwrap())
}

/// A row of `presence(who)`.
fn presence(who: &str) -> Vec<u8> {
    row_of(&Table::new("presence").col("who", Type::Text), serde_json::json!({ "who": who }))
}

/// A row of `t` from its columns' JSON.
fn row_of(t: &Table, v: serde_json::Value) -> Vec<u8> {
    ark::dynamic::row_bytes(t, &v).unwrap()
}

fn whys(s: &mut Store, c: Change) -> String {
    s.apply(&c).expect_err("refused").0
}

/// Each constraint refuses by what it is, and a refused write leaves the
/// store as it was. Falsified by skipping `judge`'s reference check: the
/// item of a playlist nobody created was taken.
#[test]
fn constraints_refuse() {
    let mut s = Store::new(demo::shared(), ark::schema::Space::Durable);
    let empty = s.clone();
    let item = |n, t: &str, p| Change::Add {
        table: "item".into(),
        row: item_row(&id(n), t, p),
    };
    assert!(whys(&mut s, item(1, "t1", 1)).contains("no playlist has this key"));
    s.put("playlist", playlist_row(&id(1), "Favorites", "alice")).unwrap();
    let dup = Change::Add {
        table: "playlist".into(),
        row: playlist_row(&id(2), "Favorites", "alice"),
    };
    assert!(whys(&mut s, dup).contains("another row has this (user_id, name)"));
    s.apply(&item(1, "t1", 1)).unwrap();
    assert!(whys(&mut s, item(1, "t2", 1)).contains("(playlist_id, pos)"));
    assert!(whys(&mut s, item(1, "t1", 2)).contains("a row with this key exists"));
    let first = Table::new("item").col("playlist_id", Type::Id("playlist".into()));
    let short = Change::Add {
        table: "item".into(),
        row: row_of(&first, serde_json::json!({"playlist_id": hex(&id(1))})),
    };
    assert!(whys(&mut s, short).contains("item"));
    // The row's bytes but not its one encoding: its last varint (`pos`, 5)
    // not the shortest.
    let mut other = item_row(&id(1), "t5", 5);
    assert_eq!(other.pop(), Some(10));
    other.extend([0x8a, 0x00]);
    let unordered = Change::Add {
        table: "item".into(),
        row: other,
    };
    assert!(whys(&mut s, unordered).contains("one encoding"));
    let rekey = Change::Edit {
        table: "item".into(),
        old: item_row(&id(1), "t1", 1),
        new: item_row(&id(1), "t9", 1),
    };
    assert!(whys(&mut s, rekey).contains("keeps its key"));
    let stale = Change::Remove {
        table: "item".into(),
        row: item_row(&id(1), "t1", 5),
    };
    assert!(whys(&mut s, stale).contains("not the one"));
    let before = s.clone();
    assert!(s.apply_all(&[item(1, "t2", 2), item(1, "t3", 2)]).is_err());
    assert_eq!(s, before, "all or none");
    s.delete("item", &key((pid(1), "t1"))).unwrap();
    s.delete("playlist", &key(pid(1))).unwrap();
    assert_eq!(s, empty);
    assert_eq!(s.state_hash(), empty.state_hash());
}

/// 3,000 random writes over the demo, each one's change applied in reverse
/// order at the end: the store is the empty one again, byte for byte and
/// hash for hash; and at every step the kept hash is the hash of the rows
/// put into a fresh store. Falsified by `Change::inverse` swapping an
/// `Edit` the wrong way round (old for old): an undo was refused.
#[test]
fn every_change_inverts_and_the_hash_is_the_rows() {
    let mut r = Rng(3);
    let schema = demo::shared();
    let mut s = Store::new(schema.clone(), ark::schema::Space::Durable);
    let empty = s.clone();
    let mut done = vec![];
    for k in 0..3000 {
        let p = id(r.below(4) as u8);
        let c = match r.below(4) {
            0 => s.put("playlist", playlist_row(&p, &format!("n{}", r.below(3)), "u")),
            1 | 2 => s.put("item", item_row(&p, &format!("t{}", r.below(6)), r.below(8) as i64)),
            _ => {
                let rows: Vec<Vec<u8>> = s.rows("item").map(|x| x.to_vec()).collect();
                match rows.get(r.below(rows.len() as u64 + 1) as usize) {
                    Some(row) => s
                        .apply(&Change::Remove {
                            table: "item".into(),
                            row: row.clone(),
                        })
                        .map(|_| Change::Remove {
                            table: "item".into(),
                            row: row.clone(),
                        }),
                    None => continue,
                }
            }
        };
        if let Ok(c) = c {
            done.push(c);
        }
        if k % 97 == 0 {
            let fresh = Store::from_rows(schema.clone(), ark::schema::Space::Durable, &s.snapshot_rows())
                .unwrap()
                .0;
            assert_eq!(fresh.state_hash(), s.state_hash(), "at write {k}");
            assert_eq!(fresh, s);
        }
    }
    assert!(done.len() > 1000 && done.iter().any(|c| matches!(c, Change::Edit { .. })));
    let full = s.state_hash();
    assert_eq!(
        s.hash_beside([&s.undo_of(done.iter().rev())]),
        empty.state_hash(),
        "the digests' arithmetic undoes it too"
    );
    for c in done.iter().rev() {
        s.apply(&c.inverse()).unwrap();
    }
    assert_eq!(s, empty);
    assert_ne!(full, empty.state_hash());
}

/// Rows order by key, an index scan by its columns then the key, in either
/// direction, bounded. Falsified by `Scan::prefix` leaving `hi` unbounded:
/// the next playlist's items were read too.
#[test]
fn scans_read_in_order() {
    let mut s = Store::new(demo::shared(), ark::schema::Space::Durable);
    for p in [1, 2] {
        s.put("playlist", playlist_row(&id(p), &format!("p{p}"), "u")).unwrap();
        for (t, pos) in [("t7", 1), ("t11", 3), ("t9", 2)] {
            s.put("item", item_row(&id(p), t, pos)).unwrap();
        }
    }
    let tracks = |rows: Vec<Vec<u8>>| rows.iter().map(|r| item_of(r).1).collect::<Vec<_>>();
    let p = key(pid(1));
    assert_eq!(tracks(s.scan("item", &Scan::prefix(None, &p, Dir::Asc, 9))), ["t11", "t7", "t9"]);
    assert_eq!(tracks(s.scan("item", &Scan::prefix(Some(0), &p, Dir::Asc, 9))), ["t7", "t9", "t11"]);
    assert_eq!(tracks(s.scan("item", &Scan::prefix(Some(0), &p, Dir::Desc, 1))), ["t11"]);
    assert_eq!(s.scan("item", &Scan::all()).len(), 6);
    assert_eq!(item_of(&s.get("item", &key((pid(1), "t7"))).unwrap()).2, 1);
}

/// Two schemas that differ in one column's name hash their empty stores
/// apart; a volatile table moves neither the hash nor a snapshot.
/// Falsified by leaving the schema hash out of `hash_of`: the two empty
/// stores hashed alike.
#[test]
fn the_schema_is_in_the_hash_and_volatile_tables_are_not() {
    let mut other = demo::schema();
    other.tables[0].columns[1].name = "title".into();
    assert_ne!(
        Store::new(demo::shared(), ark::schema::Space::Durable).state_hash(),
        Store::new(Arc::new(other), ark::schema::Space::Durable).state_hash()
    );
    let mut sch = demo::schema();
    sch.tables.push(Table::new("presence").col("who", Type::Text).volatile());
    let mut s = Store::new(Arc::new(sch), ark::schema::Space::Durable);
    let h = s.state_hash();
    s.put("presence", presence("alice")).unwrap();
    assert_eq!(s.state_hash(), h);
    assert_eq!(s.snapshot_rows().len(), 2);
}

/// A store of the volatile space is the mirror: its hash and snapshot are
/// the volatile tables', and a durable row moves neither. Falsified by
/// `hash_of` keeping the durable tables whatever the store's space: the
/// volatile store's hash moved with a playlist.
#[test]
fn a_volatile_store_hashes_its_own_space() {
    let mut sch = demo::schema();
    sch.tables.push(Table::new("presence").col("who", Type::Text).volatile());
    let mut s = Store::new(Arc::new(sch), ark::schema::Space::Volatile);
    let h = s.state_hash();
    s.put("playlist", playlist_row(&id(1), "x", "alice")).unwrap();
    assert_eq!(s.state_hash(), h, "a durable row is not the volatile space's");
    s.put("presence", presence("alice")).unwrap();
    assert_ne!(s.state_hash(), h);
    assert_eq!(s.snapshot_rows(), vec![("presence".to_string(), vec![presence("alice")])]);
}

/// A row others name is not removed while they do (v4's
/// `StillReferenced`), through an index the referring column leads, the
/// key it leads, or a scan of the referring table; once they are gone it
/// is. Falsified by `apply` skipping `unnamed`: the playlist with an item
/// on it was removed.
#[test]
fn a_row_still_named_is_not_removed() {
    let opt = Type::Option(Box::new(Type::Id("playlist".into())));
    let mut sch = demo::schema();
    sch.tables
        .push(Table::new("pin").col("n", Type::Int).col("of", opt).refs("of", "playlist"));
    let mut s = Store::new(Arc::new(sch), ark::schema::Space::Durable);
    s.put("playlist", playlist_row(&id(1), "x", "alice")).unwrap();
    s.put("playlist", playlist_row(&id(2), "y", "alice")).unwrap();
    s.put("item", item_row(&id(1), "t", 1)).unwrap();
    let pin = row_of(&s.schema().tables[2].clone(), serde_json::json!({"n": 1, "of": hex(&id(2))}));
    s.put("pin", pin).unwrap();
    let before = s.clone();
    assert!(s.delete("playlist", &key(pid(1))).unwrap_err().0.contains("named by item.playlist_id"));
    assert!(s.delete("playlist", &key(pid(2))).unwrap_err().0.contains("named by pin.of"));
    assert_eq!(s, before, "refused, nothing moved");
    s.delete("pin", &key(1i64)).unwrap();
    s.delete("playlist", &key(pid(2))).unwrap();
    s.delete("item", &key((pid(1), "t"))).unwrap();
    s.delete("playlist", &key(pid(1))).unwrap();
}

/// §1.2 compat is a prefix check: appending an `Option` column, a defaulted
/// one or a table passes; a removal, a retype or a bare appended column
/// does not. A row written under the older schema is completed under the
/// newer and one written under the newer is read without what it has
/// beyond. Falsified by `compat` comparing only the columns' count: the
/// retype passed.
#[test]
fn compat_is_a_prefix_extension() {
    let old = demo::schema();
    let mut new = old.clone();
    new.tables[1] = new.tables[1].clone().col("note", Type::Option(Box::new(Type::Text)));
    new.tables[1] = new.tables[1].clone().col("plays", Type::Int);
    new.tables[1].columns[4].default = Some(serde_json::json!(0));
    new.tables.push(Table::new("tag").col("name", Type::Text));
    assert_eq!(compat(&old, &new), Ok(()));
    let mut bare = new.clone();
    bare.tables[1] = bare.tables[1].clone().col("x", Type::Int);
    assert!(compat(&old, &bare).is_err());
    let mut retyped = old.clone();
    retyped.tables[1].columns[2].ty = Type::Text;
    assert!(compat(&old, &retyped).is_err());
    assert!(compat(&new, &old).is_err());
    assert!(compat(
        &old,
        &Schema {
            tables: vec![old.tables[0].clone()]
        }
    )
    .is_err());
    let row = item_row(&id(1), "t1", 4);
    let (full, other) = new.tables[1].fit(&row).unwrap();
    assert!(other);
    assert_eq!(
        ark::dynamic::row(&new.tables[1], &full).unwrap(),
        serde_json::json!({"playlist_id": hex(&id(1)), "track_id": "t1", "pos": 4, "note": null, "plays": 0})
    );
    assert_eq!(old.tables[1].fit(&full).unwrap(), (row.clone(), true));
    assert_eq!(old.tables[1].fit(&row).unwrap(), (row, false));
}

/// The log is a table whose invariant is the chain (§1.6): a row is added
/// only at an end it links onto, removed only at an end, never edited; a
/// write to it through `apply`, the door a function's facts take, is
/// refused; and it is in neither the state hash nor a snapshot sent.
/// Falsified by `apply_kv` taking a `Remove` of any row of the log: the
/// middle entry came out.
#[test]
fn the_log_is_a_table_whose_rows_are_a_chain() {
    use ark::format::Encode;
    use ark::hash::{EntryHash, StateHash};
    use ark::log::{Entry, Logged};
    use ark::store::LOG;
    let mut s = Store::new(demo::shared(), ark::schema::Space::Durable);
    let (hash, rows) = (s.state_hash(), s.snapshot_rows());
    let mut chain = vec![];
    let mut prev = EntryHash::default();
    for seq in 0..4 {
        let entry = Entry {
            local: seq,
            ..Entry::default()
        };
        let state = StateHash([seq as u8; 32]);
        let hash = Logged::chain(&prev, seq, &entry, &[], &state);
        prev = hash;
        chain.push(Logged {
            seq,
            entry,
            facts: vec![],
            state,
            hash,
        });
    }
    let row = |l: &Logged| l.to_bytes();
    let add = |l: &Logged| Change::Add {
        table: LOG.into(),
        row: row(l),
    };
    let remove = |l: &Logged| Change::Remove {
        table: LOG.into(),
        row: row(l),
    };
    s.start_log([row(&chain[1])]).unwrap();
    s.apply_kv(&add(&chain[2])).unwrap();
    s.apply_kv(&add(&chain[0])).expect("added at the front, linking");
    assert!(s.apply_kv(&add(&chain[2])).is_err(), "not at an end");
    let mut gap = chain[3].clone();
    gap.seq = 4;
    assert!(s.apply_kv(&add(&gap)).is_err(), "a gap");
    let mut other = chain[3].clone();
    other.state = StateHash([9; 32]);
    assert!(s.apply_kv(&add(&other)).is_err(), "it does not link");
    s.apply_kv(&add(&chain[3])).unwrap();
    assert!(s.apply_kv(&remove(&chain[1])).is_err(), "the middle stays");
    let edit = Change::Edit {
        table: LOG.into(),
        old: row(&chain[3]),
        new: row(&chain[3]),
    };
    assert!(s.apply_kv(&edit).is_err(), "never edited");
    assert!(s.apply(&add(&chain[3])).is_err(), "not a table a fact names");
    assert_eq!(s.log().entries(), chain[1..].to_vec());
    s.apply_kv(&remove(&chain[0])).expect("the horizon moves");
    s.apply_kv(&remove(&chain[3])).expect("the head comes off");
    assert_eq!((s.log().base(), s.log().head()), (chain[1].clone(), (2, chain[2].hash)));
    assert_eq!((s.state_hash(), s.snapshot_rows()), (hash, rows), "not in the hash or a snapshot sent");
}
