//! Every harken query, maintained (§1.10): `harken/domain/tests/views.rs`,
//! one test per query, held to V1's contract — after every change the
//! view's answer is a fresh evaluation's (`ark::view::query`) and its
//! patches splice the list a screen holds into the new one.
//!
//! The library is seeded through the domain's own functions, as v4's was,
//! and then churned — where v4 churned raw rows (`ark/tests/support/
//! churn.rs`, not ported), here by the domain's own mutations drawn by a
//! seeded generator: songs added, retagged onto other albums and works and
//! removed, playlists made, toggled and filled, people and works and
//! recordings described and credited — so every change is one harken
//! writes. v4's tallies of what a churn hit (joins, having flips, group
//! moves) are its generator's; here each query is held to have been
//! patched, by inserts, removes or updates, over its seeds.
//!
//! Every test was falsified at once by a `View::push` that changed nothing
//! (`rust/ark/src/view.rs`, broken in the working tree and restored): each
//! failed at the first change its view should have moved by.

mod common;

use ark::interp::V;
use ark::ir::Node;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::store::Store;
use ark::view::{query, splice, Patch, View};
use common::{domain, full, song, track, with, Lib};
use harken_domain::domain::host::LIBRARY;
use harken_domain::domain::Id;
use harken_domain::domain::*;

struct Seeded {
    lib: Lib,
    favs: Id<Playlist>,
    work: String,
    recording: String,
    media: Id<Media>,
}

fn seeded() -> Seeded {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    c.playlist("alice", "Other");
    c.playlist("bob", "Favorites");
    for s in [
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
    ] {
        c.add(s);
    }
    for (t, a, al, f) in [
        ("Glue", "Bicep", "Isles", "p1"),
        ("Opal", "Bicep", "Isles", "p2"),
        ("Gosh", "Jamie xx", "In Colour", "p3"),
        ("Loose", "Jamie xx", "", "p4"),
    ] {
        c.add(with(f, song(t, a, al)));
    }
    let works: Vec<WorkSummary> = c.list(
        "works",
        &Works {
            composer: "Johann Sebastian Bach".into(),
        },
    );
    let work = works.iter().find(|w| w.catalogue == "BWV 988").expect("the Goldberg").id.clone();
    let recording = c.list::<RecordingsEntry>("recordings", &Recordings { work_id: work.clone() })[0]
        .id
        .clone();
    let credit = CreditRecording {
        recording_id: recording.clone(),
        person_name: "Glenn Gould".into(),
        role: "piano".into(),
        instrument: "piano".into(),
        pos: 1,
    };
    c.mutate("alice", "credit_recording", &credit).unwrap();
    let bach = DescribePerson {
        name: "Johann Sebastian Bach".into(),
        sort_name: "Bach, Johann Sebastian".into(),
        born: 1685,
        died: 1750,
        art: "bach.jpg".into(),
    };
    c.mutate("alice", "describe_person", &bach).unwrap();
    let ids: Vec<Id<Media>> = c.library(&favs).iter().map(|r| r.id).collect();
    for i in [0, 3, 6] {
        c.mutate(
            "alice",
            "add_to_playlist",
            &AddToPlaylist {
                playlist_id: favs,
                media_id: ids[i],
            },
        )
        .unwrap();
    }
    Seeded {
        media: ids[0],
        lib: c,
        favs,
        work,
        recording,
    }
}

/// Knuth's MMIX generator, as the simulator's.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as usize
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

