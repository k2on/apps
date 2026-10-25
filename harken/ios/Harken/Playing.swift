// What is playing, as the screens and the lock screen draw it, derived from
// one value — the reconciler's view of what plays (`Heard.active`): the
// session's while this phone is part of it, this phone's own player's while
// it is alone — so the mini player, the full player, the queue, Home's last
// played and the lock screen can never disagree. Nothing of Apple's but
// Foundation: every derivation is a function of its inputs, held by
// `HarkenTests`. The twin of the state half of Android's `Playback.kt`
// (`PlayerState`, `draw`, `where`) and of `HomeScreen.kt`'s `lastPlayed`.
import Foundation

/// A track as a screen draws it.
struct Playing: Equatable, Identifiable {
    let id: String
    let title: String
    let artist: String
    let album: String
    /// The cover's whole URL, or "" for none: the derived square, then.
    let art: String
    /// Its place in the session's queue.
    var index: Int = 0

    /// What its derived square is made from.
    var seed: String { album.isEmpty ? title : album }
}

enum Repeat: Equatable {
    case off, all, one

    var next: Repeat {
        switch self {
        case .off: return .all
        case .all: return .one
        case .one: return .off
        }
    }
}

/// Where the sound is, as the player's device button says it, while this phone is part of the session.
struct Where: Equatable {
    let name: String
    let kind: String
    /// This phone: making the sound, or — with the sound nowhere — where the next press plays.
    let here: Bool
    /// The device the sound belongs to has gone: "(not answering)".
    var away: Bool = false
    /// The sound is on its way to it: "connecting…".
    var moving: Bool = false

    var label: String {
        if moving { return "Connecting to \(name)…" }
        if away { return "\(name) (not answering)" }
        return name
    }
}

/// The player as the screens see it — all of it but the position, which is
/// counted from [clock] when it is drawn.
struct PlayerState: Equatable {
    var now: Playing? = nil
    var playing: Bool = false
    var buffering: Bool = false
    var durationMs: Int64 = 0
    var shuffle: Bool = false
    var repeatMode: Repeat = .off
    /// What plays after this one, in the order it will play.
    var upNext: [Playing] = []
    /// Where the sound is, while this phone is part of the session; none alone.
    var on: Where? = nil
    /// What is playing's volume; none, and no slider is drawn.
    var volume: Volume? = nil
    /// Where the position was at `at` (this phone's ms), and whether it runs on from there.
    var clockMs: Int64 = 0
    var clockAt: Int64 = 0
    var counting: Bool = false
    /// Where another device's scrubber is being dragged to, while one is.
    var scrubbing: Int64? = nil

    /// How far into the track at `now`.
    func position(_ now: Int64) -> Int64 {
        let p = counting ? clockMs + max(now - clockAt, 0) : clockMs
        return durationMs > 0 ? min(p, durationMs) : p
    }

    /// The session has no shuffle and no repeat: neither is drawn while this phone is part of it.
    var alone: Bool { on == nil }

    /// Whether next can do anything.
    var hasNext: Bool { !upNext.isEmpty || repeatMode != .off }

    /// How much of what is to come the queue shows.
    static let upNextMost = 100

    /// One view of what plays, drawn. `art` is an album's cover path by its
    /// name ("" for none), `server` what a path is joined to, `name` what this
    /// phone is called; `local` this phone's own player's volume, and
    /// `shuffle`, `repeatMode` and `buffering` what only it has — all three
    /// drawn alone, never joined.
    static func of(_ h: Heard, server: String, name: String, art: (String) -> String, local: Volume?,
                   shuffle: Bool, repeatMode: Repeat, buffering: Bool, now: Int64) -> PlayerState {
        func playing(_ t: Track, _ i: Int) -> Playing {
            let a = art(t.album)
            return Playing(id: t.id.hex, title: t.title, artist: t.creator, album: t.album, art: a.isEmpty ? "" : Files.url(server, a), index: i)
        }
        let volume = h.joined ? h.listening?.volume(h.me) : local
        let on = h.joined ? place(h, name: name) : nil
        var s = PlayerState(on: on, volume: volume)
        s.repeatMode = h.joined ? .off : repeatMode
        s.scrubbing = h.joined ? h.scrubbing : nil
        guard let a = h.active, let t = a.now else { return s }
        let at = a.index
        s.now = playing(t, at)
        s.playing = a.clock.playing
        s.buffering = !h.joined && buffering
        s.durationMs = t.durationMs
        s.shuffle = !h.joined && shuffle
        s.upNext = a.queue.enumerated().dropFirst(at + 1).prefix(upNextMost).map { playing($0.element, $0.offset) }
        s.clockMs = a.position(now)
        s.clockAt = now
        s.counting = a.counting
        return s
    }

