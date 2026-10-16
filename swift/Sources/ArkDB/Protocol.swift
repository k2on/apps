// §12 The protocol, as `rust/ark/src/protocol.rs` defines it: the frames two
// peers exchange, as values (so that `Canon` is their wire form), and the
// client's state machine around them — a `Client` holding a replica of the
// log and what it has queued. Sans-io: frames go in as values or bytes and
// come out as values, and the driver owns the socket.
//
// A client writes every client frame and reads every server frame, so both
// directions are here, encoded and decoded exactly as the reference does,
// each optional field written only where the reference writes it — a frame
// a runtime writes is the bytes Rust writes for the same frame. What a
// server does with them (`Server`) is not a client's, and is not here.

/// How a client holds the log (`protocol::Mode`): `whole` replays; `byFacts` is fed facts.
public enum Mode: String {
    case whole
    case byFacts = "facts"
}

/// A `hello`'s subscription (`protocol::Subscription`).
public struct Subscription {
    /// The cursor: the last sequence applied.
    public let since: Seq
    public let mode: Mode
    /// Which log `since` is of, as the peer last heard it named; `log` on the wire, or absent.
    public let logId: Id?
    /// D2 The peer holds a union: `partial: true` on the wire, absent otherwise.
    public let partial: Bool
}

/// What a partial page covers (`protocol::Covers`): the sequences after `after` through `upto`.
public struct Covers {
    public let after: Seq
    public let upto: Seq
}

/// A frame a client writes (`protocol::ClientMsg`).
public enum ClientMsg {
    /// The last: §14 the device this connection is (`docs/plan-live.md`),
    /// present only when declared, so a client that never uses live state
    /// says the bytes it said.
    case hello(Subscription, String?, Int64, DeviceDecl?)
    case push([LogEntry])
    case needFacts([Seq])
    case needClosures([[UInt8]])
    /// Whether the authority holds the state `hash` at `seq`, of the log `logId`.
    case verify(Seq, [UInt8], Id?, Bool)
    case say([UInt8])
    /// §14 Run a transition of a live instance (`call`).
    case call(LiveCall)
    /// §14 Send this instance whole (`resync`): a gap, or a hash that did not match.
    case resync(Instance)

    /// The frame as a value; its wire form is `Canon.encode` of this.
    public func toValue() -> Value {
        switch self {
        case .hello(let sub, let token, let spec, let device):
            var f: [(String, Value)] = [("since", .int(sub.since)), ("mode", .text(sub.mode.rawValue)), ("token", token.map { .text($0) } ?? .null), ("spec", .int(spec))]
            named(&f, sub.logId)
            flagged(&f, "partial", sub.partial)
            if let d = device { f.append(("device", d.toValue())) }
            return node("hello", f)
        case .push(let es): return node("push", [("entries", .list(es.map(entryValue)))])
        case .needFacts(let ns): return node("need_facts", [("seqs", .list(ns.map { .int($0) }))])
        case .needClosures(let hs): return node("need_closures", [("hashes", .list(hs.map { .bytes($0) }))])
        case .verify(let seq, let hash, let logId, let partial):
            var f: [(String, Value)] = [("seq", .int(seq)), ("hash", .bytes(hash))]
            named(&f, logId)
            flagged(&f, "partial", partial)
            return node("verify", f)
        case .say(let b): return node("say", [("say", .bytes(b))])
        case .call(let c): return c.toValue()
        case .resync(let i): return resyncValue(i)
        }
    }

    /// Its canonical bytes.
    public func encode() -> [UInt8] { Canon.encode(toValue()) }

