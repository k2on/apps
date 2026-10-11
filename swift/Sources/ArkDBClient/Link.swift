// The one place a peer's networking lives, as `rust/ark-client/src/link` is
// the reference's: a socket that moves binary frames, polled, and a `Link`
// around it that dials, notices a drop, and dials again with a backoff (half
// a second, doubling to thirty). The engine is sans-io, so everything here
// happens on `Link.poll`, which the peer's `pump` calls — twenty times a
// second, typically, from a UI's tick or a loop of its own.
//
// Two sockets ship (`WebSocket.swift`): `NativeSocket`, the reference's
// `link::native` ported — a WebSocket on a thread of its own over POSIX
// sockets, `ws://` only — and `SessionSocket`, `URLSessionWebSocketTask`,
// `ws://` and `wss://`. `platformDial` takes the second on Apple platforms
// and the first on Linux, whose Foundation declares the task and refuses a
// `ws://` URL with it at once (NSURLErrorUnsupportedURL). A test supplies
// its own through `Dial` (`Queues`).
//
// **Keepalive** is the server's — it pings every twenty seconds — and the
// socket's: both ping every `Timing.pingEveryMs`, and the native socket
// gives the connection up when nothing at all has arrived for three of
// those, the one state no read ever reports — a socket the OS thinks is
// open to a peer that is not there.
import Foundation

/// The numbers a link runs on (`link::Timing`); the defaults are a deployment's, a test shortens them.
public struct Timing: Equatable {
    public var firstBackoffMs: Int64
    public var maxBackoffMs: Int64
    /// How often the socket pings; the native one gives up after three of these with nothing received.
    public var pingEveryMs: Int64
    /// How long a dial waits for the TCP connection, and then for the answer to its WebSocket handshake.
    public var connectTimeoutMs: Int64

    public init(firstBackoffMs: Int64 = 500, maxBackoffMs: Int64 = 30_000, pingEveryMs: Int64 = 20_000, connectTimeoutMs: Int64 = 5_000) {
        self.firstBackoffMs = firstBackoffMs
        self.maxBackoffMs = maxBackoffMs
        self.pingEveryMs = pingEveryMs
        self.connectTimeoutMs = connectTimeoutMs
    }
}

/// What a socket reports (`link::Event`).
public enum LinkEvent: Equatable {
    /// The socket is open: the peer says `Hello`.
    case opened
    /// A binary frame arrived.
    case frame([UInt8])
    /// The socket is gone, or never opened, and why.
    case closed(String)
}

/// One connection's worth of socket (`link::Transport`): made by a dial, polled by a link.
public protocol Socket: AnyObject {
    /// Queue a binary frame. Only called after `opened`.
    func send(_ frame: [UInt8])
    /// Everything that happened since the last poll, oldest first. Never blocks.
    func poll() -> [LinkEvent]
    /// Close it; nothing more is reported.
    func close()
}

/// How a link opens a socket to a URL (`link::Dial`).
public typealias Dial = (String) -> Socket

/// Where a link is (`link::State`).
public enum LinkState: Equatable {
    /// Not asked to connect, or told to stop (offline, or denied).
    case idle
    /// Dialled, waiting for the socket to open.
    case connecting
    /// Open; the peer is linked.
    case open
    /// Down; the next attempt is at this monotonic millisecond.
    case waiting(Int64)
}

/// What one poll of a link came to, for the peer to hand the engine (`link::Polled`).
public struct Polled {
    /// The socket opened: say `connected()`.
    public var opened = false
    public var frames = [[UInt8]]()
    /// The socket that was open closed: say `disconnected()`.
    public var closed: String? = nil
}

/// A reconnecting connection to one URL (`link::Link`).
public final class Link {
    public let url: String
    private let dial: Dial
    private let timing: Timing
    private var socket: Socket? = nil
    public private(set) var state: LinkState = .waiting(0)
    private var backoff: Int64
    /// Connections opened so far.
    public private(set) var opens: Int64 = 0
    /// Why the last connection ended, or the last attempt failed.
    public private(set) var lastClose: String? = nil

    public init(_ url: String, _ dial: @escaping Dial, _ timing: Timing) {
        self.url = url
        self.dial = dial
        self.timing = timing
        backoff = timing.firstBackoffMs
    }

    public var isOpen: Bool { state == .open }

