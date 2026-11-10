import SwiftUI

struct ArtistsView: View {
    @Environment(Library.self) private var library

    var body: some View {
        List(library.artists.sorted { $0.name < $1.name }) { artist in
            NavigationLink(value: Route.artist(artist.id)) {
                HStack(spacing: 12) {
                    Artwork(art: artist.art, circle: true).frame(width: 44, height: 44)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(artist.name)
                        Text("\(artist.genre) · \(library.albums(by: artist.id).count) albums")
                            .font(.subheadline).foregroundStyle(.secondary)
                    }
                }
            }
        }
        .listStyle(.plain)
        .navigationTitle("Artists")
    }
}

struct ArtistView: View {
    let id: String
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player

    var body: some View {
        if let artist = library.artist(id) {
            let albums = library.albums(by: id)
            let top = library.tracks(by: id).sorted { $0.plays > $1.plays }.prefix(5)
            ScrollView {
                VStack(alignment: .leading, spacing: 28) {
                    HStack(spacing: 20) {
                        Artwork(art: artist.art, circle: true).frame(width: 120, height: 120)
                        VStack(alignment: .leading, spacing: 8) {
                            Text(artist.name).font(.largeTitle.bold())
                            Text(artist.genre).foregroundStyle(.secondary)
                            Button { player.play(tracks: library.tracks(by: id), shuffled: true) } label: {
                                Label("Shuffle", systemImage: "shuffle")
                            }
                            .buttonStyle(.borderedProminent)
                        }
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        ShelfHeader(title: "Top Songs")
                        ForEach(Array(top)) { t in
                            TrackRow(track: t, showsArtwork: true, queue: Array(top))
                            Divider()
                        }
                    }
                    VStack(alignment: .leading, spacing: 12) {
                        ShelfHeader(title: "Albums")
                        AlbumGrid(albums: albums)
                    }
                }
                .padding(20)
            }
            .navigationTitle(artist.name)
            .inlineTitle()
        }
    }
}

struct ComposersView: View {
    @Environment(Library.self) private var library

    var body: some View {
        List(library.composers) { c in
            NavigationLink(value: Route.composer(c.name)) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(c.name)
                    Text([c.years, "\(library.tracks(composedBy: c.name).count) works"].compactMap { $0 }.joined(separator: " · "))
                        .font(.subheadline).foregroundStyle(.secondary)
                }
            }
        }
        .listStyle(.plain)
        .navigationTitle("Composers")
    }
}

/// A composer's works, grouped by recording.
struct ComposerView: View {
    let name: String
    @Environment(Library.self) private var library

    var body: some View {
        let tracks = library.tracks(composedBy: name)
        let albumIDs = tracks.map(\.albumID).reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } }
        List {
            if let years = SampleData.composerYears[name] {
                Text(years).foregroundStyle(.secondary).listRowSeparator(.hidden)
            }
            ForEach(albumIDs, id: \.self) { albumID in
                if let album = library.album(albumID) {
                    Section {
                        ForEach(tracks.filter { $0.albumID == albumID }) { t in
                            TrackRow(track: t, queue: tracks.filter { $0.albumID == albumID })
                        }
                    } header: {
                        HStack(spacing: 10) {
                            Artwork(art: album.art).frame(width: 36, height: 36)
                            VStack(alignment: .leading, spacing: 0) {
                                Text(album.title).font(.headline).foregroundStyle(.primary)
                                Text(album.artistName).font(.subheadline).foregroundStyle(.secondary)
                            }
                        }
                        .textCase(nil)
                        .padding(.vertical, 4)
                    }
                }
            }
        }
        .listStyle(.plain)
        .navigationTitle(name)
    }
}

/// iPhone's list of playlists (the sidebar lists them itself).
struct PlaylistsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav

    var body: some View {
        List {
            Section {
                Button("New Playlist…", systemImage: "plus") { nav.sheet = .newPlaylist }
            }
            ForEach(library.playlists) { p in
                NavigationLink(value: Route.playlist(p.id)) {
                    HStack(spacing: 12) {
                        Artwork(art: p.art).frame(width: 56, height: 56)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(p.name)
                            Text(p.isSmart ? "Smart Playlist" : "\(library.tracks(in: p).count) songs")
                                .font(.subheadline).foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
        .listStyle(.plain)
        .navigationTitle("Playlists")
    }
}

struct PlaylistView: View {
    let id: String
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player

    var body: some View {
        if let p = library.playlist(id) {
            let tracks = library.tracks(in: p)
            VStack(spacing: 0) {
                HStack(alignment: .center, spacing: 18) {
                    Artwork(art: p.art).frame(width: 112, height: 112)
                        .shadow(color: .black.opacity(0.15), radius: 8, y: 4)
                    VStack(alignment: .leading, spacing: 4) {
                        if p.isSmart {
                            Label("Smart Playlist", systemImage: "gearshape").font(.caption.weight(.semibold))
                                .foregroundStyle(.secondary)
                        }
                        Text(p.name).font(.title.bold())
                        Text(p.summary).foregroundStyle(.secondary).lineLimit(2)
                        Text(Format.total(tracks)).font(.footnote).foregroundStyle(.secondary)
                        HStack {
                            Button { player.play(tracks: tracks) } label: { Label("Play", systemImage: "play.fill") }
                                .buttonStyle(.borderedProminent)
                            Button { player.play(tracks: tracks, shuffled: true) } label: { Label("Shuffle", systemImage: "shuffle") }
                                .buttonStyle(.bordered)
                        }
                        .padding(.top, 4)
                    }
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 16)
                SongTable(tracks: tracks)
            }
            .navigationTitle(p.name)
            .inlineTitle()
        }
    }
}

struct NewPlaylistSheet: View {
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var summary = ""
    @State private var shared = true
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Playlist Name", text: $name).focused($focused)
                    TextField("Description", text: $summary, axis: .vertical).lineLimit(2...4)
                }
                Section {
                    Toggle("Share with Household", isOn: $shared)
                } footer: {
                    Text("Everyone in the household can listen to and add to shared playlists.")
                }
                if !nav.pendingTracks.isEmpty {
                    Section("Adding") {
                        ForEach(nav.pendingTracks, id: \.self) { id in
                            if let t = library.track(id) { Text(t.title) }
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle("New Playlist")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") {
                        let p = library.newPlaylist(named: name, summary: summary)
                        library.add(nav.pendingTracks, to: p.id)
                        nav.pendingTracks = []
                        dismiss()
                    }
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
        .frame(minWidth: 420, minHeight: 320)
        .onAppear { if Launch.sheet == "new-playlist" { name = "Saturday Chores" } else { focused = true } }
    }
}
