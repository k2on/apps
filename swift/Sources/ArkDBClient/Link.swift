import Foundation
import ArkDB

/// What a `Link` drives: the engine's `Client`, behind whoever owns it (the
/// `Session`), so the link never holds the machine itself.
public protocol LinkDriven: AnyObject {
    /// The connection is open: `Client.connected()`.
    func linkOpened()
    /// The connection is gone: `Client.disconnected()`.
    func linkClosed(_ reason: String?)
    /// A frame decoded: `Client.recv`.
    func linkReceived(_ m: ServerMsg)
    /// What is queued to send: `Client.takeOutgoing()`.
    func linkOutgoing() -> [ClientMsg]
}

/// The WebSocket transport driving a sans-io `Client`: it dials, says
/// `connected()` when the socket opens, sends every queued frame as binary
/// canonical CBOR, decodes every binary frame that arrives into a
/// `ServerMsg`, and reconnects with backoff (0.5 s doubling to 30 s) when
/// the socket fails. Everything happens on `pump()`, which a timer calls
/// every 50 ms; the transport's own threads only queue events.
public final class Link {
    public enum State: Equatable {
        /// Not asked to connect.
        case idle
        /// Dialled, waiting for the socket to open.
        case connecting
        /// The socket is open and the client is linked.
        case open
        /// Down; the next attempt is at the date.
        case waiting(until: Date)
    }

    public static let firstBackoff: TimeInterval = 0.5
    public static let maxBackoff: TimeInterval = 30
    /// How often an open socket is pinged when nothing else is said.
    public static let pingEvery: TimeInterval = 20

    public let url: URL
    private let dial: LinkDial
    private weak var driven: LinkDriven?
    private let lock = NSLock()
    private var inbox: [(Int, LinkEvent)] = []
    private var generation = 0
    private var transport: LinkTransport?
    private var backoff: TimeInterval = Link.firstBackoff
    private var wanted = false
    private var lastPing = Date.distantPast
    public private(set) var state: State = .idle
    /// Connections opened so far.
    public private(set) var opens = 0
    /// Frames that were not canonical CBOR or not a server frame, dropped.
    public private(set) var badFrames = 0

    public init(url: URL, dial: @escaping LinkDial, driven: LinkDriven) {
        self.url = url
        self.dial = dial
        self.driven = driven
    }

    /// Ask for a connection; the next `pump()` dials.
    public func connect() {
        lock.lock()
        wanted = true
        if case .idle = state { state = .waiting(until: .distantPast) }
        lock.unlock()
    }

    /// Drop the connection and stop trying; the client is told at once.
    public func disconnect() {
        lock.lock()
        wanted = false
        let t = transport
        transport = nil
        generation += 1
        inbox = []
        let wasUp = state == .open
        state = .idle
        backoff = Link.firstBackoff
        lock.unlock()
        t?.close()
        if wasUp { driven?.linkClosed("disconnected") }
    }

    /// Whether the socket is open.
    public var isOpen: Bool {
        lock.lock(); defer { lock.unlock() }
        return state == .open
    }

    private func enqueue(_ gen: Int, _ e: LinkEvent) {
        lock.lock()
        if gen == generation { inbox.append((gen, e)) }
        lock.unlock()
    }

    /// One turn: deliver what arrived, dial if due, send what is queued.
    /// `now` is a parameter so the backoff can be tested without waiting.
    public func pump(now: Date = Date()) {
        lock.lock()
        let events = inbox
        inbox = []
        lock.unlock()
        for (_, e) in events { handle(e, now: now) }

        lock.lock()
        var toDial = false
        if wanted, transport == nil, case .waiting(let until) = state, now >= until {
            toDial = true
            state = .connecting
            generation += 1
        }
        let gen = generation
        lock.unlock()
        if toDial {
            let t = dial(url)
            lock.lock()
            transport = t
            lock.unlock()
            t.open { [weak self] e in self?.enqueue(gen, e) }
        }

        lock.lock()
        let up = state == .open
        let t = transport
        let pingDue = up && now.timeIntervalSince(lastPing) >= Link.pingEvery
        if pingDue { lastPing = now }
        lock.unlock()
        if up, let t = t, let d = driven {
            for m in d.linkOutgoing() {
                t.send(Canon.encode(Wire.clientValue(m)))
            }
            if pingDue, let ws = t as? WebSocketTransport { ws.ping() }
        }
    }

    private func handle(_ e: LinkEvent, now: Date) {
        switch e {
        case .opened:
            lock.lock()
            state = .open
            backoff = Link.firstBackoff
            opens += 1
            lastPing = now
            lock.unlock()
            driven?.linkOpened()
        case .frame(let bytes):
            do {
                let m = try Wire.serverFromValue(try Canon.decode(bytes))
                driven?.linkReceived(m)
            } catch {
                lock.lock(); badFrames += 1; lock.unlock()
            }
        case .closed(let why):
            lock.lock()
            let wasUp = state == .open
            let t = transport
            transport = nil
            generation += 1
            inbox = []
            let delay = backoff
            backoff = min(backoff * 2, Link.maxBackoff)
            state = wanted ? .waiting(until: now.addingTimeInterval(delay)) : .idle
            lock.unlock()
            t?.close()
            if wasUp { driven?.linkClosed(why) }
        }
    }
}
