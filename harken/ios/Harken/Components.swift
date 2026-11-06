// The pieces harken's screens are made of: a track's row, an episode's, who
// is on it, and their menu, a
// page's header with Play and Shuffle, a shelf of covers and the card that
// opens a page out of its cover, a section's heading, and the banner a
// server that cannot be reached puts over a tab — Android's `Components.kt`
// and `ui/Components.kt`. And the two things every screen can ask of the
// frame around it (Shell.swift), handed down the environment: open a page on
// this tab's stack, and raise a sheet.
import ArkUI
import SwiftUI

/// What rises over a page.
enum Sheet: Identifiable, Equatable {
    case addTo(LibraryEntry)
    case newPlaylist(then: LibraryEntry?)
    case queue
    case devices
    /// Finding a show to follow, as a podcast (`podcast`) or as sermons (`sermon`).
    case follow(kind: String)
    /// What plays, word for word, as it is said.
    case transcript

    var id: String {
        switch self {
        case .addTo(let t): return "add:" + t.id.hex
        case .newPlaylist(let t): return "new:" + (t?.id.hex ?? "")
        case .queue: return "queue"
        case .devices: return "devices"
        case .follow(let kind): return "follow:" + kind
        case .transcript: return "transcript"
        }
    }
}

private struct OpenKey: EnvironmentKey { static let defaultValue: (Route) -> Void = { _ in } }
private struct PresentKey: EnvironmentKey { static let defaultValue: (Sheet) -> Void = { _ in } }
private struct ZoomKey: EnvironmentKey { static let defaultValue: Namespace.ID? = nil }
private struct TabKey: EnvironmentKey { static let defaultValue: (Pane, LibraryTab?) -> Void = { _, _ in } }

extension EnvironmentValues {
    /// Open a page on the stack of the tab it is asked from.
    var open: (Route) -> Void {
        get { self[OpenKey.self] }
        set { self[OpenKey.self] = newValue }
    }

    /// Raise a sheet over everything.
    var present: (Sheet) -> Void {
        get { self[PresentKey.self] }
        set { self[PresentKey.self] = newValue }
    }

    /// The namespace a page zooms out of its cover in.
    var zoom: Namespace.ID? {
        get { self[ZoomKey.self] }
        set { self[ZoomKey.self] = newValue }
    }

    /// Show a tab — the Library at one of its lists.
    var showTab: (Pane, LibraryTab?) -> Void {
        get { self[TabKey.self] }
        set { self[TabKey.self] = newValue }
    }
}

extension View {
    /// The cover a page zooms out of, and back into (`matchedTransitionSource`).
    @ViewBuilder func zoomSource(_ route: Route, _ ns: Namespace.ID?) -> some View {
        if let ns = ns { matchedTransitionSource(id: route, in: ns) } else { self }
    }

    /// A page that zooms out of the cover it was opened from (`navigationTransition(.zoom)`).
    @ViewBuilder func zoomed(_ route: Route, _ ns: Namespace.ID?) -> some View {
        if let ns = ns { navigationTransition(.zoom(sourceID: route, in: ns)) } else { self }
    }
}

/// A track's row: what stands at its start (a cover, or its number on the
/// release), its title — in the gold while it plays — who made it, its
/// length, and its menu, which is also the row's long press.
struct TrackRow<Leading: View>: View {
    let track: LibraryEntry
    let under: String
    /// The page its list came from: a playlist offers to take it off.
    let from: Page?
    let onPlay: () -> Void
    @ViewBuilder let leading: () -> Leading
    @EnvironmentObject private var model: Model

    var body: some View {
        let playing = model.playingId == track.id.hex
        HStack(spacing: Space.m) {
            // The row plays, lit under the finger as it lands (`.pressRow`).
            // Its menu is beside the button, not in its label: a menu inside
            // a button's label leaves both answering one touch, so a tap on
            // the "…" could play the song as well as open the menu.
            Button(action: onPlay) {
                HStack(spacing: Space.m) {
                    leading().frame(width: Space.thumb, height: Space.thumb)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(track.title).font(.body).foregroundStyle(playing ? Color.accentText : Color.ink).lineLimit(1)
                        Text(under).font(.subheadline).foregroundStyle(.quiet).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                    // Its file not on the server yet: how its download goes, in place of its length.
                    Text(model.download(track.id)?.short ?? formatClock(ms: track.durationMs)).font(.caption).foregroundStyle(.quiet).monospacedDigit()
                }
                .frame(minHeight: 64)
                .contentShape(Rectangle())
            }
            .buttonStyle(.pressRow)
            Menu { TrackActions(track: track, from: from) } label: {
                Image(systemName: "ellipsis").frame(width: 32, height: 44).contentShape(Rectangle())
            }
            .accessibilityLabel("More for \(track.title)")
            .foregroundStyle(.quiet)
        }
        .frame(minHeight: 64)
        .contextMenu { TrackActions(track: track, from: from) }
    }
}

