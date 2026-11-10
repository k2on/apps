import SwiftUI

/// Top-level destinations. Some exist only in the sidebar (iPad, Mac), some
/// only in the tab bar (iPhone): see RootView.
enum TabID: Hashable {
    case home, household, search
    case library, podcasts            // tab bar only
    case songs, albums, artists, composers, shows, downloads, playlist(String)  // sidebar only
}

enum Route: Hashable {
    case songs, albums, artists, composers, playlists, downloads
    case album(String), artist(String), composer(String), playlist(String), show(String), episode(String)
}

enum SearchScope: String, CaseIterable, Identifiable {
    case all = "All", music = "Music", shows = "Podcasts & Sermons"
    var id: String { rawValue }
}

enum SettingsPane: String, CaseIterable, Identifiable {
    case server, household, playback, downloads, appearance
    var id: String { rawValue }
    var title: String {
        switch self {
        case .server: "Server"
        case .household: "Household"
        case .playback: "Playback"
        case .downloads: "Downloads"
        case .appearance: "Appearance"
        }
    }
    var symbol: String {
        switch self {
        case .server: "server.rack"
        case .household: "person.2"
        case .playback: "play.circle"
        case .downloads: "arrow.down.circle"
        case .appearance: "paintpalette"
        }
    }
}

enum SheetID: String, Identifiable {
    case newPlaylist, settings
    var id: String { rawValue }
}

enum DialogID: Hashable {
    case handOff(String)            // a device id
    case removeDownload(String)     // an episode id
}

/// Where the person is: the tab, each tab's stack, search, and the transient
/// UI (player, inspector, sheets, dialogs) that the launch arguments can open.
@Observable @MainActor
final class Navigator {
    var tab: TabID = .home
    var paths: [TabID: [Route]] = [:]
    var query = ""
    var scope: SearchScope = .all
    var searchPresented = false
    var playerExpanded = false
    var playerPanel: PlayerPanel = .none
    var inspectorShown = false
    var inspectorPane: InspectorPane = .upNext
    var upNextPopover = false
    var outputPopover = false
    var sheet: SheetID?
    var dialog: DialogID?
    var settingsPane: SettingsPane = .server
    var settingsPath: [SettingsPane] = []
    /// The track the new-playlist sheet was opened for, if any.
    var pendingTracks: [String] = []

    func path(_ tab: TabID) -> Binding<[Route]> {
        Binding(get: { self.paths[tab] ?? [] }, set: { self.paths[tab] = $0 })
    }

    /// Goes to a route from anywhere: pushed on the current tab's stack.
    func go(_ route: Route) {
        var p = paths[tab] ?? []
        p.append(route)
        paths[tab] = p
    }

    /// Opens a screen named by `-screen` (SCREENS.md). Compact width has a
    /// Library and a Podcasts tab where the sidebar has their sections.
    func open(_ screen: String, compact: Bool, library: Library) {
        let parts = screen.split(separator: ":", maxSplits: 1).map(String.init)
        let key = parts[0]
        let arg = parts.count > 1 ? parts[1] : ""
        func show(_ wide: TabID, compactTab: TabID, _ routes: [Route]) {
            tab = compact ? compactTab : wide
            paths[tab] = routes
        }
        switch key {
        case "songs": show(.songs, compactTab: .library, compact ? [.songs] : [])
        case "albums": show(.albums, compactTab: .library, compact ? [.albums] : [])
        case "artists": show(.artists, compactTab: .library, compact ? [.artists] : [])
        case "composers": show(.composers, compactTab: .library, compact ? [.composers] : [])
        case "playlists": show(.playlist("favorites"), compactTab: .library, compact ? [.playlists] : [])
        case "album": show(.albums, compactTab: .library, compact ? [.albums, .album(arg)] : [.album(arg)])
        case "artist": show(.artists, compactTab: .library, compact ? [.artists, .artist(arg)] : [.artist(arg)])
        case "composer": show(.composers, compactTab: .library, compact ? [.composers, .composer(arg)] : [.composer(arg)])
        case "playlist":
            if compact { show(.library, compactTab: .library, [.playlists, .playlist(arg)]) } else { tab = .playlist(arg) }
        case "library": tab = compact ? .library : .songs
        case "shows", "podcasts": show(.shows, compactTab: .podcasts, [])
        case "show": show(.shows, compactTab: .podcasts, [.show(arg)])
        case "episode":
            let showID = library.episode(arg)?.showID ?? ""
            show(.shows, compactTab: .podcasts, [.show(showID), .episode(arg)])
        case "downloads": show(.downloads, compactTab: .podcasts, compact ? [.downloads] : [])
        case "household": tab = .household
        case "search":
            tab = .search
            query = arg
        case "settings":
            #if os(iOS)
            sheet = .settings
            #endif
            break
        default: tab = .home
        }
    }

    func applyLaunch(compact: Bool, library: Library) {
        if let screen = Launch.screen { open(screen, compact: compact, library: library) }
        if let pane = Launch.settingsTab.flatMap(SettingsPane.init(rawValue:)) {
            settingsPane = pane
            settingsPath = [pane]
        }
        if let pane = Launch.inspector {
            inspectorShown = true
            inspectorPane = pane == "upnext" || pane == "queue" ? .upNext : .lyrics
        }
        playerExpanded = Launch.playerExpanded
        if let panel = Launch.panel.flatMap(PlayerPanel.init(rawValue:)) { playerPanel = panel }
        searchPresented = Launch.searchActive
        if Launch.sheet == "new-playlist" { sheet = .newPlaylist }
    }

    /// Transient UI that needs its anchor on screen first.
    func applyLaunchTransients() {
        switch Launch.popover {
        case "output": outputPopover = true
        case "upnext", "queue": upNextPopover = true
        default: break
        }
        if let d = Launch.dialog {
            let parts = d.split(separator: ":", maxSplits: 1).map(String.init)
            if parts.count == 2 {
                dialog = parts[0] == "handoff" ? .handOff(parts[1]) : .removeDownload(parts[1])
            }
        }
    }
}

enum PlayerPanel: String { case none, lyrics, upNext = "upnext" }
enum InspectorPane: String, CaseIterable, Identifiable {
    case upNext, lyrics
    var id: String { rawValue }
}
