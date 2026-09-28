// Bodies the emitter must refuse.
package selfdemo

import demo.Demo
import demo.Playlist
import demo.PlaylistId
import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

fun twice(): Module {
    val r = router<Demo>("demo")
    return Module(
        r.routes(
            r.mutation("m") { ctx, db, _ ->
                db.playlist.insert(Playlist(id = ctx.newId("id"), name = ctx.user, userId = ctx.newId<Playlist>("id").toText()))
            },
        ),
    )
}

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
