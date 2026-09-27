import Foundation
import ArkDB

public enum SessionError: Error, CustomStringConvertible {
    case badModule(String)
    case unknownFunction(String)
    case notAMutator(String)
    case notAQuery(String)
    case corrupt(String)
    case io(String)
    /// The directory was last opened the other way — with a server, or
    /// alone — and the sequences in it mean something else under this one.
    case modeMismatch(was: String, now: String)

    public var description: String {
        switch self {
        case .badModule(let s): return "bad module: " + s
        case .unknownFunction(let s): return "unknown function " + s
        case .notAMutator(let s): return s + " is not a mutator"
        case .notAQuery(let s): return s + " is not a query"
        case .corrupt(let s): return "corrupt: " + s
        case .io(let s): return "io: " + s
        case .modeMismatch(let was, let now): return "the directory was opened \(was) before and \(now) now"
        }
    }
}

/// The generated domain, as the session needs to know it: which hashes the
/// generated `apply` dispatches on, the `apply`, and the `query`. An app
/// builds one from its `<Name>Gen` in a line each
/// (`Generated(functions: HarkenGen.functions, apply: HarkenGen.apply, query: HarkenGen.query)`).
/// With one present, every intent — authored here or replayed from the log —
/// whose function the generated code has goes through that code over a
/// `TransactionStore`; the interpreter runs only the rest, which a peer
/// generated with `--only` still receives.
public struct Generated {
    public var functions: [(String, String)]
    public var apply: (String, Store, Ctx, Args, Args) throws -> Void
    public var query: (String, Store, Args) throws -> Value
    let hashes: Set<[UInt8]>
    let names: Set<String>

    public init(functions: [(String, String)], apply: @escaping (String, Store, Ctx, Args, Args) throws -> Void, query: @escaping (String, Store, Args) throws -> Value) {
        self.functions = functions
        self.apply = apply
        self.query = query
        self.hashes = Set(functions.compactMap { Hex.decode($0.1) })
        self.names = Set(functions.map { $0.0 })
    }

    public func knows(_ h: FnHash) -> Bool { return hashes.contains(h) }
}

/// What the authority last said about a scope's state.
public struct Agreement: Equatable {
    public var scope: ScopeName
    public var seq: Seq
    public var ok: Bool
    public init(scope: ScopeName, seq: Seq, ok: Bool) { self.scope = scope; self.seq = seq; self.ok = ok }
}

public struct SessionStatus: Equatable {
    /// The socket is open and the hello has been said.
    public var linked: Bool
    /// This peer is its own authority (no server was given).
    public var alone: Bool
    /// The last confirmed sequence, per scope.
    public var cursors: [ScopeName: Seq]
    /// Intents authored here that no verdict has answered, over every scope.
    public var pending: Int
    /// The server turned this peer away, and why.
    public var denied: String?
    /// The last `Agree` the authority sent.
    public var lastAgree: Agreement?
    /// Verdicts against this peer's own intents so far.
    public var rejections: Int
    /// The link, in a word: idle, connecting, open, waiting.
    public var link: String
}

/// Everything an app needs around the sans-io machines: the replicas, one
/// per scope held whole; the `Client` and the `Link` that drives it — or,
/// with no server, an `Authority` per scope that sequences what this peer
/// authors (docs/arkdb.md §3.10); the files under a directory that make it
/// all durable; and the two things a screen does, `mutate` and `query`.
///
/// Thread-safe: every method takes one lock, and change subscribers are
/// called after it is released, on whichever thread pumped or mutated.
public final class Session: LinkDriven {
    public let module: Module
    public let schema: Schema
    public let directory: URL
    public let ctx: Ctx
    /// The scopes held, in name order.
    public let scopes: [ScopeName]
    public let server: URL?
    private let generated: Generated?
    private let closures: [FnHash: Closure]
    private var byName: [String: (Function, FnHash)] = [:]
    private var client: Client
    private var authorities: [ScopeName: Authority] = [:]
    private var link: Link?
    private let lock = NSLock()
    private var subscribers: [Int: (ScopeName, Changes) -> Void] = [:]
    private var nextSubscriber = 1
    private var timer: DispatchSourceTimer?
    private let queue = DispatchQueue(label: "arkdb.session")
    private var persisted: [ScopeName: (Seq, Int)] = [:]
    private var rng = SystemRandomNumberGenerator()
    /// What happened last, for a status line: a refusal, a denial, a bug.
    public private(set) var lastNote: String?

    // MARK: opening

