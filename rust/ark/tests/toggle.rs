//! §1.5 "A change costs the entries whose recorded dependencies it hits":
//! held for the one change a person makes most, a playlist toggle, against
//! the one view every window holds, harken's `library` — every media row in
//! `pos` order, each with its entry on one playlist beneath it. Written in
//! this crate's own vocabulary rather than by depending on the harken
//! domain (which depends on this crate): the tables, the plan and the
//! projection are harken's, column for column and node for node.
//!
//! Two things are here. [`a_toggle_reads_the_same_rows_at_any_size`] is the
//! regression test, and it counts rather than times. [`bench_toggle`] is
//! the timing, ignored by default; run it with
//! `cargo test -p ark --release --test toggle -- --ignored --nocapture --test-threads=1`.

#[path = "support/counting.rs"]
mod counting;

use std::time::{Duration, Instant};

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::schema::Schema;
use ark::store::{Change, MemoryStore, Store};
use ark::value::Value;
use ark::view::{self, Env, Patch};
use counting::{Counting, Reads};

pub struct Lib {
    pub media: Table<Media>,
    pub playlist: Table<Playlist>,
    pub playlist_item: Table<PlaylistItem>,
}
impl Tables for Lib {
    fn open() -> Self {
        Lib {
            media: table(),
            playlist: table(),
            playlist_item: table(),
        }
    }
}

