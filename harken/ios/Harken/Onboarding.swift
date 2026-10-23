// The onboarding as a state machine, and nothing of Apple's but Foundation:
// the server, signing in, and the app once both are behind it. Every
// transition is `next(event)`, returning the effects to run — remember this,
// open that — which the model carries out (Model.swift) and feeds back what
// came of them; nothing here touches the network, a browser or a clock, so
// every transition is held by `HarkenTests`. The twin of Android's
// `onboarding/Onboarding.kt`, `Address.kt`, `Probe.kt`, `Health.kt` and
// `Remembered.kt`, with the sign-in rule the web client has since the
// account menu: **there is no sign-in button**.
//
// A server with nobody signed in goes to sign in by itself — the server's
// page in an `ASWebAuthenticationSession`, back on `harken://auth` — and
// does so at most once a launch: a sign-in that came back refused, closed
// or empty, or a login turned away again straight after it was made, is said
// once on the page and not tried again, so it can never go round and round.
// What starts it again is what reloading the page is on the web: the app
// coming back to the front after it was away (`foreground`), or another
// server. iOS has no notification permission to ask before the lock screen
// shows what plays, so Android's permission step has no twin.
import Foundation

/// Where the onboarding stands.
enum Step: Equatable {
    /// The server's address: what was typed, whether it is being checked, and why it was not.
    case server(typed: String, checking: Bool, error: String?)
    /// Signing in at `server`.
    case signIn(server: String, phase: SignInPhase, error: String?)
    /// Signed in just now: the session is open and the server's snapshot on its way.
    case opening(server: String, login: Login)
    /// The app, over a session at `server` as `login`.
    case running(server: String, login: Login)

    static let start = Step.server(typed: "", checking: false, error: nil)
}

enum SignInPhase: Equatable {
    /// Nothing under way: a sign-in already tried this launch, its error said.
    case idle
    /// The server's sign-in is open in the browser sheet.
    case browser
    /// A code came back and is being exchanged.
    case exchanging
}

/// What happened.
enum Event: Equatable {
    case typed(String)
    /// "Continue" on the server step.
    case check
    /// What the server step's probe found.
    case checked(Found)
    /// "Use another server", from signing in.
    case changeServer
    /// `harken://auth?code=…` came back, from the browser sheet or as a link.
    case code(String)
    /// The browser sheet ended with no code: closed, refused, or empty.
    case cameBack(String)
    case signedIn(Login)
    case signInFailed(String)
    /// The server turned the session away.
    case denied(String)
    /// The session holds what the server had — or cannot reach it, and nothing more is coming soon.
    case synced
    case signOut
    /// The app came back to the front after it was away: a sign-in may be tried again.
    case foreground
}

/// What a transition asks for.
enum Effect: Equatable {
    /// Ask `/healthz` of what was typed (`Probe.find`); answer with `checked`.
    case findServer(String)
    case rememberServer(String)
    /// Open the server's sign-in in the browser sheet; answer with `code` or `cameBack`.
    case browse(String)
    /// Trade the code at `/auth/exchange`; answer with `signedIn` or `signInFailed`.
    case exchange(server: String, code: String)
    case rememberLogin(Login)
    case forgetLogin
    /// Open the peer at `server` as `login`, and hand it to the app.
    case openSession(server: String, login: Login)
    /// Close the peer, written down; its storage stays.
    case closeSession
}

/// The machine: where it stands, and whether this launch has already gone to sign in.
struct Onboarding {
    /// Where the sign-in sends the code back to.
    static let scheme = "harken"
    static var redirect: String { scheme + "://auth" }

    private(set) var step: Step
    /// A sign-in went this launch: not by itself again until the app comes back to the front.
    private(set) var tried = false
    /// Where a sign-in starts: `login_url`, the bridge's, a parameter so a test needs no library.
    let loginUrl: (String, String) -> String

    /// Where a launch starts, from what was remembered: nothing is the
    /// server; a server alone is signing in, gone to at once; a server and a
    /// login is the app, over the replica already on the phone.
    init(server: String?, login: Login?, loginUrl: @escaping (String, String) -> String) {
        self.loginUrl = loginUrl
        step = .start
        if let s = server, let l = login { step = .running(server: s, login: l) }
        else if let s = server { step = .signIn(server: s, phase: .idle, error: nil) }
    }

    /// What the first step asks for: the app's session, or the sign-in going by itself.
    mutating func begin() -> [Effect] {
        switch step {
        case let .running(server, login): return [.openSession(server: server, login: login)]
        case let .signIn(server, _, error): return go(server, error: error)
        default: return []
        }
    }

