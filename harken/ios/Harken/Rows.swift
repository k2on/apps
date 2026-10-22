// The domain as the screens see it, and nothing SwiftUI: rows of harken's
// views read into the types the screens draw, and every write one of the
// domain's functions by name — the generated, typed calls over the bridge
// (`Gen/Harken.swift`, `ark-dev gen swift`), run by the Rust kernel the phone
// embeds (`harken/ffi`) — returning the entry's id so that where it stands
// can be asked later. The phone's twin of `harken/android/.../Phone.kt`.
import Foundation

/// Something playable as a list draws it: a row of the `library` or
/// `playlist` view as the domain answers it — UniFFI's record, read with no
/// JSON, and Hashable as UniFFI declares it. `playlistPos` is where it sits
/// on the playlist the list was read against, when it is on it.
extension LibraryEntry: Identifiable {
    var duration: String {
        let s = Int(durationMs / 1000)
        return String(format: "%d:%02d", s / 60, s % 60)
    }
}

extension Playlist: Identifiable {}

/// What a write came to: the entry, whose standing the peer answers, or the local refusal.
enum Wrote: Equatable {
    case entry(String)
    case refused(String)
}

/// One change this phone made, and what to call it beside its standing.
struct Authored: Identifiable, Hashable {
    let id: String
    let what: String
}

/// A refusal's sentence.
func why(_ error: Error) -> String {
    if let r = error as? Refusal, case let .Refused(why: w) = r { return w }
    return "\(error)"
}

enum Phone {
    /// What a phone calls the playlist it makes for somebody who has none.
    static let defaultPlaylist = "Favorites"

    /// The playlist a list is read against when there is none: `library` marks nothing.
    static let noPlaylist = Id.none

    /// A view's rows once, and the view let go; none when it is refused.
    static func once<E, T>(_ v: () throws -> View<E>, _ of: (E) -> T) -> [T] {
        (try? v().rows().map(of)) ?? []
    }

    /// Which of the caller's playlists a track is on; nothing when it is refused.
    static func playlistsOf(_ h: Harken, media: Id) -> [Playlist] {
        once({ try h.playlistsOf(PlaylistsOf(mediaId: media)) }) { $0 }
    }

    /// What the form validator says of a name as it is typed: the sentence
    /// `create_playlist`'s own checks would refuse it with, or nil. A name
    /// the person already has passes: the log numbers it "Name (1)".
    static func nameProblem(_ h: Harken, _ name: String) -> String? {
        h.peer.check(name: "playlists.create_playlist", input: CreatePlaylist(name: name).json)
    }

    private static func wrote(_ f: () throws -> String) -> Wrote {
        do {
            return .entry(try f())
        } catch {
            return .refused(why(error))
        }
    }

    static func createPlaylist(_ h: Harken, _ name: String) -> Wrote {
        wrote { try h.createPlaylist(CreatePlaylist(name: name)) }
    }

    static func addToPlaylist(_ h: Harken, _ playlist: Id, _ media: Id) -> Wrote {
        wrote { try h.addToPlaylist(AddToPlaylist(playlistId: playlist, mediaId: media)) }
    }

    static func removeFromPlaylist(_ h: Harken, _ playlist: Id, _ media: Id) -> Wrote {
        wrote { try h.removeFromPlaylist(RemoveFromPlaylist(playlistId: playlist, mediaId: media)) }
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
