// `harken-peer-swift`: `harken-peer` (harken/server/src/bin/harken-peer.rs)
// on the Swift client — a harken peer with no screen, speaking exactly its
// JSON lines, so that the fleet drives a Swift peer as it drives a Rust one
// (`docs/plan-phones.md` round 7, `docs/plan-fleet.md`). A port of the
// Kotlin one, `kotlin/harken-peer`, which is the same program again.
//
//     harken-peer-swift --dir DIR --server URL [--user NAME]     a peer of that server
//     harken-peer-swift --dir DIR --alone [--user NAME]          its own authority
//       [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
//       [--auth-patience-ms N] [--module FILE] [--roles ROLE,ROLE]
//
// The flags, the commands and the answers are the reference's, line for
// line; its first page is the documentation, and this file says only where
// it is not the same program:
//
// - **Nothing is native.** The reference runs harken's procedures natively
//   wherever a hash matches; a phone's runtime interprets every function of
//   the module it loads — `harken.ark`, installed beside this program
//   (`share/harken-peer-swift/harken.ark`), or the `.ark` file `--module`
//   names (a grown domain, an older revision's, a scoped one).
// - **The directory is the same records.** `DIR/replica` is the storage the
//   reference writes, byte for byte (`ArkDBClient`), and `DIR/login.json` and
//   `DIR/rejections.jsonl` are the same files.
// - **The socket is `NativeSocket` on Linux** (`ArkDBClient/Link.swift`
//   says why), `ws://` only.
// - What happens on the link goes to stderr prefixed `harken-peer-swift:`.
import ArkDB
import ArkDBClient
import Foundation
#if canImport(Glibc)
import Glibc
#endif

let usage = """
usage: harken-peer-swift --dir DIR (--server URL [--user NAME] | --alone [--user NAME])
                         [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
                         [--auth-patience-ms N] [--module FILE] [--roles ROLE,ROLE]
Commands are JSON lines on stdin; see harken/server/src/bin/harken-peer.rs.
"""

setvbuf(stdout, nil, _IOLBF, 0)

func say(_ line: String) {
    FileHandle.standardError.write(Data("harken-peer-swift: \(line)\n".utf8))
}

struct Failure: Error, CustomStringConvertible {
    let what: String
    init(_ what: String) { self.what = what }
    var description: String { what }
}

/// What the command line says.
struct Flags {
    var dir = ""
    var server: String? = nil
    var user: String? = nil
    var alone = false
    var pumpMs: Int64 = 50
    var timing = Timing()
    var patience = Patience.standard
    var module: String? = nil
    var roles = [String]()
}

func trimSlash(_ s: String) -> String {
    var t = Substring(s)
    while t.hasSuffix("/") { t = t.dropLast() }
    return String(t)
}

func flags(_ args: [String]) throws -> Flags {
    var f = Flags()
    var dir: String? = nil
    var i = 0
    func next(_ what: String) throws -> String {
        i += 1
        if i >= args.count { throw Failure(what) }
        return args[i]
    }
    func ms(_ name: String) throws -> Int64 {
        guard let n = Int64(try next("\(name) needs a value")), n >= 0 else { throw Failure("\(name) takes milliseconds") }
        return n
    }
    while i < args.count {
        let a = args[i]
        switch a {
        case "--dir": dir = try next("--dir needs a value")
        case "--server": f.server = trimSlash(try next("--server needs a value"))
        case "--user": f.user = try next("--user needs a value")
        case "--alone": f.alone = true
        case "--pump-ms": f.pumpMs = max(try ms("--pump-ms"), 1)
        case "--ping-ms": f.timing.pingEveryMs = try ms("--ping-ms")
        case "--backoff-ms":
            let parts = try next("--backoff-ms needs FIRST,MAX").split(separator: ",", maxSplits: 1, omittingEmptySubsequences: false)
            if parts.count != 2 { throw Failure("--backoff-ms takes FIRST,MAX") }
            let ns = try parts.map { p -> Int64 in
                guard let n = Int64(p.trimmingCharacters(in: .whitespaces)), n >= 0 else { throw Failure("--backoff-ms takes milliseconds") }
                return n
            }
            f.timing.firstBackoffMs = ns[0]
            f.timing.maxBackoffMs = ns[1]
        case "--auth-patience-ms": f.patience = Patience.of(max(try ms("--auth-patience-ms"), 1))
        case "--module": f.module = try next("--module needs a file")
        case "--roles": f.roles = try next("--roles needs ROLE,ROLE").split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        case "-h", "--help": throw Failure(usage)
        default: throw Failure("unexpected argument \(a)\n\(usage)")
        }
        i += 1
    }
    guard let d = dir else { throw Failure("--dir is required\n\(usage)") }
    f.dir = d
    if f.alone && f.server != nil { throw Failure("--alone has no server") }
    if !f.alone && f.server == nil { throw Failure("--server or --alone\n\(usage)") }
    return f
}