    /// `ClientMsg::from_value`.
    public static func fromValue(_ v: Value) throws -> ClientMsg {
        let m = try strctOf(v)
        let t = try text(try need(m, "t"))
        switch t {
        case "hello":
            let tok = try need(m, "token")
            return .hello(try sub(m), tok.isNull ? nil : try text(tok), try int64(try need(m, "spec")), try m["device"].map(DeviceDecl.fromValue))
        case "push": return .push(try list(try need(m, "entries"), entryFromValue))
        case "need_facts": return .needFacts(try list(try need(m, "seqs"), int64))
        case "need_closures": return .needClosures(try list(try need(m, "hashes"), bytes))
        case "verify": return .verify(try int64(try need(m, "seq")), try bytes(try need(m, "hash")), try logOf(m), try flag(m, "partial"))
        case "say": return .say(try bytes(try need(m, "say")))
        case "call": return .call(try LiveCall.fromFields(m))
        case "resync": return .resync(try resyncInstance(m))
        default: throw bad("unknown client frame \(t)")
        }
    }

    private static func sub(_ m: Record) throws -> Subscription {
        let w = try text(try need(m, "mode"))
        guard let mode = Mode(rawValue: w) else { throw bad("unknown mode \(w)") }
        return Subscription(since: try int64(try need(m, "since")), mode: mode, logId: try logOf(m), partial: try flag(m, "partial"))
    }
}

/// A frame a client reads (`protocol::ServerMsg`).
public enum ServerMsg {
    /// A page: entries with their facts where the peer is fed facts; the log
    /// they are of, the server's module hash, and — for a peer that is not
    /// whole — the sequences it covers.
    case batch([(Seq, LogEntry, FactList?)], Bool, Id?, [UInt8]?, Covers?)
    case factsFor([(Seq, FactList)])
    /// The state at `seq` as rows by table; `held`, a partial snapshot's tables held in part.
    case snapshotOf(Seq, [UInt8], [(String, [Value])], Id?, [UInt8]?, [(String, [String])]?)
    /// Acknowledgements; the facts of entries stamped with other roles (D1).
    case ack([Id], [Seq], Id?, [(Seq, FactList)])
    case reject(Id, String)
    /// An intent the server cannot run yet (`docs/plan-db.md` D1): kept pending.
    case held(Id, String)
    case denied(String)
    case closures([([UInt8], Closure)])
    /// The answer to a `verify`; `unknown`, the authority could not say.
    case agree(Seq, [UInt8], Bool, Bool)
    case heard([UInt8])
    /// §14 A call a live instance took (`applied`), ordered or ephemeral.
    case applied(LiveApplied)
    /// §14 A call it did not, to its author (`refused`).
    case refused(LiveRefused)
    /// §14 An instance whole (`live_snapshot`).
    case liveSnapshot(LiveSnapshot)

    /// The frame as a value.
    public func toValue() -> Value {
        switch self {
        case .batch(let items, let hasMore, let logId, let module, let covers):
            var f: [(String, Value)] = [
                ("items", .list(items.map { n, e, fs in .record([("seq", .int(n)), ("entry", entryValue(e)), ("facts", fs.map(factsValue) ?? .null)]) })),
                ("has_more", .bool(hasMore)),
            ]
            named(&f, logId)
            ofModule(&f, module)
            if let c = covers {
                f.append(("after", .int(c.after)))
                f.append(("upto", .int(c.upto)))
            }
            return node("batch", f)
        case .factsFor(let items): return node("facts", [("items", seqFactsValue(items))])
        case .snapshotOf(let seq, let hash, let rows, let logId, let module, let held):
            var f: [(String, Value)] = [("seq", .int(seq)), ("hash", .bytes(hash)), ("rows", .record(Record(rows.map { ($0.0, Value.list($0.1)) })))]
            named(&f, logId)
            ofModule(&f, module)
            if let h = held {
                f.append(("partial", .bool(true)))
                f.append(("held", heldValue(h)))
            }
            return node("snapshot", f)
        case .ack(let ids, let seqs, let logId, let facts):
            var f: [(String, Value)] = [("ids", .list(ids.map { .id($0) })), ("seqs", .list(seqs.map { .int($0) }))]
            named(&f, logId)
            if !facts.isEmpty { f.append(("facts", seqFactsValue(facts))) }
            return node("ack", f)
        case .reject(let id, let reason): return node("reject", [("id", .id(id)), ("reason", .text(reason))])
        case .held(let id, let reason): return node("held", [("id", .id(id)), ("reason", .text(reason))])
        case .denied(let reason): return node("denied", [("reason", .text(reason))])
        case .closures(let items):
            return node("closures", [("items", .list(items.map { h, c in .record([("hash", .bytes(h)), ("closure", Encode.closure(c))]) }))])
        case .agree(let seq, let hash, let ok, let unknown):
            var f: [(String, Value)] = [("seq", .int(seq)), ("hash", .bytes(hash)), ("ok", .bool(ok))]
            if unknown { f.append(("unknown", .bool(true))) }
            return node("agree", f)
        case .heard(let b): return node("heard", [("hear", .bytes(b))])
        case .applied(let a): return a.toValue()
        case .refused(let r): return r.toValue()
        case .liveSnapshot(let s): return s.toValue()
        }
    }

