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
/// `Server` without live rooms: the log's authority, one access rule, the
/// sessions each user owns, a cursor per connection, and fan-out after
/// every message. Here so that two sessions can meet an authority in one
/// process — the tests' network, and a way to run the protocol with no
/// socket at all.
public final class InProcessServer {
    public typealias ConnId = Int

    struct Conn {
        var who: Identity
        var mode: Mode
        /// The sequence the connection has been sent up to (not what it has
        /// applied — that is its own business).
        var sent: Seq
    }

    public var authenticate: Authenticate
    /// May this identity receive the log? The read rule.
    public var access: (Identity) -> Bool
    /// Does this user own this session? A session outlives its token: an
    /// entry authored offline under one login and pushed after the same
    /// person signs in again carries the old session, and is still theirs.
    /// Only ever asked about the connection's own user. By default, no.
    public var owns: (String, String) -> Bool = { _, _ in false }
    public private(set) var authority: Authority
    private var conns: [ConnId: Conn] = [:]
    private var out: [(ConnId, ServerMsg)] = []

    /// A server that is the authority for the log.
    public init(_ authority: Authority, authenticate: @escaping Authenticate = trusting, access: @escaping (Identity) -> Bool = { _ in true }) {
        self.authority = authority
        self.authenticate = authenticate
        self.access = access
    }

    /// Install the sessions a user owns, which the authenticator's session
    /// store knows and the engine does not (`Ark.Protocol.withOwns`).
    @discardableResult
    public func withOwns(_ owns: @escaping (String, String) -> Bool) -> InProcessServer {
        self.owns = owns
        return self
    }

    func send(_ c: ConnId, _ m: ServerMsg) { out.append((c, m)) }

    /// A frame from a connection.
    public func recv(_ c: ConnId, _ msg: ClientMsg) {
        switch msg {
        case .hello(let sub, let tok, _):
            // Nobody (the empty user a peer authors as before anyone signs
            // in) is not an identity any token proves.
            guard let who = authenticate(tok), !who.user.isEmpty else { send(c, .denied(reason: "not signed in")); return }
            guard access(who) else { send(c, .denied(reason: "not allowed")); return }
            // A second hello on one connection is the log paging, and says
            // where to continue from.
            conns[c] = Conn(who: who, mode: sub.mode, sent: sub.since)
            fanout()
        case .push(let es):
            guard let conn = conns[c] else { send(c, .denied(reason: "hello first")); return }
            var ids: [Id] = []
            var seqs: [Seq] = []
            for e in es {
                if e.actor != conn.who.user || (e.session != conn.who.session && !owns(e.actor, e.session)) {
                    send(c, .reject(id: e.id, reason: "not yours"))
                    continue
                }
                switch authority.sequenceEntry(e) {
                case .appended(let n, _): ids.append(e.id); seqs.append(n)
                case .duplicate(let n): ids.append(e.id); seqs.append(n)
                case .rejected(let why): send(c, .reject(id: e.id, reason: refusalText(why)))
                }
            }
            if !ids.isEmpty { send(c, .ack(ids: ids, seqs: seqs)) }
            fanout()
        case .needFacts(let ns):
            guard conns[c] != nil else { send(c, .denied(reason: "hello first")); return }
            var items: [FactsItem] = []
            for n in ns { if let it = authority.log.entries[n] { items.append(FactsItem(seq: n, facts: it.facts)) } }
            send(c, .factsFor(items: items))
        case .needClosures(let hs):
            guard conns[c] != nil else { send(c, .denied(reason: "hello first")); return }
            send(c, .closures(items: hs.compactMap { h in authority.bodies[h].map { ClosureItem(hash: h, closure: $0) } }))
        case .verify(let n, let h):
            guard conns[c] != nil else { send(c, .denied(reason: "hello first")); return }
            let ok = authority.log.stateAt(n).map { Hash.stateHash($0) == h } ?? false
            send(c, .agree(seq: n, hash: h, ok: ok))
        case .say:
            // No live rooms here: a frame said into this server is heard by nobody.
            guard conns[c] != nil else { send(c, .denied(reason: "hello first")); return }
        }
    }

    /// A connection closed: its cursor is forgotten.
    public func disconnect(_ c: ConnId) { conns[c] = nil }

    /// §12.4 Fan-out: every connection, everything above what it has been
    /// sent, a page at a time; a snapshot for one below the horizon.
    /// Whole-mode batches carry no facts, as the spec's do.
    func fanout() {
        for c in conns.keys.sorted() {
            guard let conn = conns[c], conn.sent < authority.log.headSeq else { continue }
            switch authority.page(conn.sent, batchLimit) {
            case .belowHorizon(let sn):
                var rows: [TableName: [Value]] = [:]
                for t in sn.store.tableNames { rows[t] = sn.store.scan(t).map { .record($0) } }
                send(c, .snapshotOf(seq: sn.seq, hash: sn.hash, rows: rows))
                conns[c]?.sent = sn.seq
            case .entries(let items, let more):
                let batch = items.map { BatchItem(seq: $0.0, entry: $0.1, facts: conn.mode == .byFacts ? $0.2 : nil) }
                send(c, .batch(items: batch, hasMore: more))
                conns[c]?.sent = max(conn.sent, items.map { $0.0 }.max() ?? conn.sent)
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

    public init(server: InProcessServer) { self.server = server }

    /// An exchange whose server is a new authority over the module's
    /// closures, trusting whoever says hello.
    public convenience init(_ module: Module) {
        self.init(server: InProcessServer(Authority(module.schema, Hash.closures(module))))
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
