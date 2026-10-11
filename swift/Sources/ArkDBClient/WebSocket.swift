// The two WebSockets a `Link` dials (`Link.swift`): binary frames each way,
// nothing else.
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// A WebSocket over `URLSessionWebSocketTask`: `ws://` and `wss://`, the
/// platform's TLS and proxies, pinging every `Timing.pingEveryMs`. What the
/// session reports from its own queue is kept until the link polls. The
/// `async` send and receive are the ones Linux Foundation makes public too,
/// so this compiles — and is type-checked — on Linux, where a `ws://` task
/// fails at once and `platformDial` takes `NativeSocket` instead.
public final class SessionSocket: NSObject, Socket, URLSessionWebSocketDelegate {
    private let queue = EventQueue()
    private var session: URLSession? = nil
    private var task: URLSessionWebSocketTask? = nil
    private var pinger: DispatchSourceTimer? = nil
    private let lock = NSLock()

    public init(_ url: String, _ timing: Timing) {
        super.init()
        guard let u = URL(string: url) else {
            queue.push(.closed("\(url) is not a URL"))
            return
        }
        let config = URLSessionConfiguration.default
        // The handshake is held to the connect patience, as the reference's
        // native transport holds it; an open socket waits on the keepalive.
        config.timeoutIntervalForRequest = Double(timing.connectTimeoutMs) / 1000
        let s = URLSession(configuration: config, delegate: self, delegateQueue: nil)
        let t = s.webSocketTask(with: u)
        session = s
        task = t
        t.resume()
        receive(t)
        let p = DispatchSource.makeTimerSource(queue: DispatchQueue.global())
        let every = Int(max(timing.pingEveryMs, 1))
        p.schedule(deadline: .now() + .milliseconds(every), repeating: .milliseconds(every))
        p.setEventHandler { [weak self] in
            t.sendPing { err in
                if let e = err { self?.queue.push(.closed("no pong: \(e)")) }
            }
        }
        p.resume()
        pinger = p
    }

    private func receive(_ t: URLSessionWebSocketTask) {
        Task { [weak self] in
            while true {
                do {
                    // The protocol is binary; a text frame is not ours and is dropped.
                    if case .data(let d) = try await t.receive() { self?.queue.push(.frame([UInt8](d))) }
                } catch {
                    self?.queue.push(.closed("\(error)"))
                    return
                }
            }
        }
    }

    public func send(_ frame: [UInt8]) {
        lock.lock()
        let t = task
        lock.unlock()
        guard let t = t else { return }
        Task { [weak self] in
            do {
                try await t.send(.data(Data(frame)))
            } catch {
                self?.queue.push(.closed("\(error)"))
            }
        }
    }

    public func poll() -> [LinkEvent] { queue.take() }

    public func close() {
        lock.lock()
        let t = task
        let s = session
        task = nil
        session = nil
        pinger?.cancel()
        pinger = nil
        lock.unlock()
        queue.finish()
        t?.cancel(with: .goingAway, reason: nil)
        s?.invalidateAndCancel()
    }

    public func urlSession(_ session: URLSession, webSocketTask: URLSessionWebSocketTask, didOpenWithProtocol protocol: String?) {
        queue.push(.opened)
    }

    public func urlSession(_ session: URLSession, webSocketTask: URLSessionWebSocketTask, didCloseWith closeCode: URLSessionWebSocketTask.CloseCode, reason: Data?) {
        queue.push(.closed("closed \(closeCode.rawValue)"))
    }

    public func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        queue.push(.closed(error.map { "failed: \($0)" } ?? "closed"))
    }
}

#if os(Linux)
import Glibc

/// The reference's native transport (`rust/ark-client/src/link/native.rs`),
/// over POSIX sockets: a WebSocket (RFC 6455) on a thread of its own, the
/// TCP connection and the handshake each held to `Timing.connectTimeoutMs`,
/// reads in 20 ms slices between which what is queued is sent, a ping every
/// `Timing.pingEveryMs`, and the connection given up when nothing at all
/// has arrived for three of those. `ws://` only: TLS on Linux would be a
/// dependency, and the reference's own build without its `tls` feature
/// refuses `wss://` the same way.
public final class NativeSocket: Socket {
    private let queue = EventQueue()
    private let lock = NSLock()
    private var out = [[UInt8]]()
    private var closing = false

