// The app's one model, on the main actor: the onboarding's machine run
// (Onboarding.swift) — its effects carried out, the probe and the exchange
// off the main thread — the session it opens ([Bridge]) and the snapshot it
// publishes, this phone's player ([SoundCore]) and what the screens draw of
// what plays ([PlayerState]), and every button the screens have. Nothing of
// SwiftUI but `ObservableObject` and `@Published`, which are Combine's: the
// views are SwiftUI's and read this; it is type-checked on Linux against a
// stand-in for Combine. The twin of Android's `ArkHost`, the wiring half of
// `HarkenApp.kt` and the screens' end of `Playback.kt`.
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import Combine

/// What a screen can ask to open: a page of a tab's stack, or Settings.
enum Route: Hashable {
    case page(Page)
    case settings
    case diagnostics
}

/// The tabs: Home, the Library, and Search's own. (`Tab` is SwiftUI's.)
enum Pane: Hashable {
    case home, library, search
}

@MainActor
final class Model: ObservableObject {
    // MARK: what is published

    @Published private(set) var step: Step
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
    @Published private(set) var results: Results? = nil
    @Published private(set) var recent: [String]
    @Published var diagnosticsUnlocked: Bool

    /// Told whenever what plays moved: the lock screen's and Control Center's (NowPlaying.swift).
    var onPlayer: ((PlayerState) -> Void)? = nil

    // MARK: what it holds

