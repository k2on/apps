// The app's one model, on the main actor: the sign-in machine run
// (`ark_auth::onboarding`, Onboarding.swift) — its effects carried out, the probe and the exchange
// off the main thread — the session it opens ([Bridge]) and the snapshot it
// publishes, this phone's media player (ark-media-player's `PhoneMediaPlayer`,
// which the session's reconciler plays its own queue on) and what the screens
// draw of what plays ([PlayerState]), and every button the screens have. Nothing of
// SwiftUI but `ObservableObject` and `@Published`, which are Combine's: the
// views are SwiftUI's and read this; it is type-checked on Linux against a
// stand-in for Combine. The twin of Android's `ArkHost`, the wiring half of
// `HarkenApp.kt` and the screens' end of `Playback.kt`.
import ArkUICore
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import Combine

/// The tabs: Home, the Library, and Search's own. (`Tab` is SwiftUI's.)
enum Pane: Hashable {
    case home, library, search
}

@MainActor
final class Model: ObservableObject {
    // MARK: what is published

    @Published private(set) var step: Step
    /// What the onboarding's page says now: its question, what it waits on, its error.
    @Published private(set) var said = Said(question: nil, status: nil, error: nil)
    @Published private(set) var snapshot = Snapshot()
    @Published private(set) var player = PlayerState()
    /// The last refusal, rejection or failure, said once.
    @Published var notice: String? = nil
    /// The sign-in to open in the browser sheet, once: the view runs it and answers [signInReturned].
    @Published var browse: URL? = nil
    /// The full player is up.
    @Published var fullPlayer = false
    /// What the search last found, and the last few searches.
    @Published var needle = ""
    /// Episodes' chapters as read, by media id: read when a show's page
    /// or the player asks ([`loadChapters`]).
    @Published private(set) var chapters = [Id: [ChaptersEntry]]()
    /// Who is on episodes, and episodes' transcripts, as read, by media id:
    /// read when a page or the player asks ([`loadPeople`], [`loadTranscript`]).
    @Published private(set) var people = [Id: [PeopleEntry]]()
    @Published private(set) var transcripts = [Id: [TranscriptEntry]]()
    @Published private(set) var results: Results? = nil
    @Published private(set) var recent: [String]
    @Published var diagnosticsUnlocked: Bool

    // MARK: what it holds

