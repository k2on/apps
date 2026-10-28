// Search: the system's search field, at the tab bar's end in Liquid Glass
// (`Tab(role: .search)` with `.searchable`), over the library, answered as
// you type (Android's `SearchScreen.kt`). Songs are the domain's own
// `search`; albums and artists are the held lists whose names hold the
// needle, so the three groups answer one question. With nothing typed it
// offers the last few searches, which live on this phone and nowhere else.
// What it last found is the model's, so leaving the tab keeps it.
import ArkUI
import SwiftUI

/// How many songs a search lists.
let searchSongs = 50

struct SearchView: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    var body: some View {
        let needle = Binding(get: { model.needle }, set: { model.searchFor($0) })
        List {
            if let host = model.unreachable { Offline(host: host).listRowInsets(EdgeInsets()).listRowSeparator(.hidden) }
            if model.needle.trimmingCharacters(in: .whitespaces).isEmpty {
                recent
            } else if let r = model.results {
                found(r)
            }
        }
        .listStyle(.plain)
        .navigationTitle("Search")
        .searchable(text: needle, prompt: "Songs, albums, artists")
        .onSubmit(of: .search) { model.searched(model.needle) }
        // The covers of an answer that just arrived decoded before its rows ask.
        .onChange(of: model.results) { _, r in
            guard let r = r else { return }
            Covers.shared.warm(r.songs.prefix(searchSongs).map { RowCover.url(model, model.album(of: $0)?.art ?? "") }, side: Space.thumb)
            Covers.shared.warm(r.albums.map { RowCover.url(model, $0.art) }, side: 128)
        }
    }

    @ViewBuilder private var recent: some View {
        if model.recent.isEmpty {
            ContentUnavailableView("Search your library", systemImage: "magnifyingglass",
                                   description: Text("Find a song by its title, or everything by somebody, as you type."))
                .listRowSeparator(.hidden)
        } else {
            Section {
                ForEach(model.recent, id: \.self) { r in
                    Button { model.searchFor(r) } label: { Label(r, systemImage: "clock.arrow.circlepath") }
                        .foregroundStyle(.primary)
                }
            } header: {
                HStack {
                    Text("Recent Searches")
                    Spacer()
                    Button("Clear") { model.forgetSearches() }.foregroundStyle(Color.gold)
                }
            }
        }
    }

    @ViewBuilder private func found(_ r: Results) -> some View {
        if r.isEmpty {
            ContentUnavailableView.search(text: r.needle)
        }
        if !r.songs.isEmpty {
            Section("Songs") {
                let songs = Array(r.songs.prefix(searchSongs))
                ForEach(songs) { t in
                    TrackRow(track: t, under: [t.creator, model.album(of: t)?.name].compactMap { $0 }.joined(separator: " · "), from: nil, onPlay: {
                        model.searched(model.needle)
                        model.play(r.songs, at: r.songs.firstIndex(of: t) ?? 0, from: nil)
                    }) { RowCover(track: t) }
                }
            }
        }
        if !r.albums.isEmpty {
            Section("Albums") {
                CoverShelf(items: r.albums) { a in
                    CoverCard(seed: a.name, url: RowCover.url(model, a.art), title: a.name, under: a.playedBy, side: 128, opens: .page(.album(a.id))) {
                        model.searched(model.needle)
                        open(.page(.album(a.id)))
                    }
                }
                .listRowInsets(EdgeInsets())
            }
        }
        if !r.artists.isEmpty {
            Section("Artists") {
                CoverShelf(items: r.artists) { a in
                    let to = Route.page(.artist(a.name))
                    CoverCard(seed: a.name, url: RowCover.url(model, a.art), title: a.name, under: Spell.count(Int(a.tracks), "song"), side: 112, round: true, opens: to) {
                        model.searched(model.needle)
                        open(to)
                    }
                }
                .listRowInsets(EdgeInsets())
            }
        }
    }
}
