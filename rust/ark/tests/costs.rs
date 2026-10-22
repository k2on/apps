//! What the machine's paths cost, held as bounds rather than printed: the
//! allocations a `Batch` of a page takes to decode and a step of a peer
//! alone takes as its log grows, and what one change against each of
//! three maintained views reads and allocates as its store grows — the
//! counts sixteen times apart the same. Over the demo of `tests/demo.rs`,
//! driven through [`Peer::step`] with frames as bytes. Allocations are
//! counted on the test's own thread.
//!
//! [`timings`] is the benchmark Part 3's measurement protocol reads
//! (`docs/plan-v5.md`): ignored by default; run with
//! `cargo test -p ark --release --test costs -- --ignored --nocapture`.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ark::frames::Frame;
use ark::log::{Entry, EntryId, Logged, Origin, Seq};
use ark::peer::{Ctx, Input, Output, Peer, Role, Who, PAGE};
use ark::prelude::*;
use ark::schema::{Schema, Space};
use ark::store::{Change, Store};
use demo::*;
use support::wire::up;

// Allocations, counted on this thread ------------------------------------------------

struct Tally;

thread_local!(static ALLOCS: Cell<usize> = const { Cell::new(0) });

fn count() {
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
}

unsafe impl GlobalAlloc for Tally {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count();
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count();
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Tally = Tally;

/// What `f` returns, and the allocations it made on this thread.
fn allocs<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = ALLOCS.with(Cell::get);
    let t = f();
    (t, ALLOCS.with(Cell::get) - before)
}

// The demo, driven ---------------------------------------------------------------------

fn who(actor: &str, session: &str) -> Who {
    Who {
        actor: actor.into(),
        session: session.into(),
        roles: vec![],
    }
}

/// A log an authority sequenced from one author: `playlists` playlists,
/// then `n` adds spread over them in turn.
fn log(playlists: u64, n: u64) -> Peer {
    let mut a = support::wire::authority(cas());
    let me = who("alice", "alice-dev");
    let mut local = 0;
    let mut push = |a: &mut Peer, name: &str, input: Vec<u8>| {
        local += 1;
        let entry = Entry::new(&me, Origin::named(&me.session), local, name.to_string(), input, local);
        a.step(up(&me, &Frame::push(Space::Durable, [entry])));
    };
    for j in 0..playlists {
        push(&mut a, CREATE, create_input(&format!("P{j}")));
    }
    let pid = |j: u64| EntryId::of(Origin::named("alice-dev"), j as i64 + 1).draw(0);
    for i in 0..n {
        push(&mut a, ADD, pair_input(&pid(i % playlists), &format!("t{i}")));
    }
    assert_eq!(a.log().head().0, (playlists + n) as Seq, "every entry sequenced");
    a
}

/// A page of the log as the `Batch` a replica is sent.
fn page(a: &Peer) -> Frame {
    let logged: Vec<Logged> = a.log().entries();
    let page = &logged[logged.len() - PAGE..];
    Frame::Page {
        space: Space::Durable,
        from: page[0].seq - 1,
        logged: page.to_vec(),
    }
}

/// A peer alone holding `n` adds over `playlists` playlists, and the
/// input of its next add.
fn alone(playlists: u64, n: u64) -> (Peer, impl FnMut(&mut Peer) -> Vec<Output>) {
    let mut p = Peer::new(shared(), cas(), Role::Replica, who("me", "me-local"));
    p.step(Input::Alone);
    for j in 0..playlists {
        p.step(support::wire::mutate(CREATE, create_input(&format!("P{j}")), 0));
    }
    let pid = move |j: u64| EntryId::of(Origin::named("me-local"), j as i64 + 1).draw(0);
    let mut i = 0;
    let mut add = move |p: &mut Peer| {
        i += 1;
        p.step(support::wire::mutate(ADD, pair_input(&pid(i % playlists), &format!("t{i}")), i as i64))
    };
    for _ in 0..n {
        add(&mut p);
    }
    (p, add)
}

/// A `Batch` of a page of 256 entries with their facts decodes in at most
/// twelve allocations an entry (2,817 measured, as at N4: a row is a
/// `Vec` per fact), and encodes in at most 32 in all (14); a `Push` of one entry
/// decodes in at most sixteen. Falsified by `Frame::dec` decoding each
/// entry of a page again from its own bytes: 7,169 for the page.
#[test]
fn a_page_decodes_in_bounded_allocations() {
    let f = page(&log(10, PAGE as u64));
    let (bytes, enc) = allocs(|| f.enc());
    let (back, dec) = allocs(|| Frame::dec(&bytes).unwrap());
    assert_eq!(back, f);
    assert!(dec <= 12 * PAGE, "a page decoded in {dec} allocations");
    assert!(enc <= 32, "a page encoded in {enc} allocations");
    let Frame::Page { logged, .. } = &f else { unreachable!() };
    let push = Frame::push(Space::Durable, [logged[0].entry.clone()]).enc();
    let (_, one) = allocs(|| Frame::dec(&push).unwrap());
    assert!(one <= 16, "a push decoded in {one} allocations");
}

