import SwiftUI

/// One playlist's contents in playlist order — the `playlist` query, which
/// answers with the library's own rows. A swipe removes one, which is
/// `remove_from_playlist`. Opening it makes it the playlist the library
/// adds to.
struct PlaylistView: View {
    @EnvironmentObject private var model: Model
    let playlist: Playlist

    /// The rows for this playlist, whether or not it is the model's
    /// selection yet (it becomes so on appear).
    private var rows: [Track] {
        return model.selectedPlaylist == playlist.id ? model.items : []
    }

    var body: some View {
        Group {
            if rows.isEmpty {
                ContentUnavailableView("Nothing on it", systemImage: "music.note.list",
                                       description: Text("Add tracks from the library."))
            } else {
                List {
                    ForEach(rows) { track in
                        HStack {
                            Text(track.playlistPos.map { "\($0)" } ?? "")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                                .frame(width: 28, alignment: .trailing)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(track.title)
                                Text(track.creator)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                if let why = model.caption(for: track, on: playlist.id) {
                                    Text(why)
                                        .font(.caption2)
                                        .foregroundStyle(why.hasPrefix("not saved") ? .red : .secondary)
                                }
                            }
                            Spacer()
                            Text(track.duration)
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    }
                    .onDelete { offsets in
                        let rs = rows // the removes land on a later snapshot, so take the rows once
                        for i in offsets { model.remove(rs[i], from: playlist.id) }
                    }
                }
            }
        }
        .navigationTitle(playlist.name)
        .onAppear { model.select(playlist.id) }
    }
}