    private let machine = Onboarding(app: .harken)
    private let remembered: Kept
    private let root: URL
    let deviceName: String
    let media: NativeMediaPlayer
    private var bridge: Bridge? = nil
    private var albumsById = [String: AlbumsEntry]()
    // What plays carries its album's name and no more: its cover is found by that.
    private var albumsByName = [String: AlbumsEntry]()
    private var showsByTitle = [String: ShowsEntry]()
    /// Where this phone's hand put the position, until the session's clock agrees.
    private var sought = Sought()
    private var asked: Task<Void, Never>? = nil
    private lazy var drag = Drag(clock: { nowMs() }, later: { ms, f in
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(Int(ms)), execute: f)
    }) { [weak self] of, level in self?.bridge?.press(.loud(of, .to(level))) }

    /// The model over `shelf` (the app's `UserDefaults`), replicas under
    /// `root`, this phone called `deviceName`, playing through `media`.
    init(shelf: Shelf, root: URL, deviceName: String, media: NativeMediaPlayer) {
        remembered = Kept(shelf)
        self.root = root
        self.deviceName = deviceName
        self.media = media
        recent = remembered.searches
        diagnosticsUnlocked = remembered.diagnostics
        step = machine.step()
        send(.launched(remembered: remembered.remembered, code: nil, returned: false))
    }

    // MARK: the onboarding

    /// An event from a screen, the browser sheet, a link or the app's phase.
    func send(_ e: Event) {
        // Back in front: the server dialled now rather than when the link's backoff says, so one
        // that is there is reached before a screen says it is not.
        if case .foreground = e { bridge?.dialNow() }
        let effects = machine.next(event: e)
        step = machine.step()
        said = machine.said()
        for e in effects { carry(e) }
    }

    private func carry(_ e: Effect) {
        switch e {
        case let .findServer(typed):
            let domain = harkenDomain()
            Task {
                let found = await Probe.find(typed, domain: domain, get: Probe.get)
                self.send(.checked(found: Found(found)))
            }
        case let .remember(r): remembered.remembered = r
        case let .browse(_, url): browse = URL(string: url)
        case let .exchange(server, code): trade { try exchange(server: server, code: code) }
        case let .redeem(server, code): trade { try redeem(server: server, code: code) }
        case let .openSession(server, login): open(server, login)
        case .openDemo: break
        case .closeSession: close()
        }
    }

    // A code traded off the main thread: the login, or the sentence why not.
    private func trade(_ f: @escaping @Sendable () throws -> Login) {
        Task {
            let answer: Event = await Task.detached {
                do { return Event.signedIn(login: try f()) } catch { return Event.signInFailed(why: why(error)) }
            }.value
            self.send(answer)
        }
    }

    /// What the browser sheet came back with: the redirect, or why there
    /// was none (none for closed).
    func signInReturned(_ url: URL?, error: String?) {
        browse = nil
        if let u = url, let code = Model.code(u) { return send(.code(code: code)) }
        send(.cameBack(why: url == nil ? error : "it came back without a code"))
    }

    /// A link the system opened the app with, or a code scanned.
    /// `harken://auth?code=…` is a sign-in coming back. Any other link of this
    /// app's is a route (the domain's `Route`): `harken://join?…`, a server and
    /// a code to join with, goes to the sign-in machine (`Event.joined`,
    /// asked about first); the rest is the shell's to navigate to ([link]).
    /// A join link of another scheme (a page's `https://…/join?…`) joins too.
    func open(url: URL) {
        if let code = Model.code(url) { return send(.code(code: code)) }
        if let route = Navigator<Route>.route(of: url, scheme: AppInfo.harken.scheme, parse: { routeFromUrl(url: $0) }) {
            if case let .join(server, code) = route { return send(.joined(server: server, code: code)) }
            link = url
            return
        }
        if let j = joinOf(link: url.absoluteString) { send(.joined(server: j.server, code: j.code)) }
    }

    /// A link the system opened the app with that names a page
    /// (`harken://…`), for the shell to navigate to (`Navigator.handle`);
    /// nil again once it has.
    @Published var link: URL? = nil

    /// What a link names that is not a page: a sign-in link (`harken://join?…`)
    /// is the sign-in machine's ([open(url:)]), and is not navigated to.
    func takes(_ route: Route) -> Bool {
        if case .join = route { return true }
        return false
    }

    /// The code in `harken://auth?code=…`, or none for any other link.
    nonisolated static func code(_ url: URL) -> String? {
        Probe.code(url, scheme: AppInfo.harken.scheme)
    }

    // MARK: the session

    private func open(_ server: String, _ login: Login) {
        close()
        let b = Bridge(server: server, login: login, root: root, name: deviceName, media: media,
                       publish: { [weak self] s in self?.took(s) },
                       notice: { [weak self] n in self?.notice = n },
                       denied: { [weak self] d in self?.send(.denied(reason: d)) },
                       failed: { [weak self] w in self?.failed(server, w) })
        bridge = b
    }

    private func close() {
        bridge?.close()
        bridge = nil
        snapshot = Snapshot()
        results = nil
        redraw()
    }

    // A replica that cannot be opened is not a reason to lose the login: signing in again opens it again.
    private func failed(_ server: String, _ why: String) {
        send(.openFailed(why: why))
    }

    private func took(_ s: Snapshot) {
        // What is drawn may have moved: a screenshot waits again (ArkUI's `Ready`).
        Ready.shared.changed()
        if s.albums.count != snapshot.albums.count || s.albums != snapshot.albums {
            var byId = [String: AlbumsEntry](minimumCapacity: s.albums.count)
            var byName = [String: AlbumsEntry](minimumCapacity: s.albums.count)
            for a in s.albums {
                byId[a.id] = a
                byName[a.name] = a
            }
            albumsById = byId
            albumsByName = byName
        }
        if s.shows.count != snapshot.shows.count || s.shows != snapshot.shows {
            var m = [String: ShowsEntry](minimumCapacity: s.shows.count)
            for sh in s.shows { m[sh.title] = sh }
            showsByTitle = m
        }
        snapshot = s
        redraw()
        // Signed in just now: opening, until the replica has what the server had —
        // or the server cannot be reached, and there is nothing more to wait for.
        if case .opening = step, let st = s.status, st.linked || st.cursor > 0 || st.link == "waiting to dial" || st.link == "offline" {
            send(.synced)
        }
    }

    /// A queued track's cover path: its album's, else — an episode names no album, and its show as
    /// its creator — its show's; "" for none.
    private func cover(_ album: String, _ creator: String) -> String {
        albumsByName[album]?.art ?? showsByTitle[creator]?.art ?? ""
    }

    // What plays, drawn again: from the reconciler's one view, and what only this phone's player has
    // — and the position where this phone's hand put it, while the clock has not caught up (Sought).
    private func redraw() {
        let h = snapshot.heard
        let local = Volume(of: .here, level: Int(h.localLevel), muted: h.localMuted, remote: false)
        let now = nowMs()
        var next = PlayerState.of(h, server: server, name: deviceName, art: cover, local: local,
                                  shuffle: h.shuffle, repeatMode: h.repeatMode, buffering: h.buffering, now: now)
        if let n = next.now {
            let shown = sought.shown(clock: next.clockMs, track: n.id, counting: next.counting, now: now)
            if shown != next.clockMs { (next.clockMs, next.clockAt) = (shown, now) }
        }
        if next != player { player = next }
    }

    // MARK: reading

    var server: String { bridge?.server ?? machine.remembered().server ?? "" }
    var login: Login? { bridge?.login }
    var tracks: [LibraryEntry] { snapshot.tracks }
    var playlists: [Playlist] { snapshot.playlists }
    var albums: [AlbumsEntry] { snapshot.albums }
    var artists: [ArtistsEntry] { snapshot.artists }
    var heard: Heard { snapshot.heard }

    /// The album a track is on.
    func album(of t: LibraryEntry) -> AlbumsEntry? { AlbumsEntry.of(t, details: snapshot.details, byId: albumsById) }
    func album(id: String) -> AlbumsEntry? { albumsById[id] }
    /// The album of what plays: its track's, when the library has the track;
    /// else the one of its name.
    func album(playing p: Playing) -> AlbumsEntry? {
        if let t = snapshot.tracks.first(where: { $0.id.hex == p.id }), let a = album(of: t) { return a }
        return p.album.isEmpty ? nil : albumsByName[p.album]
    }
    func artist(named n: String) -> ArtistsEntry? { snapshot.artists.first { $0.name == n } }
    func detail(_ t: LibraryEntry) -> TrackDetailsEntry? { snapshot.details[t.id] }
    func playlist(_ id: Id) -> Playlist? { snapshot.playlists.first { $0.id == id } }

    // The catalogue: everyone some work is by, and what a held page reads beside its tracks.
    var composers: [ComposersEntry] { snapshot.composers }

    /// Every podcast and series of sermons, by title; and those of one kind (`podcast`, `sermon`).
    var shows: [ShowsEntry] { snapshot.shows }
    func shows(of kind: String) -> [ShowsEntry] { snapshot.shows.filter { $0.kind == kind } }
    func show(_ id: String) -> ShowsEntry? { snapshot.shows.first { $0.id == id } }
    /// The show an episode is of; none for a song.
    func show(of t: LibraryEntry) -> ShowsEntry? { ShowsEntry.of(t, in: snapshot.shows) }
    /// The feeds the library follows; the one a show is read from, when
    /// it is followed; and a file the server has not got yet, by its
    /// media id.
    var subscriptions: [SubscriptionsEntry] { snapshot.subscriptions }
    func subscription(of show: String) -> SubscriptionsEntry? { snapshot.subscriptions.first { !$0.showId.isEmpty && $0.showId == show } }
    func download(_ media: Id) -> DownloadsEntry? { snapshot.downloads[media] }
    /// The feeds of a kind followed and not read into a show yet: listed
    /// over the shows, so one just followed is there at once.
    func unread(of kind: String) -> [SubscriptionsEntry] {
        snapshot.subscriptions.filter { f in f.kind == kind && (f.showId.isEmpty || show(f.showId) == nil) }
    }
    /// An episode of a held show as its feed tells it: what it is about, when it came out.
    func episode(_ media: Id, on show: String) -> EpisodeDetailsEntry? { snapshot.episodes[show]?[media] }
    /// How far the signed-in person has got in an episode; and everybody,
    /// in one of a held show's.
    func mine(_ media: Id) -> ProgressEntry? { snapshot.mine[media] }
    func listened(_ media: Id, on show: String) -> [ProgressEntry] { (snapshot.progress[show] ?? []).filter { $0.mediaId == media } }
    /// Who is signed in, as a progress row names them.
    var me: String { login?.user ?? "" }

    /// Read the chapters of these episodes, those not read yet.
    func loadChapters(_ media: [Id]) {
        let wanted = media.filter { chapters[$0] == nil }
        guard !wanted.isEmpty else { return }
        bridge?.chapters(wanted) { read in self.chapters.merge(read) { _, new in new } }
    }

    /// Read who is on these episodes, those not read yet.
    func loadPeople(_ media: [Id]) {
        let wanted = media.filter { people[$0] == nil }
        guard !wanted.isEmpty else { return }
        bridge?.people(wanted) { read in self.people.merge(read) { _, new in new } }
    }

    /// Read an episode's transcript, once.
    func loadTranscript(_ media: Id) {
        guard transcripts[media] == nil else { return }
        bridge?.transcript(media) { read in self.transcripts[media] = read }
    }

    /// Who one of an episode's voices is, and its transcript read again to say so.
    func nameVoice(_ media: Id, _ voice: String, _ name: String) {
        bridge?.nameVoice(media, voice, name.trimmingCharacters(in: .whitespaces)) {
            self.bridge?.transcript(media) { read in self.transcripts[media] = read }
        }
    }

    /// Somebody as the episodes read so far name them — their picture and
    /// their link — the first with either, else the first at all.
    func face(_ name: String) -> PeopleEntry? {
        let all = people.values.joined().filter { $0.name == name }
        return all.first { !$0.art.isEmpty || !$0.link.isEmpty } ?? all.first
    }
    /// The episodes somebody is on, newest first, while their page is held; none until first read.
    func appearances(of name: String) -> [LibraryEntry]? { snapshot.appearances[name] }
    func composer(named n: String) -> ComposersEntry? { snapshot.composers.first { $0.name == n } }
    /// A person's works, by catalogue, while their page is held; none until first read.
    func works(of composer: String) -> [WorkSummary]? { snapshot.works[composer] }
    /// A work's own row, by its key, while a page of it is held.
    func work(_ id: String) -> WorkSummary? { snapshot.work[id] }
    /// A work's performances, the most complete first, while a page of it is held; none until first read.
    func recordings(of work: String) -> [RecordingsEntry]? { snapshot.recordings[work] }
    /// Who is on a performance, in billing order, while its page is held.
    func credits(of take: String) -> [CreditsEntry] { snapshot.credits[take] ?? [] }

    /// The albums somebody made, as the albums list them.
    func albums(by creator: String) -> [AlbumsEntry] { snapshot.albums.filter { $0.creator == creator } }

    /// The albums an artist's songs are on.
    func albums(of tracks: [LibraryEntry]) -> [AlbumsEntry] {
        var seen = Set<String>()
        var out = [AlbumsEntry]()
        for t in tracks {
            guard let k = snapshot.details[t.id]?.albumId, !k.isEmpty, !seen.contains(k), let a = albumsById[k] else { continue }
            seen.insert(k)
            out.append(a)
        }
        return out
    }

    /// The server's host while it cannot be reached; none while it can.
    var unreachable: String? {
        guard let st = snapshot.status else { return nil }
        return st.link == "waiting to dial" || st.link == "offline" ? Probe.display(server) : nil
    }

    /// The server's host while the library has never arrived from it.
    var fetching: String? {
        guard let st = snapshot.status else { return nil }
        return st.linked || st.cursor > 0 ? nil : Probe.display(server)
    }

    /// What Home says was played last: the session's track, else the log's newest play.
    var lastPlayed: LastPlayed? {
        LastPlayed.of(snapshot.heard, played: snapshot.lastPlayed, playlists: snapshot.playlists, art: cover,
                      server: server, here: deviceName)
    }

    /// The id of what plays, for a row to draw in the accent.
    var playingId: String? { player.now?.id }

    /// A page's rows while it is held; none until first read.
    func rows(_ page: Page) -> [LibraryEntry]? { page == .songs ? snapshot.tracks : snapshot.pages[page] }
    func hold(_ page: Page) { bridge?.hold(page) }
    func release(_ page: Page) { bridge?.release(page) }

    // MARK: search

    /// The needle moved: asked of the domain after a beat, unless it is blank.
    func searchFor(_ n: String) {
        needle = n
        asked?.cancel()
        let t = n.trimmingCharacters(in: .whitespaces)
        if t.isEmpty { return results = nil }
        let first = results == nil
        asked = Task {
            if !first { try? await Task.sleep(nanoseconds: 150_000_000) }
            if Task.isCancelled { return }
            self.bridge?.search(t) { songs in
                guard self.needle.trimmingCharacters(in: .whitespaces) == t else { return }
                self.results = Results.of(t, songs: songs, albums: self.albums, artists: self.artists)
            }
        }
    }

    /// Remember a search, at the top, once.
    func searched(_ n: String) {
        recent = Recent.adding(n, to: recent)
        remembered.searches = recent
    }

    func forgetSearches() {
        recent = []
        remembered.searches = []
    }

    // MARK: playlists

    func playlistsOf(_ t: LibraryEntry, done: @escaping (Set<Id>) -> Void) {
        if let b = bridge { b.playlistsOf(t, done: done) } else { done([]) }
    }

    func nameProblem(_ name: String, done: @escaping (String?) -> Void) {
        if let b = bridge { b.nameProblem(name, done: done) } else { done(nil) }
    }

    func createPlaylist(_ name: String, then: LibraryEntry? = nil) { bridge?.createPlaylist(name, then: then) }
    func add(_ t: LibraryEntry, to p: Playlist) { bridge?.add(t, to: p) }
    func follow(_ kind: String, _ url: String) { bridge?.follow(kind, url) }
    func unfollow(_ feed: SubscriptionsEntry) { bridge?.unfollow(feed) }
    func fetchOlder(_ feed: SubscriptionsEntry, _ more: Int64) { bridge?.fetchOlder(feed, more) }
    func remove(_ t: LibraryEntry, from p: Id) { bridge?.remove(t, from: p) }

    func toggle(_ t: LibraryEntry, on p: Playlist, now on: Set<Id>) {
        if on.contains(p.id) { remove(t, from: p.id) } else { add(t, to: p) }
    }

    // MARK: playing

    private var joined: Bool { bridge != nil && snapshot.heard.joined }

    // A press: of the session while this phone is part of it, of its own player's queue alone.
    private func press(_ p: Press) { bridge?.press(p) }

    private func albumName(_ t: LibraryEntry) -> String { album(of: t)?.name ?? "" }

    /// Play `list` from `at`, from the page it was on.
    func play(_ list: [LibraryEntry], at: Int, from page: Page?, shuffled: Bool = false, positionMs: Int64? = nil) {
        guard let q = Started.queue(list, at: at, shuffled: shuffled, album: albumName) else { return }
        let from = page?.from(playlists) ?? ""
        // An episode begun picks up where its listener left it.
        let ms = positionMs ?? (shuffled ? 0 : list.indices.contains(at) ? mine(list[at].id)?.resumeMs ?? 0 : 0)
        press(.start(queue: q.queue, at: q.at, positionMs: ms, from: from))
    }

    func markPlayed(_ t: LibraryEntry, _ finished: Bool) { bridge?.markPlayed(t, finished) }

    /// Play one performance from its first movement, its movements the
    /// queue: read when it is pressed, as the page it is pressed on lists
    /// performances, not tracks.
    func playRecording(_ id: String) {
        bridge?.read(.recording(id)) { rows in self.play(rows, at: 0, from: .recording(id)) }
    }

    /// Play a work: its most complete performance, the first of its `recordings`.
    func playWork(_ id: String) {
        bridge?.fullest(id) { got in
            guard let got = got else { return }
            self.play(got.1, at: 0, from: .recording(got.0))
        }
    }

    func shuffle(_ list: [LibraryEntry], from page: Page?) {
        play(list, at: list.indices.randomElement() ?? 0, from: page, shuffled: true)
    }

    /// Put `t` right after what is playing; or play it, when nothing is.
    func playNext(_ t: LibraryEntry) {
        let q = t.queued(album: albumName(t))
        press(.next([q]))
    }

    /// Put `t` at the end of the queue; or play it, when nothing is.
    func addToQueue(_ t: LibraryEntry) {
        let q = t.queued(album: albumName(t))
        press(.append([q]))
    }

    func toggle() { press(.toggle) }
    func resume() { press(.play) }
    func pause() { press(.pause) }
    func next() { press(.skip(1)) }
    func previous() { press(.skip(-1)) }

    /// How far an episode's skip goes back, and on, in seconds: what a podcast player's do.
    static let jogBack: Int64 = 15
    static let jogForward: Int64 = 30

    /// Whether what plays is an episode of a show — no album, and its name a
    /// show's — whose transport skips seconds rather than a track.
    var jogs: Bool { player.now?.show(in: snapshot.shows) != nil }

    /// What plays as a row, whatever it is, for the full player's More
    /// (`Playing.entry`); none while nothing plays.
    var playingEntry: LibraryEntry? {
        player.now.map { $0.entry(durationMs: player.durationMs, songs: tracks, shows: snapshot.shows) }
    }

    /// Back or on `seconds` in what plays, within it.
    func jog(_ seconds: Int64) {
        let end = player.durationMs > 0 ? player.durationMs : Int64.max
        seek(min(max(player.position(nowMs()) + seconds * 1000, 0), end))
    }

    /// Seek, on release; while joined the scrubber's slot is emptied with it.
    func seek(_ ms: Int64) {
        // Shown where the finger left it at once, not after the press's round trip (Sought).
        if let n = player.now { sought.hold(ms, track: n.id, now: nowMs()) }
        press(.seek(ms))
        if joined { bridge?.press(.scrub(nil)) }
        redraw()
    }

    /// Where this phone's scrubber is being dragged to: said to the session's other devices; nothing alone.
    func scrub(_ ms: Int64) {
        if joined { bridge?.press(.scrub(ms)) }
    }

    /// Jump to the queue's `index`.
    func jump(_ index: Int) { press(.jump(index)) }

    /// Up-next rows taken out, or moved, as the queue sheet's list edits them.
    func removeUpNext(_ offsets: [Int]) {
        guard let at = player.now?.index else { return }
        for p in Queue.removing(offsets, at: at) {
            guard case .remove = p else { continue }
            press(p)
        }
    }

    func moveUpNext(_ offsets: [Int], to: Int) {
        guard let at = player.now?.index else { return }
        for p in Queue.moving(offsets, to: to, count: player.upNext.count, at: at) {
            guard case .move = p else { continue }
            press(p)
        }
    }

    /// The `from`th up-next row dragged to be the `to`th: one `move` of the
    /// queue, or none where it was.
    func moveUpNext(from: Int, to: Int) {
        guard let at = player.now?.index, from != to else { return }
        let (f, t) = (at + 1 + from, at + 1 + to)
        press(.move(from: f, to: t))
    }

    /// Alone, what is still to come shuffled; the session has no shuffle.
    func toggleShuffle() { if !joined { bridge?.shuffle() } }

    /// Off, all, one, off; alone only.
    func cycleRepeat() { if !joined { bridge?.cycleRepeat() } }

    /// Hand the sound to another device of the account.
    func transfer(_ to: String) { if joined { bridge?.press(.transfer(to)) } }

    // MARK: volume

    /// What is playing's volume dragged to `level`: a throttled transition
    /// while this phone is part of the session, and alone this phone's player's own.
    func volume(_ level: Int) {
        guard let v = player.volume else { return }
        if v.of != .here { return drag.to(v.of, level) }
        press(.loud(.here, .to(level)))
    }

    /// A volume slider let go: whatever the throttle held goes now.
    func volumeDone() { drag.done() }

    /// What is playing's mute, the other way from how it is.
    func mute() {
        guard let v = player.volume else { return }
        press(.loud(v.of, .mute(!v.muted)))
    }

    // MARK: the picker

    var deviceRows: [DeviceRow] { snapshot.heard.listening.map { DevicePicker.rows($0, me: snapshot.heard.me) } ?? [] }

    func pick(_ row: DeviceRow) {
        if case let .transfer(to)? = DevicePicker.pick(row, snapshot.heard.listening) { transfer(to) }
    }

    func level(_ row: DeviceRow, _ level: Int) { if joined { drag.to(row.level, level) } }

    func mute(_ row: DeviceRow) { if joined, let p = DevicePicker.mute(row) { bridge?.press(p) } }

    func join(_ row: DeviceRow) { if joined, let p = DevicePicker.join(row) { bridge?.press(p) } }

    // MARK: Home

    /// Home's last played pressed: the session's own track resumed where the
    /// session has it; a play from the log played again from its page.
    func resume(_ last: LastPlayed, then open: @escaping () -> Void) {
        if last.live {
            if !player.playing { resume() }
            return open()
        }
        let go = { (list: [LibraryEntry]) in
            if let at = list.firstIndex(where: { $0.id.hex == last.track.id }) { self.play(list, at: at, from: last.from) }
            else { self.notice = "That song isn't in your library any more." }
        }
        if let page = last.from, page != .songs, page.listsTracks { bridge?.read(page, done: go) } else { go(tracks) }
    }

    // MARK: the account

    /// Sign out: the session revoked, the link stopped, the login forgotten; what is pending stays.
    func signOut() {
        bridge?.signOut()
        send(.signOut)
    }

    /// Everything the diagnostics show.
    func diagnostics(done: @escaping (Status?, [(String, String)]) -> Void) {
        if let b = bridge { b.diagnostics(done: done) } else { done(nil, []) }
    }

    func unlockDiagnostics() {
        diagnosticsUnlocked = true
        remembered.diagnostics = true
    }
}
