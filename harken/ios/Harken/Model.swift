import Foundation
import SwiftUI
import ArkDB
import ArkDBClient

/// The app's one model: a `Session` and what the screens read out of it —
/// the library, the playlists, and the selected playlist's contents —
/// refreshed on the session's change notification, plus where every change
/// this phone made stands.
///
/// Every mutation goes through `Phone` (Rows.swift) as an input of the
/// domain's own type, `session.author(name:args:)`: the session looks the
/// function up by name for its hash and autos, applies it through the
/// native procedure `module().procedures()` gave it — harken's domain, in
/// Swift, from `../domain/gen/swift` — and records the entry. Reads are
/// `session.query`, natively too. `add_song`, which the phone does not
/// author, arrives from the server's scanner and is applied by its facts.
///
/// The app works with nobody signed in: the session is opened signed out,
/// everything is authored as nobody, kept and written down, and nothing is
/// said to any server. Signing in makes all of it the signer's and syncs
/// it; signing out keeps what that person left pending for when they sign
/// in again (the server accepts an older login of the same person).
@MainActor
final class Model: ObservableObject {
    /// What the settings screen edits; kept in `UserDefaults`.
    struct Settings: Equatable {
        var server: String
        /// No server: this phone is the authority for its own log.
        var alone: Bool

        static let defaults = Settings(server: "ws://127.0.0.1:8787/sync", alone: false)

        static func load() -> Settings {
            let d = UserDefaults.standard
            return Settings(server: d.string(forKey: "server") ?? defaults.server,
                            alone: d.object(forKey: "alone") as? Bool ?? defaults.alone)
        }

        func save() {
            let d = UserDefaults.standard
            d.set(server, forKey: "server")
            d.set(alone, forKey: "alone")
        }
    }

    @Published private(set) var settings: Settings
    /// Who is signed in, or nil: remembered, so the app opens as them.
    @Published private(set) var user: String?
    @Published private(set) var tracks: [LibraryTrack] = []
    @Published private(set) var playlists: [PlaylistSummary] = []
    @Published var selectedPlaylist: ArkDB.Id? {
        didSet { if selectedPlaylist != oldValue { refresh() } }
    }
    @Published private(set) var items: [LibraryTrack] = []
    @Published private(set) var status: SessionStatus?
    /// Every change this phone made while it has been running, newest first.
    @Published private(set) var changes: [Authored] = []
    /// Where each of them stands, re-read on every tick.
    @Published private(set) var standings: [ArkDB.Id: Standing] = [:]
    /// The last refusal, rejection or denial, for the status bar.
    @Published private(set) var note: String?

    /// Which entry made a playlist, and which last touched a (playlist,
    /// item) pair: so a row can say it is not synced yet, or why it was not
    /// saved.
    private var madeBy: [ArkDB.Id: ArkDB.Id] = [:]
    private var touchedBy: [String: ArkDB.Id] = [:]

    private var session: Session?
    private var subscription: Int?
    private var poll: Timer?
    /// Since when the link has been up without a break: the default
    /// playlist waits for the log to have arrived.
    private var linkedSince: Date?
    private var defaultAsked = false
    private var rejectionsSeen = 0

