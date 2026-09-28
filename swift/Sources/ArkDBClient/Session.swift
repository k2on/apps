import Foundation
import ArkDB

public enum SessionError: Error, CustomStringConvertible {
    case badModule(String)
    case unknownFunction(String)
    case notAMutator(String)
    case notAQuery(String)
    /// A query refused: a failed check, a guard, an overflow.
    case refused(Refusal)
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
        case .refused(let r): return "refused: " + refusalText(r)
        case .corrupt(let s): return "corrupt: " + s
        case .io(let s): return "io: " + s
        case .modeMismatch(let was, let now): return "the directory was opened \(was) before and \(now) now"
        }
    }
}

/// What the authority last said about this peer's confirmed state.
public struct Agreement: Equatable {
    public var seq: Seq
    public var ok: Bool
    public init(seq: Seq, ok: Bool) { self.seq = seq; self.ok = ok }
}

/// Where one of this peer's own intents stands: what a screen shows beside
/// the item it made.
public enum Standing: Equatable {
    /// Applied here, not yet answered by the authority.
    case pending
    /// In the log.
    case confirmed
    /// Never going to be in the log, and the reason every replica reaches
    /// (`refusalText`): the sentence to show beside the item that did not
    /// happen.
    case rejected(String)
    /// Not an intent this peer authored (in this run, or pending from the
    /// last).
    case unknown
}

public struct SessionStatus: Equatable {
    /// The socket is open and the hello has been said.
    public var linked: Bool
    /// This peer is its own authority (no server was given).
    public var alone: Bool
    /// The last confirmed sequence.
    public var cursor: Seq
    /// Intents authored here that no verdict has answered.
    public var pending: Int
    /// Somebody has signed in; until then every intent is authored as
    /// `Ctx.nobody`, kept pending, and nothing is said to any server.
    public var signedIn: Bool
    /// The server turned this peer away, and why.
    public var denied: String?
    /// The last `Agree` the authority sent.
    public var lastAgree: Agreement?
    /// Verdicts against this peer's own intents so far.
    public var rejections: Int
    /// The link, in a word: idle, connecting, open, waiting.
    public var link: String
}

/// Everything an app needs around the sans-io machines: the replica of the
/// log; the `Client` and the `Link` that drives it — or, with no server, an
/// `Authority` that sequences what this peer authors (docs/arkdb.md
/// §3.10); the file under a directory that makes it durable; the two
/// things a screen does, `mutate` and `query`; and where each intent it
/// authored stands (`standing`).
///
/// Thread-safe: every method takes one lock, and change subscribers are
/// called after it is released, on whichever thread pumped or mutated.
public final class Session: LinkDriven {
    public let module: Module
    public let schema: Schema
    public let directory: URL
    /// Who authors here: the login, or `Ctx.nobody` while signed out.
    public private(set) var ctx: Ctx
    public let server: URL?
    /// The procedures this peer runs natively, by hash (`Module.procedures()`
    /// of its authored domain); everything else through the interpreter,
    /// or by facts.
    private let natives: [FnHash: Procedure]
    private let closures: [FnHash: Closure]
    private var byName: [String: (Function, FnHash)] = [:]
    private var client: Client
    private var authority: Authority?
    private var link: Link?
    private let lock = NSLock()
    private var subscribers: [Int: (Changes) -> Void] = [:]
    private var nextSubscriber = 1
    private var timer: DispatchSourceTimer?
    private let queue = DispatchQueue(label: "arkdb.session")
    private var persisted: (Seq, Int) = (0, 0)
    private var rng = SystemRandomNumberGenerator()
    /// Every intent authored here: this run's, and whatever was pending
    /// when the last one ended.
    private var authored: Set<Id> = []
    /// Every verdict so far, in arrival order, and by id.
    private var verdicts: [Rejection] = []
    private var rejected: [Id: String] = [:]
    /// What happened last, for a status line: a refusal, a denial, a bug.
    public private(set) var lastNote: String?

    // MARK: opening

