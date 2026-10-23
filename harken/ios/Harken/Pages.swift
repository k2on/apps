// The three pages a record opens, each zooming out of the cover that was
// tapped and shrinking back into it on the way back (`navigationTransition(
// .zoom)`; the edge swipe and a drag down both take it back, interactively,
// as Android's back preview and pull-down do): an album, in the order the
// work goes and in sections where it has parts; an artist, with their albums
// and then their songs; a playlist, in the order it was made. Each opens
// with a header saying what it is and the numbers true of the whole of it,
// and Play and Shuffle on glass. The lists are views held while the page is
// on its stack (`Holding`), spliced as the log moves. Android's `Pages.kt`.
import SwiftUI

struct AlbumPage: View {
    let name: String
    @StateObject private var holding: Holding
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    init(model: Model, name: String) {
        self.name = name
        _holding = StateObject(wrappedValue: Holding(model, .album(name)))
    }

    var body: some View {
        let page = Page.album(name)
        let tracks = model.rows(page) ?? []
        let album = model.album(named: name)
        let creator = album?.creator ?? tracks.first?.creator
        // Sections only where the album has more than one part: a heading over
        // every row of a one-work album would say its name twice.
        let parts = tracks.map { model.detail($0)?.part ?? "" }
        let sectioned = Set(parts).count > 1
        List {
            PageHeader(kind: "Album", title: name, by: creator, onBy: creator == nil ? nil : { open(.page(.artist(creator ?? ""))) },
                       facts: Spell.facts([Spell.count(tracks.count, "song"), Spell.total(tracks)]), canPlay: !tracks.isEmpty,
                       onPlay: { model.play(tracks, at: 0, from: page) }, onShuffle: { model.shuffle(tracks, from: page) }) {
                CoverView(seed: name, url: RowCover.url(model, album?.art ?? ""), side: Space.header).frame(width: Space.header, height: Space.header)
            }
            .listRowSeparator(.hidden)
            ForEach(Array(tracks.enumerated()), id: \.element.id) { i, t in
                if sectioned && (i == 0 || parts[i] != parts[i - 1]) && !parts[i].isEmpty {
                    Text(parts[i]).font(.subheadline.weight(.semibold)).foregroundStyle(Color.gold).padding(.top, Space.l).listRowSeparator(.hidden)
                }
                let d = model.detail(t)
                TrackRow(track: t, under: (d?.performer).flatMap { $0.isEmpty ? nil : $0 } ?? t.creator, from: page,
                         onPlay: { model.play(tracks, at: i, from: page) }) {
                    TrackNumber(n: d?.track ?? 0, playing: model.playingId == t.id.hex)
                }
            }
        }
        .listStyle(.plain)
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct ArtistPage: View {
    let name: String
    @StateObject private var holding: Holding
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @Environment(\.zoom) private var zoom

    init(model: Model, name: String) {
        self.name = name
        _holding = StateObject(wrappedValue: Holding(model, .artist(name)))
    }

    var body: some View {
        let page = Page.artist(name)
        let tracks = model.rows(page) ?? []
        let albums = model.albums(of: tracks)
        let artist = model.artist(named: name)
        List {
            PageHeader(kind: "Artist", title: name,
                       facts: Spell.facts([albums.isEmpty ? nil : Spell.count(albums.count, "album"), Spell.count(tracks.count, "song"), Spell.total(tracks)]),
                       canPlay: !tracks.isEmpty, onPlay: { model.play(tracks, at: 0, from: page) }, onShuffle: { model.shuffle(tracks, from: page) }) {
                CoverView(seed: name, url: RowCover.url(model, artist?.art ?? ""), side: 180, round: true).frame(width: 180, height: 180)
            }
            .listRowSeparator(.hidden)
            if !albums.isEmpty {
                Section("Albums") {
                    CoverShelf(items: albums) { a in
                        CoverCard(seed: a.name, url: RowCover.url(model, a.art), title: a.name, under: Spell.count(Int(a.tracks), "song"), side: 136, opens: .page(.album(a.name))) {
                            open(.page(.album(a.name)))
                        }
                    }
                    .listRowInsets(EdgeInsets())
                }
            }
            Section("Songs") {
                ForEach(Array(tracks.enumerated()), id: \.element.id) { i, t in
                    TrackRow(track: t, under: model.album(of: t)?.name ?? t.creator, from: page, onPlay: { model.play(tracks, at: i, from: page) }) { RowCover(track: t) }
                }
            }
        }
        .listStyle(.plain)
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct PlaylistPage: View {
    let id: Id
    @StateObject private var holding: Holding
    @EnvironmentObject private var model: Model

    init(model: Model, id: Id) {
        self.id = id
        _holding = StateObject(wrappedValue: Holding(model, .playlist(id)))
    }

    var body: some View {
        let page = Page.playlist(id)
        let read = model.rows(page)
        let tracks = read ?? []
        let playlist = model.playlist(id)
        let name = playlist?.name ?? "Playlist"
        List {
            PageHeader(kind: "Playlist", title: name, facts: Spell.facts([Spell.count(tracks.count, "song"), Spell.total(tracks)]), canPlay: !tracks.isEmpty,
                       onPlay: { model.play(tracks, at: 0, from: page) }, onShuffle: { model.shuffle(tracks, from: page) }) {
                CoverView(seed: name, url: "", side: Space.header).frame(width: Space.header, height: Space.header)
            }
            .listRowSeparator(.hidden)
            if playlist == nil {
                ContentUnavailableView("This playlist is gone", systemImage: "music.note.list", description: Text("It was taken off this server, or it isn't yours."))
                    .listRowSeparator(.hidden)
            } else if read != nil && tracks.isEmpty {
                ContentUnavailableView("Nothing on it yet", systemImage: "music.note.list", description: Text("Put a song on it from the song's menu, anywhere in your library."))
                    .listRowSeparator(.hidden)
            }
            ForEach(Array(tracks.enumerated()), id: \.element.id) { i, t in
                TrackRow(track: t, under: [t.creator, model.album(of: t)?.name].compactMap { $0 }.joined(separator: " · "), from: page,
                         onPlay: { model.play(tracks, at: i, from: page) }) { RowCover(track: t) }
                    .swipeActions {
                        Button(role: .destructive) { model.remove(t, from: id) } label: { Label("Remove", systemImage: "minus.circle") }
                    }
            }
        }
        .listStyle(.plain)
        .navigationBarTitleDisplayMode(.inline)
    }
}
