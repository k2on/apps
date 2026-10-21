// The phone's one peer, and nothing SwiftUI: harken's peer — the Rust
// kernel through the bridge (`harken/ffi`), which drives its own link and
// keeps its own views — and the lists the screens draw, *spliced*, never
// re-read: `library`, `playlists` and the open `playlist` are the kernel's
// maintained views, typed by the generated stubs (`Gen/Harken.swift`); when
// the peer says anything moved, each is asked what it did and its patches
// are applied to the list the screen was given — read again only on a
// reset, or when the selection moves. What the screens see goes to `Model`
// (Model.swift) as a `Snapshot`, on the main queue, when something in it
// moved. The twin of `harken/android/.../Library.kt`.
import Foundation

/// Where the app points: the server (`http://host:port`; the socket is
/// derived from it), whether to work alone, and the login, if somebody
/// signed in — kept whole, so a restart reuses it.
struct Prefs: Equatable {
    var server: String
    var workAlone: Bool
    var login: Login?

    var user: String? { login?.user }

    static let defaults = Prefs(server: "http://127.0.0.1:8787", workAlone: false, login: nil)
}

/// Everything a screen reads, at one moment.
struct Snapshot: Equatable {
    var library = [Track]()
    var playlists = [Playlist]()
    var selected: Id? = nil
    var items = [Track]()
    var status: Status? = nil
    /// Every change this phone made while it has been running, newest first.
    var changes = [Authored]()
    /// Where each of them stands, read again whenever anything moved.
    var standings = [String: Standing]()
    /// Which entry made a playlist, and which last touched a (playlist,
    /// item) pair: so a row can say it is not synced yet, or why it was not saved.
    var madeBy = [Id: String]()
    var touchedBy = [Pair: String]()

    struct Pair: Hashable {
        let playlist: Id
        let media: Id
    }

    /// What to write under a playlist: nothing once it is in the log.
    func caption(forPlaylist id: Id) -> String? { madeBy[id].flatMap { standings[$0] }.flatMap(Phone.caption) }

    /// What to write under an item of a playlist.
    func caption(for track: Track, on playlist: Id) -> String? {
        touchedBy[Pair(playlist: playlist, media: track.id)].flatMap { standings[$0] }.flatMap(Phone.caption)
    }

    func caption(for change: Authored) -> String { standings[change.id].flatMap(Phone.caption) ?? "saved" }
}

/// What the peer says when anything moved: the bridge's queue is told.
private final class Told: Listener, @unchecked Sendable {
    private let then: () -> Void
    init(_ then: @escaping () -> Void) { self.then = then }
    func changed() { then() }
}

final class Bridge {
    private let queue = DispatchQueue(label: "harken-session")
    private let root: URL
    /// Told on the main queue whenever the snapshot moved, and of anything worth a line.
    private let publish: (Snapshot) -> Void
    private let notice: (String) -> Void

    // On `queue` only.
    private var harken: Harken? = nil
    private var libraryView: View<LibraryEntry>? = nil
    private var playlistsView: View<Playlist>? = nil
    private var itemsView: View<LibraryEntry>? = nil
    private var snap = Snapshot()
    private var shown: Snapshot? = nil
    private var deniedSaid = false

    /// The replicas under `root`.
    init(root: URL, publish: @escaping (Snapshot) -> Void, notice: @escaping (String) -> Void) {
        self.root = root
        self.publish = publish
        self.notice = notice
    }

    /// Where a replica lives: one directory per server (or alone), not per
    /// person — the log is the server's and every person reads it — so that
    /// work done signed out is still here when somebody signs in.
    private func directory(_ p: Prefs) -> String {
        let place = p.workAlone ? "alone" : "server-" + String(p.server.unicodeScalars.map { CharacterSet.alphanumerics.contains($0) ? Character($0) : "_" })
        let dir = root.appendingPathComponent("harken").appendingPathComponent(place).appendingPathComponent("replica")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.path
    }

    private func say(_ s: String) { DispatchQueue.main.async { self.notice(s) } }

    /// Open the peer the prefs describe and read the three views.
    func start(_ prefs: Prefs) { queue.async { self.open(prefs) } }

