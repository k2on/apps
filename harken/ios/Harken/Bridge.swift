// The phone's one peer, and nothing SwiftUI: a Swift `Peer` over
// `harken.ark` on a serial queue of its own (`ark-peer`) — the pump every
// fifty milliseconds, every mutation, every read a screen asks for — and the
// lists the screens draw, *spliced*, never re-read: `library`, `playlists`
// and the open `playlist` are maintained views (`ClientView`), each handed
// every `Changes` the peer reports, and the patches they answer with are
// applied to the list the screen was given; only a `reset` — a rebase that
// replaced the store, a sign-in, a selection that moved — reads one again.
// What the screens see goes to `Model` (Model.swift) as a `Snapshot`, on
// the main queue, when something in it moved. The twin of the peer half of
// `harken/android/.../Model.kt`; type-checked on Linux (README.md).
import ArkDB
import ArkDBClient
import Foundation

/// Where the app points: the server (`http://host:port`; the socket is
/// derived from it), whether to work alone, and the login, if somebody
/// signed in — kept whole, so a restart reuses it.
struct Prefs: Equatable {
    var server: String
    var workAlone: Bool
    var login: Login?

    var user: String? { login?.user.id }

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
    /// Where each of them stands, read again on every pump.
    var standings = [Id: Standing]()
    /// Which entry made a playlist, and which last touched a (playlist,
    /// item) pair: so a row can say it is not synced yet, or why it was not saved.
    var madeBy = [Id: Id]()
    var touchedBy = [Pair: Id]()

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

final class Bridge {
    private let queue = DispatchQueue(label: "ark-peer")
    private let domain: Domain
    private let root: URL
    /// Told on the main queue whenever the snapshot moved, and of anything worth a line.
    private let publish: (Snapshot) -> Void
    private let notice: (String) -> Void

    // On `queue` only.
    private var peer: Peer? = nil
    private var libraryView: ClientView? = nil
    private var playlistsView: ClientView? = nil
    private var itemsView: ClientView? = nil
    private var snap = Snapshot()
    private var shown: Snapshot? = nil
    private var verdictsSeen = 0
    private var timer: DispatchSourceTimer? = nil

    /// `harken.ark` as the domain crate writes it, the replicas under `root`.
    init(module: [UInt8], root: URL, publish: @escaping (Snapshot) -> Void, notice: @escaping (String) -> Void) throws {
        domain = try Domain.fromBytes(module)
        self.root = root
        self.publish = publish
        self.notice = notice
    }

    /// Where a replica lives: one directory per server (or alone), not per
    /// person — the log is the server's and every person reads it — so that
    /// work done signed out is still here when somebody signs in.
    private func directory(_ p: Prefs) -> String {
        let place = p.workAlone ? "alone" : "server-" + String(p.server.unicodeScalars.map { CharacterSet.alphanumerics.contains($0) ? Character($0) : "_" })
        return root.appendingPathComponent("ark").appendingPathComponent(place).appendingPathComponent("replica").path
    }

    private func say(_ s: String) { DispatchQueue.main.async { self.notice(s) } }

    /// Open the peer the prefs describe, hydrate the three views, and start pumping.
    func start(_ prefs: Prefs) {
        queue.async {
            self.open(prefs)
            let t = DispatchSource.makeTimerSource(queue: self.queue)
            t.schedule(deadline: .now(), repeating: .milliseconds(50))
            t.setEventHandler { [weak self] in self?.tick() }
            t.resume()
            self.timer = t
        }
    }

    private func open(_ prefs: Prefs) {
        let opts: Options
        if prefs.workAlone {
            opts = Options.alone(prefs.user ?? "me")
        } else if let l = prefs.login {
            opts = Options.server(l.user.id, l.session, l.token)
        } else {
            opts = Options.signedOut()
        }
        let p: Peer
        do {
            p = try Peer.open(domain, DirStorage(directory(prefs)), opts)
        } catch {
            say("could not open the replica: \(error)")
            return
        }
        if let l = prefs.login { p.setRoles(l.user.roles) }
        if !prefs.workAlone { p.connect(Auth.socketUrl(prefs.server)) }
        peer = p
        _ = p.takeChanges()
        playlistsView = hold(p, "playlists", [:])
        snap.playlists = (playlistsView?.rows ?? []).compactMap(Playlist.of)
        reselect(p, snap.selected)
        snap.status = p.status()
        show()
    }

