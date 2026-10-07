//! §12.4 A `Hello` whose cursor is past the authority's head
//! (`docs/plan-perf.md` R6): a peer that confirmed thirty entries meets a
//! server that has ten — restarted over an older or emptied data
//! directory. It is answered as a peer below the horizon is, with the
//! authority's store as the snapshot at the head; it re-opens from it with
//! its pending intents on top, and the intents it pushed after its `Hello`
//! are sequenced after the ten, acknowledged or refused, and confirmed.
//!
//! And the same answer wherever the cursor is, when the `Hello` names
//! another log (Round 4): a server that lost its log and has sequenced
//! forty entries of a new one since meets a peer confirmed to thirty of the
//! old — a cursor below the head, which says nothing about whose entries
//! those thirty were. Sans-io: the two state machines and the frames
//! between them, nothing else.

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
        // One pump: what the round's frames placed is applied once (R8).
        c.settle();
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
    // it), and then by the authority's verdict, which arrives for an
    // intent no longer pending: one intent, one reason (Round 4). Reported
    // for every verdict, as `Replica::reject` did before: two.
    assert_eq!(refused, [(key(8, 3), "playlist_id: no such playlist".to_string())]);

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

/// Round 4, the lost log that went on: thirty entries of log A confirmed
/// at the peer, which learned A's name from its first page; three intents
/// pending, authored offline — two onto the playlist both logs have and
/// one making a playlist. The server that comes back has log B, forty
/// entries long: the first ten of A's again, then thirty items of its own
/// onto the same playlist. The peer's `Hello` says 30 and A; B's server
/// answers it with `SnapshotOf` at 40, named B, before anything else
/// (not a `Batch` of 31 to 40, which would land B's entries on A's state);
/// the three are acknowledged at 41, 42 and 43; and the peer ends at 43 of
/// B, its confirmed store the server's hash for hash, nothing pending, its
/// view its confirmed store, and its own item after B's thirty-nine — the
/// rebase, onto the log there is. Falsified by `Server::recv` never
/// finding a `Hello` elsewhere (`elsewhere` always false): the first frame
/// is a `Batch` from 31, and the peer's hash is not the server's.
#[test]
fn a_cursor_on_another_log_is_re_based_onto_this_one() {
    let m = Module::new((lists(),));
    let built = m.build();
    let (sch, bodies) = (built.schema.clone(), ark::hash::closures(built));
    let (create, _) = m.procedure("create_playlist").unwrap();
    let (add, _) = m.procedure("add_to_playlist").unwrap();
    let alice = Ctx::new("alice", "dev");
    let (log_a, log_b) = (key(0xa, 0xa), key(0xb, 0xb));
    let p1 = Value::Id(key(1, 1));
    let item = |k: u8, n: u8, track: String| (key(k, n), args([]), args([("playlist_id", p1.clone()), ("track_id", Value::text(track))]));

    // Every entry authored by one replica and sequenced by an authority
    // over the same state, as a server would: a playlist, then items.
    let log_of = |name, items: Vec<(u8, u8, String)>| {
        let mut a = Authority::new(sch.clone(), bodies.clone());
        a.log.name_if_unnamed(name);
        let mut r = Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]);
        let e = r
            .mutate(
                key(9, 0),
                &alice,
                &create,
                &args([("id", p1.clone())]),
                &args([("name", Value::text("One"))]),
            )
            .unwrap();
        assert!(matches!(a.sequence_entry(&e), Sequenced::Appended(1, _)));
        for (k, n, t) in items {
            let (id, autos, xs) = item(k, n, t);
            let e = r.mutate(id, &alice, &add, &autos, &xs).unwrap();
            assert!(matches!(a.sequence_entry(&e), Sequenced::Appended(..)));
        }
        a
    };
    let first_nine: Vec<(u8, u8, String)> = (2..=10u8).map(|n| (9, n, format!("t{n}"))).collect();
    let mut of_a = first_nine.clone();
    of_a.extend((11..=30u8).map(|n| (9, n, format!("a{n}"))));
    let mut of_b = first_nine;
    of_b.extend((11..=40u8).map(|n| (7, n, format!("b{n}"))));
    let (a, b) = (log_of(log_a, of_a), log_of(log_b, of_b));
    assert_eq!((a.log.head_seq(), b.log.head_seq()), (30, 40));

    // The peer, synced to 30 of A, then offline with three intents.
    let mut peer = Client::open(
        Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]),
        Mode::Whole,
        Some("alice".into()),
    );
    let mut was = Server::open(trusting(), open_access(), Silent, a);
    peer.connected();
    exchange(&mut peer, &mut was);
    assert_eq!(
        (peer.replica.cursor, peer.replica.log_id),
        (30, Some(log_a)),
        "the first page named the log"
    );
    peer.disconnected();
    let p2 = Value::Id(key(1, 2));
    let offline = [
        item(8, 0, "x".into()),
        (key(8, 1), args([("id", p2.clone())]), args([("name", Value::text("Two"))])),
        item(8, 2, "y".into()),
    ];
    for (k, (id, autos, xs)) in offline.into_iter().enumerate() {
        let fh = if k == 1 { &create } else { &add };
        peer.mutate(id, &alice, fh, &autos, &xs).unwrap();
    }

    let mut sv = Server::open(trusting(), open_access(), Silent, b);
    peer.connected();
    let heard = exchange(&mut peer, &mut sv);
    assert!(
        matches!(heard.first(), Some(ServerMsg::SnapshotOf { seq: 40, log_id, .. }) if *log_id == Some(log_b)),
        "the Hello is answered with B at its head: {:?}",
        heard.first()
    );
    let acked: Vec<i64> = heard
        .iter()
        .flat_map(|f| match f {
            ServerMsg::Ack { seqs, .. } => seqs.clone(),
            _ => vec![],
        })
        .collect();
    assert_eq!(acked, [41, 42, 43], "{heard:?}");
    assert!(peer.replica.rejections.is_empty(), "{:?}", peer.replica.rejections);
    assert_eq!((peer.replica.cursor, peer.replica.log_id), (43, Some(log_b)));
    assert_eq!(state_hash(&peer.replica.confirmed), state_hash(&sv.authority.store));
    assert!(peer.replica.pending.is_empty());
    assert_eq!(peer.replica.view, peer.replica.confirmed);
    let pos = |t: &str| {
        peer.replica
            .confirmed
            .get("item", &[p1.clone(), Value::text(t)])
            .map(|r| r["pos"].as_int())
    };
    assert_eq!((pos("b40"), pos("x"), pos("a30")), (Some(39), Some(40), None));
}

