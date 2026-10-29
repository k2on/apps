//! §1.2 A schema grows under a log: an authority reopened under a
//! prefix-extension of the schema its records were written under keeps its
//! head, its chain and its rows, the rows widened and the store's digests
//! this schema's; a replica of the older schema takes what the wider one
//! logs on faith, as it says (it is behind).

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use std::sync::Arc;

use ark::frames::Frame;
use ark::log::{Durable, EntryId, Origin};
use ark::peer::{Ctx, Input, Journal, Peer, Procedure, Role};
use ark::row::Type;
use ark::schema::{Column, Schema, Table};
use ark::store::{Change, Store};
use ark::Refusal;
use demo::{create_input, pair_input, ADD, CREATE};
use support::wire::*;

/// The demo's schema with a column appended to `playlist` (an `Option`) and
/// a table appended, `tag`.
fn wider() -> Arc<Schema> {
    let mut s = demo::schema();
    s.tables[0].columns.push(Column {
        name: "note".into(),
        ty: Type::Option(Box::new(Type::Text)),
        default: None,
    });
    s.tables.push(Table::new("tag").col("name", Type::Text));
    ark::schema::compat(&demo::schema(), &s).unwrap();
    Arc::new(s)
}

/// Tags a row in the appended table: what only the wider schema can log.
struct TagIt;

