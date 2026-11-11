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
// **The views are the core's** (`HarkenSession`, harken/ffi): it holds
// each one by count, keeps its rows spliced on the kernel's thread, and says
// by its version whether it moved. The songs, the playlists, the albums,
// the artists, the songs' details and the last play are held for the
// session's life — Home's three read first, published, and the rest a turn
// later — and after every change each whose version moved is read again
// from the core (the songs and the playlists as records, the rest as their
// JSON through the generated types). A page — an album, an artist, a
// playlist, a performance — is a view too, held while a screen shows it
// ([hold]) and let go with it; and so are the catalogue's views a page reads
// beside its tracks (`Page.listings`: a person's works and the episodes
// they are on, a work and its performances, who is on one). Who is on an
// episode, and its transcript, are read once when a screen asks.
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
    /// Every podcast and series of sermons, by title.
    var shows = [ShowsEntry]()
    /// The feeds the library follows, by when each was added.
    var subscriptions = [SubscriptionsEntry]()
    /// The files the server has not got yet, by their media id.
    var downloads = [Id: DownloadsEntry]()
    /// How far the signed-in person has got in each episode, by its media id.
    var mine = [Id: ProgressEntry]()
    /// How far everybody has listened to a held show's episodes, by its key.
    var progress = [String: [ProgressEntry]]()
    /// A held show's episodes as its feed tells them — what each is about,
    /// when it came out — by the show's key, each by its media id.
    var episodes = [String: [Id: EpisodeDetailsEntry]]()
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
    /// The episodes a held person is on, newest first, by their name.
    var appearances = [String: [LibraryEntry]]()
    var status: Status? = nil
    /// Nothing the phone asked for still on its way (`Peer.settled`): what a
    /// screenshot waits for (ArkUI's `Ready`).
    var settled = false
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

/// A view the session holds and the list read from it; on the session's queue only.
private final class Held<E> {
    let view: HarkenView
    private let read: (HarkenSession, HarkenView) -> [E]
    private var holding: Holding? = nil
    /// Its version when it was last read: what says it moved.
    private var seen: UInt64 = 0
    private(set) var rows = [E]()
    init(_ view: HarkenView, _ read: @escaping (HarkenSession, HarkenView) -> [E]) {
        self.view = view
        self.read = read
    }

    /// Held, and read.
    func hydrate(_ s: HarkenSession) {
        if holding == nil { holding = s.hold(view: view) }
        reread(s)
    }

    private func reread(_ s: HarkenSession) {
        seen = s.version(view: view)
        rows = read(s, view)
    }

    /// Read again when its version moved since it was last read; true when it did.
    func update(_ s: HarkenSession) -> Bool {
        guard holding != nil, s.version(view: view) != seen else { return false }
        reread(s)
        return true
    }

    func letGo() { holding = nil }
}

/// The JSON rows of a view, read by the generated type.
private func json<E>(_ of: @escaping (Any?) -> E) -> (HarkenSession, HarkenView) -> [E] {
    { s, v in s.rows(view: v).map { of(J.parse($0)) } }
}

private let tracks: (HarkenSession, HarkenView) -> [LibraryEntry] = { s, v in s.tracks(view: v) }

/// Views held by a key while some page holds them — each hold one of the
/// core's, which keeps the view while any is held — and read like the rest;
/// on the session's queue only.
private final class Holds<E> {
    private let view: (String) -> HarkenView
    private let read: (HarkenSession, HarkenView) -> [E]
    private var held = [String: (view: Held<E>, holdings: [Holding])]()
    init(_ view: @escaping (String) -> HarkenView, _ read: @escaping (HarkenSession, HarkenView) -> [E]) {
        self.view = view
        self.read = read
    }

