package demo

import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

class CreatePlaylist(
    val name: Text,
) : Input {
    companion object : Input.Of<CreatePlaylist> {
        override fun schema(): Schema<CreatePlaylist> = obj(field("name", text().trim().min(1, "a playlist needs a name")))
    }
}

class AddToPlaylist(
    val playlistId: Id<Playlist>,
    val trackId: Text,
) : Input {
    companion object : Input.Of<AddToPlaylist> {
        override fun schema(): Schema<AddToPlaylist> = obj(
            field("playlist_id", id<Playlist>().exists()),
            field("track_id", text().min(1)),
        )
    }
}

class PlaylistId(
    val playlistId: Id<Playlist>,
) : Input {
    companion object : Input.Of<PlaylistId> {
        override fun schema(): Schema<PlaylistId> = obj(field("playlist_id", id<Playlist>()))
    }
}

fun demo(): Router<Demo> {
    val demo = router<Demo>("demo")
    return demo.routes(
        demo.input<CreatePlaylist>().mutation("create_playlist") { ctx, db, input ->
            db.playlist
                .insert(Playlist(id = ctx.newId("id"), name = input.name, userId = ctx.user))
                .on(Playlist.userId, Playlist.name)
        },
        demo.input<AddToPlaylist>().mutation("add_to_playlist") { ctx, db, input ->
            val item = db.item.filter(Item.playlistId.eq(input.playlistId)).orderBy(Item.pos.desc()).first()
            db.item.insert(
                Item(
                    playlistId = input.playlistId,
                    trackId = input.trackId,
                    pos = item.mapOr(0) { row -> row.pos }.add(1),
                ),
            )
        },
        demo.input<PlaylistId>().query("items") { ctx, db, input ->
            db.item.filter(Item.playlistId.eq(input.playlistId)).orderBy(Item.pos.asc()).all()
        },
    )
}
