// The client's driver half, held without a network, as
// `kotlin/ark-client/.../ClientTests.kt` holds the Kotlin one: the link's
// backoff and its stops (the reference's
// `a_link_backs_off_doubling_and_resets_when_it_opens`, over `Queues`), the
// JSON dialect a headless peer reads and answers in, the sign-in's URLs, a
// maintained view over harken's own module — the list a screen splices, held
// after every change to a fresh read — and, on Linux, the native WebSocket:
// its frames and accept key against RFC 6455's own examples, and a
// connection to a loopback server in this process.
import ArkDB
import ArkDBClient
import Foundation
#if os(Linux)
import Glibc
#endif

enum ClientTests {
    static var problems = [String]()

    static func claim(_ what: @autoclosure () -> String, _ ok: Bool) {
        if !ok { problems.append(what()) }
    }

    /// Every dial hands back a fresh `Queues` and records it.
    static func link() {
        var made = [Queues]()
        let l = Link("ws://x/sync", { _ in
            let q = Queues()
            made.append(q)
            return q
        }, Timing())
        _ = l.pollAt(0)
        claim("link: dials at once", made.count == 1)
        made[0].push(.closed("refused"))
        let p = l.pollAt(0)
        claim("link: a socket that never opened is not a disconnect", p.closed == nil)
        claim("link: waits the first backoff, \(l.state)", l.state == .waiting(500))
        _ = l.pollAt(499)
        claim("link: not before the backoff", made.count == 1)
        _ = l.pollAt(500)
        claim("link: dials at the backoff", made.count == 2)
        made[1].push(.closed("refused"))
        _ = l.pollAt(500)
        claim("link: the backoff doubled, \(l.state)", l.state == .waiting(1500))
        _ = l.pollAt(1500)
        let q = made[2]
        q.push(.opened)
        q.push(.frame([1]))
        let o = l.pollAt(1500)
        claim("link: opened", o.opened && l.opens == 1)
        claim("link: a frame", o.frames == [[1]])
        l.send([9])
        claim("link: sent on the open socket", q.takeSent() == [[9]])
        q.push(.closed("gone"))
        let c = l.pollAt(2000)
        claim("link: a disconnect, \(String(describing: c.closed))", c.closed == "gone")
        claim("link: the backoff started over when it opened, \(l.state)", l.state == .waiting(2500))
        claim("link: it was not open", !l.stop("offline"))
        _ = l.pollAt(99_999)
        claim("link: a stopped link does not dial", made.count == 3)
        l.resume()
        _ = l.pollAt(99_999)
        claim("link: resumed, it dials", made.count == 4)
        print("client: the link backs off doubling, resets when it opens, and stops")
    }

