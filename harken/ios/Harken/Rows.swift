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
// The domain's tables are the struct `Harken` (`Schema.swift`), so the
// app's own namespace is `Phone`. This file imports ArkDB and ArkAuthoring
// together, so the names both define — `Id`, `Ctx`, `Module` — are written
// qualified, and the host's `Int`/`Bool` are `Swift.Int`/`Swift.Bool` (the
// vocabulary's shadow them).

/// Something playable as a list draws it: a row of the `library`,
/// `playlist` or `artist` query, read through the domain's own record type
/// `LibraryEntry`. `playlistPos` is where it sits on the playlist the list
/// was read against, when it is on it.
struct LibraryTrack: Identifiable, Hashable {
    let id: ArkDB.Id
    let title: String
    let creator: String
    let kind: String
    let durationMs: Int64
    let file: String
    let playlistPos: Int64?

    init?(_ v: Value) {
        let e = LibraryEntry(repr: .v(v))
        guard let id = e.id.raw, let title = e.title.string, let creator = e.creator.string, let kind = e.kind.string,
              let d = e.durationMs.int, let file = e.file.string else { return nil }
        self.id = id
        self.title = title
        self.creator = creator
        self.kind = kind
        durationMs = d
        self.file = file
        playlistPos = e.playlistPos.get?.int
    }

    var duration: String {
        let s = Swift.Int(durationMs / 1000)
        return String(format: "%d:%02d", s / 60, s % 60)
    }
}

/// A playlist as a screen draws it: a row of `playlist`.
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

/// One change this phone made, and what to call it beside its standing.
struct Authored: Identifiable, Hashable {
    /// The entry's id, which `Session.standing` answers about.
    let id: ArkDB.Id
    let what: String
}

enum Phone {
    /// The domain, authored in Swift: `../domain/gen/swift`.
    static let domain = module()

    /// The module the session opens: the domain's `emit()`.
    static let moduleBytes: [UInt8] = domain.emit()

    /// Every procedure the phone has, natively, by hash.
    static let procedures: [(FnHash, Procedure)] = domain.procedures()

    static var moduleHash: String { return Hex.encode(domain.hash) }
    static var procedureNames: [String] { return procedures.map { $0.1.function.name } }

    /// The playlist a list is read against when there is none: `library`
    /// takes an id it does not look up, and no playlist has this one.
    static let noPlaylist = ArkDB.Id(bytes: [UInt8](repeating: 0, count: 16))!

    /// What a phone calls the playlist it makes for somebody who has none.
    static let defaultPlaylist = "Favorites"

    // MARK: reads, through the domain's queries

    /// The whole library, in the order things were added, read against a
    /// playlist: each track's `playlistPos` says whether it is on that one.
    static func library(_ s: Session, against playlist: ArkDB.Id? = nil) throws -> [LibraryTrack] {
        let input = Library(playlistId: ArkAuthoring.Id(playlist ?? noPlaylist))
        return try s.query(name: "library", args: input.args).asList().compactMap(LibraryTrack.init)
    }

    /// The caller's playlists, in the order they were made.
    static func playlists(_ s: Session) throws -> [PlaylistSummary] {
        return try s.query(name: "playlists").asList().compactMap(PlaylistSummary.init)
    }

    /// One playlist's contents in playlist order, as library rows; what is no
    /// longer in the library is not in it. Refused for a playlist that is
    /// not the caller's.
    static func playlist(_ s: Session, _ id: ArkDB.Id) throws -> [LibraryTrack] {
        let input = PlaylistInput(playlistId: ArkAuthoring.Id(id))
        return try s.query(name: "playlist", args: input.args).asList().compactMap(LibraryTrack.init)
    }