    /// Open the session: decode the module, open every scope's replica from
    /// the directory (or empty), and either dial the server or stand as the
    /// authority for every scope. `session` is the login; under dev auth the
    /// server calls every login `"dev"` and the token is the user's name.
    public static func open(directory: URL, module bytes: [UInt8], user: String, session: String = "dev",
                            server: URL?, token: String? = nil, generated: Generated? = nil,
                            dial: LinkDial? = nil) throws -> Session {
        let m: Module
        do {
            m = try Decode.fromValue(try Canon.decode(bytes))
        } catch {
            throw SessionError.badModule("\(error)")
        }
        return try Session(module: m, directory: directory, ctx: Ctx(user: user, session: session), server: server,
                           token: token ?? user, generated: generated, dial: dial ?? WebSocketTransport.dial)
    }

    init(module: Module, directory: URL, ctx: Ctx, server: URL?, token: String, generated: Generated?, dial: @escaping LinkDial) throws {
        self.module = module
        self.schema = module.schema
        self.directory = directory
        self.ctx = ctx
        self.server = server
        self.generated = generated
        self.closures = Hash.closures(module)
        self.scopes = module.schema.scopes.map { $0.name }.sorted { compareText($0, $1) < 0 }
        self.client = Client(schema: module.schema, token: token)
        for (h, c) in closures { byName[c.fn.name] = (c.fn, h) }
        let applier: Applier? = generated.map { g in
            Applier(knows: { g.knows($0) }) { fh, ctx, autos, args, st in
                try Eval.applyBody(st) { db in try g.apply(Hex.encode(fh), db, ctx, autos, args) }
            }
        }
        let mode = server == nil ? "alone" : "server"
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        for s in scopes {
            let file = directory.appendingPathComponent(ReplicaFile.fileName(s))
            var confirmed = MemoryStore(schema: schema)
            var cursor: Seq = 0
            var pending: [Entry] = []
            if let bytes = try ReplicaFile.read(file) {
                let c = try ReplicaFile.decode(bytes, schema: schema)
                if c.mode != mode { throw SessionError.modeMismatch(was: c.mode, now: mode) }
                confirmed = c.confirmed
                cursor = c.cursor
                pending = c.pending
            }
            let r = Replica.open(schema, s, closures, confirmed, cursor, pending, applier: applier)
            client.subscribe(.whole, r)
            if server == nil {
                authorities[s] = Authority(schema, s, closures, from: Snapshot(seq: cursor, store: confirmed))
            }
            persisted[s] = (cursor, pending.count)
        }
        if let url = server {
            let l = Link(url: url, dial: dial, driven: self)
            link = l
            l.connect()
        } else {
            // Whatever was pending when the last run ended is sequenced now.
            commitAlone()
            try persistAll()
        }
        // Opening replays pending on top of confirmed: a Rebuilt, not a list.
        for s in scopes { _ = client.takeChanges(s) }
    }

    deinit { stopPumping() }

    // MARK: the engine, driven

    public func linkOpened() {
        lock.lock(); client.connected(); lock.unlock()
    }

    public func linkClosed(_ reason: String?) {
        lock.lock(); client.disconnected(); lock.unlock()
    }

    public func linkReceived(_ m: ServerMsg) {
        lock.lock()
        client.recv(m)
        if case .denied(let why) = m { lastNote = "denied: " + why }
        lock.unlock()
    }

    public func linkOutgoing() -> [ClientMsg] {
        lock.lock(); defer { lock.unlock() }
        return client.takeOutgoing()
    }

    /// One turn of the link, then persistence and the change notification.
    /// A timer calls this every 50 ms (`startPumping`), or a test calls it
    /// by hand.
    public func pump(now: Date = Date()) {
        link?.pump(now: now)
        lock.lock()
        do { try persistIfMoved() } catch { lastNote = "\(error)" }
        let notes = collectChanges()
        lock.unlock()
        notify(notes)
    }

    /// Run `pump()` on a private queue every `interval` seconds.
    public func startPumping(every interval: TimeInterval = 0.05) {
        lock.lock()
        if timer != nil { lock.unlock(); return }
        let t = DispatchSource.makeTimerSource(queue: queue)
        t.schedule(deadline: .now() + interval, repeating: interval)
        t.setEventHandler { [weak self] in self?.pump() }
        timer = t
        lock.unlock()
        t.resume()
    }

    public func stopPumping() {
        lock.lock()
        let t = timer
        timer = nil
        lock.unlock()
        t?.cancel()
    }

    /// Ask for the connection again after `goOffline()`.
    public func goOnline() { link?.connect() }

    /// Drop the connection and stop reconnecting; everything authored
    /// meanwhile is pending and pushes on the next `goOnline()`.
    public func goOffline() { link?.disconnect() }

    /// Stop the timer, close the socket, write everything.
    public func close() {
        stopPumping()
        link?.disconnect()
        lock.lock()
        try? persistAll()
        lock.unlock()
    }