/// One of the domain's mutations, drawn: refused or not, what it changed
/// is in `c.last`.
fn churn(c: &mut Lib, r: &mut Rng) {
    let files = ["g1", "g2", "g3", "b1", "w1", "m1", "p1", "p2", "p3", "p4", "n1", "n2"];
    let titles = ["Aria", "Variation 9", "Glue", "Air", "Gigue", "Largo"];
    let artists = ["Johann Sebastian Bach", "George Frideric Handel", "Bicep", "Jamie xx", ""];
    let albums = ["Goldberg Variations", "Isles", "Water Music", "In Colour", "Messiah", ""];
    let catalogues = ["BWV 988", "BWV 1046", "HWV 348", "", ""];
    let people = ["Glenn Gould", "Kimiko Ishizaka", "Johann Sebastian Bach", "Somebody"];
    let media: Vec<Id<Media>> = c.rows::<Media>("media").into_iter().map(|m| m.id).collect();
    let recordings: Vec<String> = c.rows::<Recording>("recording").into_iter().map(|r| r.id).collect();
    let mine: Vec<Id<Playlist>> = c
        .rows::<Playlist>("playlist")
        .into_iter()
        .filter(|p| p.user_id == "alice")
        .map(|p| p.id)
        .collect();
    let some_media = |r: &mut Rng| media.get(r.below(media.len().max(1))).cloned();
    c.last.clear();
    let out = match r.below(9) {
        0 | 1 => {
            let s = AddSong {
                file: r.pick(&files).into(),
                track: r.below(4) as i64,
                movement_no: r.below(3) as i64,
                catalogue: r.pick(&catalogues).into(),
                performer: r.pick(&people).into(),
                album_art: r.pick(&["", "a.jpg", "b.jpg"]).into(),
                ..song(r.pick(&titles), r.pick(&artists), r.pick(&albums))
            };
            c.mutate("alice", "add_song", &s)
        }
        2 => c.mutate(
            "alice",
            "create_playlist",
            &CreatePlaylist {
                name: r.pick(&["Favorites", "Late", "Mix"]).into(),
            },
        ),
        3 | 4 => match (some_media(r), mine.get(r.below(mine.len().max(1)))) {
            (Some(m), Some(p)) => c.mutate(
                "alice",
                "add_to_playlist",
                &AddToPlaylist {
                    playlist_id: *p,
                    media_id: m,
                },
            ),
            _ => Ok(0),
        },
        5 => match (some_media(r), mine.get(r.below(mine.len().max(1)))) {
            (Some(m), Some(p)) => c.mutate(
                "alice",
                "remove_from_playlist",
                &RemoveFromPlaylist {
                    playlist_id: *p,
                    media_id: m,
                },
            ),
            _ => Ok(0),
        },
        6 => match some_media(r) {
            Some(id) if r.below(2) == 0 => c.mutate("alice", "remove_media", &RemoveMedia { id }),
            _ => {
                let p = DescribePerson {
                    name: r.pick(&people).into(),
                    art: r.pick(&["", "x.jpg", "y.jpg"]).into(),
                    ..DescribePerson::default()
                };
                c.mutate("alice", "describe_person", &p)
            }
        },
        7 => match recordings.get(r.below(recordings.len().max(1))) {
            Some(id) => {
                let pos = r.below(3) as i64;
                let cr = CreditRecording {
                    recording_id: id.clone(),
                    person_name: r.pick(&people).into(),
                    role: r.pick(&["piano", "", "conductor"]).into(),
                    instrument: String::new(),
                    pos,
                };
                c.mutate("alice", "credit_recording", &cr)
            }
            None => Ok(0),
        },
        _ => match recordings.get(r.below(recordings.len().max(1))) {
            Some(id) => c.mutate(
                "alice",
                "describe_recording",
                &DescribeRecording {
                    id: id.clone(),
                    recorded: r.below(3) as i64 * 1000,
                    licence: r.pick(&["", "CC0"]).into(),
                    ..DescribeRecording::default()
                },
            ),
            None => Ok(0),
        },
    };
    if out.is_err() {
        c.last.clear();
    }
}

const SEEDS: u64 = 6;
const STEPS: usize = 40;

/// How a view was patched, over every seed.
#[derive(Debug, Default)]
struct Tally {
    pushes: usize,
    inserts: usize,
    removes: usize,
    updates: usize,
}

