import SwiftUI
import ArkDB

/// One playlist's items, by position — the `playlist_items` query joined to
/// the library on the phone. A swipe removes one, which is
/// `remove_from_playlist`. Opening it makes it the playlist the library adds to.
struct PlaylistView: View {
    @EnvironmentObject private var model: Model
    let playlist: PlaylistSummary

    /// The rows for this playlist, whether or not it is the model's
    /// selection yet (it becomes so on appear).
    private var rows: [PlaylistRow] {
        return model.selectedPlaylist == playlist.id ? model.items : []
    }

    var body: some View {
        Group {
            if rows.isEmpty {
                ContentUnavailableView("Nothing on it", systemImage: "music.note.list",
                                       description: Text("Swipe a track in the library to add it here."))
            } else {
                List {
                    ForEach(rows) { row in
                        HStack {
                            Text("\(row.item.pos)")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                                .frame(width: 28, alignment: .trailing)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(row.title)
                                    .foregroundStyle(row.track == nil ? .secondary : .primary)
                                if let t = row.track {
                                    Text([t.artist, t.album].compactMap { $0 }.joined(separator: " · "))
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                            Spacer()
                            if let t = row.track {
                                Text(t.duration)
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                    .onDelete { offsets in
                        let rs = rows // each remove re-reads, so take the rows once
                        for i in offsets { model.remove(rs[i]) }
                    }
                }
            }
        }
        .navigationTitle(playlist.name)
        .onAppear { model.selectedPlaylist = playlist.id }
    }
}
