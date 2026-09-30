//! §12.4 A `Hello` whose cursor is past the authority's head
//! (`docs/plan-perf.md` R6): a peer that confirmed thirty entries meets a
//! server that has ten — restarted over an older or emptied data
//! directory. It is answered as a peer below the horizon is, with the
//! authority's store as the snapshot at the head; it re-opens from it with
//! its pending intents on top, and the intents it pushed after its `Hello`
//! are sequenced after the ten, acknowledged or refused, and confirmed.
//! Sans-io: the two state machines and the frames between them, nothing
//! else.

use ark::authoring::*;
use ark::eval::{Args, Ctx};
use ark::hash::state_hash;
use ark::live::Silent;
use ark::peer::{Authority, Replica, Sequenced};
use ark::protocol::{open_access, trusting, Client, Mode, Server, ServerMsg};
use ark::store::{MemoryStore, Store};
use ark::value::Value;

pub struct Lists {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}
impl Tables for Lists {
    fn open() -> Self {
        Lists {
            playlist: table(),
            item: table(),
        }
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

pub struct Item {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
    pub pos: Int,
}
impl Row for Item {
    const NAME: &str = "item";
    type Key = (Id<Playlist>, Text);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .text(Self::track_id)
            .int(Self::pos)
            .key((Self::playlist_id, Self::track_id))
            .index((Self::playlist_id, Self::pos))
    }
}
#[allow(non_upper_case_globals)]
impl Item {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const track_id: Col<Self, Text> = col("track_id");
    pub const pos: Col<Self, Int> = col("pos");
}

pub struct Create {
    pub name: Text,
}
impl Input for Create {
    fn schema() -> Object<Self> {
        object().field("name", text().min(1))
    }
}

pub struct Add {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
}
impl Input for Add {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
    }
}

fn lists() -> Router<Lists> {
    let r = router::<Lists>("lists");
    r.routes((
        r.input::<Create>().mutation("create_playlist", |ctx, db, input| {
            db.playlist.insert(Playlist {
                id: ctx.new_id("id"),
                name: input.name,
                user_id: ctx.user,
            })
        }),
        r.input::<Add>().mutation("add_to_playlist", |_ctx, db, input| {
            let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
            db.item.insert(Item {
                playlist_id: input.playlist_id,
                track_id: input.track_id,
                pos: last.map_or(0, |row| row.pos).add(1),
            })
        }),
    ))
}

fn key(k: u8, n: u8) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = k;
    b[15] = n;
    b
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Frames both ways until neither side has anything to say; what the
/// server sent, in order.
fn exchange(c: &mut Client, sv: &mut Server<Silent>) -> Vec<ServerMsg> {
    let mut heard = vec![];
    loop {
        let up = c.take_outgoing();
        for f in up.iter().cloned() {
            sv.recv(1, f);
        }
        let down: Vec<ServerMsg> = sv.take_outgoing().into_iter().map(|(_, f)| f).collect();
        if up.is_empty() && down.is_empty() {
            return heard;
        }
        for f in down {
            heard.push(f.clone());
            c.recv(f);
        }
    }
}

