import ArkAuthoring

public struct Library {
    public var playlistId: Id<Playlist>
}
extension Library: Input {
    public static var schema: Object<Self> {
        object().field("playlist_id", id(Playlist.self))
    }
}

public struct LibraryEntry {
    public var addedMs: Int
    public var creator: Text
    public var durationMs: Int
    public var file: Text
    public var id: Id<Media>
    public var kind: Text
    public var playlistPos: Opt<Int>
    public var pos: Int
    public var title: Text
    public var userId: Text
}
extension LibraryEntry: Record {
    public static func fields() -> Fields<Self> {
        Fields<Self>()
            .field("added_ms", int())
            .field("creator", text())
            .field("duration_ms", int())
            .field("file", text())
            .field("id", ArkAuthoring.id(Media.self))
            .field("kind", text())
            .field("playlist_pos", opt(int()))
            .field("pos", int())
            .field("title", text())
            .field("user_id", text())
    }
}

public func libraryEntry(_ media: Media, _ items: List<PlaylistItem>) -> LibraryEntry {
    helper("library_entry", ("media", media), ("items", items)) { media, items in
        LibraryEntry(
            addedMs: media.addedMs, creator: media.creator, durationMs: media.durationMs, file: media.file,
            id: media.id, kind: media.kind,
            playlistPos: items.filter { row in row.mediaId.eq(media.id) }.first().map { row in row.pos },
            pos: media.pos, title: media.title, userId: media.userId)
    }
}

public func library() -> Router<Harken> {
    let library = router(Harken.self, "library")
    return library.routes(
        library.input(Library.self).query("library") { _, db, input in
            let playlistItem = db.playlistItem.filter(PlaylistItem.playlistId.eq(input.playlistId)).all()
            return db.media.orderBy(Media.pos.asc()).all().map { row in libraryEntry(row, playlistItem) }
        }
    )
}
