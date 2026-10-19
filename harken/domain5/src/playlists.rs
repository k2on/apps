//! The playlists router: what a person does on any device, offline or not
//! (`harken/domain/src/playlists.rs`, function for function).
//!
//! A playlist is a list somebody made; "Favorites" is one of these and
//! nothing special. It is ordered, so adding reads `MAX(pos) + 1`, which is
//! what makes the rebase visible: add while offline and it lands after
//! whatever arrived while you were away. Anybody may make one, nobody (a
//! peer before anyone signed in, user `""`) included; `owned` lets nobody
//! else at it.

use ark5::author::{self as ark, Id, Input, Int, Node, Text};

use crate::library::*;
use crate::schema::*;

/// A playlist's name: trimmed, and something.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct CreatePlaylist {
    #[ark(trim, non_empty, why = "a playlist needs a name", len = "..=120")]
    pub name: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct AddAllToPlaylist {
    pub playlist_id: Id<Playlist>,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct RemoveFromPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct PlaylistsOf {
    pub media_id: Id<Media>,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct PlaylistInput {
    pub playlist_id: Id<Playlist>,
}

/// The playlist a procedure is about, when it is the caller's; refused
/// otherwise (v4's `owned` provider).
#[ark::guard("playlists.owned")]
pub fn owned(ctx: Ctx, db: Harken, playlist_id: Id<Playlist>) -> Playlist {
    match db.playlist.get(playlist_id) {
        Some(p) => {
            if p.user_id == ctx.user {
                p
            } else {
                refuse!("not your playlist")
            }
        }
        None => refuse!("not your playlist"),
    }
}

/// A name with a number after it: "Favorites (1)".
#[ark::helper("playlists.numbered")]
pub fn numbered(name: Text, n: Int) -> Text {
    concat(vec![name, " (", n.to_text(), ")"])
}

/// The smallest n from 1 whose "name (n)" is none of these names. The
/// names are one person's, so distinct: each one's rank among them is one
/// of 1..len, so the answer is len + 1 or among the ranks — the least rank
/// free, found by sorting them, where v4 folded.
#[ark::helper("playlists.free_number")]
pub fn free_number(names: Vec<Text>, name: Text) -> Int {
    let ranks = names.map(|x| names.filter(|y| y <= x).len());
    ranks.filter(|n| !names.contains(numbered(name, n))).sort(|n| n).first().unwrap_or(names.len() + 1)
}

/// What a new playlist is called among a person's other playlists: the
/// name asked for, or when they already have one of that name, the first
/// "name (n)" they do not have.
#[ark::helper("playlists.playlist_name")]
pub fn playlist_name(names: Vec<Text>, name: Text) -> Text {
    if names.contains(name) {
        numbered(name, free_number(names, name))
    } else {
        name
    }
}

/// Make a playlist, after every other. A name the person already has is
/// numbered, decided from the rows at apply time so every peer replaying
/// reaches the same name in log order; of theirs, only the names from
/// `name` up to `name )` are read — the name and its numbered siblings.
/// The same entry twice is one playlist: its id is the key.
#[ark::mutator("playlists.create_playlist")]
pub fn create_playlist(ctx: Ctx, db: Harken, input: CreatePlaylist) {
    let id = ctx.new_id::<Playlist>();
    if db.playlist.get(id).is_none() {
        let last = db.playlist.sort(|p| Desc(p.pos)).first();
        let siblings = db
            .playlist
            .filter(|p| p.user_id == ctx.user && p.name >= input.name && p.name < concat(vec![input.name, " )"]));
        db.playlist.insert(Playlist {
            id,
            name: playlist_name(siblings.map(|p| p.name), input.name),
            pos: last.map_or(0, |p| p.pos) + 1,
            created_ms: ctx.now,
            user_id: ctx.user,
        });
    }
}

/// Put something on a playlist, at the end of it. A playlist holds an item
/// once, so adding one already there keeps its place; something no longer
/// in the library is a no-op.
#[ark::mutator("playlists.add_to_playlist")]
pub fn add_to_playlist(ctx: Ctx, db: Harken, input: AddToPlaylist) {
    let playlist = owned(ctx, db, input.playlist_id);
    if db.media.get(input.media_id).is_some() && db.playlist_item.get((playlist.id, input.media_id)).is_none() {
        let last = db.playlist_item.filter(|i| i.playlist_id == playlist.id).sort(|i| Desc(i.pos)).first();
        db.playlist_item.insert(PlaylistItem {
            playlist_id: playlist.id,
            media_id: input.media_id,
            pos: last.map_or(0, |i| i.pos) + 1,
            added_ms: ctx.now,
            user_id: ctx.user,
        });
    }
}

/// Put everything in the library on a playlist, in library order. One
/// entry, an intent: replayed, it covers whatever the library held by then.
#[ark::mutator("playlists.add_all_to_playlist")]
pub fn add_all_to_playlist(ctx: Ctx, db: Harken, input: AddAllToPlaylist) {
    let playlist = owned(ctx, db, input.playlist_id);
    db.media.sort(|m| m.pos).map(|m| {
        if db.playlist_item.get((playlist.id, m.id)).is_none() {
            let last = db.playlist_item.filter(|i| i.playlist_id == playlist.id).sort(|i| Desc(i.pos)).first();
            db.playlist_item.insert(PlaylistItem {
                playlist_id: playlist.id,
                media_id: m.id,
                pos: last.map_or(0, |i| i.pos) + 1,
                added_ms: ctx.now,
                user_id: ctx.user,
            })
        }
    });
}

/// Take something off a playlist. The item stays in the library.
#[ark::mutator("playlists.remove_from_playlist")]
pub fn remove_from_playlist(ctx: Ctx, db: Harken, input: RemoveFromPlaylist) {
    let playlist = owned(ctx, db, input.playlist_id);
    db.playlist_item.delete((playlist.id, input.media_id));
}

/// The caller's playlists, in the order they were made.
#[ark::query("playlists.playlists")]
pub fn playlists(ctx: Ctx, db: Harken) -> Vec<Playlist> {
    db.playlist.filter(|p| p.user_id == ctx.user).sort(|p| p.pos)
}

/// Which of the caller's playlists a track is on: what makes a playlist
/// sheet a toggle.
#[ark::query("playlists.playlists_of")]
pub fn playlists_of(ctx: Ctx, db: Harken, input: PlaylistsOf) -> Vec<Playlist> {
    db.playlist
        .filter(|p| p.user_id == ctx.user && db.playlist_item.get((p.id, input.media_id)).is_some())
        .sort(|p| p.pos)
}

/// One playlist's contents, in playlist order, as the library's own rows;
/// an entry whose media has gone is dropped.
#[ark::query("playlists.playlist")]
pub fn playlist(ctx: Ctx, db: Harken, input: PlaylistInput) -> Vec<LibraryEntry> {
    let p = owned(ctx, db, input.playlist_id);
    db.playlist_item
        .filter(|i| i.playlist_id == p.id && db.media.get(i.media_id).is_some())
        .sort(|i| i.pos)
        .map(|i| library_entry(db.media.get(i.media_id).unwrap(), Some(i.pos)))
}
