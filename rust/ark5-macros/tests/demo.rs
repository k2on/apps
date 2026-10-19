//! The demo (`rust/ark5/tests/demo.rs`) written with `#[ark::mutator]`
//! and the derives is the IR the plain builder there writes: the same
//! schema, the same four functions byte for byte, so the same hashes and
//! the same module. Falsified by spelling `add_to_playlist`'s step `+ 2`:
//! its hash moved and `the_demo_authored_is_the_demo_built` failed.

#[path = "../../ark5/tests/demo.rs"]
mod demo;

#[allow(dead_code)]
mod authored {
    use ark5::author::{self as ark, Id, Input, Int, Row, Schema, Table, Text};

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
    pub struct Name(Text);

    #[derive(Input)]
    #[ark(len = "1..")]
    pub struct TrackId(Text);

    #[ark::mutator("demo.create_playlist")]
    pub fn create_playlist(ctx: Ctx, db: Demo, name: Name) {
        if db.playlist.filter(|p| p.user_id == ctx.user && p.name == name).len() == 0 {
            db.playlist.insert(Playlist {
                id: ctx.new_id(),
                name,
                user_id: ctx.user,
            });
        }
    }

    #[ark::mutator("demo.add_to_playlist")]
    pub fn add_to_playlist(db: Demo, playlist_id: Id<Playlist>, track_id: TrackId) {
        if db.playlist.get(playlist_id).is_some() {
            if db.item.get((playlist_id, track_id)).is_some() {
            } else {
                let last = db
                    .item
                    .filter(|row| row.playlist_id == playlist_id)
                    .sort(|row| Desc(row.pos))
                    .take(1)
                    .first();
                db.item.insert(Item {
                    playlist_id,
                    track_id,
                    pos: match last {
                        None => 0,
                        Some(row) => row.pos,
                    } + 1,
                });
            }
        } else {
            refuse!("playlist_id: no such playlist");
        }
    }

    #[ark::mutator("demo.rename_playlist")]
    pub fn rename_playlist(db: Demo, id: Id<Playlist>, name: Name) -> Result<(), Refuse> {
        if db.playlist.get(id).is_some() {
            db.playlist.update(id, |p| Playlist { name, ..p })
        } else {
            refuse!("no such playlist")
        }
        Ok(())
    }

    #[ark::mutator("demo.remove_from_playlist")]
    pub fn remove_from_playlist(db: Demo, playlist_id: Id<Playlist>, track_id: Text) {
        db.item.delete((playlist_id, track_id));
    }

    pub fn domain() -> ark::Domain {
        ark::module! {
            schema: Demo,
            functions: [create_playlist, add_to_playlist, rename_playlist, remove_from_playlist],
        }
        .unwrap()
    }
}

use authored::*;

#[test]
fn the_demo_authored_is_the_demo_built() {
    assert_eq!(Demo::schema(), demo::schema());
    for (built, name) in [
        (create_playlist::built(), demo::CREATE),
        (add_to_playlist::built(), demo::ADD),
        (rename_playlist::built(), demo::RENAME),
        (remove_from_playlist::built(), demo::REMOVE),
    ] {
        assert_eq!(built.name, name);
        assert_eq!(built.f, demo::function(name), "{name}");
        assert_eq!(built.hash, demo::h(name), "{name}");
    }
    let d = domain();
    assert_eq!(d.module, demo::module(&ark5::module::Lib::default()));
    assert_eq!(d.module.hash(), demo::module(&ark5::module::Lib::default()).hash());
    ark5::module::verify(&d.module, &d.lib).unwrap();
}