    /// Held once more; its rows when it was opened just now, none when it was already held.
    func hold(_ key: String, _ s: HarkenSession?) -> [E]? {
        guard let s else { return nil }
        let h = s.hold(view: view(key))
        if var e = held[key] {
            e.holdings.append(h)
            held[key] = e
            return nil
        }
        let v = Held<E>(view(key), read)
        v.hydrate(s)
        held[key] = (v, [h])
        return v.rows
    }

    /// Held once less; true when nothing holds it now and it is let go.
    func release(_ key: String) -> Bool {
        guard var e = held[key] else { return false }
        e.holdings.removeLast()
        if !e.holdings.isEmpty {
            held[key] = e
            return false
        }
        e.view.letGo()
        held[key] = nil
        return true
    }

    /// Every held view that moved, and its rows now.
    func update(_ s: HarkenSession) -> [(String, [E])] {
        var moved = [(String, [E])]()
        for (k, e) in held where e.view.update(s) { moved.append((k, e.view.rows)) }
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
    private let media: NativeMediaPlayer
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
    private var views: HarkenSession? = nil
    private let library = Held<LibraryEntry>(.library(playlist: Phone.noPlaylist), tracks)
    private let playlists = Held<Playlist>(.playlists, { s, v in s.playlists(view: v) })
    private let albums = Held<AlbumsEntry>(.albums, json { AlbumsEntry(json: $0) })
    private let artists = Held<ArtistsEntry>(.artists, json { ArtistsEntry(json: $0) })
    private let composers = Held<ComposersEntry>(.composers, json { ComposersEntry(json: $0) })
    private let shows = Held<ShowsEntry>(.shows, json { ShowsEntry(json: $0) })
    private let subscriptions = Held<SubscriptionsEntry>(.subscriptions, json { SubscriptionsEntry(json: $0) })
    private let downloads = Held<DownloadsEntry>(.downloads, json { DownloadsEntry(json: $0) })
    private let mine = Held<ProgressEntry>(.myProgress, json { ProgressEntry(json: $0) })
    private let progress = Holds<ProgressEntry>({ .showProgress(playlist: Phone.noPlaylist, id: $0) }, json { ProgressEntry(json: $0) })
    private let episodes = Holds<EpisodeDetailsEntry>({ .episodeDetails(playlist: Phone.noPlaylist, id: $0) }, json { EpisodeDetailsEntry(json: $0) })
    private let details = Held<TrackDetailsEntry>(.trackDetails, json { TrackDetailsEntry(json: $0) })
    private let played = Held<Play>(.lastPlayed, json { Play(json: $0) })
    private var pages = [Page: (held: Held<LibraryEntry>, holdings: [Holding])]()
    private let works = Holds<WorkSummary>({ .works(composer: $0) }, json { WorkSummary(json: $0) })
    private let work = Holds<WorkSummary>({ .work(id: $0) }, json { WorkSummary(json: $0) })
    private let recordings = Holds<RecordingsEntry>({ .recordings(work: $0) }, json { RecordingsEntry(json: $0) })
    private let credits = Holds<CreditsEntry>({ .credits(recording: $0) }, json { CreditsEntry(json: $0) })
    private let appearances = Holds<LibraryEntry>({ .appearances(playlist: Phone.noPlaylist, name: $0) }, tracks)

    /// The session for `login` at `server`, its replica under `root`; this
    /// phone named `name` in the listening session, playing through `media`
    /// (ark-media-player's `PhoneMediaPlayer`).
    init(server: String, login: Login, root: URL, name: String, media: NativeMediaPlayer,
         publish: @escaping (Snapshot) -> Void, notice: @escaping (String) -> Void,
         denied: @escaping (String) -> Void, failed: @escaping (String) -> Void) {
        self.server = server
        self.login = login
        self.root = root
        self.name = name
        self.media = media
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
    /// Dial the server now, whatever the link's backoff says: the app came to the front.
    /// On `queue`, as every call into the peer is: the peer's lock is held while
    /// it takes what the server sent, and the main thread must not wait on it.
    func dialNow() { queue.async { self.harken?.peer.dialNow() } }

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
        p.attach(media: media, server: server, shell: PlayerHost(self))
        player = p
        h.peer.setListener(listener: Told { [weak self] in self?.drainSoon() })
        let v = HarkenSession(peer: h.peer)
        views = v
        library.hydrate(v)
        playlists.hydrate(v)
        albums.hydrate(v)
        snap.tracks = library.rows
        snap.playlists = playlists.rows
        snap.albums = albums.rows
        snap.loaded = true
        drain()
        // A later turn, so nothing that published waits on it.
        queue.async {
            guard let v = self.views else { return }
            self.artists.hydrate(v)
            self.composers.hydrate(v)
            self.shows.hydrate(v)
            self.subscriptions.hydrate(v)
            self.downloads.hydrate(v)
            self.mine.hydrate(v)
            self.details.hydrate(v)
            self.played.hydrate(v)
            self.snap.artists = self.artists.rows
            self.snap.composers = self.composers.rows
            self.snap.shows = self.shows.rows
            self.snap.subscriptions = self.subscriptions.rows
            self.snap.downloads = Bridge.downloadsByMedia(self.downloads.rows)
            self.snap.mine = Bridge.mineByMedia(self.mine.rows)
            self.snap.details = Bridge.byMedia(self.details.rows)
            self.snap.lastPlayed = self.played.rows.first
            self.snap.complete = true
            self.show()
        }
    }

    private static func mineByMedia(_ rows: [ProgressEntry]) -> [Id: ProgressEntry] {
        var m = [Id: ProgressEntry](minimumCapacity: rows.count)
        for r in rows { m[r.mediaId] = r }
        return m
    }

    private static func episodesByMedia(_ rows: [EpisodeDetailsEntry]) -> [Id: EpisodeDetailsEntry] {
        var m = [Id: EpisodeDetailsEntry](minimumCapacity: rows.count)
        for r in rows { m[r.mediaId] = r }
        return m
    }

    private static func downloadsByMedia(_ rows: [DownloadsEntry]) -> [Id: DownloadsEntry] {
        var m = [Id: DownloadsEntry](minimumCapacity: rows.count)
        for r in rows { m[r.mediaId] = r }
        return m
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
        snap.settled = h.peer.settled()
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
        guard let v = views else { return }
        if library.update(v) { snap.tracks = library.rows }
        if playlists.update(v) { snap.playlists = playlists.rows }
        if albums.update(v) { snap.albums = albums.rows }
        if artists.update(v) { snap.artists = artists.rows }
        if composers.update(v) { snap.composers = composers.rows }
        if shows.update(v) { snap.shows = shows.rows }
        if subscriptions.update(v) { snap.subscriptions = subscriptions.rows }
        if downloads.update(v) { snap.downloads = Bridge.downloadsByMedia(downloads.rows) }
        if mine.update(v) { snap.mine = Bridge.mineByMedia(mine.rows) }
        for (k, rows) in progress.update(v) { snap.progress[k] = rows }
        for (k, rows) in episodes.update(v) { snap.episodes[k] = Bridge.episodesByMedia(rows) }
        if details.update(v) { snap.details = Bridge.byMedia(details.rows) }
        if played.update(v) { snap.lastPlayed = played.rows.first }
        for (page, p) in pages where p.held.update(v) { snap.pages[page] = p.held.rows }
        for (k, rows) in works.update(v) { snap.works[k] = rows }
        for (k, rows) in work.update(v) { snap.work[k] = rows.first }
        for (k, rows) in recordings.update(v) { snap.recordings[k] = rows }
        for (k, rows) in credits.update(v) { snap.credits[k] = rows }
        for (k, rows) in appearances.update(v) { snap.appearances[k] = rows }
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
    /// Alone it is done to this phone's own player's queue.
    func press(_ p: Press, done: @escaping (String?) -> Void = { _ in }) {
        queue.async {
            guard let player = self.player else { return self.main { done(notPart) } }
            let w = player.press(press: p.json)
            self.step()
            self.show()
            self.main { done(w) }
        }
    }

    /// Alone: what is still to come shuffled, or not.
    func shuffle() { alone { $0.shuffle() } }

    /// Alone: off, all, one, off.
    func cycleRepeat() { alone { $0.cycleRepeat() } }

    private func alone(_ f: @escaping (Player) -> Void) {
        queue.async {
            guard let p = self.player else { return }
            f(p)
            self.step()
            self.show()
        }
    }

    /// A track's cover by its album, else its show's (an episode names its show as its creator),
    /// as the lock screen fetches it. On `queue`, where the player asks.
    fileprivate func cover(_ album: String, _ creator: String) -> String? {
        let art = snap.albums.first(where: { $0.name == album })?.art ?? snap.shows.first(where: { $0.title == creator })?.art ?? ""
        return art.isEmpty ? nil : Files.url(server, art)
    }

    /// Step again in `ms`: a play will count then.
    fileprivate func pokeIn(_ ms: Int64) {
        queue.asyncAfter(deadline: .now() + .milliseconds(Int(ms))) { self.poke() }
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
        guard let v = views, let view = Bridge.view(page) else { return false }
        let h = v.hold(view: view)
        if var p = pages[page] {
            p.holdings.append(h)
            pages[page] = p
            return false
        }
        let held = Held<LibraryEntry>(view, tracks)
        held.hydrate(v)
        pages[page] = (held, [h])
        snap.pages[page] = held.rows
        return true
    }

    /// A page's tracks as the domain names the view; none for every song,
    /// which is the library's own, and for a work's ([Page.listings]).
    private static func view(_ page: Page) -> HarkenView? {
        let none = Phone.noPlaylist
        switch page {
        case let .album(id): return .album(playlist: none, id: id)
        case let .artist(name): return .artist(playlist: none, name: name)
        case let .playlist(id): return .playlist(id: id)
        case let .recording(id): return .recording(playlist: none, id: id)
        case let .show(id): return .show(playlist: none, id: id)
        case .work, .songs: return nil
        }
    }

    // On `queue`: a listing held once more; true when it was read just now.
    private func hold(_ l: Listing) -> Bool {
        let h = views
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
        case let .progress(k):
            guard let rows = progress.hold(k, h) else { return false }
            snap.progress[k] = rows
        case let .episodes(k):
            guard let rows = episodes.hold(k, h) else { return false }
            snap.episodes[k] = Bridge.episodesByMedia(rows)
        case let .appearances(k):
            guard let rows = appearances.hold(k, h) else { return false }
            snap.appearances[k] = rows
        }
        return true
    }

    func release(_ page: Page) {
        queue.async {
            for l in page.listings { self.release(l) }
            guard var p = self.pages[page] else { return }
            p.holdings.removeLast()
            if !p.holdings.isEmpty {
                self.pages[page] = p
                return
            }
            p.held.letGo()
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
        case let .progress(k): if progress.release(k) { snap.progress[k] = nil }
        case let .episodes(k): if episodes.release(k) { snap.episodes[k] = nil }
        case let .appearances(k): if appearances.release(k) { snap.appearances[k] = nil }
        }
    }

    /// A page's tracks once, for playing it from somewhere that does not show
    /// it (Home's last played; a performance's Play on a page of performances):
    /// a person's, their songs and then the episodes they are on.
    func read(_ page: Page, done: @escaping ([LibraryEntry]) -> Void) {
        ask([], { h -> [LibraryEntry] in
            if page == .songs { return self.library.rows }
            if case let .artist(name) = page { return Phone.rows(h, page) + Phone.once({ try Phone.appearances(h, name) }) }
            return Phone.rows(h, page)
        }, done)
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

    /// Episodes' chapters once, each in order, by media id.
    func chapters(_ media: [Id], done: @escaping ([Id: [ChaptersEntry]]) -> Void) {
        ask([:], { h in
            var out = [Id: [ChaptersEntry]]()
            for m in media { out[m] = Phone.once({ try h.chapters(EpisodeInput(mediaId: m)) }) }
            return out
        }, done)
    }

    /// Who is on episodes once, each in the feed's order, by media id.
    func people(_ media: [Id], done: @escaping ([Id: [PeopleEntry]]) -> Void) {
        ask([:], { h in
            var out = [Id: [PeopleEntry]]()
            for m in media { out[m] = Phone.once({ try h.people(EpisodeInput(mediaId: m)) }) }
            return out
        }, done)
    }

    /// An episode's transcript once, its lines in the order they start.
    func transcript(_ media: Id, done: @escaping ([TranscriptEntry]) -> Void) {
        ask([], { h in Phone.once({ try h.transcript(EpisodeInput(mediaId: media)) }) }, done)
    }

    /// An episode said played, or not, by the signed-in person.
    func markPlayed(_ track: LibraryEntry, _ finished: Bool) {
        onPeer { h in self.record(Phone.markPlayed(h, track.id, finished), "mark \(track.title) \(finished ? "played" : "unplayed")", "Couldn't mark it") }
    }

    /// Who one of an episode's voices is; nobody, to take the name back. `done` once it is written.
    func nameVoice(_ media: Id, _ voice: String, _ name: String, done: @escaping () -> Void) {
        onPeer { h in
            self.record(Phone.nameVoice(h, media, voice, name), "name \(voice) \(name)", "Couldn't name them")
            self.main(done)
        }
    }

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
            if let v = self.views, self.playlists.update(v) { self.snap.playlists = self.playlists.rows }
            if let t = then, let made = self.playlists.rows.first(where: { !before.contains($0.id) }) {
                self.record(Phone.addToPlaylist(h, made.id, t.id), "add \(t.title) to \(made.name)", "Couldn't add it")
            }
        }
    }