    /// `ServerMsg::from_value`.
    public static func fromValue(_ v: Value) throws -> ServerMsg {
        let m = try strctOf(v)
        let t = try text(try need(m, "t"))
        switch t {
        case "batch":
            let items = try list(try need(m, "items")) { x -> (Seq, LogEntry, FactList?) in
                let im = try strctOf(x)
                let f = try need(im, "facts")
                let facts = f.isNull ? nil : try list(f, changeFromValue)
                return (try int64(try need(im, "seq")), try entryFromValue(try need(im, "entry")), facts)
            }
            return .batch(items, try boolean(try need(m, "has_more")), try logOf(m), try moduleOf(m), try coversOf(m))
        case "facts": return .factsFor(try seqFactsOf(try need(m, "items")))
        case "snapshot":
            let rows = try strctOf(try need(m, "rows")).pairs.map { ($0.0, try list($0.1) { $0 }) }
            return .snapshotOf(try int64(try need(m, "seq")), try bytes(try need(m, "hash")), rows, try logOf(m), try moduleOf(m), try heldOf(m))
        case "ack":
            var facts = [(Seq, FactList)]()
            if let f = m["facts"] {
                facts = try seqFactsOf(f)
                if facts.isEmpty { throw bad("ack: empty facts are written as no field") }
            }
            return .ack(try list(try need(m, "ids"), ident), try list(try need(m, "seqs"), int64), try logOf(m), facts)
        case "reject": return .reject(try ident(try need(m, "id")), try text(try need(m, "reason")))
        case "held": return .held(try ident(try need(m, "id")), try text(try need(m, "reason")))
        case "denied": return .denied(try text(try need(m, "reason")))
        case "closures":
            return .closures(try list(try need(m, "items")) { x in
                let im = try strctOf(x)
                return (try bytes(try need(im, "hash")), try Decode.closure(try need(im, "closure")))
            })
        case "agree":
            var unknown = false
            if let u = m["unknown"] {
                if u == .bool(true) { unknown = true } else { throw bad("an agree's unknown is true or absent") }
            }
            return .agree(try int64(try need(m, "seq")), try bytes(try need(m, "hash")), try boolean(try need(m, "ok")), unknown)
        case "heard": return .heard(try bytes(try need(m, "hear")))
        case "applied": return .applied(try LiveApplied.fromFields(m))
        case "refused": return .refused(try LiveRefused.fromFields(m))
        case "live_snapshot": return .liveSnapshot(try LiveSnapshot.fromFields(m))
        default: throw bad("unknown server frame \(t)")
        }
    }