    /// The device button: the output, or — with the sound nowhere — this phone, lit.
    static func place(_ h: Heard, name: String) -> Where {
        let s = h.listening
        switch s?.active?.output {
        case let .device(id, moving)?:
            if id == h.me { return Where(name: name, kind: "phone", here: true, moving: moving) }
            let d = s?.devices[id]
            let n = s?.followers(id).count ?? 0
            let named = d?.name ?? id
            return Where(name: n > 0 ? "\(named) + \(n)" : named, kind: d?.kind ?? "computer", here: false, moving: moving)
        case let .away(id, away)?:
            return Where(name: away, kind: "computer", here: id == h.me, away: true)
        case .nowhere?, nil:
            return Where(name: name, kind: "phone", here: true)
        }
    }

    /// What the mini player says under the title: the artist, or — the sound
    /// somewhere else — where it is.
    var under: String {
        guard let now = now else { return "" }
        if let on = on, !on.here { return on.moving || on.away ? on.label : "On \(on.name)" }
        return now.artist
    }

    /// The lock screen's and Control Center's title: what plays, and — on
    /// another device — where, as Android's notification says it.
    var lockScreenArtist: String {
        guard let now = now else { return "" }
        if let on = on, !on.here { return "\(now.artist) · Playing on \(on.name)" }
        return now.artist
    }
}

/// What was played last, and the page it was started from, so Home can offer
/// it back. `live` when it is the listening session's own track — then `on`
/// says where it is playing, if it is.
struct LastPlayed: Equatable {
    let track: Playing
    let from: Page?
    var live: Bool = false
    var on: String? = nil

    /// Home's last played: the session's track while this phone is part of a
    /// session that has one, else the newest play in the log. `art` is an
    /// album's cover by its name; `here` what this phone is called.
    static func of(_ h: Heard, played: Play?, playlists: [Playlist], art: (String) -> String, server: String, here: String) -> LastPlayed? {
        func cover(_ album: String) -> String {
            let a = art(album)
            return a.isEmpty ? "" : Files.url(server, a)
        }
        if h.joined, let s = h.listening, let a = s.active, let t = a.now {
            var on: String? = nil
            if s.sounding, let id = a.output.here {
                on = id == h.me ? "Playing on \(here)" : "Playing on \(s.devices[id]?.name ?? "another device")"
            }
            let p = Playing(id: t.id.hex, title: t.title, artist: t.creator, album: t.album, art: cover(t.album))
            return LastPlayed(track: p, from: Page.of(place: a.from, playlists: playlists), live: true, on: on)
        }
        guard let p = played else { return nil }
        let track = Playing(id: p.trackId?.hex ?? "", title: p.title, artist: p.creator, album: p.album, art: cover(p.album))
        return LastPlayed(track: track, from: Page.of(place: p.from, playlists: playlists))
    }

    /// Where it was played from, in words.
    func saying(_ playlists: [Playlist]) -> String {
        if let on = on { return on }
        switch from {
        case let .album(n)?: return "From \(n)"
        case let .artist(n)?: return "From \(n)"
        case .some(.recording), .some(.work): return "From a recording"
        case let .playlist(id)?: return playlists.first { $0.id == id }.map { "From \($0.name)" } ?? "From a playlist"
        default: return "From your library"
        }
    }
}

/// A queue started from a list: the list the track was started from, with
/// each track's album; shuffled, a queue in another order with this track
/// first — the session's way and the player's alike, so what is drawn is
/// what plays.
enum Started {
    static func queue(_ list: [LibraryEntry], at: Int, shuffled: Bool, album: (LibraryEntry) -> String,
                      shuffle: ([LibraryEntry]) -> [LibraryEntry] = { $0.shuffled() }) -> (queue: [Track], at: Int)? {
        if list.isEmpty { return nil }
        let start = min(max(at, 0), list.count - 1)
        var order = list
        if shuffled {
            var rest = list
            rest.remove(at: start)
            order = [list[start]] + shuffle(rest)
        }
        return (order.map { $0.queued(album: album($0)) }, shuffled ? 0 : start)
    }
}
