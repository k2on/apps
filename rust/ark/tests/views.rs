//! §1.10 maintained views told a replica's changes, and §1.8 a scoped
//! feed. A view told a replica's `Output::Changes` holds through a rebase,
//! and a replica fed by a scope holds exactly its union after every
//! delivery. The `views/` vectors — a named view of a reference domain over
//! a store and batches of changes — are `ark-dev`'s (`tests/views.rs`).

#[path = "demo.rs"]
mod demo;

use ark::format::Encode;
use ark::hash::hex;
use ark::log::{EntryId, Origin};
use ark::peer::{Ctx, Feed, Input, Journal, Output, Peer, Role, Who};
use ark::plan::{splice, Maintained};
use ark::prelude::{scan, Db, Desc, Id, Plan, Refusal};
use ark::store::{Change, Store};
use ark::Domain;
use demo::*;

fn ctx() -> Ctx {
    Ctx {
        actor: "alice".into(),
        session: "dev".into(),
        ..Default::default()
    }
}

/// Every playlist by name, with its last three items, last first.
type WithItems = (Id<Playlist>, String, Vec<Item>);

fn with_items(_: &Ctx, _: &Db<Demo>, _: ()) -> Result<Plan<Demo, WithItems>, Refusal> {
    Ok(scan::<Demo, Playlist>().sort(|p| (p.name.clone(), p.id)).map_with(|db, p| {
        let mut items = db.item.by(Item::KEY).from(p.id).all();
        items.sort_by_key(|i| std::cmp::Reverse(i.pos));
        items.truncate(3);
        (p.id, p.name.clone(), items)
    }))
}

/// A playlist's first two items by place.
fn top_two(_: &Ctx, _: &Db<Demo>, input: Items) -> Result<Plan<Demo, Item>, Refusal> {
    let p = input.playlist_id;
    Ok(scan::<Demo, Item>()
        .filter(move |i| i.playlist_id == p)
        .sort(|i| (Desc(i.pos), i.track_id.clone()))
        .take(2))
}

fn views() -> Domain {
    Domain::new::<Demo>("views", "1")
        .view("views.with_items", with_items)
        .view("views.top_two", top_two)
}

/// The contract after one push: the view's rows are its answer afresh
/// and a view made again's, and the patches splice the rows before into
/// them.
fn holds(d: &Domain, name: &str, input: &[u8], v: &dyn Maintained, store: &Store, before: &[Vec<u8>], patches: &[ark::plan::Patch<Vec<u8>>]) {
    let json = |rows: Vec<Vec<u8>>| rows.iter().map(|r| d.element_json(name, r).unwrap()).collect::<Vec<_>>().join(",");
    let fresh = d.answer_json(name, &d.ask(store, &ctx(), name, input).unwrap()).unwrap();
    assert_eq!(format!("[{}]", json(v.rows())), fresh, "{name}: the view is its answer afresh");
    assert_eq!(
        d.watch(store, &ctx(), name, input).unwrap().rows(),
        v.rows(),
        "{name}: and a view made again"
    );
    assert_eq!(splice(patches, before), v.rows(), "{name}: the patches splice");
}

// * A view told a replica's changes

