// The domain as the screens see it, and nothing of Apple's but Foundation:
// the rows of harken's views as the screens draw them — the library's and
// the playlists' typed, UniFFI's own records with no JSON (`LibraryEntry`,
// `Playlist`), the rest as the generated types (`AlbumsEntry`,
// `ArtistsEntry`, `TrackDetailsEntry`, `Play`) — and every write one of the
// domain's functions by name, through the generated, typed calls
// (`Gen/Harken.swift`, `arkdb gen swift`), run by the Rust kernel the
// phone embeds (`harken/ffi`), returning the entry's id so that where it
// stands can be asked later. Where a page is, how a track's file and a
// length are said. The twin of `harken/android/.../Phone.kt`, `Library.kt`'s
// `Page` and `Media.kt`; type-checked on Linux (`tools/typecheck.sh`).
import Foundation

// MARK: rows

/// A row of the `library`, `album`, `artist`, `playlist` and `search` views:
/// something playable as a list draws it. `playlistPos` is where it sits on
/// the playlist the list was read against, when it is on it.
extension LibraryEntry: Identifiable {
    /// The track as the listening session carries it, on `album`.
    func queued(album: String) -> Track {
        Track(album: album, creator: creator, durationMs: durationMs, file: file, id: id, title: title)
    }
}

extension Playlist: Identifiable {}

extension AlbumsEntry: Identifiable, Hashable {
    public var id: String { name }
    public func hash(into h: inout Hasher) { h.combine(name) }

    /// The album `track` is on, by its details: the album's own row, found by
    /// its name — or, while the albums have not caught up with a song that
    /// names a new one, an album made of the name. None for a track on no album.
    static func of(_ track: LibraryEntry, details: [Id: TrackDetailsEntry], byName: [String: AlbumsEntry]) -> AlbumsEntry? {
        guard let name = details[track.id]?.album, !name.isEmpty else { return nil }
        return byName[name] ?? AlbumsEntry(art: "", creator: track.creator, name: name, tracks: 0)
    }
}

extension ArtistsEntry: Identifiable, Hashable {
    public var id: String { name }
    public func hash(into h: inout Hasher) { h.combine(name) }
}

extension Play {
    /// The track's id, when the play names one this library could have.
    var trackId: Id? { track.count == 32 ? Id(track) : nil }
}

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

// MARK: the domain's functions

enum Phone {
    /// What a phone calls the playlist it makes for somebody who has none.
    static let defaultPlaylist = "Favorites"

    /// The playlist a list is read against when there is none: `library` marks nothing.
    static var noPlaylist: Id { Id.none }

    static func library(_ h: Harken) throws -> LiveView<LibraryEntry> { try h.library(Library(playlistId: noPlaylist)) }

    /// A page's songs: an album's or an artist's by name, or a playlist's.
    static func album(_ h: Harken, _ name: String) throws -> LiveView<LibraryEntry> { try h.album(AlbumInput(playlistId: noPlaylist, name: name)) }

    static func artist(_ h: Harken, _ name: String) throws -> LiveView<LibraryEntry> { try h.artist(Artist(playlistId: noPlaylist, name: name)) }

    static func playlist(_ h: Harken, _ id: Id) throws -> LiveView<LibraryEntry> { try h.playlist(PlaylistInput(playlistId: id)) }

    /// A page's view; none for every song, which is the library's own.
    static func view(_ h: Harken, _ page: Page) throws -> LiveView<LibraryEntry>? {
        switch page {
        case let .album(name): return try album(h, name)
        case let .artist(name): return try artist(h, name)
        case let .playlist(id): return try playlist(h, id)
        case .songs: return nil
        }
    }

    /// A view's rows once, and the view let go; none when it is refused.
    static func once<E>(_ v: () throws -> LiveView<E>) -> [E] {
        (try? v().rows()) ?? []
    }

    /// The library's rows whose title or creator holds `needle` (the domain's `search`).
    static func search(_ h: Harken, _ needle: String) -> [LibraryEntry] {
        once { try h.search(Search(playlistId: noPlaylist, needle: needle)) }
    }

    /// Which of the caller's playlists a track is on; nothing when it is refused.
    static func playlistsOf(_ h: Harken, media: Id) -> [Playlist] {
        once { try h.playlistsOf(PlaylistsOf(mediaId: media)) }
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

    /// The diagnostics' word for where a change stands.
    static func standing(_ st: Standing) -> String {
        switch st {
        case .pending: return "not synced yet"
        case .confirmed: return "saved"
        case .unknown: return "unknown"
        case .rejected(let why): return "not saved: " + why
        }
    }
}

// MARK: pages

/// Somewhere a screen goes, and a list it plays from.
enum Page: Hashable {
    case album(String)
    case artist(String)
    case playlist(Id)
    /// Every song, in the order they were added.
    case songs

    /// Where a track was played from, as the listening session and the plays
    /// carry it — the desktop's address-bar spelling (`#album/Water%20Music`),
    /// so a page named on one device is the same page on another. A playlist
    /// by its name; one this phone cannot name is nowhere in particular.
    func place(_ playlists: [Playlist]) -> String {
        switch self {
        case let .album(name): return "#album/" + Places.encode(name)
        case let .artist(name): return "#artist/" + Places.encode(name)
        case let .playlist(id):
            guard let p = playlists.first(where: { $0.id == id }) else { return "" }
            return "#playlist/" + Places.encode(p.name)
        case .songs: return "#library"
        }
    }