// -- the login, remembered under DIR ---------------------------------------------

func loginFile(_ dir: String) -> String { dir + "/login.json" }

/// `DIR/login.json`, `{"server":…,"login":…}`: the login for one server.
func recall(_ dir: String, _ server: String) -> Login? {
    guard let d = FileManager.default.contents(atPath: loginFile(dir)), let v = try? Json.decode(bytes: [UInt8](d)), case .record(let m) = v, m["server"] == .text(server), let l = m["login"] else { return nil }
    return try? Login.of(l)
}

func remember(_ dir: String, _ server: String, _ login: Login) throws {
    let tmp = dir + "/.login.json.tmp"
    let text = "{\"server\":\(Json.quoted(server)),\"login\":\(login.toJson())}"
    guard FileManager.default.createFile(atPath: tmp, contents: Data(text.utf8)) else { throw Failure("\(tmp): cannot write") }
    if rename(tmp, loginFile(dir)) != 0 { throw Failure("\(loginFile(dir)): cannot rename") }
}

func forget(_ dir: String) { try? FileManager.default.removeItem(atPath: loginFile(dir)) }

/// Sign `user` in at `server`: a dev server answers with a code and no page.
func logIn(_ server: String, _ user: String, _ patience: Patience) throws -> Login {
    #if os(Linux)
    return try Auth.login(server, user, patience: patience) { url in say("open \(url) to sign in") }
    #else
    return try Auth.devLogin(server, user, patience: patience)
    #endif
}

// -- a command, and its answer -----------------------------------------------------

/// One line from stdin, read.
enum Cmd: Equatable {
    case mutate(String, [String: Value])
    case status
    case hash
    case wait(Int64, Int64)
    case settle(Int64)
    case query(String, [String: Value])
    case rejections(Bool)
    case standing(Id)
    case disconnect
    case reconnect
    case signIn(String)
    case signOut
    case join(String, String?)
    case leave
    case persist
    case quit

    static func == (a: Cmd, b: Cmd) -> Bool {
        if case .quit = a, case .quit = b { return true }
        return false
    }
}

/// How long a `wait` or `settle` waits when the line does not say.
let defaultTimeoutMs: Int64 = 30_000

func parse(_ line: String) throws -> Cmd {
    let v: Value
    do {
        v = try Json.decode(line)
    } catch let e as JsonError {
        throw Failure("not JSON in the dialect: \(e.why) at byte \(e.at)")
    }
    guard case .record(let m) = v else { throw Failure("a command is an object") }
    func text(_ k: String) throws -> String {
        switch m[k] {
        case .text(let t)?: return t
        case nil: throw Failure("`\(k)` is missing")
        default: throw Failure("`\(k)` is text")
        }
    }
    func int(_ k: String) throws -> Int64? {
        switch m[k] {
        case .int(let n)?: return n
        case nil: return nil
        default: throw Failure("`\(k)` is an integer")
        }
    }
    func args() throws -> [String: Value] {
        switch m["args"] {
        case .record(let r)?:
            var out = [String: Value]()
            for (k, x) in r.pairs { out[k] = x }
            return out
        case nil: return [:]
        default: throw Failure("`args` is an object")
        }
    }
    func timeout() throws -> Int64 { try int("timeout_ms").map { max($0, 0) } ?? defaultTimeoutMs }
    let c = try text("cmd")
    switch c {
    case "mutate": return .mutate(try text("name"), try args())
    case "status": return .status
    case "hash": return .hash
    case "wait":
        guard let n = try int("cursor") else { throw Failure("`cursor` is missing") }
        return .wait(n, try timeout())
    case "settle": return .settle(try timeout())
    case "query": return .query(try text("name"), try args())
    case "rejections": return .rejections(m["all"] == .bool(true))
    case "standing":
        guard case .id(let i)? = m["id"] else { throw Failure("`id` is an id: {\"$id\":\"8-4-4-4-12\"}") }
        return .standing(i)
    case "disconnect": return .disconnect
    case "reconnect": return .reconnect
    case "sign_in": return .signIn(try text("user"))
    case "sign_out": return .signOut
    case "join":
        let user: String?
        switch m["user"] {
        case nil, .null?: user = nil
        default: user = try text("user")
        }
        return .join(trimSlash(try text("server")), user)
    case "leave": return .leave
    case "persist": return .persist
    case "quit": return .quit
    default: throw Failure("no command \(Json.quoted(c))")
    }
}

