import SwiftUI

/// The household library, in memory, as served by the home server.
@Observable @MainActor
final class Library {
    private(set) var artists: [Artist] = []
    private(set) var albums: [Album] = []
    private(set) var tracks: [Track] = []
    private(set) var composers: [Composer] = []
    private(set) var shows: [Show] = []
    private(set) var episodes: [Episode] = []
    var playlists: [Playlist] = []
    var members: [Member] = []
    let devices: [Device]
    let meID = "maya"

    private var trackIndex: [String: Int] = [:]
    private var episodeIndex: [String: Int] = [:]
    private var albumIndex: [String: Int] = [:]

    init() {
        devices = [
            Device(id: "this", name: Library.thisDeviceName, symbol: Library.thisDeviceSymbol, room: "With you"),
            Device(id: "living-room", name: "Living Room", symbol: "hifispeaker.2.fill", room: "Downstairs"),
            Device(id: "kitchen", name: "Kitchen", symbol: "homepod.mini.fill", room: "Downstairs"),
            Device(id: "studio", name: "Studio", symbol: "desktopcomputer", room: "Upstairs"),
            Device(id: "irises-ipad", name: "Iris's iPad", symbol: "ipad", room: "Iris's room"),
            Device(id: "den-tv", name: "Den TV", symbol: "tv", room: "Den"),
        ]
        build()
    }

    static var thisDeviceName: String {
        #if os(macOS)
        "This Mac"
        #else
        UIDevice.current.userInterfaceIdiom == .pad ? "This iPad" : "This iPhone"
        #endif
    }

    static var thisDeviceSymbol: String {
        #if os(macOS)
        "macbook"
        #else
        UIDevice.current.userInterfaceIdiom == .pad ? "ipad" : "iphone"
        #endif
    }

    // MARK: Lookups

    func track(_ id: String) -> Track? { trackIndex[id].map { tracks[$0] } }
    func episode(_ id: String) -> Episode? { episodeIndex[id].map { episodes[$0] } }
    func album(_ id: String) -> Album? { albumIndex[id].map { albums[$0] } }
    func artist(_ id: String) -> Artist? { artists.first { $0.id == id } }
    func show(_ id: String) -> Show? { shows.first { $0.id == id } }
    func playlist(_ id: String) -> Playlist? { playlists.first { $0.id == id } }
    func device(_ id: String) -> Device? { devices.first { $0.id == id } }
    func member(_ id: String) -> Member? { members.first { $0.id == id } }

    func tracks(of album: Album) -> [Track] { album.trackIDs.compactMap(track) }
    func albums(by artistID: String) -> [Album] { albums.filter { $0.artistID == artistID }.sorted { $0.year > $1.year } }
    func tracks(by artistID: String) -> [Track] { tracks.filter { $0.artistID == artistID } }
    func tracks(composedBy name: String) -> [Track] { tracks.filter { $0.composer == name } }
    func episodes(of show: Show) -> [Episode] { show.episodeIDs.compactMap(episode) }
    func unplayedCount(_ show: Show) -> Int { episodes(of: show).filter(\.isUnplayed).count }

    func tracks(in playlist: Playlist) -> [Track] {
        switch playlist.kind {
        case .favorites: tracks.filter(\.isFavorite)
        case .recentlyAdded: Array(tracks.sorted { ($0.added, $1.number) > ($1.added, $0.number) }.prefix(30))
        case .mostPlayed: Array(tracks.sorted { $0.plays > $1.plays }.prefix(25))
        case .user(let ids): ids.compactMap(track)
        }
    }

    var recentlyAddedAlbums: [Album] { albums.sorted { $0.added > $1.added } }
    var latestEpisodes: [Episode] { episodes.sorted { $0.date > $1.date } }
    var inProgressEpisodes: [Episode] { latestEpisodes.filter(\.isInProgress) }
    var downloadedEpisodes: [Episode] { latestEpisodes.filter { $0.download != .none } }

    func info(_ item: PlayItem) -> ItemInfo? {
        switch item {
        case .track(let id):
            guard let t = track(id), let a = album(t.albumID) else { return nil }
            return ItemInfo(title: t.title, subtitle: "\(t.artistName) — \(t.albumTitle)", art: a.art,
                            duration: t.duration, isSpoken: false, lines: t.lyrics, route: .album(a.id))
        case .episode(let id):
            guard let e = episode(id), let s = show(e.showID) else { return nil }
            return ItemInfo(title: e.title, subtitle: s.title, art: s.art, duration: e.duration,
                            isSpoken: true, lines: e.transcript, route: .episode(e.id))
        }
    }

    // MARK: Changes

    func toggleFavorite(_ id: String) {
        guard let i = trackIndex[id] else { return }
        tracks[i].isFavorite.toggle()
    }