    // MARK: mutating

    /// Author an intent by name: the function's autos are drawn here — a
    /// fresh random 16-byte id per `NewId`, the clock in milliseconds for
    /// `Now` — which is the only non-determinism there is, at origin, frozen
    /// in the entry. Applied by the generated code when a `Generated` was
    /// given and has the function, else by the interpreter. Returns the
    /// refusal if there was one; a refusal changes nothing.
    @discardableResult
    public func mutate(name: String, args: Args) -> Refusal? {
        return author(name, args) { fn, hash, autos in
            if let g = self.generated, g.knows(hash) {
                return self.client.mutateWith(fn.scope!, self.freshId(), self.ctx, hash, autos, args) { st in
                    try Eval.applyBody(st) { db in try g.apply(Hex.encode(hash), db, self.ctx, autos, args) }
                }
            }
            return self.client.mutate(fn.scope!, self.freshId(), self.ctx, hash, autos, args)
        }
    }

    /// Author an intent whose body the caller supplies — a generated
    /// mutator, as `session.mutate(name: "add_to_playlist", args: a) { db, ctx, autos in
    /// try HarkenGen.addToPlaylist(db, ctx, autos, a) }`. The session looks the
    /// function up by name for its scope, its hash and its autos, runs the
    /// body as one transaction over the optimistic view, and records the
    /// entry exactly as `mutate(name:args:)` would.
    @discardableResult
    public func mutate(name: String, args: Args, body: (Store, Ctx, Args) throws -> Void) -> Refusal? {
        return author(name, args) { fn, hash, autos in
            self.client.mutateWith(fn.scope!, self.freshId(), self.ctx, hash, autos, args) { st in
                try Eval.applyBody(st) { db in try body(db, self.ctx, autos) }
            }
        }
    }

    private func author(_ name: String, _ args: Args, _ run: (Function, FnHash, Args) -> Result<Entry, Refusal>) -> Refusal? {
        lock.lock()
        guard let (fn, hash) = byName[name], fn.kind == .mutator, fn.scope != nil else {
            lastNote = "no mutator named " + name
            lock.unlock()
            return .refused("no mutator named " + name)
        }
        let autos = drawAutos(fn)
        var refusal: Refusal? = nil
        switch run(fn, hash, autos) {
        case .failure(let why):
            refusal = why
            lastNote = "refused: " + why.text
        case .success:
            commitAlone()
            do { try persistAll() } catch { lastNote = "\(error)" }
        }
        let notes = collectChanges()
        lock.unlock()
        notify(notes)
        return refusal
    }

    /// The autos a function declares, drawn now.
    func drawAutos(_ fn: Function) -> Args {
        var autos: Args = [:]
        for a in fn.autos {
            switch a.auto {
            case .newId: autos[a.name] = .id(freshId())
            case .now: autos[a.name] = .int(Int64(Date().timeIntervalSince1970 * 1000))
            }
        }
        return autos
    }

    func freshId() -> Id {
        var b = [UInt8](repeating: 0, count: 16)
        for i in 0..<16 { b[i] = UInt8.random(in: 0...255, using: &rng) }
        return Id(bytes: b)!
    }

    /// With no server, this peer sequences its own intents: everything
    /// pending is committed and every answer delivered back, in order.
    private func commitAlone() {
        guard server == nil else { return }
        for s in scopes {
            guard var a = authorities[s] else { continue }
            client.withReplica(s) { r in localCommit(&a, &r) }
            authorities[s] = a
        }
    }

    // MARK: reading

    /// The optimistic stores of every scope, as one: what a query reads.
    /// Each scope's tables are its own, so the merge is a union.
    private func mergedView() -> MemoryStore {
        var acc: MemoryStore? = nil
        for s in scopes {
            guard let v = client.scopes[s]?.replica.view else { continue }
            acc = acc.map { $0.merge(v) } ?? v
        }
        return acc ?? MemoryStore(schema: schema)
    }

    /// Run a query by name over the merged view: through the generated
    /// `query` when it has the function, else through the interpreter.
    public func query(name: String, args: Args) throws -> Value {
        lock.lock(); defer { lock.unlock() }
        let st = mergedView()
        if let g = generated, g.names.contains(name) {
            return try g.query(name, st, args)
        }
        guard let (fn, hash) = byName[name] else { throw SessionError.unknownFunction(name) }
        guard fn.kind == .query, let c = closures[hash] else { throw SessionError.notAQuery(name) }
        return try Eval.queryClosure(schema, c, args, st)
    }