    /// Follow a feed, as a podcast (`podcast`) or as sermons (`sermon`);
    /// refused, the notice says why.
    func follow(_ kind: String, _ url: String) {
        let url = url.trimmingCharacters(in: .whitespaces)
        onPeer { h in self.record(Phone.subscribe(h, kind, url), "follow \(url)", "Couldn't follow it") }
    }

    func unfollow(_ feed: SubscriptionsEntry) {
        onPeer { h in self.record(Phone.unsubscribe(h, feed.url), "unfollow \(feed.title.isEmpty ? feed.url : feed.title)", "Couldn't unfollow it") }
    }

    /// `more` of a followed feed's older episodes, or 0 for every one.
    func fetchOlder(_ feed: SubscriptionsEntry, _ more: Int64) {
        let name = feed.title.isEmpty ? feed.url : feed.title
        onPeer { h in self.record(Phone.fetchOlder(h, feed.url, more), "fetch older episodes of \(name)", "Couldn't fetch them") }
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
            self.player?.attach(media: nil, server: self.server, shell: nil)
            self.player = nil
            self.pages = [:]
            self.works.clear()
            self.work.clear()
            self.recordings.clear()
            self.credits.clear()
            self.progress.clear()
            self.episodes.clear()
            self.appearances.clear()
            self.views = nil
            self.harken = nil
        }
    }
}

/// What the player asks of the phone: a step now or later, and a track's cover. Told on the
/// session's queue, or the player's (the main queue); each goes to the session's queue.
private final class PlayerHost: PlayerShell, @unchecked Sendable {
    private weak var bridge: Bridge?

    init(_ b: Bridge) { bridge = b }

    func wake(inMs: Int64) {
        if inMs <= 0 { bridge?.poke() } else { bridge?.pokeIn(inMs) }
    }

    func cover(album: String, creator: String) -> String? { bridge?.cover(album, creator) }
}
