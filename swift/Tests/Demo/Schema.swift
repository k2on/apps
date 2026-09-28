import ArkAuthoring

public struct Demo {
    public var playlist: Table<Playlist>
    public var item: Table<Item>
}
extension Demo: Scope {
    public static let NAME = "demo"
}

public struct Playlist {
    public var id: Id<Playlist>
    public var name: Text
    public var userId: Text
}
extension Playlist: Row {
    public static let NAME = "playlist"
    public typealias Key = Id<Playlist>
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.id)
            .text(Self.name)
            .text(Self.userId)
            .key(Self.id)
            .unique(Self.userId, Self.name)
    }
}
extension Playlist {
    public static let id = col<Playlist, Id<Playlist>>("id")
    public static let name = col<Playlist, Text>("name")
    public static let userId = col<Playlist, Text>("user_id")
    public static let item = rel<Playlist, Item>("item")
}

public struct Item {
    public var playlistId: Id<Playlist>
    public var trackId: Text
    public var pos: Int
}
extension Item: Row {
    public static let NAME = "item"
    public typealias Key = (Id<Playlist>, Text)
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.playlistId)
            .refs(Playlist.self)
            .text(Self.trackId)
            .int(Self.pos)
            .key(Self.playlistId, Self.trackId)
            .unique(Self.playlistId, Self.pos)
    }
}
extension Item {
    public static let playlistId = col<Item, Id<Playlist>>("playlist_id")
    public static let trackId = col<Item, Text>("track_id")
    public static let pos = col<Item, Int>("pos")
}
