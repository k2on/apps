//! What harken's own mutations and reads cost as the library grows, held
//! as bounds rather than printed: the rows each mutation's reads examine
//! (`ark::db::count`; the store examines only what it returns) and the
//! allocations a mutation and a read make, counted on the test's own
//! thread. Over the domain's functions in Rust (§1.16) and the store under
//! them, not a peer, a storage or a socket.
//!
//! [`timings`] is the benchmark Part 3's measurement protocol reads
//! (`docs/plan-v5.md`): ignored by default; run with
//! `cargo test -p harken-domain --release --test costs -- --ignored --nocapture`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::OnceLock;
use std::time::Instant;

use ark::db::{count, Reads};
use ark::domain::Domain;
use ark::format::Encode;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::store::Store;
use harken_domain::Id;
use harken_domain::LIBRARY;
use harken_domain::{AddSong, Library};
use harken_domain::{AddToPlaylist, CreatePlaylist};
use harken_domain::{Media, Playlist, PlaylistItem};

// Allocations, counted on this thread ------------------------------------------

struct Tally;

thread_local!(static ALLOCS: Cell<usize> = const { Cell::new(0) });

fn tally() {
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
}

unsafe impl GlobalAlloc for Tally {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        tally();
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        tally();
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

// The library, as a store holds it ------------------------------------------------

fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(harken_domain::domain)
}

