// The phone's session, and nothing of Apple's but Foundation: harken's peer
// — the Rust kernel through the bridge (`harken/ffi`), which drives its own
// link and keeps its own views — opened over the replica for one server and
// one login, and the account's listening session reconciled against this
// phone's player. The twin of Android's `onboarding/ArkSession.kt`,
// `Library.kt` and `Listen.kt` in one.
//
// **Everything runs on one queue** (`harken-session`), in order: every write,
// every read, and the drain after each change the peer reports (`Listener`)
// — never on a clock, so a library at rest runs nothing at all. A burst of
// tells is one drain.
//
// **The lists are spliced, not re-read.** The songs, the playlists, the
// albums, the artists, the songs' details and the last play are the
// kernel's maintained views (§1.10), typed by the generated stubs
// (`View<T>`), read once — Home's three first, published, and the rest a
// turn later — and after every change each is asked what it did and its
// patches spliced into the list the screens hold; only a reset reads one
// again. A page — an album, an artist, a playlist — is a view too, held
// while a screen shows it ([hold]) and let go when the last one leaves.
//
// **The listening session is the reconciler's** (`Player`, harken's one
// reconciler, S3b): each drain steps it, which makes this phone's player
// agree with the session and says what happened to it, and its view of what
// plays ([Heard]) is published with the lists. A press from anywhere — a
// button, a row, the lock screen — is [press], one way in; refused as
// "not part of the session", the caller does it to the player itself.
//
// What the screens see goes to the main queue as a [Snapshot] after a drain
// that moved something.
import Foundation

/// Everything a screen reads, at one moment.
struct Snapshot {
    /// Home's lists have been read once; the rest after them.
    var loaded = false
    var complete = false
    /// Every song, in the order it was added.
    var tracks = [LibraryEntry]()
    var playlists = [Playlist]()
    var albums = [AlbumsEntry]()
    var artists = [ArtistsEntry]()
    var details = [Id: TrackDetailsEntry]()
    /// The newest play in the log, this person's.
    var lastPlayed: Play? = nil
    /// The pages held, each its rows once read.
    var pages = [Page: [LibraryEntry]]()
    var status: Status? = nil
    var heard = Heard()
    /// How many changes the peer has reported: what the diagnostics read again on.
    var moved = 0
}

/// What the peer says when anything moved: the session's queue is told.
private final class Told: Listener, @unchecked Sendable {
    private let then: () -> Void
    init(_ then: @escaping () -> Void) { self.then = then }
    func changed() { then() }
}

/// A view and the list it is spliced into; on the session's queue only.
private final class Held<E> {
    let open: (Harken) throws -> View<E>?
    var view: View<E>? = nil
    var rows = [E]()
    init(_ open: @escaping (Harken) throws -> View<E>?) { self.open = open }

    func hydrate(_ h: Harken) {
        view = (try? open(h)) ?? nil
        rows = view?.rows() ?? []
    }

    /// What it did since last asked, spliced; true when it moved.
    func update() -> Bool {
        guard let v = view else { return false }
        switch v.take() {
        case .unchanged: return false
        case .patched(let changes):
            rows.splice(changes)
            return true
        case .reset(let all):
            rows = all
            return true
        }
    }
}

final class Bridge {
    let server: String
    let login: Login
    private let queue = DispatchQueue(label: "harken-session")
    private let root: URL
    private let name: String
    private let sound: SoundCore
    /// Told on the main queue.
    private let publish: (Snapshot) -> Void
    private let notice: (String) -> Void
    private let denied: (String) -> Void
    private let failed: (String) -> Void

    // On `queue` only.
    private var harken: Harken? = nil
    private var player: Player? = nil
    private var closed = false
    private var draining = false
    private var deniedSaid = false
    private var snap = Snapshot()
    private var changes = [Authored]()
    private let library = Held<LibraryEntry> { try Phone.library($0) }
    private let playlists = Held<Playlist> { try $0.playlists() }
    private let albums = Held<AlbumsEntry> { try $0.albums() }
    private let artists = Held<ArtistsEntry> { try $0.artists() }
    private let details = Held<TrackDetailsEntry> { try $0.trackDetails() }
    private let played = Held<Play> { try $0.lastPlayed() }
    private var pages = [Page: (held: Held<LibraryEntry>, refs: Int)]()

    /// The session for `login` at `server`, its replica under `root`; this
    /// phone named `name` in the listening session, playing through `sound`.
    init(server: String, login: Login, root: URL, name: String, sound: SoundCore,
         publish: @escaping (Snapshot) -> Void, notice: @escaping (String) -> Void,
         denied: @escaping (String) -> Void, failed: @escaping (String) -> Void) {
        self.server = server
        self.login = login
        self.root = root
        self.name = name
        self.sound = sound
        self.publish = publish
        self.notice = notice
        self.denied = denied
        self.failed = failed
        snap.heard.me = login.session
        queue.async { self.open() }
    }

