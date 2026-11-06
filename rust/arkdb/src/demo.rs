//! The demo of `spec/AUTHORING.md` in plain Rust (§1.16): the four
//! mutators of `rust/ark/tests/demo.rs` (so its pinned states hold), a
//! view of a playlist's items and a scope; and a room of live state over
//! volatile tables of its own (§1.9): who is here, which `on_join` and
//! `on_leave` write and `on_restore` keeps, a name a device renames itself
//! by, and a pointer each device moves, relayed as ephemeral.

use std::collections::BTreeSet;

use ark::prelude::*;
use ark_peer::Config;

// * The demo

/// A playlist, one person's, its name unique among theirs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(unique(user_id, name))]
pub struct Playlist {
    #[ark(key)]
    pub id: Id<Playlist>,
    pub name: String,
    pub user_id: String,
}

/// A track on a playlist, at a place unique on it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(unique(playlist_id, pos))]
pub struct Item {
    #[ark(key, refs(Playlist))]
    pub playlist_id: Id<Playlist>,
    #[ark(key)]
    pub track_id: String,
    pub pos: i64,
}

#[derive(Schema)]
pub struct Demo {
    pub playlist: Playlist,
    pub item: Item,
}

/// A playlist's name: trimmed, and something.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct CreatePlaylist {
    #[ark(trim, non_empty, why = "a playlist needs a name")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    #[ark(len = "1..")]
    pub track_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct RenamePlaylist {
    pub playlist_id: Id<Playlist>,
    #[ark(trim, non_empty, why = "a playlist needs a name")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct RemoveFromPlaylist {
    pub playlist_id: Id<Playlist>,
    pub track_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Items {
    pub playlist_id: Id<Playlist>,
}

/// A playlist, unless the person has one of that name; its id the entry's
/// first draw.
#[mutator("demo.create_playlist")]
pub fn create_playlist(ctx: &Ctx, db: &mut Db<Demo>, input: CreatePlaylist) -> Result<(), String> {
    let user = ctx.user().to_string();
    if !db.playlist.by(Playlist::USER_ID_NAME).from((user.clone(), input.name.clone())).any() {
        db.playlist.insert(Playlist {
            id: ctx.new_id(),
            name: input.name,
            user_id: user,
        })?;
    }
    Ok(())
}

/// A track at one past the playlist's last place, unless it is on it.
#[mutator("demo.add_to_playlist")]
pub fn add_to_playlist(_: &Ctx, db: &mut Db<Demo>, input: AddToPlaylist) -> Result<(), String> {
    if !db.playlist.has(input.playlist_id) {
        return Err("playlist_id: no such playlist".into());
    }
    if db.item.has((input.playlist_id, input.track_id.clone())) {
        return Ok(());
    }
    let last = db.item.by(Item::PLAYLIST_ID_POS).from(input.playlist_id).last();
    db.item.insert(Item {
        playlist_id: input.playlist_id,
        track_id: input.track_id,
        pos: last.map_or(0, |i| i.pos) + 1,
    })
}

#[mutator("demo.rename_playlist")]
pub fn rename_playlist(_: &Ctx, db: &mut Db<Demo>, input: RenamePlaylist) -> Result<(), String> {
    let p = db.playlist.get(input.playlist_id).ok_or_else(|| String::from("no such playlist"))?;
    db.playlist.update(Playlist { name: input.name, ..p })
}

#[mutator("demo.remove_from_playlist")]
pub fn remove_from_playlist(_: &Ctx, db: &mut Db<Demo>, input: RemoveFromPlaylist) -> Result<(), String> {
    db.item.delete((input.playlist_id, input.track_id))
}

/// A playlist's items, in order.
#[view("demo.items")]
pub fn items(_: &Ctx, _: &Db<Demo>, input: Items) -> Result<Plan<Demo, Item>, String> {
    Ok(scan::<Demo, Item>().filter(move |i| i.playlist_id == input.playlist_id).sort(|i| i.pos))
}

/// Every playlist, and of each the items in its first two places.
#[scope("demo.top_two")]
pub fn top_two() -> Scope<Demo> {
    Scope::new().on::<Playlist>(|_, _| true).on::<Item>(|_, i| i.pos <= 2)
}

/// The demo, every function registered.
pub fn demo() -> Domain {
    ark::register!(Domain::new::<Demo>("demo", "1");
        create_playlist, add_to_playlist, rename_playlist, remove_from_playlist, items, top_two)
}

// * The room

/// Who is here: what `on_join` writes and `on_leave` takes away.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct Here {
    #[ark(key)]
    pub session: String,
    pub name: String,
    pub since: i64,
}

