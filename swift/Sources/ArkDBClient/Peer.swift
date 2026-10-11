// A peer on ArkDB, as `rust/ark-client/src/peer.rs` is one: the engine's
// `Client` over its `Replica`, kept durable on a `Storage` record for record
// as the reference keeps it, and — alone — its own `Authority` over a local
// history kept in `ark::journal`'s shape. What it adds over the sans-io
// client is a driver's: ids and times drawn (`Autos`), who authors (a login,
// or nobody), what is written when, and going alone and back.
//
// The sans-io half is here for a transport of the caller's own:
// `connected`, `recvFrame` and `takeOutgoingFrames`.
import ArkDB

/// What a peer cannot do, or found wrong (`ark_client::Error`).
public struct PeerError: Error, CustomStringConvertible {
    public let what: String
    public init(_ what: String) { self.what = what }
    public var description: String { what }
}

/// A verdict: nothing changed (`Error::Refused`).
public struct Refused: Error, CustomStringConvertible {
    public let refusal: Refusal
    public init(_ refusal: Refusal) { self.refusal = refusal }
    public var description: String { refusalText(refusal) }
}

/// The module a peer runs (`ark_client::Domain`): its closures by hash, and
/// its hash as a client's module has it — every private block stripped, so a
/// server built from the blocks and a phone built from `harken.ark` are one
/// module to each other.
public final class Domain {
    public let module: Module
    /// Every function's closure, by its hash as hex.
    public let closures: [String: Closure]
    private let byName: [String: String]
    /// `Domain::hash`: the module hash of the module stripped of private blocks.
    public let hash: [UInt8]

    public init(_ module: Module) {
        self.module = module
        var cs = [String: Closure]()
        var names = [String: String]()
        for (h, c) in Hash.closures(module) {
            cs[h] = c
            names[c.function.name] = h
        }
        closures = cs
        byName = names
        hash = Hash.moduleHash(stripModule(module))
    }

    /// A function by name: its hash and its definition.
    public func function(_ name: String) -> ([UInt8], Function)? {
        guard let h = byName[name] else { return nil }
        return (Hex.decode(h)!, closures[h]!.function)
    }

    /// `Domain::query`.
    public func query(_ name: String) throws -> ([UInt8], Function) {
        guard let (h, f) = function(name) else { throw PeerError("the module has no function \(name)") }
        if f.kind != .query { throw PeerError("\(name) is not a query") }
        return (h, f)
    }

    /// `Domain::mutator`.
    public func mutator(_ name: String) throws -> ([UInt8], Function) {
        guard let (h, f) = function(name) else { throw PeerError("the module has no function \(name)") }
        if f.kind != .mutator { throw PeerError("\(name) is not a mutator") }
        return (h, f)
    }

    /// `Domain::from_bytes`: the canonical bytes of an `.ark` file, verified.
    public static func fromBytes(_ bytes: [UInt8]) throws -> Domain {
        Domain(try Verify.verify(try Decode.module(try Canon.decode(bytes))))
    }
}

/// How a peer opens (`ark_client::Options`): who authors, the token, alone or
/// not, the autos, and the numbers its link runs on.
public struct Options {
    public var user: String
    public var session: String
    public var token: String?
    public var alone: Bool
    public var autos: Autos

    public init(_ user: String, _ session: String, _ token: String?, alone: Bool = false, autos: Autos = .system()) {
        self.user = user
        self.session = session
        self.token = token
        self.alone = alone
        self.autos = autos
    }

    /// A login of a server.
    public static func server(_ user: String, _ session: String, _ token: String?) -> Options { Options(user, session, token) }

    /// Nobody signed in: authoring as nobody, dialling nothing until a sign-in.
    public static func signedOut() -> Options { Options("", "", nil) }

    /// Its own authority, as `user` under the session `local`.
    public static func alone(_ user: String) -> Options { Options(user, "local", nil, alone: true) }
}

