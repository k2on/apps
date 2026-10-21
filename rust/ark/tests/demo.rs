//! The demo of `spec/AUTHORING.md`, in plain Rust (§1.16): the same
//! domain as `ark-dev`'s, here because the kernel's tests cannot link the
//! dev crate. Its functions are registered by name, and its inputs encode
//! as the byte helpers below write them.
//!
//! `playlist(id: Id<playlist>, name: Text, user_id: Text)` keyed by `id`,
//! unique `(user_id, name)`; `item(playlist_id: Id<playlist> → playlist,
//! track_id: Text, pos: Int)` keyed by `(playlist_id, track_id)`, unique
//! `(playlist_id, pos)`. `create_playlist(name)`, its id the entry's first
//! draw, nothing when the person has a playlist of that name;
//! `add_to_playlist(playlist_id, track_id)`, the playlist existing
//! ("playlist_id: no such playlist"), nothing when the track is on it
//! already, at one past the playlist's last `pos`; `rename_playlist` (an
//! `Edit`) and `remove_from_playlist` (a `Remove`); and dave's wrong build
//! of `add_to_playlist`, which steps by two.
//!
//! Included by the other tests with `#[path]`; it has no tests of its own.

#![allow(dead_code)]

use std::sync::Arc;

use ark::peer::Functions;
use ark::prelude::*;
use ark::schema::Schema;
use ark::store::Read;

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
pub fn create_playlist(ctx: &Ctx, db: &mut Db<Demo>, input: CreatePlaylist) -> Result<(), Refusal> {
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
pub fn add_to_playlist(_: &Ctx, db: &mut Db<Demo>, input: AddToPlaylist) -> Result<(), Refusal> {
    if !db.playlist.has(input.playlist_id) {
        return Err(Refusal::new("playlist_id: no such playlist"));
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
pub fn rename_playlist(_: &Ctx, db: &mut Db<Demo>, input: RenamePlaylist) -> Result<(), Refusal> {
    let p = db.playlist.get(input.playlist_id).ok_or_else(|| Refusal::new("no such playlist"))?;
    db.playlist.update(Playlist { name: input.name, ..p })
}

#[mutator("demo.remove_from_playlist")]
pub fn remove_from_playlist(_: &Ctx, db: &mut Db<Demo>, input: RemoveFromPlaylist) -> Result<(), Refusal> {
    db.item.delete((input.playlist_id, input.track_id))
}

/// A playlist's items, in order.
#[view("demo.items")]
pub fn items(_: &Ctx, _: &Db<Demo>, input: Items) -> Result<Plan<Demo, Item>, Refusal> {
    Ok(scan::<Demo, Item>().filter(move |i| i.playlist_id == input.playlist_id).sort(|i| i.pos))
}

/// Every playlist, and of each the items in its first two places.
#[scope("demo.top_two")]
pub fn top_two() -> Scope<Demo> {
    Scope::new().on::<Playlist>(|_, _| true).on::<Item>(|_, i| i.pos <= 2)
}

/// Every playlist, and the items whose track is the reader's: named
/// `actor:…` (the partial fan-out's scope).
#[scope("demo.mine")]
pub fn mine() -> Scope<Demo> {
    Scope::new()
        .on::<Playlist>(|_, _| true)
        .on::<Item>(|ctx, i| i.track_id.starts_with(&format!("{}:", ctx.user())))
}

/// The demo, every function registered.
pub fn demo() -> Domain {
    ark::register!(Domain::new::<Demo>("demo", "1");
        create_playlist, add_to_playlist, rename_playlist, remove_from_playlist, items, top_two, mine)
}

pub fn schema() -> Schema {
    <Demo as Tables>::schema()
}

pub fn shared() -> Arc<Schema> {
    Arc::new(schema())
}

fn id<R>(b: &[u8]) -> Id<R> {
    Id::new(b.try_into().expect("an id is 16 bytes"))
}

pub fn playlist_row(pid: &[u8], name: &str, user: &str) -> Vec<u8> {
    Playlist {
        id: id(pid),
        name: name.into(),
        user_id: user.into(),
    }
    .to_bytes()
}

pub fn item_row(pid: &[u8], track: &str, pos: i64) -> Vec<u8> {
    Item {
        playlist_id: id(pid),
        track_id: track.into(),
        pos,
    }
    .to_bytes()
}

/// An `item` row read back: `(playlist_id, track_id, pos)`.
pub fn item_of(row: &[u8]) -> (Vec<u8>, String, i64) {
    let i = Item::from_bytes(row).unwrap();
    (i.playlist_id.0.to_vec(), i.track_id, i.pos)
}

/// The `pos` of a track on a playlist, as a store holds it.
pub fn pos_of(store: &dyn Read, pid: &[u8], track: &str) -> Option<i64> {
    let mut key = vec![];
    ark::format::put_key(&(id::<Playlist>(pid), track), &mut key);
    store.get("item", &key).map(|r| item_of(&r).2)
}

pub fn create_input(name: &str) -> Vec<u8> {
    CreatePlaylist { name: name.into() }.to_bytes()
}

/// `add_to_playlist`'s and `remove_from_playlist`'s input.
pub fn pair_input(pid: &[u8], track: &str) -> Vec<u8> {
    AddToPlaylist {
        playlist_id: id(pid),
        track_id: track.into(),
    }
    .to_bytes()
}

/// `rename_playlist`'s input.
pub fn rename_input(pid: &[u8], name: &str) -> Vec<u8> {
    RenamePlaylist {
        playlist_id: id(pid),
        name: name.into(),
    }
    .to_bytes()
}

pub const CREATE: &str = "demo.create_playlist";
pub const ADD: &str = "demo.add_to_playlist";
pub const RENAME: &str = "demo.rename_playlist";
pub const REMOVE: &str = "demo.remove_from_playlist";

/// What an entry calls a demo function by: its name.
pub fn h(name: &str) -> String {
    name.to_string()
}

/// Every demo function, as a peer that authors all of them holds them.
pub fn cas() -> Functions {
    demo().functions()
}

/// Dave's build: `add_to_playlist`'s name, running a body stepping by two.
pub fn daves_cas() -> Functions {
    let mut c = cas();
    let dave = Domain::new::<Demo>("demo", "1").mutator(ADD, daves_add).functions();
    c.insert(ADD, dave.get(ADD).unwrap().clone());
    c
}

fn daves_add(_: &Ctx, db: &mut Db<Demo>, input: AddToPlaylist) -> Result<(), Refusal> {
    if !db.playlist.has(input.playlist_id) {
        return Err(Refusal::new("playlist_id: no such playlist"));
    }
    if db.item.has((input.playlist_id, input.track_id.clone())) {
        return Ok(());
    }
    let last = db.item.by(Item::PLAYLIST_ID_POS).from(input.playlist_id).last();
    db.item.insert(Item {
        playlist_id: input.playlist_id,
        track_id: input.track_id,
        pos: last.map_or(0, |i| i.pos) + 2,
    })
}

/// Only these demo functions: a peer built without the rest.
pub fn only(names: &[&str]) -> Functions {
    let all = cas();
    let mut f = Functions::default();
    for n in names {
        f.insert(*n, all.get(n).unwrap().clone());
    }
    f
}
