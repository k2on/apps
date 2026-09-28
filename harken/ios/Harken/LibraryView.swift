import SwiftUI
import ArkDB

/// Everything in the library, in the order it was added — the `library`
/// query, read against the selected playlist, so each row knows whether it
/// is on it. The row's button puts a track on that playlist or takes it
/// off; its menu does the same for any playlist (`playlists_of` ticks
/// them). With no playlist at all, adding makes "Favorites" first.
struct LibraryView: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        NavigationStack {
            Group {
                if model.tracks.isEmpty {
                    ContentUnavailableView("Nothing here yet", systemImage: "music.note",
                                           description: Text(emptyReason))
                } else {
                    List(model.tracks) { track in
                        TrackRow(track: track)
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

    private var emptyReason: String {
        if model.settings.alone { return "The library arrives from a server's scanner; alone, it stays empty." }
        if model.status?.signedIn == false { return "Sign in (Settings) to sync the library. Playlists made meanwhile are kept." }
        return "The library arrives from the server's scanner."
    }
}

struct TrackRow: View {
    @EnvironmentObject private var model: Model
    let track: LibraryTrack

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(track.title)
                Text(track.creator)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let pid = model.selectedPlaylist, let why = model.caption(for: track, on: pid) {
                    Text(why)
                        .font(.caption2)
                        .foregroundStyle(why.hasPrefix("not saved") ? .red : .secondary)
                }
            }
            Spacer()
            Text(track.duration)
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
            Button {
                if let pid = model.selectedPlaylist, track.playlistPos != nil {
                    model.remove(track, from: pid)
                } else {
                    model.add(track)
                }
            } label: {
                Image(systemName: track.playlistPos != nil ? "checkmark.circle.fill" : "plus.circle")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(track.playlistPos != nil ? "Take off the playlist" : "Add to playlist")
        }
        .contextMenu {
            let on = model.playlistsOf(track)
            ForEach(model.playlists) { p in
                Button {
                    model.toggle(track, on: p.id)
                } label: {
                    if on.contains(p.id) {
                        Label(p.name, systemImage: "checkmark")
                    } else {
                        Text(p.name)
                    }
                }
            }
        }
    }
}