    /// Open the session: decode the module, open the replica from the
    /// directory (or empty), and either dial the server or stand as the
    /// log's authority. `session` is the login; under dev auth the
    /// server calls every login `"dev"` and the token is the user's name.
    ///
    /// `procedures` is the domain as native code — `module().procedures()`
    /// of the authored domain whose `emit()` the bytes are. Every entry,
    /// authored here or replayed from the log, whose function one of them
    /// is runs natively; the rest through the interpreter, or by facts.
    public static func open(directory: URL, module bytes: [UInt8], procedures: [(FnHash, Procedure)] = [],
                            user: String, session: String = "dev",
                            server: URL?, token: String? = nil,
                            dial: LinkDial? = nil) throws -> Session {
        let m: Module
        do {
            m = try Decode.fromValue(try Canon.decode(bytes))
        } catch {
            throw SessionError.badModule("\(error)")
        }
        return try Session(module: m, directory: directory, ctx: Ctx(user: user, session: session), server: server,
                           token: token ?? user, procedures: procedures, dial: dial ?? WebSocketTransport.dial)
    }

    /// Open the session with nobody signed in: everything authored is
    /// authored as `Ctx.nobody`, kept pending and written to the directory,
    /// and no connection is made — alone, it is sequenced here as ever.
    /// `signIn` makes all of it the signer's and connects.
    public static func openSignedOut(directory: URL, module bytes: [UInt8], procedures: [(FnHash, Procedure)] = [],
                                     server: URL?, dial: LinkDial? = nil) throws -> Session {
        let m: Module
        do {
            m = try Decode.fromValue(try Canon.decode(bytes))
        } catch {
            throw SessionError.badModule("\(error)")
        }
        return try Session(module: m, directory: directory, ctx: .nobody, server: server,
                           token: nil, procedures: procedures, dial: dial ?? WebSocketTransport.dial)
    }

