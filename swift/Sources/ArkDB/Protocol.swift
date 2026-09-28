import Foundation

/// How a client holds the log. `whole` replays intents and is exact;
/// `byFacts` is fed the facts of every entry and needs no closure.
public enum Mode: Equatable {
    case whole
    case byFacts
}

public struct Subscription: Equatable {
    /// The cursor: the last sequence applied.
    public var since: Seq
    public var mode: Mode
    public init(since: Seq, mode: Mode) { self.since = since; self.mode = mode }
}

/// §12 The frames a client sends.
public enum ClientMsg {
    case hello(sub: Subscription, token: String?, spec: Int)
    case push(entries: [Entry])
    case needFacts(seqs: [Seq])
    case needClosures(hashes: [FnHash])
    case verify(seq: Seq, hash: [UInt8])
    case say(frame: [UInt8])
}

public struct BatchItem {
    public var seq: Seq
    public var entry: Entry
    public var facts: Facts?
    public init(seq: Seq, entry: Entry, facts: Facts?) { self.seq = seq; self.entry = entry; self.facts = facts }
}

public struct FactsItem {
    public var seq: Seq
    public var facts: Facts
    public init(seq: Seq, facts: Facts) { self.seq = seq; self.facts = facts }
}

public struct ClosureItem {
    public var hash: FnHash
    public var closure: Closure
    public init(hash: FnHash, closure: Closure) { self.hash = hash; self.closure = closure }
}

/// §12 The frames a server sends.
public enum ServerMsg {
    case batch(items: [BatchItem], hasMore: Bool)
    case factsFor(items: [FactsItem])
    case snapshotOf(seq: Seq, hash: [UInt8], rows: [TableName: [Value]])
    case ack(ids: [Id], seqs: [Seq])
    /// A verdict against one entry, with the reason every replica would
    /// reach (`refusalText`): what a screen shows beside the item that did
    /// not happen.
    case reject(id: Id, reason: String)
    case denied(reason: String)
    case closures(items: [ClosureItem])
    case agree(seq: Seq, hash: [UInt8], ok: Bool)
    case heard(frame: [UInt8])
}

/// §12.5 The reason a `reject` carries, as `Ark.Protocol.refusalText`: a
/// mutator's own refusal is its text, word for word, because that is what
/// an author wrote for a person to read; the store's constraint refusals
/// are named in a sentence.
public func refusalText(_ r: Refusal) -> String {
    switch r {
    case .refused(let t): return t
    case .noSuchTable(let t): return "no table " + t
    case .malformedRow(let t, let why): return t + ": " + why
    case .notNull(let t, let c): return t + "." + c + " may not be empty"
    case .uniqueViolation(let t, let cs): return t + ": another row has the same " + cs.joined(separator: ", ")
    case .missingParent(let t, let c, let p): return t + "." + c + " names no " + p
    case .stillReferenced(let t, let child): return t + ": still referenced by " + child
    }
}

/// Entries per page.
public let batchLimit = 256

/// The frames as values, and back: `Ark.Protocol`'s `clientValue`,
/// `serverValue`, `entryValue`, `changeValue` and their inverses.
public enum Wire {
    static func node(_ t: String, _ fs: [(String, Value)]) -> Value {
        var m: [String: Value] = ["t": .text(t)]
        for (k, v) in fs { m[k] = v }
        return .record(m)
    }

    static func int<T: BinaryInteger>(_ n: T) -> Value { return .int(Int64(n)) }

    public static func entryValue(_ e: Entry) -> Value {
        return .record([
            "id": .id(e.id),
            "actor": .text(e.actor),
            "session": .text(e.session),
            "fn": .bytes(e.fn),
            "args": .record(e.args),
            "autos": .record(e.autos),
        ])
    }

    public static func changeValue(_ c: Change) -> Value {
        switch c {
        case .add(let t, let r): return node("add", [("table", .text(t)), ("row", .record(r))])
        case .remove(let t, let r): return node("remove", [("table", .text(t)), ("row", .record(r))])
        case .edit(let t, let o, let n): return node("edit", [("table", .text(t)), ("old", .record(o)), ("new", .record(n))])
        }
    }

    public static func factsValue(_ f: Facts) -> Value { return .list(f.map(changeValue)) }

