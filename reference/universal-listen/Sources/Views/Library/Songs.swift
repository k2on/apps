import SwiftUI

struct SongsView: View {
    @Environment(Library.self) private var library

    var body: some View {
        SongTable(tracks: library.tracks)
            .navigationTitle("Songs")
    }
}

enum SongSort: String, CaseIterable, Identifiable {
    case title = "Title", artist = "Artist", album = "Album", composer = "Composer", time = "Time", plays = "Plays", added = "Date Added"
    var id: String { rawValue }

    func comparator(_ order: SortOrder) -> KeyPathComparator<Track> {
        switch self {
        case .title: KeyPathComparator(\Track.title, order: order)
        case .artist: KeyPathComparator(\Track.artistName, order: order)
        case .album: KeyPathComparator(\Track.albumTitle, order: order)
        case .composer: KeyPathComparator(\Track.composer, order: order)
        case .time: KeyPathComparator(\Track.duration, order: order)
        case .plays: KeyPathComparator(\Track.plays, order: order)
        case .added: KeyPathComparator(\Track.added, order: order)
        }
    }

    init?(_ comparator: KeyPathComparator<Track>?) {
        guard let comparator else { return nil }
        guard let match = SongSort.allCases.first(where: { $0.comparator(.forward).keyPath == comparator.keyPath }) else { return nil }
        self = match
    }
}

/// Songs as a sortable table: every column on iPad and the Mac; on iPhone
/// the table shows its first column, which then carries artist and album.
struct SongTable: View {
    let tracks: [Track]
    @State private var sortOrder: [KeyPathComparator<Track>] = []
    @State private var selection: Set<Track.ID> = []
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library
    @Environment(\.isCompact) private var isCompact

    private var rows: [Track] { sortOrder.isEmpty ? tracks : tracks.sorted(using: sortOrder) }

    var body: some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("Title", value: \.title) { t in
                TitleCell(track: t, isCompact: isCompact, isCurrent: player.currentTrackID == t.id, isPlaying: player.isPlaying)
            }
            .width(min: 200, ideal: 280)
            TableColumn("Artist", value: \.artistName) { Text($0.artistName).lineLimit(1) }
                .width(min: 110, ideal: 170)
            TableColumn("Album", value: \.albumTitle) { Text($0.albumTitle).lineLimit(1) }
                .width(min: 110, ideal: 190)
            TableColumn("Composer", value: \.composer) { Text($0.composer).lineLimit(1).foregroundStyle(.secondary) }
                .width(min: 100, ideal: 160)
            TableColumn("Time", value: \.duration) { t in
                Text(Format.clock(t.duration)).monospacedDigit().foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .width(min: 48, ideal: 56, max: 72)
            TableColumn("Plays", value: \.plays) { t in
                Text("\(t.plays)").monospacedDigit().foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .width(min: 40, ideal: 48, max: 64)
        }
        .contextMenu(forSelectionType: Track.ID.self) { ids in
            TrackMenuItems(tracks: ids.compactMap(library.track))
        } primaryAction: { ids in
            let start = ids.first.flatMap(library.track)
            player.play(tracks: rows, from: start)
        }
        .toolbar {
            ToolbarItemGroup {
                SortMenu(sortOrder: $sortOrder)
                Button("Shuffle All", systemImage: "shuffle") { player.play(tracks: rows, shuffled: true) }
            }
        }
    }
}

struct SortMenu: View {
    @Binding var sortOrder: [KeyPathComparator<Track>]

    var body: some View {
        let current = SongSort(sortOrder.first) ?? .title
        let order = sortOrder.first?.order ?? .forward
        Menu {
            Picker("Sort By", selection: Binding(get: { current }, set: { sortOrder = [$0.comparator(order)] })) {
                ForEach(SongSort.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.inline)
            Picker("Order", selection: Binding(get: { order }, set: { sortOrder = [current.comparator($0)] })) {
                Text("Ascending").tag(SortOrder.forward)
                Text("Descending").tag(SortOrder.reverse)
            }
            .pickerStyle(.inline)
        } label: {
            Label("Sort", systemImage: "arrow.up.arrow.down")
        }
        .accessibilityIdentifier("menu.sort")
    }
}

private struct TitleCell: View {
    let track: Track
    let isCompact: Bool
    let isCurrent: Bool
    let isPlaying: Bool
    @Environment(Library.self) private var library

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                if let album = library.album(track.albumID) {
                    Artwork(art: album.art)
                }
                if isCurrent {
                    RoundedRectangle(cornerRadius: 4).fill(.black.opacity(0.35))
                    NowPlayingIndicator(playing: isPlaying).foregroundStyle(.white).font(.caption)
                }
            }
            .frame(width: isCompact ? 44 : 26, height: isCompact ? 44 : 26)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 4) {
                    Text(track.title).lineLimit(1)
                        .foregroundStyle(isCurrent ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
                    if track.isFavorite {
                        Image(systemName: "star.fill").font(.caption2).foregroundStyle(.tint)
                            .accessibilityLabel("Favorite")
                    }
                }
                if isCompact {
                    Text("\(track.artistName) · \(track.albumTitle)")
                        .font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            if isCompact {
                Spacer()
                Text(Format.clock(track.duration)).font(.subheadline.monospacedDigit()).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("song.\(track.id)")
    }
}
