// The pages a record, a person and a work open, each zooming out of the
// cover that was tapped and shrinking back into it on the way back
// (`navigationTransition(.zoom)`; the edge swipe and a drag down both take
// it back, interactively, as Android's back preview and pull-down do). Laid
// out as the desktop lays them out (`harken/iced/src/view.rs`), a record
// shop's catalogue: an album, its cover beside its title and who played it,
// then its tracks under the works they are movements of, then its small
// print; a person — one page, whatever they did — under their banner,
// which runs up under the navigation bar so its back button is glass over
// the picture, their most recorded work set apart with a Play, their best
// known works and a "See All", their records, and the songs of somebody
// who wrote none; a work's fullest performance set apart and every
// performance of it; a performance, laid out as its album is, then the
// records it is on and everybody on it. And a playlist, in the order it was
// made. The lists are views held while the page is on its stack
// (`Holding`), spliced as the log moves. Android's `Pages.kt`.
import SwiftUI

/// A catalogue's page: one scrolling column — with a `hero` banner, run up
/// under the navigation bar, whose background stands aside for it.
private struct CataloguePage<Content: View>: View {
    var hero = false
    @ViewBuilder let content: () -> Content

    var body: some View {
        let under: Edge.Set = hero ? .top : []
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) { content() }
                .padding(.bottom, Space.xl)
        }
        .ignoresSafeArea(.container, edges: under)
        .toolbarBackgroundVisibility(hero ? .hidden : .automatic, for: .navigationBar)
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// A shelf of records, each opening its page out of its cover.
private struct AlbumShelf: View {
    let albums: [AlbumsEntry]
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    var body: some View {
        CoverShelf(items: albums) { a in
            CoverCard(seed: a.name, url: RowCover.url(model, a.art), title: a.name, under: a.playedBy, side: 136, opens: .page(.album(a.name))) {
                open(.page(.album(a.name)))
            }
        }
        .padding(.top, Space.xs)
    }
}

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
        let creator = album?.creator ?? tracks.first?.creator ?? ""
        // Who played it, in the gold: whoever is credited on the most of it,
        // which on a record by one orchestra is the orchestra; whose it is
        // then goes under, in small capitals. Nobody credited, and it is
        // whoever made it.
        let credited = album?.performer ?? ""
        let performer: String? = credited.isEmpty || credited == creator ? nil : credited
        let year = album?.released ?? 0
        let small = Spell.facts([performer == nil ? nil : creator.uppercased(), year > 0 ? String(year) : nil])
        let toCreator: (() -> Void)? = creator.isEmpty ? nil : { open(.page(.artist(creator))) }
        CataloguePage {
            RecordHeader(title: name, by: performer ?? creator, onBy: performer == nil ? toCreator : nil,
                         small: small, onSmall: performer == nil ? nil : toCreator, canPlay: !tracks.isEmpty,
                         onPlay: { model.play(tracks, at: 0, from: page) }, onShuffle: { model.shuffle(tracks, from: page) }) {
                CoverView(seed: name, url: RowCover.url(model, album?.art ?? ""), side: Space.record)
            }
            CatalogueTracks(tracks: tracks, page: page, creator: creator)
            RecordFoot(year: year, tracks: tracks, label: album?.label ?? "")
        }
    }
}

/// A person's page — the one page a person has, whatever they did — laid
/// out as a catalogue opens a composer's: their banner (or, for somebody
/// with no wide picture, their portrait beside their name); their most
/// recorded work set apart with a Play; their best known works and a "See
/// All" that lists every one by catalogue; their records; and, for somebody
/// who wrote nothing here, their songs. Each section only when it has
/// something in it.
struct ArtistPage: View {
    let name: String
    @StateObject private var holding: Holding
    @State private var all = false
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    init(model: Model, name: String) {
        self.name = name
        _holding = StateObject(wrappedValue: Holding(model, .artist(name)))
    }

