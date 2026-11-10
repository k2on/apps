import SwiftUI

/// iPhone's Library tab: the categories, then recently added albums.
struct LibraryView: View {
    @Environment(Library.self) private var library

    private struct Link: Hashable { let title, symbol: String; let route: Route }
    private let links: [Link] = [
        Link(title: "Playlists", symbol: "music.note.list", route: .playlists),
        Link(title: "Artists", symbol: "music.mic", route: .artists),
        Link(title: "Albums", symbol: "square.stack", route: .albums),
        Link(title: "Songs", symbol: "music.note", route: .songs),
        Link(title: "Composers", symbol: "music.quarternote.3", route: .composers),
    ]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(links, id: \.self) { link in
                    NavigationLink(value: link.route) {
                        HStack(spacing: 14) {
                            Image(systemName: link.symbol).font(.title3).foregroundStyle(.tint).frame(width: 30)
                            Text(link.title).font(.title3)
                            Spacer()
                            Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
                        }
                        .padding(.vertical, 12)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    Divider().padding(.leading, 44)
                }
                ShelfHeader(title: "Recently Added").padding(.top, 28).padding(.bottom, 12)
                AlbumGrid(albums: Array(library.recentlyAddedAlbums.prefix(8)))
            }
            .padding(.horizontal, 20)
        }
        .navigationTitle("Library")
    }
}

struct AlbumsView: View {
    @Environment(Library.self) private var library
    @State private var sort = "Recently Added"

    private var albums: [Album] {
        switch sort {
        case "Title": library.albums.sorted { $0.title < $1.title }
        case "Artist": library.albums.sorted { ($0.artistName, $0.year) < ($1.artistName, $1.year) }
        case "Year": library.albums.sorted { $0.year > $1.year }
        default: library.recentlyAddedAlbums
        }
    }

    var body: some View {
        ScrollView {
            AlbumGrid(albums: albums).padding(20)
        }
        .navigationTitle("Albums")
        .toolbar {
            Menu {
                Picker("Sort By", selection: $sort) {
                    ForEach(["Recently Added", "Title", "Artist", "Year"], id: \.self) { Text($0) }
                }
                .pickerStyle(.inline)
            } label: {
                Label("Sort", systemImage: "arrow.up.arrow.down")
            }
            .accessibilityIdentifier("menu.sort")
        }
    }
}

struct AlbumGrid: View {
    let albums: [Album]

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 150, maximum: 230), spacing: 18, alignment: .top)], spacing: 24) {
            ForEach(albums) { album in
                NavigationLink(value: Route.album(album.id)) {
                    AlbumTile(album: album)
                }
                .buttonStyle(.plain)
                .contextMenu { AlbumMenuItems(album: album) }
                .accessibilityIdentifier("album.\(album.id)")
            }
        }
    }
}