/// A mutator run over `st`, as the machine runs it.
fn run(st: &mut Store, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Vec<ark::store::Change>, ark::Refusal> {
    domain().get(name).and_then(|f| f.proc.clone()).expect("a mutator").apply(st, ctx, input)
}

fn idv(tag: u8, n: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = tag;
    b[8..].copy_from_slice(&n.to_be_bytes());
    b
}

/// A media row, as `add_song` writes one.
fn media(i: u64) -> Media {
    Media {
        id: Id::new(idv(1, i)),
        kind: "song".into(),
        title: format!("Track {i}"),
        creator: format!("Artist {}", i % 50),
        duration_ms: 180_000,
        file: format!("music/a{}/t{i}.flac", i % 50),
        pos: i as i64 + 1,
        added_ms: 1_000 + i as i64,
        user_id: "library".into(),
    }
}

/// A library of `n` media rows, and nothing else.
fn library(n: u64) -> Store {
    let mut st = Store::new(domain().schema.clone(), ark::schema::Space::Durable);
    for i in 0..n {
        st.put("media", media(i).to_bytes()).unwrap();
    }
    st
}

fn playlist_row(id: [u8; 16], name: &str, pos: i64, user: &str) -> Vec<u8> {
    Playlist {
        id: Id::new(id),
        name: name.into(),
        pos,
        created_ms: 1,
        user_id: user.into(),
    }
    .to_bytes()
}

/// A library of `n` media rows and one playlist of alice's holding the
/// first `items` of them, written as rows.
fn with_playlist(n: u64, items: u64) -> (Store, [u8; 16]) {
    let mut st = library(n);
    let pl = idv(2, 1);
    st.put("playlist", playlist_row(pl, "Mine", 1, "alice")).unwrap();
    for i in 0..items {
        let row = PlaylistItem {
            playlist_id: Id::new(pl),
            media_id: Id::new(idv(1, i)),
            pos: i as i64 + 1,
            added_ms: 1,
            user_id: "alice".into(),
        };
        st.put("playlist_item", row.to_bytes()).unwrap();
    }
    (st, pl)
}

/// A library of `n` media rows, and a thousand playlists each for alice
/// and bob: alice's "Favorites", a hundred "Favorites (k)" and 899 others,
/// bob's a thousand "Favorites (k)".
fn with_playlists(n: u64) -> Store {
    let mut st = library(n);
    let alice = std::iter::once("Favorites".to_string())
        .chain((1..=100).map(|k| format!("Favorites ({k})")))
        .chain((0..899).map(|k| format!("List {k}")))
        .map(|name| ("alice", name));
    let bob = (1..=1000).map(|k| ("bob", format!("Favorites ({k})")));
    for (i, (user, name)) in alice.chain(bob).enumerate() {
        st.put("playlist", playlist_row(idv(3, i as u64), &name, i as i64 + 1, user)).unwrap();
    }
    st
}

fn song(i: u64) -> Vec<u8> {
    AddSong {
        title: format!("Track {i}"),
        artist: format!("Artist {}", i % 50),
        album: format!("Album {}", i % 200),
        duration_ms: 180_000,
        file: format!("music/new/{i}.flac"),
        track: (i % 12) as i64 + 1,
        ..AddSong::default()
    }
    .to_bytes()
}

fn on(pl: [u8; 16], m: [u8; 16]) -> Vec<u8> {
    AddToPlaylist {
        playlist_id: Id::new(pl),
        media_id: Id::new(m),
    }
    .to_bytes()
}

fn named(name: &str) -> Vec<u8> {
    CreatePlaylist { name: name.into() }.to_bytes()
}

type Make = Box<dyn Fn(u64) -> (Store, Vec<u8>)>;

/// The mutations whose reads are about, each with the store and the input
/// it is applied to at a library of `n`.
fn read_cases() -> Vec<(&'static str, &'static str, Make)> {
    let onto = |filled: bool| {
        move |n: u64| {
            let (st, pl) = with_playlist(n, if filled { n - 1 } else { 0 });
            (st, on(pl, idv(1, n - 1)))
        }
    };
    vec![
        ("add_song (a new file)", "library.add_song", Box::new(|n| (library(n), song(99_999_999)))),
        ("add_to_playlist (an empty playlist)", "playlists.add_to_playlist", Box::new(onto(false))),
        ("add_to_playlist (a playlist of N-1)", "playlists.add_to_playlist", Box::new(onto(true))),
        (
            "create_playlist (alice's second)",
            "playlists.create_playlist",
            Box::new(|n| (with_playlist(n, 0).0, named("Other"))),
        ),
        (
            "create_playlist (a 102nd Favorites, of 1,000)",
            "playlists.create_playlist",
            Box::new(|n| (with_playlists(n), named("Favorites"))),
        ),
    ]
}

fn ctx_of(n: i64) -> Ctx {
    Ctx {
        id: EntryId::of("local", n),
        actor: "alice".into(),
        session: "local".into(),
        roles: vec![LIBRARY.into()],
        now: 5,
        ..Default::default()
    }
}

fn reads_of(n: u64, name: &str, make: &dyn Fn(u64) -> (Store, Vec<u8>)) -> Reads {
    let (mut st, input) = make(n);
    let (out, reads) = count(|| run(&mut st, &ctx_of(1_000_000), name, &input));
    assert!(out.as_ref().is_ok_and(|cs| !cs.is_empty()), "{name}: {out:?}");
    reads
}

/// What the mutations read is bounded by what they need, as v4's was, at
/// any library size: `add_song` at most v4's 15 gets and one row (the
/// file's media by `Media::FILE`, the newest by `Media::POS` from its
/// end), `add_to_playlist` onto a playlist of N-1 one row, the 102nd
/// "Favorites" the 101 names from "Favorites" up to "Favorites )" and the
/// last playlist by `pos` — not every playlist. Not ignored: it pins the
/// paths the domain names. Falsified by `add_song` reading the media by
/// scan for its file (500 rows at 500) and by `create_playlist` reading
/// all of the person's names (1,001 rows: alice's 1,000 and the last).
#[test]
fn reads_are_bounded_by_what_they_need() {
    for n in [500u64, 8000] {
        let reads: Vec<_> = read_cases().iter().map(|(_, name, make)| reads_of(n, name, &**make)).collect();
        let (song, onto, favorites) = (reads[0], reads[2], reads[4]);
        assert!(song.gets <= 15 && song.rows <= 1, "add_song at {n}: {song:?}");
        assert!(onto.rows <= 1, "add_to_playlist onto N-1 at {n}: {onto:?}");
        assert!(favorites.rows <= 102, "the 102nd Favorites at {n}: {favorites:?}");
    }
}

/// `add_to_playlist` onto a playlist of N/3 items allocates what it did at
/// a library of 500 at 8,000, and at most 128 times (94 measured);
/// `library` read whole allocates at most 32 times an entry (26.4).
/// Falsified by `add_to_playlist` finding the last place by reading every
/// item of the playlist (760 allocations at 500, 10,764 at 8,000), and by
/// `library` formatting each entry twice (70,705 at 2,000).
#[test]
fn a_mutation_allocates_the_same_as_the_library_grows() {
    let per = |n: u64| {
        let (mut st, pl) = with_playlist(n, n / 3);
        let input = on(pl, idv(1, n - 1));
        let (out, made) = allocs(|| run(&mut st, &ctx_of(1), "playlists.add_to_playlist", &input));
        assert!(out.is_ok_and(|cs| cs.len() == 1));
        made
    };
    let (small, big) = (per(500), per(8000));
    assert!(
        big <= small + 4 && big <= 128,
        "add_to_playlist: {small} allocations at 500, {big} at 8,000"
    );
    let n = 2000;
    let (st, pl) = with_playlist(n, n / 3);
    let input = Library { playlist_id: Id::new(pl) }.to_bytes();
    let (v, made) = allocs(|| domain().ask(&st, &ctx_of(1), "library.library", &input).unwrap());
    assert_eq!(harken_domain::LibraryEntry::list(&v).unwrap().len(), n as usize);
    assert!(made <= 32 * n as usize, "library of {n}: {made} allocations");
}

/// The numbers Part 3 reads: `add_song` into a library growing to 8,000,
/// one per file as the scanner authors them, applied to the store.
#[test]
#[ignore]
fn timings() {
    let mut st = Store::new(domain().schema.clone(), ark::schema::Space::Durable);
    let n = 8000;
    let t = Instant::now();
    for i in 0..n {
        let changes = run(&mut st, &ctx_of(i as i64 + 1), "library.add_song", &song(i)).unwrap();
        assert!(!changes.is_empty());
    }
    eprintln!(
        "harken add_song, a library growing to {n}: {:.1} µs",
        t.elapsed().as_secs_f64() * 1e6 / n as f64
    );
}
