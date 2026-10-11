// Who a peer is, and how it gets a token to say so (`rust/ark-auth`'s client
// half, `client.rs` and `util.rs`).
//
// The server is the only OpenID Connect client. A peer opens one URL —
// `/auth/login?redirect=R` — and receives one single-use code at `R`, which
// it trades at `/auth/exchange` for a `Login`: the token every `Hello`
// carries, the session its entries are authored under, and who it is. In dev
// mode, with a name given, the server answers the first request with the
// redirect itself and no page: that is the whole of a headless peer's
// sign-in, and of a phone in this repository.
//
// Over `URLSession`, which iOS, macOS and Linux Foundation all have (on Linux
// it is libcurl underneath, and plain HTTP is all a dev server needs), and
// the JSON reader the runtime already has: no dependency. `login`'s
// loopback port — the desktop's flow, `ark_auth::client::login_with` — is
// POSIX and Linux-only here; a phone comes back on its own scheme
// (`devLogin`'s `redirect`), and a provider's page on a phone needs the
// platform's web-authentication sheet, which is not built.
import ArkDB
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// A signed-in person, as the server knows them (`ark_auth::Account`).
public struct Account: Equatable {
    public var id: String
    public var name: String
    public var email: String
    /// The roles the server says this person holds; what the device's `Ctx` carries.
    public var roles: [String]

    public init(id: String, name: String = "", email: String = "", roles: [String] = []) {
        self.id = id
        self.name = name
        self.email = email
        self.roles = roles
    }
}

/// What a sign-in hands a client and the client keeps (`ark_auth::Login`).
public struct Login: Equatable {
    /// Proves the session; sent in every `Hello`, never in a URL.
    public var token: String
    /// The session's id, which entries authored under it carry.
    public var session: String
    public var user: Account
    /// When the token stops working, as milliseconds since the epoch.
    public var expiresMs: Int64

    public init(token: String, session: String, user: Account, expiresMs: Int64) {
        self.token = token
        self.session = session
        self.user = user
        self.expiresMs = expiresMs
    }

    /// The login as `serde_json` writes `ark_auth::Login`, roles left out when there are none.
    public func toJson() -> String {
        var u = "{\"id\":" + Json.quoted(user.id) + ",\"name\":" + Json.quoted(user.name) + ",\"email\":" + Json.quoted(user.email)
        if !user.roles.isEmpty { u += ",\"roles\":" + Json.array(user.roles.map(Json.quoted)) }
        u += "}"
        return "{\"token\":\(Json.quoted(token)),\"session\":\(Json.quoted(session)),\"user\":\(u),\"expires_ms\":\(expiresMs)}"
    }

    /// A login as the server's `/auth/exchange` answers it, or as `toJson` wrote it.
    public static func of(_ v: Value) throws -> Login {
        func bad(_ w: String) -> AuthError { AuthError("a login: \(w)") }
        guard case .record(let s) = v, case .record(let u)? = s["user"] else { throw bad("not an object with a user") }
        guard case .text(let token)? = s["token"], case .text(let session)? = s["session"], case .text(let id)? = u["id"] else { throw bad("no token, session or user id") }
        func text(_ k: String) throws -> String {
            switch u[k] {
            case nil, .null?: return ""
            case .text(let t)?: return t
            default: throw bad("\(k) is not text")
            }
        }
        var roles = [String]()
        switch u["roles"] {
        case nil, .null?: break
        case .list(let rs)?:
            roles = try rs.map { r in
                guard case .text(let t) = r else { throw bad("a role is not text") }
                return t
            }
        default: throw bad("roles is not a list")
        }
        var expires: Int64 = 0
        switch s["expires_ms"] {
        case nil, .null?: break
        case .int(let n)?: expires = n
        default: throw bad("expires_ms is not an integer")
        }
        return Login(token: token, session: session, user: Account(id: id, name: try text("name"), email: try text("email"), roles: roles), expiresMs: expires)
    }

    public static func parse(_ json: String) throws -> Login { try of(try Json.decode(json)) }
}

/// What a sign-in could not do.
public struct AuthError: Error, CustomStringConvertible {
    public let what: String
    public init(_ what: String) { self.what = what }
    public var description: String { what }
}

/// How long a request waits on the server (`ark_auth::client::Patience`):
/// the connection, and each read. Ten and twenty seconds by default.
public struct Patience: Equatable {
    public var connectMs: Int64
    public var readMs: Int64
    public init(connectMs: Int64 = 10_000, readMs: Int64 = 20_000) {
        self.connectMs = connectMs
        self.readMs = readMs
    }

    public static let standard = Patience()

    /// The same bound on the connection and on each read.
    public static func of(_ ms: Int64) -> Patience { Patience(connectMs: ms, readMs: ms) }
}

