package harken.gen

import dev.arkdb.authoring.*

class Owned(
    val playlistId: Id<Playlist>,
) : Input {
    companion object : Input.Of<Owned> {
        override fun schema(): Schema<Owned> = obj(field("playlist_id", id<Playlist>()))
    }
}

class CreatePlaylist(
    val name: Text,
) : Input {
    companion object : Input.Of<CreatePlaylist> {
        override fun schema(): Schema<CreatePlaylist> =
            obj(field("name", text().trim().min(1).why("a playlist needs a name").max(120)))
    }
}

class AddToPlaylist(
    val playlistId: Id<Playlist>,
    val trackId: Id<Track>,
) : Input {
    companion object : Input.Of<AddToPlaylist> {
        override fun schema(): Schema<AddToPlaylist> =
            obj(field("playlist_id", id<Playlist>().exists()), field("track_id", id<Track>()))
    }
}

class RemoveFromPlaylist(
    val playlistId: Id<Playlist>,
    val trackId: Id<Track>,
) : Input {
    companion object : Input.Of<RemoveFromPlaylist> {
        override fun schema(): Schema<RemoveFromPlaylist> =
            obj(field("playlist_id", id<Playlist>().exists()), field("track_id", id<Track>()))
    }
}

class PlaylistItems(
    val playlistId: Id<Playlist>,
) : Input {
    companion object : Input.Of<PlaylistItems> {
        override fun schema(): Schema<PlaylistItems> =
            obj(field("playlist_id", id<Playlist>().exists()))
    }
}

fun playlists(): Router<Playlists> {
    val playlists = router<Playlists>("playlists")
    val signedIn =
        playlists.guard("signed_in") { ctx, _ ->
            `when`(ctx.user.isEmpty()) { refuse("sign in first") }
        }
    val owned =
        signedIn.provide("owned") { ctx, db, input: Owned ->
            db.playlist
                .get(input.playlistId)
                .filter { row -> row.userId.eq(ctx.user) }
                .orRefuse("not your playlist")
        }
    return playlists.routes(
        signedIn.input<CreatePlaylist>().mutation("create_playlist") { ctx, db, input ->
            db.playlist
                .insert(
                    Playlist(
                        id = ctx.newId("id"),
                        name = input.name,
                        userId = ctx.user,
                        createdMs = ctx.now("created_ms")))
                .on(Playlist.userId, Playlist.name)
        },
        owned.input<AddToPlaylist>().mutation("add_to_playlist") { ctx, db, input, playlist ->
            val playlistItem =
                db.playlistItem
                    .filter(PlaylistItem.playlistId.eq(playlist.id))
                    .orderBy(PlaylistItem.pos.desc())
                    .first()
            db.playlistItem.insert(
                PlaylistItem(
                    playlistId = playlist.id,
                    trackId = input.trackId,
                    pos = playlistItem.mapOr(0) { row -> row.pos }.add(1),
                    addedMs = ctx.now("added_ms"),
                    userId = ctx.user))
        },
        owned.input<RemoveFromPlaylist>().mutation("remove_from_playlist") { _, db, input, playlist
            ->
            db.playlistItem.delete(playlist.id, input.trackId)
        },
        signedIn.query("playlists") { ctx, db, _ ->
            db.playlist.filter(Playlist.userId.eq(ctx.user)).orderBy(Playlist.name.asc()).all()
        },
        owned.input<PlaylistItems>().query("playlist_items") { _, db, _, playlist ->
            db.playlistItem
                .filter(PlaylistItem.playlistId.eq(playlist.id))
                .orderBy(PlaylistItem.pos.asc(), PlaylistItem.trackId.asc())
                .all()
        },
    )
}