/// A peer alone's step of an add allocates no more over a log of 8,000
/// than over one of 500 (at most 128 either way; 115 and 116 measured): nothing it does per
/// step walks the log or the store. Falsified by `Peer::step` cloning its
/// log's entries first: 5,734 allocations at 500, 88,239 at 8,000.
#[test]
fn a_step_alone_allocates_the_same_as_the_log_grows() {
    let per = |n: u64| {
        let (mut p, mut add) = alone(10, n);
        let (o, a) = allocs(|| add(&mut p));
        assert!(matches!(o.last(), Some(Output::Confirmed(_))), "{o:?}");
        a
    };
    let (small, big) = (per(500), per(8000));
    assert!(big <= small + 4 && big <= 128, "a step alone: {small} allocations at 500, {big} at 8,000");
}

// Three maintained views over the library's schema -----------------------------------

/// A song, as the library's `song` table has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(index(creator))]
pub struct Song {
    #[ark(key)]
    pub id: String,
    pub creator: String,
    pub pos: i64,
}

#[derive(Schema)]
pub struct Lib {
    pub playlist: Playlist,
    pub item: Item,
    pub song: Song,
}

fn pid(n: u32) -> Vec<u8> {
    [&[0; 12][..], &n.to_be_bytes()].concat()
}

fn add(table: &str, row: Vec<u8>) -> Change {
    Change::Add { table: table.into(), row }
}

fn store_of(rows: &[Change]) -> Store {
    let schema: Schema = <Lib as Tables>::schema();
    let mut by: Vec<(String, Vec<Vec<u8>>)> = schema.tables.iter().map(|t| (t.name.clone(), vec![])).collect();
    for c in rows {
        let Change::Add { table, row } = c else { panic!("a store is rows") };
        by.iter_mut().find(|(t, _)| t == table).unwrap().1.push(row.clone());
    }
    Store::from_rows(schema.into(), Space::Durable, &by).unwrap().0
}

type Made<T> = ark::plan::Make<Lib, T>;
type WithItems = (Id<Playlist>, String, Vec<Item>);

/// `playlist-with-items`: playlists by name, each with its last three items.
fn playlist_with_items() -> Made<WithItems> {
    Arc::new(|_, _| {
        Ok(ark::plan::scan::<Lib, Playlist>().sort(|p| (p.name.clone(), p.id)).map_with(|db, p| {
            let last = db.item.by(Item::PLAYLIST_ID_POS).from(p.id).rev().take(3);
            (p.id, p.name.clone(), last)
        }))
    })
}

/// `top-two-by-pos`: playlist 1's items by `pos`, the first two.
fn top_two_by_pos() -> Made<Item> {
    let one = Id::new(pid(1).try_into().unwrap());
    Arc::new(move |_, _| {
        Ok(ark::plan::scan::<Lib, Item>()
            .filter(move |i| i.playlist_id == one)
            .sort(|i| (i.pos, i.track_id.clone()))
            .take(2))
    })
}

/// `group-source`: songs grouped by creator, each with how many.
fn group_source() -> Made<(String, i64)> {
    Arc::new(|_, _| {
        Ok(ark::plan::scan::<Lib, Song>()
            .group(|s| s.creator.clone(), |_, k, ms| (k.clone(), ms.len() as i64))
            .sort(|g| g.0.clone()))
    })
}

fn song(id: &str, creator: &str) -> Vec<u8> {
    Song {
        id: id.into(),
        creator: creator.into(),
        pos: 0,
    }
    .to_bytes()
}