    /// Stop, and stay stopped until `resume`. Whether a socket that was open went.
    @discardableResult
    public func stop(_ why: String) -> Bool {
        let was = state == .open
        socket?.close()
        socket = nil
        state = .idle
        backoff = timing.firstBackoffMs
        lastClose = why
        return was
    }

    /// Dial again on the next poll, after `stop`.
    public func resume() {
        if state == .idle { state = .waiting(0) }
    }

    /// Queue a frame on the open socket; dropped otherwise, which is right:
    /// the engine re-offers everything durable on the next `Hello`.
    public func send(_ frame: [UInt8]) {
        if state == .open { socket?.send(frame) }
    }

    /// One turn: dial if due, and collect what the socket did.
    public func poll() -> Polled { pollAt(Link.monotonicMs()) }

    /// `poll` at a given time, so the backoff is testable.
    public func pollAt(_ now: Int64) -> Polled {
        var out = Polled()
        if case .waiting(let at) = state, now >= at {
            socket = dial(url)
            state = .connecting
        }
        guard let t = socket else { return out }
        for e in t.poll() {
            switch e {
            case .opened:
                state = .open
                opens += 1
                backoff = timing.firstBackoffMs
                out.opened = true
                out.closed = nil
            case .frame(let b):
                if state == .open { out.frames.append(b) }
            case .closed(let why):
                if state == .open { out.closed = why }
                socket?.close()
                socket = nil
                lastClose = why
                state = .waiting(now + backoff)
                backoff = min(backoff * 2, timing.maxBackoffMs)
                return out
            }
        }
        return out
    }

    /// A monotonic clock in milliseconds (`autos::monotonic_ms`).
    public static func monotonicMs() -> Int64 { Int64(DispatchTime.now().uptimeNanoseconds / 1_000_000) }
}

/// A socket's events kept until polled, for a socket whose events arrive on
/// another thread: `opened`, then frames, then exactly one `closed`.
final class EventQueue {
    private let lock = NSLock()
    private var events = [LinkEvent]()
    private var ended = false
    private var done = false

    /// Queue an event; nothing after the first `closed`.
    func push(_ e: LinkEvent) {
        lock.lock()
        defer { lock.unlock() }
        if ended { return }
        if case .closed = e { ended = true }
        events.append(e)
    }

    /// Everything since the last take; nothing once the `closed` was taken or the socket closed.
    func take() -> [LinkEvent] {
        lock.lock()
        defer { lock.unlock() }
        if done { return [] }
        let got = events
        events.removeAll()
        if got.contains(where: { if case .closed = $0 { return true } else { return false } }) { done = true }
        return got
    }

    /// The socket was closed by its owner: nothing more is reported.
    func finish() {
        lock.lock()
        done = true
        ended = true
        events.removeAll()
        lock.unlock()
    }

    var isDone: Bool {
        lock.lock()
        defer { lock.unlock() }
        return done
    }
}

/// A socket over two queues, for a test or an in-process server
/// (`link::Queues`): what the link sends goes to `sent`, and what is pushed
/// with `push` is what it reports.
public final class Queues: Socket {
    private let lock = NSLock()
    private var out = [[UInt8]]()
    private var events = [LinkEvent]()
    public private(set) var closed = false

    public init() {}

    public func push(_ e: LinkEvent) {
        lock.lock()
        events.append(e)
        lock.unlock()
    }

    /// What the link sent since the last ask.
    public func takeSent() -> [[UInt8]] {
        lock.lock()
        defer { lock.unlock() }
        let s = out
        out.removeAll()
        return s
    }

    public func send(_ frame: [UInt8]) {
        lock.lock()
        out.append(frame)
        lock.unlock()
    }

    public func poll() -> [LinkEvent] {
        lock.lock()
        defer { lock.unlock() }
        let got = events
        events.removeAll()
        return got
    }

    public func close() {
        lock.lock()
        closed = true
        lock.unlock()
    }
}

/// The platform's WebSocket, held to `timing` (`link::platform_dial`):
/// `URLSessionWebSocketTask` on Apple platforms, `NativeSocket` on Linux.
public func platformDial(_ timing: Timing) -> Dial {
    #if canImport(Darwin)
    return { url in SessionSocket(url, timing) }
    #else
    return { url in NativeSocket(url, timing) }
    #endif
}