    // To sign in, by itself, unless this launch already has: then the page says why it is waiting.
    private mutating func go(_ server: String, error: String?) -> [Effect] {
        if tried {
            step = .signIn(server: server, phase: .idle, error: error ?? Onboarding.again)
            return []
        }
        tried = true
        step = .signIn(server: server, phase: .browser, error: error)
        return [.browse(loginUrl(server, Onboarding.redirect))]
    }

    /// What a sign-in page says when it will not go again by itself.
    static let again = "Signing in didn't finish. Leave Harken and come back to try again, or use another server."

    // Back to signing in, the server kept, the session closed and the login forgotten.
    private mutating func out(_ server: String, _ event: Event) -> [Effect]? {
        switch event {
        case let .denied(reason):
            let close: [Effect] = [.closeSession, .forgetLogin]
            return close + go(server, error: "The server signed you out (\(reason)). Sign in again — what's on this phone is kept, and syncs once you do.")
        case .signOut:
            // Signing out on purpose is not a sign-in to start again at once: the page waits.
            tried = true
            step = .signIn(server: server, phase: .idle, error: nil)
            return [.closeSession, .forgetLogin]
        default:
            return nil
        }
    }

    /// One event; an event that means nothing where the machine stands leaves it as it was.
    mutating func next(_ event: Event) -> [Effect] {
        if case .foreground = event {
            tried = false
            if case let .signIn(server, .idle, error) = step { return go(server, error: error == Onboarding.again ? nil : error) }
            return []
        }
        switch step {
        case let .server(typed, checking, _):
            switch event {
            case let .typed(t):
                step = .server(typed: t, checking: false, error: nil)
            case .check:
                if checking { return [] }
                switch Address.parse(typed) {
                case let .invalid(why): step = .server(typed: typed, checking: false, error: why)
                case .candidates:
                    step = .server(typed: typed, checking: true, error: nil)
                    return [.findServer(typed)]
                }
            case let .checked(found):
                if !checking { return [] }
                switch found {
                case let .server(url, _):
                    // Another server is another sign-in: it goes by itself.
                    tried = false
                    return [.rememberServer(url)] + go(url, error: nil)
                case let .failed(why): step = .server(typed: typed, checking: false, error: why)
                }
            default: break
            }
            return []
        case let .signIn(server, phase, _):
            switch event {
            case let .code(code):
                if phase == .exchanging { return [] }
                tried = true
                step = .signIn(server: server, phase: .exchanging, error: nil)
                return [.exchange(server: server, code: code)]
            case let .cameBack(why):
                if phase != .browser { return [] }
                step = .signIn(server: server, phase: .idle, error: why)
            case let .signedIn(login):
                if phase != .exchanging { return [] }
                step = .opening(server: server, login: login)
                return [.rememberLogin(login), .openSession(server: server, login: login)]
            case let .signInFailed(why):
                if phase != .exchanging { return [] }
                step = .signIn(server: server, phase: .idle, error: why)
            case .changeServer:
                if phase == .exchanging { return [] }
                step = .server(typed: Probe.display(server), checking: false, error: nil)
            default: break
            }
            return []
        case let .opening(server, login):
            if case .synced = event {
                step = .running(server: server, login: login)
                return []
            }
            return out(server, event) ?? []
        case let .running(server, _):
            return out(server, event) ?? []
        }
    }
}

// MARK: the server's address

/// What the server field came to: the URLs to try, in order, or why there are none.
///
/// One field takes `host`, `host:port` or a whole URL. A URL with a scheme
/// is taken at its word. Anything else is tried over `https` first, and over
/// `http` as well only where the address is one a certificate is unlikely to
/// cover and nobody outside can reach: loopback, the private ranges,
/// link-local, the carrier-grade range a tailnet uses, and a name with no dot
/// or one under `.local`, `.lan`, `.home.arpa` or `.internal`.
enum Address: Equatable {
    /// Base URLs with no trailing slash, `https` first.
    case candidates(urls: [String], host: String)
    case invalid(String)