    public init(_ url: String, _ timing: Timing) {
        let t = Thread { [queue] in
            let why = NativeSocket.run(url, timing, queue, self)
            queue.push(.closed(why))
        }
        t.name = "ark-link"
        t.start()
    }

    public func send(_ frame: [UInt8]) {
        lock.lock()
        if !closing { out.append(frame) }
        lock.unlock()
    }

    public func poll() -> [LinkEvent] { queue.take() }

    /// The thread's signal: it closes the socket properly at its next slice and ends.
    public func close() {
        lock.lock()
        closing = true
        lock.unlock()
        queue.finish()
    }

    // What is queued, and whether the owner closed it.
    private func drain() -> ([[UInt8]], Bool) {
        lock.lock()
        defer { lock.unlock() }
        let o = out
        out.removeAll()
        return (o, closing)
    }

    /// `host`, `port` and the request target of a `ws://` URL
    /// (`native::authority`), or why there are none.
    public static func endpoint(_ url: String) -> (host: String, port: String, target: String)? {
        if case .success(let e) = parse(url) { return e }
        return nil
    }

    static func parse(_ url: String) -> Result<(String, String, String), WsFailure> {
        let rest: Substring
        if url.hasPrefix("ws://") {
            rest = url.dropFirst(5)
        } else if url.hasPrefix("wss://") {
            return .failure(WsFailure("\(url): this build has no TLS (wss:// is URLSession's, on Apple platforms)"))
        } else {
            return .failure(WsFailure("\(url) is not a ws:// or wss:// URL"))
        }
        let hostEnd = rest.firstIndex(where: { $0 == "/" || $0 == "?" || $0 == "#" }) ?? rest.endIndex
        let authority = String(rest[..<hostEnd])
        var target = String(rest[hostEnd...])
        if let hash = target.firstIndex(of: "#") { target = String(target[..<hash]) }
        if target.isEmpty || target.hasPrefix("?") { target = "/" + target }
        if authority.isEmpty { return .failure(WsFailure("\(url) names no host")) }
        var host = authority
        var port = "80"
        if let colon = authority.lastIndex(of: ":"), !authority[colon...].contains("]") {
            host = String(authority[..<colon])
            port = String(authority[authority.index(after: colon)...])
        }
        if host.hasPrefix("[") && host.hasSuffix("]") { host = String(host.dropFirst().dropLast()) }
        return .success((host, port, target))
    }

    struct WsFailure: Error {
        let why: String
        init(_ why: String) { self.why = why }
    }

    private static func timeval(ms: Int64) -> Glibc.timeval {
        Glibc.timeval(tv_sec: Int(ms / 1000), tv_usec: Int((ms % 1000) * 1000))
    }

    private static func setTimeout(_ fd: Int32, _ option: Int32, _ ms: Int64) {
        var tv = timeval(ms: ms)
        _ = setsockopt(fd, SOL_SOCKET, option, &tv, socklen_t(MemoryLayout<Glibc.timeval>.size))
    }

    private static func errno() -> String { String(cString: strerror(Glibc.errno)) }

    // `native::connect`: every address the host resolves to in turn, each
    // held to the connect patience.
    private static func connect(_ host: String, _ port: String, _ timing: Timing) -> Result<Int32, WsFailure> {
        var hints = addrinfo()
        hints.ai_family = AF_UNSPEC
        hints.ai_socktype = Int32(SOCK_STREAM.rawValue)
        var res: UnsafeMutablePointer<addrinfo>? = nil
        let rc = getaddrinfo(host, port, &hints, &res)
        if rc != 0 { return .failure(WsFailure("\(host):\(port): \(String(cString: gai_strerror(rc)))")) }
        defer { freeaddrinfo(res) }
        var last = "\(host):\(port) resolves to nothing"
        var p = res
        while let a = p {
            p = a.pointee.ai_next
            let fd = socket(a.pointee.ai_family, a.pointee.ai_socktype, a.pointee.ai_protocol)
            if fd < 0 {
                last = "socket: \(errno())"
                continue
            }
            let flags = fcntl(fd, F_GETFL, 0)
            _ = fcntl(fd, F_SETFL, flags | O_NONBLOCK)
            var ok = Glibc.connect(fd, a.pointee.ai_addr, a.pointee.ai_addrlen) == 0
            if !ok && Glibc.errno == EINPROGRESS {
                var pfd = pollfd(fd: fd, events: Int16(POLLOUT), revents: 0)
                let n = Glibc.poll(&pfd, 1, Int32(max(timing.connectTimeoutMs, 1)))
                if n == 1 {
                    var err: Int32 = 0
                    var len = socklen_t(MemoryLayout<Int32>.size)
                    _ = getsockopt(fd, SOL_SOCKET, SO_ERROR, &err, &len)
                    ok = err == 0
                    if !ok { last = "\(host):\(port): \(String(cString: strerror(err)))" }
                } else {
                    last = "\(host):\(port): connection timed out"
                }
            } else if !ok {
                last = "\(host):\(port): \(errno())"
            }
            if !ok {
                Glibc.close(fd)
                continue
            }
            _ = fcntl(fd, F_SETFL, flags)
            var one: Int32 = 1
            _ = setsockopt(fd, Int32(IPPROTO_TCP), TCP_NODELAY, &one, socklen_t(MemoryLayout<Int32>.size))
            return .success(fd)
        }
        return .failure(WsFailure(last))
    }