/// The store at `n` rows and a change and its inverse, for each view:
/// an item onto one of n/4 playlists of four; playlist 1's first of n
/// items leaving; a song changing creator among n of eight creators.
fn cases(n: u32) -> [(&'static str, Store, [Change; 2]); 3] {
    let mut rows = vec![];
    for p in 0..n / 4 {
        rows.push(add("playlist", playlist_row(&pid(p), &format!("p{p}"), "alice")));
        rows.extend((1..=4).map(|k| add("item", item_row(&pid(p), &format!("t{k}"), k))));
    }
    let fresh = add("item", item_row(&pid(n / 8), "t5", 5));
    let with_items = store_of(&rows);
    let mut rows = vec![add("playlist", playlist_row(&pid(1), "Viewed", "alice"))];
    rows.extend((0..n).map(|i| add("item", item_row(&pid(1), &format!("t{i}"), i as i64 + 1))));
    let first = add("item", item_row(&pid(1), "t0", 1));
    let top = store_of(&rows);
    let songs: Vec<Change> = (0..n).map(|i| add("song", song(&format!("s{i}"), &format!("c{}", i % 8)))).collect();
    let (c0, c1) = (song("s0", "c0"), song("s0", "c1"));
    let edit = |old: &[u8], new: &[u8]| Change::Edit {
        table: "song".into(),
        old: old.to_vec(),
        new: new.to_vec(),
    };
    [
        ("playlist-with-items", with_items, [fresh.clone(), fresh.inverse()]),
        ("top-two-by-pos", top, [first.inverse(), first]),
        ("group-source", store_of(&songs), [edit(&c0, &c1), edit(&c1, &c0)]),
    ]
}

fn ctx() -> Ctx {
    Ctx {
        actor: "alice".into(),
        session: "dev".into(),
        ..Default::default()
    }
}

/// One view made over `store`; then each change applied and pushed,
/// `rounds` times over: what the second push of the first change read and
/// allocated, and the mean time of a push.
fn pushed<T: ark::plan::Elem + std::fmt::Debug>(make: Made<T>, mut store: Store, ops: &[Change], rounds: usize) -> (ark::db::Reads, usize, Duration) {
    let mut v = ark::plan::View::new(make.clone(), ctx(), &store).unwrap();
    let mut push = |store: &mut Store, c: &Change| {
        store.apply(c).unwrap();
        v.push(std::slice::from_ref(c), store).unwrap();
    };
    ops.iter().for_each(|c| push(&mut store, c));
    store.apply(&ops[0]).unwrap();
    let ((_, reads), made) = allocs(|| ark::db::count(|| v.push(std::slice::from_ref(&ops[0]), &store).unwrap()));
    let mut push = |store: &mut Store, c: &Change| {
        store.apply(c).unwrap();
        v.push(std::slice::from_ref(c), store).unwrap();
    };
    ops[1..].iter().for_each(|c| push(&mut store, c));
    let t = Instant::now();
    for _ in 0..rounds {
        ops.iter().for_each(|c| push(&mut store, c));
    }
    let spent = t.elapsed() / (rounds * ops.len()).max(1) as u32;
    assert_eq!(v.rows(), ark::plan::answer(&make, &ctx(), &store).unwrap(), "the view is a fresh answer");
    (reads, made, spent)
}

/// Each view of `n` rows: its name, what one push read and allocated, and
/// the mean time of a push over `rounds`.
fn views(n: u32, rounds: usize) -> Vec<(&'static str, ark::db::Reads, usize, Duration)> {
    let [a, b, c] = cases(n);
    vec![
        (a.0, pushed(playlist_with_items(), a.1, &a.2, rounds)),
        (b.0, pushed(top_two_by_pos(), b.1, &b.2, rounds)),
        (c.0, pushed(group_source(), c.1, &c.2, rounds)),
    ]
    .into_iter()
    .map(|(name, (r, a, t))| (name, r, a, t))
    .collect()
}

/// One change against each view reads and allocates what it did at 1,000
/// rows at 16,000: `playlist-with-items` reads its playlist's last items,
/// the others nothing; no push reads more than three rows or allocates
/// more than 128 times (104, 87 and 97 measured). Falsified by
/// `View::push` answering afresh before it patches: playlist-with-items'
/// reads differed between the sizes.
#[test]
fn a_view_push_costs_the_same_as_its_store_grows() {
    let (small, big) = (views(1000, 0), views(16000, 0));
    for ((name, r1, a1, _), (_, r16, a16, _)) in small.iter().zip(&big) {
        assert_eq!((r1.gets, r1.rows), (r16.gets, r16.rows), "{name}: reads at 1,000 and 16,000");
        assert!(r16.rows <= 3, "{name}: {r16:?}");
        assert!(*a16 <= a1 + 8 && *a16 <= 128, "{name}: {a1} allocations at 1,000, {a16} at 16,000");
    }
}

/// The numbers Part 3 reads: a peer alone's add over a log of 8,000, a
/// page of 256 decoded, and a push against each view at 16,000 rows.
#[test]
#[ignore]
fn timings() {
    let (mut p, mut add) = alone(800, 8000);
    let t = Instant::now();
    (0..1000).for_each(|_| drop(add(&mut p)));
    eprintln!("a: a peer alone's add, log of 8,000, playlists of 10: {:.2} µs", us(t.elapsed() / 1000));
    let bytes = page(&log(10, PAGE as u64)).enc();
    let t = Instant::now();
    (0..50).for_each(|_| drop(Frame::dec(&bytes).unwrap()));
    eprintln!("h: a Batch of {PAGE} decoded: {:.1} µs", us(t.elapsed() / 50));
    for (name, _, _, t) in views(16000, 2000) {
        eprintln!("views: a change on {name} at 16,000: {:.2} µs", us(t));
    }
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}
