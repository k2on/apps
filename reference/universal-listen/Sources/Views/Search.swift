import SwiftUI

/// Search across the library and shows, with scopes and suggestions.
struct SearchView: View {
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav
    @Environment(Player.self) private var player

    private struct Category: Hashable { let title, symbol: String; let hue: Double }
    private let categories: [Category] = [
        Category(title: "Classical", symbol: "music.quarternote.3", hue: 0.10), Category(title: "Jazz", symbol: "music.mic", hue: 0.58),
        Category(title: "Folk", symbol: "guitars", hue: 0.07), Category(title: "Ambient", symbol: "leaf", hue: 0.38),
        Category(title: "Sermons", symbol: "book.closed", hue: 0.12), Category(title: "Science", symbol: "atom", hue: 0.62),
        Category(title: "Kids", symbol: "balloon.2", hue: 0.16), Category(title: "Electronic", symbol: "waveform", hue: 0.72),
    ]

    var body: some View {
        @Bindable var nav = nav
        let q = nav.query.trimmingCharacters(in: .whitespaces)
        Group {
            if q.isEmpty {
                ScrollView {
                    VStack(alignment: .leading, spacing: 12) {
                        ShelfHeader(title: "Browse")
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 160), spacing: 14)], spacing: 14) {
                            ForEach(categories, id: \.self) { c in
                                Button { nav.query = c.title } label: { CategoryTile(title: c.title, symbol: c.symbol, hue: c.hue) }
                                    .buttonStyle(PressableStyle(scale: 0.96))
                            }
                        }
                    }
                    .padding(20)
                }
            } else {
                SearchResults(query: q, scope: nav.scope)
            }
        }
        .navigationTitle("Search")
        .searchable(text: $nav.query, isPresented: $nav.searchPresented, prompt: "Songs, albums, shows and more")
        .searchScopes($nav.scope, activation: .onSearchPresentation) {
            ForEach(SearchScope.allCases) { Text($0.rawValue).tag($0) }
        }
        .searchSuggestions {
            if q.count < 4 {
                ForEach(suggestions(for: q), id: \.self) { s in
                    Label(s, systemImage: "magnifyingglass").searchCompletion(s)
                }
            }
        }
    }

    private func suggestions(for q: String) -> [String] {
        let pool = library.artists.map(\.name) + library.albums.map(\.title) + library.shows.map(\.title) + library.composers.map(\.name)
        guard !q.isEmpty else { return ["Debussy", "Harbor Lights", "Slow Science", "Luke 15"] }
        return Array(pool.filter { $0.localizedStandardContains(q) }.prefix(6))
    }
}

private struct CategoryTile: View {
    let title: String
    let symbol: String
    let hue: Double

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            LinearGradient(colors: [Color(hue: hue, saturation: 0.5, brightness: 0.85), Color(hue: hue, saturation: 0.75, brightness: 0.5)],
                           startPoint: .topLeading, endPoint: .bottomTrailing)
            Image(systemName: symbol).font(.system(size: 44, weight: .light)).foregroundStyle(.white.opacity(0.35))
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing).padding(12)
            Text(title).font(.headline).foregroundStyle(.white).padding(12)
        }
        .frame(height: 96)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}

private struct SearchResults: View {
    let query: String
    let scope: SearchScope
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player

    var body: some View {
        let music = scope != .shows
        let spoken = scope != .music
        let tracks = music ? library.tracks.filter {
            $0.title.localizedStandardContains(query) || $0.artistName.localizedStandardContains(query)
                || $0.albumTitle.localizedStandardContains(query) || $0.composer.localizedStandardContains(query)
        } : []
        let albums = music ? library.albums.filter { $0.title.localizedStandardContains(query) || $0.artistName.localizedStandardContains(query) || $0.genre.localizedStandardContains(query) } : []
        let artists = music ? library.artists.filter { $0.name.localizedStandardContains(query) || $0.genre.localizedStandardContains(query) } : []
        let episodes = spoken ? library.episodes.filter {
            $0.title.localizedStandardContains(query) || $0.summary.localizedStandardContains(query)
                || ($0.scripture ?? "").localizedStandardContains(query)
                || (library.show($0.showID)?.title ?? "").localizedStandardContains(query)
                || (library.show($0.showID)?.author ?? "").localizedStandardContains(query)
        } : []
        if tracks.isEmpty && albums.isEmpty && artists.isEmpty && episodes.isEmpty {
            ContentUnavailableView.search(text: query)
        } else {
            List {
                if !albums.isEmpty {
                    Section("Albums") {
                        ScrollView(.horizontal) {
                            LazyHStack(alignment: .top, spacing: 16) {
                                ForEach(albums) { a in
                                    NavigationLink(value: Route.album(a.id)) { AlbumTile(album: a, width: 140) }
                                        .buttonStyle(.plain)
                                        .listenContextMenu { AlbumMenuItems(album: a) }
                                }
                            }
                            .padding(.vertical, 4)
                        }
                        .scrollIndicators(.hidden)
                        .listRowSeparator(.hidden)
                    }
                }
                if !artists.isEmpty {
                    Section("Artists") {
                        ForEach(artists) { a in
                            NavigationLink(value: Route.artist(a.id)) {
                                HStack(spacing: 12) {
                                    Artwork(art: a.art, circle: true).frame(width: 40, height: 40)
                                    Text(a.name)
                                }
                            }
                        }
                    }
                }
                if !tracks.isEmpty {
                    Section("Songs") {
                        ForEach(tracks.prefix(12)) { t in
                            TrackRow(track: t, showsArtwork: true, queue: Array(tracks.prefix(12)))
                        }
                    }
                }
                if !episodes.isEmpty {
                    Section("Episodes") {
                        ForEach(episodes.prefix(10)) { EpisodeRow(episode: $0, showsArtwork: true) }
                    }
                }
            }
            .listStyle(.plain)
        }
    }
}