    /// `ServerMsg::decode_for`: a frame from its bytes, for a client of
    /// `schema` — the one way a client reads the socket: a snapshot with its
    /// rows built (`Snapshot.ofValues`), any other frame as `fromValue` reads it.
    public static func decodeFor(_ bytes: [UInt8], _ schema: Schema) throws -> Received {
        let msg = try fromValue(try Canon.decode(bytes))
        if case .snapshotOf(let seq, _, let rows, let logId, let module, let held) = msg {
            return .snap(Snapshot.ofValues(schema, seq, rows, logId, module, held))
        }
        return .msg(msg)
    }
}

/// D7.4 What a `snapshot` frame brings a client (`protocol::Snapshot`): the
/// state at `seq` as its rows — each a struct, what is not one dropped, a
/// table left with none not named — the log, the server's module, and the
/// tables a partial one holds in part.
public struct Snapshot {
    public let seq: Seq
    public let rows: [(String, [Row])]
    public let logId: Id?
    public let module: [UInt8]?
    public let held: [(String, [String])]?

    /// The same snapshot, as a value — for comparing two of them.
    public func toValue() -> Value {
        var f: [(String, Value)] = [("seq", .int(seq)), ("rows", .record(Record(rows.map { ($0.0, Value.list($0.1.map { .record($0) })) })))]
        named(&f, logId)
        ofModule(&f, module)
        if let h = held { f.append(("held", heldValue(h))) }
        return .record(f)
    }

    /// `Snapshot::of_values`.
    public static func ofValues(_ schema: Schema, _ seq: Seq, _ rows: [(String, [Value])], _ logId: Id?, _ module: [UInt8]?, _ held: [(String, [String])]?) -> Snapshot {
        var out = [(String, [Row])]()
        for (t, vs) in rows.sorted(by: { compareText($0.0, $1.0) < 0 }) {
            let rs = vs.compactMap { $0.recordValue }
            if !rs.isEmpty { out.append((t, rs)) }
        }
        return Snapshot(seq: seq, rows: out, logId: logId, module: module, held: held)
    }
}

/// A server frame as a client of a known schema reads it (`protocol::Received`).
public enum Received {
    case msg(ServerMsg)
    case snap(Snapshot)
}

// The wire's spelling ---------------------------------------------------------

private func node(_ t: String, _ fields: [(String, Value)]) -> Value { .record([("t", .text(t))] + fields) }

// `named`: a log's identity on the wire — `log`, an id, absent where none is known.
private func named(_ f: inout [(String, Value)], _ id: Id?) {
    if let i = id { f.append(("log", .id(i))) }
}

// `of_module`: the server's module hash — `module`, bytes, absent where not said.
private func ofModule(_ f: inout [(String, Value)], _ module: [UInt8]?) {
    if let m = module { f.append(("module", .bytes(m))) }
}

// `flagged`: a flag that is `true` when present; absent is the one encoding of `false`.
private func flagged(_ f: inout [(String, Value)], _ name: String, _ on: Bool) {
    if on { f.append((name, .bool(true))) }
}

private func heldValue(_ h: [(String, [String])]) -> Value { .record(Record(h.map { ($0.0, Value.list($0.1.map { .text($0) })) })) }

/// `entry_value`: an entry as a value — its wire form in a `push` or a
/// `batch`, and its form on disk. `roles` is present only when the entry
/// holds one (D1).
public func entryValue(_ e: LogEntry) -> Value {
    var f: [(String, Value)] = [("id", .id(e.id)), ("actor", .text(e.actor)), ("session", .text(e.session)), ("fn", .bytes(e.fnHash)), ("args", .record(argsRecord(e.args))), ("autos", .record(argsRecord(e.autos)))]
    if !e.roles.isEmpty { f.append(("roles", .list(e.roles.map { .text($0) }))) }
    return .record(f)
}

/// `entry_from_value`.
public func entryFromValue(_ v: Value) throws -> LogEntry {
    let m = try strctOf(v)
    return LogEntry(try ident(try need(m, "id")), try text(try need(m, "actor")), try text(try need(m, "session")), try rolesOf(m), try bytes(try need(m, "fn")), try argsOf(try strctOf(try need(m, "args"))), try argsOf(try strctOf(try need(m, "autos"))))
}