    private static func sendAll(_ fd: Int32, _ bytes: [UInt8]) -> Bool {
        var off = 0
        while off < bytes.count {
            let n = bytes.withUnsafeBufferPointer { p in Glibc.send(fd, p.baseAddress! + off, bytes.count - off, Int32(MSG_NOSIGNAL)) }
            if n <= 0 {
                if n < 0 && (Glibc.errno == EINTR) { continue }
                return false
            }
            off += n
        }
        return true
    }

    // One `recv` into `buf`: the bytes read, 0 at the end of the stream, nil
    // on a timeout, or a failure.
    private static func recvSome(_ fd: Int32, _ buf: inout [UInt8]) -> Result<Int?, WsFailure> {
        var chunk = [UInt8](repeating: 0, count: 65536)
        let n = chunk.withUnsafeMutableBufferPointer { p in Glibc.recv(fd, p.baseAddress!, p.count, 0) }
        if n > 0 {
            buf.append(contentsOf: chunk[0..<n])
            return .success(n)
        }
        if n == 0 { return .success(0) }
        let e = Glibc.errno
        if e == EAGAIN || e == EWOULDBLOCK || e == EINTR { return .success(nil) }
        return .failure(WsFailure(errno()))
    }

    // The upgrade, held to the connect patience: the request, the `101`, and
    // the accept key checked; whatever followed the answer's head, kept.
    private static func handshake(_ fd: Int32, _ host: String, _ port: String, _ target: String, _ timing: Timing) -> Result<[UInt8], WsFailure> {
        var rng = SystemRandomNumberGenerator()
        let key = Data((0..<16).map { _ in UInt8.random(in: 0...255, using: &rng) }).base64EncodedString()
        let hostHeader = (host.contains(":") ? "[\(host)]" : host) + (port == "80" ? "" : ":\(port)")
        let req = "GET \(target) HTTP/1.1\r\nHost: \(hostHeader)\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: \(key)\r\nSec-WebSocket-Version: 13\r\n\r\n"
        setTimeout(fd, SO_RCVTIMEO, max(timing.connectTimeoutMs, 1))
        setTimeout(fd, SO_SNDTIMEO, max(timing.connectTimeoutMs, 1))
        if !sendAll(fd, Array(req.utf8)) { return .failure(WsFailure("the handshake could not be sent: \(errno())")) }
        var buf = [UInt8]()
        let deadline = Link.monotonicMs() + max(timing.connectTimeoutMs, 1)
        var end: Int? = nil
        while end == nil {
            switch recvSome(fd, &buf) {
            case .failure(let f): return .failure(WsFailure("the handshake: \(f.why)"))
            case .success(0): return .failure(WsFailure("the server closed the connection during the handshake"))
            case .success(nil):
                if Link.monotonicMs() >= deadline { return .failure(WsFailure("no answer to the handshake")) }
            case .success:
                if buf.count >= 4 {
                    for i in 0...(buf.count - 4) where buf[i] == 13 && buf[i + 1] == 10 && buf[i + 2] == 13 && buf[i + 3] == 10 {
                        end = i + 4
                        break
                    }
                }
                if Link.monotonicMs() >= deadline && end == nil { return .failure(WsFailure("no answer to the handshake")) }
            }
        }
        let head = String(decoding: buf[0..<end!], as: UTF8.self)
        let lines = head.components(separatedBy: "\r\n")
        let status = lines.first ?? ""
        if status.split(separator: " ").dropFirst().first != "101" { return .failure(WsFailure("the handshake was answered \(status)")) }
        let want = WsWire.acceptKey(key)
        let accept = lines.dropFirst().first { $0.lowercased().hasPrefix("sec-websocket-accept:") }
        let got = accept.map { $0.dropFirst("sec-websocket-accept:".count).trimmingCharacters(in: .whitespaces) }
        if got != want { return .failure(WsFailure("the handshake's accept key is not this request's")) }
        return .success(Array(buf[end!...]))
    }

