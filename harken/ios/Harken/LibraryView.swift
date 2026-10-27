// Library: everything, four ways — the person's playlists, the albums, the
// people (the composers some work here is by, then everyone else who made
// something), and every song in the order it arrived — each opening the page
// it stands for, zooming out of its cover (Android's `LibraryScreen.kt`) —
// and the podcasts and the sermons, always there, so the first can be
// followed. They are a segmented control in the glass of the navigation
// bar's bottom; the playlists list is where a new one is made, and the
// domain's own checks decide whether a name will do; the + on Podcasts and
// Sermons finds a show to follow (`FollowSheet`).
import SwiftUI

enum LibraryTab: String, CaseIterable, Identifiable {
    case playlists = "Playlists", albums = "Albums", artists = "Artists", songs = "Songs", podcasts = "Podcasts", sermons = "Sermons"
    var id: String { rawValue }

    /// The show kind the tab lists, for Podcasts and Sermons.
    var kind: String? {
        switch self {
        case .podcasts: return "podcast"
        case .sermons: return "sermon"
        default: return nil
        }
    }
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
                case .podcasts, .sermons: if s.complete { ShowsGrid(kind: tab.kind ?? "") } else { Color.clear }
                }
            }
        }
        .navigationTitle("Library")
        .safeAreaInset(edge: .top, spacing: 0) {
            VStack(spacing: 0) {
                SwiftUI.Picker("Show", selection: $tab) {
                    // Podcasts and Sermons always, with none of the kind yet too: their + is where the first is followed.
                    ForEach(LibraryTab.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .padding(.horizontal, Space.page)
                .padding(.vertical, Space.s)
                if let host = model.unreachable { Offline(host: host) }
            }
        }
        .toolbar {
            // On Podcasts and Sermons the + finds a show to follow; on the rest it makes a playlist.
            ToolbarItem(placement: .topBarLeading) {
                if let kind = tab.kind {
                    Button { present(.follow(kind: kind)) } label: { Image(systemName: "plus") }
                        .accessibilityLabel(kind == "sermon" ? "Follow sermons" : "Follow a podcast")
                } else {
                    Button { present(.newPlaylist(then: nil)) } label: { Image(systemName: "plus") }
                        .accessibilityLabel("New playlist")
                }
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
                                Text(a.playedBy).font(.caption).foregroundStyle(.secondary).lineLimit(1)
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

private struct ShowsGrid: View {
    let kind: String
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @Environment(\.present) private var present
    @Environment(\.zoom) private var zoom

    var body: some View {
        let shows = model.shows(of: kind)
        let unread = model.unread(of: kind)
        if shows.isEmpty && unread.isEmpty {
            ContentUnavailableView {
                Label(kind == "sermon" ? "No sermons yet" : "No podcasts yet", systemImage: "mic")
            } description: {
                Text("Tap + to follow one: search Apple Podcasts, or paste its feed's address. A folder of it under \(kind)s/ on the server appears here too.")
            } actions: {
                Button(kind == "sermon" ? "Follow Sermons" : "Follow a Podcast") { present(.follow(kind: kind)) }
                    .buttonStyle(.glassProminent)
            }
        } else {
            ScrollView {
                VStack(spacing: Space.xl) {
                    // Followed and not a show yet: the server has not read its feed, or could not.
                    if !unread.isEmpty {
                        VStack(spacing: Space.s) {
                            ForEach(unread) { f in UnreadFeed(feed: f) }
                        }
                    }
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: Space.l)], spacing: Space.xl) {
                        ForEach(shows) { s in
                            let r = Route.page(.show(s.id))
                            Button { open(r) } label: {
                                VStack(alignment: .leading, spacing: 2) {
                                    CoverView(seed: s.title, url: RowCover.url(model, s.art), side: 180)
                                        .aspectRatio(1, contentMode: .fit)
                                        .zoomSource(r, zoom)
                                        .padding(.bottom, Space.s - 2)
                                    Text(s.title).font(.subheadline.weight(.semibold)).lineLimit(2)
                                    Text(s.author.isEmpty ? Spell.count(Int(s.episodes), "episode") : s.author).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                                }
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
                .padding(Space.page)
                .animation(Springs.move, value: unread.map { $0.url })
            }
        }
    }
}

/// A feed followed and not read into a show yet: its address, whether the
/// server has read it at all or why it could not, and Unfollow — on glass.
private struct UnreadFeed: View {
    let feed: SubscriptionsEntry
    @EnvironmentObject private var model: Model

    var body: some View {
        HStack(spacing: Space.m) {
            Image(systemName: feed.error.isEmpty ? "clock" : "exclamationmark.triangle").foregroundStyle(.secondary).frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(feed.title.isEmpty ? feed.url : feed.title).font(.subheadline.weight(.semibold)).lineLimit(1).truncationMode(.middle)
                Text(feed.error.isEmpty ? "Not read yet" : "Could not read it: \(feed.error)").font(.caption).foregroundStyle(.secondary).lineLimit(3)
            }
            Spacer(minLength: 0)
            Button("Unfollow", role: .destructive) { model.unfollow(feed) }.buttonStyle(.borderless).font(.subheadline)
        }
        .padding(Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassEffect(.regular, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .transition(.opacity.combined(with: .move(edge: .top)))
    }
}

private struct ArtistsList: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        let composers = model.composers
        let named = Set(composers.map { $0.name })
        let others = model.artists.filter { !named.contains($0.name) }
        if composers.isEmpty && others.isEmpty {
            ContentUnavailableView("No artists yet", systemImage: "music.microphone", description: Text("Whoever made what the server finds appears here."))
        } else {
            List {
                // A composer once, under the composers; their page is the one page a person has.
                if !composers.isEmpty {
                    Section("Composers") {
                        ForEach(composers) { c in
                            let life = Catalogue.life(born: c.born, died: c.died)
                            PersonRow(name: c.name, art: c.art, under: life.isEmpty ? Spell.count(Int(c.works), "work") : life, to: .page(.artist(c.name)))
                        }
                    }
                }
                if composers.isEmpty {
                    artists(others)
                } else if !others.isEmpty {
                    Section("Artists") { artists(others) }
                }
            }
            .listStyle(.plain)
        }
    }

    private func artists(_ list: [ArtistsEntry]) -> some View {
        ForEach(list) { a in
            PersonRow(name: a.name, art: a.art, under: Spell.count(Int(a.tracks), "song"), to: .page(.artist(a.name)))
        }
    }
}

/// Somebody in the Library's list: their face, their name, a line under, and
/// the page they open, zooming out of the face.
private struct PersonRow: View {
    let name: String
    let art: String
    let under: String
    let to: Route
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @Environment(\.zoom) private var zoom

    var body: some View {
        Button { open(to) } label: {
            HStack(spacing: Space.l) {
                CoverView(seed: name, url: RowCover.url(model, art), side: 56, round: true).frame(width: 56, height: 56).zoomSource(to, zoom)
                VStack(alignment: .leading) {
                    Text(name).font(.headline).lineLimit(1)
                    Text(under).font(.subheadline).foregroundStyle(.secondary)
                }
                Spacer()
                Image(systemName: "chevron.right").font(.footnote).foregroundStyle(.tertiary)
            }
        }
        .foregroundStyle(.primary)
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
