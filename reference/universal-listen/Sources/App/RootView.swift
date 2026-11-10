import SwiftUI

/// One TabView, adaptable to a sidebar: iPhone shows five tabs; iPad and
/// the Mac show a sidebar of sections. Library and Podcasts are tab-bar
/// tabs whose contents the sidebar lists as its own items.
struct RootView: View {
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav
    @Environment(\.isCompact) private var isCompact
    #if os(macOS)
    @Environment(\.openSettings) private var openSettings
    #endif

    var body: some View {
        @Bindable var nav = nav
        TabView(selection: $nav.tab) {
            Tab("Home", systemImage: "house", value: TabID.home) {
                stack(.home) { HomeView() }
            }
            #if os(iOS)
            // Compact width only: the sidebar lists their contents itself.
            Tab("Library", systemImage: "square.stack.fill", value: TabID.library) {
                stack(.library) { LibraryView() }
            }
            .defaultVisibility(.hidden, for: .sidebar)
            Tab("Podcasts", systemImage: "antenna.radiowaves.left.and.right", value: TabID.podcasts) {
                stack(.podcasts) { ShowsView() }
            }
            .defaultVisibility(.hidden, for: .sidebar)
            #endif
            Tab("Household", systemImage: "person.2", value: TabID.household) {
                stack(.household) { HouseholdView() }
            }
            Tab(value: TabID.search, role: .search) {
                stack(.search) { SearchView() }
            }

            TabSection("Library") {
                Tab("Artists", systemImage: "music.mic", value: TabID.artists) { stack(.artists) { ArtistsView() } }
                Tab("Albums", systemImage: "square.stack", value: TabID.albums) { stack(.albums) { AlbumsView() } }
                Tab("Songs", systemImage: "music.note", value: TabID.songs) { stack(.songs) { SongsView() } }
                Tab("Composers", systemImage: "music.quarternote.3", value: TabID.composers) { stack(.composers) { ComposersView() } }
            }
            .defaultVisibility(.hidden, for: .tabBar)

            TabSection("Podcasts & Sermons") {
                Tab("Shows", systemImage: "antenna.radiowaves.left.and.right", value: TabID.shows) { stack(.shows) { ShowsView() } }
                Tab("Downloaded", systemImage: "arrow.down.circle", value: TabID.downloads) { stack(.downloads) { DownloadsView() } }
            }
            .defaultVisibility(.hidden, for: .tabBar)

            TabSection("Playlists") {
                ForEach(library.playlists) { p in
                    Tab(p.name, systemImage: p.isSmart ? p.symbol : "music.note.list", value: TabID.playlist(p.id)) {
                        stack(.playlist(p.id)) { PlaylistView(id: p.id) }
                    }
                }
            }
            .defaultVisibility(.hidden, for: .tabBar)
        }
        .tabViewStyle(.sidebarAdaptable)
        .modifier(MiniPlayerAccessory())
        .modifier(FullPlayerPresentation())
        .sheet(item: $nav.sheet) { sheet in
            switch sheet {
            case .newPlaylist: NewPlaylistSheet()
            case .settings: SettingsSheet()
            }
        }
        .onKeyPress(.space) {
            player.togglePlay()
            return .handled
        }
        .sensoryFeedback(.impact(weight: .light), trigger: player.actionCount)
        .task {
            nav.applyLaunch(compact: isCompact, library: library)
            #if os(macOS)
            if Launch.screen == "settings" { openSettings() }
            #endif
            try? await Task.sleep(for: .milliseconds(700))
            nav.applyLaunchTransients()
        }
    }

    private func stack<Content: View>(_ tab: TabID, @ViewBuilder _ content: () -> Content) -> some View {
        NavigationStack(path: nav.path(tab)) {
            content()
                .navigationDestination(for: Route.self) { RouteView(route: $0) }
                .playbackToolbar()
        }
        .modifier(PlayerInspector())
    }
}

struct RouteView: View {
    let route: Route

    var body: some View {
        Group {
            switch route {
            case .songs: SongsView()
            case .albums: AlbumsView()
            case .artists: ArtistsView()
            case .composers: ComposersView()
            case .playlists: PlaylistsView()
            case .downloads: DownloadsView()
            case .album(let id): AlbumView(id: id)
            case .artist(let id): ArtistView(id: id)
            case .composer(let name): ComposerView(name: name)
            case .playlist(let id): PlaylistView(id: id)
            case .show(let id): ShowView(id: id)
            case .episode(let id): EpisodeView(id: id)
            }
        }
        .playbackToolbar()
    }
}
