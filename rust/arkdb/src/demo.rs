//! The demo of `spec/AUTHORING.md` in plain Rust (§1.16): the four
//! mutators of `rust/ark/tests/demo.rs` (so its pinned states hold), a
//! view of a playlist's items and a scope. Its live room is [`crate::room`].

use ark::prelude::*;

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
    ark::functions!(Domain::new::<Demo>("demo", "1"))
}

/// A domain of this crate's by its name: what a trace is replayed with.
pub fn named(name: &str) -> Option<Domain> {
    match name {
        "demo" => Some(demo()),
        "room" => Some(crate::room::room()),
        _ => None,
    }
}
