import ArkAuthoring

public struct CreatePlaylist {
    public var name: Text
}
extension CreatePlaylist: Input {
    public static var schema: Object<Self> {
        object().field("name", text().trim().min(1, "a playlist needs a name"))
    }
}

public struct AddToPlaylist {
    public var playlistId: Id<Playlist>
    public var trackId: Text
}
extension AddToPlaylist: Input {
    public static var schema: Object<Self> {
        object()
            .field("playlist_id", id(Playlist.self).exists())
            .field("track_id", text().min(1))
    }
}

public struct PlaylistId {
    public var playlistId: Id<Playlist>
}
extension PlaylistId: Input {
    public static var schema: Object<Self> {
        object().field("playlist_id", id(Playlist.self))
    }
}

public func demo() -> Router<Demo> {
    let demo = router(Demo.self, "demo")
    return demo.routes(
        demo.input(CreatePlaylist.self).mutation("create_playlist") { ctx, db, input in
            db.playlist
                .insert(Playlist(id: ctx.newId("id"), name: input.name, userId: ctx.user))
                .on(Playlist.userId, Playlist.name)
        },
        demo.input(AddToPlaylist.self).mutation("add_to_playlist") { ctx, db, input in
            let item = db.item.filter(Item.playlistId.eq(input.playlistId)).orderBy(Item.pos.desc()).first()
            return db.item.insert(Item(
                playlistId: input.playlistId,
                trackId: input.trackId,
                pos: item.mapOr(0) { row in row.pos }.add(1)
            ))
        },
        demo.input(PlaylistId.self).query("items") { ctx, db, input in
            db.item.filter(Item.playlistId.eq(input.playlistId)).orderBy(Item.pos.asc()).all()
        }
    )
}