    func setDownload(_ id: String, _ state: DownloadState) {
        guard let i = episodeIndex[id] else { return }
        episodes[i].download = state
    }

    func markPlayed(_ id: String, _ played: Bool) {
        guard let i = episodeIndex[id] else { return }
        episodes[i].progress = played ? 1 : 0
    }

    func add(_ trackIDs: [String], to playlistID: String) {
        guard let i = playlists.firstIndex(where: { $0.id == playlistID }), case .user(let ids) = playlists[i].kind else { return }
        let p = playlists[i]
        playlists[i] = Playlist(id: p.id, name: p.name, symbol: p.symbol, summary: p.summary, kind: .user(ids + trackIDs), art: p.art)
    }

    @discardableResult
    func newPlaylist(named name: String, summary: String) -> Playlist {
        let p = Playlist(id: "user-\(playlists.count)", name: name, symbol: "music.note.list", summary: summary,
                         kind: .user([]), art: Art(hue: Seed.unit(name), hue2: Seed.unit(name + "2"), symbol: "music.note.list", style: .blobs))
        playlists.append(p)
        return p
    }

    // MARK: Building the sample library

    private func build() {
        for spec in SampleData.artists {
            artists.append(Artist(id: spec.id, name: spec.name, genre: spec.genre,
                                  art: Art(hue: spec.hue, hue2: spec.hue + 0.08, symbol: spec.symbol, style: .blobs)))
        }
        var composerNames: [String] = []
        for spec in SampleData.albums {
            let artist = artists.first { $0.id == spec.artist }!
            let classical = spec.genre == "Classical" || spec.genre == "Hymns"
            let art = Art(hue: spec.hue, hue2: spec.hue2, symbol: spec.symbol, style: spec.style,
                          label: spec.style == .type ? spec.title : nil,
                          sublabel: spec.style == .type ? (classical ? spec.composer.components(separatedBy: " ").last : artist.name) : nil)
            var album = Album(id: spec.id, title: spec.title, artistID: artist.id, artistName: artist.name, year: spec.year,
                              genre: spec.genre, added: Launch.daysAgo(spec.addedDaysAgo), art: art, trackIDs: [])
            for (n, raw) in spec.tracks.enumerated() {
                let parts = raw.components(separatedBy: "|")
                let title = parts[0]
                let composer = parts.count > 1 ? parts[1] : spec.composer
                let id = "\(spec.id)-\(n + 1)"
                let duration: Double = classical
                    ? Double(Seed.int(id, 140...560))
                    : Double(Seed.int(id, 150...290))
                let sung = !classical && spec.genre != "Ambient" && spec.genre != "Jazz" && !title.contains("Interlude")
                let popularity = spec.addedDaysAgo < 30 ? 6 : spec.addedDaysAgo / 12
                let track = Track(id: id, title: title, artistID: artist.id, artistName: artist.name, albumID: spec.id,
                                  albumTitle: spec.title, composer: composer, number: n + 1, duration: duration,
                                  plays: Seed.int(id + "plays", 0...max(4, popularity)), added: album.added,
                                  isFavorite: Seed.unit(id + "fav") < 0.16 || id == "harbor-lights-2",
                                  lyrics: sung ? lyrics(for: id, title: title) : [])
                trackIndex[id] = tracks.count
                tracks.append(track)
                album.trackIDs.append(id)
                if !composerNames.contains(composer) { composerNames.append(composer) }
            }
            albumIndex[album.id] = albums.count
            albums.append(album)
        }
        composers = composerNames.sorted().map { Composer(name: $0, years: SampleData.composerYears[$0]) }

        for spec in SampleData.shows {
            var show = Show(id: spec.id, title: spec.title, author: spec.author, kind: spec.kind, summary: spec.summary,
                            feed: spec.feed, art: Art(hue: spec.hue, hue2: spec.hue2, symbol: spec.symbol, style: spec.style,
                                                      label: spec.style == .type ? spec.title : nil,
                                                      sublabel: spec.style == .type ? "Sermons" : nil),
                            episodeIDs: [])
            let cadence = spec.id == "vespers" ? 1 : 7
            for (n, ep) in spec.episodes.enumerated() {
                let id = "\(spec.id)-\(n + 1)"
                let minutes = Seed.int(id, spec.minutes)
                let progress: Double
                switch n {
                case 0: progress = spec.id == "slow-science" ? 0.34 : 0
                case 1: progress = spec.id == "workbench" || spec.id == "grace-hill" ? 0.42 : (spec.id == "vespers" ? 0 : 1)
                case 2: progress = spec.id == "vespers" ? 0.7 : 1
                default: progress = Seed.unit(id) < 0.25 ? 0 : 1
                }
                let download: DownloadState = n == 0 && spec.id == "workbench" ? .downloading(0.62)
                    : (n < 3 || Seed.unit(id + "dl") < 0.2 ? .downloaded : .none)
                let lines = spec.transcript.enumerated().map { i, line in
                    TimedLine(id: i, time: 4 + Double(i) * 9.5 + Seed.unit(id + "\(i)") * 3, text: line.1, speaker: line.0)
                }
                let episode = Episode(id: id, showID: spec.id, number: spec.episodes.count - n, title: ep.0, summary: ep.1,
                                      date: Launch.daysAgo(n * cadence + (spec.id == "grace-hill" ? 2 : (spec.id == "slow-science" ? 1 : 0))),
                                      duration: Double(minutes * 60 + Seed.int(id + "s", 0...59)),
                                      progress: progress, download: download, scripture: ep.2, transcript: lines)
                episodeIndex[id] = episodes.count
                episodes.append(episode)
                show.episodeIDs.append(id)
            }
            shows.append(show)
        }

        playlists = [
            Playlist(id: "favorites", name: "Favorites", symbol: "star.fill", summary: "Songs anyone in the house starred.",
                     kind: .favorites, art: Art(hue: 0.02, hue2: 0.95, symbol: "star.fill", style: .rings)),
            Playlist(id: "recently-added", name: "Recently Added", symbol: "clock", summary: "The newest 30 songs on the server.",
                     kind: .recentlyAdded, art: Art(hue: 0.55, hue2: 0.62, symbol: "clock", style: .rings)),
            Playlist(id: "most-played", name: "Most Played", symbol: "chart.bar", summary: "The 25 songs the household plays most.",
                     kind: .mostPlayed, art: Art(hue: 0.75, hue2: 0.85, symbol: "flame", style: .rings)),
            Playlist(id: "sunday-morning", name: "Sunday Morning", symbol: "music.note.list", summary: "Slow coffee, open windows.",
                     kind: .user(["kitchen-hymns-2", "gymnopedies-1", "clair-de-lune-4", "harbor-lights-9", "low-tide-4",
                                  "field-recordings-5", "kitchen-hymns-4", "cello-suites-4", "small-hours-2", "copper-roads-5",
                                  "nocturnes-2", "wildflower-radio-4"]),
                     art: Art(hue: 0.12, hue2: 0.06, symbol: "sun.max", style: .blobs)),
            Playlist(id: "focus", name: "Deep Focus", symbol: "music.note.list", summary: "No words, no surprises.",
                     kind: .user(["goldberg-1", "goldberg-2", "goldberg-3", "field-recordings-1", "field-recordings-2",
                                  "field-recordings-3", "gymnopedies-4", "gymnopedies-5", "night-bus-5", "cello-suites-1",
                                  "low-tide-2", "small-hours-6", "four-seasons-2"]),
                     art: Art(hue: 0.45, hue2: 0.55, symbol: "brain", style: .stripes)),
            Playlist(id: "road-trip", name: "Road Trip", symbol: "music.note.list", summary: "For the long drive north.",
                     kind: .user(["copper-roads-1", "copper-roads-3", "northern-lines-1", "night-bus-1", "glasshouse-1",
                                  "harbor-lights-4", "paper-satellites-1", "wildflower-radio-1", "copper-roads-6",
                                  "northern-lines-4", "glasshouse-6", "night-bus-8", "harbor-lights-2", "saturday-cartoons-5"]),
                     art: Art(hue: 0.06, hue2: 0.58, symbol: "car", style: .stripes)),
        ]

        members = [
            Member(id: "maya", name: "Maya", role: "Owner", hue: 0.02, activity: nil, lastSeen: "Now"),
            Member(id: "jonah", name: "Jonah", role: "Adult", hue: 0.58,
                   activity: Activity(item: .episode("slow-science-1"), deviceID: "kitchen", position: 640, isPlaying: true),
                   lastSeen: "Now"),
            Member(id: "iris", name: "Iris", role: "Teen", hue: 0.80,
                   activity: Activity(item: .track("paper-satellites-3"), deviceID: "irises-ipad", position: 95, isPlaying: true),
                   lastSeen: "Now"),
            Member(id: "leo", name: "Leo", role: "Child", hue: 0.33,
                   activity: Activity(item: .track("saturday-cartoons-5"), deviceID: "den-tv", position: 61, isPlaying: false),
                   lastSeen: "Paused 20 min ago"),
        ]
    }

    private func lyrics(for id: String, title: String) -> [TimedLine] {
        let pool = SampleData.lyricPool
        let start = Seed.int(id, 0...(pool.count - 1))
        var lines: [TimedLine] = []
        var time = 11.0
        for i in 0..<18 {
            let text = i % 6 == 5 ? title : pool[(start + i * (i % 2 == 0 ? 1 : 5)) % pool.count]
            lines.append(TimedLine(id: i, time: time, text: text))
            time += 5.5 + Seed.unit(id + "\(i)") * 3
            if i % 6 == 5 { time += 6 }
        }
        return lines
    }
}