/// Where one of this peer's own intents stands (`ark_client::Standing`).
public enum Standing: Equatable {
    /// Applied here, not yet answered by the authority.
    case pending
    /// In the log.
    case confirmed
    /// Never going to be in the log, and the reason every replica reaches.
    case rejected(String)
    /// Not an intent this peer authored (in this run, or pending from the last).
    case unknown
}

/// A verdict against an intent this peer authored (`ark_client::Rejection`).
public struct Rejection: Equatable {
    public let id: Id
    public let reason: String
}

/// A peer (`ark_client::Peer`): opened over a storage, authoring by name,
/// written down on every `persist` in the reference's records and order.
public final class Peer {
    public let domain: Domain
    private let schema: Schema
    /// The engine's client: the replica, and what is queued for the server.
    public let client: Client
    let storage: Storage
    /// Who this peer authors as.
    public private(set) var ctx: Ctx
    let autos: Autos
    private let writer: ReplicaWriter
    /// Where this peer last shared a log with a server.
    public internal(set) var fork: Fork
    public private(set) var alone = false
    /// Nobody is signed in: nothing is dialled until a sign-in.
    public internal(set) var signedOut: Bool

    private var authority: Authority? = nil
    private var aloneLog: AloneLog? = nil
    private var authored: Set<Id>
    private var rejected = [Id: String]()
    var local = Set<Id>()
    /// Every verdict against an intent of this peer's, oldest first, each once.
    public private(set) var rejections = [Rejection]()
    private let checks = Checks()
    var heardFrames: Int64 = 0
    var badFrames: Int64 = 0

    private final class AloneLog {
        let journal: Journal
        var unwritten = [(Seq, LogEntry, FactList)]()
        init(_ journal: Journal) { self.journal = journal }
    }

    private final class Checks {
        var epoch: Int64 = 0
        var at: Seq? = nil
        var asked = [Bool]()
        var served = false
        var agreed = [(Seq, Bool)]()
        var unknown = [Seq]()
    }

    private init(_ domain: Domain, _ schema: Schema, _ client: Client, _ storage: Storage, _ ctx: Ctx, _ autos: Autos, _ writer: ReplicaWriter, _ fork: Fork, _ authored: Set<Id>, _ signedOut: Bool) {
        self.domain = domain
        self.schema = schema
        self.client = client
        self.storage = storage
        self.ctx = ctx
        self.autos = autos
        self.writer = writer
        self.fork = fork
        self.authored = authored
        self.signedOut = signedOut
    }

    /// The replica.
    public var replica: Replica { client.replica }

    /// The schema this peer's store is laid out under: the module's, or — for
    /// a peer holding a union of the module's scopes — the device's, whose
    /// tables have only the held columns (D2).
    public var storeSchema: Schema { client.replica.schema }

    /// The optimistic store — `confirmed` with `pending` replayed — which every query reads.
    public var store: MemoryStore { client.replica.view }

    /// The cursor: the last sequence confirmed.
    public var cursor: Seq { client.replica.cursor }

    /// Every answer to a verify this peer was asked to make: agreed, or not.
    public var agreed: [(Seq, Bool)] { checks.agreed }

    /// Every verify the authority could not answer.
    public var unknown: [Seq] { checks.unknown }

