//! harken's domain against itself: the module it emits verifies and is the
//! committed `harken.ark`, and every procedure run natively agrees with the
//! interpreter over its own emit — verdicts, changes, stores and values —
//! step after step over one evolving store, every procedure at least once.

mod common;

use std::path::Path;

use ark::ir::{Expr, FnKind, StdFn, Stmt};
use ark::value::Value;
use common::{args, track, Lib, Song};
use harken_domain::module;

#[test]
fn the_module_verifies_and_is_the_committed_file() {
    let m = module();
    let bytes = m.emit();
    let decoded = ark::ir::module_from_value(&ark::canon::decode(&bytes).unwrap()).unwrap();
    let verified = ark::verify::verify(&decoded).unwrap_or_else(|es| panic!("{es:?}"));
    assert_eq!(
        ark::ir::module_value(&verified),
        ark::ir::module_value(&decoded),
        "emit is already the verified form"
    );
    let names: Vec<&str> = decoded.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "work_title",
            "slug",
            "key_part",
            "work_key",
            "work_id",
            "movement_key",
            "recording_key",
            "recording_id",
            "credited_as",
            "movement_id",
            "add_song",
            "describe_work",
            "describe_recording",
            "describe_person",
            "credit_recording",
            "remove_media",
            "library_entry",
            "library",
            // `docs/plan-db.md` D4: the search box's query.
            "search",
            "albums",
            "artists",
            "credited",
            "joined",
            "performers",
            "track_details",
            "album",
            "artist",
            // At v4 `composers` counts down its plan's tree and sums with
            // `total`, where it asked `tracks_on` of two whole tables.
            "total",
            "composers",
            "work_summary",
            "works",
            "work",
            "recordings",
            "credits",
            "recording",
            "owned",
            "numbered",
            "free_number",
            "playlist_name",
            "create_playlist",
            "add_to_playlist",
            "add_all_to_playlist",
            "remove_from_playlist",
            "playlists",
            "playlists_of",
            "playlist",
        ],
        "a helper sits immediately before the first function that calls it"
    );
    // Helpers are pure and say what they are; each is called by name.
    let slug = decoded.lookup_function("slug").unwrap();
    assert_eq!(slug.kind, FnKind::Helper);
    assert_eq!(slug.router, None);
    assert_eq!(slug.arg_types(), [("text".to_string(), ark::schema::Ty::Text)]);
    let key_part = decoded.lookup_function("key_part").unwrap();
    assert!(
        ark::ir::calls(key_part).contains(&"slug".to_string()),
        "key_part calls slug: {:?}",
        ark::ir::calls(key_part)
    );
    // A record is a struct type that is no table's row.
    let albums = decoded.lookup_function("albums").unwrap();
    let Some(ark::schema::Ty::List(entry)) = &albums.ret else {
        panic!("{:?}", albums.ret)
    };
    let ark::schema::Ty::Struct(fields) = &**entry else {
        panic!("{entry:?}")
    };
    assert_eq!(fields.keys().collect::<Vec<_>>(), ["art", "creator", "name", "tracks"]);
    // One auto per name, however often a body reads it.
    let add = decoded.lookup_function("add_song").unwrap();
    let autos: Vec<&str> = add.autos.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(autos, ["id", "added_ms"]);

    assert_eq!(decoded.lookup_function("add_to_playlist").unwrap().uses, ["owned"]);
    assert!(decoded.lookup_function("create_playlist").unwrap().uses.is_empty());
    assert_eq!(decoded.lookup_router("playlists").unwrap().uses, ["owned"]);
    assert!(decoded.lookup_router("library").unwrap().uses.is_empty());
    // §6: a provide returns `or_refuse`'s value whole, `EStd Unwrap [EVar s]`.
    let owned = decoded.lookup_function("owned").unwrap();
    assert!(
        matches!(owned.body.last(), Some(Stmt::Return(Some(Expr::Std(StdFn::Unwrap, xs)))) if matches!(xs[..], [Expr::Var(_)])),
        "{:?}",
        owned.body.last()
    );
    let committed = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("harken.ark")).unwrap();
    assert!(
        committed == bytes,
        "harken/domain/harken.ark is stale: `cargo run -p harken-domain -- ../harken/domain/harken.ark` rewrites it"
    );
}

