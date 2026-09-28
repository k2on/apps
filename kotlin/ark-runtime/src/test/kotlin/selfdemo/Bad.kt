// Bodies the emitter must refuse.
package selfdemo

import demo.Demo
import demo.Playlist
import demo.PlaylistId
import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

fun readInMap(): Module {
    val r = router<Demo>("demo")
    return Module(
        r.routes(
            r.input<PlaylistId>().query("q") { _, db, input ->
                db.playlist.all().map { p -> db.playlist.exists(p.id) }
            },
        ),
    )
}

fun onLate(): Module {
    val r = router<Demo>("demo")
    return Module(
        r.routes(
            r.input<PlaylistId>().mutation("m") { ctx, db, input ->
                val w = db.playlist.insert(Playlist(id = input.playlistId, name = ctx.user, userId = ctx.user))
                db.playlist.exists(input.playlistId)
                w.on(Playlist.userId, Playlist.name)
            },
        ),
    )
}
