package harken.gen

import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

fun library(): Router<Library> {
    val library = router<Library>("library")
    return library.routes(
        library.query("library") { _, db, _ ->
            db.track
                .orderBy(Track.artist.asc(), Track.album.asc(), Track.title.asc())
                .all()
        },
    )
}