/// §1.8 Spec v4 changed what a query is and nothing about a mutator: the
/// log names every mutator by its closure's hash, so each is pinned here to
/// the hash it had at spec v3 (`f45790f`, before any of v4 landed), both as
/// the domain builds it and as the committed `harken.ark` carries it. The
/// Haskell `arkc check` refuses a spec-4 module outright (`BadSpecVersion
/// 4`), so this is where the check the spec asks for (§1.8) is made.
/// Falsified by writing a plan's `row` key even when it is absent (the
/// encoder's `if let Some(r) = p.row`): every mutator that reads moves.
///
/// Two have moved since, on purpose, and are pinned to their new hashes:
/// `add_song` and `create_playlist`, in the commit "harken-domain: slug in
/// one pass, key_part once, playlist_name lazy (R1)" (`docs/plan-perf.md`
/// R1). The helpers they reach (`slug`, `key_part`, `playlist_name`) got
/// cheaper bodies that answer the same (`keys::tests`), and a closure is
/// its helpers too. At v3 they were `32150bd3…6eef` and `1dade18e…9e90a`;
/// a log's entries naming those keep naming them, and run the closures the
/// authority kept for them, which still verify against this schema
/// (`compat::check_retained`, old module against new, found nothing).
///
/// `create_playlist` has moved once more, in the commit "harken-domain:
/// create_playlist reads its name's siblings, add_song derives once
/// (R6)" (`docs/plan-perf.md` R6): it reads the person's names from the
/// name up to `name )` rather than all of them, a range of `(user_id,
/// name)`. It was `904226b2…9c08` after R1. `compat::check` of the module
/// before against this one found nothing, and `check_retained` of all
/// forty-five of the earlier module's closures against this schema found
/// nothing. `add_song` binds what it derives once each natively and
/// emits the same bytes (§6 inlines a pure `let`), so it did not move.
#[test]
fn every_mutator_hashes_as_it_did_at_spec_v3() {
    let v3 = [
        ("add_song", "1633aca2eb971dd1d5cc829e5d7a2bb6f33eec6f28e0b2268e3a40a7dc8795a7"),
        ("describe_work", "a74fc779a35bbd4ee5abf56d3c1c4c4a80abea624e5ea8f47083fc136d39ab8d"),
        ("describe_recording", "4edffc9e12721bf58d4a2ab0a61f252c615f2dbda44559e5c22eece4e270f98f"),
        ("describe_person", "15508559e7c540069012698099398b0f74fdb7f66fa59f0ebaf6527be5728692"),
        ("credit_recording", "3e10d18b33c7b703822cb4969ed8b8c483e001f4f882864325893580c780546e"),
        ("remove_media", "885266129e9c3955a8dc12270305b1843e07e43ff195588c3d0cd08bbd02c06b"),
        ("create_playlist", "2eb47c7fe50b8b6d4aca5816c0f1127dbf08ca4b05adffac589fbab2d5edf005"),
        ("add_to_playlist", "29ef6578cbda8f224d8b279461e1544a8fcf910ee259c32379edcbc4da32d2c1"),
        ("add_all_to_playlist", "ca05acf23e131c0ffbecb7f302224cd690a2da78a9d41ef10185dcc846d1587a"),
        ("remove_from_playlist", "e6b2807ee3556a5e85dbab34795fb8abdbfe951d3fa994065fc2542cd6e2230e"),
    ];
    let built = module();
    let committed = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("harken.ark")).unwrap();
    let decoded = ark::ir::module_from_value(&ark::canon::decode(&committed).unwrap()).unwrap();
    for m in [built.build(), &decoded] {
        let mutators: Vec<&str> = m
            .functions
            .iter()
            .filter(|f| f.kind == FnKind::Mutator)
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(mutators, v3.map(|(n, _)| n), "the same mutators, in the same order");
        for (name, hash) in v3 {
            let f = m.lookup_function(name).unwrap();
            let h = ark::hash::function_hash(&ark::hash::closure(m, f));
            assert_eq!(ark::value::hex(&h), hash, "{name}: its closure hashes as pinned");
        }
    }
}