/// An answer, a field at a time, each already printed: `{"k":v,…}` in the order written.
struct Answer {
    private var fields = [(String, String)]()

    func raw(_ k: String, _ v: String) -> Answer {
        var a = self
        a.fields.append((k, v))
        return a
    }

    func num(_ k: String, _ n: Int64) -> Answer { raw(k, String(n)) }
    func num(_ k: String, _ n: Int) -> Answer { raw(k, String(n)) }
    func text(_ k: String, _ t: String) -> Answer { raw(k, Json.quoted(t)) }
    func value(_ k: String, _ v: Value) -> Answer { raw(k, Json.json(v)) }

    func line() -> String { "{" + fields.map { "\(Json.quoted($0.0)):\($0.1)" }.joined(separator: ",") + "}" }

    static func ok(_ ok: Bool) -> Answer { Answer().raw("ok", ok ? "true" : "false") }
    static func refused(_ why: String) -> Answer { ok(false).text("why", why) }
}

func why(_ e: Error) -> String {
    switch e {
    case let r as Refused: return r.description
    case let p as PeerError: return p.what
    case let s as StorageError: return s.what
    case let a as AuthError: return a.what
    default: return "\(e)"
    }
}

func nowNs() -> UInt64 { DispatchTime.now().uptimeNanoseconds }

// -- the peer ------------------------------------------------------------------------

final class Headless {
    let dir: String
    var server: String?
    private let patience: Patience
    let pumpMs: Int64
    /// Verdicts collected and not yet asked for; each also appended to `DIR/rejections.jsonl`.
    private var unasked = [String]()
    /// `--roles`: what this device's `Ctx` carries beside its login's.
    private let roles: [String]
    let peer: Peer

    init(_ f: Flags, _ domain: Domain) throws {
        dir = f.dir
        server = f.server
        patience = f.patience
        pumpMs = f.pumpMs
        roles = f.roles
        do {
            try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        } catch {
            throw PeerError("\(dir): cannot make it")
        }
        var loginRoles = [String]()
        var opts: Options
        if f.alone {
            opts = Options.alone(f.user ?? "me")
        } else {
            let s = f.server!
            var login = recall(dir, s).flatMap { l in f.user == nil || f.user == l.user.id ? l : nil }
            if login == nil, let u = f.user {
                do {
                    let l = try logIn(s, u, patience)
                    try remember(dir, s, l)
                    login = l
                } catch let e as AuthError {
                    throw PeerError("signing \(u) in at \(s): \(e.what)")
                }
            }
            if let l = login {
                loginRoles = l.user.roles
                opts = Options.server(l.user.id, l.session, l.token)
            } else {
                opts = Options.signedOut()
            }
        }
        opts.timing = f.timing
        peer = try Peer.open(domain, DirStorage(dir + "/replica"), opts)
        peer.setRoles(loginRoles + roles)
        if let s = f.server { peer.connect(Auth.socketUrl(s)) }
    }

    /// One turn, with whatever it came to said on stderr.
    func pump() {
        let p = peer.pump()
        if p.opened { say("linked (epoch \(peer.epoch), cursor \(peer.cursor))") }
        if let d = p.dropped { say("dropped: \(d)") }
        if let d = p.denied { say("denied: \(d)") }
        if let n = p.note { say(n) }
        // Nothing here listens to the room; what it said is not kept.
        _ = peer.heard()
        collect()
    }

    /// Take the verdicts that arrived, and write each down.
    private func collect() {
        let fresh = peer.takeRejections().map { Answer().value("id", .id($0.id)).text("reason", $0.reason).line() }
        if fresh.isEmpty { return }
        let file = dir + "/rejections.jsonl"
        let bytes = Data((fresh.joined(separator: "\n") + "\n").utf8)
        if let h = FileHandle(forWritingAtPath: file) {
            h.seekToEndOfFile()
            h.write(bytes)
            h.closeFile()
        } else if !FileManager.default.createFile(atPath: file, contents: bytes) {
            say("\(file): cannot write")
        }
        unasked += fresh
    }