    init(module: Module, directory: URL, ctx: Ctx, server: URL?, token: String?, procedures: [(FnHash, Procedure)], dial: @escaping LinkDial) throws {
        self.module = module
        self.schema = module.schema
        self.directory = directory
        self.ctx = ctx
        self.server = server
        var natives: [FnHash: Procedure] = [:]
        for (h, p) in procedures { natives[h] = p }
        self.natives = natives
        let closures = Hash.closures(module)
        self.closures = closures
        let mode = server == nil ? "alone" : "server"
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var confirmed = MemoryStore(schema: module.schema)
        var cursor: Seq = 0
        var pending: [Entry] = []
        if let bytes = try ReplicaFile.read(directory.appendingPathComponent(ReplicaFile.fileName)) {
            let c = try ReplicaFile.decode(bytes, schema: module.schema)
            if c.mode != mode { throw SessionError.modeMismatch(was: c.mode, now: mode) }
            confirmed = c.confirmed
            cursor = c.cursor
            pending = c.pending
        }
        self.client = Client(Replica.open(module.schema, closures, confirmed, cursor, pending, natives: natives), .whole, token: token)
        if server == nil {
            self.authority = Authority(module.schema, closures, from: Snapshot(seq: cursor, store: confirmed))
        }
        self.persisted = (cursor, pending.count)
        self.authored = Set(pending.map { $0.id })
        for (h, c) in closures { byName[c.fn.name] = (c.fn, h) }
        // Opened signed in over work done before anyone had: it is the
        // signer's, as `signIn` would have made it.
        if !ctx.isNobody && pending.contains(where: { $0.actor.isEmpty && $0.session.isEmpty }) {
            client.signIn(ctx, token: token)
            try persist()
        }
        collectRejections()
        if let url = server {
            let l = Link(url: url, dial: dial, driven: self)
            link = l
            if !ctx.isNobody { l.connect() }
        } else {
            // Whatever was pending when the last run ended is sequenced now.
            commitAlone()
            try persist()
        }
        // Opening replays pending on top of confirmed: a Rebuilt, not a list.
        _ = client.takeChanges()
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
        collectRejections()
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

    /// Ask for the connection again after `goOffline()`. Signed out,
    /// there is nobody to connect as, and this does nothing.
    public func goOnline() {
        lock.lock()
        let nobody = ctx.isNobody
        lock.unlock()
        if !nobody { link?.connect() }
    }

    /// Somebody signs in: every intent authored so far as nobody becomes
    /// theirs, under this login (`Client.signIn`), the view is replayed so
    /// the rows say whose they are, the file is written, and the link
    /// connects — the first hello pushes all of it. Each entry keeps its
    /// id, so `standing` goes on answering about it. `token` defaults to
    /// the user's name, as dev auth reads one.
    public func signIn(user: String, session: String = "dev", token: String? = nil) {
        lock.lock()
        ctx = Ctx(user: user, session: session)
        client.signIn(ctx, token: token ?? user)
        collectRejections()
        commitAlone()
        do { try persist() } catch { lastNote = "\(error)" }
        let notes = collectChanges()
        lock.unlock()
        notify(notes)
        link?.connect()
    }

    /// Drop the connection and stop reconnecting; everything authored
    /// meanwhile is pending and pushes on the next `goOnline()`.
    public func goOffline() { link?.disconnect() }

    /// Stop the timer, close the socket, write everything.
    public func close() {
        stopPumping()
        link?.disconnect()
        lock.lock()
        try? persist()
        lock.unlock()
    }

    // MARK: mutating

    /// Author an intent by name: the function's autos are drawn here — a
    /// fresh random 16-byte id per `NewId`, the clock in milliseconds for
    /// `Now` — which is the only non-determinism there is, at origin, frozen
    /// in the entry. Applied natively when the session holds the procedure,
    /// else by the interpreter. Returns the entry's id, which `standing`
    /// answers about, or the refusal; a refusal changes nothing. `args` is
    /// the input (`Input.args` of an authored input struct).
    @discardableResult
    public func author(name: String, args: Args) -> Result<Id, Refusal> {
        lock.lock()
        guard let (fn, hash) = byName[name], fn.kind == .mutator else {
            lastNote = "no mutator named " + name
            lock.unlock()
            return .failure(.refused("no mutator named " + name))
        }
        let autos = drawAutos(fn)
        let result: Result<Id, Refusal>
        switch client.mutate(freshId(), ctx, hash, autos, args) {
        case .failure(let why):
            result = .failure(why)
            lastNote = "refused: " + refusalText(why)
        case .success(let e):
            result = .success(e.id)
            authored.insert(e.id)
            commitAlone()
            do { try persist() } catch { lastNote = "\(error)" }
        }
        let notes = collectChanges()
        lock.unlock()
        notify(notes)
        return result
    }

    /// `author`, for a caller that only wants the refusal, if there was one.
    @discardableResult
    public func mutate(name: String, args: Args) -> Refusal? {
        if case .failure(let why) = author(name: name, args: args) { return why }
        return nil
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
        guard server == nil, var a = authority else { return }
        client.withReplica { r in localCommit(&a, &r) }
        authority = a
        collectRejections()
    }

    /// Move the replica's verdicts beside the ids they are about, with the
    /// sentence each carries, so a screen can ask about any one of them.
    private func collectRejections() {
        for r in client.withReplica({ $0.takeRejections() }) {
            rejected[r.id] = refusalText(r.why)
            verdicts.append(r)
        }
    }

    // MARK: reading

    /// The optimistic store: what a query reads.
    private var view: MemoryStore { return client.replica.view }

    /// Run a query by name over the optimistic view, as this peer's user:
    /// natively when the session holds the procedure, else through the
    /// interpreter. A refusal — a failed check, a guard — is thrown as
    /// `SessionError.refused`.
    public func query(name: String, args: Args = [:]) throws -> Value {
        lock.lock(); defer { lock.unlock() }
        let st = view
        guard let (fn, hash) = byName[name] else { throw SessionError.unknownFunction(name) }
        guard fn.kind == .query, let c = closures[hash] else { throw SessionError.notAQuery(name) }
        let r: Result<Value, Refusal>
        if let run = natives[hash]?.query {
            r = try run(ctx, args, st)
        } else {
            r = try Eval.queryResult(schema, c, args, st, ctx: ctx)
        }
        switch r {
        case .success(let v): return v
        case .failure(let why): throw SessionError.refused(why)
        }
    }

    /// The form validator (AUTHORING.md §1.3): a procedure's input checks
    /// over the fields present, against the optimistic view — each failing
    /// field's message, and the values as the checks normalised them.
    public func validate(name: String, partial: Args) throws -> (messages: [(String, String)], values: Args) {
        lock.lock(); defer { lock.unlock() }
        guard let (_, hash) = byName[name], let c = closures[hash] else { throw SessionError.unknownFunction(name) }
        return try Eval.check(schema, c, partial, view, ctx: ctx)
    }

    /// Read the optimistic view directly. A write through this store is a bug:
    /// it reaches nothing durable.
    public func run<T>(_ body: (Store) throws -> T) throws -> T {
        lock.lock(); defer { lock.unlock() }
        return try body(view)
    }

    // MARK: changes

    /// Be told what moved after every mutate and every frame: the changes to
    /// the optimistic view since last time, or that it was rebuilt and a
    /// view must re-hydrate. Returns a token for `unsubscribe`.
    @discardableResult
    public func subscribe(_ f: @escaping (Changes) -> Void) -> Int {
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

    private func collectChanges() -> Changes? {
        let ch = client.takeChanges()
        if case .applied(let xs) = ch, xs.isEmpty { return nil }
        return ch
    }

    private func notify(_ note: Changes?) {
        guard let ch = note else { return }
        lock.lock()
        let subs = Array(subscribers.values)
        lock.unlock()
        for f in subs { f(ch) }
    }

    // MARK: durability

    private func persist() throws {
        let r = client.replica
        let c = ReplicaFile.Contents(mode: server == nil ? "alone" : "server", cursor: r.cursor, confirmed: r.confirmed, pending: r.pending)
        try ReplicaFile.write(ReplicaFile.encode(c), to: directory.appendingPathComponent(ReplicaFile.fileName))
        persisted = (c.cursor, c.pending.count)
    }

    /// Write when the cursor or the pending queue moved since the last
    /// write — which is what a frame from the server changes.
    private func persistIfMoved() throws {
        let r = client.replica
        if persisted.0 == r.cursor && persisted.1 == r.pending.count { return }
        try persist()
    }

    // MARK: verifying and status

    /// Ask the authority whether it agrees with the confirmed state; the
    /// answer arrives as `status.lastAgree`. Alone, the answer is
    /// immediate: the authority is here.
    public func verify() {
        lock.lock()
        if let a = authority {
            let (n, h) = client.replica.verifyAt()
            let ok = a.log.stateAt(n).map { Hash.stateHash($0) == h } ?? false
            localAgreements.append(Agreement(seq: n, ok: ok))
        } else {
            client.verifyAll()
        }
        lock.unlock()
    }

    private var localAgreements: [Agreement] = []

    /// The hash of the confirmed state, and its cursor.
    public func stateHash() -> (Seq, [UInt8]) {
        lock.lock(); defer { lock.unlock() }
        return client.replica.verifyAt()
    }

    /// Where one of this peer's intents stands: pending, confirmed, or
    /// rejected with the sentence to show beside it.
    public func standing(_ id: Id) -> Standing {
        lock.lock(); defer { lock.unlock() }
        if let why = rejected[id] { return .rejected(why) }
        if client.replica.pending.contains(where: { $0.id == id }) { return .pending }
        return authored.contains(id) ? .confirmed : .unknown
    }

    /// Verdicts against this peer's own intents, in the order they arrived;
    /// each one's `reason` is the sentence a screen shows.
    public var rejections: [Rejection] {
        lock.lock(); defer { lock.unlock() }
        return verdicts
    }

    public var status: SessionStatus {
        lock.lock(); defer { lock.unlock() }
        let r = client.replica
        let agree: Agreement? = server == nil
            ? localAgreements.last
            : client.agreed.last.map { Agreement(seq: $0.0, ok: $0.1) }
        let linkWord: String
        switch link?.state {
        case nil: linkWord = "alone"
        case .idle?: linkWord = "idle"
        case .connecting?: linkWord = "connecting"
        case .open?: linkWord = "open"
        case .waiting?: linkWord = "waiting"
        }
        return SessionStatus(linked: client.linked, alone: server == nil, cursor: r.cursor, pending: r.pending.count,
                             signedIn: !ctx.isNobody, denied: client.denied, lastAgree: agree, rejections: verdicts.count, link: linkWord)
    }
}