public enum Auth {
    /// The URL a client sends a person to (`ark_auth::login_url`): `redirect`
    /// is where the code comes back to; `user` is honoured only by a server
    /// in dev mode.
    public static func loginUrl(_ server: String, _ redirect: String, _ user: String?) -> String {
        var s = trimSlash(server) + "/auth/login?redirect=" + percentEncode(redirect)
        if let u = user { s += "&user=" + percentEncode(u) }
        return s
    }

    /// The socket beside the login, `/sync`, with the scheme to match (`ark_auth::socket_url`).
    public static func socketUrl(_ server: String) -> String {
        let s = trimSlash(server)
        let socket: String
        if s.hasPrefix("https://") {
            socket = "wss://" + s.dropFirst(8)
        } else if s.hasPrefix("http://") {
            socket = "ws://" + s.dropFirst(7)
        } else if s.hasPrefix("ws://") || s.hasPrefix("wss://") {
            socket = s
        } else {
            socket = "ws://" + s
        }
        return socket + "/sync"
    }

    static func trimSlash(_ s: String) -> String {
        var t = Substring(s)
        while t.hasSuffix("/") { t = t.dropLast() }
        return String(t)
    }

    /// Everything but the unreserved characters, as `%XX` (`ark_auth::percent_encode`).
    public static func percentEncode(_ s: String) -> String {
        var out = ""
        for b in s.utf8 {
            switch b {
            case 0x41...0x5a, 0x61...0x7a, 0x30...0x39, 0x2d, 0x5f, 0x2e, 0x7e: out.append(Character(Unicode.Scalar(b)))
            default: out += String(format: "%%%02X", b)
            }
        }
        return out
    }

    /// A query value back out of a URL's query (`ark_auth::query_value`).
    public static func queryValue(_ query: String, _ key: String) -> String? {
        let q = query.firstIndex(of: "?").map { String(query[query.index(after: $0)...]) } ?? query
        for pair in q.split(separator: "&", omittingEmptySubsequences: false) {
            let kv = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            if kv[0] == key { return percentDecode(kv.count > 1 ? String(kv[1]) : "") }
        }
        return nil
    }

    private static func percentDecode(_ s: String) -> String {
        let u = Array(s.utf8)
        var out = [UInt8]()
        var i = 0
        while i < u.count {
            if u[i] == 0x25, i + 2 < u.count, u[i + 1] != 0x2b, u[i + 1] != 0x2d, let b = UInt8(String(decoding: u[(i + 1)...(i + 2)], as: UTF8.self), radix: 16) {
                out.append(b)
                i += 3
                continue
            }
            out.append(u[i] == 0x2b ? 0x20 : u[i])
            i += 1
        }
        return String(decoding: out, as: UTF8.self)
    }

    /// Sign `user` in at a dev `server` with `redirect` as where the code
    /// comes back to: the server answers the first request with that
    /// redirect and a code, and no page; the code is exchanged. A server that
    /// answers with a page wants a person, and is an `AuthError` here.
    public static func devLogin(_ server: String, _ user: String, redirect: String = "http://127.0.0.1:9/", patience: Patience = .standard) throws -> Login {
        let url = loginUrl(server, redirect, user)
        let (status, location, body) = try Http.get(url, patience)
        guard status / 100 == 3 else {
            if status / 100 == 2 { throw AuthError("\(server) wants a person to sign in at \(url)") }
            throw AuthError("\(server) answered \(status): \(body)")
        }
        guard let loc = location, loc.hasPrefix(redirect) else { throw AuthError("\(server) sent the sign-in somewhere else: \(location ?? "nowhere")") }
        guard let code = queryValue(String(loc.dropFirst(redirect.count)), "code") else { throw AuthError("the server sent no code") }
        return try exchange(server, code, patience: patience)
    }

    /// The code, for the login it stands for (`ark_auth::client::exchange_with`).
    public static func exchange(_ server: String, _ code: String, patience: Patience = .standard) throws -> Login {
        let (status, body) = try Http.post(trimSlash(server) + "/auth/exchange", "{\"code\":\(Json.quoted(code))}", patience)
        if status / 100 != 2 { throw AuthError(body) }
        do {
            return try Login.parse(body)
        } catch {
            throw AuthError("the login did not parse: \(error)")
        }
    }

    #if os(Linux)
    /// Sign in to `server` (`ark_auth::client::login_with`): a dev server
    /// with `user` answers at once and no page opens; anything else is a page
    /// for a person, handed to `open`, and the code is read off the loopback
    /// request the server's redirect makes the browser send. Blocking, for as
    /// long as the person takes; the server is waited on for `patience` at most.
    public static func login(_ server: String, _ user: String?, patience: Patience = .standard, open: (String) -> Void = { _ in }) throws -> Login {
        let listener = try Loopback()
        defer { listener.close() }
        let redirect = "http://127.0.0.1:\(listener.port)/"
        let url = loginUrl(server, redirect, user)
        let (status, location, body) = try Http.get(url, patience)
        let code: String
        switch status / 100 {
        case 3:
            let loc = location ?? ""
            if loc.hasPrefix(redirect) {
                guard let c = queryValue(String(loc.dropFirst(redirect.count)), "code") else { throw AuthError("the server sent no code") }
                code = c
            } else {
                open(loc)
                code = try listener.waitForCode()
            }
        case 2:
            open(url)
            code = try listener.waitForCode()
        default:
            throw AuthError("\(server) answered \(status): \(body)")
        }
        return try exchange(server, code, patience: patience)
    }
    #endif
}

