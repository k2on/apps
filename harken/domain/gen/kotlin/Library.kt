package harken.gen

import dev.arkdb.authoring.*

fun library(): Router<Library> {
    val library = router<Library>("library")
    return library.routes(
        library.query("library") { _, db, _ ->
            db.track.orderBy(Track.artist.asc(), Track.album.asc(), Track.title.asc()).all()
        },
    )
}
