import Foundation
import ArkDB
import ArkDBClient
import ArkAuthoring

// The domain as the screens see it. `../domain/gen/swift` is harken's domain
// in the authoring vocabulary (what `arkc gen swift --only …` prints), and is
// compiled into this app: `module()` is that domain, `emit()` the module the
// session opens, and `procedures()` every procedure as native Swift — so an
// entry this phone authors, and every entry replayed from the log whose
// function the phone has, runs as the Swift below `library()` and
// `playlists()` rather than through an interpreter. Nothing here is SwiftUI,
// so this file compiles on Linux beside the domain (see README.md).
//
// This file imports ArkDB and ArkAuthoring together, so the names both
// define — `Id`, `Ctx`, `Module` — are written qualified, and the host's
// `Int`/`Bool` are `Swift.Int`/`Swift.Bool` (the vocabulary's shadow them).

/// A track as a screen draws it: read out of the `library` query's rows
/// through the domain's own `Track` row type.
struct LibraryTrack: Identifiable, Hashable {
    let id: ArkDB.Id
    let title: String
    let artist: String
    let album: String?
    let durationMs: Int64
    let file: String

    init?(_ v: Value) {
        let t = Track(repr: .v(v))
        guard let id = t.id.raw, let title = t.title.string, let artist = t.artist.string,
              let d = t.durationMs.int, let file = t.file.string else { return nil }
        self.id = id
        self.title = title
        self.artist = artist
        album = t.album.get?.string
        durationMs = d
        self.file = file
    }

    var duration: String {
        let s = Swift.Int(durationMs / 1000)
        return String(format: "%d:%02d", s / 60, s % 60)
    }
}

/// A playlist as a screen draws it.
struct PlaylistSummary: Identifiable, Hashable {
    let id: ArkDB.Id
    let name: String
    let userId: String

    init?(_ v: Value) {
        let p = Playlist(repr: .v(v))
        guard let id = p.id.raw, let name = p.name.string, let user = p.userId.string else { return nil }
        self.id = id
        self.name = name
        self.userId = user
    }
}

/// An item of a playlist.
struct PlaylistEntry: Hashable {
    let playlistId: ArkDB.Id
    let trackId: ArkDB.Id
    let pos: Int64

    init?(_ v: Value) {
        let i = PlaylistItem(repr: .v(v))
        guard let p = i.playlistId.raw, let t = i.trackId.raw, let pos = i.pos.int else { return nil }
        playlistId = p
        trackId = t
        self.pos = pos
    }
}

/// An item joined to its track. `track` is nil when the track has not
/// arrived: the two are in different scopes, and a playlist item names a
/// track across the boundary unchecked (harken/README.md).
struct PlaylistRow: Identifiable, Hashable {
    let item: PlaylistEntry
    let track: LibraryTrack?
    var id: ArkDB.Id { return item.trackId }
    var title: String { return track?.title ?? "(unavailable)" }
}

enum Harken {
    /// The domain, authored in Swift: `../domain/gen/swift`.
    static let domain = module()

    /// The module the session opens: the domain's `emit()`.
    static let moduleBytes: [UInt8] = domain.emit()

    /// Every procedure the phone has, natively, by hash.
    static let procedures: [(FnHash, Procedure)] = domain.procedures()

    static var moduleHash: String { return Hex.encode(domain.hash) }
    static var procedureNames: [String] { return procedures.map { $0.1.function.name } }

    static let scopes: [ScopeName] = ["library", "playlists"]

    // MARK: reads, through the domain's queries

    static func library(_ s: Session) throws -> [LibraryTrack] {
        return try s.query(name: "library").asList().compactMap(LibraryTrack.init)
    }

    static func playlists(_ s: Session) throws -> [PlaylistSummary] {
        return try s.query(name: "playlists").asList().compactMap(PlaylistSummary.init)
    }

    static func items(_ s: Session, of playlist: ArkDB.Id) throws -> [PlaylistEntry] {
        let input = PlaylistId(playlistId: ArkAuthoring.Id(playlist))
        return try s.query(name: "playlist_items", args: input.args).asList().compactMap(PlaylistEntry.init)
    }

    /// The join a screen does itself: a map lookup on the track id.
    static func rows(_ items: [PlaylistEntry], _ tracks: [LibraryTrack]) -> [PlaylistRow] {
        var byId: [ArkDB.Id: LibraryTrack] = [:]
        for t in tracks { byId[t.id] = t }
        return items.map { PlaylistRow(item: $0, track: byId[$0.trackId]) }
    }

    // MARK: writes, each an input of the domain's own type

    /// What the new-playlist form says about a name as it is typed: the
    /// `create_playlist` input's own checks (trim, then at least one
    /// character, at most 120), run by the form validator — the message the
    /// mutation would refuse with, before it is attempted. Nil when the name
    /// would pass.
    static func nameProblem(_ s: Session, _ name: String) -> String? {
        let input = CreatePlaylist(name: Text(name))
        guard let (messages, _) = try? s.validate(name: "create_playlist", partial: input.args) else { return nil }
        return messages.first?.1
    }

    /// The session looks the function up by name for its scope, hash and
    /// autos, and applies it natively through the procedure it holds.
    static func createPlaylist(_ s: Session, name: String) -> Refusal? {
        return s.mutate(name: "create_playlist", args: CreatePlaylist(name: Text(name)).args)
    }

    static func addToPlaylist(_ s: Session, playlist: ArkDB.Id, track: ArkDB.Id) -> Refusal? {
        let input = OnPlaylist(playlistId: ArkAuthoring.Id(playlist), trackId: ArkAuthoring.Id(track))
        return s.mutate(name: "add_to_playlist", args: input.args)
    }

    static func removeFromPlaylist(_ s: Session, playlist: ArkDB.Id, track: ArkDB.Id) -> Refusal? {
        let input = OnPlaylist(playlistId: ArkAuthoring.Id(playlist), trackId: ArkAuthoring.Id(track))
        return s.mutate(name: "remove_from_playlist", args: input.args)
    }
}
