//! The client driver held through the simulator and the real hub: what it
//! keeps on its disk survives a reopen, pending intents included; a
//! sign-in authors what was pending again as the new login; a view is its
//! patches and a fresh evaluation through a rebase; the live room's
//! devices come and go as `on_join` and `on_leave`, and a pointer is
//! relayed coalesced.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ark::format::Encode;
use ark::log::{EntryId, Origin};
use ark::peer::{Ctx, Who};
use ark::row::Id;
use ark::schema::Space;
use ark_dev::demo::{arrival, live, room};
use ark_dev::demo::{AddToPlaylist, CreatePlaylist, Items, Point, Rename};
use ark_dev::sim::{who, Net, Op};
use ark_peer::{Config, Disk, Live, Memory, Module, Peer};

fn demo() -> Config {
    Config::of(Arc::new(ark_dev::demo::demo()))
}

fn net(n: i64) -> Net {
    Net::new(demo(), |_| demo(), n, 3)
}

/// `create_playlist`'s input.
fn named(name: &str) -> Vec<u8> {
    CreatePlaylist { name: name.into() }.to_bytes()
}

fn create(name: &str) -> Op {
    Op::Mutate(0, "demo.create_playlist".into(), named(name))
}

/// An intent authored offline survives a reopen: its entry, as it was,
/// pending on the disk, pushed on the next connection and logged. Falsified
/// by the client not writing its pending intents (`put("pending")` never
/// called): the reopened client had none and the playlist was never made.
#[test]
fn a_reopened_client_keeps_its_pending_intents() {
    let mut n = net(2);
    n.run(&Op::Partition(0));
    n.run(&create("Road"));
    let id = n.clients[&0].log.pending[0].entry.id;
    n.run(&Op::Reopen(0));
    assert_eq!(n.clients[&0].log.pending.iter().map(|p| p.entry.id).collect::<Vec<_>>(), vec![id]);
    n.settle().unwrap();
    n.converged().unwrap();
    assert_eq!(n.hub.log.log().entries().last().map(|l| l.entry.id), Some(id));
}

/// What was authored before signing in is the login's: its actor and
/// session are the new login's, its id is still its origin's (§1.6) — the
/// row it drew keeps its id — and the playlist is its user's. Falsified by
/// `sign_in` re-authoring the pending entries from a new origin whatever
/// the actor was: the id was another.
#[test]
fn a_sign_in_authors_what_was_pending_as_the_login() {
    let anon = Who {
        actor: "".into(),
        session: "anon".into(),
        roles: vec![],
    };
    let mut n = Net::of(demo(), vec![(anon, demo()), (who(1), demo())], 5);
    n.run(&Op::Partition(0));
    n.run(&create("Road"));
    let alice = Who {
        actor: "alice".into(),
        session: "alice-session".into(),
        roles: vec!["listener".into()],
    };
    n.run(&Op::SignIn(0, alice.clone()));
    n.settle().unwrap();
    n.converged().unwrap();
    assert!(n.refused.is_empty(), "{:?}", n.refused);
    let e = &n.hub.log.log().entries()[0].entry;
    assert_eq!(
        (e.actor.as_str(), e.session.as_str(), e.id, &e.roles),
        ("alice", "alice-session", EntryId::of(Origin::named("anon"), 1), &alice.roles)
    );
}

/// A view of a playlist's items, kept by a client whose own adds rebase
/// under another's: after every step its rows are its patches spliced and
/// a fresh evaluation. Falsified by the client not pushing a machine's
/// changes into its views (`changed` doing nothing): the view stayed empty.
#[test]
fn a_view_is_its_patches_and_a_fresh_evaluation() {
    let mut n = net(2);
    n.run(&create("Mix"));
    n.settle().unwrap();
    let pid = EntryId::of(Origin::named("peer-0-session"), 1).draw(0);
    let ctx = Ctx {
        draws: Default::default(),
        id: EntryId::default(),
        actor: "peer-0".into(),
        session: "peer-0-session".into(),
        roles: vec![],
        now: 0,
    };
    n.watch(0, Space::Durable, "demo.items", Items { playlist_id: Id::new(pid) }.to_bytes(), ctx)
        .unwrap();
    let add = |i: i64, t: &str| {
        let input = AddToPlaylist {
            playlist_id: Id::new(pid),
            track_id: t.into(),
        };
        Op::Mutate(i, "demo.add_to_playlist".into(), input.to_bytes())
    };
    for op in [Op::Partition(0), add(0, "a"), add(1, "b"), add(1, "c"), Op::Step, Op::Step, Op::Heal(0)] {
        n.run(&op);
        n.views_hold().unwrap();
    }
    for _ in 0..40 {
        n.run(&Op::Step);
        n.views_hold().unwrap();
    }
    n.settle().unwrap();
    n.views_hold().unwrap();
    assert_eq!(n.clients[&0].values(0).unwrap().len(), 3);
}