pub struct Media {
    pub id: Id<Media>,
    pub kind: Text,
    pub title: Text,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for Media {
    const NAME: &str = "media";
    type Key = (Id<Media>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::kind)
            .text(Self::title)
            .text(Self::creator)
            .int(Self::duration_ms)
            .text(Self::file)
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Media {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const kind: Col<Self, Text> = col("kind");
    pub const title: Col<Self, Text> = col("title");
    pub const creator: Col<Self, Text> = col("creator");
    pub const duration_ms: Col<Self, Int> = col("duration_ms");
    pub const file: Col<Self, Text> = col("file");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub pos: Int,
    pub created_ms: Int,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .int(Self::pos)
            .int(Self::created_ms)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const pos: Col<Self, Int> = col("pos");
    pub const created_ms: Col<Self, Int> = col("created_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct PlaylistItem {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
    pub pos: Int,
    pub added_ms: Int,
    pub user_id: Text,
}
impl Row for PlaylistItem {
    const NAME: &str = "playlist_item";
    type Key = (Id<Playlist>, Id<Media>);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .id(Self::media_id)
            .refs::<Media>()
            .int(Self::pos)
            .int(Self::added_ms)
            .text(Self::user_id)
            .key((Self::playlist_id, Self::media_id))
    }
}
#[allow(non_upper_case_globals)]
impl PlaylistItem {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const media_id: Col<Self, Id<Media>> = col("media_id");
    pub const pos: Col<Self, Int> = col("pos");
    pub const added_ms: Col<Self, Int> = col("added_ms");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct LibraryEntry {
    pub added_ms: Int,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub id: Id<Media>,
    pub kind: Text,
    pub playlist_pos: Opt<Int>,
    pub pos: Int,
    pub title: Text,
    pub user_id: Text,
}
impl Record for LibraryEntry {
    fn fields() -> Fields<Self> {
        fields()
            .field("added_ms", int())
            .field("creator", text())
            .field("duration_ms", int())
            .field("file", text())
            .field("id", id::<Media>())
            .field("kind", text())
            .field("playlist_pos", opt(int()))
            .field("pos", int())
            .field("title", text())
            .field("user_id", text())
    }
}

pub struct Library {
    pub playlist_id: Id<Playlist>,
}
impl Input for Library {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

fn library_entry(media: Media, playlist_pos: Opt<Int>) -> LibraryEntry {
    helper(
        "library_entry",
        (("media", media), ("playlist_pos", playlist_pos)),
        |media: Media, playlist_pos: Opt<Int>| LibraryEntry {
            added_ms: media.added_ms,
            creator: media.creator,
            duration_ms: media.duration_ms,
            file: media.file,
            id: media.id,
            kind: media.kind,
            playlist_pos,
            pos: media.pos,
            title: media.title,
            user_id: media.user_id,
        },
    )
}

fn lib() -> Router<Lib> {
    let r = router::<Lib>("lib");
    r.routes((r.input::<Library>().query("library", |_ctx, db, input| {
        db.media
            .order_by(Media::pos.asc())
            .each(|media, ()| {
                db.playlist_item
                    .filter(PlaylistItem::playlist_id.eq(input.playlist_id))
                    .on(PlaylistItem::media_id.eq(media.id))
            })
            .map(|media, (items,)| library_entry(media, items.first().map(|row| row.pos)))
    }),))
}

// An id as harken's are: sixteen bytes that say nothing about the order
// the rows were made in (splitmix64 of the number), so that key order and
// `pos` order disagree the way they do in a real library.
fn key(tag: u8, n: usize) -> Value {
    let mix = |mut z: u64| {
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let a = mix((n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(tag));
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&a.to_be_bytes());
    b[8..].copy_from_slice(&mix(a).to_be_bytes());
    b[0] = tag;
    Value::Id(b)
}

fn row(pairs: Vec<(&str, Value)>) -> ark::store::Row {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn media(n: usize) -> ark::store::Row {
    row(vec![
        ("id", key(1, n)),
        ("kind", Value::text("song")),
        ("title", Value::text(format!("Track {n}"))),
        ("creator", Value::text(format!("Artist {}", n % 97))),
        ("duration_ms", Value::int(180_000 + n as i64)),
        ("file", Value::text(format!("music/{}/{n}.flac", n % 97))),
        ("pos", Value::int(n as i64 + 1)),
        ("added_ms", Value::int(1_700_000_000_000 + n as i64)),
        ("user_id", Value::text("library")),
    ])
}

/// The playlist the view is read against.
const ON: usize = 0;
/// How many playlists there are: the one read against, and others the
/// media are also on, so that both of `playlist_item`'s references have a
/// posting list worth choosing between.
const PLAYLISTS: usize = 3;

fn item(p: usize, m: usize, pos: usize) -> ark::store::Row {
    row(vec![
        ("playlist_id", key(2, p)),
        ("media_id", key(1, m)),
        ("pos", Value::int(pos as i64)),
        ("added_ms", Value::int(0)),
        ("user_id", Value::text("alice")),
    ])
}

/// A library of `n` media, half of them on the playlist the view reads and
/// every one of them on the other two: the playlist's own items grow with
/// the library, which is what a read through its index would walk.
fn seeded(sch: &Schema, n: usize) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    for p in 0..PLAYLISTS {
        st.apply_change(&Change::Add(
            "playlist".into(),
            row(vec![
                ("id", key(2, p)),
                ("name", Value::text(format!("List {p}"))),
                ("pos", Value::int(p as i64)),
                ("created_ms", Value::int(0)),
                ("user_id", Value::text("alice")),
            ]),
        ));
    }
    for m in 0..n {
        st.apply_change(&Change::Add("media".into(), media(m)));
        for p in 0..PLAYLISTS {
            if p != ON || m % 2 == 0 {
                st.apply_change(&Change::Add("playlist_item".into(), item(p, m, m)));
            }
        }
    }
    st
}

/// The schema, the `library` plan and its environment, read against `ON`.
fn library_view() -> (Schema, ark::ir::Plan, Env) {
    let m = Module::new((lib(),));
    let built = m.build();
    let f = built.lookup_function("library").expect("the library query");
    let c = ark::hash::closure(built, f);
    let args = Args::from([("playlist_id".to_string(), key(2, ON))]);
    let env = Env {
        helpers: c.helpers.clone(),
        ctx: eval::Ctx::default(),
        args,
        provided: Args::new(),
    };
    (built.schema.clone(), f.plan.clone().expect("a plan"), env)
}

/// One toggle: the media in the middle of the library, not on the
/// playlist, added to it and then taken off.
fn toggle(n: usize) -> (Change, Change) {
    let m = n / 2 + 1; // odd: not on `ON`
    let r = item(ON, m, n + 1);
    (Change::Add("playlist_item".into(), r.clone()), Change::Remove("playlist_item".into(), r))
}

/// What pushing one change reads from the store, after applying it.
fn push_counted(sch: &Schema, st: &mut MemoryStore, ch: &Change, v: &mut view::View) -> (Reads, Vec<Patch>) {
    st.apply_change(ch);
    let counted = Counting::new(st);
    let ps = view::push_all(sch, &counted, std::slice::from_ref(ch), v).expect("push");
    (counted.reads(), ps)
}

/// §1.5 A toggle reads the same rows from the store whatever the size of
/// the library: the media row its entry is keyed by (one `get`), and the
/// `playlist_item` row under `(playlist, media)` — one row looked at when
/// the toggle put it there, none when it took it away — at N = 200 and at
/// N = 3200 alike. The patch is one `Update` at the media's place, and the
/// view stays a fresh hydrate.
///
/// Counted, not timed, so it holds on any machine; "looked at" is every
/// row the store's filter was asked about ([`counting`]), so a read
/// through the wrong index shows here even though it returns the right
/// row. Falsified twice:
///
/// - answering the child pull through the *first* secondary whose columns
///   the equalities hold (no key read, no posting probe): the read walks
///   every item of the playlist, 101 rows at N = 200 and 1601 at N = 3200;
/// - dropping the key read alone: the store reads through the `media_id`
///   index, 3 rows on and 2 off at both sizes — flat, but every playlist
///   the media is on rather than the one row the key names. That is also
///   what this toggle read before the key read existed: the widest-index
///   rule broke its tie by position, and `media_id` is declared last.
#[test]
fn a_toggle_reads_the_same_rows_at_any_size() {
    let (sch, plan, env) = library_view();
    let mut seen = vec![];
    for n in [200, 3200] {
        let mut st = seeded(&sch, n);
        let mut v = view::hydrate(&sch, &plan, env.clone(), &st).expect("hydrate");
        let (on, off) = toggle(n);
        let at = n / 2 + 1;
        let (added, ps) = push_counted(&sch, &mut st, &on, &mut v);
        assert!(
            matches!(ps.as_slice(), [Patch::Update { at: a, node }] if *a == at && node.field("playlist_pos") == Value::int(n as i64 + 1)),
            "{ps:?}"
        );
        assert!(view::contract(&sch, &st, &v));
        let (removed, ps) = push_counted(&sch, &mut st, &off, &mut v);
        assert!(
            matches!(ps.as_slice(), [Patch::Update { at: a, node }] if *a == at && node.field("playlist_pos") == Value::Null),
            "{ps:?}"
        );
        assert!(view::contract(&sch, &st, &v));
        eprintln!("n = {n}: on {added:?}, off {removed:?}");
        seen.push((added, removed));
    }
    assert_eq!(seen[0], seen[1], "a toggle reads what it read at a sixteenth of the size");
    let on = Reads { gets: 1, rows: 1 };
    let off = Reads { gets: 1, rows: 0 };
    assert_eq!(
        seen[0],
        (on, off),
        "the media row by key, and the item under (playlist, media) if there is one"
    );
}

const ROUNDS: usize = 201;

fn median(mut xs: Vec<Duration>) -> Duration {
    xs.sort();
    xs[xs.len() / 2]
}

/// The cost of one toggle pushed through the `library` view, at four
/// sizes: the median of `ROUNDS` adds and of `ROUNDS` removes, each pushed
/// against the store after it. Flat is the claim (§1.5, "Cost").
#[test]
#[ignore]
fn bench_toggle() {
    let (sch, plan, env) = library_view();
    eprintln!(
        "{:>7} {:>10} {:>10} {:>10} {:>12}",
        "media", "hydrate", "toggle on", "toggle off", "after a walk"
    );
    for n in [250, 1000, 4000, 16000] {
        let mut st = seeded(&sch, n);
        let t0 = Instant::now();
        let mut v = view::hydrate(&sch, &plan, env.clone(), &st).expect("hydrate");
        let hydrate = t0.elapsed();
        let (on, off) = toggle(n);
        let (mut ons, mut offs, mut walked) = (vec![], vec![], vec![]);
        for round in 0..ROUNDS * 2 {
            // The second half walks every table before each push, as
            // harken's `bench_views` does: what a push costs with the
            // view's own structures out of the cache.
            let walk = round >= ROUNDS;
            for (i, ch) in [&on, &off].into_iter().enumerate() {
                st.apply_change(ch);
                if walk {
                    for t in sch.tables() {
                        std::hint::black_box(st.scan(&t.name));
                    }
                }
                let t = Instant::now();
                let ps = view::push_all(&sch, &st, std::slice::from_ref(ch), &mut v).expect("push");
                let took = t.elapsed();
                assert_eq!(ps.len(), 1);
                match (walk, i) {
                    (true, _) => walked.push(took),
                    (false, 0) => ons.push(took),
                    (false, _) => offs.push(took),
                }
            }
        }
        eprintln!(
            "{n:>7} {:>10.1?} {:>10.1?} {:>10.1?} {:>12.1?}",
            hydrate,
            median(ons),
            median(offs),
            median(walked)
        );
    }
}