    public static func clientValue(_ m: ClientMsg) -> Value {
        switch m {
        case .hello(let sub, let tok, let spec):
            return node("hello", [
                ("since", int(sub.since)),
                ("mode", .text(sub.mode == .whole ? "whole" : "facts")),
                ("token", tok.map { .text($0) } ?? .null),
                ("spec", int(spec)),
            ])
        case .push(let es): return node("push", [("entries", .list(es.map(entryValue)))])
        case .needFacts(let ns): return node("need_facts", [("seqs", .list(ns.map(int)))])
        case .needClosures(let hs): return node("need_closures", [("hashes", .list(hs.map { .bytes($0) }))])
        case .verify(let n, let h): return node("verify", [("seq", int(n)), ("hash", .bytes(h))])
        case .say(let f): return node("say", [("say", .bytes(f))])
        }
    }

    public static func serverValue(_ m: ServerMsg) -> Value {
        switch m {
        case .batch(let items, let more):
            return node("batch", [
                ("items", .list(items.map { .record(["seq": int($0.seq), "entry": entryValue($0.entry), "facts": $0.facts.map(factsValue) ?? .null]) })),
                ("has_more", .bool(more)),
            ])
        case .factsFor(let items):
            return node("facts", [("items", .list(items.map { .record(["seq": int($0.seq), "facts": factsValue($0.facts)]) }))])
        case .snapshotOf(let n, let h, let rows):
            return node("snapshot", [("seq", int(n)), ("hash", .bytes(h)), ("rows", .record(rows.mapValues { .list($0) }))])
        case .ack(let ids, let ns):
            return node("ack", [("ids", .list(ids.map { .id($0) })), ("seqs", .list(ns.map(int)))])
        case .reject(let i, let why):
            return node("reject", [("id", .id(i)), ("reason", .text(why))])
        case .denied(let why): return node("denied", [("reason", .text(why))])
        case .closures(let cs):
            return node("closures", [("items", .list(cs.map { .record(["hash": .bytes($0.hash), "closure": Hash.closureValue($0.closure)]) }))])
        case .agree(let n, let h, let ok):
            return node("agree", [("seq", int(n)), ("hash", .bytes(h)), ("ok", .bool(ok))])
        case .heard(let f): return node("heard", [("hear", .bytes(f))])
        }
    }

    // MARK: decoding

    static func bad(_ what: String) -> ModuleDecodeError { return ModuleDecodeError(["frame"], what) }

    static func structOf(_ v: Value) throws -> [String: Value] {
        guard case .record(let m) = v else { throw bad("expected a struct") }
        return m
    }

    static func need(_ m: [String: Value], _ k: String) throws -> Value {
        guard let v = m[k] else { throw bad("missing " + k) }
        return v
    }

    static func text(_ v: Value) throws -> String {
        guard case .text(let t) = v else { throw bad("expected text") }
        return t
    }

    static func bytes(_ v: Value) throws -> [UInt8] {
        guard case .bytes(let b) = v else { throw bad("expected bytes") }
        return b
    }

    static func int64(_ v: Value) throws -> Int64 {
        guard case .int(let n) = v else { throw bad("expected an int") }
        return n
    }

    static func bool(_ v: Value) throws -> Bool {
        guard case .bool(let b) = v else { throw bad("expected a bool") }
        return b
    }

    static func ident(_ v: Value) throws -> Id {
        guard case .id(let i) = v else { throw bad("expected an id") }
        return i
    }

    static func list<T>(_ v: Value, _ f: (Value) throws -> T) throws -> [T] {
        guard case .list(let xs) = v else { throw bad("expected a list") }
        return try xs.map(f)
    }

    public static func entryFromValue(_ v: Value) throws -> Entry {
        let m = try structOf(v)
        return Entry(
            id: try ident(try need(m, "id")),
            actor: try text(try need(m, "actor")),
            session: try text(try need(m, "session")),
            fn: try bytes(try need(m, "fn")),
            args: try structOf(try need(m, "args")),
            autos: try structOf(try need(m, "autos")))
    }

    public static func changeFromValue(_ v: Value) throws -> Change {
        let m = try structOf(v)
        let t = try text(try need(m, "t"))
        let tbl = try text(try need(m, "table"))
        switch t {
        case "add": return .add(tbl, try structOf(try need(m, "row")))
        case "remove": return .remove(tbl, try structOf(try need(m, "row")))
        case "edit": return .edit(tbl, try structOf(try need(m, "old")), try structOf(try need(m, "new")))
        default: throw bad("unknown change " + t)
        }
    }

