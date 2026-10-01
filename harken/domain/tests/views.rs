//! Every harken query, maintained (`docs/plan-v4.md` §1.5): the contract
//! held under a seeded generator of change sequences, one test per query.
//!
//! The library is seeded through the domain's own mutations (`common`), so
//! the rows are the rows harken writes — albums, people, works, movements,
//! recordings, credits, playlists — and then churned as raw rows by
//! `rust/ark/tests/support/churn.rs`: adds, edits and removes across every
//! table the query's plan reads, rows that join and rows that do not,
//! group-key moves, having flips, and batches of several changes at once.
//! After every batch the view is a fresh hydrate, its answer is what the
//! plan reads now, and the patches splice the old answer into the new; a
//! failure names the query, the seed, the step and the batch.
//!
//! The middleware is the client's business (§1.7), so here it runs once at
//! seed time and the plan is maintained in the scope it gave. The library
//! is also followed through the domain's own mutations — the playlist's
//! toggles among them — and through a rollback, which is what the view it
//! replaces (`tests/view.rs`, the plan hack) held.

mod common;

#[path = "../../../rust/ark/tests/support/churn.rs"]
mod churn;

use ark::eval::{self, Args, Ctx};
use ark::store::MemoryStore;
use ark::value::{Id, Value};
use ark::view::{self, Env};
use churn::{drive, Tally};
use common::{args, track, Lib, Song};

/// A seeded library and the arguments each query is asked with.
struct Seeded {
    lib: Lib,
    favs: Id,
    work: Value,
    recording: Value,
    media: Value,
}

fn seeded() -> Seeded {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    c.playlist("alice", "Other");
    c.playlist("bob", "Favorites");
    let classical = [
        track(
            "Aria",
            "Johann Sebastian Bach",
            "Goldberg Variations",
            "BWV 988",
            "Glenn Gould",
            "g1",
            1,
            "Goldberg Variations",
            1,
        ),
        track(
            "Variation 1",
            "Johann Sebastian Bach",
            "Goldberg Variations",
            "BWV 988",
            "Glenn Gould",
            "g2",
            2,
            "Goldberg Variations",
            2,
        ),
        track(
            "Variation 2",
            "Johann Sebastian Bach",
            "Goldberg Variations",
            "BWV 988",
            "Kimiko Ishizaka",
            "g3",
            3,
            "Goldberg Variations",
            3,
        ),
        track(
            "Allegro",
            "Johann Sebastian Bach",
            "Brandenburg Concertos",
            "BWV 1046",
            "The English Concert, Trevor Pinnock",
            "b1",
            1,
            "Brandenburg Concerto No. 1",
            1,
        ),
        track(
            "Overture",
            "George Frideric Handel",
            "Water Music",
            "HWV 348",
            "",
            "w1",
            1,
            "Water Music Suite No. 1",
            1,
        ),
        track(
            "Hallelujah",
            "George Frideric Handel",
            "Messiah",
            "HWV 56",
            "London Symphony Orchestra, Hermann Scherchen",
            "m1",
            44,
            "Messiah",
            44,
        ),
    ];
    for s in classical {
        c.add(s);
    }
    for (t, a, al, f) in [
        ("Glue", "Bicep", "Isles", "p1"),
        ("Opal", "Bicep", "Isles", "p2"),
        ("Gosh", "Jamie xx", "In Colour", "p3"),
        ("Loose", "Jamie xx", "", "p4"),
    ] {
        c.add(Song {
            file: f.into(),
            ..Song::new(t, a, al)
        });
    }
    let works = c.list("works", args([("composer", Value::text("Johann Sebastian Bach"))]));
    let work = works
        .iter()
        .find(|w| w.field("catalogue") == Value::text("BWV 988"))
        .expect("the Goldberg")
        .field("id");
    let recording = c.list("recordings", args([("work_id", work.clone())]))[0].field("id");
    c.mutate(
        "alice",
        "credit_recording",
        args([
            ("recording_id", recording.clone()),
            ("person_name", Value::text("Glenn Gould")),
            ("role", Value::text("piano")),
            ("instrument", Value::text("piano")),
            ("pos", Value::int(1)),
        ]),
    )
    .unwrap();
    c.mutate(
        "alice",
        "describe_person",
        args([
            ("name", Value::text("Johann Sebastian Bach")),
            ("sort_name", Value::text("Bach, Johann Sebastian")),
            ("born", Value::int(1685)),
            ("died", Value::int(1750)),
            ("art", Value::text("bach.jpg")),
        ]),
    )
    .unwrap();
    let ids: Vec<Value> = c.library(favs).iter().map(|r| r.field("id")).collect();
    for i in [0, 3, 6] {
        c.mutate(
            "alice",
            "add_to_playlist",
            args([("playlist_id", Value::Id(favs)), ("media_id", ids[i].clone())]),
        )
        .unwrap();
    }
    Seeded {
        media: ids[0].clone(),
        lib: c,
        favs,
        work,
        recording,
    }
}

