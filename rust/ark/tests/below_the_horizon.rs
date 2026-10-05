//! §12.4 A peer below the horizon of a quiet server (`docs/plan-perf.md`
//! R10): the server has compacted, nobody else is saying anything, and a
//! peer at cursor 0 with nothing pending says `Hello`. It is sent the
//! snapshot *and* the first page above it in that one turn, so it reaches
//! the head with no further frame of its own — it has none to send: a
//! client asks again only after a `Batch` with `has_more`. Sans-io: the two
//! state machines and the frames between them, nothing else.

use ark::authoring::*;
use ark::eval::{Args, Ctx};
use ark::hash::state_hash;
use ark::live::Silent;
use ark::peer::{Authority, Replica, Sequenced};
use ark::protocol::{open_access, trusting, Client, Mode, Server, ServerMsg};
use ark::store::MemoryStore;
use ark::value::Value;

pub struct Lists {
    pub playlist: Table<Playlist>,
}
impl Tables for Lists {
    fn open() -> Self {
        Lists { playlist: table() }
    }
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns().id(Self::id).text(Self::name).text(Self::user_id).key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Create {
    pub name: Text,
}
impl Input for Create {
    fn schema() -> Object<Self> {
        object().field("name", text().min(1))
    }
}

fn lists() -> Router<Lists> {
    let r = router::<Lists>("lists");
    r.routes((r.input::<Create>().mutation("create_playlist", |ctx, db, input| {
        db.playlist.insert(Playlist {
            id: ctx.new_id("id"),
            name: input.name,
            user_id: ctx.user,
        })
    }),))
}

fn key(k: u8, n: u16) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = k;
    b[14..].copy_from_slice(&n.to_be_bytes());
    b
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Four hundred entries, compacted to 300; a fresh peer's `Hello` is
/// answered, in the one turn it prompts, with `SnapshotOf` at 300 and a
/// `Batch` of the hundred above it, and the peer is at the head with the
/// server's state and nothing more to say. Falsified by sending one
/// message per connection per turn again (`fanout` not following a
/// snapshot): the turn carries the snapshot alone and the peer stops at
/// 300.
#[test]
fn a_peer_below_the_horizon_of_a_quiet_server_reaches_the_head() {
    let m = Module::new((lists(),));
    let built = m.build();
    let (sch, bodies) = (built.schema.clone(), ark::hash::closures(built));
    let (create, _) = m.procedure("create_playlist").unwrap();
    let alice = Ctx::new("alice", "dev");

    let mut a = Authority::new(sch.clone(), bodies.clone());
    let mut author = Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]);
    for n in 1..=400u16 {
        let e = author
            .mutate(
                key(9, n),
                &alice,
                &create,
                &args([("id", Value::Id(key(1, n)))]),
                &args([("name", Value::text(format!("p{n}")))]),
            )
            .unwrap();
        assert!(matches!(a.sequence_entry(&e), Sequenced::Appended(..)));
    }
    assert!(a.compact(300));
    let head_hash = state_hash(&a.store);
    let mut sv = Server::open(trusting(), open_access(), Silent, a);

    let mut peer = Client::open(
        Replica::open(sch.clone(), bodies, MemoryStore::empty(sch), 0, vec![]),
        Mode::Whole,
        Some("alice".into()),
    );
    peer.connected();
    let hello = peer.take_outgoing();
    assert_eq!(hello.len(), 1, "a Hello and nothing pending");
    for f in hello {
        sv.recv(1, f);
    }
    let turn: Vec<ServerMsg> = sv.take_outgoing().into_iter().map(|(_, f)| f).collect();
    assert!(
        matches!(
            turn.as_slice(),
            [ServerMsg::SnapshotOf { seq: 300, .. }, ServerMsg::Batch { items, has_more: false, .. }]
                if items.first().map(|(n, _, _)| *n) == Some(301) && items.len() == 100
        ),
        "the snapshot and the page above it: {:?}",
        turn.iter()
            .map(|f| format!("{f:?}").chars().take(60).collect::<String>())
            .collect::<Vec<_>>()
    );
    for f in turn {
        peer.recv(f);
    }
    peer.settle();
    assert_eq!(peer.replica.cursor, 400, "at the head");
    assert_eq!(state_hash(&peer.replica.confirmed), head_hash);
    assert!(peer.take_outgoing().is_empty(), "with nothing more to say");
}