    /// Which of the caller's playlists a track is on: what makes the
    /// playlist menu a toggle.
    static func playlistsOf(_ s: Session, media: ArkDB.Id) throws -> [PlaylistSummary] {
        let input = PlaylistsOf(mediaId: ArkAuthoring.Id(media))
        return try s.query(name: "playlists_of", args: input.args).asList().compactMap(PlaylistSummary.init)
    }

    // MARK: writes, each an input of the domain's own type

    /// What the new-playlist form says about a name as it is typed: the
    /// `create_playlist` input's own checks (trim, then at least one
    /// character, at most 120), run by the form validator — the message the
    /// mutation would refuse with, before it is attempted. Nil when the name
    /// would pass. A name the person already has passes: the log makes it
    /// "Name (1)".
    static func nameProblem(_ s: Session, _ name: String) -> String? {
        let input = CreatePlaylist(name: Text(name))
        guard let (messages, _) = try? s.validate(name: "create_playlist", partial: input.args) else { return nil }
        return messages.first?.1
    }

    /// Each write returns the entry's id — which `standing` follows to
    /// confirmed or rejected-with-a-reason — or the local refusal.
    static func createPlaylist(_ s: Session, name: String) -> Result<ArkDB.Id, Refusal> {
        return s.author(name: "create_playlist", args: CreatePlaylist(name: Text(name)).args)
    }

    static func addToPlaylist(_ s: Session, playlist: ArkDB.Id, media: ArkDB.Id) -> Result<ArkDB.Id, Refusal> {
        let input = AddToPlaylist(playlistId: ArkAuthoring.Id(playlist), mediaId: ArkAuthoring.Id(media))
        return s.author(name: "add_to_playlist", args: input.args)
    }

    static func removeFromPlaylist(_ s: Session, playlist: ArkDB.Id, media: ArkDB.Id) -> Result<ArkDB.Id, Refusal> {
        let input = RemoveFromPlaylist(playlistId: ArkAuthoring.Id(playlist), mediaId: ArkAuthoring.Id(media))
        return s.author(name: "remove_from_playlist", args: input.args)
    }

    /// Make the default playlist, and only for somebody who has no playlist
    /// at all: a name that person already has would be renamed "Favorites
    /// (1)" by the log rather than refused, so the one question worth asking
    /// is whether they have any. `known` is the caller's word that the view
    /// holds every playlist of theirs (alone; or signed in and caught up) —
    /// asked before the log has arrived, every second device would make one.
    /// Nil when nothing was made.
    static func ensureDefault(_ s: Session, known: Swift.Bool) -> Result<ArkDB.Id, Refusal>? {
        guard known, let mine = try? playlists(s), mine.isEmpty else { return nil }
        return createPlaylist(s, name: defaultPlaylist)
    }

    /// Where a track goes when nobody said: the playlist, and the entry
    /// that made it when it was made just now.
    struct Target: Equatable {
        let playlist: ArkDB.Id
        let made: ArkDB.Id?
    }

    /// The playlist a track goes on when the person has none yet: the
    /// default, made now. For somebody signed out, this is the only way one
    /// is made for them — not on opening, so that a person who signs in
    /// later with playlists of their own is not handed a "Favorites (1)" for
    /// work they never did.
    static func playlistForAdding(_ s: Session) -> Result<Target, Refusal> {
        if let first = try? playlists(s).first { return .success(Target(playlist: first.id, made: nil)) }
        switch createPlaylist(s, name: defaultPlaylist) {
        case .failure(let why): return .failure(why)
        case .success(let entry):
            guard let made = try? playlists(s).first else { return .failure(.refused("no playlist to add to")) }
            return .success(Target(playlist: made.id, made: entry))
        }
    }

    // MARK: standing

    /// What a screen writes beside an item: nothing once it is in the log,
    /// and the reason, word for word, when it will never be.
    static func caption(_ st: Standing) -> String? {
        switch st {
        case .pending: return "not synced yet"
        case .confirmed, .unknown: return nil
        case .rejected(let why): return "not saved: " + why
        }
    }
}
