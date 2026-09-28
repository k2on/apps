import SwiftUI
import ArkDB

/// Every track, by artist, album and title — the `library` query. A swipe
/// or the button puts a track on the selected playlist.
struct LibraryView: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        NavigationStack {
            Group {
                if model.tracks.isEmpty {
                    ContentUnavailableView("No tracks yet", systemImage: "music.note",
                                           description: Text("Tracks arrive from the server's scanner. Alone, the library stays empty."))
                } else {
                    List(model.tracks) { track in
                        TrackRow(track: track)
                            .swipeActions(edge: .leading) {
                                if let pid = model.selectedPlaylist {
                                    Button { model.add(track, to: pid) } label: {
                                        Label("Add", systemImage: "plus")
                                    }
                                    .tint(.green)
                                }
                            }
                    }
                }
            }
            .navigationTitle("Library")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        ForEach(model.playlists) { p in
                            Button {
                                model.selectedPlaylist = p.id
                            } label: {
                                if p.id == model.selectedPlaylist {
                                    Label(p.name, systemImage: "checkmark")
                                } else {
                                    Text(p.name)
                                }
                            }
                        }
                    } label: {
                        Label(model.selected.map { "Adding to \($0.name)" } ?? "No playlist", systemImage: "text.badge.plus")
                    }
                    .disabled(model.playlists.isEmpty)
                }
            }
        }
    }
}

struct TrackRow: View {
    @EnvironmentObject private var model: Model
    let track: LibraryTrack

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(track.title)
                Text([track.artist, track.album].compactMap { $0 }.joined(separator: " · "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Text(track.duration)
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
            if let pid = model.selectedPlaylist {
                Button {
                    model.add(track, to: pid)
                } label: {
                    Image(systemName: "plus.circle")
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Add to playlist")
            }
        }
    }
}