    private func open(_ prefs: Prefs) {
        let device = Device(canPlay: false, id: "", kind: "phone", name: "iPhone", user: "")
        let peer: Peer
        do {
            peer = try harkenOpen(dir: directory(prefs), login: prefs.workAlone ? nil : prefs.login, device: device.json)
        } catch {
            say("could not open the replica: \(why(error))")
            return
        }
        if prefs.workAlone {
            peer.alone()
        } else {
            peer.connect(url: socketUrl(server: prefs.server))
        }
        let h = Harken(peer: peer)
        harken = h
        peer.setListener(listener: Told { [weak self] in self?.queue.async { self?.tick() } })
        playlistsView = try? h.playlists()
        snap.playlists = playlistsView?.rows() ?? []
        reselect(h, snap.selected)
        snap.status = peer.status()
        show()
    }

    // The selection moved, or has to: the first playlist when the one held
    // went away. The library is read against it and the playlist view is of
    // it, so both are opened again — a read, once per selection.
    private func reselect(_ h: Harken, _ want: Id?) {
        let sel = want.flatMap { id in snap.playlists.contains { $0.id == id } ? id : nil } ?? snap.playlists.first?.id
        snap.selected = sel
        libraryView = try? h.library(Library(playlistId: sel ?? Phone.noPlaylist))
        snap.library = (libraryView?.rows() ?? []).map(Track.init)
        itemsView = sel.flatMap { try? h.playlist(PlaylistInput(playlistId: $0)) }
        snap.items = (itemsView?.rows() ?? []).map(Track.init)
    }

    // Each view asked what it did, and its list spliced by its patches, or
    // read again on a reset.
    private func splice(_ h: Harken) {
        let before = snap.selected
        apply(playlistsView, &snap.playlists) { $0 }
        if let b = before, !snap.playlists.contains(where: { $0.id == b }) { return reselect(h, nil) }
        if before == nil, let first = snap.playlists.first { return reselect(h, first.id) }
        apply(libraryView, &snap.library, Track.init)
        apply(itemsView, &snap.items, Track.init)
    }

    private func apply<E, T>(_ v: View<E>?, _ rows: inout [T], _ of: (E) -> T) {
        guard let view = v else { return }
        switch view.take() {
        case .unchanged: break
        case .patched(let changes):
            rows.splice(changes.map { c -> Change<T> in
                switch c {
                case let .insert(at, row): return .insert(at: at, row: of(row))
                case let .remove(at): return .remove(at: at)
                case let .update(at, row): return .update(at: at, row: of(row))
                }
            })
        case .reset(let all): rows = all.map(of)
        }
    }

    /// Whatever moved: the views, the status, where each change stands, a rejection's reason.
    private func tick() {
        guard let h = harken else { return }
        splice(h)
        let st = h.peer.status()
        snap.status = st
        if let d = st.denied, !deniedSaid {
            deniedSaid = true
            say("turned away: \(d)")
        }
        var now = [String: Standing]()
        for c in snap.changes { now[c.id] = h.peer.standing(id: c.id) }
        snap.standings = now
        if let last = h.peer.takeRejections().last { say("not saved: \(last.why)") }
        show()
    }

    private func show() {
        guard shown != snap else { return }
        shown = snap
        let s = snap
        DispatchQueue.main.async { self.publish(s) }
    }

    /// Something to do with the peer, on its queue; what moved spliced.
    func onPeer(_ f: @escaping (Harken) -> Void) {
        queue.async {
            guard let h = self.harken else { return }
            f(h)
            self.tick()
        }
    }

    /// A read a screen needs at once, on the peer's queue.
    func now<T>(_ fallback: T, _ f: (Harken) -> T) -> T {
        queue.sync { harken.map(f) ?? fallback }
    }

    // -- what the screens ask -----------------------------------------------------

    func select(_ id: Id) {
        queue.async {
            guard let h = self.harken else { return }
            self.reselect(h, id)
            self.show()
        }
    }