/// An episode's row, as a podcast app has it: when it came out — and, away
/// from its show's page, which show — in small type; its title, bold;
/// what it is about; who is on it besides its hosts, their faces and their
/// names; and a play button that says how long it is, how much is left of
/// it or that it is played — or, while its file is not on the server yet,
/// how its download goes — beside who else has listened, with how far the
/// signed-in person has got under it once they have begun. Tapped anywhere,
/// it plays; its menu is also its long press.
struct EpisodeRow: View {
    let track: LibraryEntry
    /// What its feed says of it; none until the show's details are read.
    let detail: EpisodeDetailsEntry?
    /// How far everybody else has got in it: "Ada played".
    let others: String
    /// Who is on it to name, each opening their page: its guests.
    var people: [PeopleEntry] = []
    /// The show it is of, on a page that is not the show's.
    var show: String = ""
    let from: Page?
    let onPlay: () -> Void
    @EnvironmentObject private var model: Model

    var body: some View {
        let playing = model.playingId == track.id.hex
        let mine = model.mine(track.id)
        let whole = (mine?.durationMs ?? 0) > 0 ? mine!.durationMs : track.durationMs
        let begun = mine.map { !$0.finished && $0.positionMs > 0 && whole > 0 } ?? false
        let said = model.download(track.id)?.short ?? Spell.left(track.durationMs, mine)
        let day = formatPublished(ms: detail?.publishedMs ?? 0, nowMs: nowMs(), offsetMinutes: Int32(TimeZone.current.secondsFromGMT() / 60))
        let over = Spell.facts([show, day])
        VStack(alignment: .leading, spacing: Space.xs) {
            // The words let a touch through to the row's button behind them.
            Group {
                if !over.isEmpty {
                    Text(over).font(.caption.weight(.semibold)).foregroundStyle(.quiet).lineLimit(1)
                }
                Text(track.title).font(.headline).foregroundStyle(playing ? Color.accentText : Color.ink).lineLimit(2)
                if let about = detail?.description, !about.isEmpty {
                    Text(about).font(.subheadline).foregroundStyle(.quiet).lineLimit(3)
                }
            }
            .allowsHitTesting(false)
            if !people.isEmpty {
                PeopleLine(people: people, words: "with " + formatNames(names: people.map(\.name)))
                    .padding(.top, 2)
            }
            HStack(spacing: Space.m) {
                Button(action: onPlay) {
                    HStack(spacing: Space.xs) {
                        if playing {
                            Image(systemName: "waveform").symbolEffect(.variableColor.iterative, isActive: true)
                        } else {
                            Image(systemName: mine?.finished == true ? "arrow.counterclockwise" : "play.fill")
                        }
                        if !said.isEmpty { Text(said).monospacedDigit() }
                    }
                    .font(.caption.weight(.semibold))
                }
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .controlSize(.small)
                .tint(Color.accent)
                .accessibilityLabel(said.isEmpty ? "Play \(track.title)" : "Play \(track.title), \(said)")
                if !others.isEmpty {
                    Text(others).font(.caption).foregroundStyle(.quiet).lineLimit(1).allowsHitTesting(false)
                }
                Spacer(minLength: 0)
                Menu { TrackActions(track: track, from: from) } label: {
                    Image(systemName: "ellipsis").frame(width: 32, height: 32).contentShape(Rectangle())
                }
                .accessibilityLabel("More for \(track.title)")
                .foregroundStyle(.quiet)
            }
            .padding(.top, Space.xs)
            if begun, let m = mine {
                ProgressView(value: Double(min(m.positionMs, whole)), total: Double(whole))
                    .tint(Color.accent)
                    .accessibilityLabel("\(Spell.left(track.durationMs, m))")
                    .allowsHitTesting(false)
            }
        }
        .padding(.vertical, Space.s)
        // Tapped anywhere, it plays, and lights as the finger lands
        // (`.pressRow`): the button is behind the row, not around it, so
        // the play button, the people and the menu laid over it each take
        // their own touch, and a tap on the "…" opens the menu without
        // playing. Its play button says the same to VoiceOver, so this one
        // is hidden from it.
        .background {
            Button(action: onPlay) { Color.clear.contentShape(Rectangle()) }
                .buttonStyle(.pressRow)
                .accessibilityHidden(true)
        }
        .contextMenu { TrackActions(track: track, from: from) }
        .accessibilityElement(children: .contain)
    }
}

