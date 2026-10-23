// What the tests build sessions and rows from: the generated types made by
// hand, as the reconciler's view would carry them.
@testable import Harken
import Foundation

enum Fx {
    static func id(_ n: Int) -> Id { Id(String(format: "%032x", n)) }

    static func track(_ n: Int, album: String = "Water Music") -> Track {
        Track(album: album, creator: "Handel", durationMs: 200_000, file: "music/\(n).flac", id: id(n), title: "Song \(n)")
    }

    static func entry(_ n: Int) -> LibraryEntry {
        LibraryEntry(addedMs: 0, creator: "Handel", durationMs: 200_000, file: "music/\(n).flac", id: id(n), kind: "song",
                     playlistPos: nil, pos: Int64(n), title: "Song \(n)", userId: "ana")
    }

    static func device(_ id: String, _ name: String, kind: String = "phone", canPlay: Bool = true, volume: Int64? = nil,
                       muted: Bool = false, family: String? = nil, follows: String? = nil) -> Present {
        Present(canPlay: canPlay, canVolume: volume != nil, family: family, follows: follows, id: id, kind: kind, muted: muted,
                name: name, user: "ana", volume: volume ?? 0)
    }

    static func active(_ queue: [Track], at: Int = 0, output: Output, clock: Clock = .playing(atMs: 1_000, since: 10_000), from: String = "") -> Active {
        Active(at: Int64(at), clock: clock, from: from, output: output, queue: queue)
    }

    static func session(_ devices: [Present], _ active: Active?, scrub: [String: Int64] = [:]) -> LiveSession {
        var s = LiveSession()
        for d in devices { s.devices[d.id] = d }
        s.active = active
        s.scrub = scrub
        return s
    }

    static func joined(_ s: LiveSession, me: String = "me") -> Heard {
        var h = Heard()
        h.me = me
        h.joined = true
        h.linked = true
        h.listening = s
        h.active = s.active
        return h
    }

    static let login = Login(token: "t", session: "s1", user: "ana", name: "Ana", email: "ana@example.com", roles: ["listener"], expiresMs: 42)
}