/// The room: two devices arrive and are here, by `on_join` the hub authored;
/// a rename is heard; three pointer moves inside one relay interval go up
/// as two calls; a device leaving is gone from every view. Falsified by the
/// client relaying every call at once: the hub's volatile chain held three
/// moves.
#[test]
fn the_room_comes_and_goes_and_the_pointer_is_relayed() {
    let r = room();
    let mut hub = demo();
    hub.live = Some(Live {
        on_join: Some("room.on_join".into()),
        on_leave: Some("room.on_leave".into()),
        arrive: std::sync::Arc::new(arrival),
        keep: Some(8),
        ..Live::new(Module::of(&r))
    });
    let config = |name: &str| Config {
        live: Some(live(name, 100)),
        ..demo()
    };
    let mut n = Net::of(hub, vec![(who(0), config("phone")), (who(1), config("laptop"))], 9);
    n.deliver();
    let ctx = Ctx {
        draws: Default::default(),
        id: EntryId::default(),
        actor: "".into(),
        session: "".into(),
        roles: vec![],
        now: 0,
    };
    n.watch(1, Space::Volatile, "room.present", vec![], ctx).unwrap();
    assert_eq!(n.clients[&1].values(0).unwrap().len(), 2, "both here");
    n.run(&Op::Call(0, "room.rename".into(), Rename { name: "kitchen".into() }.to_bytes()));
    for x in [1, 2, 3] {
        n.run(&Op::Call(0, "room.point".into(), Point { x }.to_bytes()));
    }
    n.run(&Op::Wait(100));
    n.deliver();
    n.views_hold().unwrap();
    let moves = n
        .hub
        .live
        .as_ref()
        .unwrap()
        .log()
        .entries()
        .iter()
        .filter(|l| l.entry.func == "room.point")
        .count();
    assert_eq!(moves, 2, "the second and third coalesced");
    n.converged().unwrap();
    n.run(&Op::Partition(0));
    n.deliver();
    n.views_hold().unwrap();
    assert_eq!(n.clients[&1].values(0).unwrap().len(), 1, "the phone has gone");
    assert!(n.refused.is_empty(), "{:?}", n.refused);
}

/// A memory disk that counts the bytes written to it.
struct Tally(Memory, Arc<AtomicUsize>);

impl Disk for Tally {
    fn put(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.1.fetch_add(b.len(), Ordering::Relaxed);
        self.0.put(n, b)
    }
    fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.1.fetch_add(b.len(), Ordering::Relaxed);
        self.0.append(n, b)
    }
    fn get(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
        self.0.get(n)
    }
}

/// A mutate offline writes its own intent, not every one pending: at
/// 8,000 pending the next costs the disk under 512 bytes (its entry, one
/// journal record), and what was written reads back on a reopen as the
/// same 8,001 pending intents in order; the reopen writes the journal
/// whole once, and the next mutate is one record again. Falsified by
/// writing the `pending` blob whole on every change, as before: the
/// 8,001st mutate wrote 871,554 bytes.
#[test]
fn a_mutate_writes_one_pending_record() {
    let written = Arc::new(AtomicUsize::new(0));
    let mut c = Peer::open(demo(), who(0), Box::new(Tally(Memory::default(), written.clone())), false);
    let create = "demo.create_playlist";
    let mut last = 0;
    for i in 0..=8000 {
        let before = written.load(Ordering::Relaxed);
        let _ = c.author(Space::Durable, create, named(&format!("List {i}")), i);
        last = written.load(Ordering::Relaxed) - before;
    }
    assert_eq!(c.log.pending.len(), 8001);
    assert!(last < 512, "the 8,001st mutate wrote {last} bytes");
    let ids: Vec<EntryId> = c.log.pending.iter().map(|p| p.entry.id).collect();
    let mut c = c.reopen();
    assert_eq!(c.log.pending.iter().map(|p| p.entry.id).collect::<Vec<_>>(), ids);
    let before = written.load(Ordering::Relaxed);
    let _ = c.author(Space::Durable, create, named("One more"), 9000);
    assert!(written.load(Ordering::Relaxed) - before < 512);
    let mut c = c.reopen();
    assert_eq!(c.log.pending.len(), 8002);
    assert_eq!(c.log.pending[8001].entry.local, 8002);
    let _ = c.author(Space::Durable, create, named("And one"), 9001);
    assert_eq!(c.log.pending.last().unwrap().entry.local, 8003, "the session's local read back");
}