    var body: some View {
        let page = Page.artist(name)
        let tracks = model.rows(page) ?? []
        let works = model.works(of: name) ?? []
        let albums = model.albums(by: name)
        let artist = model.artist(named: name)
        let composer = model.composer(named: name)
        let counts = Spell.facts([Spell.count(albums.count, "album"), Spell.count(tracks.count, "track")])
        // What they are and when they lived, as a catalogue heads a person;
        // nobody's years known, and it is how much of them there is here.
        let lived = Catalogue.life(born: artist?.born ?? 0, died: artist?.died ?? 0)
        let life = lived.isEmpty ? Catalogue.life(born: composer?.born ?? 0, died: composer?.died ?? 0) : lived
        let kicker = Spell.facts([works.isEmpty ? "Artist" : "Composer", life.isEmpty ? counts : life])
        let banner = [artist?.banner, composer?.banner].compactMap { $0 }.first(where: { !$0.isEmpty }) ?? ""
        let portrait = [artist?.art, composer?.art].compactMap { $0 }.first(where: { !$0.isEmpty }) ?? ""
        // Songs only for somebody who wrote nothing here: a composer's are
        // their works', and those are the works' pages.
        let songs = works.isEmpty && !tracks.isEmpty
        let play: (() -> Void)? = songs ? { model.play(tracks, at: 0, from: page) } : nil
        let shuffle = { model.shuffle(tracks, from: page) }
        let listed = Catalogue.listed(works, all: all)
        let short = listed.count < works.count
        let by = Catalogue.initials(name)
        CataloguePage(hero: !banner.isEmpty) {
            if !banner.isEmpty {
                Banner(url: RowCover.url(model, banner), kind: kicker, name: name)
                if let play = play {
                    PlayShuffle(canPlay: true, onPlay: play, onShuffle: shuffle)
                        .padding(.horizontal, Space.page)
                        .padding(.top, Space.l)
                }
            } else {
                RecordHeader(over: kicker, title: name, small: life.isEmpty ? "" : counts, onPlay: play, onShuffle: shuffle) {
                    CoverView(seed: name, url: RowCover.url(model, portrait), side: Space.record, round: true)
                }
            }
            if let best = Catalogue.featured(works) {
                Featured(label: "Most Recorded", seed: best.id, url: RowCover.url(model, best.art), title: best.title,
                         facts: Catalogue.featuredFacts(best), opens: .page(.work(best.id))) { model.playWork(best.id) }
            }
            if !works.isEmpty {
                SerifHeading(title: Catalogue.heading(works, all: all))
                // Each under who it is by, its catalogue number under it, and
                // how many performances of it there are at its right.
                ForEach(listed) { w in
                    EntryRow(over: by, title: w.title, under: w.catalogue, count: String(w.recordings), onOpen: { open(.page(.work(w.id))) }) {
                        EmptyView()
                    }
                }
                if short {
                    MoreRow(title: "See All \(works.count) Works") { all = true }
                }
            }
            if !albums.isEmpty {
                SerifHeading(title: "Albums")
                AlbumShelf(albums: albums)
            }
            if songs {
                SerifHeading(title: "Songs")
                CatalogueTracks(tracks: tracks, page: page, creator: name)
            }
        }
    }
}

/// A work's page, as a catalogue sets one: its composer in small capitals
/// over its title, the way back to them, what it is under, a hairline; then
/// its fullest performance set apart with a Play, and every performance —
/// cover, who played it, when, how long — the same music told apart by who
/// played it. A work of one performance has only that one, set apart.
struct WorkPage: View {
    let id: String
    @StateObject private var holding: Holding
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    init(model: Model, id: String) {
        self.id = id
        _holding = StateObject(wrappedValue: Holding(model, .work(id)))
    }