    /// Which of the person's playlists a track is on, for the menu's ticks.
    func playlistsOf(_ track: Track) -> Set<Id> { now([]) { Set(Phone.playlistsOf($0, media: track.id).map { $0.id }) } }

    /// The `create_playlist` input's checks on a name being typed; nil when it would pass.
    func nameProblem(_ name: String) -> String? { now(nil) { Phone.nameProblem($0, name) } }

    func createPlaylist(_ name: String) { onPeer { h in self.created(h, Phone.createPlaylist(h, name), name.trimmingCharacters(in: .whitespaces)) } }

    // A playlist made: which entry made it, found by the row it added.
    private func created(_ h: Harken, _ w: Wrote, _ name: String) {
        let before = Set(snap.playlists.map { $0.id })
        let entry = record(w, "make the playlist \(name)", nil)
        splice(h)
        if let e = entry { for pl in snap.playlists where !before.contains(pl.id) { snap.madeBy[pl.id] = e } }
    }

    /// Put a track on a playlist — the given one, the selected one, or for
    /// somebody with none, "Favorites", made now.
    func add(_ track: Track, to playlist: Id? = nil) {
        onPeer { h in
            var target = playlist ?? self.snap.selected
            if target == nil {
                self.created(h, Phone.createPlaylist(h, Phone.defaultPlaylist), Phone.defaultPlaylist)
                target = self.snap.playlists.last?.id
            }
            guard let t = target else { return self.say("no playlist to add to") }
            let name = self.snap.playlists.first { $0.id == t }?.name ?? Phone.defaultPlaylist
            _ = self.record(Phone.addToPlaylist(h, t, track.id), "add \(track.title) to \(name)", Snapshot.Pair(playlist: t, media: track.id))
        }
    }

    func remove(_ track: Track, from playlist: Id) {
        onPeer { h in
            let name = self.snap.playlists.first { $0.id == playlist }?.name ?? "a playlist"
            _ = self.record(Phone.removeFromPlaylist(h, playlist, track.id), "take \(track.title) off \(name)", Snapshot.Pair(playlist: playlist, media: track.id))
        }
    }

    /// On the playlist it is not on, or off the one it is on.
    func toggle(_ track: Track, on playlist: Id) {
        if playlistsOf(track).contains(playlist) { remove(track, from: playlist) } else { add(track, to: playlist) }
    }

    // A change this phone made, kept so its standing is read whenever anything moves.
    private func record(_ w: Wrote, _ what: String, _ key: Snapshot.Pair?) -> String? {
        switch w {
        case .refused(let why):
            say("refused: \(why)")
            return nil
        case .entry(let id):
            snap.changes.insert(Authored(id: id, what: what), at: 0)
            if snap.changes.count > 100 { snap.changes.removeLast(snap.changes.count - 100) }
            if let k = key { snap.touchedBy[k] = id }
            if let h = harken { snap.standings[id] = h.peer.standing(id: id) }
            return id
        }
    }

    /// Sign in as `login`: everything done so far as nobody becomes theirs
    /// and is pushed; whoever is signed in is every view's reader, so each
    /// is read again.
    func signIn(_ login: Login) {
        onPeer { h in
            h.peer.login(login: login)
            self.deniedSaid = false
            self.playlistsView = try? h.playlists()
            self.snap.playlists = self.playlistsView?.rows() ?? []
            self.reselect(h, self.snap.selected)
        }
    }

    /// Sign out: the link stopped; what that person left pending stays theirs.
    func signOut() { onPeer { $0.peer.logout() } }

    /// Reopen under new prefs: the old peer let go first.
    func reopen(_ prefs: Prefs) {
        queue.async {
            self.close()
            // What this phone made, and where it stands, outlive the peer.
            let kept = self.snap
            self.snap = Snapshot()
            self.snap.changes = kept.changes
            self.snap.madeBy = kept.madeBy
            self.snap.touchedBy = kept.touchedBy
            self.open(prefs)
        }
    }

    // On `queue`: the views and the peer let go; the kernel writes as it goes.
    private func close() {
        harken?.peer.setListener(listener: nil)
        libraryView = nil
        playlistsView = nil
        itemsView = nil
        harken = nil
    }
}
