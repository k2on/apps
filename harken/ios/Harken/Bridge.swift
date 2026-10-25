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
// (`LiveView<T>`), read once — Home's three first, published, and the rest a
// turn later — and after every change each is asked what it did and its
// patches spliced into the list the screens hold; only a reset reads one
// again. A page — an album, an artist, a playlist, a performance — is a view
// too, held while a screen shows it ([hold]) and let go when the last one
// leaves; and so are the catalogue's views a page reads beside its tracks
// (`Page.listings`: a person's works, a work and its performances, who is
// on one), each held by its key for as long as some page holds it.
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
    /// Everyone some work here is by.
    var composers = [ComposersEntry]()
    var details = [Id: TrackDetailsEntry]()
    /// The newest play in the log, this person's.
    var lastPlayed: Play? = nil
    /// The pages held, each its rows once read.
    var pages = [Page: [LibraryEntry]]()
    /// The listings held, once read: a person's works by their name, a
    /// work's row by its key, its performances by its key, who is on a
    /// performance by its key.
    var works = [String: [WorkSummary]]()
    var work = [String: WorkSummary]()
    var recordings = [String: [RecordingsEntry]]()
    var credits = [String: [CreditsEntry]]()
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
    let open: (Harken) throws -> LiveView<E>?
    var view: LiveView<E>? = nil
    var rows = [E]()
    init(_ open: @escaping (Harken) throws -> LiveView<E>?) { self.open = open }

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

/// Views opened by a key while some page holds them, each held as many
/// times as pages hold it, spliced like the rest and let go with the last;
/// on the session's queue only.
private final class Holds<E> {
    private let open: (Harken, String) throws -> LiveView<E>
    private var held = [String: (view: Held<E>, refs: Int)]()
    init(_ open: @escaping (Harken, String) throws -> LiveView<E>) { self.open = open }

    /// Held once more; its rows when it was opened just now, none when it was already held.
    func hold(_ key: String, _ h: Harken?) -> [E]? {
        if var e = held[key] {
            e.refs += 1
            held[key] = e
            return nil
        }
        let open = self.open
        let v = Held<E> { try open($0, key) }
        held[key] = (v, 1)
        guard let h = h else { return nil }
        v.hydrate(h)
        return v.rows
    }

    /// Held once less; true when nothing holds it now and it is let go.
    func release(_ key: String) -> Bool {
        guard var e = held[key] else { return false }
        e.refs -= 1
        if e.refs > 0 {
            held[key] = e
            return false
        }
        held[key] = nil
        return true
    }

    /// Every held view that moved, and its rows now.
    func update() -> [(String, [E])] {
        var moved = [(String, [E])]()
        for (k, e) in held where e.view.update() { moved.append((k, e.view.rows)) }
        return moved
    }

    func clear() { held = [:] }
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
    private let composers = Held<ComposersEntry> { try $0.composers() }
    private let details = Held<TrackDetailsEntry> { try $0.trackDetails() }
    private let played = Held<Play> { try $0.lastPlayed() }
    private var pages = [Page: (held: Held<LibraryEntry>, refs: Int)]()
    private let works = Holds<WorkSummary> { h, composer in try h.works(Works(composer: composer)) }
    private let work = Holds<WorkSummary> { h, id in try h.work(WorkInput(id: id)) }
    private let recordings = Holds<RecordingsEntry> { h, work in try h.recordings(Recordings(workId: work)) }
    private let credits = Holds<CreditsEntry> { h, take in try h.credits(Credits(recordingId: take)) }

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
            self.composers.hydrate(h)
            self.details.hydrate(h)
            self.played.hydrate(h)
            self.snap.artists = self.artists.rows
            self.snap.composers = self.composers.rows
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
        if composers.update() { snap.composers = composers.rows }
        if details.update() { snap.details = Bridge.byMedia(details.rows) }
        if played.update() { snap.lastPlayed = played.rows.first }
        for (page, p) in pages where p.held.update() { snap.pages[page] = p.held.rows }
        for (k, rows) in works.update() { snap.works[k] = rows }
        for (k, rows) in work.update() { snap.work[k] = rows.first }
        for (k, rows) in recordings.update() { snap.recordings[k] = rows }
        for (k, rows) in credits.update() { snap.credits[k] = rows }
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

    /// A page's tracks and its listings, kept up to date while it is held;
    /// [release] it when the screen goes.
    func hold(_ page: Page) {
        if page == .songs { return }
        queue.async {
            var read = false
            if page.listsTracks { read = self.holdTracks(page) }
            for l in page.listings where self.hold(l) { read = true }
            if read { self.show() }
        }
    }

    // On `queue`: a page's tracks held once more; true when they were read just now.
    private func holdTracks(_ page: Page) -> Bool {
        if var p = pages[page] {
            p.refs += 1
            pages[page] = p
            return false
        }
        let held = Held<LibraryEntry> { try Phone.view($0, page) }
        pages[page] = (held, 1)
        guard let h = harken else { return false }
        held.hydrate(h)
        snap.pages[page] = held.rows
        return true
    }

    // On `queue`: a listing held once more; true when it was read just now.
    private func hold(_ l: Listing) -> Bool {
        let h = harken
        switch l {
        case let .works(k):
            guard let rows = works.hold(k, h) else { return false }
            snap.works[k] = rows
        case let .work(k):
            guard let rows = work.hold(k, h) else { return false }
            snap.work[k] = rows.first
        case let .recordings(k):
            guard let rows = recordings.hold(k, h) else { return false }
            snap.recordings[k] = rows
        case let .credits(k):
            guard let rows = credits.hold(k, h) else { return false }
            snap.credits[k] = rows
        }
        return true
    }

    func release(_ page: Page) {
        queue.async {
            for l in page.listings { self.release(l) }
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

    // On `queue`: a listing held once less, and gone from the snapshot with the last.
    private func release(_ l: Listing) {
        switch l {
        case let .works(k): if works.release(k) { snap.works[k] = nil }
        case let .work(k): if work.release(k) { snap.work[k] = nil }
        case let .recordings(k): if recordings.release(k) { snap.recordings[k] = nil }
        case let .credits(k): if credits.release(k) { snap.credits[k] = nil }
        }
    }

    /// A page's tracks once, for playing it from somewhere that does not show
    /// it (Home's last played; a performance's Play on a page of performances).
    func read(_ page: Page, done: @escaping ([LibraryEntry]) -> Void) {
        ask([], { h in page == .songs ? self.library.rows : Phone.rows(h, page) }, done)
    }

    /// A work's fullest performance — the first of its `recordings` — and its
    /// tracks once, in the order the work goes; none for a work nobody recorded.
    func fullest(_ work: String, done: @escaping ((String, [LibraryEntry])?) -> Void) {
        ask(nil, { h -> (String, [LibraryEntry])? in
            guard let take = Phone.once({ try h.recordings(Recordings(workId: work)) }).first else { return nil }
            return (take.id, Phone.rows(h, .recording(take.id)))
        }, done)
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
            self.works.clear()
            self.work.clear()
            self.recordings.clear()
            self.credits.clear()
            self.harken = nil
        }
    }
}