/// `docs/plan-db.md` D7.4 The snapshot frame as a client reads it off the
/// socket — its rows built as they are decoded (`ServerMsg::decode_for`,
/// `Client::recv_snapshot`) — adopts exactly the store the frame decoded
/// whole made: the frame's value, `ServerMsg::from_value`, and each struct
/// in it laid out as the adoption loop always laid it out, written out
/// here as it was. Over the rows that try it: one of the table's exactly,
/// one with a column the table lacks and one without a column it requires
/// (both kept as they came), one that is not a struct (dropped), and one
/// of a table this schema has not got; and again behind (D1), where each
/// is projected. The same frames are refused, with the same words: a row's
/// keys out of order, a key twice, a table that is not a list, rows that
/// are not a struct, a frame cut short, bytes after it.
///
/// Falsified twice: by `canon::decode_rows` dropping a table whose value
/// is not a list rather than keeping it (the new read took the frame the
/// whole read refused with "expected a list"), and by reading a row's
/// fields without `each_pair`'s key checks (the unsorted and the
/// duplicated keys taken).
#[test]
fn a_snapshot_read_as_it_is_decoded_is_the_snapshot_read_whole() {
    use ark::canon;
    use ark::protocol::{Received, Snapshot};
    use ark::store::{project_row, Change, Row, Store};
    use std::collections::BTreeMap;

    let m = Module::new((lists(),));
    let built = m.build();
    let (sch, bodies) = (built.schema.clone(), ark::hash::closures(built));
    let strct = |pairs: Vec<(&str, Value)>| Value::record(pairs);
    let playlist = |n: u16| {
        vec![
            ("id", Value::Id(key(1, n))),
            ("name", Value::text(format!("p{n}"))),
            ("user_id", Value::text("alice")),
        ]
    };
    let mut wider = playlist(2);
    wider.push(("x", Value::Int(1)));
    let mut narrower = playlist(3);
    narrower.retain(|(k, _)| *k != "user_id");
    let rows: BTreeMap<String, Vec<Value>> = [
        (
            "playlist".to_string(),
            vec![strct(playlist(1)), strct(wider), Value::Int(7), strct(narrower), strct(playlist(4))],
        ),
        ("ghost".to_string(), vec![strct(vec![("id", Value::Int(1))])]),
    ]
    .into();
    let frame = |module: Option<Vec<u8>>| ServerMsg::SnapshotOf {
        seq: 5,
        hash: vec![0xcd; 32],
        rows: rows.clone(),
        log_id: Some(key(0xa1, 0)),
        module,
        partial: false,
    };
    // The adoption loop as it stood before D7.4, over the whole tree.
    let old_store = |behind: bool| {
        let mut st = MemoryStore::empty(sch.clone());
        for (t, vs) in &rows {
            let tbl = sch.lookup_table(t);
            for v in vs {
                if let Value::Struct(row) = v {
                    let row = match tbl {
                        None => Row::from_struct_ref(row),
                        Some(tbl) if behind => {
                            let raw = Row::from_struct_ref(row);
                            project_row(tbl, &raw).unwrap_or(raw)
                        }
                        Some(tbl) => Row::stored_in(tbl, row),
                    };
                    st.apply_change(&Change::Add(t.clone(), row));
                }
            }
        }
        st
    };
    let client = |behind: bool| {
        let mut c = Client::open(
            Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]),
            Mode::ByFacts,
            None,
        );
        c.module = behind.then(|| vec![1]);
        c
    };
    for behind in [false, true] {
        let msg = frame(Some(vec![2]));
        let bytes = canon::encode(&msg.to_value());
        let mut whole = client(behind);
        whole.recv(ServerMsg::from_value(&canon::decode(&bytes).unwrap()).unwrap());
        let mut read = client(behind);
        let Ok(Received::Snapshot(s)) = ServerMsg::decode_for(&bytes, &sch) else {
            panic!("a snapshot")
        };
        let ServerMsg::SnapshotOf {
            seq,
            rows,
            log_id,
            module,
            partial,
            ..
        } = msg
        else {
            unreachable!()
        };
        assert_eq!(
            s,
            Snapshot::of_values(&sch, seq, rows, log_id, module, partial),
            "the rows built as the values make them"
        );
        read.recv_snapshot(s);
        assert_eq!(read.replica.behind, behind);
        let want = old_store(behind);
        assert_eq!(want.scan("playlist").len(), 4, "every struct a row, and what is not one dropped");
        assert_eq!(whole.replica.confirmed, want, "behind {behind}: whole");
        assert_eq!(read.replica.confirmed, want, "behind {behind}: as decoded");
        assert_eq!(read.replica.view, want);
        assert_eq!((read.replica.cursor, read.replica.log_id), (5, Some(key(0xa1, 0))));
        assert_eq!(state_hash(&read.replica.confirmed), state_hash(&whole.replica.confirmed));
    }

    // The refusals: the words the whole read gave, and nothing taken.
    let good = canon::encode(&frame(None).to_value());
    let whole = |b: &[u8]| -> Result<(), String> {
        let v = canon::decode(b).map_err(|e| e.to_string())?;
        ServerMsg::from_value(&v).map(|_| ()).map_err(|e| e.to_string())
    };
    // A frame whose one playlist row is `pairs`, in the order given and
    // encoded by hand: no encoder writes a struct's keys out of order.
    let by_hand = |pairs: &[(&str, Value)]| {
        let mut row = vec![0xa0 + pairs.len() as u8];
        for (k, v) in pairs {
            row.extend(canon::encode(&Value::text(*k)));
            row.extend(canon::encode(v));
        }
        let Value::Struct(mut fs) = frame(None).to_value() else { unreachable!() };
        fs.insert("rows".into(), strct(vec![("playlist", Value::list(vec![Value::Int(0)]))]));
        let b = canon::encode(&Value::Struct(fs));
        // The one element, `0x00`, after the list's head, `0x81`.
        let at = b.windows(2).position(|w| w == [0x81, 0x00]).unwrap();
        [&b[..at + 1], &row[..], &b[at + 2..]].concat()
    };
    let with_rows = |r: Value| {
        let Value::Struct(mut fs) = frame(None).to_value() else { unreachable!() };
        fs.insert("rows".into(), r);
        canon::encode(&Value::Struct(fs))
    };
    let mut cut = good.clone();
    cut.pop();
    let mut trailing = good.clone();
    trailing.push(0);
    let bad = [
        ("unsorted", by_hand(&[("nm", Value::Int(1)), ("id", Value::Id(key(1, 1)))])),
        ("duplicate", by_hand(&[("id", Value::Id(key(1, 1))), ("id", Value::Id(key(1, 1)))])),
        ("not a list", with_rows(strct(vec![("playlist", Value::Int(1))]))),
        ("not a struct", with_rows(Value::list(vec![]))),
        ("cut short", cut),
        ("trailing", trailing),
    ];
    for (what, b) in bad {
        let before = whole(&b);
        assert!(before.is_err(), "{what}: refused whole");
        assert_eq!(
            ServerMsg::decode_for(&b, &sch).map(|_| ()),
            before,
            "{what}: refused as decoded, in the same words"
        );
    }
}