    init() {
        settings = Settings.load()
        user = UserDefaults.standard.string(forKey: "user")
        open()
        // Acks and verdicts move no row, so where things stand is polled.
        poll = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.tick() }
        }
    }

    // MARK: the session

    /// Where the replica lives: one directory per server (or alone), not
    /// per person — the log is the server's and every person reads it —
    /// so that work done signed out is still here when somebody signs in.
    private func directory(for s: Settings) -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let place = s.alone ? "alone" : "server-" + s.server.map { $0.isLetter || $0.isNumber ? String($0) : "_" }.joined()
        return base.appendingPathComponent("harken").appendingPathComponent(place)
    }

    private func open() {
        closeSession()
        let s = settings
        let server: URL? = s.alone ? nil : URL(string: s.server)
        if !s.alone && server == nil {
            note = "not a URL: \(s.server)"
            return
        }
        do {
            let dir = directory(for: s)
            let session: Session
            if let who = user {
                session = try Session.open(directory: dir, module: Phone.moduleBytes, procedures: Phone.procedures,
                                           user: who, server: server)
            } else {
                session = try Session.openSignedOut(directory: dir, module: Phone.moduleBytes, procedures: Phone.procedures,
                                                    server: server)
            }
            self.session = session
            subscription = session.subscribe { [weak self] _ in
                // Called on the session's pump queue; the screens live on main.
                Task { @MainActor in self?.refresh() }
            }
            session.startPumping()
            rejectionsSeen = session.rejections.count
            note = nil
            refresh()
        } catch {
            note = "could not open: \(error)"
            self.session = nil
            tracks = []
            playlists = []
            items = []
            status = nil
        }
    }

    private func closeSession() {
        if let s = session {
            if let t = subscription { s.unsubscribe(t) }
            s.close()
        }
        session = nil
        subscription = nil
        linkedSince = nil
        defaultAsked = false
    }

    /// Apply new settings: save them and reopen against the new server, or
    /// alone.
    func apply(_ s: Settings) {
        s.save()
        settings = s
        selectedPlaylist = nil
        open()
    }

    /// Somebody signs in (dev auth: the name is the login's token, and the
    /// server calls every login "dev"). Everything done so far as nobody
    /// becomes theirs and is pushed.
    func signIn(as name: String) {
        let who = name.trimmingCharacters(in: .whitespaces)
        guard !who.isEmpty, let s = session else { return }
        s.signIn(user: who)
        user = who
        UserDefaults.standard.set(who, forKey: "user")
        defaultAsked = false
        refresh()
    }

    /// Sign out: reopen with nobody signed in, over the same replica. What
    /// that person left pending stays theirs, and goes when they sign in
    /// again.
    func signOut() {
        user = nil
        UserDefaults.standard.removeObject(forKey: "user")
        selectedPlaylist = nil
        open()
    }

    // MARK: reading

    func refresh() {
        guard let s = session else { return }
        do {
            playlists = try Phone.playlists(s)
            // A playlist can vanish under the selection (a rebase dropped
            // it); fall back to the first.
            if let sel = selectedPlaylist, !playlists.contains(where: { $0.id == sel }) { selectedPlaylist = nil }
            if selectedPlaylist == nil, let first = playlists.first?.id { selectedPlaylist = first }
            tracks = try Phone.library(s, against: selectedPlaylist)
            items = try selectedPlaylist.map { try Phone.playlist(s, $0) } ?? []
        } catch {
            note = "read failed: \(error)"
        }
        tick()
    }

    /// Once a second, and after every refresh: the status line, where each
    /// change stands, a new rejection's reason, and the default playlist.
    private func tick() {
        guard let s = session else { status = nil; return }
        let st = s.status
        status = st
        if let d = st.denied { note = "denied: \(d)" }
        linkedSince = st.linked ? (linkedSince ?? Date()) : nil
        var now: [ArkDB.Id: Standing] = [:]
        for c in changes { now[c.id] = s.standing(c.id) }
        if now != standings { standings = now }
        let rejected = s.rejections
        if rejected.count > rejectionsSeen, let last = rejected.last { note = "not saved: " + last.reason }
        rejectionsSeen = rejected.count
        makeDefaultIfNone(s, st)
    }

    /// The default playlist, for somebody who has none at all, once the view
    /// is known to hold every playlist of theirs: alone at once, signed in
    /// after the link has been up a moment (the log arrives first). Signed
    /// out it is made on first use instead (`add`).
    private func makeDefaultIfNone(_ s: Session, _ st: SessionStatus) {
        guard !defaultAsked else { return }
        let caughtUp = linkedSince.map { Date().timeIntervalSince($0) >= 2 } ?? false
        let known = st.alone || (st.signedIn && caughtUp)
        guard known else { return }
        defaultAsked = true
        if let r = Phone.ensureDefault(s, known: true) { recordCreate(s, r, name: Phone.defaultPlaylist) }
    }

    var selected: PlaylistSummary? {
        return playlists.first { $0.id == selectedPlaylist }
    }

    /// Which of the person's playlists a track is on, for the menu's ticks.
    func playlistsOf(_ track: LibraryTrack) -> Set<ArkDB.Id> {
        guard let s = session, let on = try? Phone.playlistsOf(s, media: track.id) else { return [] }
        return Set(on.map { $0.id })
    }

    // MARK: where things stand

    /// What to write under a playlist: nothing once it is in the log.
    func caption(forPlaylist id: ArkDB.Id) -> String? {
        return madeBy[id].flatMap { standings[$0] }.flatMap(Phone.caption)
    }

    /// What to write under an item of a playlist.
    func caption(for track: LibraryTrack, on playlist: ArkDB.Id) -> String? {
        return touchedBy[key(playlist, track.id)].flatMap { standings[$0] }.flatMap(Phone.caption)
    }

    func caption(for change: Authored) -> String {
        return standings[change.id].flatMap(Phone.caption) ?? "saved"
    }

    // MARK: writing, each through the domain's procedure

    /// The `create_playlist` input's checks on a name being typed, for the
    /// new-playlist sheet's inline message; nil when it would pass.
    func nameProblem(_ name: String) -> String? {
        guard let s = session else { return nil }
        return Phone.nameProblem(s, name)
    }

    func createPlaylist(named name: String) {
        guard let s = session else { return }
        recordCreate(s, Phone.createPlaylist(s, name: name), name: name.trimmingCharacters(in: .whitespaces))
        refresh()
    }

    /// Put a track on a playlist — the selected one, or, for somebody with
    /// none, the default, made now.
    func add(_ track: LibraryTrack, to playlist: ArkDB.Id? = nil) {
        guard let s = session else { return }
        let target: ArkDB.Id
        if let p = playlist ?? selectedPlaylist {
            target = p
        } else {
            switch Phone.playlistForAdding(s) {
            case .failure(let why):
                note = "refused: " + refusalText(why)
                return
            case .success(let t):
                target = t.playlist
                if let entry = t.made {
                    record(.success(entry), "make the playlist \(Phone.defaultPlaylist)", nil)
                    madeBy[t.playlist] = entry
                }
            }
        }
        let name = playlists.first { $0.id == target }?.name ?? Phone.defaultPlaylist
        record(Phone.addToPlaylist(s, playlist: target, media: track.id), "add \(track.title) to \(name)", key(target, track.id))
        refresh()
    }

    func remove(_ track: LibraryTrack, from playlist: ArkDB.Id) {
        guard let s = session else { return }
        let name = playlists.first { $0.id == playlist }?.name ?? "a playlist"
        record(Phone.removeFromPlaylist(s, playlist: playlist, media: track.id), "take \(track.title) off \(name)", key(playlist, track.id))
        refresh()
    }

    /// On the playlist it is not on, or off the one it is on.
    func toggle(_ track: LibraryTrack, on playlist: ArkDB.Id) {
        if playlistsOf(track).contains(playlist) { remove(track, from: playlist) } else { add(track, to: playlist) }
    }

    private func key(_ playlist: ArkDB.Id, _ media: ArkDB.Id) -> String {
        return Hex.encode(playlist.bytes) + "/" + Hex.encode(media.bytes)
    }

    private func recordCreate(_ s: Session, _ r: Result<ArkDB.Id, Refusal>, name: String) {
        let before = Set(playlists.map { $0.id })
        record(r, "make the playlist \(name)", nil)
        if case .success(let entry) = r, let now = try? Phone.playlists(s) {
            for p in now where !before.contains(p.id) { madeBy[p.id] = entry }
        }
    }

    private func record(_ r: Result<ArkDB.Id, Refusal>, _ what: String, _ key: String?) {
        switch r {
        case .failure(let why):
            note = "refused: " + refusalText(why)
        case .success(let id):
            note = nil
            changes.insert(Authored(id: id, what: what), at: 0)
            if changes.count > 100 { changes.removeLast(changes.count - 100) }
            if let k = key { touchedBy[k] = id }
            if let s = session { standings[id] = s.standing(id) }
        }
    }

    func verify() {
        session?.verify()
        tick()
    }

    func goOnline() { session?.goOnline() }
    func goOffline() { session?.goOffline() }

    // MARK: the status line

    var statusLine: String {
        guard let st = status else { return session == nil ? "no session" : "…" }
        let where_ = st.alone ? "alone" : (!st.signedIn ? "signed out" : (st.linked ? "linked" : st.link))
        let pend = st.pending > 0 ? " · \(st.pending) not synced" : ""
        return "\(where_) · #\(st.cursor)\(pend)"
    }

    var linkColor: Color {
        guard let st = status else { return .gray }
        if st.denied != nil { return .red }
        if st.alone { return .blue }
        if !st.signedIn { return .gray }
        return st.linked ? .green : .orange
    }
}