/// Views over a replica's store, told nothing but its `Output::Changes`,
/// hold through a rebase: alice previews tracks offline, bob's land under
/// them — hers undone, his applied, hers redone — and after every step
/// each view is its answer afresh and its patches splice. One view reads
/// the items in a closure (`with_items`), one scans them on its spine
/// (`top_two`, last place first). Falsified by the view taking each
/// key's first change as its net (`Pushed::of` keeping a key's first
/// move): alice's tracks, undone and redone at new places, left
/// `top_two`, which then held one of bob's where it should hold hers.
#[test]
fn a_view_follows_a_replica_through_a_rebase() {
    let mut server = Peer::new(
        demo::shared(),
        demo::cas(),
        Role::Authority { journal: Journal::default() },
        who_of("server"),
    );
    let (mut alice, mut bob) = (replica_of("alice"), replica_of("bob"));
    let pid = EntryId::of(Origin::named("alice-session"), 1).draw(0);
    let d = views();
    let queries = [
        ("views.with_items", ().to_bytes()),
        ("views.top_two", Items { playlist_id: Id::new(pid) }.to_bytes()),
    ];
    let mut views: Vec<(&str, Vec<u8>, Box<dyn Maintained>)> = queries
        .into_iter()
        .map(|(name, input)| {
            let v = d.watch(&alice.store, &ctx(), name, &input).unwrap();
            (name, input, v)
        })
        .collect();
    let told = std::cell::Cell::new(0);
    let mut tell = |alice: &mut Peer, input: Input| -> Vec<Output> {
        let o = alice.step(input);
        let cs: Vec<Change> = o
            .iter()
            .flat_map(|x| if let Output::Changes(cs) = x { cs.clone() } else { vec![] })
            .collect();
        told.set(told.get() + cs.len());
        for (name, input, v) in views.iter_mut() {
            let was = v.rows();
            let patches = v.push(&cs, &alice.store).unwrap();
            holds(&d, name, input, v.as_ref(), &alice.store, &was, &patches);
        }
        o
    };
    let up = |server: &mut Peer, w: &str, o: &[Output]| -> Vec<Vec<u8>> {
        let mut down = vec![];
        for b in o.iter().filter_map(|x| if let Output::Send(b) = x { Some(b.clone()) } else { None }) {
            for r in server.step(Input::Frame {
                from: Some(who_of(w)),
                bytes: b,
            }) {
                if let Output::Fanout(b) = r {
                    down.push(b);
                }
            }
        }
        down
    };
    let o = tell(&mut alice, mutate_input(CREATE, create_input("Mix")));
    for b in up(&mut server, "alice", &o) {
        tell(
            &mut alice,
            Input::Frame {
                from: None,
                bytes: b.clone(),
            },
        );
        bob.step(Input::Frame { from: None, bytes: b });
    }
    tell(&mut alice, mutate_input(ADD, pair_input(&pid, "a1")));
    tell(&mut alice, mutate_input(ADD, pair_input(&pid, "a2")));
    let mut landed = vec![];
    for t in ["b1", "b2", "b3"] {
        let o = bob.step(mutate_input(ADD, pair_input(&pid, t)));
        landed.extend(up(&mut server, "bob", &o));
    }
    for b in landed {
        tell(&mut alice, Input::Frame { from: None, bytes: b });
    }
    assert_eq!(alice.pending.len(), 2, "alice's two tracks still pending");
    assert!(told.get() > 12, "the rebases were told as changes: {}", told.get());
    let o = tell(&mut alice, Input::Connected);
    for b in up(&mut server, "alice", &o) {
        tell(&mut alice, Input::Frame { from: None, bytes: b });
    }
    assert!(alice.pending.is_empty() && alice.store == server.store);
}

fn who_of(name: &str) -> Who {
    Who {
        actor: name.into(),
        session: format!("{name}-session"),
        roles: vec![],
    }
}

fn replica_of(name: &str) -> Peer {
    Peer::new(demo::shared(), demo::cas(), Role::Replica, who_of(name))
}

fn mutate_input(name: &str, input: Vec<u8>) -> Input {
    Input::Mutate {
        func: h(name),
        input,
        now: 0,
    }
}

// * A scoped feed (§1.8)

/// What `actor` holds of `store`, by the scope's meaning written out.
fn union_of(store: &Store, actor: &str) -> Store {
    let rows: Vec<(String, Vec<Vec<u8>>)> = store
        .snapshot_rows()
        .into_iter()
        .map(|(t, rs)| {
            let held = rs
                .into_iter()
                .filter(|r| t == "playlist" || item_of(r).1.starts_with(&format!("{actor}:")))
                .collect();
            (t, held)
        })
        .collect();
    Store::from_rows(demo::shared(), ark::schema::Space::Durable, &rows).unwrap().0
}

/// A peer's confirmed state: its store with its pending changes undone.
fn confirmed(p: &Peer) -> Store {
    let mut s = p.store.clone();
    for c in p.pending.iter().rev().flat_map(|x| x.changes.iter().rev()) {
        s.apply(&c.inverse()).unwrap();
    }
    s
}

/// The authority's state at `seq`, its later facts undone.
fn state_at(server: &Peer, seq: i64) -> Store {
    let mut s = server.store.clone();
    for c in server
        .log()
        .after(seq)
        .collect::<Vec<_>>()
        .iter()
        .rev()
        .flat_map(|l| l.facts.iter().rev())
    {
        s.apply(&c.inverse()).unwrap();
    }
    s
}

/// A fleet of an authority, a client fed whole and two fed by
/// `demo.mine` (every playlist, and the items whose track is the
/// reader's — `actor:…`), on
/// v4's scheduler (a random frame from a random direction, one in eight
/// twice, one in sixteen lost).
struct Fleet {
    server: Peer,
    clients: Vec<Peer>,
    linked: Vec<bool>,
    up: Vec<Vec<Vec<u8>>>,
    down: Vec<Vec<Vec<u8>>>,
    seed: u64,
}