/// A pointer a device moves, relayed as ephemeral.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct Pointer {
    #[ark(key)]
    pub session: String,
    pub x: i64,
}

#[derive(Schema)]
pub struct Room {
    pub here: Here,
    pub pointer: Pointer,
}

/// A device as its `Hello` says it, and `on_join` and `on_leave` are told
/// of it, as its login: its session is the login's, whatever it says.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Arrival {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Point {
    pub x: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Rename {
    pub name: String,
}

#[mutator("room.on_join")]
pub fn on_join(ctx: &Ctx, db: &mut Db<Room, Volatile>, a: Arrival) -> Result<(), String> {
    db.here.delete(ctx.device())?;
    db.here.insert(Here {
        session: ctx.device().into(),
        name: a.name,
        since: ctx.now,
    })
}

#[mutator("room.on_leave")]
pub fn on_leave(ctx: &Ctx, db: &mut Db<Room, Volatile>, _: Arrival) -> Result<(), String> {
    db.pointer.delete(ctx.device())?;
    db.here.delete(ctx.device())
}

/// After a restart: who was here is kept, every pointer let go.
#[mutator("room.on_restore")]
pub fn on_restore(_: &Ctx, db: &mut Db<Room, Volatile>, _: ()) -> Result<(), String> {
    for p in db.pointer.scan() {
        db.pointer.remove(&p)?;
    }
    Ok(())
}

#[mutator("room.point")]
pub fn point(ctx: &Ctx, db: &mut Db<Room, Volatile>, input: Point) -> Result<(), String> {
    db.pointer.delete(ctx.device())?;
    db.pointer.insert(Pointer {
        session: ctx.device().into(),
        x: input.x,
    })
}

#[mutator("room.rename")]
pub fn rename(ctx: &Ctx, db: &mut Db<Room, Volatile>, input: Rename) -> Result<(), String> {
    let one = db.here.get(ctx.device()).ok_or_else(|| String::from("not here"))?;
    db.here.update(Here { name: input.name, ..one })
}

#[view("room.present")]
pub fn present(_: &Ctx, _: &Db<Room>, _: ()) -> Result<Plan<Room, Here>, String> {
    Ok(scan::<Room, Here>().filter(|h| h.since >= 0))
}

/// The room as a connection the demo's scope feeds holds it (§1.8, §1.9:
/// the live space is fed by the log's rules, its scope of the same name):
/// who is here, and of the pointers its own.
#[scope("demo.top_two")]
pub fn room_top_two() -> Scope<Room> {
    Scope::new().on::<Here>(|_, _| true).on::<Pointer>(|ctx, p| p.session == ctx.device())
}

/// The room, every function registered.
pub fn room() -> Domain {
    ark::register!(Domain::new::<Room>("room", "1"); on_join, on_leave, on_restore, point, rename, present, room_top_two)
}

/// The room as a client of it is configured: the device it says it is,
/// `config` with the room as its live space: a device's, named `name`,
/// relaying its pointer at most every `every_ms`.
pub fn device(config: Config, name: &str, every_ms: i64) -> Config {
    Config {
        device: Some(Arrival { name: name.into() }.to_bytes()),
        ephemeral: BTreeSet::from(["room.point".to_string()]),
        every_ms,
        ..hub(config)
    }
}

/// `config` with the room as its live space: a server's, authoring its
/// `on_join`, `on_leave` and `on_restore`.
pub fn hub(config: Config) -> Config {
    Config {
        live: Some(std::sync::Arc::new(room())),
        ..config
    }
}

/// A domain of this crate's by its name: what a trace is replayed with.
pub fn named(name: &str) -> Option<Domain> {
    match name {
        "demo" => Some(demo()),
        "room" => Some(room()),
        _ => None,
    }
}