private func argsOf(_ r: Record) -> [String: Value] {
    var out = [String: Value]()
    for (k, v) in r.pairs { out[k] = v }
    return out
}

// `roles_of`: none where absent (an empty list refused), otherwise texts in
// strictly ascending order.
private func rolesOf(_ m: Record) throws -> [String] {
    guard let v = m["roles"] else { return [] }
    let rs = try list(v, text)
    if rs.isEmpty { throw bad("roles: an empty list is written as no field") }
    for i in rs.indices.dropFirst() where compareText(rs[i - 1], rs[i]) >= 0 { throw bad("roles: not in ascending order, or repeated") }
    return rs
}

/// `change_value`.
public func changeValue(_ c: Change) -> Value { Facts.changeValue(c) }

/// `change_from_value`, refusing what is not a change with a frame's error.
public func changeFromValue(_ v: Value) throws -> Change {
    let m = try strctOf(v)
    let t = try text(try need(m, "t"))
    let tbl = try text(try need(m, "table"))
    func row(_ k: String) throws -> Row {
        guard case .record(let r) = try need(m, k) else { throw bad("expected a struct") }
        return r
    }
    switch t {
    case "add": return .add(tbl, try row("row"))
    case "remove": return .remove(tbl, try row("row"))
    case "edit": return .edit(tbl, try row("old"), try row("new"))
    default: throw bad("unknown change \(t)")
    }
}

/// A list of facts as a value: a list of changes.
public func factsValue(_ f: FactList) -> Value { .list(f.map(changeValue)) }

private func seqFactsValue(_ items: [(Seq, FactList)]) -> Value { .list(items.map { .record([("seq", .int($0.0)), ("facts", factsValue($0.1))]) }) }

private func seqFactsOf(_ v: Value) throws -> [(Seq, FactList)] {
    try list(v) { x in
        let m = try strctOf(x)
        return (try int64(try need(m, "seq")), try list(try need(m, "facts"), changeFromValue))
    }
}

private func flag(_ m: Record, _ name: String) throws -> Bool {
    guard let v = m[name] else { return false }
    if v == .bool(true) { return true }
    throw bad("\(name) is true or absent")
}

private func coversOf(_ m: Record) throws -> Covers? {
    switch (m["after"], m["upto"]) {
    case (nil, nil): return nil
    case (let a?, let u?): return Covers(after: try int64(a), upto: try int64(u))
    default: throw bad("a page's after and upto go together")
    }
}

private func heldOf(_ m: Record) throws -> [(String, [String])]? {
    let partial = try flag(m, "partial")
    let h = m["held"]
    if !partial && h == nil { return nil }
    guard partial, let hv = h else { throw bad("a snapshot's partial and held go together") }
    return try strctOf(hv).pairs.map { ($0.0, try list($0.1, text)) }
}

private func logOf(_ m: Record) throws -> Id? { try m["log"].map(ident) }

private func moduleOf(_ m: Record) throws -> [UInt8]? { try m["module"].map(bytes) }

// Decoding primitives, each refusing with the frame's error (`protocol::DecodeError`).

private func bad(_ what: String) -> FactError { FactError("frame: \(what)") }

private func strctOf(_ v: Value) throws -> Record {
    guard case .record(let r) = v else { throw bad("expected a struct") }
    return r
}

private func need(_ m: Record, _ k: String) throws -> Value {
    guard let v = m[k] else { throw bad("missing \(k)") }
    return v
}

private func text(_ v: Value) throws -> String {
    guard case .text(let s) = v else { throw bad("expected text") }
    return s
}

private func bytes(_ v: Value) throws -> [UInt8] {
    guard case .bytes(let b) = v else { throw bad("expected bytes") }
    return b
}

private func int64(_ v: Value) throws -> Int64 {
    guard case .int(let n) = v else { throw bad("expected an int") }
    return n
}

