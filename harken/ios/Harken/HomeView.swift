// Home: where you were, and what is new (Android's `HomeScreen.kt`). What
// was played last and a way back to it — the same list, from the same track
// — then what arrived most recently, the person's playlists, and the albums,
// each a shelf of covers zooming open its page. What was played last is the
// account's, not the phone's: the listening session's track while there is
// a session to be part of, else the newest play in the log, the same on
// every device and there offline.
import SwiftUI

/// How many of the newest songs Home's shelf shows.
let recentMost = 12

struct HomeView: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @Environment(\.showTab) private var showTab
    @Environment(\.present) private var present

    var body: some View {
        let s = model.snapshot
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if let host = model.unreachable { Offline(host: host) }
                if !s.loaded {
                    EmptyView()
                } else if s.tracks.isEmpty, let host = model.fetching {
                    Fetching(host: host)
                } else if s.tracks.isEmpty {
                    ContentUnavailableView("Nothing here yet", systemImage: "music.note",
                                           description: Text("Harken plays what your server finds. Put music in its media folder, under music/, and it appears here."))
                        .padding(.top, 48)
                } else {
                    content(s)
                }
            }
            .padding(.bottom, Space.xl)
        }
        .navigationTitle("Home")
        .animation(Springs.fade, value: s.loaded)
    }

    @ViewBuilder private func content(_ s: Snapshot) -> some View {
        if let last = model.lastPlayed {
            Heading(title: "Last Played")
            LastCard(last: last, from: last.saying(s.playlists)) {
                model.resume(last) { model.fullPlayer = true }
            }
        }
        let recent = Array(s.tracks.suffix(recentMost).reversed())
        Heading(title: "Recently Added", action: "All Songs") { showTab(.library, .songs) }
        CoverShelf(items: recent) { t in
            let a = model.album(of: t)
            CoverCard(seed: a?.name ?? t.title, url: RowCover.url(model, a?.art ?? ""), title: t.title, under: t.creator, side: Space.shelf) {
                model.play(recent, at: recent.firstIndex(of: t) ?? 0, from: nil)
            }
        }
        Heading(title: "Your Playlists", action: "New") { present(.newPlaylist(then: nil)) }
        if s.playlists.isEmpty {
            Text("None yet. Make one, then put songs on it from any song's menu.")
                .font(.subheadline).foregroundStyle(.secondary).padding(.horizontal, Space.page)
        } else {
            CoverShelf(items: s.playlists) { p in
                CoverCard(seed: p.name, url: "", title: p.name, under: "Playlist", side: Space.shelf, opens: .page(.playlist(p.id))) {
                    open(.page(.playlist(p.id)))
                }
            }
        }
        if !s.albums.isEmpty {
            Heading(title: "Albums", action: "See All") { showTab(.library, .albums) }
            CoverShelf(items: s.albums) { a in
                CoverCard(seed: a.name, url: RowCover.url(model, a.art), title: a.name, under: a.playedBy, side: Space.shelf, opens: .page(.album(a.name))) {
                    open(.page(.album(a.name)))
                }
            }
        }
    }
}

/// What was played last, and the button that plays it again from there — on glass.
private struct LastCard: View {
    let last: LastPlayed
    let from: String
    let resume: () -> Void

    var body: some View {
        Button(action: resume) {
            HStack(spacing: Space.m) {
                CoverView(seed: last.track.seed, url: last.track.art, side: 64).frame(width: 64, height: 64)
                VStack(alignment: .leading, spacing: 2) {
                    Text(last.track.title).font(.headline).lineLimit(1)
                    Text(last.track.artist).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                    Text(from).font(.caption).foregroundStyle(last.on != nil ? Color.gold : .secondary).lineLimit(1)
                }
                Spacer()
                Image(systemName: "play.fill").font(.title3).frame(width: 44, height: 44)
                    .glassEffect(.regular.tint(.gold).interactive(), in: Circle())
                    .accessibilityLabel("Play again")
            }
            .padding(Space.m)
            .glassEffect(.regular.interactive(), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        }
        .buttonStyle(.plain)
        .padding(.horizontal, Space.page)
    }
}