    /// The page a place names; nil for nowhere, or a page this phone has not got.
    static func of(place: String, playlists: [Playlist]) -> Page? {
        var body = Substring(place)
        if body.hasPrefix("#") { body = body.dropFirst() }
        let kind = body.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? ""
        let rest = body.firstIndex(of: "/").map { String(body[body.index(after: $0)...]) } ?? ""
        let name = Places.decode(rest)
        switch kind {
        case "album": return name.isEmpty ? nil : .album(name)
        case "artist": return name.isEmpty ? nil : .artist(name)
        case "playlist": return playlists.first(where: { $0.name == name }).map { .playlist($0.id) }
        case "library": return .songs
        default: return nil
        }
    }
}

/// A place's name, percent-encoded as `rust/arkui`'s `url::encode` writes it: what is not unreserved.
enum Places {
    static func unreserved(_ c: UInt8) -> Bool {
        (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) || (c >= 0x30 && c <= 0x39) || c == 0x2D || c == 0x5F || c == 0x2E || c == 0x7E
    }

    static func encode(_ s: String) -> String {
        var out = ""
        for b in Array(s.utf8) {
            if unreserved(b) { out.unicodeScalars.append(Unicode.Scalar(b)) } else { out += String(format: "%%%02X", b) }
        }
        return out
    }

    static func decode(_ s: String) -> String {
        let u = Array(s.utf8)
        var bytes = [UInt8]()
        var i = 0
        while i < u.count {
            if u[i] == 0x25, i + 2 < u.count, let n = UInt8(String(decoding: u[(i + 1)...(i + 2)], as: UTF8.self), radix: 16) {
                bytes.append(n)
                i += 3
                continue
            }
            bytes.append(u[i])
            i += 1
        }
        return String(decoding: bytes, as: UTF8.self)
    }
}

// MARK: files and lengths

/// Where a track's bytes are. A track's `file` is a path under the server's
/// media root, and `/media/` serves that same path back, with range requests
/// and no token — so every device joins it to its own server, each segment
/// percent-encoded, because a `#` or a `?` in a file name would otherwise
/// end the path mid-name. A whole URL is already an answer and passes through.
enum Files {
    /// The URL `file` is at on `server`.
    static func url(_ server: String, _ file: String) -> String {
        if file.isEmpty || file.hasPrefix("http://") || file.hasPrefix("https://") { return file }
        var base = server
        while base.hasSuffix("/") { base.removeLast() }
        var out = base + "/media"
        for part in file.split(separator: "/", omittingEmptySubsequences: false) {
            out += "/" + Places.encode(String(part))
        }
        return out
    }
}

/// How a length and a count are said.
enum Spell {
    /// A track's length as a row shows it: `m:ss`, or `h:mm:ss` past an hour.
    static func clock(_ ms: Int64) -> String {
        let s = max(ms, 0) / 1000
        func two(_ n: Int64) -> String { n < 10 ? "0\(n)" : "\(n)" }
        if s >= 3600 { return "\(s / 3600):\(two(s / 60 % 60)):\(two(s % 60))" }
        return "\(s / 60):\(two(s % 60))"
    }

    /// A record's length, as a header says it: `48 min`, `2 hr 30 min`.
    static func length(_ ms: Int64) -> String {
        let min = (ms + 30_000) / 60_000
        if min < 1 { return "under a minute" }
        if min < 60 { return "\(min) min" }
        if min % 60 == 0 { return "\(min / 60) hr" }
        return "\(min / 60) hr \(min % 60) min"
    }

    /// `1 song`, `12 songs`.
    static func count(_ n: Int, _ one: String, _ many: String? = nil) -> String {
        "\(n) \(n == 1 ? one : (many ?? one + "s"))"
    }

    /// The facts a page's header says, joined: "12 songs · 48 min".
    static func facts(_ parts: [String?]) -> String {
        parts.compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// A list's whole length, or none for nothing.
    static func total(_ tracks: [LibraryEntry]) -> String? {
        let ms = tracks.reduce(Int64(0)) { $0 + $1.durationMs }
        return ms > 0 ? length(ms) : nil
    }
}

/// FNV-1a over UTF-16 code units: `arkui::art::hash`, which every client
/// pins, so one album is one derived square on every screen — and the name
/// a cover is kept under on the disk.
enum Art {
    static func hash(_ text: String) -> UInt32 {
        var h: UInt32 = 0x811c_9dc5
        for unit in text.utf16 {
            h ^= UInt32(unit)
            h = h &* 0x0100_0193
        }
        return h
    }

    /// Which of `count` gradients `seed` draws as.
    static func pick(_ count: Int, _ seed: String) -> Int { Int(hash(seed) % UInt32(max(count, 1))) }
}

/// What a search found, in three groups: the songs the domain's `search`
/// answered, and the held albums and artists whose names hold the needle.
struct Results: Equatable {
    let needle: String
    let songs: [LibraryEntry]
    let albums: [AlbumsEntry]
    let artists: [ArtistsEntry]

    var isEmpty: Bool { songs.isEmpty && albums.isEmpty && artists.isEmpty }

    static func of(_ needle: String, songs: [LibraryEntry], albums: [AlbumsEntry], artists: [ArtistsEntry]) -> Results {
        let n = needle.trimmingCharacters(in: .whitespaces)
        func holds(_ s: String) -> Bool { s.range(of: n, options: .caseInsensitive) != nil }
        return Results(needle: n, songs: songs, albums: albums.filter { holds($0.name) || holds($0.creator) }, artists: artists.filter { holds($0.name) })
    }
}

/// The last few searches, newest first, once each: this phone's, and nobody else's.
enum Recent {
    static let most = 8

    static func adding(_ needle: String, to list: [String]) -> [String] {
        let n = needle.trimmingCharacters(in: .whitespaces)
        if n.isEmpty { return list }
        return Array(([n] + list.filter { $0.caseInsensitiveCompare(n) != .orderedSame }).prefix(most))
    }
}
