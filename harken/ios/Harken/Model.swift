import Foundation
import SwiftUI
import ArkDB
import ArkDBClient

/// The app's one model: a `Session` and what the screens read out of it —
/// the library, the playlists, and the selected playlist's items joined to
/// their tracks — refreshed on the session's change notification.
///
/// Every mutation goes through `Harken` (Rows.swift) as an input of the
/// domain's own type, `session.mutate(name:args:)`: the session looks the
/// function up by name for its scope, hash and autos, applies it through
/// the native procedure `module().procedures()` gave it — harken's domain,
/// in Swift, from `../domain/gen/swift` — and records the entry. Reads are
/// `session.query`, natively too. `add_track`, which the phone does not
/// author, arrives from the server's scanner and is applied by its facts.
@MainActor
final class Model: ObservableObject {
    /// What the settings screen edits; kept in `UserDefaults`.
    struct Settings: Equatable {
        var server: String
        var user: String
        /// No server: this phone is the authority for its own scopes.
        var alone: Bool

        static let defaults = Settings(server: "ws://127.0.0.1:8787/sync", user: "alice", alone: false)

        static func load() -> Settings {
            let d = UserDefaults.standard
            return Settings(server: d.string(forKey: "server") ?? defaults.server,
                            user: d.string(forKey: "user") ?? defaults.user,
                            alone: d.object(forKey: "alone") as? Bool ?? defaults.alone)
        }

        func save() {
            let d = UserDefaults.standard
            d.set(server, forKey: "server")
            d.set(user, forKey: "user")
            d.set(alone, forKey: "alone")
        }
    }

    @Published private(set) var settings: Settings
    @Published private(set) var tracks: [LibraryTrack] = []
    @Published private(set) var playlists: [PlaylistSummary] = []
    @Published var selectedPlaylist: Id? {
        didSet { if selectedPlaylist != oldValue { refreshItems() } }
    }
    @Published private(set) var items: [PlaylistRow] = []
    @Published private(set) var status: SessionStatus?
    /// The last refusal, error or denial, for the status bar.
    @Published private(set) var note: String?

    private var session: Session?
    private var subscription: Int?
    private var poll: Timer?

    init() {
        settings = Settings.load()
        open()
        // The link's state changes without a change notification, so the
        // status line is polled.
        poll = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.refreshStatus() }
        }
    }

    // MARK: the session

    /// Where this user's replicas live: one directory per (mode, user), so
    /// a directory opened alone is never reopened against a server — the
    /// session refuses that, since the sequences mean different things.
    private func directory(for s: Settings) -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("harken").appendingPathComponent(s.alone ? "alone" : "server").appendingPathComponent(s.user)
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
            let session = try Session.open(directory: directory(for: s), module: Harken.moduleBytes,
                                           procedures: Harken.procedures, user: s.user, server: server)
            self.session = session
            subscription = session.subscribe { [weak self] _, _ in
                // Called on the session's pump queue; the screens live on main.
                Task { @MainActor in self?.refresh() }
            }
            session.startPumping()
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
    }

    /// Apply new settings: save them and reopen against the new server, or
    /// alone.
    func apply(_ s: Settings) {
        s.save()
        settings = s
        selectedPlaylist = nil
        open()
    }

    // MARK: reading

    func refresh() {
        guard let s = session else { return }
        do {
            tracks = try Harken.library(s)
            playlists = try Harken.playlists(s)
        } catch {
            note = "read failed: \(error)"
        }
        // A playlist can vanish under the selection (a rebase dropped it, or
        // another device's duplicate lost); fall back to the first.
        if let sel = selectedPlaylist, !playlists.contains(where: { $0.id == sel }) { selectedPlaylist = nil }
        if selectedPlaylist == nil { selectedPlaylist = playlists.first?.id }
        refreshItems()
        refreshStatus()
    }

    private func refreshItems() {
        guard let s = session, let pid = selectedPlaylist else { items = []; return }
        do {
            items = Harken.rows(try Harken.items(s, of: pid), tracks)
        } catch {
            note = "read failed: \(error)"
        }
    }

    private func refreshStatus() {
        status = session?.status
        if let d = status?.denied { note = "denied: \(d)" }
    }

    var selected: PlaylistSummary? {
        return playlists.first { $0.id == selectedPlaylist }
    }

    // MARK: writing, each through the domain's procedure

    /// The `create_playlist` input's checks on a name being typed, for the
    /// new-playlist sheet's inline message; nil when it would pass.
    func nameProblem(_ name: String) -> String? {
        guard let s = session else { return nil }
        return Harken.nameProblem(s, name)
    }

    func createPlaylist(named name: String) {
        guard let s = session else { return }
        noteRefusal(Harken.createPlaylist(s, name: name))
        refresh()
    }

    func add(_ track: LibraryTrack, to playlist: Id) {
        guard let s = session else { return }
        noteRefusal(Harken.addToPlaylist(s, playlist: playlist, track: track.id))
        refresh()
    }

    func remove(_ row: PlaylistRow) {
        guard let s = session else { return }
        noteRefusal(Harken.removeFromPlaylist(s, playlist: row.item.playlistId, track: row.item.trackId))
        refresh()
    }

    private func noteRefusal(_ r: Refusal?) {
        if let r = r { note = "refused: \(r.text)" } else { note = nil }
    }

    func verify() {
        session?.verify()
        refreshStatus()
    }

    func goOnline() { session?.goOnline() }
    func goOffline() { session?.goOffline() }

    // MARK: the status line

    var statusLine: String {
        guard let st = status else { return session == nil ? "no session" : "…" }
        let cursors = st.cursors.keys.sorted().map { "\($0) \(st.cursors[$0] ?? 0)" }.joined(separator: " · ")
        let where_ = st.alone ? "alone" : (st.linked ? "linked" : st.link)
        let pend = st.pending > 0 ? " · \(st.pending) pending" : ""
        return "\(where_) · \(cursors)\(pend)"
    }

    var linkColor: Color {
        guard let st = status else { return .gray }
        if st.denied != nil { return .red }
        if st.alone { return .blue }
        return st.linked ? .green : .orange
    }
}