/// The query `name` on `input` as alice, maintained over every seed's
/// churn: after every push the view is the query, and the patches splice.
fn hold(name: &str, input: &impl Node) -> Tally {
    let d = domain();
    let (f, input) = (d.get(&full(name)), input.to_bytes());
    let ctx = Ctx {
        id: EntryId::of("q", 0),
        actor: "alice".into(),
        session: "q".into(),
        roles: vec![],
        now: 0,
    };
    let mut sum = Tally::default();
    for seed in 0..SEEDS {
        let mut c = seeded().lib;
        let mut v = View::new(&d.lib, &f, &input, ctx.clone(), &c.store).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut shown = v.rows();
        assert!(seed > 0 || !shown.is_empty(), "{name}: the seed answers something to maintain");
        let mut r = Rng(seed);
        for step in 0..STEPS {
            churn(&mut c, &mut r);
            let ps = v
                .push(&c.last, &c.store)
                .unwrap_or_else(|e| panic!("{name} seed {seed} step {step}: {e}"));
            sum.pushes += 1;
            for p in &ps {
                match p {
                    Patch::Insert { .. } => sum.inserts += 1,
                    Patch::Remove { .. } => sum.removes += 1,
                    Patch::Update { .. } => sum.updates += 1,
                }
            }
            shown = splice(&ps, &shown);
            let fresh = query(&d.lib, &f, &input, &ctx, &c.store).unwrap();
            assert_eq!(v.rows(), fresh, "{name} seed {seed} step {step}: the view is the query");
            assert_eq!(shown, fresh, "{name} seed {seed} step {step}: the patches splice");
        }
    }
    eprintln!("{name}: {sum:?}");
    sum
}

fn moved(t: &Tally) -> bool {
    t.inserts > 0 && t.removes > 0 && t.updates > 0
}

/// v4's search box, for a needle in titles, one folded from capitals, and
/// one of one character. Also falsified by `has` folding the text and not
/// the needle: "BACH" answered nothing.
#[test]
fn search() {
    let s = seeded();
    for needle in ["ari", "BACH", "a"] {
        let t = hold(
            "search",
            &Search {
                playlist_id: s.favs,
                needle: needle.into(),
            },
        );
        assert!(t.inserts + t.removes + t.updates > 0, "{needle}: {t:?}");
    }
    let hits: Vec<LibraryEntry> = s.lib.list(
        "search",
        &Search {
            playlist_id: s.favs,
            needle: "BACH".into(),
        },
    );
    assert_eq!(hits.len(), 4, "every track whose creator holds it");
}

#[test]
fn library() {
    let s = seeded();
    assert!(moved(&hold("library", &Library { playlist_id: s.favs })));
}

#[test]
fn albums() {
    assert!(moved(&hold("albums", &())));
}

#[test]
fn artists() {
    assert!(moved(&hold("artists", &())));
}

#[test]
fn track_details() {
    assert!(moved(&hold("track_details", &())));
}

#[test]
fn album() {
    let s = seeded();
    let t = hold(
        "album",
        &AlbumInput {
            playlist_id: s.favs,
            name: "Goldberg Variations".into(),
        },
    );
    assert!(t.inserts + t.removes > 0 && t.updates > 0, "{t:?}");
}

#[test]
fn artist() {
    let s = seeded();
    let t = hold(
        "artist",
        &Artist {
            playlist_id: s.favs,
            name: "Bicep".into(),
        },
    );
    assert!(t.inserts + t.removes + t.updates > 0, "{t:?}");
}

#[test]
fn composers() {
    let t = hold("composers", &());
    assert!(t.updates > 0, "{t:?}");
}

#[test]
fn works() {
    let t = hold(
        "works",
        &Works {
            composer: "Johann Sebastian Bach".into(),
        },
    );
    assert!(t.inserts + t.removes > 0 && t.updates > 0, "{t:?}");
}

#[test]
fn work() {
    let s = seeded();
    let t = hold("work", &WorkInput { id: s.work });
    assert!(t.updates > 0, "{t:?}");
}

#[test]
fn recordings() {
    let s = seeded();
    let t = hold("recordings", &Recordings { work_id: s.work });
    assert!(t.inserts + t.removes + t.updates > 0, "{t:?}");
}

#[test]
fn credits() {
    let s = seeded();
    let t = hold("credits", &Credits { recording_id: s.recording });
    assert!(t.inserts + t.removes > 0, "{t:?}");
}

#[test]
fn recording() {
    let s = seeded();
    let t = hold(
        "recording",
        &RecordingInput {
            playlist_id: s.favs,
            id: s.recording,
        },
    );
    assert!(t.inserts + t.removes + t.updates > 0, "{t:?}");
}