    // `native::run`: one connection, until it ends; why it did.
    private static func run(_ url: String, _ timing: Timing, _ queue: EventQueue, _ me: NativeSocket) -> String {
        let (host, port, target): (String, String, String)
        switch parse(url) {
        case .failure(let f): return f.why
        case .success(let p): (host, port, target) = p
        }
        let fd: Int32
        switch connect(host, port, timing) {
        case .failure(let f): return f.why
        case .success(let s): fd = s
        }
        defer { Glibc.close(fd) }
        var buf: [UInt8]
        switch handshake(fd, host, port, target, timing) {
        case .failure(let f): return f.why
        case .success(let rest): buf = rest
        }
        setTimeout(fd, SO_RCVTIMEO, 20)
        setTimeout(fd, SO_SNDTIMEO, 0)
        if queue.isDone {
            _ = sendAll(fd, WsWire.frame(8, [0x03, 0xe8]))
            return "closed"
        }
        queue.push(.opened)
        let pingEvery = max(timing.pingEveryMs, 1)
        var heard = Link.monotonicMs()
        var pinged = heard
        var fragment: [UInt8]? = nil
        var fragmentBinary = false
        var at = 0
        while true {
            // What arrived: every whole frame in the buffer.
            while let (fin, op, payload, size) = WsWire.parse(buf, at) {
                at += size
                heard = Link.monotonicMs()
                switch op {
                case 0:
                    fragment?.append(contentsOf: payload)
                    if fin, let whole = fragment {
                        if fragmentBinary { queue.push(.frame(whole)) }
                        fragment = nil
                    }
                case 1, 2:
                    // The protocol is binary; a text frame is not ours and is dropped.
                    if fin {
                        if op == 2 { queue.push(.frame(payload)) }
                    } else {
                        fragment = payload
                        fragmentBinary = op == 2
                    }
                case 8:
                    _ = sendAll(fd, WsWire.frame(8, Array(payload.prefix(2))))
                    return "closed by the server"
                case 9:
                    if !sendAll(fd, WsWire.frame(10, payload)) { return "a pong could not be sent: \(errno())" }
                default:
                    // A pong: the server is there, which is all a keepalive asks.
                    break
                }
            }
            if at > 0 {
                buf.removeFirst(at)
                at = 0
            }
            // What is queued, sent; a close from the owner, said properly.
            let (frames, closing) = me.drain()
            for f in frames where !sendAll(fd, WsWire.frame(2, f)) { return errno() }
            if closing {
                _ = sendAll(fd, WsWire.frame(8, [0x03, 0xe8]))
                return "closed"
            }
            let now = Link.monotonicMs()
            if now - heard >= pingEvery * 3 { return "nothing from the server for \(pingEvery * 3 / 1000)s" }
            if now - pinged >= pingEvery {
                pinged = now
                if !sendAll(fd, WsWire.frame(9, [])) { return errno() }
            }
            switch recvSome(fd, &buf) {
            case .failure(let f): return f.why
            case .success(0): return "the server closed the connection"
            default: break
            }
        }
    }
}
#endif