/// Every procedure, natively and through the interpreter, over one store
/// that grows through the whole library shape: refusals, no-ops, the
/// classical chain, pop, playlists, removal.
#[test]
fn every_procedure_agrees_with_the_interpreter() {
    let mut c = Lib::new();
    let favs = c.playlist("alice", "Favorites");
    let evening = c.playlist("alice", "Evening");
    let bobs = c.playlist("bob", "Favorites");
    for (i, (performer, file)) in [("Kimiko Ishizaka", "k"), ("", "x"), ("Glenn Gould", "g")].into_iter().enumerate() {
        for no in 1..=3 {
            let title = ["Aria", "Variatio 1", "Variatio 2"][no as usize - 1];
            c.add(Song {
                album_art: ["", "cover.jpg", ""][i],
                artist_art: ["bach.jpg", "", "bach2.jpg"][i],
                bpm: 60 * no,
                ..track(
                    title,
                    "Johann Sebastian Bach",
                    "Goldberg Variations",
                    "BWV 988",
                    performer,
                    &format!("music/{file}{no}.mp3"),
                    no,
                    ["Goldberg Variations", "", ""][i],
                    [no, 0, no][i],
                )
            });
        }
    }
    c.add(Song {
        part: "Suite No. 2",
        track: 4,
        file: "music/h.mp3".into(),
        ..Song::new("Air", "George Frideric Handel", "Water Music")
    });
    c.add(Song {
        file: "music/p.mp3".into(),
        ..Song::new("Low Tide", "The Quiet Hours", "")
    });
    c.add(Song::new("Hand-typed", "", ""));
    assert_eq!(
        c.mutate("alice", "add_song", Song::new("", "x", "").args()),
        Err("a song needs a title".into())
    );
    // The same file again is nothing at all.
    assert_eq!(
        c.mutate(
            "alice",
            "add_song",
            Song {
                file: "music/p.mp3".into(),
                ..Song::new("Other", "Y", "Z")
            }
            .args()
        ),
        Ok(0)
    );

    let lib = c.library(favs);
    assert_eq!(lib.len(), 12);
    let ids: Vec<Value> = lib.iter().map(|r| r.field("id")).collect();
    let on = |p, m: &Value| args([("playlist_id", Value::Id(p)), ("media_id", m.clone())]);
    for id in ids.iter().step_by(3) {
        c.mutate("alice", "add_to_playlist", on(favs, id)).unwrap();
    }
    assert_eq!(c.mutate("bob", "add_to_playlist", on(favs, &ids[0])), Err("not your playlist".into()));
    assert_eq!(
        c.mutate("alice", "add_to_playlist", on([9; 16], &ids[0])),
        Err("playlist_id: no such playlist".into())
    );
    c.mutate("alice", "add_all_to_playlist", args([("playlist_id", Value::Id(evening))]))
        .unwrap();
    c.mutate("bob", "add_all_to_playlist", args([("playlist_id", Value::Id(bobs))])).unwrap();
    // A name alice already has: numbered, and a playlist of its own.
    let favs_1 = c.playlist("alice", "Favorites");
    c.mutate("alice", "add_to_playlist", on(favs_1, &ids[2])).unwrap();
    c.mutate("alice", "remove_from_playlist", on(evening, &ids[1])).unwrap();
    assert_eq!(
        c.mutate("", "remove_from_playlist", on(evening, &ids[1])),
        Err("not your playlist".into())
    );

    let bach = args([("composer", Value::text("Johann Sebastian Bach"))]);
    let work = c.list("works", bach.clone())[0].field("id");
    let takes = c.list("recordings", args([("work_id", work.clone())]));
    assert_eq!(
        takes.len(),
        3,
        "three performers of one work: Ishizaka, Gould, and the composer's own line"
    );
    let rid = takes[0].field("id");
    c.mutate(
        "alice",
        "credit_recording",
        args([
            ("recording_id", rid.clone()),
            ("person_name", Value::text("Kimiko Ishizaka")),
            ("role", Value::text("soloist")),
            ("instrument", Value::text("Piano")),
            ("pos", Value::int(1)),
        ]),
    )
    .unwrap();
    c.mutate(
        "alice",
        "describe_work",
        args([
            ("id", work.clone()),
            ("opus", Value::text("")),
            ("key_sig", Value::text("G Major")),
            ("form", Value::text("Variations")),
            ("period", Value::text("Baroque")),
            ("composed", Value::int(1741)),
            ("art", Value::text("")),
        ]),
    )
    .unwrap();
    c.mutate(
        "alice",
        "describe_recording",
        args([
            ("id", rid.clone()),
            ("recorded", Value::int(2012)),
            ("venue", Value::text("")),
            ("label", Value::text("")),
            ("licence", Value::text("CC0")),
            ("art", Value::text("")),
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
            ("art", Value::text("")),
        ]),
    )
    .unwrap();

    for q in ["albums", "artists", "track_details", "composers", "playlists"] {
        c.query("alice", q, args([])).unwrap();
    }
    let with_favs = |k: &str, v: Value| args([("playlist_id", Value::Id(favs)), (k, v)]);
    // A search of the title or the creator, case folded (D4): Bach's tracks
    // by creator, and nothing for a needle nobody holds.
    let found = c.list("search", with_favs("needle", Value::text("SEBASTIAN")));
    assert!(!found.is_empty() && found.iter().all(|r| r.field("creator") == Value::text("Johann Sebastian Bach")));
    assert!(c.list("search", with_favs("needle", Value::text("zzz"))).is_empty());
    c.list("album", with_favs("name", Value::text("Goldberg Variations")));
    c.list("artist", with_favs("name", Value::text("Johann Sebastian Bach")));
    c.list("recording", with_favs("id", rid.clone()));
    c.list("work", args([("id", work.clone())]));
    c.list("credits", args([("recording_id", rid.clone())]));
    c.list("playlists_of", args([("media_id", ids[0].clone())]));
    c.list("playlist", args([("playlist_id", Value::Id(evening))]));
    assert_eq!(
        c.query("bob", "playlist", args([("playlist_id", Value::Id(evening))])),
        Err("not your playlist".into())
    );

    // Removal, which touches both sides of the library.
    for id in &ids[..4] {
        c.mutate("alice", "remove_media", args([("id", id.clone())])).unwrap();
    }
    assert_eq!(c.library(favs).len(), 8);
    for q in ["albums", "artists", "track_details", "composers"] {
        c.query("alice", q, args([])).unwrap();
    }
    c.list("works", bach);

    let procedures: std::collections::BTreeSet<String> = c.procs.keys().cloned().collect();
    assert_eq!(*c.called.borrow(), procedures, "every procedure was run both ways");
}