const SEEDS: u64 = 6;
const STEPS: usize = 40;

// Every seed of one query, asked as alice, summed.
fn hold(name: &str, a: Args) -> Tally {
    let s = seeded();
    let m = harken_domain::module();
    let built = m.build();
    let f = built.lookup_function(name).unwrap_or_else(|| panic!("no query {name}"));
    let c = ark::hash::closure(built, f);
    let ctx = Ctx::new("alice", "s");
    let (args, provided) = eval::middleware(&built.schema, &c, &ctx, &a, &s.lib.store).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    let env = Env {
        helpers: c.helpers.clone(),
        ctx,
        args,
        provided,
    };
    let plan = f.plan.clone().expect("a query is a plan");
    let first = view::read(&built.schema, &plan, &env.scope(&built.schema), &s.lib.store).unwrap();
    assert!(!first.is_empty(), "{name}: the seed answers something to maintain");
    let mut sum = Tally::default();
    for seed in 0..SEEDS {
        let t = drive(name, &built.schema, &plan, &env, s.lib.store.clone(), seed, STEPS);
        sum.steps += t.steps;
        sum.batches += t.batches;
        sum.inserts += t.inserts;
        sum.removes += t.removes;
        sum.updates += t.updates;
        sum.joined += t.joined;
        sum.unjoined += t.unjoined;
        sum.flips += t.flips;
        sum.moves += t.moves;
        sum.window += t.window;
    }
    eprintln!("{name}: {sum:?}");
    assert!(sum.batches > 0 && sum.inserts + sum.removes + sum.updates > 0, "{name}: {sum:?}");
    sum
}

fn with_favs(s: &Seeded, more: &[(&str, Value)]) -> Args {
    let mut a = args([("playlist_id", Value::Id(s.favs))]);
    for (k, v) in more {
        a.insert(k.to_string(), v.clone());
    }
    a
}

/// `docs/plan-db.md` D4 The search box's query: a `Has` on the title or the
/// creator, read through the text indexes on both and maintained as any
/// filter is — a row whose title or creator is edited is re-admitted by the
/// predicate — for a needle the indexes serve, one folded from capitals, and
/// one too short to have a trigram. Falsified by reading an `or` of `Has`es
/// through its first branch alone (`ark::view::needles`): "BACH", which
/// only creators hold, answers nothing at all.
#[test]
fn search() {
    let s = seeded();
    for needle in ["ari", "BACH", "a"] {
        let t = hold("search", with_favs(&s, &[("needle", Value::text(needle))]));
        assert!(t.joined > 0 && t.updates + t.inserts + t.removes > 0, "{needle}: {t:?}");
    }
}

/// Falsified by recording no dependency for a `Related` node in
/// `ark::view::entry` (a playlist entry added leaves `playlist_pos` stale).
#[test]
fn library() {
    let s = seeded();
    let t = hold("library", with_favs(&s, &[]));
    assert!(t.joined > 0 && t.unjoined > 0 && t.updates > 0, "{t:?}");
}

