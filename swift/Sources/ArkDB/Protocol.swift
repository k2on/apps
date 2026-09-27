import Foundation

/// How a client holds a scope.
public enum Mode: Equatable {
    case whole
    case byFacts
}

public struct Subscription: Equatable {
    public var scope: ScopeName
    /// The cursor: the last sequence applied.
    public var since: Seq
    public var mode: Mode
    public init(scope: ScopeName, since: Seq, mode: Mode) { self.scope = scope; self.since = since; self.mode = mode }
}

/// §12 The frames a client sends.
public enum ClientMsg {
    case hello(subs: [Subscription], token: String?, spec: Int)
    case push(scope: ScopeName, entries: [Entry])
    case needFacts(scope: ScopeName, seqs: [Seq])
    case needClosures(hashes: [FnHash])
    case verify(scope: ScopeName, seq: Seq, hash: [UInt8])
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
    case batch(scope: ScopeName, items: [BatchItem], hasMore: Bool)
    case factsFor(scope: ScopeName, items: [FactsItem])
    case snapshotOf(scope: ScopeName, seq: Seq, hash: [UInt8], rows: [TableName: [Value]])
    case ack(scope: ScopeName, ids: [Id], seqs: [Seq])
    case reject(scope: ScopeName, id: Id, reason: String)
    case denied(reason: String)
    case closures(items: [ClosureItem])
    case agree(scope: ScopeName, seq: Seq, hash: [UInt8], ok: Bool)
    case heard(frame: [UInt8])
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
        case .hello(let subs, let tok, let spec):
            return node("hello", [
                ("scopes", .list(subs.map { node("sub", [("scope", .text($0.scope)), ("since", int($0.since)), ("mode", .text($0.mode == .whole ? "whole" : "facts"))]) })),
                ("token", tok.map { .text($0) } ?? .null),
                ("spec", int(spec)),
            ])
        case .push(let s, let es): return node("push", [("scope", .text(s)), ("entries", .list(es.map(entryValue)))])
        case .needFacts(let s, let ns): return node("need_facts", [("scope", .text(s)), ("seqs", .list(ns.map(int)))])
        case .needClosures(let hs): return node("need_closures", [("hashes", .list(hs.map { .bytes($0) }))])
        case .verify(let s, let n, let h): return node("verify", [("scope", .text(s)), ("seq", int(n)), ("hash", .bytes(h))])
        case .say(let f): return node("say", [("say", .bytes(f))])
        }
    }

    public static func serverValue(_ m: ServerMsg) -> Value {
        switch m {
        case .batch(let s, let items, let more):
            return node("batch", [
                ("scope", .text(s)),
                ("items", .list(items.map { .record(["seq": int($0.seq), "entry": entryValue($0.entry), "facts": $0.facts.map(factsValue) ?? .null]) })),
                ("has_more", .bool(more)),
            ])
        case .factsFor(let s, let items):
            return node("facts", [("scope", .text(s)), ("items", .list(items.map { .record(["seq": int($0.seq), "facts": factsValue($0.facts)]) }))])
        case .snapshotOf(let s, let n, let h, let rows):
            return node("snapshot", [("scope", .text(s)), ("seq", int(n)), ("hash", .bytes(h)), ("rows", .record(rows.mapValues { .list($0) }))])
        case .ack(let s, let ids, let ns):
            return node("ack", [("scope", .text(s)), ("ids", .list(ids.map { .id($0) })), ("seqs", .list(ns.map(int)))])
        case .reject(let s, let i, let why):
            return node("reject", [("scope", .text(s)), ("id", .id(i)), ("reason", .text(why))])
        case .denied(let why): return node("denied", [("reason", .text(why))])
        case .closures(let cs):
            return node("closures", [("items", .list(cs.map { .record(["hash": .bytes($0.hash), "closure": Hash.closureValue($0.closure)]) }))])
        case .agree(let s, let n, let h, let ok):
            return node("agree", [("scope", .text(s)), ("seq", int(n)), ("hash", .bytes(h)), ("ok", .bool(ok))])
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
            let subs = try list(try need(m, "scopes")) { x -> Subscription in
                let sm = try structOf(x)
                let md: Mode
                switch try text(try need(sm, "mode")) {
                case "whole": md = .whole
                case "facts": md = .byFacts
                case let other: throw bad("unknown mode " + other)
                }
                return Subscription(scope: try text(try need(sm, "scope")), since: try int64(try need(sm, "since")), mode: md)
            }
            let tokV = try need(m, "token")
            let tok: String? = tokV.isNull() ? nil : try text(tokV)
            return .hello(subs: subs, token: tok, spec: Int(try int64(try need(m, "spec"))))
        case "push": return .push(scope: try text(try need(m, "scope")), entries: try list(try need(m, "entries"), entryFromValue))
        case "need_facts": return .needFacts(scope: try text(try need(m, "scope")), seqs: try list(try need(m, "seqs"), int64))
        case "need_closures": return .needClosures(hashes: try list(try need(m, "hashes"), bytes))
        case "verify": return .verify(scope: try text(try need(m, "scope")), seq: try int64(try need(m, "seq")), hash: try bytes(try need(m, "hash")))
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
            return .batch(scope: try text(try need(m, "scope")), items: items, hasMore: try bool(try need(m, "has_more")))
        case "facts":
            let items = try list(try need(m, "items")) { x -> FactsItem in
                let im = try structOf(x)
                return FactsItem(seq: try int64(try need(im, "seq")), facts: try list(try need(im, "facts"), changeFromValue))
            }
            return .factsFor(scope: try text(try need(m, "scope")), items: items)
        case "snapshot":
            let rm = try structOf(try need(m, "rows"))
            var rows: [TableName: [Value]] = [:]
            for (k, x) in rm { rows[k] = try list(x) { $0 } }
            return .snapshotOf(scope: try text(try need(m, "scope")), seq: try int64(try need(m, "seq")), hash: try bytes(try need(m, "hash")), rows: rows)
        case "ack":
            return .ack(scope: try text(try need(m, "scope")), ids: try list(try need(m, "ids"), ident), seqs: try list(try need(m, "seqs"), int64))
        case "reject":
            return .reject(scope: try text(try need(m, "scope")), id: try ident(try need(m, "id")), reason: try text(try need(m, "reason")))
        case "denied": return .denied(reason: try text(try need(m, "reason")))
        case "closures":
            let items = try list(try need(m, "items")) { x -> ClosureItem in
                let im = try structOf(x)
                return ClosureItem(hash: try bytes(try need(im, "hash")), closure: try Decode.closureFromValue(try need(im, "closure")))
            }
            return .closures(items: items)
        case "agree":
            return .agree(scope: try text(try need(m, "scope")), seq: try int64(try need(m, "seq")), hash: try bytes(try need(m, "hash")), ok: try bool(try need(m, "ok")))
        case "heard": return .heard(frame: try bytes(try need(m, "hear")))
        default: throw bad("unknown server frame " + t)
        }
    }
}

