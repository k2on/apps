// Library: everything, four ways — the person's playlists, the albums, the
// artists, and every song in the order it arrived — each opening the page it
// stands for, zooming out of its cover (Android's `LibraryScreen.kt`). The
// four are a segmented control in the glass of the navigation bar's bottom;
// the playlists list is where a new one is made, and the domain's own checks
// decide whether a name will do.
import SwiftUI

enum LibraryTab: String, CaseIterable, Identifiable {
    case playlists = "Playlists", albums = "Albums", artists = "Artists", songs = "Songs"
    var id: String { rawValue }
}

struct LibraryView: View {
    @Binding var tab: LibraryTab
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present

    var body: some View {
        let s = model.snapshot
        Group {
            if !s.loaded {
                Color.clear
            } else {
                switch tab {
                case .playlists: PlaylistsList()
                case .albums: AlbumsGrid()
                // Before the artists arrive, nothing — not "no artists": they are milliseconds away.
                case .artists: if s.complete { ArtistsList() } else { Color.clear }
                case .songs: SongsList()
                }
            }
        }
        .navigationTitle("Library")
        .safeAreaInset(edge: .top, spacing: 0) {
            VStack(spacing: 0) {
                SwiftUI.Picker("Show", selection: $tab) {
                    ForEach(LibraryTab.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .padding(.horizontal, Space.page)
                .padding(.vertical, Space.s)
                if let host = model.unreachable { Offline(host: host) }
            }
        }
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button { present(.newPlaylist(then: nil)) } label: { Image(systemName: "plus") }
                    .accessibilityLabel("New playlist")
            }
        }
    }
}

private struct PlaylistsList: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @Environment(\.present) private var present
    @Environment(\.zoom) private var zoom

    var body: some View {
        List {
            Button { present(.newPlaylist(then: nil)) } label: {
                HStack(spacing: Space.l) {
                    Image(systemName: "plus").font(.title2).foregroundStyle(Color.gold).frame(width: 56, height: 56)
                        .glassEffect(.regular, in: RoundedRectangle(cornerRadius: Space.corner, style: .continuous))
                    Text("New Playlist").font(.headline)
                }
            }
            .foregroundStyle(.primary)
            if model.playlists.isEmpty {
                ContentUnavailableView("No playlists yet", systemImage: "music.note.list", description: Text("Make one, then put songs on it from any song's menu."))
                    .listRowSeparator(.hidden)
            }
            ForEach(model.playlists) { p in
                let r = Route.page(.playlist(p.id))
                Button { open(r) } label: {
                    HStack(spacing: Space.l) {
                        CoverView(seed: p.name, url: "", side: 56).frame(width: 56, height: 56).zoomSource(r, zoom)
                        VStack(alignment: .leading) {
                            Text(p.name).font(.headline).lineLimit(1)
                            Text("Playlist").font(.subheadline).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Image(systemName: "chevron.right").font(.footnote).foregroundStyle(.tertiary)
                    }
                }
                .foregroundStyle(.primary)
            }
        }
        .listStyle(.plain)
    }
}

private struct AlbumsGrid: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @Environment(\.zoom) private var zoom

    var body: some View {
        if model.albums.isEmpty {
            ContentUnavailableView("No albums yet", systemImage: "square.stack", description: Text("An album appears here when the server finds a song that names one."))
        } else {
            ScrollView {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: Space.l)], spacing: Space.xl) {
                    ForEach(model.albums) { a in
                        let r = Route.page(.album(a.name))
                        Button { open(r) } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                CoverView(seed: a.name, url: RowCover.url(model, a.art), side: 180)
                                    .aspectRatio(1, contentMode: .fit)
                                    .zoomSource(r, zoom)
                                    .padding(.bottom, Space.s - 2)
                                Text(a.name).font(.subheadline.weight(.semibold)).lineLimit(1)
                                Text(a.creator).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(Space.page)
            }
        }
    }
}

private struct ArtistsList: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @Environment(\.zoom) private var zoom

    var body: some View {
        if model.artists.isEmpty {
            ContentUnavailableView("No artists yet", systemImage: "music.microphone", description: Text("Whoever made what the server finds appears here."))
        } else {
            List(model.artists) { a in
                let r = Route.page(.artist(a.name))
                Button { open(r) } label: {
                    HStack(spacing: Space.l) {
                        CoverView(seed: a.name, url: RowCover.url(model, a.art), side: 56, round: true).frame(width: 56, height: 56).zoomSource(r, zoom)
                        VStack(alignment: .leading) {
                            Text(a.name).font(.headline).lineLimit(1)
                            Text(Spell.count(Int(a.tracks), "song")).font(.subheadline).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Image(systemName: "chevron.right").font(.footnote).foregroundStyle(.tertiary)
                    }
                }
                .foregroundStyle(.primary)
            }
            .listStyle(.plain)
        }
    }
}

/// Every song, in the order they arrived.
struct SongsList: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        let tracks = model.tracks
        if tracks.isEmpty {
            ContentUnavailableView("No songs yet", systemImage: "music.note",
                                   description: Text("Harken plays what your server finds. Put music in its media folder, under music/, and it appears here."))
        } else {
            List(Array(tracks.enumerated()), id: \.element.id) { i, t in
                TrackRow(track: t, under: [t.creator, model.album(of: t)?.name].compactMap { $0 }.joined(separator: " · "), from: .songs,
                         onPlay: { model.play(tracks, at: i, from: .songs) }) { RowCover(track: t) }
            }
            .listStyle(.plain)
        }
    }
}