    private var machine: Onboarding
    private let remembered: Remembered
    private let root: URL
    let deviceName: String
    let sound: SoundCore
    private var bridge: Bridge? = nil
    private var albumsByName = [String: AlbumsEntry]()
    private var asked: Task<Void, Never>? = nil
    private lazy var drag = Drag(clock: { nowMs() }, later: { ms, f in
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(Int(ms)), execute: f)
    }) { [weak self] of, level in self?.bridge?.press(.loud(of, .to(level))) }

    /// The model over `shelf` (the app's `UserDefaults`), replicas under
    /// `root`, this phone called `deviceName`, playing through `engine`.
    init(shelf: Shelf, root: URL, deviceName: String, engine: Engine) {
        remembered = Remembered(shelf)
        self.root = root
        self.deviceName = deviceName
        sound = SoundCore(engine: engine)
        recent = remembered.searches
        diagnosticsUnlocked = remembered.diagnostics
        machine = Onboarding(server: remembered.server, login: remembered.login) { loginUrl(server: $0, redirect: $1) }
        step = machine.step
        sound.moved = { [weak self] in self?.redraw() }
        run(machine.begin())
    }

    // MARK: the onboarding

    /// An event from a screen, the browser sheet, a link or the app's phase.
    func send(_ e: Event) {
        let effects = machine.next(e)
        step = machine.step
        run(effects)
    }

    private func run(_ effects: [Effect]) {
        step = machine.step
        for e in effects { carry(e) }
    }

    private func carry(_ e: Effect) {
        switch e {
        case let .findServer(typed):
            let domain = harkenDomain()
            Task {
                let found = await Probe.find(typed, domain: domain, get: Model.get)
                self.send(.checked(found))
            }
        case let .rememberServer(url): remembered.server = url
        case let .browse(url): browse = URL(string: url)
        case let .exchange(server, code):
            Task {
                let answer: Event = await Task.detached {
                    do {
                        return Event.signedIn(try exchange(server: server, code: code))
                    } catch {
                        return Event.signInFailed("Signing in didn't finish: \(why(error)).")
                    }
                }.value
                self.send(answer)
            }
        case let .rememberLogin(l): remembered.login = l
        case .forgetLogin: remembered.login = nil
        case let .openSession(server, login): open(server, login)
        case .closeSession: close()
        }
    }

    /// What the browser sheet came back with: the redirect, or why there was none.
    func signInReturned(_ url: URL?, error: String?) {
        browse = nil
        if let u = url, let code = Model.code(u) { return send(.code(code)) }
        send(.cameBack(error ?? "The sign-in came back without a login."))
    }

    /// A link the system opened the app with: `harken://auth?code=…` is a sign-in coming back.
    func open(url: URL) {
        if let code = Model.code(url) { send(.code(code)) }
    }

    /// The code in `harken://auth?code=…`, or none for any other link.
    nonisolated static func code(_ url: URL) -> String? {
        guard url.scheme == Onboarding.scheme, url.host == "auth" else { return nil }
        let c = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "code" }?.value
        return (c?.isEmpty ?? true) ? nil : c
    }

    /// A GET asking for JSON, ten seconds to connect and ten to answer.
    nonisolated static func get(_ url: String) async -> Answer {
        guard let u = URL(string: url) else { return .unreachable("that isn't a URL") }
        var r = URLRequest(url: u, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10)
        r.setValue("application/json", forHTTPHeaderField: "Accept")
        return await withCheckedContinuation { done in
            URLSession.shared.dataTask(with: r) { data, response, error in
                if let e = error as? URLError { return done.resume(returning: .unreachable(Model.reason(e))) }
                if let e = error { return done.resume(returning: .unreachable("\(e.localizedDescription)")) }
                let status = (response as? HTTPURLResponse)?.statusCode ?? 0
                done.resume(returning: .answered(status: status, body: String(decoding: data ?? Data(), as: UTF8.self)))
            }.resume()
        }
    }

    /// Why a request failed, in words.
    nonisolated static func reason(_ e: URLError) -> String {
        switch e.code {
        case .cannotFindHost, .dnsLookupFailed: return "there is no such name"
        case .cannotConnectToHost: return "nothing answered there"
        case .notConnectedToInternet, .networkConnectionLost: return "this phone has no way to it"
        case .timedOut: return "it took too long to answer"
        case .serverCertificateUntrusted, .serverCertificateHasBadDate, .serverCertificateNotYetValid, .serverCertificateHasUnknownRoot, .secureConnectionFailed:
            return "its certificate isn't one this phone trusts"
        case .appTransportSecurityRequiresSecureConnection: return "this phone only speaks https to it"
        default: return e.localizedDescription
        }
    }

    // MARK: the session

    private func open(_ server: String, _ login: Login) {
        close()
        sound.urlOf = { URL(string: Files.url(server, $0.file)) }
        let b = Bridge(server: server, login: login, root: root, name: deviceName, sound: sound,
                       publish: { [weak self] s in self?.took(s) },
                       notice: { [weak self] n in self?.notice = n },
                       denied: { [weak self] d in self?.send(.denied(d)) },
                       failed: { [weak self] w in self?.failed(server, w) })
        sound.poke = { [weak b] in b?.poke() }
        bridge = b
    }

    private func close() {
        bridge?.close()
        bridge = nil
        sound.poke = {}
        snapshot = Snapshot()
        results = nil
        redraw()
    }

    // A replica that cannot be opened is not a reason to lose the login: signing in again opens it again.
    private func failed(_ server: String, _ why: String) {
        notice = "This phone's copy of the library couldn't be opened (\(why))."
        send(.signOut)
    }

    private func took(_ s: Snapshot) {
        if s.albums.count != snapshot.albums.count || s.albums != snapshot.albums {
            var m = [String: AlbumsEntry](minimumCapacity: s.albums.count)
            for a in s.albums { m[a.name] = a }
            albumsByName = m
        }
        snapshot = s
        redraw()
        // Signed in just now: opening, until the replica has what the server had —
        // or the server cannot be reached, and there is nothing more to wait for.
        if case .opening = step, let st = s.status, st.linked || st.cursor > 0 || st.link == "waiting to dial" || st.link == "offline" {
            send(.synced)
        }
    }

    // What plays, drawn again: from the reconciler's one view, and what only this phone's player has.
    private func redraw() {
        let h = snapshot.heard
        let local = sound.volume().map { Volume(of: .here, level: Int($0.level), muted: $0.muted, remote: false) }
        let next = PlayerState.of(h, server: server, name: deviceName, art: { [albumsByName] in albumsByName[$0]?.art ?? "" }, local: local,
                                  shuffle: sound.shuffled, repeatMode: sound.repeatMode, buffering: sound.state().buffering, now: nowMs())
        if next != player {
            player = next
            onPlayer?(next)
        }
    }

    // MARK: reading

    var server: String { bridge?.server ?? remembered.server ?? "" }
    var login: Login? { bridge?.login }
    var tracks: [LibraryEntry] { snapshot.tracks }
    var playlists: [Playlist] { snapshot.playlists }
    var albums: [AlbumsEntry] { snapshot.albums }
    var artists: [ArtistsEntry] { snapshot.artists }
    var heard: Heard { snapshot.heard }

    /// The album a track is on.
    func album(of t: LibraryEntry) -> AlbumsEntry? { AlbumsEntry.of(t, details: snapshot.details, byName: albumsByName) }
    func album(named n: String) -> AlbumsEntry? { albumsByName[n] }
    func artist(named n: String) -> ArtistsEntry? { snapshot.artists.first { $0.name == n } }
    func detail(_ t: LibraryEntry) -> TrackDetailsEntry? { snapshot.details[t.id] }
    func playlist(_ id: Id) -> Playlist? { snapshot.playlists.first { $0.id == id } }

    // The catalogue: everyone some work is by, and what a held page reads beside its tracks.
    var composers: [ComposersEntry] { snapshot.composers }
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
            guard let n = snapshot.details[t.id]?.album, !n.isEmpty, !seen.contains(n), let a = albumsByName[n] else { continue }
            seen.insert(n)
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
        LastPlayed.of(snapshot.heard, played: snapshot.lastPlayed, playlists: snapshot.playlists, art: { [albumsByName] in albumsByName[$0]?.art ?? "" },
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
    func remove(_ t: LibraryEntry, from p: Id) { bridge?.remove(t, from: p) }

    func toggle(_ t: LibraryEntry, on p: Playlist, now on: Set<Id>) {
        if on.contains(p.id) { remove(t, from: p.id) } else { add(t, to: p) }
    }

    // MARK: playing

    private var joined: Bool { bridge != nil && snapshot.heard.joined }

    // A press of the session while this phone is part of it, or — alone, or
    // refused because the link went since — `alone`, to the player.
    private func press(_ p: Press, alone: @escaping () -> Void) {
        guard let b = bridge, snapshot.heard.joined else { return alone() }
        b.press(p) { why in if why == notPart { alone() } }
    }

    private func albumName(_ t: LibraryEntry) -> String { album(of: t)?.name ?? "" }

    /// Play `list` from `at`, from the page it was on.
    func play(_ list: [LibraryEntry], at: Int, from page: Page?, shuffled: Bool = false) {
        guard let q = Started.queue(list, at: at, shuffled: shuffled, album: albumName) else { return }
        let from = page?.place(playlists) ?? ""
        press(.start(queue: q.queue, at: q.at, positionMs: 0, from: from)) { [sound] in sound.start(q.queue, at: q.at, from: from, shuffled: shuffled) }
    }

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
        press(.next([q])) { [sound] in sound.insert([q], next: true, from: "") }
    }

    /// Put `t` at the end of the queue; or play it, when nothing is.
    func addToQueue(_ t: LibraryEntry) {
        let q = t.queued(album: albumName(t))
        press(.append([q])) { [sound] in sound.insert([q], next: false, from: "") }
    }

    func toggle() { press(.toggle) { [sound] in sound.toggle() } }
    func resume() { press(.play) { [sound] in sound.resume() } }
    func pause() { press(.pause) { [sound] in sound.pause() } }
    func next() { press(.skip(1)) { [sound] in sound.next() } }
    func previous() { press(.skip(-1)) { [sound] in sound.previous() } }

    /// Seek, on release; while joined the scrubber's slot is emptied with it.
    func seek(_ ms: Int64) {
        press(.seek(ms)) { [sound] in sound.seek(ms: ms) }
        if joined { bridge?.press(.scrub(nil)) }
    }

    /// Where this phone's scrubber is being dragged to: said to the session's other devices; nothing alone.
    func scrub(_ ms: Int64) {
        if joined { bridge?.press(.scrub(ms)) }
    }

    /// Jump to the queue's `index`.
    func jump(_ index: Int) { press(.jump(index)) { [sound] in sound.jumpTo(index) } }

    /// Up-next rows taken out, or moved, as the queue sheet's list edits them.
    func removeUpNext(_ offsets: [Int]) {
        guard let at = player.now?.index else { return }
        for p in Queue.removing(offsets, at: at) {
            guard case let .remove(i) = p else { continue }
            press(p) { [sound] in sound.remove(i) }
        }
    }

    func moveUpNext(_ offsets: [Int], to: Int) {
        guard let at = player.now?.index else { return }
        for p in Queue.moving(offsets, to: to, count: player.upNext.count, at: at) {
            guard case let .move(from, to) = p else { continue }
            press(p) { [sound] in sound.move(from: from, to: to) }
        }
    }

    /// Alone, what is still to come shuffled; the session has no shuffle.
    func toggleShuffle() { if !joined { sound.shuffle() } }

    /// Off, all, one, off; alone only.
    func cycleRepeat() { if !joined { sound.cycleRepeat() } }

    /// Hand the sound to another device of the account.
    func transfer(_ to: String) { if joined { bridge?.press(.transfer(to)) } }

    /// The lock screen's seek: within the track, a seek.
    func lockScreenSeek(_ ms: Int64) { seek(ms) }

    // MARK: volume

    /// What is playing's volume dragged to `level`: a throttled transition
    /// while this phone is part of the session, and alone this phone's player's own.
    func volume(_ level: Int) {
        guard let v = player.volume else { return }
        if v.of != .here { return drag.to(v.of, level) }
        sound.setVolume(level: Int64(level), muted: false)
        redraw()
    }

    /// A volume slider let go: whatever the throttle held goes now.
    func volumeDone() { drag.done() }

    /// What is playing's mute, the other way from how it is.
    func mute() {
        guard let v = player.volume else { return }
        if v.of != .here { return press(.loud(v.of, .mute(!v.muted))) {} }
        sound.setVolume(level: Int64(v.level), muted: !v.muted)
        redraw()
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
