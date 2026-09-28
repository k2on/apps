import ArkAuthoring

public func library() -> Router<Library> {
    let library = router(Library.self, "library")
    return library.routes(
        library.query("library") { _, db, _ in
            db.track.orderBy(Track.artist.asc(), Track.album.asc(), Track.title.asc()).all()
        }
    )
}