    /// Read the merged view directly, as generated queries do
    /// (`session.run { db in try HarkenGen.query("library", db, [:]) }`).
    /// A write through this store is a bug: it reaches nothing durable.
    public func run<T>(_ body: (Store) throws -> T) throws -> T {
        lock.lock(); defer { lock.unlock() }
        return try body(mergedView())
    }

    // MARK: changes

    /// Be told what moved after every mutate and every frame: the changes to
    /// a scope's optimistic view since last time, or that it was rebuilt
    /// and a view must re-hydrate. Returns a token for `unsubscribe`.
    @discardableResult
    public func subscribe(_ f: @escaping (ScopeName, Changes) -> Void) -> Int {
        lock.lock(); defer { lock.unlock() }
        let n = nextSubscriber
        nextSubscriber += 1
        subscribers[n] = f
        return n
    }

    public func unsubscribe(_ token: Int) {
        lock.lock(); defer { lock.unlock() }
        subscribers[token] = nil
    }

    private func collectChanges() -> [(ScopeName, Changes)] {
        var out: [(ScopeName, Changes)] = []
        for s in scopes {
            guard let ch = client.takeChanges(s) else { continue }
            if case .applied(let xs) = ch, xs.isEmpty { continue }
            out.append((s, ch))
        }
        return out
    }

    private func notify(_ notes: [(ScopeName, Changes)]) {
        if notes.isEmpty { return }
        lock.lock()
        let subs = Array(subscribers.values)
        lock.unlock()
        for (s, ch) in notes { for f in subs { f(s, ch) } }
    }

    // MARK: durability

    private func contents(_ s: ScopeName) -> ReplicaFile.Contents? {
        guard let r = client.scopes[s]?.replica else { return nil }
        return ReplicaFile.Contents(scope: s, mode: server == nil ? "alone" : "server", cursor: r.cursor, confirmed: r.confirmed, pending: r.pending)
    }

    private func persist(_ s: ScopeName) throws {
        guard let c = contents(s) else { return }
        try ReplicaFile.write(ReplicaFile.encode(c), to: directory.appendingPathComponent(ReplicaFile.fileName(s)))
        persisted[s] = (c.cursor, c.pending.count)
    }

    private func persistAll() throws {
        for s in scopes { try persist(s) }
    }

    /// Write a scope when its cursor or its pending queue moved since the
    /// last write — which is what a frame from the server changes.
    private func persistIfMoved() throws {
        for s in scopes {
            guard let r = client.scopes[s]?.replica else { continue }
            if let p = persisted[s], p.0 == r.cursor, p.1 == r.pending.count { continue }
            try persist(s)
        }
    }

    // MARK: verifying and status

    /// Ask the authority whether it agrees with every scope's confirmed
    /// state; the answer arrives as `status.lastAgree`. Alone, the answer is
    /// immediate: the authority is here.
    public func verify() {
        lock.lock()
        if server == nil {
            for s in scopes {
                guard let a = authorities[s], let r = client.scopes[s]?.replica else { continue }
                let (n, h) = r.verifyAt()
                let ok = a.log.stateAt(n).map { Hash.stateHash($0) == h } ?? false
                localAgreements.append(Agreement(scope: s, seq: n, ok: ok))
            }
        } else {
            client.verifyAll()
        }
        lock.unlock()
    }

    private var localAgreements: [Agreement] = []

    /// The hash of a scope's confirmed state, and its cursor.
    public func stateHash(_ scope: ScopeName) -> (Seq, [UInt8])? {
        lock.lock(); defer { lock.unlock() }
        return client.scopes[scope]?.replica.verifyAt()
    }

    /// Verdicts against this peer's own intents, over every scope.
    public var rejections: [Rejection] {
        lock.lock(); defer { lock.unlock() }
        return scopes.flatMap { client.scopes[$0]?.replica.rejections ?? [] }
    }

    public var status: SessionStatus {
        lock.lock(); defer { lock.unlock() }
        var cursors: [ScopeName: Seq] = [:]
        var pending = 0
        var rejected = 0
        for s in scopes {
            guard let r = client.scopes[s]?.replica else { continue }
            cursors[s] = r.cursor
            pending += r.pending.count
            rejected += r.rejections.count
        }
        let agree: Agreement? = server == nil
            ? localAgreements.last
            : client.agreed.last.map { Agreement(scope: $0.0, seq: $0.1, ok: $0.2) }
        let linkWord: String
        switch link?.state {
        case nil: linkWord = "alone"
        case .idle?: linkWord = "idle"
        case .connecting?: linkWord = "connecting"
        case .open?: linkWord = "open"
        case .waiting?: linkWord = "waiting"
        }
        return SessionStatus(linked: client.linked, alone: server == nil, cursors: cursors, pending: pending,
                             denied: client.denied, lastAgree: agree, rejections: rejected, link: linkWord)
    }
}
