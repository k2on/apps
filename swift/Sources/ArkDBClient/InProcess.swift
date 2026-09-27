import Foundation
import ArkDB

/// Who a connection is: the user, and the login (one login on one device).
/// Every entry the connection pushes is held to both.
public struct Identity: Equatable {
    public var user: String
    public var session: String
    public init(user: String, session: String) { self.user = user; self.session = session }
}

/// What a token proves; asked once, at `Hello`.
public typealias Authenticate = (String?) -> Identity?

/// Dev auth, `Ark.Protocol.trusting`: anyone is whoever they say, the token
/// is their name, and the login is `"dev"`.
public func trusting(_ token: String?) -> Identity? {
    return Identity(user: token ?? "anonymous", session: "dev")
}

/// §12.3 The server's end of the protocol, sans-io, as `Ark.Protocol`'s
/// `Server` without live rooms: authorities for the scopes it hosts, a
/// cursor per connection per scope, and fan-out after every message. Here
/// so that two sessions can meet an authority in one process — the tests'
/// network, and a way to run the protocol with no socket at all.
public final class InProcessServer {
    public typealias ConnId = Int

    struct Conn {
        var who: Identity
        /// Per scope: the mode, and the sequence the connection has been sent up to.
        var scopes: [ScopeName: (Mode, Seq)]
    }

    public var authenticate: Authenticate
    /// May this identity receive this scope? The scope-level read rule.
    public var access: (Identity, ScopeName) -> Bool
    public private(set) var scopes: [ScopeName: Authority] = [:]
    private var conns: [ConnId: Conn] = [:]
    private var out: [(ConnId, ServerMsg)] = []

    public init(authenticate: @escaping Authenticate = trusting, access: @escaping (Identity, ScopeName) -> Bool = { _, _ in true }) {
        self.authenticate = authenticate
        self.access = access
    }

    /// Host a scope: become its authority.
    public func host(_ a: Authority) { scopes[a.scope] = a }

    /// The authority of a scope, to look at.
    public func authority(_ s: ScopeName) -> Authority? { return scopes[s] }

    func send(_ c: ConnId, _ m: ServerMsg) { out.append((c, m)) }

    /// A frame from a connection.
    public func recv(_ c: ConnId, _ msg: ClientMsg) {
        switch msg {
        case .hello(let subs, let tok, _):
            guard let who = authenticate(tok) else { send(c, .denied(reason: "not signed in")); return }
            var held: [ScopeName: (Mode, Seq)] = [:]
            for s in subs where access(who, s.scope) && scopes[s.scope] != nil {
                held[s.scope] = (s.mode, s.since)
            }
            conns[c] = Conn(who: who, scopes: held)
            fanout()
        case .push(let s, let es):
            guard let conn = conns[c] else { send(c, .denied(reason: "hello first")); return }
            guard var a = scopes[s] else { send(c, .denied(reason: "unknown scope " + s)); return }
            var ids: [Id] = []
            var seqs: [Seq] = []
            for e in es {
                if e.actor != conn.who.user || e.session != conn.who.session {
                    send(c, .reject(scope: s, id: e.id, reason: "not yours"))
                    continue
                }
                switch a.sequenceEntry(e) {
                case .appended(let n, _): ids.append(e.id); seqs.append(n)
                case .duplicate(let n): ids.append(e.id); seqs.append(n)
                case .rejected(let why): send(c, .reject(scope: s, id: e.id, reason: why.text))
                }
            }
            scopes[s] = a
            if !ids.isEmpty { send(c, .ack(scope: s, ids: ids, seqs: seqs)) }
            fanout()
        case .needFacts(let s, let ns):
            guard conns[c] != nil else { send(c, .denied(reason: "hello first")); return }
            guard let a = scopes[s] else { return }
            var items: [FactsItem] = []
            for n in ns { if let it = a.log.entries[n] { items.append(FactsItem(seq: n, facts: it.facts)) } }
            send(c, .factsFor(scope: s, items: items))
        case .needClosures(let hs):
            guard conns[c] != nil else { send(c, .denied(reason: "hello first")); return }
            var items: [ClosureItem] = []
            for h in hs {
                for a in scopes.values { if let cl = a.bodies[h] { items.append(ClosureItem(hash: h, closure: cl)); break } }
            }
            send(c, .closures(items: items))
        case .verify(let s, let n, let h):
            guard conns[c] != nil else { send(c, .denied(reason: "hello first")); return }
            guard let a = scopes[s] else { return }
            let ok = a.log.stateAt(n).map { Hash.stateHash($0) == h } ?? false
            send(c, .agree(scope: s, seq: n, hash: h, ok: ok))
        case .say:
            // No live rooms here: a frame said into this server is heard by nobody.
            guard conns[c] != nil else { send(c, .denied(reason: "hello first")); return }
        }
    }