    /// The replica's directory for `server` under `root`: one per server, not
    /// per person — the log is the server's and every person reads it, and a
    /// person signing in again after a denial finds their pending intents.
    static func directory(_ root: URL, _ server: String) -> URL {
        let place = "server-" + String(server.unicodeScalars.map { CharacterSet.alphanumerics.contains($0) && $0.isASCII ? Character($0) : "_" })
        return root.appendingPathComponent("harken").appendingPathComponent(place).appendingPathComponent("replica")
    }

    private func main(_ f: @escaping () -> Void) { DispatchQueue.main.async(execute: f) }

    // On `queue`: the peer opened and dialling, Home's views read and shown, then the rest.
    private func open() {
        let dir = Bridge.directory(root, server)
        let h: Harken
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            // A phone, which can play, named as its owner named it; its id is its
            // login's session — the server fills in an empty one — so a device is a login.
            let device = Device(canPlay: true, id: "", kind: "phone", name: name, user: "")
            let peer = try harkenOpen(dir: dir.path, login: login, device: device.json)
            peer.connect(url: socketUrl(server: server))
            h = Harken(peer: peer)
        } catch {
            closed = true
            let w = why(error)
            main { self.failed(w) }
            return
        }
        harken = h
        // What it counted is kept beside the replica.
        let p = Player(peer: h.peer, me: login.session, dir: dir.path)
        p.attach(sound: sound)
        player = p
        h.peer.setListener(listener: Told { [weak self] in self?.drainSoon() })
        library.hydrate(h)
        playlists.hydrate(h)
        albums.hydrate(h)
        snap.tracks = library.rows
        snap.playlists = playlists.rows
        snap.albums = albums.rows
        snap.loaded = true
        drain()
        // A later turn, so nothing that published waits on it.
        queue.async {
            guard let h = self.harken else { return }
            self.artists.hydrate(h)
            self.details.hydrate(h)
            self.played.hydrate(h)
            self.snap.artists = self.artists.rows
            self.snap.details = Bridge.byMedia(self.details.rows)
            self.snap.lastPlayed = self.played.rows.first
            self.snap.complete = true
            self.show()
        }
    }

    private static func byMedia(_ rows: [TrackDetailsEntry]) -> [Id: TrackDetailsEntry] {
        var m = [Id: TrackDetailsEntry](minimumCapacity: rows.count)
        for r in rows { m[r.mediaId] = r }
        return m
    }

    private func drainSoon() {
        queue.async {
            if self.draining { return }
            self.draining = true
            self.queue.async { self.drain() }
        }
    }

    // On `queue`: the views spliced, a rejection said once, a denial said
    // once, the reconciler stepped, and what moved shown.
    private func drain() {
        draining = false
        guard !closed, let h = harken else { return }
        snap.moved += 1
        splice()
        let st = h.peer.status()
        snap.status = st
        if let d = st.denied, !deniedSaid {
            deniedSaid = true
            main { self.denied(d) }
        }
        if let last = h.peer.takeRejections().last {
            let w = last.why
            main { self.notice("Not saved: \(w)") }
        }
        step()
        show()
    }

    private func splice() {
        if library.update() { snap.tracks = library.rows }
        if playlists.update() { snap.playlists = playlists.rows }
        if albums.update() { snap.albums = albums.rows }
        if artists.update() { snap.artists = artists.rows }
        if details.update() { snap.details = Bridge.byMedia(details.rows) }
        if played.update() { snap.lastPlayed = played.rows.first }
        for (page, p) in pages where p.held.update() { snap.pages[page] = p.held.rows }
    }

    private func step() {
        guard let p = player else { return }
        snap.heard = Heard.of(p.step())
    }

    private func show() {
        let s = snap
        main { self.publish(s) }
    }

    /// Something to do with the peer, on its queue; then a drain, so what it moved is shown.
    private func onPeer(_ f: @escaping (Harken) -> Void) {
        queue.async {
            guard !self.closed, let h = self.harken else { return }
            f(h)
            self.drain()
        }
    }

    /// A read on the peer's queue, answered on the main queue.
    private func ask<T>(_ fallback: T, _ f: @escaping (Harken) -> T, _ done: @escaping (T) -> Void) {
        queue.async {
            let v = self.harken.map(f) ?? fallback
            self.main { done(v) }
        }
    }

    // MARK: the listening session

    /// A press — a transition while this phone is part of the session —
    /// previewed on the session's queue, so what is published next already
    /// shows it; then `done` on the main queue with the refusal, if any.
    /// Unlinked it is refused ([notPart]), and the caller does it to the player itself.
    func press(_ p: Press, done: @escaping (String?) -> Void = { _ in }) {
        queue.async {
            guard let player = self.player else { return self.main { done(notPart) } }
            let w = player.press(press: p.json)
            self.step()
            self.show()
            self.main { done(w) }
        }
    }

    /// The player said something happened to it: reconcile.
    func poke() {
        queue.async {
            guard !self.closed, self.player != nil else { return }
            self.step()
            self.show()
        }
    }

    // MARK: pages

    /// A page's tracks, kept up to date while it is held; [release] it when the screen goes.
    func hold(_ page: Page) {
        if page == .songs { return }
        queue.async {
            if var p = self.pages[page] {
                p.refs += 1
                self.pages[page] = p
                return
            }
            let held = Held<LibraryEntry> { try Phone.view($0, page) }
            self.pages[page] = (held, 1)
            if let h = self.harken {
                held.hydrate(h)
                self.snap.pages[page] = held.rows
                self.show()
            }
        }
    }

    func release(_ page: Page) {
        queue.async {
            guard var p = self.pages[page] else { return }
            p.refs -= 1
            if p.refs > 0 {
                self.pages[page] = p
                return
            }
            self.pages[page] = nil
            self.snap.pages[page] = nil
        }
    }

    /// A page's tracks once, for playing it from somewhere that does not show it (Home's last played).
    func read(_ page: Page, done: @escaping ([LibraryEntry]) -> Void) {
        ask([], { h in page == .songs ? self.library.rows : Phone.once { try Phone.view(h, page)! } }, done)
    }

    /// The library's songs matching `needle`, by the domain's `search`.
    func search(_ needle: String, done: @escaping ([LibraryEntry]) -> Void) { ask([], { Phone.search($0, needle) }, done) }

    /// Which of the person's playlists `track` is on.
    func playlistsOf(_ track: LibraryEntry, done: @escaping (Set<Id>) -> Void) {
        ask([], { Set(Phone.playlistsOf($0, media: track.id).map { $0.id }) }, done)
    }

    /// What `create_playlist`'s own checks say of a name as it is typed; nil when it would pass.
    func nameProblem(_ name: String, done: @escaping (String?) -> Void) { ask(nil, { Phone.nameProblem($0, name) }, done) }

    // MARK: writing

    private func record(_ w: Wrote, _ what: String, _ refused: String) {
        switch w {
        case .refused(let why): main { self.notice("\(refused): \(why)") }
        case .entry(let id):
            changes.insert(Authored(id: id, what: what), at: 0)
            if changes.count > 100 { changes.removeLast(changes.count - 100) }
        }
    }

    /// Make a playlist, and put `then` on it once it is there.
    func createPlaylist(_ name: String, then: LibraryEntry? = nil) {
        onPeer { h in
            let before = Set(self.playlists.rows.map { $0.id })
            self.record(Phone.createPlaylist(h, name), "make the playlist \(name.trimmingCharacters(in: .whitespaces))", "Couldn't make it")
            if self.playlists.update() { self.snap.playlists = self.playlists.rows }
            if let t = then, let made = self.playlists.rows.first(where: { !before.contains($0.id) }) {
                self.record(Phone.addToPlaylist(h, made.id, t.id), "add \(t.title) to \(made.name)", "Couldn't add it")
            }
        }
    }

    func add(_ track: LibraryEntry, to playlist: Playlist) {
        onPeer { h in self.record(Phone.addToPlaylist(h, playlist.id, track.id), "add \(track.title) to \(playlist.name)", "Couldn't add it") }
    }

    func remove(_ track: LibraryEntry, from playlist: Id) {
        onPeer { h in
            let name = self.playlists.rows.first { $0.id == playlist }?.name ?? "a playlist"
            self.record(Phone.removeFromPlaylist(h, playlist, track.id), "take \(track.title) off \(name)", "Couldn't take it off")
        }
    }

    // MARK: the account

    /// Offline on purpose, or back online.
    func setOnline(_ online: Bool) { onPeer { $0.peer.setOnline(online: online) } }

    /// Signed out: the session revoked through the domain (pushed while
    /// linked, pending for the next sign-in otherwise), nothing dialled after
    /// it, what is pending kept. The model closes the session after.
    func signOut() { onPeer { $0.peer.logout() } }

    /// Every number the diagnostics show: the status, and every change with where it stands.
    func diagnostics(done: @escaping (Status?, [(String, String)]) -> Void) {
        queue.async {
            let st = self.harken?.peer.status()
            let list = self.changes.map { c in (c.what, self.harken.map { Phone.standing($0.peer.standing(id: c.id)) } ?? "unknown") }
            self.main { done(st, list) }
        }
    }

    /// Stop telling and let the peer go; its disk stays for the next session.
    func close() {
        queue.async {
            self.closed = true
            self.harken?.peer.setListener(listener: nil)
            self.player?.attach(sound: nil)
            self.player = nil
            self.pages = [:]
            self.harken = nil
        }
    }
}
