import Foundation
import ArkDB
import ArkDBClient

// The domain as the screens see it: typed rows read out of the generated
// queries, and the three mutations, each through its generated function.
// Nothing here is SwiftUI, so this file compiles on Linux beside
// `HarkenGen.swift` — which is how it was checked (see README.md).

struct Track: Identifiable, Hashable {
    let id: Id
    let title: String
    let artist: String
    let album: String?
    let durationMs: Int64
    let file: String

    init?(_ v: Value) {
        guard case .record(let m) = v, case .id(let id)? = m["id"], case .text(let title)? = m["title"],
              case .text(let artist)? = m["artist"], case .int(let d)? = m["duration_ms"], case .text(let file)? = m["file"] else { return nil }
        self.id = id
        self.title = title
        self.artist = artist
        if case .text(let a)? = m["album"] { album = a } else { album = nil }
        durationMs = d
        self.file = file
    }

    var duration: String {
        let s = Int(durationMs / 1000)
        return String(format: "%d:%02d", s / 60, s % 60)
    }
}

struct Playlist: Identifiable, Hashable {
    let id: Id
    let name: String
    let userId: String

    init?(_ v: Value) {
        guard case .record(let m) = v, case .id(let id)? = m["id"], case .text(let name)? = m["name"], case .text(let user)? = m["user_id"] else { return nil }
        self.id = id
        self.name = name
        self.userId = user
    }
}

struct PlaylistItem: Hashable {
    let playlistId: Id
    let trackId: Id
    let pos: Int64

    init?(_ v: Value) {
        guard case .record(let m) = v, case .id(let p)? = m["playlist_id"], case .id(let t)? = m["track_id"], case .int(let pos)? = m["pos"] else { return nil }
        playlistId = p
        trackId = t
        self.pos = pos
    }
}

/// An item joined to its track. `track` is nil when the track has not
/// arrived: the two are in different scopes, and a playlist item names a
/// track across the boundary unchecked (harken/README.md).
struct PlaylistRow: Identifiable, Hashable {
    let item: PlaylistItem
    let track: Track?
    var id: Id { return item.trackId }
    var title: String { return track?.title ?? "(unavailable)" }
}

enum HarkenDomain {
    /// The module the generated code was made from, as the session takes it.
    static var moduleBytes: [UInt8] { return Hex.decode(HarkenGen.moduleBytes) ?? [] }

    /// What tells the session to run intents through `HarkenGen` — its own
    /// and every replayed one whose function the phone was generated with.
    static let generated = Generated(functions: HarkenGen.functions, apply: HarkenGen.apply, query: HarkenGen.query)

    static let scopes: [ScopeName] = ["library", "playlists"]

    // MARK: reads, through the generated queries

    static func library(_ s: Session) throws -> [Track] {
        return try s.run { db in try HarkenGen.query("library", db, [:]) }.asList().compactMap(Track.init)
    }

    static func playlists(_ s: Session) throws -> [Playlist] {
        return try s.run { db in try HarkenGen.query("playlists", db, [:]) }.asList().compactMap(Playlist.init)
    }

    static func items(_ s: Session, of playlist: Id) throws -> [PlaylistItem] {
        return try s.run { db in try HarkenGen.query("playlist_items", db, ["playlist_id": .id(playlist)]) }.asList().compactMap(PlaylistItem.init)
    }

    /// The join a screen does itself: a map lookup on the track id.
    static func rows(_ items: [PlaylistItem], _ tracks: [Track]) -> [PlaylistRow] {
        var byId: [Id: Track] = [:]
        for t in tracks { byId[t.id] = t }
        return items.map { PlaylistRow(item: $0, track: byId[$0.trackId]) }
    }

    // MARK: writes, each through its generated mutator

    /// The session looks the function up by name for its scope, hash and
    /// autos, and runs the generated body as one transaction over the view.
    static func createPlaylist(_ s: Session, name: String) -> Refusal? {
        let args = HarkenGen.createPlaylistArgs(name: name)
        return s.mutate(name: "create_playlist", args: args) { db, ctx, autos in try HarkenGen.createPlaylist(db, ctx, autos, args) }
    }

    static func addToPlaylist(_ s: Session, playlist: Id, track: Id) -> Refusal? {
        let args = HarkenGen.addToPlaylistArgs(playlistId: playlist, trackId: track)
        return s.mutate(name: "add_to_playlist", args: args) { db, ctx, autos in try HarkenGen.addToPlaylist(db, ctx, autos, args) }
    }

    static func removeFromPlaylist(_ s: Session, playlist: Id, track: Id) -> Refusal? {
        let args = HarkenGen.removeFromPlaylistArgs(playlistId: playlist, trackId: track)
        return s.mutate(name: "remove_from_playlist", args: args) { db, ctx, autos in try HarkenGen.removeFromPlaylist(db, ctx, autos, args) }
    }
}