    /// A connection closed: its cursors are forgotten.
    public func disconnect(_ c: ConnId) { conns[c] = nil }

    /// §12.4 Fan-out: every connection, every scope it holds, everything
    /// above what it has been sent, a page at a time; a snapshot for one
    /// below the horizon. Whole-mode batches carry no facts, as the spec's do.
    func fanout() {
        for c in conns.keys.sorted() {
            guard let conn = conns[c] else { continue }
            for s in conn.scopes.keys.sorted(by: { compareText($0, $1) < 0 }) {
                guard let a = scopes[s], let (md, sent) = conn.scopes[s] else { continue }
                if sent >= a.log.headSeq { continue }
                switch a.page(sent, batchLimit) {
                case .belowHorizon(let sn):
                    var rows: [TableName: [Value]] = [:]
                    for t in sn.store.tableNames { rows[t] = sn.store.scan(t).map { .record($0) } }
                    send(c, .snapshotOf(scope: s, seq: sn.seq, hash: sn.hash, rows: rows))
                    conns[c]?.scopes[s] = (md, sn.seq)
                case .entries(let items, let more):
                    let batch = items.map { BatchItem(seq: $0.0, entry: $0.1, facts: md == .byFacts ? $0.2 : nil) }
                    let last = items.map { $0.0 }.max() ?? sent
                    send(c, .batch(scope: s, items: batch, hasMore: more))
                    conns[c]?.scopes[s] = (md, max(sent, last))
                }
            }
        }
    }

    public func takeOutgoing() -> [(ConnId, ServerMsg)] {
        let r = out
        out = []
        return r
    }
}

/// A network in one process: an `InProcessServer` and the transports dialled
/// into it. `dial` is a `LinkDial`, so a `Session` opened with it runs the
/// whole protocol — hello, push, batch, ack, rebase — with no socket. Frames
/// cross as the bytes they would on the wire, so the codecs are exercised.
public final class MemoryExchange {
    public let server: InProcessServer
    private let lock = NSRecursiveLock()
    private var next: InProcessServer.ConnId = 1
    private var transports: [InProcessServer.ConnId: MemoryTransport] = [:]
    /// Frames that were not a client frame, dropped.
    public private(set) var badFrames = 0

    public init(server: InProcessServer = InProcessServer()) { self.server = server }

    /// Host a scope on the exchange's server.
    public func host(_ a: Authority) {
        lock.lock(); defer { lock.unlock() }
        server.host(a)
    }

    /// The `LinkDial` a `Session` takes; the URL is ignored.
    public func dial(_ url: URL) -> LinkTransport {
        return MemoryTransport(exchange: self)
    }

    func attach(_ t: MemoryTransport) -> InProcessServer.ConnId {
        lock.lock(); defer { lock.unlock() }
        let c = next
        next += 1
        transports[c] = t
        return c
    }

    func detach(_ c: InProcessServer.ConnId) {
        lock.lock(); defer { lock.unlock() }
        transports[c] = nil
        server.disconnect(c)
    }

    func handle(_ c: InProcessServer.ConnId, _ frame: [UInt8]) {
        lock.lock(); defer { lock.unlock() }
        do {
            let m = try Wire.clientFromValue(try Canon.decode(frame))
            server.recv(c, m)
        } catch {
            badFrames += 1
        }
        deliver()
    }

    func deliver() {
        for (c, m) in server.takeOutgoing() {
            transports[c]?.emit(.frame(Canon.encode(Wire.serverValue(m))))
        }
    }

    /// Every connection dropped at once, as a server restart would.
    public func dropAll() {
        lock.lock()
        let ts = transports
        transports = [:]
        for c in ts.keys { server.disconnect(c) }
        lock.unlock()
        for t in ts.values { t.emit(.closed("server went away")) }
    }
}

/// One connection into a `MemoryExchange`.
public final class MemoryTransport: LinkTransport {
    private let exchange: MemoryExchange
    private var events: ((LinkEvent) -> Void)?
    private var conn: InProcessServer.ConnId?
    private let lock = NSLock()

    init(exchange: MemoryExchange) { self.exchange = exchange }

    func emit(_ e: LinkEvent) {
        lock.lock()
        let f = events
        lock.unlock()
        f?(e)
    }

    public func open(_ events: @escaping (LinkEvent) -> Void) {
        lock.lock()
        self.events = events
        lock.unlock()
        let c = exchange.attach(self)
        lock.lock()
        conn = c
        lock.unlock()
        emit(.opened)
    }

    public func send(_ frame: [UInt8]) {
        lock.lock()
        let c = conn
        lock.unlock()
        guard let c = c else { return }
        exchange.handle(c, frame)
    }

    public func close() {
        lock.lock()
        let c = conn
        conn = nil
        lock.unlock()
        if let c = c { exchange.detach(c) }
        emit(.closed(nil))
    }
}