/// A having over a related list whose entries each look up their media:
/// an album gaining its first song, or losing its last. Falsified by
/// recording no dependency for a `Lookup` node (a media's creator edited
/// leaves the album's `creator` stale).
#[test]
fn albums() {
    let t = hold("albums", args([]));
    assert!(t.joined > 0 && t.flips > 0 && t.updates > 0, "{t:?}");
}

/// A group source: a media's creator edited moves it between groups.
/// Falsified by not removing the old row from its group's members.
#[test]
fn artists() {
    let t = hold("artists", args([]));
    assert!(t.moves > 0 && t.joined > 0, "{t:?}");
}

/// Three lookups, one through another, and a related list. Falsified as
/// `albums` is.
#[test]
fn track_details() {
    let t = hold("track_details", args([]));
    assert!(t.joined > 0 && t.unjoined > 0 && t.updates > 0, "{t:?}");
}

/// Expression order keys over lookups, a having on one. Falsified by
/// comparing entries by key alone in `View::position`.
#[test]
fn album() {
    let s = seeded();
    let t = hold("album", with_favs(&s, &[("name", Value::text("Goldberg Variations"))]));
    assert!(t.joined > 0 && t.flips > 0, "{t:?}");
}

/// Falsified as `library` is.
#[test]
fn artist() {
    let s = seeded();
    let t = hold("artist", with_favs(&s, &[("name", Value::text("Bicep"))]));
    assert!(t.joined > 0 && t.updates > 0, "{t:?}");
}

/// Related three deep under a having: a composer losing their last work.
/// Falsified by carrying no child entry's dependencies up to its parent.
#[test]
fn composers() {
    let t = hold("composers", args([]));
    assert!(t.joined > 0 && t.flips > 0 && t.updates > 0, "{t:?}");
}

/// Falsified as `composers` is.
#[test]
fn works() {
    let t = hold("works", args([("composer", Value::text("Johann Sebastian Bach"))]));
    assert!(t.joined > 0 && t.updates > 0, "{t:?}");
}

/// One row by key, and what hangs beneath it. Falsified as `composers` is.
#[test]
fn work() {
    let s = seeded();
    let t = hold("work", args([("id", s.work)]));
    assert!(t.joined > 0 && t.updates > 0, "{t:?}");
}

/// An expression order over a related list's length. Falsified by
/// recording no dependency for a `Related` node (a song added to the
/// recording leaves its `tracks` short).
#[test]
fn recordings() {
    let s = seeded();
    let t = hold("recordings", args([("work_id", s.work)]));
    assert!(t.joined > 0 && t.updates > 0, "{t:?}");
}

/// A sibling related on the source table itself, and a having over it:
/// the first real credit displaces the lumped one. Falsified by keying
/// dependency hits on the new row alone (a credit's `pos` lowered to 0
/// leaves the lumped credit refused).
#[test]
fn credits() {
    let s = seeded();
    let t = hold("credits", args([("recording_id", s.recording)]));
    assert!(t.joined > 0 && t.flips > 0, "{t:?}");
}

/// Falsified as `album` is.
#[test]
fn recording() {
    let s = seeded();
    let t = hold("recording", with_favs(&s, &[("id", s.recording.clone())]));
    assert!(t.joined > 0 && t.updates > 0, "{t:?}");
}

/// A bare plan filtered by the user. Falsified by admitting every source
/// row in `touched` (bob's playlists appear).
#[test]
fn playlists() {
    let t = hold("playlists", args([]));
    assert!(t.inserts > 0 && t.removes > 0 && t.updates > 0, "{t:?}");
}

/// A having over a related list filtered by the input. Falsified by
/// settling a refused entry as admitted.
#[test]
fn playlists_of() {
    let s = seeded();
    let t = hold("playlists_of", args([("media_id", s.media)]));
    assert!(t.joined > 0 && t.flips > 0, "{t:?}");
}