    // A view, or none when its middleware refuses (a playlist that is not
    // this person's): a screen draws an empty list, not an error.
    private func hold(_ p: Peer, _ name: String, _ args: [String: Value]) -> ClientView? { try? p.view(name, args) }

    // The selection moved, or has to: the first playlist when the one held
    // went away. The library is read against it and the playlist view is of
    // it, so both are opened again — a hydrate, once per selection.
    private func reselect(_ p: Peer, _ want: Id?) {
        let sel = want.flatMap { id in snap.playlists.contains { $0.id == id } ? id : nil } ?? snap.playlists.first?.id
        snap.selected = sel
        libraryView = hold(p, "library", Phone.libraryArgs(sel))
        snap.library = (libraryView?.rows ?? []).compactMap(Track.of)
        itemsView = sel.flatMap { hold(p, "playlist", Phone.playlistArgs($0)) }
        snap.items = (itemsView?.rows ?? []).compactMap(Track.of)
    }

    // What moved, handed to every view; each list spliced by the patches it
    // answers with, or read again on a reset.
    private func splice(_ p: Peer) {
        let ch = p.takeChanges()
        if case .applied(let cs) = ch, cs.isEmpty { return }
        let before = snap.selected
        apply(p, playlistsView, ch, &snap.playlists, Playlist.of)
        if let b = before, !snap.playlists.contains(where: { $0.id == b }) {
            reselect(p, nil)
            return
        }
        if before == nil, let first = snap.playlists.first {
            reselect(p, first.id)
            return
        }
        apply(p, libraryView, ch, &snap.library, Track.of)
        apply(p, itemsView, ch, &snap.items, Track.of)
    }

    private func apply<T>(_ p: Peer, _ v: ClientView?, _ ch: Changes, _ rows: inout [T], _ of: (Value) -> T?) {
        guard let view = v else { return }
        do {
            switch try view.update(p, ch) {
            case .unchanged: break
            case .patched(let ps):
                // A row that does not read as the type is a module that is
                // not harken's: take the view's rows whole rather than
                // splice around a hole.
                let readable = ps.allSatisfy { patch in
                    switch patch {
                    case .insert(_, let n), .update(_, let n): return of(n) != nil
                    case .remove: return true
                    }
                }
                if readable {
                    ClientView.splice(&rows, ps) { of($0)! }
                } else {
                    rows = view.rows.compactMap(of)
                }
            case .reset: rows = view.rows.compactMap(of)
            }
        } catch {
            say("\(view.name): \(error)")
        }
    }

    /// Every pump: what moved, the status, where each change stands, a new verdict's reason.
    private func tick() {
        guard let p = peer else { return }
        let pumped = p.pump()
        if let d = pumped.denied { say("turned away: \(d)") }
        splice(p)
        snap.status = p.status()
        var now = [Id: Standing]()
        for c in snap.changes { now[c.id] = p.standing(c.id) }
        snap.standings = now
        let rejected = p.takeRejections()
        if let last = rejected.last { say("not saved: \(last.reason)") }
        if p.agreed.count > verdictsSeen, let (n, ok) = p.agreed.last {
            verdictsSeen = p.agreed.count
            say("at \(n): \(ok ? "agrees" : "DIVERGED")")
        }
        show()
    }

    private func show() {
        guard shown != snap else { return }
        shown = snap
        let s = snap
        DispatchQueue.main.async { self.publish(s) }
    }

    /// Something to do with the peer, on its queue; its error said, and what moved spliced.
    func onPeer(_ f: @escaping (Peer) throws -> Void) {
        queue.async {
            guard let p = self.peer else { return }
            do {
                try f(p)
            } catch {
                self.say("\(error)")
            }
            self.splice(p)
            self.show()
        }
    }

    /// A read a screen needs at once, on the peer's queue.
    func now<T>(_ fallback: T, _ f: (Peer) -> T) -> T {
        queue.sync { peer.map(f) ?? fallback }
    }

    // -- what the screens ask -----------------------------------------------------

    func select(_ id: Id) {
        queue.async {
            guard let p = self.peer else { return }
            self.reselect(p, id)
            self.show()
        }
    }

    /// Which of the person's playlists a track is on, for the menu's ticks.
    func playlistsOf(_ track: Track) -> Set<Id> { now([]) { Set(Phone.playlistsOf($0, media: track.id).map { $0.id }) } }