/// One request at a time over `URLSession`, waited for: what a sign-in needs
/// and no more. Redirects are not followed — a dev server's answer is one.
enum Http {
    private final class NoRedirects: NSObject, URLSessionTaskDelegate {
        func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
            completionHandler(nil)
        }
    }

    private static func run(_ req: URLRequest, _ patience: Patience) throws -> (Int, String?, String) {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = Double(max(patience.readMs, 1)) / 1000
        config.timeoutIntervalForResource = Double(max(patience.connectMs + patience.readMs, 1)) / 1000
        let session = URLSession(configuration: config, delegate: NoRedirects(), delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        let done = DispatchSemaphore(value: 0)
        var result: (Int, String?, String)? = nil
        var failure: Error? = nil
        let task = session.dataTask(with: req) { data, response, error in
            if let e = error {
                failure = e
            } else if let r = response as? HTTPURLResponse {
                let location = r.allHeaderFields.first { ($0.key as? String)?.lowercased() == "location" }?.value as? String
                result = (r.statusCode, location, data.map { String(decoding: $0, as: UTF8.self) } ?? "")
            }
            done.signal()
        }
        task.resume()
        done.wait()
        if let e = failure { throw AuthError("cannot reach \(req.url?.absoluteString ?? "the server"): \(e)") }
        guard let r = result else { throw AuthError("no answer from \(req.url?.absoluteString ?? "the server")") }
        return r
    }

    static func get(_ url: String, _ patience: Patience) throws -> (Int, String?, String) {
        guard let u = URL(string: url) else { throw AuthError("\(url) is not a URL") }
        var req = URLRequest(url: u)
        req.httpMethod = "GET"
        req.cachePolicy = .reloadIgnoringLocalCacheData
        return try run(req, patience)
    }

    static func post(_ url: String, _ json: String, _ patience: Patience) throws -> (Int, String) {
        guard let u = URL(string: url) else { throw AuthError("\(url) is not a URL") }
        var req = URLRequest(url: u)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = Data(json.utf8)
        let (s, _, b) = try run(req, patience)
        return (s, b)
    }
}

#if os(Linux)
import Glibc

/// The loopback listener a desktop sign-in comes back to: one port on
/// 127.0.0.1, and the code read off the request the browser makes to it.
final class Loopback {
    let fd: Int32
    let port: Int

    init() throws {
        let s = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
        if s < 0 { throw AuthError("cannot open a loopback port") }
        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = 0
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        var len = socklen_t(MemoryLayout<sockaddr_in>.size)
        let bound = withUnsafePointer(to: &addr) { p in p.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(s, $0, len) } }
        if bound != 0 || listen(s, 1) != 0 {
            Glibc.close(s)
            throw AuthError("cannot listen on a loopback port")
        }
        _ = withUnsafeMutablePointer(to: &addr) { p in p.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(s, $0, &len) } }
        fd = s
        port = Int(UInt16(bigEndian: addr.sin_port))
    }

    func close() { Glibc.close(fd) }

    // One request on the listener, which the server's redirect makes the
    // browser send: `GET /?code=… HTTP/1.1`.
    func waitForCode() throws -> String {
        while true {
            let c = accept(fd, nil, nil)
            if c < 0 { throw AuthError("the loopback port: \(String(cString: strerror(errno)))") }
            defer { Glibc.close(c) }
            var buf = [UInt8](repeating: 0, count: 8192)
            let n = buf.withUnsafeMutableBufferPointer { recv(c, $0.baseAddress!, $0.count, 0) }
            let text = n > 0 ? String(decoding: buf[0..<n], as: UTF8.self) : ""
            let line = text.components(separatedBy: "\r\n").first ?? ""
            let parts = line.split(separator: " ")
            let path = parts.count > 1 ? String(parts[1]) : ""
            let code = path.contains("?") ? Auth.queryValue(path, "code") : nil
            let (status, body) = code != nil ? ("200 OK", "signed in — you can close this tab") : ("404 Not Found", "nothing here")
            let bytes = Array(body.utf8)
            let head = "HTTP/1.1 \(status)\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: \(bytes.count)\r\nConnection: close\r\n\r\n"
            let reply = Array(head.utf8) + bytes
            _ = reply.withUnsafeBufferPointer { send(c, $0.baseAddress!, $0.count, Int32(MSG_NOSIGNAL)) }
            if let code = code { return code }
        }
    }
}
#endif