impl Fleet {
    fn new(seed: u64) -> Fleet {
        let cas = demo::cas();
        let h = "demo.mine";
        let auth = Role::Authority { journal: Journal::default() };
        let mut server = Peer::new(demo::shared(), cas.clone(), auth, who_of("server"));
        let mut clients = vec![];
        for i in 0..3 {
            let mut c = Peer::new(demo::shared(), cas.clone(), Role::Replica, who_of(&format!("peer-{i}")));
            if i > 0 {
                c.feed = Feed::Scoped(h.into());
                server.step(Input::Feed {
                    who: c.me.clone(),
                    feed: c.feed.clone(),
                });
            }
            clients.push(c);
        }
        let mut f = Fleet {
            server,
            clients,
            linked: vec![false; 3],
            up: vec![vec![]; 3],
            down: vec![vec![]; 3],
            seed,
        };
        (0..3).for_each(|i| f.heal(i));
        f
    }

    fn heard(&mut self, i: usize, o: Vec<Output>) {
        for x in o {
            if let (Output::Send(b), true) = (x, self.linked[i]) {
                self.up[i].push(b);
            }
        }
    }

    fn deliver_up(&mut self, i: usize, b: Vec<u8>) {
        let o = self.server.step(Input::Frame {
            from: Some(self.clients[i].me.clone()),
            bytes: b,
        });
        for x in o {
            match x {
                Output::Send(b) => self.down[i].push(b),
                Output::Fanout(b) => (0..3)
                    .filter(|j| self.linked[*j] && self.clients[*j].feed == Feed::Whole)
                    .for_each(|j| self.down[j].push(b.clone())),
                Output::To { session, bytes } => {
                    let j = self.clients.iter().position(|c| c.me.session == session).unwrap();
                    if self.linked[j] {
                        self.down[j].push(bytes);
                    }
                }
                _ => {}
            }
        }
    }

    fn deliver_down(&mut self, i: usize, b: Vec<u8>) {
        let o = self.clients[i].step(Input::Frame { from: None, bytes: b });
        self.heard(i, o);
    }

    fn mutate(&mut self, i: usize, track: &str) {
        let pid = EntryId::of(Origin::named("peer-0-session"), 1).draw(0);
        let o = self.clients[i].step(mutate_input(ADD, pair_input(&pid, track)));
        self.heard(i, o);
    }

    fn partition(&mut self, i: usize) {
        self.linked[i] = false;
        self.up[i].clear();
        self.down[i].clear();
    }

    fn heal(&mut self, i: usize) {
        if !self.linked[i] {
            self.linked[i] = true;
            let o = self.clients[i].step(Input::Connected);
            self.heard(i, o);
        }
    }

    fn roll(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.seed >> 11
    }

    fn step(&mut self) {
        let r = self.roll();
        let mut ready: Vec<(usize, bool)> = (0..3).filter(|i| !self.up[*i].is_empty()).map(|i| (i, true)).collect();
        ready.extend((0..3).filter(|i| !self.down[*i].is_empty()).map(|i| (i, false)));
        if ready.is_empty() {
            return;
        }
        let (i, upward) = ready[(r % ready.len() as u64) as usize];
        let times = match self.roll() % 16 {
            0 => 0,
            1 | 2 => 2,
            _ => 1,
        };
        let b = if upward { self.up[i].remove(0) } else { self.down[i].remove(0) };
        for _ in 0..times {
            if upward {
                self.deliver_up(i, b.clone())
            } else {
                self.deliver_down(i, b.clone())
            }
        }
    }

    /// Every client fed by the scope holds exactly its union of the
    /// authority's state at its cursor: no row outside it, nothing in it
    /// missing.
    fn unions_hold(&self) -> Result<(), String> {
        for c in self.clients.iter().filter(|c| c.feed != Feed::Whole) {
            let at = c.log().head().0;
            if confirmed(c) != union_of(&state_at(&self.server, at), &c.me.actor) {
                return Err(format!("{} at {at} does not hold its union", c.me.actor));
            }
        }
        Ok(())
    }

    fn settle(&mut self) {
        for i in 0..3 {
            self.partition(i);
            self.heal(i);
        }
        for _ in 0..1000 {
            if self.up.iter().chain(&self.down).all(|q| q.is_empty()) {
                return;
            }
            for i in 0..3 {
                for b in std::mem::take(&mut self.up[i]) {
                    self.deliver_up(i, b);
                }
                for b in std::mem::take(&mut self.down[i]) {
                    self.deliver_down(i, b);
                }
            }
        }
        panic!("the fleet did not settle");
    }
}

