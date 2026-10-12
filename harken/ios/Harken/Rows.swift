// The domain as the screens see it, and nothing SwiftUI: rows of harken's
// queries read into the types the screens draw, every write one of
// `harken.ark`'s mutations by name — the module interpreted by the Swift
// client, as every peer interprets it — returning the entry's id so that
// where it stands can be asked later. The phone's twin of
// `harken/android/.../Phone.kt`; type-checked on Linux (README.md).
import ArkDB
import ArkDBClient
import Foundation

/// Something playable as a list draws it: a row of the `library` or
/// `playlist` query (the domain's `LibraryEntry` record). `playlistPos` is
/// where it sits on the playlist the list was read against, when it is on it.
struct Track: Identifiable, Hashable {
    let id: Id
    let title: String
    let creator: String
    let kind: String
    let durationMs: Int64
    let file: String
    let playlistPos: Int64?

    static func of(_ v: Value) -> Track? {
        guard case .record(let r) = v, case .id(let id)? = r["id"], case .text(let title)? = r["title"], case .text(let creator)? = r["creator"], case .text(let kind)? = r["kind"], case .int(let d)? = r["duration_ms"], case .text(let file)? = r["file"] else { return nil }
        var pos: Int64? = nil
        if case .int(let p)? = r["playlist_pos"] { pos = p }
        return Track(id: id, title: title, creator: creator, kind: kind, durationMs: d, file: file, playlistPos: pos)
    }

    var duration: String {
        let s = Int(durationMs / 1000)
        return String(format: "%d:%02d", s / 60, s % 60)
    }
}

/// A row of `playlists`.
struct Playlist: Identifiable, Hashable {
    let id: Id
    let name: String
    let userId: String
    let createdMs: Int64

    static func of(_ v: Value) -> Playlist? {
        guard case .record(let r) = v, case .id(let id)? = r["id"], case .text(let name)? = r["name"], case .text(let user)? = r["user_id"], case .int(let made)? = r["created_ms"] else { return nil }
        return Playlist(id: id, name: name, userId: user, createdMs: made)
    }
}

/// What a write came to: the entry, whose standing `Peer.standing` answers, or the local refusal.
enum Wrote: Equatable {
    case entry(Id)
    case refused(String)
}

/// One change this phone made, and what to call it beside its standing.
struct Authored: Identifiable, Hashable {
    let id: Id
    let what: String
}

enum Phone {
    /// What a phone calls the playlist it makes for somebody who has none.
    static let defaultPlaylist = "Favorites"

    /// The playlist a list is read against when there is none: `library` marks nothing.
    static let noPlaylist = Id.nilId

    /// The `library` view's arguments, read against a playlist.
    static func libraryArgs(_ against: Id?) -> [String: Value] { ["playlist_id": .id(against ?? noPlaylist)] }

    /// The `playlist` view's arguments.
    static func playlistArgs(_ id: Id) -> [String: Value] { ["playlist_id": .id(id)] }

    /// Which of the caller's playlists a track is on; nothing when it is refused.
    static func playlistsOf(_ p: Peer, media: Id) -> [Playlist] {
        guard case .list(let rows)? = try? p.query("playlists_of", ["media_id": .id(media)]) else { return [] }
        return rows.compactMap(Playlist.of)
    }

    /// What the form validator says of a name as it is typed: the message
    /// `create_playlist`'s own checks would refuse it with, or nil. A name
    /// the person already has passes: the log numbers it "Name (1)".
    static func nameProblem(_ p: Peer, _ name: String) -> String? {
        guard let c = try? p.check("create_playlist", ["name": .text(name)]) else { return nil }
        return c.messages.first { $0.0 == "name" }?.1
    }

    private static func wrote(_ f: () throws -> Id) -> Wrote {
        do {
            return .entry(try f())
        } catch let r as Refused {
            return .refused(r.description)
        } catch {
            return .refused("\(error)")
        }
    }

    static func createPlaylist(_ p: Peer, _ name: String) -> Wrote {
        wrote { try p.mutate("create_playlist", ["name": .text(name)]) }
    }

    static func addToPlaylist(_ p: Peer, _ playlist: Id, _ media: Id) -> Wrote {
        wrote { try p.mutate("add_to_playlist", ["playlist_id": .id(playlist), "media_id": .id(media)]) }
    }

    static func removeFromPlaylist(_ p: Peer, _ playlist: Id, _ media: Id) -> Wrote {
        wrote { try p.mutate("remove_from_playlist", ["playlist_id": .id(playlist), "media_id": .id(media)]) }
    }

    /// What a screen writes beside an item: nothing once it is in the log,
    /// the reason word for word when it never will be.
    static func caption(_ st: Standing) -> String? {
        switch st {
        case .pending: return "not synced yet"
        case .confirmed, .unknown: return nil
        case .rejected(let why): return "not saved: " + why
        }
    }
}