private func boolean(_ v: Value) throws -> Bool {
    guard case .bool(let b) = v else { throw bad("expected a bool") }
    return b
}

private func ident(_ v: Value) throws -> Id {
    guard case .id(let i) = v else { throw bad("expected an id") }
    return i
}

private func list<T>(_ v: Value, _ f: (Value) throws -> T) throws -> [T] {
    guard case .list(let xs) = v else { throw bad("expected a list") }
    return try xs.map(f)
}

/// What an intent naming a function no closure is held for is answered with
/// (`protocol::unknown_function`): a `held` from a server that holds, and
/// before that a `reject` with this reason, which a `Client` reads as the
/// hold it was.
public func unknownFunction(_ fh: [UInt8]) -> String { "unknown function \(Hex.encode(fh))" }

/// §12.5 `refusal_text`: the reason a `reject` carries — a mutator's own
/// refusal word for word, a constraint named in a sentence.
public func refusalText(_ r: Refusal) -> String {
    switch r {
    case .refused(let w): return w
    case .noSuchTable(let t): return "no table \(t)"
    case .malformedRow(let t, let w): return "\(t): \(w)"
    case .notNull(let t, let c): return "\(t).\(c) may not be empty"
    case .uniqueViolation(let t, let cs): return "\(t): another row has the same \(cs.joined(separator: ", "))"
    case .missingParent(let t, let c, let p): return "\(t).\(c) names no \(p)"
    case .stillReferenced(let t, let c): return "\(t): still referenced by \(c)"
    case .forbidden(let t): return "\(t): not this login's to write"
    }
}

// ---------------------------------------------------------------------------
// The client

/// A peer's end of one connection (`protocol::Client`): its replica of the
/// log, and what it has queued. The fields are the Rust's, each documented there.
public final class Client {
    public var replica: Replica
    public var mode: Mode
    /// The token every `hello` carries.
    public var token: String?
    /// The module's schema: what every later snapshot's device schema is made from (D2).
    public var schema: Schema
    public var linked = false
    /// Counts connections.
    public var epoch: Int64 = 0
    /// Oldest first.
    public var out = [ClientMsg]()
    public var denied: String? = nil
    /// Every answer to a `verify`, oldest first: true, false, or nil where it could not say.
    public var agreed = [(Seq, Bool?)]()
    /// A page arrived since the last `settle`.
    public var paged = false
    /// A page said there is more: the `hello` asking for it is said at the settle.
    public var more = false
    /// This peer's own module hash, where the driver says it.
    public var module: [UInt8]? = nil
    /// The server's module hash, as its last page or snapshot said it (D1).
    public var serverModule: [UInt8]? = nil
    /// Pending intents the server answered `held` (D1).
    public var heldIds = Set<Id>()
    /// D2 This connection has been sent a page or a snapshot.
    public var served = false
    /// This connection has brought a snapshot.
    public var started = false
    /// §14 Live state (`docs/plan-live.md`): the instances this client holds
    /// and the device it declares. Nil — every client of a module without
    /// live state — and nothing here says a byte it did not say before.
    public var live: Watch? = nil

    public init(_ replica: Replica, _ mode: Mode, _ token: String?) {
        self.replica = replica
        self.mode = mode
        self.token = token
        self.schema = replica.schema
    }

    /// `Client::open`: over the replica as opened from what was durable.
    public static func open(_ replica: Replica, _ mode: Mode, _ token: String?) -> Client { Client(replica, mode, token) }

    /// `with_live`: hold live state, as the device `watch` declares (or as none).
    public func withLive(_ watch: Watch) -> Client {
        live = watch
        return self
    }

    /// `with_schema`: the module's schema, for a client opened over a union.
    public func withSchema(_ module: Schema) -> Client {
        schema = module
        return self
    }

    /// §12 `behind`: both module hashes are known and they differ.
    public func behind() -> Bool {
        guard let ours = module, let theirs = serverModule else { return false }
        return ours != theirs
    }