// MARK: - The client

/// A scope a client holds: the replica and how it is fed.
public struct Held {
    public var replica: Replica
    public var mode: Mode
}

/// A peer's end of one connection: its replicas, and what it has queued.
/// Sans-io: a transport feeds it frames and drains what it queues.
public struct Client {
    public var schema: Schema
    public private(set) var scopes: [ScopeName: Held]
    public var token: String?
    public private(set) var linked: Bool
    /// Counts connections, so a live room that has never heard of this
    /// device can be told apart from one that has.
    public private(set) var epoch: Int
    var out: [ClientMsg] // oldest first
    var heardFrames: [[UInt8]] // oldest first
    public private(set) var denied: String?
    public private(set) var agreed: [(ScopeName, Seq, Bool)]

    public init(schema: Schema, token: String?) {
        self.schema = schema
        self.scopes = [:]
        self.token = token
        self.linked = false
        self.epoch = 0
        self.out = []
        self.heardFrames = []
        self.denied = nil
        self.agreed = []
    }

    public static func openClient(_ schema: Schema, _ token: String?) -> Client {
        return Client(schema: schema, token: token)
    }

    /// The scopes in name order, which is the order the spec walks them.
    var scopeNames: [ScopeName] { return scopes.keys.sorted { compareText($0, $1) < 0 } }