/// A replica fed by a scope (§1.8) in a fleet holds exactly its union
/// after every delivery, mutation, partition and heal — v4's `sim.rs`
/// partial-client assertion — and at the end every client is settled: the
/// whole one the authority's store, each scoped one its union of it,
/// nothing pending. Each client adds tracks of its own and of the others'
/// names, so facts cross every union's line. Falsified by the authority
/// sending a scoped connection the entry's facts whole: at seed 7's second
/// op peer-1 held tracks outside its union.
#[test]
fn a_scoped_replica_holds_its_union() {
    for seed in [7u64, 11, 23] {
        let mut f = Fleet::new(seed);
        let o = f.clients[0].step(mutate_input(CREATE, create_input("Fleet")));
        f.heard(0, o);
        f.settle();
        f.unions_hold().unwrap();
        for k in 0..30u64 {
            match (f.roll() % 10, k) {
                (0, _) => f.partition((k % 3) as usize),
                (1, _) => f.heal((k % 3) as usize),
                (r, _) if r < 6 => {
                    let (i, j) = ((k % 3) as usize, (r % 3) as usize);
                    f.mutate(i, &format!("peer-{j}:{k}"));
                }
                _ => (0..4).for_each(|_| f.step()),
            }
            f.unions_hold().unwrap_or_else(|e| panic!("seed {seed}, op {k}: {e}"));
        }
        f.settle();
        f.unions_hold().unwrap();
        for c in &f.clients {
            assert!(c.pending.is_empty(), "{} settled", c.me.actor);
            let want = if c.feed == Feed::Whole {
                f.server.store.clone()
            } else {
                union_of(&f.server.store, &c.me.actor)
            };
            assert!(c.store == want, "{} holds what it is fed", c.me.actor);
            let (held, all) = (c.store.rows("item").count(), f.server.store.rows("item").count());
            assert!(
                c.feed == Feed::Whole || (0 < held && held < all),
                "{} holds some items, not all",
                c.me.actor
            );
        }
        assert!(f.server.store.rows("item").count() > 5, "tracks were added");
    }
}

// * A table browsed

/// A table browsed (`ark::dynamic::Browse`) is a plan like any other: the
/// items of one playlist, from track `b` on, last place first, the first
/// two, kept by patches through adds, a move, a remove and a row of another
/// playlist — after each, its rows those a fresh browse gives, and the
/// patches splice. Falsified by the descending sort not complementing the
/// column's bytes: the first rows were `b`, `c` where the browse asks `d`,
/// `c`.
#[test]
fn a_browse_is_kept_by_patches() {
    let (p, q) = ([1u8; 16], [2u8; 16]);
    let item = |pl: &[u8], t: &str, pos| Change::Add {
        table: "item".into(),
        row: item_row(pl, t, pos),
    };
    let mut store = Store::from_rows(
        shared(),
        ark::schema::Space::Durable,
        &[("playlist".into(), vec![playlist_row(&p, "P", "alice"), playlist_row(&q, "Q", "bob")])],
    )
    .unwrap()
    .0;
    for c in [item(&p, "a", 1), item(&p, "b", 2), item(&p, "c", 3), item(&p, "d", 4), item(&q, "z", 9)] {
        store.apply(&c).unwrap();
    }
    let b = ark::dynamic::Browse::table("item")
        .eq("playlist_id", serde_json::json!(hex(&p)))
        .range(Some(serde_json::json!([hex(&p), "b"])), None)
        .sort("pos", true)
        .take(2);
    let mut v = ark::plan::View::new(b.make(&shared()).unwrap(), ctx(), &store).unwrap();
    let tracks = |rows: Vec<Vec<u8>>| rows.iter().map(|r| item_of(r).1).collect::<Vec<_>>();
    assert_eq!(tracks(v.rows()), ["d", "c"]);
    let moved = Change::Edit {
        table: "item".into(),
        old: item_row(&p, "c", 3),
        new: item_row(&p, "c", 7),
    };
    let gone = Change::Remove {
        table: "item".into(),
        row: item_row(&p, "d", 4),
    };
    for batch in [vec![item(&p, "e", 5)], vec![moved], vec![gone, item(&q, "y", 8)], vec![item(&p, "a0", 9)]] {
        let was = v.rows();
        store.apply_all(&batch).unwrap();
        let patches = v.push(&batch, &store).unwrap();
        assert_eq!(v.rows(), b.rows(&store).unwrap(), "a fresh browse");
        assert_eq!(splice(&patches, &was), v.rows(), "the patches splice");
    }
    assert_eq!(tracks(v.rows()), ["c", "e"], "a0 is below the range");
}
