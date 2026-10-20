//! §1.15 The Rust builders: the demo (`tests/demo.rs`) written with them
//! is the IR the plain builder there writes — the same schema, the same
//! four functions byte for byte, so the same hashes and the same module.
//! Falsified by stepping `add_to_playlist`'s `pos` by 2: its hash moved
//! and `the_demo_built_is_the_demo` failed.

#[path = "demo.rs"]
mod demo;

use ark::author::*;

#[derive(Row)]
#[ark(unique(user_id, name))]
pub struct Playlist {
    #[ark(key)]
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}

#[derive(Row)]
#[ark(table = "item", unique(playlist_id, pos))]
pub struct Item {
    #[ark(key, refs = Playlist)]
    pub playlist_id: Id<Playlist>,
    #[ark(key)]
    pub track_id: Text,
    pub pos: Int,
}

#[derive(Schema)]
pub struct Demo {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}

#[derive(Input)]
#[ark(trim, non_empty, why = "a playlist needs a name")]
pub struct Name(pub Text);

#[derive(Input)]
#[ark(len = "1..")]
pub struct TrackId(pub Text);

static CREATE: Def1<Name, ()> = Def1 {
    name: "demo.create_playlist",
    kind: Kind::Mutator,
    body: |name| {
        let db = Demo::db();
        let mine = db.playlist.filter(|p| p.f().user_id.eq(Ctx.user()).and(p.f().name.eq(&name)));
        let row = PlaylistEx {
            id: Ctx.new_id::<Playlist>(),
            name,
            user_id: Ctx.user(),
        };
        if_(mine.len().eq(0), db.playlist.insert(row), unit())
    },
};

static ADD: Def2<Id<Playlist>, TrackId, ()> = Def2 {
    name: "demo.add_to_playlist",
    kind: Kind::Mutator,
    body: |pid, tid| {
        let db = Demo::db();
        let last = db.item.filter(|r| r.f().playlist_id.eq(&pid)).sort(|r| desc(r.f().pos)).take(1).first();
        let add = let_(last, |last| {
            let pos = last.map_or(0, |r| r.f().pos) + 1;
            db.item.insert(ItemEx {
                playlist_id: pid.clone(),
                track_id: tid.clone(),
                pos,
            })
        });
        let on = db.item.get((pid.clone(), tid.clone())).is_some();
        if_(
            db.playlist.get(&pid).is_some(),
            if_(on, unit(), add),
            refuse("playlist_id: no such playlist"),
        )
    },
};

static RENAME: Def2<Id<Playlist>, Name, ()> = Def2 {
    name: "demo.rename_playlist",
    kind: Kind::Mutator,
    body: |id, name| {
        let db = Demo::db();
        let update = db.playlist.update(&id, |p| PlaylistEx { name, ..p.f() }.into());
        if_(db.playlist.get(&id).is_some(), update, refuse("no such playlist"))
    },
};

static REMOVE: Def2<Id<Playlist>, Text, ()> = Def2 {
    name: "demo.remove_from_playlist",
    kind: Kind::Mutator,
    body: |pid, tid| Demo::db().item.delete((pid, tid)),
};

#[test]
fn the_demo_built_is_the_demo() {
    assert_eq!(Demo::schema(), demo::schema());
    for (f, name) in [
        (CREATE.built(), demo::CREATE),
        (ADD.built(), demo::ADD),
        (RENAME.built(), demo::RENAME),
        (REMOVE.built(), demo::REMOVE),
    ] {
        assert_eq!(f.name, name);
        assert_eq!(f.f, demo::function(name), "{name}");
    }
    let d = assemble(Demo::schema(), &[&CREATE, &ADD, &RENAME, &REMOVE]).unwrap();
    assert_eq!(d.module, demo::module(&ark::module::Lib::default()));
    ark::module::verify(&d.module, &d.lib).unwrap();
}

static BY_HAND: Def1<Id<Playlist>, Vec<Item>> = Def1 {
    name: "fragment.by_hand",
    kind: Kind::Query,
    body: |pid| Demo::db().item.filter(|i| i.f().playlist_id.eq(&pid)).sort(|i| desc(i.f().pos)),
};

static BY_TEXT: Def1<Id<Playlist>, Vec<Item>> = Def1 {
    name: "fragment.by_text",
    kind: Kind::Query,
    body: |pid| {
        text(
            "(sort (filter item (fn (i) (== i.playlist_id pid))) (fn (i) (desc i.pos)))",
            &[("pid", &pid)],
        )
    },
};

/// `text` inside a builder: a fragment over the builder's variables and
/// the module's tables is the IR the builders write. Falsified by sorting
/// the fragment ascending: the bodies differed.
#[test]
fn a_fragment_is_what_the_builders_write() {
    let d = assemble(Demo::schema(), &[&BY_HAND, &BY_TEXT]).unwrap();
    let body = |n: &str| d.lib.get(&d.get(n)).unwrap().f.body.clone();
    assert_eq!(body("fragment.by_hand"), body("fragment.by_text"));
}

static ADDS_TO_AN_ID: Def1<Id<Playlist>, Int> = Def1 {
    name: "fragment.wrong",
    kind: Kind::Helper,
    body: |pid| text("(+ pid 1)", &[("pid", &pid)]),
};

/// A fragment is checked where it is built, in the checker's words.
/// Falsified by writing `(== pid pid)`, which checks: no panic, the test failed.
#[test]
#[should_panic(expected = "which is not Int")]
fn a_fragment_that_does_not_check_is_refused() {
    let _ = assemble(Demo::schema(), &[&ADDS_TO_AN_ID]);
}
