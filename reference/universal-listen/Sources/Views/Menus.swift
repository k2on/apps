import SwiftUI

/// The actions on one or more songs: the context menu, the row's More menu
/// and the Song menu all use these.
struct TrackMenuItems: View {
    let tracks: [Track]
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav

    var body: some View {
        let items = tracks.map { PlayItem.track($0.id) }
        Section {
            Button("Play", systemImage: "play") { player.play(tracks: tracks) }
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") { player.playNext(items) }
            Button("Play Later", systemImage: "text.line.last.and.arrowtriangle.forward") { player.playLater(items) }
        }
        Section {
            AddToPlaylistMenu(trackIDs: tracks.map(\.id))
            if tracks.count == 1, let t = tracks.first {
                Button(t.isFavorite ? "Undo Favorite" : "Favorite", systemImage: t.isFavorite ? "star.slash" : "star") {
                    library.toggleFavorite(t.id)
                }
            }
        }
        if tracks.count == 1, let t = tracks.first {
            Section {
                Button("Go to Album", systemImage: "square.stack") { nav.go(.album(t.albumID)) }
                Button("Go to Artist", systemImage: "music.mic") { nav.go(.artist(t.artistID)) }
                if SampleData.composerYears[t.composer] != nil {
                    Button("Go to Composer", systemImage: "music.quarternote.3") { nav.go(.composer(t.composer)) }
                }
            }
        }
    }
}

/// "Add to Playlist ▸": the household's own playlists, and a new one.
struct AddToPlaylistMenu: View {
    let trackIDs: [String]
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav

    var body: some View {
        Menu("Add to Playlist", systemImage: "text.badge.plus") {
            Button("New Playlist…", systemImage: "plus") {
                nav.pendingTracks = trackIDs
                nav.sheet = .newPlaylist
            }
            Divider()
            ForEach(library.playlists.filter { !$0.isSmart }) { p in
                Button(p.name, systemImage: "music.note.list") { library.add(trackIDs, to: p.id) }
            }
        }
    }
}

struct AlbumMenuItems: View {
    let album: Album
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav

    var body: some View {
        let tracks = library.tracks(of: album)
        Section {
            Button("Play", systemImage: "play") { player.play(tracks: tracks) }
            Button("Shuffle", systemImage: "shuffle") { player.play(tracks: tracks, shuffled: true) }
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") { player.playNext(tracks.map { .track($0.id) }) }
            Button("Play Later", systemImage: "text.line.last.and.arrowtriangle.forward") { player.playLater(tracks.map { .track($0.id) }) }
        }
        Section {
            AddToPlaylistMenu(trackIDs: album.trackIDs)
            Button("Go to Artist", systemImage: "music.mic") { nav.go(.artist(album.artistID)) }
        }
    }
}

struct EpisodeMenuItems: View {
    let episode: Episode
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav

    var body: some View {
        Section {
            Button(episode.isInProgress ? "Resume" : "Play", systemImage: "play") { player.play(.episode(episode.id)) }
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") { player.playNext([.episode(episode.id)]) }
            Button("Play Later", systemImage: "text.line.last.and.arrowtriangle.forward") { player.playLater([.episode(episode.id)]) }
        }
        Section {
            if episode.progress < 1 {
                Button("Mark as Played", systemImage: "checkmark.circle") { library.markPlayed(episode.id, true) }
            } else {
                Button("Mark as Unplayed", systemImage: "circle") { library.markPlayed(episode.id, false) }
            }
            switch episode.download {
            case .none:
                Button("Download", systemImage: "arrow.down.circle") { library.setDownload(episode.id, .downloaded) }
            case .downloading:
                Button("Stop Download", systemImage: "stop.circle") { library.setDownload(episode.id, .none) }
            case .downloaded:
                Button("Remove Download…", systemImage: "trash", role: .destructive) { nav.dialog = .removeDownload(episode.id) }
            }
        }
        Section {
            Button("Go to Show", systemImage: "play.square.stack") { nav.go(.show(episode.showID)) }
            Button("Episode Details", systemImage: "info.circle") { nav.go(.episode(episode.id)) }
        }
    }
}

/// The "Remove Download?" confirmation, on whichever screen asks.
struct RemoveDownloadDialog: ViewModifier {
    @Environment(Navigator.self) private var nav
    @Environment(Library.self) private var library

    @ViewBuilder func body(content: Content) -> some View {
        let id: String? = if case .removeDownload(let eid) = nav.dialog { eid } else { nil }
        let episode = id.flatMap(library.episode)
        content.confirmationDialog(
            "Remove this download?",
            isPresented: Binding(get: { episode != nil }, set: { if !$0 { nav.dialog = nil } }),
            titleVisibility: .visible,
            presenting: episode
        ) { e in
            Button("Remove Download", role: .destructive) { library.setDownload(e.id, .none) }
            Button("Remove and Mark as Played", role: .destructive) {
                library.setDownload(e.id, .none)
                library.markPlayed(e.id, true)
            }
            Button("Cancel", role: .cancel) {}
        } message: { e in
            Text("“\(e.title)” will stay in your library and can be streamed from the home server.")
        }
    }
}