    /// `Peer::open`: the replica from `storage` (or empty), what was pending
    /// replayed on top, and — alone — sequenced. A storage last used the
    /// other way is carried across: opened alone over a server's replica, the
    /// peer leaves it; opened for a server over local history, the history
    /// is re-queued as a join does.
    public static func open(_ domain: Domain, _ storage: Storage, _ opts: Options) throws -> Peer {
        let schema = domain.module.schema
        let confirmed: MemoryStore
        let cursor: Seq
        let pending: [LogEntry]
        let was: Ctx
        let writer: ReplicaWriter
        let fork: Fork
        var partial: [String: [String]]? = nil
        if let st = try Records.load(storage, schema) {
            let f = st.file
            partial = f.partial
            was = Ctx(f.user, f.session)
            writer = ReplicaWriter.of(storage, st)
            confirmed = f.confirmed
            cursor = f.cursor
            pending = f.pending
            fork = f.fork
        } else {
            let (ps, pst) = try Records.loadPending(storage)
            pending = ps ?? []
            writer = ReplicaWriter.fresh(storage, pst)
            confirmed = MemoryStore.empty(schema)
            cursor = 0
            was = Ctx.nobody
            fork = Fork()
        }
        let hadLog = storage.load(Layout.alone().snapshot) != nil
        let r = Replica.open(confirmed.schema, domain.closures, confirmed, cursor, pending)
        if let p = partial {
            var m = SortedMap<CodePoints, [String]>()
            for (k, v) in p { _ = m.put(CodePoints(k), v) }
            r.partial = m
        }
        r.logId = writer.namedLog
        let client = Client.open(r, .whole, opts.token).withSchema(schema)
        client.module = domain.hash
        _ = client.replica.takeConfirmed()
        var ctx = Ctx(opts.user, opts.session)
        let signedOut = !opts.alone && ctx.isNobody
        if signedOut {
            ctx = hadLog ? Ctx.nobody : was
        } else if !opts.alone && client.replica.pending.contains(where: { $0.actor.isEmpty && $0.session.isEmpty }) {
            client.signIn(ctx, opts.token)
            writer.forgetPending()
        }
        let peer = Peer(domain, schema, client, storage, ctx, opts.autos, writer, fork, Set(pending.map { $0.id }), signedOut)
        if hadLog {
            try peer.resumeAlone()
        } else if opts.alone {
            try peer.startAlone()
        }
        if hadLog && !opts.alone { try peer.forkBackTo(peer.ctx) }
        peer.commitAlone()
        peer.collectRejections()
        try peer.persist()
        _ = peer.client.replica.takeChanges()
        return peer
    }

    // -- who authors -------------------------------------------------------------

    /// `sign_in`: `user` under `session`, proven with `token`; every intent
    /// still pending authored as nobody becomes theirs, and is written down
    /// before anything is said.
    public func signIn(_ user: String, _ session: String, _ token: String?) throws {
        if alone { throw PeerError("a peer alone has no server to sign in to") }
        ctx = Ctx(user, session)
        client.signIn(ctx, token)
        signedOut = false
        collectRejections()
        writer.forgetPending()
        try persist()
        signedIn()
    }

    /// `sign_out`: the token forgotten and the connection dropped; the login is kept.
    public func signOut() {
        if alone { return }
        signedOut = true
        client.token = nil
        dropLink("signed out")
        if client.linked { client.disconnected() }
    }

    /// `set_roles`: the roles this login believes it holds, frozen in what it authors (D1).
    public func setRoles(_ roles: [String]) { ctx = ctx.withRoles(roles) }

    // -- alone and back ----------------------------------------------------------

    /// `leave`, server → alone (`docs/plan-alone.md` §1): the connection
    /// dropped, the fork recorded where the replica stands, and this peer its
    /// own authority from its confirmed store on.
    public func leave() throws {
        if alone { return }
        forgetLink("alone")
        if client.linked { client.disconnected() }
        client.denied = nil
        try startAlone()
        commitAlone()
        collectRejections()
        try persist()
    }

    /// `join`, alone → server, up to the connecting: the local history taken
    /// back out of the confirmed store and re-queued as `login`'s (or
    /// nobody's), written down; the caller connects.
    public func join(_ user: String?, _ session: String?, _ token: String?) throws {
        var who = user.map { Ctx($0, session ?? "") } ?? Ctx.nobody
        if user == nil { who = who.withRoles(ctx.roles) }
        if alone {
            ctx = who
            signedOut = user == nil
            client.token = user != nil ? token : nil
            try forkBackTo(who)
        } else if let u = user {
            try signIn(u, session ?? "", token)
        }
    }