    /// Pump until `done`, or the timeout. Whether it was done.
    private func pumpUntil(_ timeoutMs: Int64, _ done: (Peer) -> Bool) -> Bool {
        let deadline = nowNs() + UInt64(timeoutMs) * 1_000_000
        while true {
            pump()
            if done(peer) { return true }
            let now = nowNs()
            if now >= deadline { return false }
            let left = Double(deadline - now) / 1e9
            Thread.sleep(forTimeInterval: min(Double(pumpMs) / 1000, left))
        }
    }

    private func status() -> Answer {
        let s = peer.status()
        return Answer()
            .num("cursor", s.cursor)
            .num("pending", s.pending)
            .num("held", s.held)
            .raw("behind", s.behind ? "true" : "false")
            .raw("linked", s.linked ? "true" : "false")
            .raw("denied", s.denied.map(Json.quoted) ?? "null")
            .text("user", s.user)
            .text("session", s.session)
            .raw("signed_out", s.signedOut ? "true" : "false")
            .num("epoch", s.epoch)
            .text("link", s.link)
            .num("opens", s.opens)
            .num("diverged", s.diverged)
            .num("joining", s.joining)
            .num("fork", s.fork.cursor)
    }

    /// Answer one command. `quit` is answered like `persist`; the loop is what stops.
    func handle(_ cmd: Cmd) -> Answer {
        switch cmd {
        case .mutate(let name, let args):
            do {
                return Answer.ok(true).value("id", .id(try peer.mutate(name, args)))
            } catch {
                return Answer.refused(why(error))
            }
        case .status: return status()
        case .hash:
            let r = peer.replica
            return Answer().num("cursor", r.cursor).text("hash", Hex.encode(Hash.stateHash(r.confirmed))).text("view", Hex.encode(Hash.stateHash(r.view)))
        case .wait(let cursor, let timeout):
            let ok = pumpUntil(timeout) { $0.cursor >= cursor }
            return Answer.ok(ok).num("cursor", peer.cursor)
        case .settle(let timeout):
            // Linked, nothing pending, and the cursor still for two pumps:
            // what "caught up" can mean from this end alone.
            var last: Int64 = -1
            var still = 0
            let ok = pumpUntil(timeout) { p in
                if p.cursor == last {
                    still += 1
                } else {
                    last = p.cursor
                    still = 0
                }
                return p.linked && p.pendingLen == 0 && still >= 2
            }
            return Answer.ok(ok).num("cursor", peer.cursor).num("pending", peer.pendingLen)
        case .query(let name, let args):
            do {
                return Answer().value("rows", try peer.query(name, args))
            } catch {
                return Answer.refused(why(error))
            }
        case .rejections(let all):
            collect()
            let asked = unasked
            unasked.removeAll()
            var items = asked
            if all {
                let text = FileManager.default.contents(atPath: dir + "/rejections.jsonl").map { String(decoding: $0, as: UTF8.self) } ?? ""
                items = text.split(separator: "\n", omittingEmptySubsequences: true).map(String.init)
            }
            return Answer().raw("rejections", Json.array(items))
        case .standing(let id):
            switch peer.standing(id) {
            case .pending: return Answer().text("standing", "pending")
            case .confirmed: return Answer().text("standing", "confirmed")
            case .rejected(let w): return Answer().text("standing", "rejected").text("why", w)
            case .unknown: return Answer().text("standing", "unknown")
            }
        case .disconnect:
            peer.disconnect()
            return Answer.ok(true)
        case .reconnect:
            peer.reconnect()
            return Answer.ok(true)
        case .signIn(let user): return signIn(user)
        case .signOut:
            peer.signOut()
            forget(dir)
            return Answer.ok(true)
        case .join(let to, let user): return join(to, user)
        case .leave:
            do {
                try peer.leave()
                server = nil
                return Answer.ok(true).num("cursor", peer.cursor)
            } catch {
                return Answer.refused(why(error))
            }
        case .persist, .quit:
            pump()
            do {
                try peer.persist()
                return Answer.ok(true)
            } catch {
                return Answer.refused(why(error))
            }
        }
    }

    private func signIn(_ user: String) -> Answer {
        guard let s = server else { return Answer.refused("a peer alone has no server to sign in to") }
        let login: Login
        do {
            login = try logIn(s, user, patience)
        } catch {
            return Answer.refused("signing \(user) in at \(s): \(why(error))")
        }
        do {
            try remember(dir, s, login)
        } catch {
            say("the login is not remembered: \(why(error))")
        }
        do {
            try peer.signIn(login.user.id, login.session, login.token)
            peer.setRoles(login.user.roles + roles)
            return Answer.ok(true).text("user", login.user.id).text("session", login.session)
        } catch {
            return Answer.refused(why(error))
        }
    }

