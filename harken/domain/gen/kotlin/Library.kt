package harken.gen

import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

class Library(
    val playlistId: Id<Playlist>,
) : Input {
    companion object : Input.Of<Library> {
        override fun schema(): Schema<Library> = obj(field("playlist_id", id<Playlist>()))
    }
}

class LibraryEntry(
    val addedMs: Int,
    val creator: Text,
    val durationMs: Int,
    val file: Text,
    val id: Id<Media>,
    val kind: Text,
    val playlistPos: Opt<Int>,
    val pos: Int,
    val title: Text,
    val userId: Text,
) : Record {
    companion object : Record.Of<LibraryEntry> {
        override fun fields(): Fields<LibraryEntry> =
            fields<LibraryEntry>()
                .field("added_ms", int())
                .field("creator", text())
                .field("duration_ms", int())
                .field("file", text())
                .field("id", id<Media>())
                .field("kind", text())
                .field("playlist_pos", opt(int()))
                .field("pos", int())
                .field("title", text())
                .field("user_id", text())
    }
}

fun libraryEntry(media: Media, items: List<PlaylistItem>): LibraryEntry =
    helper("library_entry", "media" to media, "items" to items) { media, items ->
        LibraryEntry(
            addedMs = media.addedMs,
            creator = media.creator,
            durationMs = media.durationMs,
            file = media.file,
            id = media.id,
            kind = media.kind,
            playlistPos =
                items.filter { row -> row.mediaId.eq(media.id) }.first().map { row -> row.pos },
            pos = media.pos,
            title = media.title,
            userId = media.userId)
    }

fun library(): Router<Harken> {
    val library = router<Harken>("library")
    return library.routes(
        library.input<Library>().query("library") { _, db, input ->
            val playlistItem =
                db.playlistItem.filter(PlaylistItem.playlistId.eq(input.playlistId)).all()
            db.media.orderBy(Media.pos.asc()).all().map { row -> libraryEntry(row, playlistItem) }
        },
    )
}
