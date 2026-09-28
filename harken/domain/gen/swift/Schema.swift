import ArkAuthoring

public struct Library {
    public var track: Table<Track>
}
extension Library: Scope {
    public static let NAME = "library"
}

public struct Playlists {
    public var playlist: Table<Playlist>
    public var playlistItem: Table<PlaylistItem>
}
extension Playlists: Scope {
    public static let NAME = "playlists"
}

public struct Track {
    public var id: Id<Track>
    public var title: Text
    public var artist: Text
    public var album: Opt<Text>
    public var durationMs: Int
    public var file: Text
    public var addedMs: Int
    public var userId: Text
}
extension Track: Row {
    public static let NAME = "track"
    public typealias Key = Id<Track>
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.id)
            .text(Self.title)
            .text(Self.artist)
            .text(Self.album)
            .nullable()
            .int(Self.durationMs)
            .text(Self.file)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.id)
            .unique(Self.file)
    }
}
extension Track {
    public static let id = col<Track, Id<Track>>("id")
    public static let title = col<Track, Text>("title")
    public static let artist = col<Track, Text>("artist")
    public static let album = col<Track, Opt<Text>>("album")
    public static let durationMs = col<Track, Int>("duration_ms")
    public static let file = col<Track, Text>("file")
    public static let addedMs = col<Track, Int>("added_ms")
    public static let userId = col<Track, Text>("user_id")
}

public struct Playlist {
    public var id: Id<Playlist>
    public var name: Text
    public var userId: Text
    public var createdMs: Int
}
extension Playlist: Row {
    public static let NAME = "playlist"
    public typealias Key = Id<Playlist>
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.id)
            .text(Self.name)
            .text(Self.userId)
            .int(Self.createdMs)
            .key(Self.id)
            .unique(Self.userId, Self.name)
    }
}
extension Playlist {
    public static let id = col<Playlist, Id<Playlist>>("id")
    public static let name = col<Playlist, Text>("name")
    public static let userId = col<Playlist, Text>("user_id")
    public static let createdMs = col<Playlist, Int>("created_ms")
    public static let playlistItem = rel<Playlist, PlaylistItem>("playlist_item")
}

public struct PlaylistItem {
    public var playlistId: Id<Playlist>
    public var trackId: Id<Track>
    public var pos: Int
    public var addedMs: Int
    public var userId: Text
}
extension PlaylistItem: Row {
    public static let NAME = "playlist_item"
    public typealias Key = (Id<Playlist>, Id<Track>)
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.playlistId)
            .refs(Playlist.self)
            .id(Self.trackId)
            .int(Self.pos)
            .int(Self.addedMs)
            .text(Self.userId)
            .key(Self.playlistId, Self.trackId)
    }
}
extension PlaylistItem {
    public static let playlistId = col<PlaylistItem, Id<Playlist>>("playlist_id")
    public static let trackId = col<PlaylistItem, Id<Track>>("track_id")
    public static let pos = col<PlaylistItem, Int>("pos")
    public static let addedMs = col<PlaylistItem, Int>("added_ms")
    public static let userId = col<PlaylistItem, Text>("user_id")
}