    /// Pending intents the server has answered `held`, and nothing since.
    public func held() -> Int { replica.pending.filter { heldIds.contains($0.id) }.count }

    private func heardModule(_ m: [UInt8]?) {
        if let m = m { serverModule = m }
        replica.behind = behind()
    }

    private func holdIntent(_ id: Id) {
        if replica.pending.contains(where: { $0.id == id }) { heldIds.insert(id) }
    }

    // Unlinked: nothing is queued; `connected` says it all again.
    private func emit(_ m: ClientMsg) {
        if linked { out.append(m) }
    }

    /// `sign_in`: the token every later `hello` carries, and nobody's intents made `who`'s.
    public func signIn(_ who: Ctx, _ token: String?) {
        replica.signIn(who)
        self.token = token
    }

    private func hello() -> ClientMsg {
        .hello(Subscription(since: replica.cursor, mode: mode, logId: replica.logId, partial: replica.partial != nil), token, SPEC_VERSION, live?.device)
    }

    /// §12.1 `connected`: hello at the cursor, then everything pending pushed.
    public func connected() {
        linked = true
        epoch += 1
        out.removeAll()
        more = false
        served = false
        started = false
        emit(hello())
        if !replica.pending.isEmpty { emit(.push(replica.pending)) }
        // §14 Every live call still pending, said again in order: the server
        // answers one it already took with the `applied` it was.
        if live != nil {
            for c in live!.connected() { emit(.call(c)) }
        }
    }

    public func disconnected() {
        linked = false
        out.removeAll()
        more = false
    }

    /// §14 `call`: this device calls a transition of a live instance, run over
    /// its view at once with `now` its guess at the server's clock, and sent
    /// if linked — an ordered call kept pending and said again on the next
    /// connection until the server answers; an ephemeral one applied and
    /// done, `counter` its place among this device's, and dropped while
    /// unlinked. A refusal here is the preview's, thrown as a `LiveRefusal`,
    /// and nothing is sent.
    public func call(_ who: Ctx, _ i: Instance, _ transition: String, _ input: [String: Value], _ id: Id, _ now: Int64, _ counter: Int64) throws -> Kind {
        guard live != nil else { throw LiveRefusal(refusal: .refused("this client holds no live state")) }
        let called = try live!.call(who, i, transition, input, id, now, counter)
        emit(.call(called.call))
        return called.kind
    }

    // What the server said of live state, applied; a snapshot asked for where
    // the replica fell out of step.
    private func heardLive(_ said: Said) {
        guard live != nil else { return }
        for i in live!.heard(said) { emit(.resync(i)) }
    }

    /// `mutate`: author an intent and push it if linked. A refusal here is
    /// the optimistic verdict, on the state this peer has.
    public func mutate(_ id: Id, _ ctx: Ctx, _ fh: [UInt8], _ autos: [String: Value], _ args: [String: Value]) -> (LogEntry?, Refusal?) {
        let (e, why) = replica.mutate(id, ctx, fh, autos, args)
        if let e = e { emit(.push([e])) }
        return (e, why)
    }