/// Round 4: a peer that names no log — opened from storage written before
/// logs had names — is served as it always was, and learns the name from
/// the first page it is sent; a server whose log is unnamed names none.
/// Falsified by the client ignoring a page's `log` (`r.log_id` left
/// alone): still `None` after the page.
#[test]
fn a_peer_that_names_no_log_is_served_as_before_and_learns_it() {
    let m = Module::new((lists(),));
    let built = m.build();
    let (sch, bodies) = (built.schema.clone(), ark::hash::closures(built));
    let (create, _) = m.procedure("create_playlist").unwrap();
    let alice = Ctx::new("alice", "dev");
    for named in [Some(key(0xc, 0xc)), None] {
        let mut a = Authority::new(sch.clone(), bodies.clone());
        if let Some(n) = named {
            a.log.name_if_unnamed(n);
        }
        let mut r = Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]);
        for n in 1..=3u8 {
            let e = r
                .mutate(
                    key(9, n),
                    &alice,
                    &create,
                    &args([("id", Value::Id(key(1, n)))]),
                    &args([("name", Value::text(format!("L{n}")))]),
                )
                .unwrap();
            assert!(matches!(a.sequence_entry(&e), Sequenced::Appended(..)));
        }
        let mut sv = Server::open(trusting(), open_access(), Silent, a);
        let mut c = Client::open(
            Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]),
            Mode::Whole,
            Some("alice".into()),
        );
        c.connected();
        let heard = exchange(&mut c, &mut sv);
        assert!(matches!(heard.first(), Some(ServerMsg::Batch { .. })), "{named:?}: {heard:?}");
        assert_eq!((c.replica.cursor, c.replica.log_id), (3, named));
    }
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
        held: None,
        seq: 5,
        hash: state_hash(&empty),
        rows: Default::default(),
        log_id: None,
        module: None,
    });
    let told: Vec<_> = c.replica.rejections.iter().map(|(i, _)| *i).collect();
    assert_eq!(told, [key(8, 1)]);
    assert_eq!((c.replica.cursor, c.replica.pending.len()), (5, 1));
}
