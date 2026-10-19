//! §1.2 and §1.3 held: constraints refuse, every change inverts exactly,
//! the incremental state hash is the hash of the rows, the schema is in
//! it, a volatile table is not, and the compat check is a prefix check.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod gen;
}

use std::sync::Arc;

use ark5::codec::{bytes_enc, put_bytes};
use ark5::schema::{compat, Schema, Table};
use ark5::store::{Change, Dir, Read, Scan, Store};
use ark5::ty::Ty;
use demo::{item_of, item_row, playlist_row};
use support::gen::Rng;

fn id(n: u8) -> Vec<u8> {
    let mut i = vec![0; 16];
    i[15] = n;
    i
}

fn whys(s: &mut Store, c: Change) -> String {
    s.apply(&c).expect_err("refused").0
}

/// Each constraint refuses by what it is, and a refused write leaves the
/// store as it was. Falsified by skipping `judge`'s reference check: the
/// item of a playlist nobody created was taken.
#[test]
fn constraints_refuse() {
    let mut s = Store::new(demo::shared());
    let empty = s.clone();
    let item = |n, t: &str, p| Change::Add { table: "item".into(), row: item_row(&id(n), t, p) };
    assert!(whys(&mut s, item(1, "t1", 1)).contains("no playlist has this key"));
    s.put("playlist", playlist_row(&id(1), "Favorites", "alice")).unwrap();
    let dup = Change::Add { table: "playlist".into(), row: playlist_row(&id(2), "Favorites", "alice") };
    assert!(whys(&mut s, dup).contains("another row has this (user_id, name)"));
    s.apply(&item(1, "t1", 1)).unwrap();
    assert!(whys(&mut s, item(1, "t2", 1)).contains("(playlist_id, pos)"));
    assert!(whys(&mut s, item(1, "t1", 2)).contains("a row with this key exists"));
    let short = Change::Add { table: "item".into(), row: bytes_enc(&id(1)) };
    assert!(whys(&mut s, short).contains("item.track_id"));
    let rekey = Change::Edit {
        table: "item".into(),
        old: item_row(&id(1), "t1", 1),
        new: item_row(&id(1), "t9", 1),
    };
    assert!(whys(&mut s, rekey).contains("keeps its key"));
    let stale = Change::Remove { table: "item".into(), row: item_row(&id(1), "t1", 5) };
    assert!(whys(&mut s, stale).contains("not the one"));
    let before = s.clone();
    assert!(s.apply_all(&[item(1, "t2", 2), item(1, "t3", 2)]).is_err());
    assert_eq!(s, before, "all or none");
    s.delete("item", &item_row(&id(1), "t1", 1)[..bytes_enc(&id(1)).len() + 4]).unwrap();
    s.delete("playlist", &bytes_enc(&id(1))).unwrap();
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
    let mut s = Store::new(schema.clone());
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
                    Some(row) => s.apply(&Change::Remove { table: "item".into(), row: row.clone() }).map(|_| Change::Remove {
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
            let fresh = Store::from_rows(schema.clone(), &s.snapshot_rows()).unwrap();
            assert_eq!(fresh.state_hash(), s.state_hash(), "at write {k}");
            assert_eq!(fresh, s);
        }
    }
    assert!(done.len() > 1000 && done.iter().any(|c| matches!(c, Change::Edit { .. })));
    let full = s.state_hash();
    assert_eq!(s.hash_beside(done.iter().rev()), empty.state_hash(), "the digests' arithmetic undoes it too");
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
    let mut s = Store::new(demo::shared());
    for p in [1, 2] {
        s.put("playlist", playlist_row(&id(p), &format!("p{p}"), "u")).unwrap();
        for (t, pos) in [("t7", 1), ("t11", 3), ("t9", 2)] {
            s.put("item", item_row(&id(p), t, pos)).unwrap();
        }
    }
    let tracks = |rows: Vec<Vec<u8>>| rows.iter().map(|r| item_of(r).1).collect::<Vec<_>>();
    let key = bytes_enc(&id(1));
    assert_eq!(tracks(s.scan("item", &Scan::prefix(None, &key, Dir::Asc, 9))), ["t11", "t7", "t9"]);
    assert_eq!(tracks(s.scan("item", &Scan::prefix(Some(0), &key, Dir::Asc, 9))), ["t7", "t9", "t11"]);
    assert_eq!(tracks(s.scan("item", &Scan::prefix(Some(0), &key, Dir::Desc, 1))), ["t11"]);
    assert_eq!(s.scan("item", &Scan::all()).len(), 6);
    let mut k = key.clone();
    put_bytes(&mut k, b"t7");
    assert_eq!(item_of(&s.get("item", &k).unwrap()).2, 1);
}

/// Two schemas that differ in one column's name hash their empty stores
/// apart; a volatile table moves neither the hash nor a snapshot.
/// Falsified by leaving the schema hash out of `hash_of`: the two empty
/// stores hashed alike.
#[test]
fn the_schema_is_in_the_hash_and_volatile_tables_are_not() {
    let mut other = demo::schema();
    other.tables[0].columns[1].name = "title".into();
    assert_ne!(Store::new(demo::shared()).state_hash(), Store::new(Arc::new(other)).state_hash());
    let mut sch = demo::schema();
    sch.tables.push(Table::new("presence").col("who", "Text", Ty::Bytes).volatile());
    let mut s = Store::new(Arc::new(sch));
    let h = s.state_hash();
    s.put("presence", bytes_enc(b"alice")).unwrap();
    assert_eq!(s.state_hash(), h);
    assert_eq!(s.snapshot_rows().len(), 2);
}

/// §1.2 compat is a prefix check: appending an `Option` column, a defaulted
/// one or a table passes; a removal, a retype or a bare appended column
/// does not. A row written under the older schema is completed under the
/// newer and one written under the newer is read by its prefix. Falsified
/// by `compat` comparing only the columns' count: the retype passed.
#[test]
fn compat_is_a_prefix_extension() {
    let old = demo::schema();
    let mut new = old.clone();
    new.tables[1] = new.tables[1].clone().col("note", "Option<Text>", Ty::option(Ty::Bytes));
    new.tables[1] = new.tables[1].clone().col("plays", "Int", Ty::Int);
    new.tables[1].columns[4].default = Some(ark5::codec::int_enc(0).to_vec());
    new.tables.push(Table::new("tag").col("name", "Text", Ty::Bytes));
    assert_eq!(compat(&old, &new), Ok(()));
    let mut bare = new.clone();
    bare.tables[1] = bare.tables[1].clone().col("x", "Int", Ty::Int);
    assert!(compat(&old, &bare).is_err());
    let mut retyped = old.clone();
    retyped.tables[1].columns[2].ty = Ty::Bytes;
    assert!(compat(&old, &retyped).is_err());
    assert!(compat(&new, &old).is_err());
    assert!(compat(&old, &Schema { tables: vec![old.tables[0].clone()] }).is_err());
    let row = item_row(&id(1), "t1", 4);
    let full = new.tables[1].complete(&row).unwrap();
    assert_eq!(new.tables[1].spans(&full).unwrap().len(), 5);
    assert_eq!(old.tables[1].prefix(&full).unwrap(), &row[..]);
}