    /// §12.2 `recv`: a frame from the server, placed in the replica's inbox
    /// and applied by `settle` (`docs/plan-perf.md` R8).
    public func recv(_ msg: ServerMsg) {
        switch msg {
        // §14 The opaque rooms' frame: nothing in this runtime listens to one.
        case .heard: break
        // §14 Not the log: nothing here waits for a settle.
        case .applied(let a): heardLive(.applied(a))
        case .refused(let r): heardLive(.refused(r))
        case .liveSnapshot(let s): heardLive(.snapshot(s))
        case .denied(let reason):
            denied = reason
            linked = false
            out.removeAll()
            more = false
        case .batch(let items, let hasMore, let logId, let module, let covers):
            heardModule(module)
            served = true
            let r = replica
            if (covers != nil) != (r.partial != nil) {
                more = true
                return
            }
            if covers != nil && !started {
                more = true
                return
            }
            if let c = covers {
                if c.upto <= r.through { return }
                if c.after > r.through {
                    more = true
                    return
                }
                r.through = c.upto
            }
            if r.logId == nil { r.logId = logId }
            for (n, e, f) in items {
                if let f = f { r.receiveWith(n, e, f) } else { r.receive(n, e) }
            }
            paged = true
            more = more || hasMore
        case .factsFor(let items): for (n, f) in items { replica.receiveFacts(n, f) }
        case .snapshotOf(let seq, _, let rows, let logId, let module, let held):
            recvSnapshot(Snapshot.ofValues(schema, seq, rows, logId, module, held))
        case .ack(let ids, let seqs, let logId, let facts):
            if replica.logId == nil { replica.logId = logId }
            for (n, f) in facts { replica.receiveFacts(n, f) }
            for (i, n) in zip(ids, seqs) {
                heldIds.remove(i)
                replica.ack(i, n)
            }
        case .reject(let id, let reason):
            let unknown = replica.pending.contains { $0.id == id && reason == unknownFunction($0.fnHash) }
            if unknown {
                holdIntent(id)
            } else {
                heldIds.remove(id)
                replica.reject(id, .refused(reason))
            }
        case .held(let id, _): holdIntent(id)
        case .closures(let items): for (h, c) in items { replica.bodies[hashKey(h)] = c }
        case .agree(let seq, _, let ok, let unknown): agreed.append((seq, unknown ? nil : ok))
        }
    }

    /// §12.2 `recv_snapshot`: below the horizon, past the head, or of another
    /// log — the confirmed store replaced by the snapshot, pending intents
    /// kept and replayed on top, verdicts not yet taken kept ahead of any the
    /// replay makes; what earlier frames of the pump placed applied first.
    public func recvSnapshot(_ s: Snapshot) {
        heardModule(s.module)
        served = true
        started = true
        replica.settle()
        let behind = replica.behind
        var heldMap: [String: [String]]? = nil
        if let h = s.held {
            var m = [String: [String]]()
            for (t, cs) in h { m[t] = cs }
            heldMap = m
        }
        let sch = heldMap != nil ? deviceSchema(schema, heldMap!) : schema
        let st = MemoryStore.empty(sch)
        for (t, rs) in s.rows {
            let tbl = sch.lookupTable(t)
            for row0 in rs {
                var row = row0
                if let tb = tbl, behind, case .success(let p) = Rules.projectRow(tb, row0) { row = p }
                st.applyChange(.add(t, row))
            }
        }
        let r = replica
        let opened = Replica.open(sch, r.bodies, st, s.seq, r.pending)
        opened.logId = s.logId
        opened.behind = r.behind
        if let h = s.held {
            var p = SortedMap<CodePoints, [String]>()
            for (t, cs) in h { p[CodePoints(t)] = cs }
            opened.partial = p
        }
        opened.through = s.seq
        let told = r.rejections + opened.rejections
        opened.rejections = told
        replica = opened
    }

    /// §12.2 `settle`: the end of a pump — the inbox applied once, the facts
    /// a page left it waiting on asked for, and the next page if it said there is one.
    public func settle() {
        replica.settle()
        if paged {
            paged = false
            let needs = replica.needs()
            if !needs.isEmpty { emit(.needFacts(needs)) }
        }
        if more {
            more = false
            emit(hello())
        }
    }

    /// `verify_all`: ask whether the authority agrees with the confirmed state.
    public func verifyAll() {
        let partial = replica.partial != nil
        if behind() || (partial && !served) { return }
        let (seq, hash) = replica.verifyAt()
        emit(.verify(seq, hash, replica.logId, partial))
    }

    public func takeOutgoing() -> [ClientMsg] {
        let o = out
        out.removeAll()
        return o
    }
}
