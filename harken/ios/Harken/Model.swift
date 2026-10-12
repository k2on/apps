// The app's one model: the `Bridge` (Bridge.swift) and what the screens
// read out of it, published on the main actor. Everything about the peer —
// its queue, its views, its writes — is the bridge's; what is here is
// SwiftUI's half: the prefs kept in `UserDefaults`, the snapshot the bridge
// publishes, the last notice, and the sign-in, which is a request to the
// server off the main thread.
//
// The app works with nobody signed in: the peer is opened signed out, what
// is authored is nobody's, kept and written down, and nothing is dialled.
// Signing in (dev auth: a name) makes all of it the signer's and syncs it;
// signing out keeps what that person left pending for when they sign in
// again. Alone, the peer is its own authority over a local history.
import ArkDB
import ArkDBClient
import Foundation
import SwiftUI

@MainActor
final class Model: ObservableObject {
    @Published private(set) var prefs: Prefs
    @Published private(set) var snapshot = Snapshot()
    /// The last refusal, rejection or failure, for the status bar.
    @Published var notice: String? = nil

    private var bridge: Bridge? = nil

    init() {
        prefs = Model.load()
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("harken")
        do {
            // harken's module, as the domain crate writes it: in the app's
            // bundle (project.yml copies `../domain/harken.ark`), verified on load.
            guard let url = Bundle.main.url(forResource: "harken", withExtension: "ark"), let bytes = FileManager.default.contents(atPath: url.path) else {
                throw PeerError("this build carries no harken.ark")
            }
            let b = try Bridge(module: [UInt8](bytes), root: support, publish: { [weak self] s in self?.snapshot = s }, notice: { [weak self] n in self?.notice = n })
            bridge = b
            b.start(prefs)
        } catch {
            notice = "could not load harken.ark: \(error)"
        }
    }

    // MARK: prefs

    private static func load() -> Prefs {
        let d = UserDefaults.standard
        let login = d.string(forKey: "login").flatMap { try? Login.parse($0) }
        return Prefs(server: d.string(forKey: "server") ?? Prefs.defaults.server, workAlone: d.object(forKey: "workAlone") as? Bool ?? false, login: login)
    }

    private func save(_ p: Prefs) {
        let d = UserDefaults.standard
        d.set(p.server, forKey: "server")
        d.set(p.workAlone, forKey: "workAlone")
        if let l = p.login { d.set(l.toJson(), forKey: "login") } else { d.removeObject(forKey: "login") }
        prefs = p
    }

    /// Save and reopen: the old peer is closed and written down first.
    func apply(server: String, workAlone: Bool) {
        var p = prefs
        p.server = server
        p.workAlone = workAlone
        save(p)
        bridge?.reopen(p)
    }

    // MARK: who

    var user: String? { prefs.user }

    /// Somebody signs in with a name: the server's dev auth, which answers
    /// `/auth/login` with a code and no page — the redirect is `harken://`,
    /// the scheme the server allows a phone, and is read off the answer, not
    /// followed. A server with a real provider answers with a page, which
    /// this app does not open: it says so instead. Everything done so far as
    /// nobody becomes theirs and is pushed.
    func signIn(as name: String) {
        let who = name.trimmingCharacters(in: .whitespaces)
        guard !who.isEmpty else { return }
        let server = prefs.server
        Task {
            let result = await Task.detached { Result { try Auth.devLogin(server, who, redirect: "harken://auth") } }.value
            switch result {
            case .failure(let e):
                notice = "could not sign in: \(e)"
            case .success(let login):
                var p = prefs
                p.login = login
                save(p)
                bridge?.signIn(login)
            }
        }
    }

    /// Sign out: the token forgotten and the link stopped; what that person
    /// left pending stays theirs.
    func signOut() {
        var p = prefs
        p.login = nil
        save(p)
        bridge?.signOut()
    }

    // MARK: reading

    var library: [Track] { snapshot.library }
    var playlists: [Playlist] { snapshot.playlists }
    var items: [Track] { snapshot.items }
    var status: Status? { snapshot.status }
    var changes: [Authored] { snapshot.changes }
    var selectedPlaylist: Id? { snapshot.selected }
    var selected: Playlist? { snapshot.playlists.first { $0.id == snapshot.selected } }

    func select(_ id: Id) { bridge?.select(id) }

    /// Which of the person's playlists a track is on, for the menu's ticks.
    func playlistsOf(_ track: Track) -> Set<Id> { bridge?.playlistsOf(track) ?? [] }

    func caption(forPlaylist id: Id) -> String? { snapshot.caption(forPlaylist: id) }
    func caption(for track: Track, on playlist: Id) -> String? { snapshot.caption(for: track, on: playlist) }
    func caption(for change: Authored) -> String { snapshot.caption(for: change) }

    // MARK: writing, each one of harken.ark's mutations

    /// The `create_playlist` input's checks on a name being typed; nil when it would pass.
    func nameProblem(_ name: String) -> String? { bridge?.nameProblem(name) }

    func createPlaylist(named name: String) { bridge?.createPlaylist(name) }
    func add(_ track: Track, to playlist: Id? = nil) { bridge?.add(track, to: playlist) }
    func remove(_ track: Track, from playlist: Id) { bridge?.remove(track, from: playlist) }
    func toggle(_ track: Track, on playlist: Id) { bridge?.toggle(track, on: playlist) }

    /// Ask the authority whether it agrees: alone, at once; linked, on a later pump.
    func verify() { bridge?.verify() }

    // MARK: the status line

    var statusLine: String {
        guard let st = status else { return bridge == nil ? "no peer" : "…" }
        let pend = st.pending > 0 ? " · \(st.pending) not synced" : ""
        return "\(st.link) · #\(st.cursor)\(pend)"
    }

    var linkColor: Color {
        guard let st = status else { return .gray }
        if st.denied != nil { return .red }
        if st.alone { return .blue }
        if st.signedOut { return .gray }
        return st.linked ? .green : .orange
    }
}
