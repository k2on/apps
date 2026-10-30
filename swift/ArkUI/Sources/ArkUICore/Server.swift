// Finding an Ark app's server from what somebody typed, any app's: the
// address read (`Address`), its `/healthz` asked (`Probe`) and what it
// answered decided (`Health`). Foundation alone, so an app's onboarding is
// type-checked and tested on Linux against it. The same rules as harken's
// own onboarding (harken/ios/Harken/Onboarding.swift), which predates this
// and keeps its copy; money's is this one.
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// What the server field came to: the URLs to try, in order, or why there are none.
///
/// One field takes `host`, `host:port` or a whole URL. A URL with a scheme
/// is taken at its word. Anything else is tried over `https` first, and over
/// `http` as well only where the address is one a certificate is unlikely to
/// cover and nobody outside can reach: loopback, the private ranges,
/// link-local, the carrier-grade range a tailnet uses, and a name with no dot
/// or one under `.local`, `.lan`, `.home.arpa` or `.internal`.
public enum Address: Equatable, Sendable {
    /// Base URLs with no trailing slash, `https` first.
    case candidates(urls: [String], host: String)
    case invalid(String)

    public static func parse(_ typed: String) -> Address {
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
    public static func isLocal(_ host: String) -> Bool {
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

/// What a request came back with, or why it did not.
public enum Answer: Equatable, Sendable {
    case answered(status: Int, body: String)
    /// No answer: the sentence for why ("nothing answered there").
    case unreachable(String)
}

/// Where looking for a server ended.
public enum Found: Equatable, Sendable {
    /// The base URL to sign in at and sync with; `newer` when it runs another schema of the domain.
    case server(String, newer: Bool)
    case failed(String)
}

/// Whether an address is an Ark server this app can use, from what its `/healthz` answers.
public enum Health: Equatable, Sendable {
    case serving(newer: Bool)
    case refused(String)

    /// `ok`, its connections, the log's head and horizon, and `domain <schema
    /// hash> <version> current`; `domain` is this app's own line
    /// (`<app>Domain()`), and `app` what to call the app in a refusal.
    public static func decide(_ host: String, status: Int, body: String, domain: String, app: String = "this app") -> Health {
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

public enum Probe {
    /// Ask `/healthz` of each URL `typed` could mean, in order, with `get`
    /// making the request: the first answer decides — an `https` server that
    /// answers and is the wrong app is not tried again in the clear.
    public static func find(_ typed: String, domain: String, get: (String) async -> Answer) async -> Found {
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
    public static func display(_ url: String) -> String {
        url.range(of: "://").map { String(url[$0.upperBound...]) } ?? url
    }

    /// A GET asking for JSON, ten seconds to connect and ten to answer: what
    /// an app hands `find` as its `get`.
    public static func get(_ url: String) async -> Answer {
        guard let u = URL(string: url) else { return .unreachable("that isn't a URL") }
        var r = URLRequest(url: u, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10)
        r.setValue("application/json", forHTTPHeaderField: "Accept")
        return await withCheckedContinuation { done in
            URLSession.shared.dataTask(with: r) { data, response, error in
                if let e = error as? URLError { return done.resume(returning: .unreachable(reason(e))) }
                if let e = error { return done.resume(returning: .unreachable("\(e.localizedDescription)")) }
                let status = (response as? HTTPURLResponse)?.statusCode ?? 0
                done.resume(returning: .answered(status: status, body: String(decoding: data ?? Data(), as: UTF8.self)))
            }.resume()
        }
    }

    /// Why a request failed, in words.
    public static func reason(_ e: URLError) -> String {
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

    /// The code in `<scheme>://auth?code=…`, the sign-in coming back, or none for any other link.
    public static func code(_ url: URL, scheme: String) -> String? {
        guard url.scheme == scheme, url.host == "auth" else { return nil }
        let c = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "code" }?.value
        return (c?.isEmpty ?? true) ? nil : c
    }
}

/// A key/value store of strings; nil removes. `UserDefaults` in an app, a dictionary in a test.
public protocol Shelf: AnyObject {
    func get(_ key: String) -> String?
    func put(_ key: String, _ value: String?)
}

public final class MemoryShelf: Shelf {
    public var map = [String: String]()
    public init() {}
    public func get(_ key: String) -> String? { map[key] }
    public func put(_ key: String, _ value: String?) { map[key] = value }
}

public final class DefaultsShelf: Shelf {
    private let d: UserDefaults
    public init(_ d: UserDefaults = .standard) { self.d = d }
    public func get(_ key: String) -> String? { d.string(forKey: key) }
    public func put(_ key: String, _ value: String?) {
        if let v = value { d.set(v, forKey: key) } else { d.removeObject(forKey: key) }
    }
}
