//! The playlists scope: what a person does on any device, offline or not.
use ark::authoring::*;

use crate::schema::*;

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name").max(120))
    }
}

pub struct OnPlaylist {
    pub playlist_id: Id<Playlist>,
    pub track_id: Id<Track>,
}
impl Input for OnPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("track_id", id::<Track>())
    }
}

pub struct PlaylistId {
    pub playlist_id: Id<Playlist>,
}
impl Input for PlaylistId {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists())
    }
}

pub fn playlists() -> Router<Playlists> {
    let playlists = router::<Playlists>("playlists");
    let signed_in = playlists.guard("signed_in", |ctx, _db| when(ctx.user.is_empty(), || refuse("sign in first")));
    let owned = signed_in.provide("owned", |ctx, db, input: &PlaylistId| {
        db.playlist
            .get((input.playlist_id,))
            .filter(|row| row.user_id.eq(ctx.user))
            .or_refuse("not your playlist")
    });
    playlists.routes((
        // Trims; refuses an empty name; a second one by the same person
        // with the same name is a no-op, so a second device's default
        // playlist is not a duplicate and the first keeps its id.
        signed_in.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            db.playlist
                .insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                    created_ms: ctx.now("created_ms"),
                })
                .on((Playlist::user_id, Playlist::name))
        }),
        // After everything already on it, which is what makes the rebase
        // visible: add while offline and it lands after what arrived.
        owned.input::<OnPlaylist>().mutation("add_to_playlist", |ctx, db, input, playlist| {
            let playlist_item = db
                .playlist_item
                .filter(PlaylistItem::playlist_id.eq(playlist.id))
                .order_by(PlaylistItem::pos.desc())
                .first();
            db.playlist_item.insert(PlaylistItem {
                playlist_id: playlist.id,
                track_id: input.track_id,
                pos: playlist_item.map_or(0, |row| row.pos).add(1),
                added_ms: ctx.now("added_ms"),
                user_id: ctx.user,
            })
        }),
        owned.input::<OnPlaylist>().mutation("remove_from_playlist", |_ctx, db, input, playlist| {
            db.playlist_item.delete((playlist.id, input.track_id))
        }),
        signed_in.query("playlists", |ctx, db, _input: ()| {
            db.playlist.filter(Playlist::user_id.eq(ctx.user)).order_by(Playlist::name.asc()).all()
        }),
        owned.input::<PlaylistId>().query("playlist_items", |_ctx, db, _input, playlist| {
            db.playlist_item
                .filter(PlaylistItem::playlist_id.eq(playlist.id))
                .order_by((PlaylistItem::pos.asc(), PlaylistItem::track_id.asc()))
                .all()
        }),
    ))
}