    /// The dialect: every wrapper, an escape of each kind, a key order not insertion order.
    static func json() {
        let v = Value.record(Record([("b", .list([.int(-1), .bytes([0xab, 1]), .id(Id.nilId)])), ("a", .text("q\"\\\u{1f}水")), ("c", .null), ("d", .bool(true))]))
        let printed = Json.json(v)
        claim("json: printed \(printed)", printed == #"{"a":"q\"\\\u001f水","b":[{"$int":"-1"},{"$bytes":"ab01"},{"$id":"00000000-0000-0000-0000-000000000000"}],"c":null,"d":true}"#)
        claim("json: read back", (try? Json.decode(printed)) == v)
        claim("json: a bare integer", (try? Json.decode(" { \"n\" : 3 } ")) == .record(Record([("n", .int(3))])))
        claim("json: a surrogate pair", (try? Json.decode("\"\\ud83c\\udfb5\"")) == .text("\u{1f3b5}"))
        for (bad, says) in [("1.5", "no floats"), (#"{"$int":3}"#, "wraps text"), (#"{"$int":"x"}"#, "a decimal integer"), (#"{"a":1,"a":2}"#, "a key twice"), ("[1,]", "not a value"), (#""\ud83c""#, "half a surrogate pair"), ("1 2", "something after the value")] {
            do {
                _ = try Json.decode(bad)
                claim("json: \(bad) is read", false)
            } catch let e as JsonError {
                claim("json: \(bad) is refused with \(says), \(e.why)", e.why.contains(says))
            } catch {
                claim("json: \(bad): \(error)", false)
            }
        }
        print("client: the dialect prints and reads as the reference does")
    }

    /// The sign-in's URLs, as `ark_auth` spells them.
    static func auth() {
        claim("auth: login url", Auth.loginUrl("http://h:1/", "http://127.0.0.1:9/", "a b") == "http://h:1/auth/login?redirect=http%3A%2F%2F127.0.0.1%3A9%2F&user=a%20b")
        claim("auth: socket of http", Auth.socketUrl("http://h:1") == "ws://h:1/sync")
        claim("auth: socket of https", Auth.socketUrl("https://h/") == "wss://h/sync")
        claim("auth: a code", Auth.queryValue("?code=a%2Bb&x=1", "code") == "a+b")
        let l = Login(token: "t", session: "s", user: Account(id: "alice", roles: ["library"]), expiresMs: 7)
        claim("auth: a login read back", (try? Login.parse(l.toJson())) == l)
        claim("auth: no roles, no key", !Login(token: "t", session: "s", user: Account(id: "bob"), expiresMs: 7).toJson().contains("roles"))
        print("client: the sign-in's URLs and its login as ark_auth writes them")
    }

    /// A peer alone over harken.ark: `playlists` held as a view, and after
    /// every mutation the patches spliced into a list of the screen's own are
    /// the fresh read — through a create, a refused create and an add of nothing.
    static func views(_ harken: String) throws {
        let domain = try Domain.fromBytes(try readFile(harken))
        var o = Options.alone("me")
        o.autos = .seeded(7)
        let p = try Peer.open(domain, MemoryStorage(), o)
        let playlists = try p.view("playlists", [:])
        var shown = playlists.rows
        var patched = 0
        func step(_ what: String) throws {
            switch try playlists.update(p, p.takeChanges()) {
            case .patched(let ps):
                patched += 1
                ClientView.splice(&shown, ps) { $0 }
            case .reset: shown = playlists.rows
            case .unchanged: break
            }
            let fresh = try p.query("playlists", [:]).list()
            claim("views: \(what): the view is the fresh read", playlists.rows == fresh)
            claim("views: \(what): the spliced list is the fresh read", shown == fresh)
        }
        try p.mutate("create_playlist", ["name": .text("Road trip")])
        try step("a playlist made")
        do {
            try p.mutate("create_playlist", ["name": .text("Road trip")])
        } catch is Refused {}
        try step("the same name again")
        guard let first = playlists.rows.first else {
            claim("views: the playlist made is not in the view", false)
            return
        }
        let id = try first.field("id")
        let songs = try p.view("playlist", ["playlist_id": id])
        claim("views: an empty playlist", songs.rows.isEmpty)
        // Nothing to put on it: the library is the scanner's, and a peer alone
        // has none. `add_to_playlist` of a missing medium is a no-op, and says so
        // by changing nothing.
        _ = try? p.mutate("add_to_playlist", ["playlist_id": id, "media_id": .id(Id.nilId)])
        let u = try songs.update(p, .applied([]))
        claim("views: nothing moved, nothing patched", u == .unchanged)
        try step("an add of nothing")
        claim("views: patched, not re-read: \(patched)", patched == 2)
        // §1.7 Whoever is signed in is the middleware's input: another `Ctx`
        // re-runs it and reads the list again, though nothing moved.
        p.setRoles(["listener"])
        claim("views: another Ctx re-reads", try songs.update(p, .applied([])) == .reset)
        claim("views: the same Ctx again does not", try songs.update(p, .applied([])) == .unchanged)
        do {
            _ = try p.view("playlist", ["playlist_id": .id(Id.nilId)])
            claim("views: a view the middleware refuses is refused at open", false)
        } catch is Refused {}
        print("client: views over harken.ark patch as a fresh read does (\(patched) patched)")
    }

    /// RFC 6455's own examples: the accept key of §1.3, and the frames of §5.7.
    static func wire() {
        claim("ws: the accept key of RFC 6455 §1.3", WsWire.acceptKey("dGhlIHNhbXBsZSBub25jZQ==") == "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        // §5.7: "Hello", unmasked and masked with 37 fa 21 3d.
        let unmasked: [UInt8] = [0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f]
        let masked: [UInt8] = [0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58]
        for (what, f) in [("unmasked", unmasked), ("masked", masked)] {
            let got = WsWire.parse(f, 0)
            claim("ws: §5.7's \(what) Hello", got.map { $0.0 && $0.1 == 1 && $0.2 == Array("Hello".utf8) && $0.3 == f.count } == true)
        }
        claim("ws: half a frame is not a frame", WsWire.parse(Array(masked.dropLast()), 0) == nil)
        for n in [0, 125, 126, 65535, 65536, 70000] {
            let payload = (0..<n).map { UInt8(truncatingIfNeeded: $0 &* 31) }
            let f = WsWire.frame(2, payload)
            let got = WsWire.parse(f, 0)
            claim("ws: a \(n)-byte frame, masked, round-trips", got.map { $0.0 && $0.1 == 2 && $0.2 == payload && $0.3 == f.count } == true)
            claim("ws: a \(n)-byte frame is masked", f[1] & 0x80 != 0)
        }
        #if os(Linux)
        let e = NativeSocket.endpoint("ws://127.0.0.1:8787/sync")
        claim("ws: an endpoint", e.map { $0.host == "127.0.0.1" && $0.port == "8787" && $0.target == "/sync" } == true)
        claim("ws: the scheme's port", NativeSocket.endpoint("ws://h.example").map { $0.port == "80" && $0.target == "/" } == true)
        claim("ws: an IPv6 host", NativeSocket.endpoint("ws://[::1]:9/sync").map { $0.host == "::1" && $0.port == "9" } == true)
        claim("ws: wss is refused here", NativeSocket.endpoint("wss://h/sync") == nil)
        claim("ws: http is not a WebSocket", NativeSocket.endpoint("http://h/sync") == nil)
        #endif
        print("client: the WebSocket wire as RFC 6455's examples spell it")
    }

    #if os(Linux)
    /// A loopback WebSocket server in this process: it accepts one
    /// connection, answers the handshake, pings, echoes every binary frame
    /// back doubled, and closes when it is sent `[0]`. The native socket must
    /// open, send, receive, answer the ping and close.
    static func native() throws {
        let s = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        var len = socklen_t(MemoryLayout<sockaddr_in>.size)
        _ = withUnsafePointer(to: &addr) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(s, $0, len) } }
        _ = listen(s, 1)
        _ = withUnsafeMutablePointer(to: &addr) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(s, $0, &len) } }
        let port = UInt16(bigEndian: addr.sin_port)
        let ponged = NSLock()
        var pong = false
        let server = Thread {
            let c = accept(s, nil, nil)
            defer {
                Glibc.close(c)
                Glibc.close(s)
            }
            var buf = [UInt8]()
            func more() -> Bool {
                var chunk = [UInt8](repeating: 0, count: 65536)
                let n = chunk.withUnsafeMutableBufferPointer { recv(c, $0.baseAddress!, $0.count, 0) }
                if n <= 0 { return false }
                buf += chunk[0..<n]
                return true
            }
            func write(_ b: [UInt8]) { _ = b.withUnsafeBufferPointer { send(c, $0.baseAddress!, $0.count, Int32(MSG_NOSIGNAL)) } }
            while String(decoding: buf, as: UTF8.self).range(of: "\r\n\r\n") == nil { if !more() { return } }
            let head = String(decoding: buf, as: UTF8.self)
            buf.removeAll()
            let key = head.components(separatedBy: "\r\n").first { $0.lowercased().hasPrefix("sec-websocket-key:") }!.dropFirst(18).trimmingCharacters(in: .whitespaces)
            write(Array("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: \(WsWire.acceptKey(key))\r\n\r\n".utf8))
            // A server's frames are not masked.
            write([0x89, 0x02, 0x68, 0x69])
            while true {
                guard let (_, op, payload, size) = WsWire.parse(buf, 0) else {
                    if !more() { return }
                    continue
                }
                buf.removeFirst(size)
                if op == 10 && payload == Array("hi".utf8) {
                    ponged.lock()
                    pong = true
                    ponged.unlock()
                } else if op == 2 && payload == [0] {
                    // The close, then the client's answer read before the
                    // socket goes, so nothing it says meets a reset.
                    write([0x88, 0x02, 0x03, 0xe8])
                    shutdown(c, Int32(SHUT_WR))
                    while more() {}
                    return
                } else if op == 2 {
                    let back = payload + payload
                    write([0x82, UInt8(back.count)] + back)
                }
            }
        }
        server.start()
        let sock = NativeSocket("ws://127.0.0.1:\(port)/sync", Timing(connectTimeoutMs: 2000))
        var got = [LinkEvent]()
        let deadline = Link.monotonicMs() + 5000
        var sent = false
        var asked = false
        while Link.monotonicMs() < deadline {
            let es = sock.poll()
            got += es
            if !sent && got.contains(.opened) {
                sock.send([1, 2])
                sent = true
            }
            if !asked && got.contains(.frame([1, 2, 1, 2])) {
                sock.send([0])
                asked = true
            }
            if got.contains(where: { if case .closed = $0 { return true } else { return false } }) { break }
            usleep(5000)
        }
        claim("native: opened, \(got)", got.first == .opened)
        claim("native: a frame sent and echoed, \(got)", got.contains(.frame([1, 2, 1, 2])))
        claim("native: closed by the server, \(got)", got.last == .closed("closed by the server"))
        ponged.lock()
        claim("native: the server's ping answered", pong)
        ponged.unlock()
        let refused = NativeSocket("ws://127.0.0.1:1/sync", Timing(connectTimeoutMs: 500))
        var why = [LinkEvent]()
        let until = Link.monotonicMs() + 3000
        while why.isEmpty && Link.monotonicMs() < until {
            why = refused.poll()
            usleep(5000)
        }
        claim("native: a refused connection is one closed, \(why)", why.count == 1 && { if case .closed = why[0] { return true } else { return false } }())
        print("client: the native WebSocket opens, sends, receives, answers a ping and closes, against a loopback server")
    }
    #endif

    static func run(_ harken: String) -> [String] {
        problems = []
        link()
        json()
        auth()
        wire()
        do { try views(harken) } catch { problems.append("views: \(error)") }
        #if os(Linux)
        do { try native() } catch { problems.append("native: \(error)") }
        #endif
        return problems
    }
}