    static func parse(_ typed: String) -> Address {
        let text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { return .invalid("Type the address of your server.") }
        if text.contains(where: { $0.isWhitespace }) { return .invalid("An address has no spaces in it.") }
        var scheme = ""
        var rest = text
        if let r = text.range(of: "://") {
            scheme = String(text[..<r.lowerBound])
            rest = String(text[r.upperBound...])
            if !["http", "https"].contains(scheme.lowercased()) { return .invalid("A server is reached over http or https, not \(scheme).") }
        }
        let authority = String(rest.prefix { $0 != "/" && $0 != "?" && $0 != "#" })
        var path = String(rest.dropFirst(authority.count).prefix { $0 != "?" && $0 != "#" })
        while path.hasSuffix("/") { path.removeLast() }
        let bad = Address.invalid("That isn't an address a server can have: \(text)")
        if authority.isEmpty || authority.contains("@") { return bad }
        guard let host = hostOf(authority) else { return bad }
        let port = String(authority.dropFirst(authority.hasPrefix("[") ? host.count + 2 : host.count))
        if !port.isEmpty {
            let n = Int(port.dropFirst())
            if !port.hasPrefix(":") || n == nil || n! < 1 || n! > 65535 { return .invalid("The port has to be a number from 1 to 65535.") }
        }
        let base = authority.lowercased() + path
        let urls: [String]
        if !scheme.isEmpty { urls = [scheme.lowercased() + "://" + base] }
        else if isLocal(host) { urls = ["https://" + base, "http://" + base] }
        else { urls = ["https://" + base] }
        return .candidates(urls: urls, host: host.lowercased())
    }

    // The host of `host`, `host:port`, `[v6]` or `[v6]:port`, or nil when it has characters no host has.
    private static func hostOf(_ authority: String) -> String? {
        if authority.hasPrefix("[") {
            guard let end = authority.firstIndex(of: "]") else { return nil }
            let v6 = String(authority[authority.index(after: authority.startIndex)..<end])
            let ok = !v6.isEmpty && v6.allSatisfy { $0.isLetter || $0.isNumber || $0 == ":" || $0 == "." }
            return ok ? v6 : nil
        }
        let h = String(authority.prefix { $0 != ":" })
        if h.isEmpty { return nil }
        let ok = h.unicodeScalars.allSatisfy { ($0.value < 128) && (CharacterSet.alphanumerics.contains($0) || $0 == "-" || $0 == ".") }
        return ok && !h.hasPrefix(".") && !h.hasPrefix("-") ? h : nil
    }

    /// Whether `host` is one nobody outside this network reaches, where a
    /// server is as likely to speak plain http as https.
    static func isLocal(_ host: String) -> Bool {
        var h = host.lowercased()
        while h.hasSuffix(".") { h.removeLast() }
        if h == "localhost" || h.hasSuffix(".localhost") { return true }
        if !h.contains(".") && !h.contains(":") { return true }
        if [".local", ".lan", ".home.arpa", ".internal"].contains(where: { h.hasSuffix($0) }) { return true }
        if h.contains(":") { return h == "::1" || h.hasPrefix("fc") || h.hasPrefix("fd") || h.hasPrefix("fe80:") }
        let parts = h.split(separator: ".", omittingEmptySubsequences: false).map { Int($0) }
        if parts.count != 4 || parts.contains(where: { $0 == nil || $0! < 0 || $0! > 255 }) { return false }
        let a = parts[0]!, b = parts[1]!
        return a == 127 || a == 10 || (a == 172 && (16...31).contains(b)) || (a == 192 && b == 168) || (a == 169 && b == 254) || (a == 100 && (64...127).contains(b))
    }
}

// MARK: the probe

/// What a request came back with, or why it did not.
enum Answer: Equatable {
    case answered(status: Int, body: String)
    /// No answer: the sentence for why ("nothing answered there").
    case unreachable(String)
}

/// Where the server step ended.
enum Found: Equatable {
    /// The base URL to sign in at and sync with; `newer` when it runs another schema of the domain.
    case server(String, newer: Bool)
    case failed(String)
}

/// Whether an address is a harken server this phone can use, from what its `/healthz` answers.
enum Health: Equatable {
    case serving(newer: Bool)
    case refused(String)

    /// `ok`, its connections, the log's head and horizon, and `domain <schema
    /// hash> <version> current`; `domain` is this app's own line.
    static func decide(_ host: String, status: Int, body: String, domain: String) -> Health {
        if status == 404 { return .refused("\(host) answered, but it isn't an Ark server: it has no health page.") }
        if status != 200 { return .refused("\(host) answered \(status). It may be starting up or broken; try again in a moment.") }
        let lines = body.split(separator: "\n", omittingEmptySubsequences: false).map { $0.trimmingCharacters(in: .whitespaces) }
        if lines.first != "ok" { return .refused("\(host) answered, but not the way an Ark server does.") }
        guard let line = lines.first(where: { $0.hasPrefix("domain ") }) else { return .refused("\(host) is an Ark server with no app on it yet.") }
        var runs = String(line.dropFirst("domain ".count))
        if runs.hasSuffix(" current") { runs = String(runs.dropLast(" current".count)) }
        let ours = domain.split(separator: " ").first.map(String.init) ?? domain
        let theirs = runs.split(separator: " ").first.map(String.init) ?? runs
        return .serving(newer: theirs != ours)
    }
}

