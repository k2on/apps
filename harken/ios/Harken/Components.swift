// The pieces harken's screens are made of: a track's row and its menu, a
// page's header with Play and Shuffle, a shelf of covers and the card that
// opens a page out of its cover, a section's heading, and the banner a
// server that cannot be reached puts over a tab — Android's `Components.kt`
// and `ui/Components.kt`. And the two things every screen can ask of the
// frame around it (Shell.swift), handed down the environment: open a page on
// this tab's stack, and raise a sheet.
import SwiftUI

/// What rises over a page.
enum Sheet: Identifiable, Equatable {
    case addTo(LibraryEntry)
    case newPlaylist(then: LibraryEntry?)
    case queue
    case devices

    var id: String {
        switch self {
        case .addTo(let t): return "add:" + t.id.hex
        case .newPlaylist(let t): return "new:" + (t?.id.hex ?? "")
        case .queue: return "queue"
        case .devices: return "devices"
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
            leading().frame(width: Space.thumb, height: Space.thumb)
            VStack(alignment: .leading, spacing: 2) {
                Text(track.title).font(.body).foregroundStyle(playing ? Color.gold : Color.primary).lineLimit(1)
                Text(under).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 0)
            // Its file not on the server yet: how its download goes, in place of its length.
            Text(model.download(track.id)?.short ?? Spell.clock(track.durationMs)).font(.caption).foregroundStyle(.secondary).monospacedDigit()
            Menu { TrackActions(track: track, from: from) } label: {
                Image(systemName: "ellipsis").frame(width: 32, height: 44).contentShape(Rectangle())
            }
            .accessibilityLabel("More for \(track.title)")
            .foregroundStyle(.secondary)
        }
        .frame(minHeight: 64)
        .contentShape(Rectangle())
        .onTapGesture(perform: onPlay)
        .contextMenu { TrackActions(track: track, from: from) }
        .accessibilityAddTraits(.isButton)
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
        if let a = model.album(of: track), from != .album(a.name) {
            Button { open(.page(.album(a.name))) } label: { Label("Go to Album", systemImage: "square.stack") }
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
            Image(systemName: "waveform").symbolEffect(.variableColor.iterative, isActive: true).foregroundStyle(Color.gold)
        } else {
            Text(n > 0 ? "\(n)" : "").foregroundStyle(.secondary).monospacedDigit()
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
            if let a = action { Button(a, action: onAction).buttonStyle(.borderless).foregroundStyle(Color.gold) }
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
                if !under.isEmpty { Text(under).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
            }
            .frame(width: side, alignment: round ? .center : .leading)
            .multilineTextAlignment(round ? .center : .leading)
        }
        .buttonStyle(.plain)
    }
}

/// The header of a page that is about something: its cover, what it is, its
/// name, who it is by (a link, when it goes somewhere), the numbers true of
/// the whole of it, and Play and Shuffle — Liquid Glass buttons.
struct PageHeader<Cover: View>: View {
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
        VStack(spacing: Space.xs) {
            cover().padding(.top, Space.s).padding(.bottom, Space.l)
            Text(kind.uppercased()).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
            Text(title).font(.title2.bold()).multilineTextAlignment(.center)
            if let by = by {
                Button(by) { onBy?() }.buttonStyle(.borderless).font(.headline).foregroundStyle(Color.gold).disabled(onBy == nil)
            }
            Text(facts).font(.footnote).foregroundStyle(.secondary)
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
            .padding(.top, Space.l)
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, Space.page)
        .padding(.bottom, Space.m)
    }
}

/// Over a tab while the server cannot be reached: which one, and that what
/// is on the phone still works — on glass.
struct Offline: View {
    let host: String

    var body: some View {
        HStack(spacing: Space.m) {
            Image(systemName: "icloud.slash").foregroundStyle(.secondary)
            Text("Can't reach \(host). What's on this phone still plays, and your changes sync when it's back.")
                .font(.footnote).foregroundStyle(.secondary)
        }
        .padding(Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassEffect(.regular, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .padding(.horizontal, Space.page)
        .padding(.vertical, Space.s)
    }
}

/// Who is signed in, round, `side` across: their provider's picture once it
/// has arrived — fetched and kept as a cover is (`Covers`) — over their
/// initials on the name's own colour, as the desktop's account button draws
/// them (`arkui::account::face`) and Android's `Avatar` does.
struct Avatar: View {
    let name: String
    let picture: String
    var side: CGFloat = 32
    @Environment(\.colorScheme) private var scheme
    @State private var image: UIImage? = nil

    var body: some View {
        let set = scheme == .dark ? Squares.dark : Squares.light
        let rgb = set[Art.pick(set.count, name)]
        ZStack {
            Circle().fill(Color(uiColor: UIColor(rgb: rgb)))
            Text(Spell.initials(name))
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
                Text("Getting your library from \(host)…").font(.subheadline).foregroundStyle(.secondary)
                ProgressView().tint(.gold)
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

/// The account at the top right of every tab, as the browser has it: who,
/// as their picture, whether the link is up, Settings, signing out. Nobody
/// goes offline by hand: the phone does by itself when the server cannot be
/// reached, and says so.
struct AccountMenu: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open

    var body: some View {
        Menu {
            if let l = model.login {
                Section(Logins.name(l) + (l.email.isEmpty ? "" : " · " + l.email)) {
                    Text(linkWords)
                }
            }
            Button { open(.settings) } label: { Label("Settings", systemImage: "gearshape") }
            Divider()
            Button(role: .destructive) { model.signOut() } label: { Label("Sign Out", systemImage: "rectangle.portrait.and.arrow.right") }
        } label: {
            Group {
                if let l = model.login {
                    Avatar(name: Logins.name(l), picture: l.picture, side: 32)
                } else {
                    Image(systemName: "person.crop.circle")
                }
            }
            .accessibilityLabel("Account")
        }
    }

    private var linkWords: String {
        guard let st = model.snapshot.status else { return "Opening…" }
        if st.linked { return "Connected to \(Probe.display(model.server))" }
        return "Can't reach \(Probe.display(model.server)) right now"
    }
}