/// People on an episode or a show in a line: their faces, overlapping —
/// the first three — and `words`, who they are. Tapped, it opens the one
/// person's page, or offers each of several.
struct PeopleLine: View {
    let people: [PeopleEntry]
    let words: String
    var side: CGFloat = 22
    var font: Font = .footnote
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    var body: some View {
        let faces = Array(people.prefix(3))
        let line = HStack(spacing: Space.s) {
            HStack(spacing: -side / 3) {
                ForEach(faces) { p in
                    Avatar(name: p.name, picture: RowCover.url(model, p.art), side: side)
                        .overlay { Circle().stroke(Color(uiColor: .systemBackground), lineWidth: 1.5) }
                }
            }
            Text(words).font(font).foregroundStyle(.quiet).lineLimit(1)
        }
        .contentShape(Rectangle())
        if people.count == 1, let p = people.first {
            Button { open(.page(.artist(p.name))) } label: { line }
                .buttonStyle(.plain)
                .accessibilityLabel(words)
                .accessibilityHint("Opens \(p.name)'s page")
        } else {
            Menu {
                ForEach(people) { p in
                    Button { open(.page(.artist(p.name))) } label: { Label(p.name, systemImage: "person") }
                }
            } label: { line }
            .accessibilityLabel(words)
        }
    }
}

/// What a song's menu can do: play it next, put it on the queue or on a
/// playlist, go to its album, its artist or the composer of the work it is
/// a movement of, take it off the playlist it was opened from (Android's
/// `TrackMenu`).
struct TrackActions: View {
    let track: LibraryEntry
    let from: Page?
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @Environment(\.present) private var present

    var body: some View {
        Button { model.playNext(track) } label: { Label("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") }
        Button { model.addToQueue(track) } label: { Label("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") }
        Button { present(.addTo(track)) } label: { Label("Add to Playlist…", systemImage: "text.badge.plus") }
        if let a = model.album(of: track), from != .album(a.id) {
            Button { open(.page(.album(a.id))) } label: { Label("Go to Album", systemImage: "square.stack") }
        }
        // An episode goes to its show, whose title is its creator, and to nobody's page.
        if let s = model.show(of: track), from != .show(s.id) {
            Button { open(.page(.show(s.id))) } label: { Label("Go to Show", systemImage: "mic") }
        }
        if track.kind == "song", !track.creator.isEmpty, from != .artist(track.creator) {
            Button { open(.page(.artist(track.creator))) } label: { Label("Go to Artist", systemImage: "music.microphone") }
        }
        if let c = model.detail(track)?.composer, !c.isEmpty, c != track.creator, from != .artist(c) {
            Button { open(.page(.artist(c))) } label: { Label("Go to Composer", systemImage: "music.quarternote.3") }
        }
        if case let .playlist(id)? = from {
            Button(role: .destructive) { model.remove(track, from: id) } label: { Label("Remove from This Playlist", systemImage: "minus.circle") }
        }
        // An episode: played or not by hand, and each of its chapters.
        if track.kind != "song" {
            let played = model.mine(track.id)?.finished == true
            Button { model.markPlayed(track, !played) } label: {
                Label(played ? "Mark as Unplayed" : "Mark as Played", systemImage: played ? "circle" : "checkmark.circle")
            }
            let chapters = model.chapters[track.id] ?? []
            if !chapters.isEmpty, case let .show(show)? = from {
                Menu {
                    ForEach(chapters, id: \.startMs) { c in
                        Button("\(formatClock(ms: c.startMs))  \(c.title)") {
                            let list = model.rows(.show(show)) ?? [track]
                            model.play(list, at: list.firstIndex { $0.id == track.id } ?? 0, from: .show(show), positionMs: c.startMs)
                        }
                    }
                } label: { Label("Chapters", systemImage: "list.bullet") }
            }
        }
    }
}