    /// What the next `Hello` proves the login with; a reconnect for it to be said now.
    public func setToken(_ token: String?) { client.token = token }

    // -- a screen's side ---------------------------------------------------------

    /// `mutate`: an intent by name, its autos drawn here once and frozen in
    /// the entry, applied to the optimistic store, kept durable as pending,
    /// pushed if linked. The entry's id, or `Refused`.
    @discardableResult
    public func mutate(_ name: String, _ args: [String: Value]) throws -> Id {
        let (fh, f) = try domain.mutator(name)
        let drawn = autos.draw(f)
        let id = autos.newId()
        let (e, why) = client.mutate(id, ctx, fh, drawn, args)
        if e == nil { throw Refused(why!) }
        authored.insert(id)
        commitAlone()
        collectRejections()
        try persistLog()
        try writer.persistPending(client.replica.pending)
        return id
    }

    /// What a view of the optimistic store is told (`take_changes`).
    public func takeChanges() -> Changes { client.replica.takeChanges() }

    /// `query`: a query by name over the optimistic store, as this peer's user.
    public func query(_ name: String, _ args: [String: Value]) throws -> Value {
        let (h, _) = try domain.query(name)
        let st = store
        do {
            return try Eval.queryClosure(st.schema, domain.closures[Hex.encode(h)]!, ctx, args, st)
        } catch EvalFault.verdict(let r) {
            throw Refused(r)
        } catch EvalFault.bug(let e) {
            throw PeerError("bug: \(name): \(e)")
        }
    }

    /// `check`: the form validator over a partial input; a message per failing field.
    public func check(_ name: String, _ partial: [String: Value]) throws -> Checked {
        guard let (h, _) = domain.function(name) else { throw PeerError("the module has no function \(name)") }
        let st = store
        do {
            return try Eval.check(st.schema, domain.closures[Hex.encode(h)]!, ctx, partial, st)
        } catch let e as EvalFault {
            throw PeerError("\(name): \(e)")
        }
    }

    /// Verdicts against this peer's intents since the last ask (`take_rejections`).
    public func takeRejections() -> [Rejection] {
        collectRejections()
        let out = rejections
        rejections.removeAll()
        return out
    }

    /// Where one of this peer's intents stands: what a screen shows beside the item it made.
    public func standing(_ id: Id) -> Standing {
        if let why = rejected[id] { return .rejected(why) }
        let r = client.replica
        if let hit = r.rejections.first(where: { $0.0 == id }) { return .rejected(refusalText(hit.1)) }
        if r.pending.contains(where: { $0.id == id }) { return .pending }
        if authored.contains(id) { return .confirmed }
        return .unknown
    }

    /// Intents authored here and not yet answered.
    public var pendingLen: Int { client.replica.pending.count }

    /// Whether the engine is linked: the socket is open and `Hello` said.
    public var linked: Bool { client.linked }

    /// How many connections this peer has had.
    public var epoch: Int64 { client.epoch }

    /// Why the server last turned this login away, until a reconnect.
    public var denied: String? { client.denied }

    /// What the room said since the last ask; never durable.
    public func heard() -> [[UInt8]] { client.takeHeard() }

    // Intents a join re-queued that are still pending.
    func joining() -> Int { local.isEmpty ? 0 : client.replica.pending.filter { local.contains($0.id) }.count }

    // -- sans-io: a transport of the caller's own --------------------------------

    public func connected() { client.connected() }

    public func disconnected() { client.disconnected() }

    /// `recv_frame`: a frame from the server, placed and settled.
    public func recvFrame(_ bytes: [UInt8]) throws {
        try placeFrame(bytes)
        _ = settle()
    }

    /// Frames from the server, each placed, then one settle and one write: `pump`'s shape.
    public func pump(_ frames: [[UInt8]]) throws {
        for f in frames { try placeFrame(f) }
        _ = settle()
        collectRejections()
        try persist()
    }

