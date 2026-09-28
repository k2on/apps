//! The playlists router: what a person does on any device, offline or not.
//!
//! A playlist is a list somebody made; "Favorites" is one of these and
//! nothing special. It is ordered, so adding reads `MAX(pos) + 1`, which is
//! what makes the rebase visible: add while offline and it lands after
//! whatever arrived while you were away.
use ark::authoring::*;

use crate::library::library_entry;
use crate::schema::*;

/// What `owned` reads of a procedure's input: the playlist it is about.
pub struct Owned {
    pub playlist_id: Id<Playlist>,
}
impl Input for Owned {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name").max(120))
    }
}

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("media_id", id::<Media>())
    }
}

pub struct AddAllToPlaylist {
    pub playlist_id: Id<Playlist>,
}
impl Input for AddAllToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists())
    }
}

pub struct RemoveFromPlaylist {
    pub playlist_id: Id<Playlist>,
    pub media_id: Id<Media>,
}
impl Input for RemoveFromPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("media_id", id::<Media>())
    }
}

pub struct PlaylistsOf {
    pub media_id: Id<Media>,
}
impl Input for PlaylistsOf {
    fn schema() -> Object<Self> {
        object().field("media_id", id::<Media>())
    }
}

pub struct PlaylistInput {
    pub playlist_id: Id<Playlist>,
}
impl Input for PlaylistInput {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists())
    }
}

pub fn playlists() -> Router<Harken> {
    let playlists = router::<Harken>("playlists");
    let signed_in = playlists.guard("signed_in", |ctx, _db| when(ctx.user.is_empty(), || refuse("sign in first")));
    let owned = signed_in.provide("owned", |ctx, db, input: &Owned| {
        db.playlist
            .get((input.playlist_id,))
            .filter(|row| row.user_id.eq(ctx.user))
            .or_refuse("not your playlist")
    });
    playlists.routes((
        // Make a playlist, after every other. The same name from the same
        // person is a no-op: every client makes a default playlist before it
        // has seen the log, so a second device's "Favorites" must not be a
        // second one — and it is decided here, where every peer replaying
        // reaches the same answer. By person, not by library; case is kept.
        signed_in.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            let playlist = db.playlist.order_by(Playlist::pos.desc()).first();
            db.playlist
                .insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    pos: playlist.map_or(0, |row| row.pos).add(1),
                    created_ms: ctx.now("created_ms"),
                    user_id: ctx.user,
                })
                .on((Playlist::user_id, Playlist::name))
        }),
        // Put something on a playlist, at the end of it. A playlist holds an
        // item once, so adding one already there keeps its place; something
        // no longer in the library is a no-op.
        owned.input::<AddToPlaylist>().mutation("add_to_playlist", |ctx, db, input, playlist| {
            let media = db.media.exists((input.media_id,));
            when(media, || {
                let playlist_item = db
                    .playlist_item
                    .filter(PlaylistItem::playlist_id.eq(playlist.id))
                    .order_by(PlaylistItem::pos.desc())
                    .first();
                db.playlist_item.insert(PlaylistItem {
                    playlist_id: playlist.id,
                    media_id: input.media_id,
                    pos: playlist_item.map_or(0, |row| row.pos).add(1),
                    added_ms: ctx.now("added_ms"),
                    user_id: ctx.user,
                })
            })
        }),
        // Put everything in the library on a playlist, in library order. One
        // entry, an intent: a replica replaying it covers whatever the library
        // held by then, including what another peer added meanwhile.
        owned
            .input::<AddAllToPlaylist>()
            .mutation("add_all_to_playlist", |ctx, db, _input, playlist| {
                let media = db.media.order_by(Media::pos.asc()).all();
                for_each(media, |row| {
                    let playlist_item = db
                        .playlist_item
                        .filter(PlaylistItem::playlist_id.eq(playlist.id))
                        .order_by(PlaylistItem::pos.desc())
                        .first();
                    db.playlist_item.insert(PlaylistItem {
                        playlist_id: playlist.id,
                        media_id: row.id,
                        pos: playlist_item.map_or(0, |row_2| row_2.pos).add(1),
                        added_ms: ctx.now("added_ms"),
                        user_id: ctx.user,
                    })
                })
            }),
        // Take something off a playlist. The item stays in the library.
        owned
            .input::<RemoveFromPlaylist>()
            .mutation("remove_from_playlist", |_ctx, db, input, playlist| {
                db.playlist_item.delete((playlist.id, input.media_id))
            }),
        // The caller's playlists, in the order they were made.
        signed_in.query("playlists", |ctx, db, _input: ()| {
            db.playlist.filter(Playlist::user_id.eq(ctx.user)).order_by(Playlist::pos.asc()).all()
        }),
        // Which of the caller's playlists a track is on: what makes a
        // playlist sheet a toggle rather than a one-way door.
        signed_in.input::<PlaylistsOf>().query("playlists_of", |ctx, db, input| {
            let playlist_item = db.playlist_item.filter(PlaylistItem::media_id.eq(input.media_id)).all();
            db.playlist
                .filter(Playlist::user_id.eq(ctx.user))
                .order_by(Playlist::pos.asc())
                .all()
                .filter(|row| playlist_item.any(|row_2| row_2.playlist_id.eq(row.id)))
        }),
        // One playlist's contents, in playlist order, as the library's own
        // rows; an entry whose media has gone is dropped.
        owned.input::<PlaylistInput>().query("playlist", |_ctx, db, _input, playlist| {
            let playlist_item = db.playlist_item.filter(PlaylistItem::playlist_id.eq(playlist.id)).all();
            db.media
                .all()
                .filter(|row| playlist_item.any(|row_2| row_2.media_id.eq(row.id)))
                .sort_by(|row| {
                    playlist_item
                        .filter(|row_2| row_2.media_id.eq(row.id))
                        .first()
                        .map_or(0, |row_2| row_2.pos)
                })
                .map(|row| library_entry(row, playlist_item))
        }),
    ))
}
