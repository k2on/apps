// The first run as a state machine, and nothing of Apple's but Foundation:
// the server, signing in, and the budget once both are behind it — or the
// demo's household, which needs neither. Every transition is
// `next(event)`, returning the effects to run, which the model carries out
// (Model.swift) and feeds back what came of them; nothing here touches the
// network, a browser or a clock, so every transition is held by
// `MoneyTests`. harken's onboarding (harken/ios/Harken/Onboarding.swift),
// with its rule: **there is no sign-in button**. A server with nobody
// signed in goes to sign in by itself, at most once a launch; what starts
// it again is the app coming back to the front, or another server.
//
// The server's address, its `/healthz` and what is remembered are ArkUI's
// (`Address`, `Probe`, `Health`, `Shelf`), any Ark app's.
import ArkUICore
import Foundation

/// Where the first run stands.
enum Step: Equatable {
    /// The server's address: what was typed, whether it is being checked, and why it was not.
    case server(typed: String, checking: Bool, error: String?)
    /// Signing in at `server`.
    case signIn(server: String, phase: SignInPhase, error: String?)
    /// Signed in just now: the session is open and the server's snapshot on its way.
    case opening(server: String, login: Login)
    /// The budget, over a session at `server` as `login`.
    case running(server: String, login: Login)
    /// The demo's household, on this phone alone.
    case demo

    static let start = Step.server(typed: Onboarding.usual, checking: false, error: nil)
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
    /// "Try the Demo", from the server step.
    case tryDemo
    /// "Use Another Server", from signing in or the demo.
    case changeServer
    /// `money://auth?code=…` came back, from the browser sheet or as a link.
    case code(String)
    /// The browser sheet ended with no code: closed, refused, or empty.
    case cameBack(String)
    case signedIn(Login)
    case signInFailed(String)
    /// The server turned the session away.
    case denied(String)
    /// The session holds what the server had.
    case synced
    case signOut
    /// The app came back to the front after it was away: a sign-in may be tried again.
    case foreground
}

/// What a transition asks for.
enum Effect: Equatable {
    /// Ask `/healthz` of what was typed (`Probe.find`); answer with `checked`.
    case findServer(String)
    case rememberServer(String?)
    /// Open the server's sign-in in the browser sheet; answer with `code` or `cameBack`.
    case browse(String)
    /// Trade the code at `/auth/exchange`; answer with `signedIn` or `signInFailed`.
    case exchange(server: String, code: String)
    case rememberLogin(Login?)
    case rememberDemo(Bool)
    /// Open the peer at `server` as `login`, and hand it to the app.
    case openSession(server: String, login: Login)
    /// Open the demo's household.
    case openDemo
    /// Close the peer, written down; a server's storage stays.
    case closeSession
}

/// The machine: where it stands, and whether this launch has already gone to sign in.
struct Onboarding {
    /// Where the sign-in sends the code back to.
    static let scheme = "money"
    static var redirect: String { scheme + "://auth" }
    /// The server the field starts with: the household's.
    static let usual = "money.koon.us"

    private(set) var step: Step
    /// A sign-in went this launch: not by itself again until the app comes back to the front.
    private(set) var tried = false
    /// Where a sign-in starts: `loginUrl`, the bridge's, a parameter so a test needs no library.
    let loginUrl: (String, String) -> String

    /// Where a launch starts, from what was remembered: the demo; a server
    /// and a login is the budget over the replica already on the phone; a
    /// server alone is signing in, gone to at once; nothing is the server.
    init(server: String?, login: Login?, demo: Bool, loginUrl: @escaping (String, String) -> String) {
        self.loginUrl = loginUrl
        step = .start
        if demo { step = .demo }
        else if let s = server, let l = login { step = .running(server: s, login: l) }
        else if let s = server { step = .signIn(server: s, phase: .idle, error: nil) }
    }

    /// What the first step asks for: the session, the demo, or the sign-in going by itself.
    mutating func begin() -> [Effect] {
        switch step {
        case let .running(server, login): return [.openSession(server: server, login: login)]
        case .demo: return [.openDemo]
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
    static let again = "Signing in didn't finish. Leave Money and come back to try again, or use another server."

    // Back to signing in, the server kept, the session closed and the login forgotten.
    private mutating func out(_ server: String, _ event: Event) -> [Effect]? {
        switch event {
        case let .denied(reason):
            let close: [Effect] = [.closeSession, .rememberLogin(nil)]
            return close + go(server, error: "The server signed you out (\(reason)). Sign in again — what's on this phone is kept, and syncs once you do.")
        case .signOut:
            tried = true
            step = .signIn(server: server, phase: .idle, error: nil)
            return [.closeSession, .rememberLogin(nil)]
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
                    tried = false
                    return [.rememberServer(url)] + go(url, error: nil)
                case let .failed(why): step = .server(typed: typed, checking: false, error: why)
                }
            case .tryDemo:
                if checking { return [] }
                step = .demo
                return [.rememberDemo(true), .openDemo]
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
        case .demo:
            switch event {
            case .changeServer, .signOut:
                step = .start
                return [.closeSession, .rememberDemo(false)]
            default:
                return []
            }
        }
    }
}

/// What the first run remembers, so that it is asked once: the server, the
/// login kept whole (a restart reuses the token; one that no longer parses
/// is forgotten rather than trusted), and whether this phone is showing the demo.
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

    var demo: Bool {
        get { shelf.get("money.demo") == "1" }
        set { shelf.put("money.demo", newValue ? "1" : nil) }
    }
}

/// A login as `ark_auth::Login`'s JSON: the token, the session, and the account.
enum Logins {
    static func json(_ l: Login) -> String {
        J.obj(["\"token\":" + J.q(l.token), "\"session\":" + J.q(l.session),
               "\"user\":" + J.obj(["\"id\":" + J.q(l.user), "\"name\":" + J.q(l.name), "\"email\":" + J.q(l.email), "\"picture\":" + J.q(l.picture), "\"roles\":" + J.wlist(l.roles, J.q)]),
               "\"expires_ms\":" + String(l.expiresMs)])
    }

    static func parse(_ s: String) -> Login? {
        guard let o = J.parse(s) as? [String: Any], let u = o["user"] as? [String: Any],
              let token = o["token"] as? String, let session = o["session"] as? String, let id = u["id"] as? String else { return nil }
        return Login(token: token, session: session, user: id, name: u["name"] as? String ?? "", email: u["email"] as? String ?? "",
                     roles: u["roles"] as? [String] ?? [], expiresMs: o["expires_ms"].map { J.int($0) } ?? 0,
                     picture: u["picture"] as? String ?? "")
    }

    /// What to call whoever is signed in.
    static func name(_ l: Login) -> String { l.name.isEmpty ? l.user : l.name }
}