/// A row's cover: the album's art, small.
struct RowCover: View {
    let track: LibraryEntry
    @EnvironmentObject private var model: Model

    var body: some View {
        let a = model.album(of: track)
        CoverView(seed: a?.name ?? track.title, url: Self.url(model, a?.art ?? ""), side: Space.thumb, corner: 4)
    }

    static func url(_ model: Model, _ art: String) -> String { art.isEmpty ? "" : Files.url(model.server, art) }
}

/// The number a track has on its release, or the playing mark.
struct TrackNumber: View {
    let n: Int64
    let playing: Bool

    var body: some View {
        if playing {
            Image(systemName: "waveform").symbolEffect(.variableColor.iterative, isActive: true).foregroundStyle(Color.accentText)
        } else {
            Text(n > 0 ? "\(n)" : "").foregroundStyle(.quiet).monospacedDigit()
        }
    }
}

/// A heading over a section, with an optional action at its end.
struct Heading: View {
    let title: String
    var action: String? = nil
    var onAction: () -> Void = {}

    var body: some View {
        HStack {
            Text(title).font(.title2.bold())
            Spacer()
            if let a = action { Button(a, action: onAction).buttonStyle(.borderless).foregroundStyle(Color.accentText) }
        }
        .padding(.horizontal, Space.page)
        .padding(.top, Space.xl)
        .padding(.bottom, Space.s)
    }
}

/// A row of cards that scrolls sideways, inset like the rest of the page.
struct CoverShelf<Item: Identifiable, Card: View>: View {
    let items: [Item]
    @ViewBuilder let card: (Item) -> Card

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            LazyHStack(alignment: .top, spacing: Space.l) {
                ForEach(items) { card($0) }
            }
            .padding(.horizontal, Space.page)
        }
        .scrollClipDisabled()
    }
}

/// One thing on a shelf or in a grid: its cover, its name, and a line under
/// it. One that opens a page is the zoom's source, so the page grows out of
/// its cover and shrinks back into it.
struct CoverCard: View {
    let seed: String
    let url: String
    let title: String
    let under: String
    let side: CGFloat
    var round = false
    var opens: Route? = nil
    let onTap: () -> Void
    @Environment(\.zoom) private var zoom

    var body: some View {
        Button(action: onTap) {
            VStack(alignment: round ? .center : .leading, spacing: 2) {
                Group {
                    if let r = opens { CoverView(seed: seed, url: url, side: side, round: round).zoomSource(r, zoom) }
                    else { CoverView(seed: seed, url: url, side: side, round: round) }
                }
                .frame(width: side, height: side)
                .padding(.bottom, Space.s - 2)
                Text(title).font(.subheadline.weight(.semibold)).lineLimit(1)
                if !under.isEmpty { Text(under).font(.caption).foregroundStyle(.quiet).lineLimit(1) }
            }
            .frame(width: side, alignment: round ? .center : .leading)
            .multilineTextAlignment(round ? .center : .leading)
        }
        .buttonStyle(.press)
    }
}

/// The header of a page that is about something that plays: ArkUI's
/// `PageHeader` — its cover, what it is, its name, who it is by, the
/// numbers true of the whole of it — with Play and Shuffle, Liquid Glass
/// buttons.
struct PlayHeader<Cover: View>: View {
    let kind: String
    let title: String
    var by: String? = nil
    var onBy: (() -> Void)? = nil
    let facts: String
    let canPlay: Bool
    let onPlay: () -> Void
    let onShuffle: () -> Void
    @ViewBuilder let cover: () -> Cover

    var body: some View {
        PageHeader(kind: kind, title: title, facts: facts, by: by, onBy: onBy, cover: cover) {
            GlassEffectContainer(spacing: Space.m) {
                HStack(spacing: Space.m) {
                    Button(action: onPlay) { Label("Play", systemImage: "play.fill").frame(maxWidth: .infinity) }
                        .buttonStyle(.glassProminent)
                    Button(action: onShuffle) { Label("Shuffle", systemImage: "shuffle").frame(maxWidth: .infinity) }
                        .buttonStyle(.glass)
                }
                .controlSize(.large)
                .frame(maxWidth: 320)
            }
            .disabled(!canPlay)
        }
    }
}

/// Over a tab while the server cannot be reached: which one, and that what
/// is on the phone still works — on glass.
struct Offline: View {
    let host: String

