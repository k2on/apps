// Library: everything, four ways — the person's playlists, the albums, the
// people (the composers some work here is by, then everyone else who made
// something), and every song in the order it arrived — each opening the page
// it stands for, zooming out of its cover (Android's `LibraryScreen.kt`) —
// and the podcasts and the sermons, always there, so the first can be
// followed. They are a row of capsules under the large title that scrolls
// sideways, as Android's chips are; the playlists list's first row, New
// Playlist, is where a new one is made, and the domain's own checks decide
// whether a name will do; on Podcasts and Sermons the first row (or, with
// none yet, the empty page's button) finds a show to follow (`FollowSheet`).
// The large title's line has the account's face alone: no +.
import ArkUI
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
        // The filter row, inset under the navigation bar rather than a bar of
        // its own (`safeAreaBar`), whose soft edge would wash out the large
        // title over it. On the page's own colour, so a list scrolled up goes
        // under it rather than through the capsules.
        .safeAreaInset(edge: .top, spacing: 0) {
            VStack(spacing: 0) {
                // The large title, the account's face at the end of its line:
                // what a list makes is its own first row.
                LargeTitleRow("Library") { AccountButton() }
                // Podcasts and Sermons always, with none of the kind yet too: their page is where the first is followed.
                LibraryChips(tab: $tab)
                if let host = model.unreachable { Offline(host: host) }
            }
            .background(Color.page)
        }
        // Its title stays put over the chips, so the bar stays empty.
        .largeTitleBar("Library")
    }
}