/// The wire of RFC 6455 as a client speaks it: frames out (masked), frames
/// in, and the handshake's accept key — the part of `NativeSocket` that is
/// arithmetic, and held to the RFC's own examples by the tests.
public enum WsWire {
    /// `Sec-WebSocket-Accept` for a `Sec-WebSocket-Key`: SHA-1 of the key and the RFC's GUID, in base64.
    public static func acceptKey(_ key: String) -> String {
        Data(Sha1.hash(Array((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").utf8))).base64EncodedString()
    }

    /// A frame from this end: FIN set, masked as a client's must be.
    public static func frame(_ opcode: UInt8, _ payload: [UInt8]) -> [UInt8] {
        var f: [UInt8] = [0x80 | opcode]
        let n = payload.count
        if n < 126 {
            f.append(0x80 | UInt8(n))
        } else if n <= 0xffff {
            f.append(0x80 | 126)
            f.append(UInt8(n >> 8))
            f.append(UInt8(n & 0xff))
        } else {
            f.append(0x80 | 127)
            for i in (0..<8).reversed() { f.append(UInt8(truncatingIfNeeded: UInt64(n) >> (8 * UInt64(i)))) }
        }
        var rng = SystemRandomNumberGenerator()
        let mask = (0..<4).map { _ in UInt8.random(in: 0...255, using: &rng) }
        f.append(contentsOf: mask)
        f.reserveCapacity(f.count + n)
        for i in 0..<n { f.append(payload[i] ^ mask[i & 3]) }
        return f
    }

    /// One whole frame at the front of `buf` from `at`: (fin, opcode, payload, its length in bytes), or nil if it has not all arrived.
    public static func parse(_ buf: [UInt8], _ at: Int) -> (Bool, UInt8, [UInt8], Int)? {
        let avail = buf.count - at
        if avail < 2 { return nil }
        let b0 = buf[at]
        let b1 = buf[at + 1]
        var len = Int(b1 & 0x7f)
        var head = 2
        if len == 126 {
            if avail < 4 { return nil }
            len = Int(buf[at + 2]) << 8 | Int(buf[at + 3])
            head = 4
        } else if len == 127 {
            if avail < 10 { return nil }
            var l: UInt64 = 0
            for i in 0..<8 { l = l << 8 | UInt64(buf[at + 2 + i]) }
            len = Int(l)
            head = 10
        }
        let masked = b1 & 0x80 != 0
        let maskAt = at + head
        if masked { head += 4 }
        if avail < head + len { return nil }
        var payload = Array(buf[(at + head)..<(at + head + len)])
        if masked { for i in 0..<len { payload[i] ^= buf[maskAt + (i & 3)] } }
        return (b0 & 0x80 != 0, b0 & 0x0f, payload, head + len)
    }

}

/// SHA-1, for the one thing RFC 6455 uses it for: the handshake's accept key.
enum Sha1 {
    static func hash(_ message: [UInt8]) -> [UInt8] {
        var h: [UInt32] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0]
        var m = message
        let bits = UInt64(message.count) * 8
        m.append(0x80)
        while m.count % 64 != 56 { m.append(0) }
        for i in (0..<8).reversed() { m.append(UInt8(truncatingIfNeeded: bits >> (8 * UInt64(i)))) }
        var w = [UInt32](repeating: 0, count: 80)
        for chunk in stride(from: 0, to: m.count, by: 64) {
            for i in 0..<16 {
                w[i] = UInt32(m[chunk + 4 * i]) << 24 | UInt32(m[chunk + 4 * i + 1]) << 16 | UInt32(m[chunk + 4 * i + 2]) << 8 | UInt32(m[chunk + 4 * i + 3])
            }
            for i in 16..<80 {
                let x = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]
                w[i] = x << 1 | x >> 31
            }
            var a = h[0], b = h[1], c = h[2], d = h[3], e = h[4]
            for i in 0..<80 {
                let f: UInt32
                let k: UInt32
                switch i {
                case 0..<20: f = (b & c) | (~b & d); k = 0x5A82_7999
                case 20..<40: f = b ^ c ^ d; k = 0x6ED9_EBA1
                case 40..<60: f = (b & c) | (b & d) | (c & d); k = 0x8F1B_BCDC
                default: f = b ^ c ^ d; k = 0xCA62_C1D6
                }
                let t = (a << 5 | a >> 27) &+ f &+ e &+ k &+ w[i]
                e = d
                d = c
                c = b << 30 | b >> 2
                b = a
                a = t
            }
            h[0] = h[0] &+ a
            h[1] = h[1] &+ b
            h[2] = h[2] &+ c
            h[3] = h[3] &+ d
            h[4] = h[4] &+ e
        }
        return h.flatMap { v in (0..<4).map { UInt8(truncatingIfNeeded: v >> (24 - 8 * UInt32($0))) } }
    }
}