    var body: some View {
        Banner(symbol: "icloud.slash", text: "Can't reach \(host). What's on this phone still plays, and your changes sync when it's back.")
    }
}

/// Who is signed in, round, `side` across: their provider's picture once it
/// has arrived — fetched and kept as a cover is (`Covers`) — over their
/// initials on the name's own colour, as the desktop's account button draws
/// them (`arkui::account::face`) and Android's `Avatar` does. It grows
/// with the person's text size, its initials with it, up to half as large
/// again, so it stays beside the words it is with.
struct Avatar: View {
    let name: String
    let picture: String
    var side: CGFloat = 32
    @Environment(\.colorScheme) private var scheme
    @Environment(\.squares) private var squares
    @State private var image: UIImage? = nil
    @ScaledMetric(relativeTo: .body) private var grows: CGFloat = 1

    var body: some View {
        let side = self.side * min(grows, 1.5)
        let set = squares.of(dark: scheme == .dark)
        let rgb = set[Art.pick(set.count, name)]
        ZStack {
            Circle().fill(Color(uiColor: UIColor(rgb: rgb)))
            Text(Pictures.initials(name))
                .font(.system(size: side * 0.4, weight: .semibold))
                .foregroundStyle(Avatar.ink(on: rgb))
            if let i = image ?? (picture.isEmpty ? nil : Covers.shared.cached(picture, side: side)) {
                Image(uiImage: i).renderingMode(.original).resizable().scaledToFill()
            }
        }
        .frame(width: side, height: side)
        .clipShape(Circle())
        .task(id: picture) {
            image = nil
            if picture.isEmpty { return }
            image = await Covers.shared.load(picture, side: side)
        }
    }

    /// Black or white, whichever reads on `rgb`: the desktop's rule, on the same colours.
    static func ink(on rgb: UInt32) -> Color {
        let r = Double((rgb >> 16) & 0xff) / 255, g = Double((rgb >> 8) & 0xff) / 255, b = Double(rgb & 0xff) / 255
        return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.55 ? .black : .white
    }
}

/// What the app shows is not on this phone yet and is on its way from
/// `host`: said quietly, and only after a beat, so an answer that lands at
/// once never shows it.
struct Fetching: View {
    let host: String
    @State private var shown = false

    var body: some View {
        VStack(spacing: Space.l) {
            if shown {
                Text("Getting your library from \(host)…").font(.subheadline).foregroundStyle(.quiet)
                ProgressView().tint(Color.accent)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 48)
        .task {
            try? await Task.sleep(nanoseconds: 600_000_000)
            withAnimation(Springs.fade) { shown = true }
        }
    }
}

/// The account at the end of every tab's large title (ArkUI's `AccountFace`,
/// on the title's own line by `largeTitleBar`): who is listening, as their
/// face, raising the account sheet — who they are, whether the link is up,
/// Settings and the way out. Settings is the page it always was, pushed on
/// the tab once the sheet has gone. Nobody goes offline by hand: the phone
/// does by itself when the server cannot be reached, and says so.
struct AccountButton: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @State private var showing = false
    /// What the sheet's row asked for, done once the sheet has gone.
    @State private var then: Then? = nil

    private enum Then { case settings, signOut }

    var body: some View {
        let who = model.login.map(Logins.name) ?? ""
        let picture = model.login.map { Pictures.url(server: model.server, user: $0.user, picture: $0.picture) } ?? ""
        AccountFace(name: who, picture: picture) { showing = true }
            .sheet(isPresented: $showing, onDismiss: after) {
                AccountSheet(name: who, picture: picture,
                             detail: model.login.map { $0.email.isEmpty ? $0.user : $0.email } ?? "",
                             server: Probe.display(model.server),
                             linked: model.snapshot.status.map { $0.linked },
                             onSettings: { leave(.settings) },
                             onSignOut: { leave(.signOut) })
                    .presentationDetents([.medium, .large])
            }
    }

    private func leave(_ t: Then) {
        then = t
        showing = false
    }

    private func after() {
        let t = then
        then = nil
        switch t {
        case .settings?: open(.settings)
        case .signOut?: model.signOut()
        case nil: break
        }
    }
}

extension Image {
    /// A show's mark: a pulpit for sermons — drawn, in the asset catalog,
    /// as SF Symbols has none — and a microphone for a podcast.
    static func show(sermon: Bool) -> Image {
        sermon ? Image("Pulpit") : Image(systemName: "mic")
    }
}