/// Thirty entries confirmed at the peer, ten at the server; four intents
/// pending at the peer — two onto the playlist both have, one making a
/// playlist, one onto a playlist that only the lost twenty made. The
/// server answers the `Hello` with `SnapshotOf` at 10 (not a `Batch`, and
/// not nothing); the three it can apply are acknowledged at 11, 12 and 13
/// and the fourth refused with its reason; the peer ends at 13, its
/// confirmed store the server's hash for hash, nothing pending, its view
/// equal to its confirmed store, and the refusal in its rejections.
/// (A verdict the peer had not yet taken before the snapshot is kept
/// through it; `a_verdict_not_yet_taken_survives_a_snapshot`.) The
/// two items land after the nine the server has — the rebase, onto the
/// log there is. Falsified by serving a cursor past the head nothing, as
/// before (`sent >= head` in `Server::fanout`): no `SnapshotOf` is sent,
/// and the three acknowledgements name sequences no page ever brings to a
/// peer that believes it is at 30.
#[test]
fn a_cursor_past_the_head_is_re_based_onto_the_head() {
    let m = Module::new((lists(),));
    let built = m.build();
    let (sch, bodies) = (built.schema.clone(), ark::hash::closures(built));
    let (create, _) = m.procedure("create_playlist").unwrap();
    let (add, _) = m.procedure("add_to_playlist").unwrap();
    let alice = Ctx::new("alice", "dev");
    let (p1, p0, p2) = (Value::Id(key(1, 1)), Value::Id(key(1, 0)), Value::Id(key(1, 2)));

    // The log as it was: a playlist, eighteen items, a second playlist at
    // 20, and ten more items onto the first — thirty in all.
    let mut old = Authority::new(sch.clone(), bodies.clone());
    let mut author = Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]);
    let mut entries = vec![];
    for n in 1..=30u8 {
        let e = match n {
            1 => author.mutate(
                key(9, n),
                &alice,
                &create,
                &args([("id", p1.clone())]),
                &args([("name", Value::text("One"))]),
            ),
            20 => author.mutate(
                key(9, n),
                &alice,
                &create,
                &args([("id", p0.clone())]),
                &args([("name", Value::text("Lost"))]),
            ),
            _ => author.mutate(
                key(9, n),
                &alice,
                &add,
                &args([]),
                &args([("playlist_id", p1.clone()), ("track_id", Value::text(format!("t{n}")))]),
            ),
        }
        .unwrap();
        assert!(matches!(old.sequence_entry(&e), Sequenced::Appended(s, _) if s == n as i64));
        entries.push(e);
    }

    // The peer, synced to 30 against the old server, then offline.
    let mut peer = Client::open(
        Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]),
        Mode::Whole,
        Some("alice".into()),
    );
    let mut was = Server::open(trusting(), open_access(), Silent, old);
    peer.connected();
    exchange(&mut peer, &mut was);
    assert_eq!(peer.replica.cursor, 30);
    peer.disconnected();
    let offline = [
        (add.clone(), args([]), args([("playlist_id", p1.clone()), ("track_id", Value::text("x"))])),
        (create.clone(), args([("id", p2.clone())]), args([("name", Value::text("Two"))])),
        (add.clone(), args([]), args([("playlist_id", p2.clone()), ("track_id", Value::text("y"))])),
        (add.clone(), args([]), args([("playlist_id", p0.clone()), ("track_id", Value::text("z"))])),
    ];
    for (k, (fh, autos, a)) in offline.iter().enumerate() {
        peer.mutate(key(8, k as u8), &alice, fh, autos, a).unwrap();
    }
    assert_eq!(peer.replica.pending.len(), 4);

    // The server that came back with the first ten.
    let mut new = Authority::new(sch.clone(), bodies.clone());
    for e in &entries[..10] {
        assert!(matches!(new.sequence_entry(e), Sequenced::Appended(..)));
    }
    let mut sv = Server::open(trusting(), open_access(), Silent, new);
    peer.connected();
    let heard = exchange(&mut peer, &mut sv);

    let snapshot = heard.iter().position(|f| matches!(f, ServerMsg::SnapshotOf { seq: 10, .. }));
    assert_eq!(snapshot, Some(0), "the Hello is answered with the head: {heard:?}");
    let acked: Vec<i64> = heard
        .iter()
        .flat_map(|f| match f {
            ServerMsg::Ack { seqs, .. } => seqs.clone(),
            _ => vec![],
        })
        .collect();
    assert_eq!(acked, [11, 12, 13], "{heard:?}");
    let refused: Vec<(ark::value::Id, String)> = peer
        .replica
        .rejections
        .iter()
        .map(|(i, r)| (*i, ark::protocol::refusal_text(r)))
        .collect();
    // Refused by the replay the snapshot opens (the playlist is not in
    // it) and again by the authority's verdict: one intent, one reason.
    assert!(!refused.is_empty());
    assert!(
        refused.iter().all(|(i, why)| *i == key(8, 3) && why == "playlist_id: no such playlist"),
        "{refused:?}"
    );

    let head = sv.authority.log.head_seq();
    assert_eq!((peer.replica.cursor, head), (13, 13));
    assert_eq!(state_hash(&peer.replica.confirmed), state_hash(&sv.authority.store));
    assert!(peer.replica.pending.is_empty());
    assert_eq!(peer.replica.view, peer.replica.confirmed);
    let pos = |t: &str| {
        peer.replica
            .confirmed
            .get("item", &[p1.clone(), Value::text(t)])
            .map(|r| r["pos"].as_int())
    };
    assert_eq!((pos("t10"), pos("x"), pos("t29")), (Some(9), Some(10), None));
}

/// A snapshot replaces what is confirmed and not what the peer was told:
/// a verdict it had not yet taken when a `SnapshotOf` arrived — which a
/// past-the-head `Hello` now makes an ordinary thing to receive — is still
/// there to take after it. Falsified by re-opening without carrying the
/// old replica's rejections over (`told` left out): none.
#[test]
fn a_verdict_not_yet_taken_survives_a_snapshot() {
    let m = Module::new((lists(),));
    let built = m.build();
    let (sch, bodies) = (built.schema.clone(), ark::hash::closures(built));
    let (create, _) = m.procedure("create_playlist").unwrap();
    let alice = Ctx::new("alice", "dev");
    let mut c = Client::open(
        Replica::open(sch.clone(), bodies, MemoryStore::empty(sch.clone()), 0, vec![]),
        Mode::Whole,
        Some("alice".into()),
    );
    let made = |n: u8| (args([("id", Value::Id(key(1, n)))]), args([("name", Value::text(format!("L{n}")))]));
    for n in 1..=2 {
        let (autos, a) = made(n);
        c.mutate(key(8, n), &alice, &create, &autos, &a).unwrap();
    }
    c.recv(ServerMsg::Reject {
        id: key(8, 1),
        reason: "no".into(),
    });
    let empty = MemoryStore::empty(sch);
    c.recv(ServerMsg::SnapshotOf {
        seq: 5,
        hash: state_hash(&empty),
        rows: Default::default(),
    });
    let told: Vec<_> = c.replica.rejections.iter().map(|(i, _)| *i).collect();
    assert_eq!(told, [key(8, 1)]);
    assert_eq!((c.replica.cursor, c.replica.pending.len()), (5, 1));
}