enum Probe {
    /// Ask `/healthz` of each URL `typed` could mean, in order, with `get`
    /// making the request: the first answer decides — an `https` server that
    /// answers and is the wrong app is not tried again in the clear.
    static func find(_ typed: String, domain: String, get: (String) async -> Answer) async -> Found {
        let urls: [String]
        switch Address.parse(typed) {
        case let .invalid(why): return .failed(why)
        case let .candidates(u, _): urls = u
        }
        var last = "no answer"
        for url in urls {
            switch await get(url + "/healthz") {
            case let .unreachable(why): last = why
            case let .answered(status, body):
                switch Health.decide(display(url), status: status, body: body, domain: domain) {
                case let .serving(newer): return .server(url, newer: newer)
                case let .refused(why): return .failed(why)
                }
            }
        }
        let over = urls.count > 1 ? " over https or http" : ""
        return .failed("Couldn't reach \(display(urls[0]))\(over): \(last).")
    }

    /// A URL as a person would write it: the host and port.
    static func display(_ url: String) -> String {
        url.range(of: "://").map { String(url[$0.upperBound...]) } ?? url
    }
}

// MARK: what is remembered

/// A key/value store of strings; nil removes. `UserDefaults` in the app, a dictionary in a test.
protocol Shelf: AnyObject {
    func get(_ key: String) -> String?
    func put(_ key: String, _ value: String?)
}

final class MemoryShelf: Shelf {
    var map = [String: String]()
    func get(_ key: String) -> String? { map[key] }
    func put(_ key: String, _ value: String?) { map[key] = value }
}

final class DefaultsShelf: Shelf {
    private let d = UserDefaults.standard
    func get(_ key: String) -> String? { d.string(forKey: key) }
    func put(_ key: String, _ value: String?) {
        if let v = value { d.set(v, forKey: key) } else { d.removeObject(forKey: key) }
    }
}

/// What the onboarding remembers, so that it is asked once: the server, and
/// the login kept whole, so a restart reuses the token; one that no longer
/// parses is forgotten rather than trusted. And what this phone remembers
/// that is nobody else's business: the last few searches, and whether
/// diagnostics have been unlocked.
final class Remembered {
    private let shelf: Shelf
    init(_ shelf: Shelf) { self.shelf = shelf }

    var server: String? {
        get { shelf.get("ark.server") }
        set { shelf.put("ark.server", newValue) }
    }

    var login: Login? {
        get { shelf.get("ark.login").flatMap(Logins.parse) }
        set { shelf.put("ark.login", newValue.map(Logins.json)) }
    }

    var searches: [String] {
        get { (shelf.get("harken.searches") ?? "").split(separator: "\n").map(String.init).filter { !$0.isEmpty } }
        set { shelf.put("harken.searches", newValue.joined(separator: "\n")) }
    }

    var diagnostics: Bool {
        get { shelf.get("harken.diagnostics") == "1" }
        set { shelf.put("harken.diagnostics", newValue ? "1" : nil) }
    }
}

/// A login as `ark_auth::Login`'s JSON: the token, the session, and the account.
enum Logins {
    static func json(_ l: Login) -> String {
        J.obj(["\"token\":" + J.q(l.token), "\"session\":" + J.q(l.session),
               "\"user\":" + J.obj(["\"id\":" + J.q(l.user), "\"name\":" + J.q(l.name), "\"email\":" + J.q(l.email), "\"roles\":" + J.wlist(l.roles, J.q)]),
               "\"expires_ms\":" + String(l.expiresMs)])
    }

    static func parse(_ s: String) -> Login? {
        guard let o = J.parse(s) as? [String: Any], let u = o["user"] as? [String: Any],
              let token = o["token"] as? String, let session = o["session"] as? String, let id = u["id"] as? String else { return nil }
        return Login(token: token, session: session, user: id, name: u["name"] as? String ?? "", email: u["email"] as? String ?? "",
                     roles: u["roles"] as? [String] ?? [], expiresMs: o["expires_ms"].map { J.int($0) } ?? 0)
    }

    /// What to call whoever is signed in.
    static func name(_ l: Login) -> String { l.name.isEmpty ? l.user : l.name }
}