    public static func clientFromValue(_ v: Value) throws -> ClientMsg {
        let m = try structOf(v)
        let t = try text(try need(m, "t"))
        switch t {
        case "hello":
            let md: Mode
            switch try text(try need(m, "mode")) {
            case "whole": md = .whole
            case "facts": md = .byFacts
            case let other: throw bad("unknown mode " + other)
            }
            let sub = Subscription(since: try int64(try need(m, "since")), mode: md)
            let tokV = try need(m, "token")
            let tok: String? = tokV.isNull() ? nil : try text(tokV)
            return .hello(sub: sub, token: tok, spec: Int(try int64(try need(m, "spec"))))
        case "push": return .push(entries: try list(try need(m, "entries"), entryFromValue))
        case "need_facts": return .needFacts(seqs: try list(try need(m, "seqs"), int64))
        case "need_closures": return .needClosures(hashes: try list(try need(m, "hashes"), bytes))
        case "verify": return .verify(seq: try int64(try need(m, "seq")), hash: try bytes(try need(m, "hash")))
        case "say": return .say(frame: try bytes(try need(m, "say")))
        default: throw bad("unknown client frame " + t)
        }
    }

    public static func serverFromValue(_ v: Value) throws -> ServerMsg {
        let m = try structOf(v)
        let t = try text(try need(m, "t"))
        switch t {
        case "batch":
            let items = try list(try need(m, "items")) { x -> BatchItem in
                let im = try structOf(x)
                let fv = try need(im, "facts")
                let f: Facts? = fv.isNull() ? nil : try list(fv, changeFromValue)
                return BatchItem(seq: try int64(try need(im, "seq")), entry: try entryFromValue(try need(im, "entry")), facts: f)
            }
            return .batch(items: items, hasMore: try bool(try need(m, "has_more")))
        case "facts":
            let items = try list(try need(m, "items")) { x -> FactsItem in
                let im = try structOf(x)
                return FactsItem(seq: try int64(try need(im, "seq")), facts: try list(try need(im, "facts"), changeFromValue))
            }
            return .factsFor(items: items)
        case "snapshot":
            let rm = try structOf(try need(m, "rows"))
            var rows: [TableName: [Value]] = [:]
            for (k, x) in rm { rows[k] = try list(x) { $0 } }
            return .snapshotOf(seq: try int64(try need(m, "seq")), hash: try bytes(try need(m, "hash")), rows: rows)
        case "ack":
            return .ack(ids: try list(try need(m, "ids"), ident), seqs: try list(try need(m, "seqs"), int64))
        case "reject":
            return .reject(id: try ident(try need(m, "id")), reason: try text(try need(m, "reason")))
        case "denied": return .denied(reason: try text(try need(m, "reason")))
        case "closures":
            let items = try list(try need(m, "items")) { x -> ClosureItem in
                let im = try structOf(x)
                return ClosureItem(hash: try bytes(try need(im, "hash")), closure: try Decode.closureFromValue(try need(im, "closure")))
            }
            return .closures(items: items)
        case "agree":
            return .agree(seq: try int64(try need(m, "seq")), hash: try bytes(try need(m, "hash")), ok: try bool(try need(m, "ok")))
        case "heard": return .heard(frame: try bytes(try need(m, "hear")))
        default: throw bad("unknown server frame " + t)
        }
    }
}

// MARK: - The client

/// A peer's end of one connection: its replica of the log, and what it has
/// queued. Sans-io: a transport feeds it frames and drains what it queues.
public struct Client {
    public var schema: Schema { return replica.schema }
    public private(set) var replica: Replica
    public private(set) var mode: Mode
    public var token: String?
    public private(set) var linked: Bool
    /// Counts connections, so a live room that has never heard of this
    /// device can be told apart from one that has.
    public private(set) var epoch: Int
    var out: [ClientMsg] // oldest first
    var heardFrames: [[UInt8]] // oldest first
    public private(set) var denied: String?
    public private(set) var agreed: [(Seq, Bool)]

    /// A client over the replica as opened from what was durable.
    public init(_ replica: Replica, _ mode: Mode, token: String?) {
        self.replica = replica
        self.mode = mode
        self.token = token
        self.linked = false
        self.epoch = 0
        self.out = []
        self.heardFrames = []
        self.denied = nil
        self.agreed = []
    }