impl Procedure for TagIt {
    fn apply(&self, store: &mut Store, _: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal> {
        let c = Change::Add {
            table: "tag".into(),
            row: input.to_vec(),
        };
        store.apply(&c)?;
        Ok(vec![c])
    }
}

const TAG: &str = "grow.tag";

fn durable(p: &Peer) -> Durable {
    Durable {
        rows: p.confirmed_rows(),
        ..Durable::default()
    }
}

fn wide_authority(d: Durable) -> Peer {
    let mut cas = demo::cas();
    cas.insert(TAG, Arc::new(TagIt));
    let mut p = Peer::new(wider(), cas, Role::Authority { journal: Journal::default() }, who("server"));
    p.step(Input::Open(d));
    p
}

/// An authority reopened under an appended column and an appended table
/// keeps its head, its chain entry for entry and its rows, the playlist
/// row widened with `None`; its store hashes as the wider schema's from
/// those rows; what it logs next carries that hash; a `Hello` below the
/// entries logged under the old schema is answered with the snapshot at
/// the head; reopened again, and after compacting, it is the same. Alice,
/// on the old schema, takes the entry touching the table she lacks as it
/// says and stays at the head. Falsified by `open` deriving each entry's
/// state again (cutting the chain where it differs): the head was 0.
#[test]
fn a_log_reopened_under_a_wider_schema_keeps_head_chain_and_rows() {
    let mut server = authority(demo::cas());
    let mut alice = replica("alice", demo::cas());
    for (f, input) in [
        (CREATE, create_input("Road")),
        (ADD, pair_input(&EntryId::of(Origin::named("alice-session"), 1).draw(0), "t1")),
    ] {
        let o = alice.step(mutate(f, input, 1));
        let (_, fan) = deliver_up(&mut server, &who("alice"), &o);
        alice.step(down(&fan[0]));
    }
    assert_eq!(server.log().head().0, 2);
    let mut wide = wide_authority(durable(&server));
    assert_eq!(wide.log(), server.log(), "head and chain kept");
    assert_eq!(wide.foreign, Some(2));
    let rows = |p: &Peer, t: &str| p.store.rows(t).map(<[u8]>::to_vec).collect::<Vec<_>>();
    let widened: Vec<Vec<u8>> = rows(&server, "playlist")
        .into_iter()
        .map(|r| {
            let mut v = ark::dynamic::row(&demo::schema().tables[0], &r).unwrap();
            v["note"] = serde_json::Value::Null;
            ark::dynamic::row_bytes(&wider().tables[0], &v).unwrap()
        })
        .collect();
    assert_eq!(rows(&wide, "playlist"), widened, "a row widened with None");
    assert_eq!(rows(&wide, "item"), rows(&server, "item"));
    let fresh = Store::from_rows(wider(), ark::schema::Space::Durable, &wide.store.snapshot_rows())
        .unwrap()
        .0;
    assert_eq!(wide.store.state_hash(), fresh.state_hash(), "the digests are the wider schema's");
    assert_ne!(wide.store.state_hash(), server.store.state_hash());

    let o = wide.step(Input::Mutate {
        func: TAG.into(),
        input: ark::dynamic::row_bytes(&wider().tables[2], &serde_json::json!({"name": "loud"})).unwrap(),
        now: 3,
        cause: None,
    });
    let fan = fanouts(&o);
    assert_eq!(logged(&fan[0]).state, wide.store.state_hash(), "a new entry carries the wider hash");
    let s = sends(&wide.step(up(
        &who("bob"),
        &Frame::Hello {
            space: ark::schema::Space::Durable,
            cursor: 0,
            hash: None,
            device: None,
            schema: Default::default(),
            version: String::new(),
        },
    )));
    assert!(
        matches!(&s[..], [Frame::Part { part, .. }] if part.first && part.seq == 3 && part.state == wide.store.state_hash() && part.next.is_none()),
        "{s:?}"
    );

    alice.step(down(&fan[0]));
    assert!(alice.faith && alice.diverged.is_empty(), "alice is behind, on faith");
    assert_eq!(alice.log(), wide.log());
    assert_eq!(rows(&alice, "playlist"), rows(&server, "playlist"), "she holds the prefix");

    let again = wide_authority(durable(&wide));
    assert_eq!((again.log(), &again.store), (wide.log(), &wide.store));
    wide.step(Input::Compact(3));
    let compacted = wide_authority(durable(&wide));
    assert_eq!(compacted.log().head(), wide.log().head());
    assert_eq!((compacted.store.clone(), compacted.foreign), (wide.store.clone(), None));
}

/// A replica of the old schema beside an authority whose schema only
/// appended a table: an entry's facts read the same under both, its state
/// does not. Alice diverges there, starts again, diverges there again, and
/// then asks for the snapshot (cursor -1), which names the table she lacks:
/// she takes it as it says and is at the head, her intent confirmed.
/// Falsified by `diverge` asking from 0 every time: she never left the
/// divergence (a bounded loop of Hellos, never a snapshot).
#[test]
fn a_replica_behind_an_appended_table_asks_for_the_snapshot() {
    let mut s = demo::schema();
    s.tables.push(Table::new("tag").col("name", Type::Text));
    let mut server = Peer::new(Arc::new(s), demo::cas(), Role::Authority { journal: Journal::default() }, who("server"));
    let mut alice = replica("alice", demo::cas());
    let mut up_frames = sends(&alice.step(mutate(CREATE, create_input("Road"), 1)));
    for _ in 0..8 {
        let mut back = vec![];
        for f in up_frames.drain(..) {
            let o = server.step(up(&who("alice"), &f));
            back.extend(fanouts(&o));
            back.extend(sends(&o));
        }
        if back.is_empty() {
            break;
        }
        for f in back {
            up_frames.extend(sends(&alice.step(down(&f))));
        }
    }
    assert!(up_frames.is_empty(), "settled: {up_frames:?}");
    assert_eq!(alice.diverged, vec![1, 1]);
    assert!(alice.faith && alice.pending.is_empty());
    assert_eq!(alice.log().head(), server.log().head());
    assert_eq!(alice.store.rows("playlist").count(), 1);
}

/// A log reopened under the schema it was written under is wholly its
/// own: nothing foreign, nothing on faith, each entry's state its rows'
/// with the facts after it undone (§1.3's arithmetic); and reopened from
/// its entries appended, the same. Falsified by `foreign` comparing each
/// entry's state with the head's digests, never undoing: entry 1 was
/// foreign.
#[test]
fn a_log_reopened_under_its_own_schema_is_its_own() {
    let mut server = authority(demo::cas());
    let mut alice = replica("alice", demo::cas());
    for (f, input) in [
        (CREATE, create_input("Road")),
        (ADD, pair_input(&EntryId::of(Origin::named("alice-session"), 1).draw(0), "t1")),
    ] {
        let o = alice.step(mutate(f, input, 1));
        let (_, fan) = deliver_up(&mut server, &who("alice"), &o);
        alice.step(down(&fan[0]));
    }
    let mut whole = authority(demo::cas());
    whole.step(Input::Open(durable(&server)));
    let base = Durable {
        rows: authority(demo::cas()).confirmed_rows(),
        logged: server.log().entries(),
        ..Durable::default()
    };
    let mut appended = authority(demo::cas());
    appended.step(Input::Open(base));
    for p in [&whole, &appended] {
        assert_eq!(p.log(), server.log());
        assert_eq!((p.foreign, p.faith), (None, false));
        assert_eq!(p.store.state_hash(), server.store.state_hash());
    }
}