    /// The `create_playlist` input's checks on a name being typed; nil when it would pass.
    func nameProblem(_ name: String) -> String? { now(nil) { Phone.nameProblem($0, name) } }

    func createPlaylist(_ name: String) { onPeer { p in self.created(p, Phone.createPlaylist(p, name), name.trimmingCharacters(in: .whitespaces)) } }

    // A playlist made: which entry made it, found by the row it added.
    private func created(_ p: Peer, _ w: Wrote, _ name: String) {
        let before = Set(snap.playlists.map { $0.id })
        let entry = record(w, "make the playlist \(name)", nil)
        splice(p)
        if let e = entry { for pl in snap.playlists where !before.contains(pl.id) { snap.madeBy[pl.id] = e } }
    }

    /// Put a track on a playlist — the given one, the selected one, or for
    /// somebody with none, "Favorites", made now.
    func add(_ track: Track, to playlist: Id? = nil) {
        onPeer { p in
            var target = playlist ?? self.snap.selected
            if target == nil {
                self.created(p, Phone.createPlaylist(p, Phone.defaultPlaylist), Phone.defaultPlaylist)
                target = self.snap.playlists.last?.id
            }
            guard let t = target else { return self.say("no playlist to add to") }
            let name = self.snap.playlists.first { $0.id == t }?.name ?? Phone.defaultPlaylist
            _ = self.record(Phone.addToPlaylist(p, t, track.id), "add \(track.title) to \(name)", Snapshot.Pair(playlist: t, media: track.id))
        }
    }

    func remove(_ track: Track, from playlist: Id) {
        onPeer { p in
            let name = self.snap.playlists.first { $0.id == playlist }?.name ?? "a playlist"
            _ = self.record(Phone.removeFromPlaylist(p, playlist, track.id), "take \(track.title) off \(name)", Snapshot.Pair(playlist: playlist, media: track.id))
        }
    }

    /// On the playlist it is not on, or off the one it is on.
    func toggle(_ track: Track, on playlist: Id) {
        if playlistsOf(track).contains(playlist) { remove(track, from: playlist) } else { add(track, to: playlist) }
    }

    // A change this phone made, kept so its standing is read on every pump.
    private func record(_ w: Wrote, _ what: String, _ key: Snapshot.Pair?) -> Id? {
        switch w {
        case .refused(let why):
            say("refused: \(why)")
            return nil
        case .entry(let id):
            snap.changes.insert(Authored(id: id, what: what), at: 0)
            if snap.changes.count > 100 { snap.changes.removeLast(snap.changes.count - 100) }
            if let k = key { snap.touchedBy[k] = id }
            if let p = peer { snap.standings[id] = p.standing(id) }
            return id
        }
    }

    /// Sign in as `login`: everything done so far as nobody becomes theirs
    /// and is pushed. Whoever is signed in is every view's input, so each is
    /// read again — a view only notices another `Ctx` when something moves.
    func signIn(_ login: Login) {
        onPeer { p in
            try p.signIn(login.user.id, login.session, login.token)
            p.setRoles(login.user.roles)
            self.rehydrate(p)
        }
    }

    /// Sign out: the token forgotten and the link stopped; what that person left pending stays theirs.
    func signOut() { onPeer { $0.signOut() } }

    private func rehydrate(_ p: Peer) {
        _ = p.takeChanges()
        try? playlistsView?.rehydrate(p)
        snap.playlists = (playlistsView?.rows ?? []).compactMap(Playlist.of)
        reselect(p, snap.selected)
    }

    /// Reopen under new prefs: the old peer closed and written down first.
    func reopen(_ prefs: Prefs) {
        queue.async {
            try? self.peer?.close()
            self.peer = nil
            // What this phone made, and where it stands, outlive the peer.
            let kept = self.snap
            self.snap = Snapshot()
            self.snap.changes = kept.changes
            self.snap.madeBy = kept.madeBy
            self.snap.touchedBy = kept.touchedBy
            self.open(prefs)
        }
    }

    /// Ask the authority whether it agrees: alone, at once; linked, on a later pump.
    func verify() { onPeer { $0.verify() } }

    /// Stop pumping and write down what is not yet written.
    func close() {
        queue.sync {
            timer?.cancel()
            timer = nil
            try? peer?.close()
            peer = nil
        }
    }
}