#[test]
fn playlists() {
    let t = hold("playlists", &());
    assert!(t.inserts > 0, "{t:?}");
}

#[test]
fn playlists_of() {
    let s = seeded();
    let t = hold("playlists_of", &PlaylistsOf { media_id: s.media });
    assert!(t.inserts > 0 && t.removes > 0, "{t:?}");
}

#[test]
fn playlist() {
    let s = seeded();
    assert!(moved(&hold("playlist", &PlaylistInput { playlist_id: s.favs })));
}

/// v4's `tests/view.rs` through the domain's own mutations: the library
/// read against a playlist, each mutation's changes pushed as one batch,
/// equal to the query and to its own splice after every step; then a
/// rollback (what a rebase is), after which a view made again is the query.
#[test]
fn the_library_follows_the_mutations_and_a_rollback() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    let other = c.playlist("alice", "Other");
    let d = domain();
    let (f, input) = (d.get(&full("library")), Library { playlist_id: favs }.to_bytes());
    let ctx = Ctx {
        id: EntryId::of("q", 0),
        actor: "alice".into(),
        session: "q".into(),
        roles: vec![],
        now: 0,
    };
    let mut v = View::new(&d.lib, &f, &input, ctx.clone(), &c.store).unwrap();
    let mut shown = v.rows();
    let mut patched = 0;
    let mut kept: Option<Store> = None;
    let mut step = |c: &mut Lib, name: &str, bytes: Vec<u8>| {
        let author = c.ctx("alice", &[LIBRARY]);
        c.mutate_bytes(&author, name, &bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let ps = v.push(&c.last, &c.store).unwrap();
        patched += ps.len();
        shown = splice(&ps, &shown);
        let fresh: Vec<V> = query(&d.lib, &f, &input, &ctx, &c.store).unwrap();
        assert_eq!(v.rows(), fresh, "{name}: the view is the query");
        assert_eq!(shown, fresh, "{name}: the spliced list is the query");
        if kept.is_none() && name == "add_to_playlist" {
            kept = Some(c.store.clone());
        }
    };
    for (t, file) in [("Glue", "a"), ("Opal", "b"), ("Gosh", "c"), ("Air", "d")] {
        step(&mut c, "add_song", with(file, song(t, "X", "Y")).to_bytes());
    }
    let ids: Vec<Id<Media>> = c.library(&favs).iter().map(|r| r.id).collect();
    let on = |p: &Id<Playlist>, m: &Id<Media>| {
        AddToPlaylist {
            playlist_id: *p,
            media_id: *m,
        }
        .to_bytes()
    };
    step(&mut c, "add_to_playlist", on(&favs, &ids[2]));
    step(&mut c, "add_to_playlist", on(&favs, &ids[0]));
    step(&mut c, "add_to_playlist", on(&other, &ids[1]));
    step(&mut c, "remove_from_playlist", on(&favs, &ids[2]));
    step(&mut c, "add_all_to_playlist", AddAllToPlaylist { playlist_id: favs }.to_bytes());
    step(&mut c, "remove_media", RemoveMedia { id: ids[0] }.to_bytes());
    step(&mut c, "add_song", with("e", song("Late", "Z", "")).to_bytes());
    assert!(patched > 0);
    let entries: Vec<LibraryEntry> = shown.iter().map(|r| LibraryEntry::from_bytes(&r.to_bytes()).unwrap()).collect();
    let got: Vec<(String, Option<i64>)> = entries.iter().map(|e| (e.title.clone(), e.playlist_pos)).collect();
    assert_eq!(
        got,
        [
            ("Opal".into(), Some(3)),
            ("Gosh".into(), Some(4)),
            ("Air".into(), Some(5)),
            ("Late".into(), None)
        ]
    );

    // A rollback: the view made again over the store as it was is the query.
    c.store = kept.expect("a store to roll back to");
    let v = View::new(&d.lib, &f, &input, ctx.clone(), &c.store).unwrap();
    assert_eq!(v.rows(), query(&d.lib, &f, &input, &ctx, &c.store).unwrap());
    assert_eq!(v.rows().len(), c.library(&favs).len());
}
