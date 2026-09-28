import ArkAuthoring

public struct CreatePlaylist {
    public var name: Text
}
extension CreatePlaylist: Input {
    public static var schema: Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name").max(120))
    }
}

public struct OnPlaylist {
    public var playlistId: Id<Playlist>
    public var trackId: Id<Track>
}
extension OnPlaylist: Input {
    public static var schema: Object<Self> {
        object().field("playlist_id", id(Playlist.self).exists()).field("track_id", id(Track.self))
    }
}

public struct PlaylistId {
    public var playlistId: Id<Playlist>
}
extension PlaylistId: Input {
    public static var schema: Object<Self> {
        object().field("playlist_id", id(Playlist.self).exists())
    }
}

public func playlists() -> Router<Playlists> {
    let playlists = router(Playlists.self, "playlists")
    let signedIn = playlists.guard("signed_in") { ctx, _ in when(ctx.user.isEmpty()) { refuse("sign in first") } }
    let owned = signedIn.provide("owned") { (ctx, db, input: PlaylistId) in
        db.playlist
            .get(input.playlistId)
            .filter { row in row.userId.eq(ctx.user) }
            .orRefuse("not your playlist")
    }
    return playlists.routes(
        signedIn.input(CreatePlaylist.self).mutation("create_playlist") { ctx, db, input in
            db.playlist
                .insert(Playlist(
                    id: ctx.newId("id"),
                    name: input.name,
                    userId: ctx.user,
                    createdMs: ctx.now("created_ms")
                ))
                .on(Playlist.userId, Playlist.name)
        },
        owned.input(OnPlaylist.self).mutation("add_to_playlist") { ctx, db, input, playlist in
            let playlistItem = db
                .playlistItem
                .filter(PlaylistItem.playlistId.eq(playlist.id))
                .orderBy(PlaylistItem.pos.desc())
                .first()
            return db.playlistItem.insert(PlaylistItem(
                playlistId: playlist.id,
                trackId: input.trackId,
                pos: playlistItem.mapOr(0) { row in row.pos }.add(1),
                addedMs: ctx.now("added_ms"),
                userId: ctx.user
            ))
        },
        owned.input(OnPlaylist.self).mutation("remove_from_playlist") { _, db, input, playlist in
            db.playlistItem.delete(playlist.id, input.trackId)
        },
        signedIn.query("playlists") { ctx, db, _ in
            db.playlist.filter(Playlist.userId.eq(ctx.user)).orderBy(Playlist.name.asc()).all()
        },
        owned.input(PlaylistId.self).query("playlist_items") { _, db, _, playlist in
            db.playlistItem
                .filter(PlaylistItem.playlistId.eq(playlist.id))
                .orderBy(PlaylistItem.pos.asc(), PlaylistItem.trackId.asc())
                .all()
        }
    )
}
