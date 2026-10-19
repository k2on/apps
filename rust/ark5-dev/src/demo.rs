//! The demo of `spec5/AUTHORING.md` written with `#[ark::*]`: the four
//! mutators of `rust/ark5/tests/demo.rs`, byte for byte (so its pinned
//! states hold), a query of a playlist's items and a scope; and a room of
//! live state over volatile tables of its own (§1.9): who is here, which
//! `on_join` and `on_leave` write, a name a device renames itself by, and
//! a pointer each device moves, relayed as ephemeral.

use std::collections::BTreeSet;

use ark5::author::{self as ark, Id, Input, Int, Node, Row, Schema, Table, Text};
use ark5_client::{Live, Module};

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

/// A playlist's items in order.
#[ark::query("demo.items")]
pub fn items(db: Demo, playlist_id: Id<Playlist>) -> Vec<Item> {
    db.item.filter(|i| i.playlist_id == playlist_id).sort(|i| i.pos)
}

/// Every playlist, and the first two places of each.
#[ark::scope("demo.top_two")]
pub fn top_two(row: DemoRow) -> bool {
    match row {
        DemoRow::Playlist(p) => true,
        DemoRow::Item(i) => i.pos <= 2,
    }
}

pub fn domain() -> ark::Domain {
    ark::module! {
        schema: Demo,
        functions: [create_playlist, add_to_playlist, rename_playlist, remove_from_playlist, items, top_two],
    }
    .unwrap_or_else(|r| panic!("the demo: {r}"))
}

// * The room

#[derive(Row)]
#[ark(volatile)]
pub struct Here {
    #[ark(key)]
    pub session: Text,
    pub name: Text,
    pub since: Int,
}

#[derive(Row)]
#[ark(volatile)]
pub struct Pointer {
    #[ark(key)]
    pub session: Text,
    pub x: Int,
}

#[derive(Schema)]
pub struct Room {
    pub here: Table<Here>,
    pub pointer: Table<Pointer>,
}

/// A device as `on_join` and `on_leave` are told of it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Arrival {
    pub session: Text,
    pub name: Text,
}

#[ark::live("room.on_join")]
pub fn on_join(ctx: Ctx, db: Room, a: Arrival) {
    db.here.delete(a.session);
    db.here.insert(Here {
        session: a.session,
        name: a.name,
        since: ctx.now,
    });
}

#[ark::live("room.on_leave")]
pub fn on_leave(db: Room, a: Arrival) {
    db.pointer.delete(a.session);
    db.here.delete(a.session);
}

#[ark::live("room.rename")]
pub fn rename(ctx: Ctx, db: Room, name: Text) {
    if db.here.get(ctx.session).is_some() {
        db.here.update(ctx.session, |h| Here { name, ..h });
    } else {
        refuse!("not here");
    }
}

#[ark::live("room.point")]
pub fn point(ctx: Ctx, db: Room, x: Int) {
    db.pointer.delete(ctx.session);
    db.pointer.insert(Pointer { session: ctx.session, x });
}

#[ark::query("room.present")]
pub fn present(db: Room) -> Vec<Here> {
    db.here.filter(|h| h.since >= 0)
}

pub fn room() -> ark::Domain {
    ark::module! {
        schema: Room,
        functions: [on_join, on_leave, rename, point, present],
    }
    .unwrap_or_else(|r| panic!("the room: {r}"))
}

/// A domain as a client or a hub holds it.
pub fn module(d: &ark::Domain) -> Module {
    Module {
        schema: d.schema.clone(),
        cas: d.cas(),
        lib: d.lib.clone(),
    }
}

/// The room as a client of it is configured: the device it says it is,
/// its name; `point` relayed every `every_ms`.
pub fn live(name: &str, every_ms: i64) -> Live {
    let r = room();
    Live {
        module: module(&r),
        device: Some(name.as_bytes().to_vec()),
        ephemeral: BTreeSet::from([r.get("room.point")]),
        every_ms,
    }
}

/// What a device's `Hello` says, made the input of `on_join`: its session
/// is the login's, whatever it says; its name is what it says.
pub fn arrival(who: &ark5::peer::Who, said: &[u8]) -> Option<Vec<u8>> {
    let name = String::from_utf8(said.to_vec()).ok()?;
    Some(
        Arrival {
            session: who.session.clone(),
            name,
        }
        .to_bytes(),
    )
}
