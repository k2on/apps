import SwiftUI

/// Procedural cover art: two hues, a pattern and a symbol (or set type).
struct Art: Hashable {
    enum Style: Int, Hashable { case rings, blobs, stripes, type }
    var hue: Double
    var hue2: Double
    var symbol: String
    var style: Style
    var label: String? = nil
    var sublabel: String? = nil

    var top: Color { Color(hue: hue, saturation: 0.52, brightness: 0.88) }
    var bottom: Color { Color(hue: hue2, saturation: 0.72, brightness: 0.42) }
    var paper: Color { Color(hue: hue, saturation: 0.12, brightness: 0.95) }
    var ink: Color { Color(hue: hue2, saturation: 0.65, brightness: 0.35) }
}

struct Artist: Identifiable, Hashable {
    let id: String
    let name: String
    let genre: String
    let art: Art
}

struct Composer: Identifiable, Hashable {
    var id: String { name }
    let name: String
    let years: String?
}

struct Album: Identifiable, Hashable {
    let id: String
    let title: String
    let artistID: String
    let artistName: String
    let year: Int
    let genre: String
    let added: Date
    let art: Art
    var trackIDs: [String]
}

struct TimedLine: Identifiable, Hashable {
    let id: Int
    let time: TimeInterval
    let text: String
    var speaker: String? = nil
}

struct Track: Identifiable, Hashable {
    let id: String
    let title: String
    let artistID: String
    let artistName: String
    let albumID: String
    let albumTitle: String
    let composer: String
    let number: Int
    let duration: TimeInterval
    var plays: Int
    let added: Date
    var isFavorite: Bool
    let lyrics: [TimedLine]
}

enum DownloadState: Hashable {
    case none
    case downloading(Double)
    case downloaded
}

struct Show: Identifiable, Hashable {
    enum Kind: String, Hashable { case podcast = "Podcast", sermons = "Sermons" }
    let id: String
    let title: String
    let author: String
    let kind: Kind
    let summary: String
    let feed: String
    let art: Art
    var episodeIDs: [String]
}

struct Episode: Identifiable, Hashable {
    let id: String
    let showID: String
    let number: Int
    let title: String
    let summary: String
    let date: Date
    let duration: TimeInterval
    /// 0 unplayed, 1 played, between: in progress.
    var progress: Double
    var download: DownloadState
    let scripture: String?
    let transcript: [TimedLine]

    var isUnplayed: Bool { progress == 0 }
    var isInProgress: Bool { progress > 0 && progress < 1 }
}

enum PlaylistKind: Hashable {
    case favorites, recentlyAdded, mostPlayed
    case user([String])
}

struct Playlist: Identifiable, Hashable {
    let id: String
    let name: String
    let symbol: String
    let summary: String
    let kind: PlaylistKind
    let art: Art
    var isSmart: Bool { if case .user = kind { return false } else { return true } }
}

struct Device: Identifiable, Hashable {
    let id: String
    let name: String
    let symbol: String
    let room: String
}

/// Something one person is playing somewhere in the house.
struct Activity: Hashable {
    var item: PlayItem
    var deviceID: String
    var position: TimeInterval
    var isPlaying: Bool
}

struct Member: Identifiable, Hashable {
    let id: String
    let name: String
    let role: String
    let hue: Double
    var activity: Activity?
    var lastSeen: String
    var initials: String { String(name.prefix(1)) }
}

enum PlayItem: Hashable {
    case track(String)
    case episode(String)
}

/// What the player shows for an item, whatever it is.
struct ItemInfo {
    let title: String
    let subtitle: String
    let art: Art
    let duration: TimeInterval
    let isSpoken: Bool
    let lines: [TimedLine]
    let route: Route?
}