struct AlbumTile: View {
    let album: Album
    var width: CGFloat? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Artwork(art: album.art)
                .shadow(color: .black.opacity(0.12), radius: 6, y: 3)
            VStack(alignment: .leading, spacing: 1) {
                Text(album.title).font(.subheadline.weight(.medium)).lineLimit(1)
                Text(album.artistName).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .frame(width: width)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

/// An album: a header (artwork beside the title where it fits, above it
/// where it doesn't) and its tracks.
struct AlbumView: View {
    let id: String
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player

    var body: some View {
        if let album = library.album(id) {
            let tracks = library.tracks(of: album)
            let classical = album.genre == "Classical"
            List {
                AlbumHeader(album: album, tracks: tracks)
                    .listRowSeparator(.hidden)
                    .listRowInsets(EdgeInsets(top: 12, leading: 20, bottom: 20, trailing: 20))
                ForEach(tracks) { t in
                    TrackRow(track: t, showsComposer: classical && t.composer != tracks.first?.composer || classical && album.id == "american",
                             queue: tracks)
                }
                Text("\(Format.total(tracks)) · \(String(album.year))\n℗ \(String(album.year)) \(album.artistName). Served from the Household library.")
                    .font(.footnote).foregroundStyle(.secondary)
                    .listRowSeparator(.hidden)
                    .padding(.vertical, 12)
            }
            .listStyle(.plain)
            .navigationTitle(album.title)
            .inlineTitle()
        }
    }
}

private struct AlbumHeader: View {
    let album: Album
    let tracks: [Track]
    @Environment(Player.self) private var player

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .bottom, spacing: 28) {
                Artwork(art: album.art).frame(width: 240, height: 240)
                    .shadow(color: .black.opacity(0.18), radius: 14, y: 8)
                VStack(alignment: .leading, spacing: 6) {
                    titles(alignment: .leading)
                    buttons.padding(.top, 10)
                }
                .fixedSize()
                Spacer(minLength: 0)
            }
            VStack(spacing: 6) {
                Artwork(art: album.art).frame(width: 250, height: 250)
                    .shadow(color: .black.opacity(0.18), radius: 14, y: 8)
                    .padding(.bottom, 10)
                titles(alignment: .center)
                buttons.padding(.top, 10)
            }
            .frame(maxWidth: .infinity)
        }
    }

    private func titles(alignment: HorizontalAlignment) -> some View {
        VStack(alignment: alignment, spacing: 4) {
            Text(album.title).font(.title.bold()).multilineTextAlignment(alignment == .center ? .center : .leading)
            NavigationLink(value: Route.artist(album.artistID)) {
                Text(album.artistName).font(.title2).foregroundStyle(.tint)
            }
            .buttonStyle(.plain)
            Text("\(album.genre.uppercased()) · \(String(album.year))").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
        }
    }

    private var buttons: some View {
        HStack(spacing: 12) {
            Button { player.play(tracks: tracks) } label: {
                Label("Play", systemImage: "play.fill").frame(minWidth: 110)
            }
            .buttonStyle(.borderedProminent)
            Button { player.play(tracks: tracks, shuffled: true) } label: {
                Label("Shuffle", systemImage: "shuffle").frame(minWidth: 110)
            }
            .buttonStyle(.bordered)
            Menu {
                AlbumMenuItems(album: album)
            } label: {
                Label("More", systemImage: "ellipsis")
            }
            .labelStyle(.iconOnly)
            .menuIndicator(.hidden)
            .buttonStyle(.bordered)
            .buttonBorderShape(.circle)
            .accessibilityIdentifier("menu.album-more")
        }
        .controlSize(.large)
    }
}

/// A song in a list: number (or the playing indicator), title, time, More.
struct TrackRow: View {
    let track: Track
    var showsComposer = false
    var showsArtwork = false
    var queue: [Track] = []
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library

    var body: some View {
        let current = player.currentTrackID == track.id
        Button {
            player.play(tracks: queue.isEmpty ? [track] : queue, from: track)
        } label: {
            HStack(spacing: 12) {
                if showsArtwork, let album = library.album(track.albumID) {
                    Artwork(art: album.art).frame(width: 44, height: 44)
                } else {
                    Group {
                        if current {
                            NowPlayingIndicator(playing: player.isPlaying)
                        } else {
                            Text("\(track.number)").foregroundStyle(.secondary).monospacedDigit()
                        }
                    }
                    .frame(width: 24, alignment: .trailing)
                }
                VStack(alignment: .leading, spacing: 1) {
                    Text(track.title).lineLimit(2).foregroundStyle(current ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
                    if showsComposer || showsArtwork {
                        Text(showsArtwork ? "\(track.artistName) · \(track.albumTitle)" : track.composer)
                            .font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
                Spacer()
                if track.isFavorite {
                    Image(systemName: "star.fill").font(.caption).foregroundStyle(.tint).accessibilityLabel("Favorite")
                }
                Text(Format.clock(track.duration)).font(.subheadline.monospacedDigit()).foregroundStyle(.secondary)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu { TrackMenuItems(tracks: [track]) }
        .swipeActions(edge: .leading) {
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") { player.playNext([.track(track.id)]) }
                .tint(.indigo)
        }
        .swipeActions(edge: .trailing) {
            Button(track.isFavorite ? "Undo Favorite" : "Favorite", systemImage: "star") { library.toggleFavorite(track.id) }
                .tint(.orange)
        }
        .accessibilityIdentifier("song.\(track.id)")
    }
}