    var body: some View {
        let work = model.work(id)
        let composer = work?.composer ?? ""
        let facts = work.map { Catalogue.facts($0) } ?? ""
        let read = model.recordings(of: id)
        let takes = read ?? []
        CataloguePage {
            VStack(alignment: .leading, spacing: Space.xs) {
                if !composer.isEmpty {
                    Button { open(.page(.artist(composer))) } label: {
                        Text(composer.uppercased() + "  \u{203A}").font(.footnote.weight(.semibold)).foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
                Text(work?.title ?? "").font(.serif(30, relativeTo: .largeTitle)).fixedSize(horizontal: false, vertical: true)
                if !facts.isEmpty { Text(facts).font(.subheadline).foregroundStyle(.secondary) }
            }
            .padding(.horizontal, Space.page)
            .padding(.vertical, Space.l)
            Divider().padding(.horizontal, Space.page)
            if read != nil && takes.isEmpty {
                Text("No recordings of it yet.").font(.subheadline).foregroundStyle(.secondary)
                    .padding(.horizontal, Space.page).padding(.top, Space.l)
            }
            if let first = takes.first {
                Featured(label: takes.count > 1 ? "Most Complete" : "The Recording", seed: first.id, url: RowCover.url(model, first.art),
                         title: Catalogue.who(first), facts: Catalogue.facts(first), opens: .page(.recording(first.id))) {
                    model.playRecording(first.id)
                }
            }
            if takes.count > 1 {
                SerifHeading(title: "Recordings")
                ForEach(Array(takes.enumerated()), id: \.element.id) { i, r in
                    EntryRow(title: Catalogue.who(r), under: Catalogue.facts(r), onOpen: { open(.page(.recording(r.id))) }) {
                        // The first is set apart above and zooms from there.
                        TakeCover(take: r, opens: i == 0 ? nil : .page(.recording(r.id)))
                    }
                }
            }
        }
    }
}

/// A performance's cover beside its line, the zoom's source when it opens a page from here.
private struct TakeCover: View {
    let take: RecordingsEntry
    let opens: Route?
    @EnvironmentObject private var model: Model
    @Environment(\.zoom) private var zoom

    var body: some View {
        let cover = CoverView(seed: take.id, url: RowCover.url(model, take.art), side: 56, corner: 4).frame(width: 56, height: 56)
        if let r = opens { cover.zoomSource(r, zoom) } else { cover }
    }
}

/// One performance's page, laid out as its album's is: the cover beside the
/// work and who played it, the movements, the small print — then the
/// records it is on, and everybody on it, the composer first.
struct RecordingPage: View {
    let id: String
    @StateObject private var holding: Holding
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    init(model: Model, id: String) {
        self.id = id
        _holding = StateObject(wrappedValue: Holding(model, .recording(id)))
    }

    var body: some View {
        let page = Page.recording(id)
        let tracks = model.rows(page) ?? []
        let key = Catalogue.workKey(id)
        let work = key.flatMap { model.work($0) }
        let take = key.flatMap { model.recordings(of: $0) }?.first(where: { $0.id == id })
        let composer = work?.composer ?? ""
        let who = take?.performers ?? ""
        let year = take?.recorded ?? 0
        let albums = model.albums(of: tracks)
        let people = Catalogue.people(composer: composer, credits: model.credits(of: id))
        CataloguePage {
            RecordHeader(over: composer, onOver: composer.isEmpty ? nil : { open(.page(.artist(composer))) },
                         title: work == nil ? who : Catalogue.title(work), by: work == nil ? "" : who,
                         small: year > 0 ? String(year) : "", canPlay: !tracks.isEmpty, onPlay: { model.play(tracks, at: 0, from: page) }) {
                CoverView(seed: id, url: RowCover.url(model, take?.art ?? ""), side: Space.record)
            }
            CatalogueTracks(tracks: tracks, page: page, creator: "", headings: false)
            RecordFoot(year: year, tracks: tracks, label: take?.label ?? "")
            if !albums.isEmpty {
                SerifHeading(title: albums.count == 1 ? "From the Album" : "From the Albums")
                AlbumShelf(albums: albums)
            }
            if !people.isEmpty {
                SerifHeading(title: "On This Recording")
                CoverShelf(items: people) { p in Face(billed: p) }
                    .padding(.top, Space.xs)
            }
        }
    }
}

/// Somebody on a performance, as a face: their name and what they did on
/// it, opening their page where they have one.
private struct Face: View {
    let billed: Catalogue.Billed
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    var body: some View {
        let composer = model.composer(named: billed.name)
        let artist = model.artist(named: billed.name)
        let art = [artist?.art, composer?.art].compactMap { $0 }.first(where: { !$0.isEmpty }) ?? ""
        let to: Route? = composer != nil || artist != nil ? .page(.artist(billed.name)) : nil
        CoverCard(seed: billed.name, url: RowCover.url(model, art), title: billed.name, under: billed.role, side: 104, round: true, opens: to) {
            if let r = to { open(r) }
        }
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
