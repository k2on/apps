package harken.gen

import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

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
    val mediaId: Id<Media>,
) : Input {
    companion object : Input.Of<AddToPlaylist> {
        override fun schema(): Schema<AddToPlaylist> =
            obj(field("playlist_id", id<Playlist>().exists()), field("media_id", id<Media>()))
    }
}

class RemoveFromPlaylist(
    val playlistId: Id<Playlist>,
    val mediaId: Id<Media>,
) : Input {
    companion object : Input.Of<RemoveFromPlaylist> {
        override fun schema(): Schema<RemoveFromPlaylist> =
            obj(field("playlist_id", id<Playlist>().exists()), field("media_id", id<Media>()))
    }
}

class PlaylistsOf(
    val mediaId: Id<Media>,
) : Input {
    companion object : Input.Of<PlaylistsOf> {
        override fun schema(): Schema<PlaylistsOf> = obj(field("media_id", id<Media>()))
    }
}

class PlaylistInput(
    val playlistId: Id<Playlist>,
) : Input {
    companion object : Input.Of<PlaylistInput> {
        override fun schema(): Schema<PlaylistInput> =
            obj(field("playlist_id", id<Playlist>().exists()))
    }
}

fun numbered(name: Text, n: Int): Text =
    helper("numbered", "name" to name, "n" to n) { name, n ->
        concat(list(name, lit(" ("), n.toText(), lit(")")))
    }

fun freeNumber(names: List<Text>, name: Text): Int =
    helper("free_number", "names" to names, "name" to name) { names, name ->
        names.fold(names.len().add(1)) { acc, x ->
            pick(
                names.contains(numbered(name, names.filter { x2 -> x2.le(x) }.len())),
                acc,
                acc.min(names.filter { x2 -> x2.le(x) }.len()))
        }
    }

fun playlistName(names: List<Text>, name: Text): Text =
    helper("playlist_name", "names" to names, "name" to name) { names, name ->
        pick(names.contains(name), numbered(name, freeNumber(names, name)), name)
    }

fun playlists(): Router<Harken> {
    val playlists = router<Harken>("playlists")
    val owned =
        playlists.provide("owned") { ctx, db, input: Owned ->
            db.playlist
                .get(input.playlistId)
                .filter { row -> row.userId.eq(ctx.user) }
                .orRefuse("not your playlist")
        }
    return playlists.routes(
        playlists.input<CreatePlaylist>().mutation("create_playlist") { ctx, db, input ->
            val playlist = db.playlist.orderBy(Playlist.pos.desc()).first()
            val playlist2 = db.playlist.filter(Playlist.userId.eq(ctx.user)).all()
            db.playlist.insert(
                Playlist(
                    id = ctx.newId("id"),
                    name = playlistName(playlist2.map { row -> row.name }, input.name),
                    pos = playlist.mapOr(0) { row -> row.pos }.add(1),
                    createdMs = ctx.now("created_ms"),
                    userId = ctx.user))
        },
        owned.input<AddToPlaylist>().mutation("add_to_playlist") { ctx, db, input, playlist ->
            val media = db.media.exists(input.mediaId)
            `when`(media) {
                val playlistItem =
                    db.playlistItem
                        .filter(PlaylistItem.playlistId.eq(playlist.id))
                        .orderBy(PlaylistItem.pos.desc())
                        .first()
                db.playlistItem.insert(
                    PlaylistItem(
                        playlistId = playlist.id,
                        mediaId = input.mediaId,
                        pos = playlistItem.mapOr(0) { row -> row.pos }.add(1),
                        addedMs = ctx.now("added_ms"),
                        userId = ctx.user))
            }
        },
        owned.input<RemoveFromPlaylist>().mutation("remove_from_playlist") { _, db, input, playlist
            ->
            db.playlistItem.delete(playlist.id, input.mediaId)
        },
        playlists.query("playlists") { ctx, db, _ ->
            db.playlist.filter(Playlist.userId.eq(ctx.user)).orderBy(Playlist.pos.asc()).all()
        },
        playlists.input<PlaylistsOf>().query("playlists_of") { ctx, db, input ->
            val playlistItem = db.playlistItem.filter(PlaylistItem.mediaId.eq(input.mediaId)).all()
            db.playlist
                .filter(Playlist.userId.eq(ctx.user))
                .orderBy(Playlist.pos.asc())
                .all()
                .filter { row -> playlistItem.any { row2 -> row2.playlistId.eq(row.id) } }
        },
        owned.input<PlaylistInput>().query("playlist") { _, db, _, playlist ->
            val playlistItem = db.playlistItem.filter(PlaylistItem.playlistId.eq(playlist.id)).all()
            db.media
                .all()
                .filter { row -> playlistItem.any { row2 -> row2.mediaId.eq(row.id) } }
                .sortBy { row ->
                    playlistItem
                        .filter { row2 -> row2.mediaId.eq(row.id) }
                        .first()
                        .mapOr(0) { row2 -> row2.pos }
                }
                .map { row -> libraryEntry(row, playlistItem) }
        },
    )
}
