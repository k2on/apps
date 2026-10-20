//! The client driver held through the simulator and the real hub: what it
//! keeps on its disk survives a reopen, pending intents included; a
//! sign-in authors what was pending again as the new login; a view is its
//! patches and a fresh evaluation through a rebase; the live room's
//! devices come and go as `on_join` and `on_leave`, and a pointer is
//! relayed coalesced.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ark5::codec::bytes_enc;
use ark5::log::EntryId;
use ark5::peer::{Ctx, Who};
use ark5::schema::Space;
use ark5_client::storage::Stored;
use ark5_client::{Client, Config, Disk, Memory};
use ark5_dev::demo::{arrival, domain, live, module, room};
use ark5_dev::sim::{who, Net, Op};
use ark5_server::hub::{Config as HubConfig, Live as HubLive};

fn demo() -> Config {
    Config {
        log: module(&domain()),
        live: None,
    }
}

fn net(n: i64) -> Net {
    Net::new(HubConfig::new(module(&domain())), |_| demo(), n, 3)
}

fn create(name: &str) -> Op {
    Op::Mutate(0, domain().get("demo.create_playlist"), bytes_enc(name.as_bytes()))
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
    assert_eq!(n.hub.log.log.entries.last().map(|l| l.entry.id), Some(id));
}

/// What was authored before signing in is authored again as the login:
/// its actor, session and id are the new login's, and the playlist is its
/// user's. Falsified by `sign_in` leaving the pending entries as they
/// were: the hub refused them as another session's.
#[test]
fn a_sign_in_authors_what_was_pending_as_the_login() {
    let anon = Who {
        actor: "".into(),
        session: "anon".into(),
        roles: vec![],
    };
    let mut n = Net::of(HubConfig::new(module(&domain())), vec![(anon, demo()), (who(1), demo())], 5);
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
    let e = &n.hub.log.log.entries[0].entry;
    assert_eq!(
        (e.actor.as_str(), e.session.as_str(), e.id, &e.roles),
        ("alice", "alice-session", EntryId::of("alice-session", 1), &alice.roles)
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
    let pid = EntryId::of("peer-0-session", 1).draw(0);
    let ctx = Ctx {
        id: EntryId::default(),
        actor: "peer-0".into(),
        session: "peer-0-session".into(),
        roles: vec![],
        now: 0,
    };
    n.watch(0, Space::Durable, domain().get("demo.items"), bytes_enc(&pid), ctx).unwrap();
    let add = |i: i64, t: &str| {
        Op::Mutate(
            i,
            domain().get("demo.add_to_playlist"),
            [bytes_enc(&pid), bytes_enc(t.as_bytes())].concat(),
        )
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
    assert_eq!(n.clients[&0].rows(0).unwrap().len(), 3);
}

/// The room: two devices arrive and are here, by `on_join` the hub authored;
/// a rename is heard; three pointer moves inside one relay interval go up
/// as two calls; a device leaving is gone from every view. Falsified by the
/// client relaying every call at once: the hub's volatile chain held three
/// moves.
#[test]
fn the_room_comes_and_goes_and_the_pointer_is_relayed() {
    let r = room();
    let mut hub = HubConfig::new(module(&domain()));
    hub.live = Some(HubLive {
        module: module(&r),
        on_join: Some(r.get("room.on_join")),
        on_leave: Some(r.get("room.on_leave")),
        on_restore: None,
        device: std::sync::Arc::new(arrival),
        keep: 8,
    });
    let config = |name: &str| Config {
        log: module(&domain()),
        live: Some(live(name, 100)),
    };
    let mut n = Net::of(hub, vec![(who(0), config("phone")), (who(1), config("laptop"))], 9);
    n.deliver();
    let ctx = Ctx {
        id: EntryId::default(),
        actor: "".into(),
        session: "".into(),
        roles: vec![],
        now: 0,
    };
    n.watch(1, Space::Volatile, r.get("room.present"), vec![], ctx).unwrap();
    assert_eq!(n.clients[&1].rows(0).unwrap().len(), 2, "both here");
    n.run(&Op::Call(0, r.get("room.rename"), bytes_enc(b"kitchen")));
    for x in [1, 2, 3] {
        n.run(&Op::Call(0, r.get("room.point"), ark5::codec::int_enc(x).to_vec()));
    }
    n.run(&Op::Wait(100));
    n.deliver();
    n.views_hold().unwrap();
    let moves = n
        .hub
        .live
        .as_ref()
        .unwrap()
        .log
        .entries
        .iter()
        .filter(|l| l.entry.func == r.get("room.point"))
        .count();
    assert_eq!(moves, 2, "the second and third coalesced");
    n.converged().unwrap();
    n.run(&Op::Partition(0));
    n.deliver();
    n.views_hold().unwrap();
    assert_eq!(n.clients[&1].rows(0).unwrap().len(), 1, "the phone has gone");
    assert!(n.refused.is_empty(), "{:?}", n.refused);
}

/// A memory disk that counts the bytes written to it.
struct Tally(Memory, Arc<AtomicUsize>);

impl Disk for Tally {
    fn load(&mut self) -> io::Result<Stored> {
        self.0.load()
    }
    fn record(&mut self, b: &[u8]) -> io::Result<()> {
        self.1.fetch_add(b.len(), Ordering::Relaxed);
        self.0.record(b)
    }
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
    let mut c = Client::open(demo(), who(0), Box::new(Tally(Memory::default(), written.clone())), false);
    let create = domain().get("demo.create_playlist");
    let mut last = 0;
    for i in 0..=8000 {
        let before = written.load(Ordering::Relaxed);
        c.mutate(create, bytes_enc(format!("List {i}").as_bytes()), i);
        last = written.load(Ordering::Relaxed) - before;
    }
    assert_eq!(c.log.pending.len(), 8001);
    assert!(last < 512, "the 8,001st mutate wrote {last} bytes");
    let ids: Vec<EntryId> = c.log.pending.iter().map(|p| p.entry.id).collect();
    let mut c = c.reopen();
    assert_eq!(c.log.pending.iter().map(|p| p.entry.id).collect::<Vec<_>>(), ids);
    let before = written.load(Ordering::Relaxed);
    c.mutate(create, bytes_enc(b"One more"), 9000);
    assert!(written.load(Ordering::Relaxed) - before < 512);
    let mut c = c.reopen();
    assert_eq!(c.log.pending.len(), 8002);
    assert_eq!(c.log.pending[8001].entry.local, 8002);
    c.mutate(create, bytes_enc(b"And one"), 9001);
    assert_eq!(c.log.pending.last().unwrap().entry.local, 8003, "the session's local read back");
}
