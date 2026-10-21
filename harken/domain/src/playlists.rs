//! The playlists: what a person does on any device, offline or not.
//!
//! A playlist is a list somebody made; "Favorites" is one of these and
//! nothing special. It is ordered, so adding reads the last `pos` and goes
//! one past it, which is what makes the rebase visible: add while offline
//! and it lands after whatever arrived while you were away. Anybody may make
//! one, nobody (a peer before anyone signed in, user "") included; [`owned`]
//! lets nobody else at it.

use ark::prelude::*;

use crate::browse::Plan;
use crate::library::library_entry;
use crate::schema::*;
use crate::types::*;

/// The playlist a function is about, when it is the caller's; refused
/// otherwise.
pub fn owned(ctx: &Ctx, db: &Db<Harken>, playlist_id: Id<Playlist>) -> Result<Playlist, Refusal> {
    match db.playlist.get(playlist_id) {
        Some(p) if p.user_id == ctx.user() => Ok(p),
        _ => Err(Refusal::new("not your playlist")),
    }
}

/// A name with a number after it: "Favorites (1)".
pub fn numbered(name: &str, n: i64) -> String {
    format!("{name} ({n})")
}

/// The smallest n from 1 whose "name (n)" is none of these names. The names
/// are one person's, so distinct: each one's rank among them is one of
/// 1..len, so the answer is len + 1 or among the ranks — the least rank
/// free, found by sorting.
pub fn free_number(names: &[String], name: &str) -> i64 {
    let ranks = names.iter().map(|it| names.iter().filter(|other| *other <= it).count() as i64);
    let free = ranks.filter(|rank| !names.contains(&numbered(name, *rank))).min();
    free.unwrap_or(names.len() as i64 + 1)
}

/// What a new playlist is called among a person's other playlists: the
/// name asked for, or when they have one of that name, the first "name (n)"
/// they do not have.
pub fn playlist_name(names: &[String], name: &str) -> String {
    if names.iter().any(|n| n == name) {
        numbered(name, free_number(names, name))
    } else {
        name.into()
    }
}

/// Make a playlist, after every other. A name the person already has is
/// numbered, decided from the rows at apply time so every peer replaying
/// reaches the same name in log order; of theirs, only the names from
/// `name` up to `name )` are read — the name and its numbered siblings.
/// The same entry twice is one playlist: its id is the key.
#[mutator("playlists.create_playlist")]
pub fn create_playlist(ctx: &Ctx, db: &mut Db<Harken>, input: CreatePlaylist) -> Result<(), Refusal> {
    let id = ctx.new_id();
    if db.playlist.has(id) {
        return Ok(());
    }
    let user = ctx.user().to_string();
    let last = db.playlist.by(Playlist::POS).whole().last();
    let siblings = db
        .playlist
        .by(Playlist::USER_ID_NAME)
        .between((user.clone(), input.name.clone()), (user.clone(), format!("{} )", input.name)))
        .all();
    let names: Vec<String> = siblings.into_iter().map(|s| s.name).collect();
    db.playlist.insert(Playlist {
        id,
        name: playlist_name(&names, &input.name),
        pos: last.map_or(0, |p| p.pos) + 1,
        created_ms: ctx.now,
        user_id: user,
    })
}

/// The last place on a playlist, if anything is on it.
fn last_pos(db: &Db<Harken>, playlist: Id<Playlist>) -> Option<i64> {
    db.playlist_item.by(PlaylistItem::PLAYLIST_ID_POS).from(playlist).last().map(|i| i.pos)
}

/// Put something on a playlist, at the end of it. A playlist holds an item
/// once, so adding one already there keeps its place; something no longer
/// in the library is a no-op.
#[mutator("playlists.add_to_playlist")]
pub fn add_to_playlist(ctx: &Ctx, db: &mut Db<Harken>, input: AddToPlaylist) -> Result<(), Refusal> {
    let playlist = owned(ctx, db, input.playlist_id)?;
    if db.media.has(input.media_id) && !db.playlist_item.has((playlist.id, input.media_id)) {
        let pos = last_pos(db, playlist.id).unwrap_or(0) + 1;
        db.playlist_item.insert(PlaylistItem {
            playlist_id: playlist.id,
            media_id: input.media_id,
            pos,
            added_ms: ctx.now,
            user_id: ctx.user().into(),
        })?;
    }
    Ok(())
}

/// Put everything in the library on a playlist, in library order. One
/// entry, an intent: replayed, it covers whatever the library held by then.
#[mutator("playlists.add_all_to_playlist")]
pub fn add_all_to_playlist(ctx: &Ctx, db: &mut Db<Harken>, input: AddAllToPlaylist) -> Result<(), Refusal> {
    let playlist = owned(ctx, db, input.playlist_id)?;
    for track in db.media.by(Media::POS).whole().all() {
        if !db.playlist_item.has((playlist.id, track.id)) {
            let pos = last_pos(db, playlist.id).unwrap_or(0) + 1;
            db.playlist_item.insert(PlaylistItem {
                playlist_id: playlist.id,
                media_id: track.id,
                pos,
                added_ms: ctx.now,
                user_id: ctx.user().into(),
            })?;
        }
    }
    Ok(())
}

/// Take something off a playlist. The item stays in the library.
#[mutator("playlists.remove_from_playlist")]
pub fn remove_from_playlist(ctx: &Ctx, db: &mut Db<Harken>, input: RemoveFromPlaylist) -> Result<(), Refusal> {
    let playlist = owned(ctx, db, input.playlist_id)?;
    db.playlist_item.delete((playlist.id, input.media_id))
}

/// The caller's playlists, in the order they were made.
#[view("playlists.playlists")]
pub fn playlists(ctx: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<Playlist>, Refusal> {
    let user = ctx.user().to_string();
    Ok(scan::<Harken, Playlist>().filter(move |p| p.user_id == user).sort(|p| p.pos))
}

/// Which of the caller's playlists a track is on: what makes a playlist
/// sheet a toggle.
#[view("playlists.playlists_of")]
pub fn playlists_of(ctx: &Ctx, _: &Db<Harken>, input: PlaylistsOf) -> Result<Plan<Playlist>, Refusal> {
    let user = ctx.user().to_string();
    Ok(scan::<Harken, Playlist>()
        .filter_with(move |db, p| p.user_id == user && db.playlist_item.has((p.id, input.media_id)))
        .sort(|p| p.pos))
}

/// One playlist's contents, in playlist order, as the library's own rows;
/// an entry whose media has gone is dropped.
#[view("playlists.playlist")]
pub fn playlist(ctx: &Ctx, db: &Db<Harken>, input: PlaylistInput) -> Result<Plan<LibraryEntry>, Refusal> {
    let playlist = owned(ctx, db, input.playlist_id)?;
    Ok(scan::<Harken, PlaylistItem>()
        .filter_with(move |db, i| i.playlist_id == playlist.id && db.media.has(i.media_id))
        .sort(|i| i.pos)
        .map_with(|db, i| library_entry(&db.media.get(i.media_id).expect("an item's media"), Some(i.pos))))
}
