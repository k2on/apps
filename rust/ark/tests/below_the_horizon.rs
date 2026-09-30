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