    private func join(_ to: String, _ user: String?) -> Answer {
        var login: Login? = nil
        if let u = user {
            do {
                login = try logIn(to, u, patience)
            } catch {
                return Answer.refused("signing \(u) in at \(to): \(why(error))")
            }
            do {
                try remember(dir, to, login!)
            } catch {
                say("the login is not remembered: \(why(error))")
            }
        }
        let took = nowNs()
        do {
            try peer.join(Auth.socketUrl(to), login)
            peer.setRoles((login?.user.roles ?? []) + roles)
            say("joined \(to): \(peer.status().joining) re-queued in \((nowNs() - took) / 1_000_000)ms")
            server = to
            return Answer.ok(true).num("pending", peer.pendingLen).num("cursor", peer.cursor)
        } catch {
            return Answer.refused(why(error))
        }
    }
}

/// Where this program is, so that the module installed beside it is found.
func executableDir() -> String {
    #if os(Linux)
    if let p = try? FileManager.default.destinationOfSymbolicLink(atPath: "/proc/self/exe") { return (p as NSString).deletingLastPathComponent }
    #endif
    return ((Bundle.main.executablePath ?? CommandLine.arguments[0]) as NSString).deletingLastPathComponent
}

/// The module: `--module FILE`, or harken's own, installed beside this program
/// (`share/harken-peer-swift/harken.ark`) — or, run from a checkout, the one
/// the repository carries.
func domain(_ module: String?) throws -> Domain {
    var path = module
    if path == nil {
        let installed = executableDir() + "/../share/harken-peer-swift/harken.ark"
        let checkout = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../../harken/domain/harken.ark").path
        path = [installed, checkout].first { FileManager.default.fileExists(atPath: $0) }
        if path == nil { throw PeerError("this build carries no harken.ark (looked at \(installed))") }
    }
    guard let d = FileManager.default.contents(atPath: path!) else { throw PeerError("the module \(path!): cannot read it") }
    do {
        return try Domain.fromBytes([UInt8](d))
    } catch {
        throw PeerError("the module \(module ?? "harken.ark"): \(error)")
    }
}

/// Every line of stdin, on a thread of its own, so the peer pumps while nothing is said.
final class Lines {
    private let cond = NSCondition()
    private var queue = [String]()
    private var ended = false

    init() {
        let t = Thread { [self] in
            while let l = readLine(strippingNewline: true) {
                cond.lock()
                queue.append(l)
                cond.signal()
                cond.unlock()
            }
            cond.lock()
            ended = true
            cond.signal()
            cond.unlock()
        }
        t.start()
    }

    enum Got {
        case line(String)
        case end
        case nothing
    }

    /// A line, the end, or nothing by `deadline`.
    func take(until deadline: Date) -> Got {
        cond.lock()
        defer { cond.unlock() }
        while queue.isEmpty && !ended {
            if !cond.wait(until: deadline) { return .nothing }
        }
        if !queue.isEmpty { return .line(queue.removeFirst()) }
        return .end
    }
}

func run(_ f: Flags) throws {
    let h = try Headless(f, try domain(f.module))
    let input = Lines()
    // The pump keeps its own clock rather than waiting for a quiet stdin: a
    // driver asking `status` every ten milliseconds would otherwise be a
    // peer that never pumps.
    var due = Date()
    while true {
        if Date() >= due {
            h.pump()
            due = Date().addingTimeInterval(Double(h.pumpMs) / 1000)
        }
        switch input.take(until: due) {
        case .nothing: continue
        case .end:
            _ = h.handle(.quit)
            try h.peer.close()
            return
        case .line(let line):
            if line.trimmingCharacters(in: .whitespaces).isEmpty { continue }
            var quit = false
            let answer: Answer
            do {
                let cmd = try parse(line)
                quit = cmd == .quit
                answer = h.handle(cmd)
            } catch {
                answer = Answer.refused(why(error))
            }
            // A reader that has gone is a driver that has gone: stop, with
            // what is pending written down.
            let written = fputs(answer.line() + "\n", stdout) >= 0 && fflush(stdout) == 0
            if !written || quit {
                try h.peer.close()
                return
            }
        }
    }
}

signal(SIGPIPE, SIG_IGN)
let f: Flags
do {
    f = try flags(Array(CommandLine.arguments.dropFirst()))
} catch {
    say(why(error))
    exit(2)
}
do {
    try run(f)
} catch {
    say(why(error))
    exit(1)
}
exit(0)