    /// What is queued for the server, as canonical bytes.
    public func takeOutgoingFrames() -> [[UInt8]] { client.takeOutgoing().map { $0.encode() } }

    func placeFrame(_ bytes: [UInt8]) throws {
        switch try ServerMsg.decodeFor(bytes, client.schema) {
        case .msg(let m):
            if case .heard = m { heardFrames += 1 }
            switch m {
            case .batch, .snapshotOf: served()
            default: break
            }
            client.recv(m)
        case .snap(let s):
            served()
            client.recvSnapshot(s)
        }
    }

    private func served() {
        thisConnection()
        checks.served = true
    }

    private func thisConnection() {
        if checks.epoch != client.epoch {
            checks.epoch = client.epoch
            checks.at = nil
            checks.asked.removeAll()
            checks.served = false
        }
    }

    /// `verify`: alone, asked of the local authority; otherwise of the server.
    public func verify() {
        guard let a = authority else { return askVerify(false) }
        let (n, h) = client.replica.verifyAt()
        // `Log::hash_at`, at the head: the alone log's base stands at its head
        // with its entries taken (`Log.takeEntries`), so the head is all it can say.
        if n == a.log.headSeq() {
            checks.agreed.append((n, Hash.stateHash(a.store) == h))
        } else {
            checks.unknown.append(n)
        }
    }

    private func askVerify(_ auto: Bool) {
        thisConnection()
        let before = client.out.count
        client.verifyAll()
        if client.out.count > before {
            checks.asked.append(auto)
            checks.at = client.replica.cursor
        }
    }

    /// `settle`: the client settled, verify answers sorted, and a verify asked after a page.
    func settle() -> [Seq] {
        let paging = client.more
        client.settle()
        thisConnection()
        var disagreed = [Seq]()
        let answers = client.agreed
        client.agreed.removeAll()
        for (n, answer) in answers {
            let auto = checks.asked.isEmpty ? false : checks.asked.removeFirst()
            if let a = answer {
                if !a { disagreed.append(n) }
                if !auto || !a { checks.agreed.append((n, a)) }
            } else if !auto {
                checks.unknown.append(n)
            }
        }
        let due = checks.served && !paging && checks.at != client.replica.cursor
        if authority == nil && client.linked && due { askVerify(true) }
        return disagreed
    }

    // -- writing it down ---------------------------------------------------------

    /// `persist`: what moved since the last write — the local history first,
    /// then the pending intents, the login, and the confirmed store, each as
    /// the reference writes it.
    public func persist() throws {
        try persistLog()
        try writer.persist(client.replica, fork, ctx)
    }

    // Alone, what is pending sequenced, and the entries taken out of the
    // authority's memory into the log's next write.
    func commitAlone() {
        guard let a = authority else { return }
        localCommit(a, client.replica)
        aloneLog?.unwritten.append(contentsOf: a.log.takeEntries())
    }

    // The entries sequenced alone since the last write, as one page of the
    // local history; then its pages merged.
    private func persistLog() throws {
        guard let l = aloneLog, let to = l.unwritten.last?.0 else { return }
        var page = [UInt8]()
        for (n, e, f) in l.unwritten { page += LogRecords.encodeRecord(n, e, f) }
        try l.journal.append(storage, page, to)
        l.unwritten.removeAll()
        try l.journal.merge(storage)
    }

    // The authority a peer alone is: the confirmed store as its state at
    // `head`, the ids its local history holds, and no entries.
    private func aloneAuthority(_ head: Seq, _ ids: [Id: Seq]) -> Authority {
        let a = Authority(schema, domain.closures)
        a.log = Log(head, MemoryStore.empty(schema), [], nil)
        for (k, v) in ids { a.log.ids[k] = v }
        a.store = client.replica.confirmed.copy()
        return a
    }