    public static func openClient(_ replica: Replica, _ mode: Mode, _ token: String?) -> Client {
        return Client(replica, mode, token: token)
    }

    /// Somebody signed in (`Ark.Protocol.clientSignIn`): the token every
    /// later hello carries, and every intent authored before anyone had
    /// signed in made theirs (`Replica.signIn`). A peer used without an
    /// account has said nothing to any server — `connected` is only called
    /// once there is a token — so what it authored is pending, and the
    /// first hello after this pushes all of it.
    public mutating func signIn(_ who: Ctx, token: String?) {
        replica.signIn(who)
        self.token = token
    }

    mutating func emit(_ m: ClientMsg) {
        if linked { out.append(m) }
    }

    var hello: ClientMsg {
        return .hello(sub: Subscription(since: replica.cursor, mode: mode), token: token, spec: specVersion)
    }

    /// §12.1 A connection opened: say hello at the cursor, then push
    /// everything pending. What was queued before is dropped, since the
    /// hello resends it all.
    public mutating func connected() {
        linked = true
        epoch += 1
        out = []
        heardFrames = []
        emit(hello)
        if !replica.pending.isEmpty { emit(.push(entries: replica.pending)) }
    }

    public mutating func disconnected() {
        linked = false
        out = []
        heardFrames = []
    }

    /// Author an intent and push it if linked. A refusal here is the
    /// optimistic verdict, on the state this peer has; the authority's may
    /// differ, and arrives as a `reject` with its own reason.
    public mutating func mutate(_ i: Id, _ ctx: Ctx, _ fh: FnHash, _ autos: Args, _ args: Args) -> Result<Entry, Refusal> {
        switch replica.mutate(i, ctx, fh, autos, args) {
        case .failure(let why): return .failure(why)
        case .success(let e):
            emit(.push(entries: [e]))
            return .success(e)
        }
    }

    /// Work on the replica in place — what a peer that is its own authority
    /// needs for `localCommit`.
    public mutating func withReplica<T>(_ body: (inout Replica) throws -> T) rethrows -> T {
        return try body(&replica)
    }

    /// What a view is told, and the slate wiped (`Replica.takeChanges`).
    public mutating func takeChanges() -> Changes {
        return replica.takeChanges()
    }

    /// §12.2 A frame from the server.
    public mutating func recv(_ m: ServerMsg) {
        switch m {
        case .heard(let f):
            heardFrames.append(f)
        case .denied(let why):
            denied = why
            linked = false
            out = []
        case .batch(let items, let more):
            for it in items {
                if let f = it.facts { replica.receiveWith(it.seq, it.entry, f) } else { replica.receive(it.seq, it.entry) }
            }
            let needs = replica.needs
            if !needs.isEmpty { emit(.needFacts(seqs: needs)) }
            if more { emit(hello) }
        case .factsFor(let items):
            for it in items { replica.receiveFacts(it.seq, it.facts) }
        case .snapshotOf(let n, _, let rows):
            // Below the horizon: the confirmed store is replaced by the
            // snapshot and the cursor moves to it; pending replays on top.
            let st = MemoryStore(schema: replica.schema)
            for (t, vs) in rows {
                for v in vs { if case .record(let row) = v { st.applyChange(.add(t, row)) } }
            }
            replica = Replica.open(replica.schema, replica.bodies, st, n, replica.pending, natives: replica.natives)
        case .ack(let ids, let ns):
            for (i, n) in zip(ids, ns) { replica.ack(i, n) }
        case .reject(let i, let why):
            replica.reject(i, .refused(why))
        case .closures(let cs):
            // New closures may unblock entries waiting in the inbox.
            for c in cs { replica.bodies[c.hash] = c.closure }
            replica.retry()
        case .agree(let n, _, let ok):
            agreed.append((n, ok))
        }
    }

    /// A live frame; dropped while unlinked, never queued.
    public mutating func say(_ f: [UInt8]) {
        emit(.say(frame: f))
    }

    /// Ask the authority whether it agrees with the replica's confirmed state.
    public mutating func verifyAll() {
        let (n, h) = replica.verifyAt()
        emit(.verify(seq: n, hash: h))
    }

    public mutating func takeOutgoing() -> [ClientMsg] {
        let r = out
        out = []
        return r
    }

    public mutating func takeHeard() -> [[UInt8]] {
        let r = heardFrames
        heardFrames = []
        return r
    }
}