/// Under `owned`: the plan filtered by what the middleware provided, a
/// lookup and a having on it. Falsified as `albums` is.
#[test]
fn playlist() {
    let s = seeded();
    let t = hold("playlist", with_favs(&s, &[]));
    assert!(t.joined > 0 && t.flips > 0, "{t:?}");
}

/// What `tests/view.rs` held, through the domain's own mutations: the
/// library read against a playlist, pushed each mutation's changes as one
/// batch — songs added, the playlist's own toggles, another playlist's
/// entries (nothing to this view), everything onto the playlist at once, a
/// song removed from the library and its entries with it — equal to the
/// query and to its own splice after every step; then a rollback (what a
/// rebase is), after which a rebuild is the query again. Falsified by
/// recording no dependency for a `Related` node (the first toggle leaves
/// `playlist_pos` stale).
#[test]
fn the_library_follows_the_mutations_and_a_rollback() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    let other = c.playlist("alice", "Other");
    let m = harken_domain::module();
    let built = m.build();
    let f = built.lookup_function("library").unwrap();
    let cl = ark::hash::closure(built, f);
    let sch = &built.schema;
    let env = Env {
        helpers: cl.helpers.clone(),
        args: args([("playlist_id", Value::Id(favs))]),
        ..Env::default()
    };
    let plan = f.plan.clone().unwrap();
    let mut v = view::hydrate(sch, &plan, env, &c.store).unwrap();
    let mut shown = v.rows();
    let mut patched = 0;
    let mut kept: Option<MemoryStore> = None;

    let mut step = |c: &mut Lib, name: &str, a: Args| {
        c.mutate("alice", name, a).unwrap_or_else(|e| panic!("{name}: {e}"));
        let ps = view::push_all(sch, &c.store, &c.last, &mut v).unwrap();
        patched += ps.len();
        shown = view::splice(&ps, &shown);
        assert!(view::contract(sch, &c.store, &v), "{name}: the view is a fresh hydrate");
        assert_eq!(shown, c.library(favs), "{name}: the spliced list is the query");
        if kept.is_none() && name == "add_to_playlist" {
            kept = Some(c.store.clone());
        }
    };
    for (t, f) in [("Glue", "a"), ("Opal", "b"), ("Gosh", "c"), ("Air", "d")] {
        step(
            &mut c,
            "add_song",
            Song {
                file: f.into(),
                ..Song::new(t, "X", "Y")
            }
            .args(),
        );
    }
    let ids: Vec<Value> = c.library(favs).iter().map(|r| r.field("id")).collect();
    let on = |p, m: &Value| args([("playlist_id", Value::Id(p)), ("media_id", m.clone())]);
    step(&mut c, "add_to_playlist", on(favs, &ids[2]));
    step(&mut c, "add_to_playlist", on(favs, &ids[0]));
    step(&mut c, "add_to_playlist", on(other, &ids[1]));
    step(&mut c, "remove_from_playlist", on(favs, &ids[2]));
    step(&mut c, "add_all_to_playlist", args([("playlist_id", Value::Id(favs))]));
    step(&mut c, "remove_media", args([("id", ids[0].clone())]));
    step(
        &mut c,
        "add_song",
        Song {
            file: "e".into(),
            ..Song::new("Late", "Z", "")
        }
        .args(),
    );
    assert!(patched > 0);
    let titles: Vec<(String, Value)> = shown
        .iter()
        .map(|r| (r.field("title").as_text().to_string(), r.field("playlist_pos")))
        .collect();
    assert_eq!(
        titles,
        [
            ("Opal".into(), Value::int(3)),
            ("Gosh".into(), Value::int(4)),
            ("Air".into(), Value::int(5)),
            ("Late".into(), Value::Null)
        ]
    );

    // A rollback reports no changes: the view is rebuilt, and is the query.
    c.store = kept.expect("a store to roll back to");
    let v = view::rebuild(sch, &c.store, &v).unwrap();
    assert_eq!(v.rows(), c.library(favs));
    assert!(view::contract(sch, &c.store, &v));
}
