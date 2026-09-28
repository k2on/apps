import ArkAuthoring

public struct Owned {
    public var playlistId: Id<Playlist>
}
extension Owned: Input {
    public static var schema: Object<Self> {
        object().field("playlist_id", id(Playlist.self))
    }
}

public struct CreatePlaylist {
    public var name: Text
}
extension CreatePlaylist: Input {
    public static var schema: Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name").max(120))
    }
}

public struct AddToPlaylist {
    public var playlistId: Id<Playlist>
    public var mediaId: Id<Media>
}
extension AddToPlaylist: Input {
    public static var schema: Object<Self> {
        object().field("playlist_id", id(Playlist.self).exists()).field("media_id", id(Media.self))
    }
}

public struct RemoveFromPlaylist {
    public var playlistId: Id<Playlist>
    public var mediaId: Id<Media>
}
extension RemoveFromPlaylist: Input {
    public static var schema: Object<Self> {
        object().field("playlist_id", id(Playlist.self).exists()).field("media_id", id(Media.self))
    }
}

public struct PlaylistsOf {
    public var mediaId: Id<Media>
}
extension PlaylistsOf: Input {
    public static var schema: Object<Self> {
        object().field("media_id", id(Media.self))
    }
}

public struct PlaylistInput {
    public var playlistId: Id<Playlist>
}
extension PlaylistInput: Input {
    public static var schema: Object<Self> {
        object().field("playlist_id", id(Playlist.self).exists())
    }
}

public func numbered(_ name: Text, _ n: Int) -> Text {
    helper("numbered", ("name", name), ("n", n)) { name, n in concat(list([name, " (", n.toText(), ")"])) }
}

public func freeNumber(_ names: List<Text>, _ name: Text) -> Int {
    helper("free_number", ("names", names), ("name", name)) { names, name in
        names.fold(names.len().add(1)) { acc, x in
            pick(
                names.contains(numbered(name, names.filter { x2 in x2.le(x) }.len())), acc,
                acc.min(names.filter { x2 in x2.le(x) }.len()))
        }
    }
}

public func playlistName(_ names: List<Text>, _ name: Text) -> Text {
    helper("playlist_name", ("names", names), ("name", name)) { names, name in
        pick(names.contains(name), numbered(name, freeNumber(names, name)), name)
    }
}

public func playlists() -> Router<Harken> {
    let playlists = router(Harken.self, "playlists")
    let owned = playlists.provide("owned") { (ctx, db, input: Owned) in
        db.playlist
            .get(input.playlistId)
            .filter { row in row.userId.eq(ctx.user) }
            .orRefuse("not your playlist")
    }
    return playlists.routes(
        playlists.input(CreatePlaylist.self).mutation("create_playlist") { ctx, db, input in
            let playlist = db.playlist.orderBy(Playlist.pos.desc()).first()
            let playlist2 = db.playlist.filter(Playlist.userId.eq(ctx.user)).all()
            return db.playlist.insert(
                Playlist(
                    id: ctx.newId("id"), name: playlistName(playlist2.map { row in row.name }, input.name),
                    pos: playlist.mapOr(0) { row in row.pos }.add(1), createdMs: ctx.now("created_ms"), userId: ctx.user
                ))
        },
        owned.input(AddToPlaylist.self).mutation("add_to_playlist") { ctx, db, input, playlist in
            let media = db.media.exists(input.mediaId)
            return when(media) {
                let playlistItem = db.playlistItem
                    .filter(PlaylistItem.playlistId.eq(playlist.id))
                    .orderBy(PlaylistItem.pos.desc())
                    .first()
                return db.playlistItem.insert(
                    PlaylistItem(
                        playlistId: playlist.id, mediaId: input.mediaId,
                        pos: playlistItem.mapOr(0) { row in row.pos }.add(1), addedMs: ctx.now("added_ms"),
                        userId: ctx.user))
            }
        },
        owned.input(RemoveFromPlaylist.self).mutation("remove_from_playlist") { _, db, input, playlist in
            db.playlistItem.delete(playlist.id, input.mediaId)
        },
        playlists.query("playlists") { ctx, db, _ in
            db.playlist.filter(Playlist.userId.eq(ctx.user)).orderBy(Playlist.pos.asc()).all()
        },
        playlists.input(PlaylistsOf.self).query("playlists_of") { ctx, db, input in
            let playlistItem = db.playlistItem.filter(PlaylistItem.mediaId.eq(input.mediaId)).all()
            return db.playlist
                .filter(Playlist.userId.eq(ctx.user))
                .orderBy(Playlist.pos.asc())
                .all()
                .filter { row in playlistItem.any { row2 in row2.playlistId.eq(row.id) } }
        },
        owned.input(PlaylistInput.self).query("playlist") { _, db, _, playlist in
            let playlistItem = db.playlistItem.filter(PlaylistItem.playlistId.eq(playlist.id)).all()
            return db.media
                .all()
                .filter { row in playlistItem.any { row2 in row2.mediaId.eq(row.id) } }
                .sortBy { row in
                    playlistItem.filter { row2 in row2.mediaId.eq(row.id) }.first().mapOr(0) { row2 in row2.pos }
                }
                .map { row in libraryEntry(row, playlistItem) }
        }
    )
}