/// The Library's lists as a row of capsules that scrolls sideways, the one
/// shown filled in the accent and the rest quiet: Android's chips, in the
/// same order and words. Each is a button named by its list, which is what
/// the screenshot driver taps.
private struct LibraryChips: View {
    @Binding var tab: LibraryTab

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: Space.s) {
                ForEach(LibraryTab.allCases) { t in
                    Chip(t.rawValue, selected: t == tab) { tab = t }
                }
            }
            .padding(.horizontal, Space.page)
            // The page's first content: the title's gap over it, as every tab has.
            .padding(.top, AccountSizes.titleBelow)
            .padding(.bottom, Space.s)
        }
        .scrollClipDisabled()
        .animation(Springs.fade, value: tab)
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
                    Image(systemName: "plus").font(.title2).foregroundStyle(Color.accentText).frame(width: 56, height: 56)
                        .surface(in: RoundedRectangle(cornerRadius: Space.corner, style: .continuous))
                    Text("New Playlist").font(.headline)
                }
            }
            .foregroundStyle(.ink)
            if model.playlists.isEmpty {
                ContentUnavailableView("No playlists yet", systemImage: "music.note.list", description: Text("Make one, then put songs on it from any song's menu."))
                    .listRowSeparator(.hidden)
            }
            ForEach(model.playlists) { p in
                let r = Route.playlist(name: p.name)
                Button { open(r) } label: {
                    HStack(spacing: Space.l) {
                        CoverView(seed: p.name, url: "", side: 56).frame(width: 56, height: 56).zoomSource(r, zoom)
                        VStack(alignment: .leading) {
                            Text(p.name).font(.headline).lineLimit(1)
                            Text("Playlist").font(.subheadline).foregroundStyle(.quiet)
                        }
                        Spacer()
                        Image(systemName: "chevron.right").font(.footnote).foregroundStyle(.outline)
                    }
                }
                .foregroundStyle(.ink)
            }
        }
        .listStyle(.plain)
        .scrollEdges()
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
                        let r = Route.album(key: a.id, title: a.name)
                        Button { open(r) } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                CoverView(seed: a.name, url: RowCover.url(model, a.art), side: 180)
                                    .aspectRatio(1, contentMode: .fit)
                                    .zoomSource(r, zoom)
                                    .padding(.bottom, Space.s - 2)
                                Text(a.name).font(.subheadline.weight(.semibold)).lineLimit(1)
                                Text(a.playedBy).font(.caption).foregroundStyle(.quiet).lineLimit(1)
                            }
                        }
                        .buttonStyle(.press)
                    }
                }
                .padding(Space.page)
            }
            .scrollEdges()
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
                Label {
                    Text(kind == "sermon" ? "No sermons yet" : "No podcasts yet")
                } icon: {
                    Image.show(sermon: kind == "sermon")
                }
            } description: {
                Text("Follow one: search Apple Podcasts, or paste its feed's address. A folder of it under \(kind)s/ on the server appears here too.")
            } actions: {
                Button(kind == "sermon" ? "Follow Sermons" : "Follow a Podcast") { present(.follow(kind: kind)) }
                    .buttonStyle(.glassProminent)
            }
        } else {
            ScrollView {
                VStack(spacing: Space.xl) {
                    // Following another, first, as New Playlist is the playlists' first row.
                    Button { present(.follow(kind: kind)) } label: {
                        HStack(spacing: Space.l) {
                            Image(systemName: "plus").font(.title2).foregroundStyle(Color.accentText).frame(width: 56, height: 56)
                                .surface(in: RoundedRectangle(cornerRadius: Space.corner, style: .continuous))
                            Text(kind == "sermon" ? "Follow Sermons" : "Follow a Podcast").font(.headline)
                            Spacer(minLength: 0)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.pressRow)
                    .foregroundStyle(.ink)
                    // Followed and not a show yet: the server has not read its feed, or could not.
                    if !unread.isEmpty {
                        VStack(spacing: Space.s) {
                            ForEach(unread) { f in UnreadFeed(feed: f) }
                        }
                    }
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: Space.l)], spacing: Space.xl) {
                        ForEach(shows) { s in
                            let r = Route.show(key: s.id, title: s.title)
                            Button { open(r) } label: {
                                VStack(alignment: .leading, spacing: 2) {
                                    CoverView(seed: s.title, url: RowCover.url(model, s.art), side: 180)
                                        .aspectRatio(1, contentMode: .fit)
                                        .zoomSource(r, zoom)
                                        .padding(.bottom, Space.s - 2)
                                    Text(s.title).font(.subheadline.weight(.semibold)).lineLimit(2)
                                    Text(s.author.isEmpty ? formatCount(n: s.episodes, word: "episode") : s.author).font(.caption).foregroundStyle(.quiet).lineLimit(1)
                                }
                            }
                            .buttonStyle(.press)
                        }
                    }
                }
                .padding(Space.page)
                .motion(Springs.move, value: unread.map { $0.url })
            }
            .scrollEdges()
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
            Image(systemName: feed.error.isEmpty ? "clock" : "exclamationmark.triangle").foregroundStyle(.quiet).frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(feed.title.isEmpty ? feed.url : feed.title).font(.subheadline.weight(.semibold)).lineLimit(1).truncationMode(.middle)
                Text(feed.error.isEmpty ? "Not read yet" : "Could not read it: \(feed.error)").font(.caption).foregroundStyle(.quiet).lineLimit(3)
            }
            Spacer(minLength: 0)
            Button("Unfollow", role: .destructive) { model.unfollow(feed) }.buttonStyle(.borderless).font(.subheadline)
        }
        .padding(Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .surface(in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .motionTransition(.opacity.combined(with: .move(edge: .top)))
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
                            let life = formatSpan(born: c.born, died: c.died)
                            PersonRow(name: c.name, art: c.art, under: life.isEmpty ? formatCount(n: c.works, word: "work") : life, to: .artist(name: c.name))
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
            .scrollEdges()
        }
    }

    private func artists(_ list: [ArtistsEntry]) -> some View {
        ForEach(list) { a in
            PersonRow(name: a.name, art: a.art, under: formatCount(n: a.tracks, word: "song"), to: .artist(name: a.name))
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
                    Text(under).font(.subheadline).foregroundStyle(.quiet)
                }
                Spacer()
                Image(systemName: "chevron.right").font(.footnote).foregroundStyle(.outline)
            }
        }
        .foregroundStyle(.ink)
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
            .scrollEdges()
        }
    }
}