    /// Hold a scope, with the replica as opened from what was durable.
    public mutating func subscribe(_ mode: Mode, _ r: Replica) {
        scopes[r.scope] = Held(replica: r, mode: mode)
    }

    mutating func emit(_ m: ClientMsg) {
        if linked { out.append(m) }
    }

    /// §12.1 A connection opened: hello for every scope at its cursor, then
    /// push everything pending. What was queued before is dropped.
    public mutating func connected() {
        linked = true
        epoch += 1
        out = []
        heardFrames = []
        let subs = scopeNames.map { Subscription(scope: $0, since: scopes[$0]!.replica.cursor, mode: scopes[$0]!.mode) }
        emit(.hello(subs: subs, token: token, spec: 1))
        for s in scopeNames where !scopes[s]!.replica.pending.isEmpty {
            emit(.push(scope: s, entries: scopes[s]!.replica.pending))
        }
    }

    public mutating func disconnected() {
        linked = false
        out = []
        heardFrames = []
    }

    /// Author an intent into a scope and push it if linked.
    public mutating func mutate(_ s: ScopeName, _ i: Id, _ ctx: Ctx, _ fh: FnHash, _ autos: Args, _ args: Args) -> Result<Entry, Refusal> {
        guard var held = scopes[s] else { return .failure(.refused("not holding scope " + s)) }
        switch held.replica.mutate(i, ctx, fh, autos, args) {
        case .failure(let why): return .failure(why)
        case .success(let e):
            scopes[s] = held
            emit(.push(scope: s, entries: [e]))
            return .success(e)
        }
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
        case .batch(let s, let items, let more):
            guard var held = scopes[s] else { return }
            for it in items {
                if let f = it.facts { held.replica.receiveWith(it.seq, it.entry, f) } else { held.replica.receive(it.seq, it.entry) }
            }
            scopes[s] = held
            let needs = held.replica.needs
            if !needs.isEmpty { emit(.needFacts(scope: s, seqs: needs)) }
            if more { emit(.hello(subs: [Subscription(scope: s, since: held.replica.cursor, mode: held.mode)], token: token, spec: 1)) }
        case .factsFor(let s, let items):
            guard var held = scopes[s] else { return }
            for it in items { held.replica.receiveFacts(it.seq, it.facts) }
            scopes[s] = held
        case .snapshotOf(let s, let n, _, let rows):
            // Below the horizon: the confirmed store is replaced by the
            // snapshot and the cursor moves to it; pending replays on top.
            guard let held = scopes[s] else { return }
            let st = MemoryStore(schema: schema)
            for (t, vs) in rows {
                for v in vs { if case .record(let row) = v { st.applyChange(.add(t, row)) } }
            }
            let r = Replica.open(held.replica.schema, s, held.replica.bodies, st, n, held.replica.pending)
            scopes[s] = Held(replica: r, mode: held.mode)
        case .ack(let s, let ids, let ns):
            guard var held = scopes[s] else { return }
            for (i, n) in zip(ids, ns) { held.replica.ack(i, n) }
            scopes[s] = held
        case .reject(let s, let i, let why):
            guard var held = scopes[s] else { return }
            held.replica.reject(i, .refused(why))
            scopes[s] = held
        case .closures(let cs):
            // New closures may unblock entries waiting in an inbox.
            for s in scopeNames {
                var held = scopes[s]!
                for c in cs { held.replica.bodies[c.hash] = c.closure }
                held.replica.retry()
                scopes[s] = held
            }
        case .agree(let s, let n, _, let ok):
            agreed.append((s, n, ok))
        }
    }

    /// A live frame; dropped while unlinked, never queued.
    public mutating func say(_ f: [UInt8]) {
        emit(.say(frame: f))
    }

    /// Ask the authority whether it agrees with every replica's confirmed state.
    public mutating func verifyAll() {
        for s in scopeNames {
            let (n, h) = scopes[s]!.replica.verifyAt()
            emit(.verify(scope: s, seq: n, hash: h))
        }
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
