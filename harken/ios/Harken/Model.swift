// The app's one model: the `Bridge` (Bridge.swift) and what the screens
// read out of it, published on the main actor. Everything about the peer —
// its queue, its views, its writes — is the bridge's; what is here is
// SwiftUI's half: the prefs kept in `UserDefaults`, the snapshot the bridge
// publishes, the last notice, and the sign-in, which is a request to the
// server off the main thread.
//
// The app works with nobody signed in: the peer is opened as nobody, what is
// authored is nobody's, kept and written down, and nothing is dialled.
// Signing in (dev auth: a name) makes all of it the signer's and syncs it;
// signing out keeps what that person left pending for when they sign in
// again. Alone, the peer is its own authority over a local history.
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
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let b = Bridge(root: support, publish: { [weak self] s in self?.snapshot = s }, notice: { [weak self] n in self?.notice = n })
        bridge = b
        b.start(prefs)
    }

    // MARK: prefs

    private static func load() -> Prefs {
        let d = UserDefaults.standard
        let login = d.string(forKey: "login").flatMap(Logins.parse)
        return Prefs(server: d.string(forKey: "server") ?? Prefs.defaults.server, workAlone: d.object(forKey: "workAlone") as? Bool ?? false, login: login)
    }

    private func save(_ p: Prefs) {
        let d = UserDefaults.standard
        d.set(p.server, forKey: "server")
        d.set(p.workAlone, forKey: "workAlone")
        if let l = p.login { d.set(Logins.json(l), forKey: "login") } else { d.removeObject(forKey: "login") }
        prefs = p
    }

    /// Save and reopen: the old peer let go first.
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
    /// followed — and the code exchanged by the bridge (`exchange`). A server
    /// with a real provider answers with a page, which this app does not
    /// open: it says so instead. Everything done so far as nobody becomes
    /// theirs and is pushed.
    func signIn(as name: String) {
        let who = name.trimmingCharacters(in: .whitespaces)
        guard !who.isEmpty else { return }
        let server = prefs.server
        Task {
            do {
                let login = try await Logins.dev(server, who)
                var p = prefs
                p.login = login
                save(p)
                bridge?.signIn(login)
            } catch {
                notice = "could not sign in: \(why(error))"
            }
        }
    }

    /// Sign out: the link stopped; what that person left pending stays theirs.
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

    // MARK: writing, each one of the domain's functions

    /// The `create_playlist` input's checks on a name being typed; nil when it would pass.
    func nameProblem(_ name: String) -> String? { bridge?.nameProblem(name) }

    func createPlaylist(named name: String) { bridge?.createPlaylist(name) }
    func add(_ track: Track, to playlist: Id? = nil) { bridge?.add(track, to: playlist) }
    func remove(_ track: Track, from playlist: Id) { bridge?.remove(track, from: playlist) }
    func toggle(_ track: Track, on playlist: Id) { bridge?.toggle(track, on: playlist) }

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
        if st.link == "signed out" { return .gray }
        return st.linked ? .green : .orange
    }
}

/// A login kept whole as `ark_auth::Login`'s JSON, and the dev sign-in.
enum Logins {
    static func json(_ l: Login) -> String {
        let user: [String: Any] = ["id": l.user, "name": l.name, "email": l.email, "roles": l.roles]
        let o: [String: Any] = ["token": l.token, "session": l.session, "user": user, "expires_ms": l.expiresMs]
        let d = (try? JSONSerialization.data(withJSONObject: o)) ?? Data()
        return String(decoding: d, as: UTF8.self)
    }

    static func parse(_ s: String) -> Login? {
        guard let o = (try? JSONSerialization.jsonObject(with: Data(s.utf8))) as? [String: Any], let u = o["user"] as? [String: Any],
              let token = o["token"] as? String, let session = o["session"] as? String, let id = u["id"] as? String else { return nil }
        return Login(token: token, session: session, user: id, name: u["name"] as? String ?? "", email: u["email"] as? String ?? "",
                     roles: u["roles"] as? [String] ?? [], expiresMs: (o["expires_ms"] as? NSNumber)?.int64Value ?? 0)
    }

    struct NoCode: Error, CustomStringConvertible {
        var description: String { "the server answered with a page, not a code: only dev auth signs in here" }
    }

    /// Dev auth: the code off `/auth/login`'s redirect, not followed, then exchanged.
    static func dev(_ server: String, _ user: String) async throws -> Login {
        let q = user.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? user
        guard let url = URL(string: loginUrl(server: server, redirect: "harken://auth") + "&user=" + q) else { throw NoCode() }
        let session = URLSession(configuration: .ephemeral, delegate: Stay(), delegateQueue: nil)
        let answer: URLResponse? = try await withCheckedThrowingContinuation { done in
            session.dataTask(with: url) { _, answer, error in
                if let e = error { done.resume(throwing: e) } else { done.resume(returning: answer) }
            }.resume()
        }
        guard let to = (answer as? HTTPURLResponse)?.value(forHTTPHeaderField: "Location"),
              let code = URLComponents(string: to)?.queryItems?.first(where: { $0.name == "code" })?.value else { throw NoCode() }
        return try await Task.detached { try exchange(server: server, code: code) }.value
    }

    /// A redirect read, not followed.
    private final class Stay: NSObject, URLSessionTaskDelegate {
        func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                        newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
            completionHandler(nil)
        }
    }
}