    // `resume_alone`: carry on alone from the local history on the storage.
    private func resumeAlone() throws {
        let layout = Layout.alone()
        let cursor = client.replica.cursor
        var ids = [Id: Seq]()
        var ahead = [(Seq, LogEntry, FactList?)]()
        let (journal, o) = try Journal.open(storage, layout, schema) { n, e, f in
            ids[e.id] = n
            if n > cursor { ahead.append((n, e, f)) }
        }
        guard let base = o.snapshot else { throw StorageError("the local history has no snapshot") }
        let fork = Fork(base.logId, base.horizon())
        let head = o.head
        var r = client.replica
        if cursor > head || cursor < fork.cursor {
            let st = base.baseStore
            _ = try LogRecords.read(storage, layout, schema) { n, _, f in
                if n <= head { st.applyChanges(f) }
            }
            r = Replica.open(r.schema, r.bodies, st, head, r.pending)
            client.replica = r
        } else if !ahead.isEmpty {
            r.receiveBatch(ahead)
        }
        if r.pending.contains(where: { e in ids[e.id].map { $0 <= head } == true }) {
            let pending = r.pending.filter { e in ids[e.id].map { $0 > head } ?? true }
            r = Replica.open(r.schema, r.bodies, r.confirmed.copy(), r.cursor, pending)
            client.replica = r
            writer.forgetPending()
        }
        r.logId = nil
        authority = aloneAuthority(head, ids)
        self.fork = fork
        alone = true
        signedOut = false
        aloneLog = AloneLog(journal)
    }

    // `start_alone`, server → alone: the fork where the replica stands, the
    // local history's snapshot its confirmed store, written before anything
    // else moves; what the server sent and was not applied left behind.
    private func startAlone() throws {
        let r = client.replica
        r.inbox.removeAll()
        r.through = r.cursor
        let fork = Fork(r.logId, r.cursor)
        let st = r.confirmed.copy()
        let base = Log(fork.cursor, st, Hash.stateHash(st), fork.logId)
        let journal = try Journal.create(storage, Layout.alone(), base)
        r.logId = nil
        authority = aloneAuthority(fork.cursor, [:])
        self.fork = fork
        alone = true
        signedOut = false
        local.removeAll()
        aloneLog = AloneLog(journal)
    }

    // `fork_back_to`, alone → server up to the connecting: the local history
    // read back from the storage, taken out of the confirmed store, and
    // re-queued as `who`'s; written down, and only then the history removed.
    private func forkBackTo(_ who: Ctx) throws {
        try persistLog()
        let fork = self.fork
        let layout = Layout.alone()
        var facts = [FactList]()
        var requeue = [LogEntry]()
        let o = try LogRecords.read(storage, layout, schema) { n, e, f in
            if n > fork.cursor {
                requeue.append(e.by(who.user, who.session))
                facts.append(f)
            }
        }
        let cursor = client.replica.cursor
        if o.head != cursor || cursor - fork.cursor != Int64(facts.count) {
            throw StorageError("the local history runs from \(fork.cursor) to \(o.head) and the store is at \(cursor)")
        }
        local = Set(requeue.map { $0.id })
        authored.formUnion(local)
        client.replica.forkBack(facts, fork.cursor, fork.logId, requeue)
        authority = nil
        aloneLog = nil
        alone = false
        collectRejections()
        writer.forgetPending()
        try persist()
        try Journal.destroy(storage, layout)
    }

    func collectRejections() {
        let rs = client.replica.rejections
        client.replica.rejections.removeAll()
        for (id, why) in rs {
            if rejected[id] != nil { continue }
            let reason = refusalText(why)
            rejected[id] = reason
            rejections.append(Rejection(id: id, reason: reason))
        }
    }

    // The link's hooks, which `Link.swift` fills in: a sign-in redials, a
    // sign-out or a leave stops the dialling.
    func signedIn() {}
    func dropLink(_ why: String) {}
    func forgetLink(_ why: String) {}
}
