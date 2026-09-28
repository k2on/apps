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
            "albums",
            "artists",
            "credited",
            "joined",
            "performers",
            "track_details",
            "album",
            "artist",
            "tracks_on",
            "composers",
            "work_summary",
            "works",
            "work",
            "recordings",
            "credits",
            "recording",
            "signed_in",
            "owned",
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

    assert_eq!(decoded.lookup_function("add_to_playlist").unwrap().uses, ["signed_in", "owned"]);
    assert_eq!(decoded.lookup_function("create_playlist").unwrap().uses, ["signed_in"]);
    assert_eq!(decoded.lookup_router("playlists").unwrap().uses, ["signed_in", "owned"]);
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
    c.mutate("alice", "remove_from_playlist", on(evening, &ids[1])).unwrap();
    assert_eq!(c.mutate("", "remove_from_playlist", on(evening, &ids[1])), Err("sign in first".into()));

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
