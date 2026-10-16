// §14.1 Live state (`rust/ark/src/live/state.rs`, `docs/plan-live.md`): a
// typed state machine per scope, beside the log, on the one socket — the
// frames, how a module's states run, and the client's half.
//
// An **instance** is one state under one key (`listening` of `alice`). The
// server holds every instance and sequences the calls on each with a
// sequence of its own; a client holds what the server last confirmed of the
// instances it subscribes to, and its own calls not yet answered replayed on
// top — the log's rule, **confirmed, then pending replayed on top**, without
// the log.
//
// What crosses the socket, beside the log's frames:
//
// - a `hello` may declare the connection a **device** (`DeviceDecl`);
// - `call`: run this transition on this instance with this input — an
//   ordered call carries an id, an ephemeral one its counter too;
// - `applied`: a call the server took — ordered, with its sequence, who made
//   it, when, and the hash of the state it produced; or ephemeral, with the
//   device's counter and nothing of order;
// - `refused`: to its author alone, a call the server did not take;
// - `live_snapshot`: an instance whole, ephemeral slots included;
// - `resync`: a client's request for one.
//
// What a server does with them is not a client's, and is not here. Sans-io,
// as everything in this runtime is: what arrived is handed in, and what to
// say is handed back.

/// The most bytes an ephemeral slot's value may encode to (`state::SLOT_CAP`).
public let slotCap = 1024

// ---------------------------------------------------------------------------
// Frames

/// One instance: a live state under one key (`state::Instance`), ordered by state then key.
public struct Instance: Hashable, Comparable {
    public let state: String
    public let key: String
    public init(_ state: String, _ key: String) {
        self.state = state
        self.key = key
    }

    public static func < (a: Instance, b: Instance) -> Bool {
        let s = compareText(a.state, b.state)
        return s != 0 ? s < 0 : compareText(a.key, b.key) < 0
    }
}

/// What a connection declares itself as at `hello` (`state::DeviceDecl`): an
/// id that survives its reconnects (a login's session for a client, an
/// entity for a speaker; empty is the connection's session), what kind of
/// thing it is, its name, and whether it can play.
public struct DeviceDecl: Hashable {
    public let id: String
    public let kind: String
    public let name: String
    public let canPlay: Bool
    public init(id: String, kind: String, name: String, canPlay: Bool) {
        self.id = id
        self.kind = kind
        self.name = name
        self.canPlay = canPlay
    }

    /// On the wire, in a `hello`: `{id, kind, name, can_play}`.
    public func toValue() -> Value {
        .record(Record([("id", .text(id)), ("kind", .text(kind)), ("name", .text(name)), ("can_play", .bool(canPlay))]))
    }

    /// The device as a hook is handed it (`deviceTy`), as the server verified its user.
    public func record(_ user: String) -> Value {
        .record(Record([("can_play", .bool(canPlay)), ("id", .text(id)), ("kind", .text(kind)), ("name", .text(name)), ("user", .text(user))]))
    }

    public static func fromValue(_ v: Value) throws -> DeviceDecl {
        let m = try LiveWire.strct(v, "a device")
        return DeviceDecl(id: try LiveWire.text(m, "id"), kind: try LiveWire.text(m, "kind"), name: try LiveWire.text(m, "name"), canPlay: try LiveWire.boolean(m, "can_play"))
    }
}

/// `call` (`state::Call`): run a transition on an instance. `id` names the
/// call; `counter`, an ephemeral call's place among its device's, rising, and
/// nil for an ordered call.
public struct LiveCall: Hashable {
    public let instance: Instance
    public let transition: String
    public let input: [String: Value]
    public let id: Id
    public let counter: Int64?

    public init(_ instance: Instance, _ transition: String, _ input: [String: Value], _ id: Id, _ counter: Int64?) {
        self.instance = instance
        self.transition = transition
        self.input = input
        self.id = id
        self.counter = counter
    }

    public func toValue() -> Value {
        var f = LiveWire.instanceFields(instance)
        f.append(("transition", .text(transition)))
        f.append(("input", .record(Record(input.map { ($0.key, $0.value) }))))
        f.append(("id", .id(id)))
        if let c = counter { f.append(("counter", .int(c))) }
        return LiveWire.tagged("call", f)
    }

    public static func fromFields(_ m: Record) throws -> LiveCall {
        LiveCall(try LiveWire.instanceOf(m), try LiveWire.text(m, "transition"), try LiveWire.args(try LiveWire.get(m, "input"), "an input"), try LiveWire.ident(m, "id"), try m["counter"].map { try LiveWire.int($0, "counter") })
    }
}

/// How an `applied` call was taken (`state::Order`).
public enum Order: Hashable {
    /// Sequenced: the instance's `seq`, the call it answers (nil for a hook
    /// the server ran itself), its author, its roles in ascending order, the
    /// server's time it ran at, and the hash of the state it produced.
    case ordered(seq: Int64, id: Id?, user: String, session: String, roles: [String], at: Int64, hash: [UInt8])
    /// Relayed: the device's counter, which a replica takes only above the last it took.
    case ephemeral(counter: Int64)
}

/// `applied` (`state::Applied`): a call the server took, for every subscriber to run.
public struct LiveApplied: Hashable {
    public let instance: Instance
    public let transition: String
    public let input: [String: Value]
    /// The device that made it — for a hook, the device arriving or leaving.
    public let device: String
    public let order: Order

    public init(_ instance: Instance, _ transition: String, _ input: [String: Value], _ device: String, _ order: Order) {
        self.instance = instance
        self.transition = transition
        self.input = input
        self.device = device
        self.order = order
    }

    /// `ctx`: the context every replica runs it under.
    public var ctx: LiveCtx {
        switch order {
        case .ordered(_, _, let user, let session, let roles, let at, _): return LiveCtx(user: user, session: session, roles: roles, device: device, now: at)
        case .ephemeral: return LiveCtx(device: device)
        }
    }

    public func toValue() -> Value {
        var f = LiveWire.instanceFields(instance)
        f.append(("transition", .text(transition)))
        f.append(("input", .record(Record(input.map { ($0.key, $0.value) }))))
        f.append(("device", .text(device)))
        switch order {
        case .ordered(let seq, let id, let user, let session, let roles, let at, let hash):
            f.append(("seq", .int(seq)))
            if let id = id { f.append(("id", .id(id))) }
            f.append(("user", .text(user)))
            f.append(("session", .text(session)))
            if !roles.isEmpty { f.append(("roles", .list(roles.map { .text($0) }))) }
            f.append(("at", .int(at)))
            f.append(("hash", .bytes(hash)))
        case .ephemeral(let counter):
            f.append(("counter", .int(counter)))
        }
        return LiveWire.tagged("applied", f)
    }

    /// An `applied` carries a `seq` or a `counter`, never both, and what it
    /// carries decides how it is read: one carrying both is refused.
    public static func fromFields(_ m: Record) throws -> LiveApplied {
        let order: Order
        switch (m["seq"], m["counter"]) {
        case (let seq?, nil):
            guard case .bytes(let h) = try LiveWire.get(m, "hash") else { throw LiveWire.bad("hash is bytes") }
            order = .ordered(
                seq: try LiveWire.int(seq, "seq"),
                id: try m["id"].map { _ in try LiveWire.ident(m, "id") },
                user: try LiveWire.text(m, "user"),
                session: try LiveWire.text(m, "session"),
                roles: try rolesOf(m),
                at: try LiveWire.int(try LiveWire.get(m, "at"), "at"),
                hash: h)
        case (nil, let c?):
            for k in ["id", "user", "session", "roles", "at", "hash"] where m.has(k) {
                throw LiveWire.bad("an ephemeral applied carries no \(k)")
            }
            order = .ephemeral(counter: try LiveWire.int(c, "counter"))
        default:
            throw LiveWire.bad("an applied carries a seq or a counter, and not both")
        }
        return LiveApplied(try LiveWire.instanceOf(m), try LiveWire.text(m, "transition"), try LiveWire.args(try LiveWire.get(m, "input"), "an input"), try LiveWire.text(m, "device"), order)
    }

    private static func rolesOf(_ m: Record) throws -> [String] {
        guard let r = m["roles"] else { return [] }
        guard case .list(let rs) = r, !rs.isEmpty else { throw LiveWire.bad("roles: a non-empty list, or no field") }
        let out = try rs.map { x -> String in
            guard case .text(let t) = x else { throw LiveWire.bad("a role is text") }
            return t
        }
        for i in 1..<out.count where compareText(out[i - 1], out[i]) >= 0 {
            throw LiveWire.bad("roles: not in ascending order, or repeated")
        }
        return out
    }
}

/// `refused` (`state::Refused`): a call the server did not take, to its author.
public struct LiveRefused: Hashable {
    public let instance: Instance
    public let id: Id
    public let reason: String
    public init(_ instance: Instance, _ id: Id, _ reason: String) {
        self.instance = instance
        self.id = id
        self.reason = reason
    }

    public func toValue() -> Value {
        var f = LiveWire.instanceFields(instance)
        f.append(("id", .id(id)))
        f.append(("reason", .text(reason)))
        return LiveWire.tagged("refused", f)
    }

    public static func fromFields(_ m: Record) throws -> LiveRefused {
        LiveRefused(try LiveWire.instanceOf(m), try LiveWire.ident(m, "id"), try LiveWire.text(m, "reason"))
    }
}

/// `live_snapshot` (`state::Snapshot`): an instance whole, ephemeral slots and
/// all, with the last counter the server took from each device — `counters`,
/// present only when there is one.
public struct LiveSnapshot: Hashable {
    public let instance: Instance
    public let seq: Int64
    public let value: Value
    public let counters: [String: Int64]
    public init(_ instance: Instance, _ seq: Int64, _ value: Value, _ counters: [String: Int64]) {
        self.instance = instance
        self.seq = seq
        self.value = value
        self.counters = counters
    }

    public func toValue() -> Value {
        var f = LiveWire.instanceFields(instance)
        f.append(("seq", .int(seq)))
        f.append(("value", value))
        if !counters.isEmpty { f.append(("counters", .record(Record(counters.map { ($0.key, .int($0.value)) })))) }
        return LiveWire.tagged("live_snapshot", f)
    }

    public static func fromFields(_ m: Record) throws -> LiveSnapshot {
        var counters = [String: Int64]()
        if let v = m["counters"] {
            let c = try LiveWire.strct(v, "counters")
            if c.isEmpty { throw LiveWire.bad("no counters is written as no field") }
            for (k, n) in c.pairs { counters[k] = try LiveWire.int(n, "a counter") }
        }
        return LiveSnapshot(try LiveWire.instanceOf(m), try LiveWire.int(try LiveWire.get(m, "seq"), "seq"), try LiveWire.get(m, "value"), counters)
    }
}

/// What the server says of live state, to one connection (`state::Said`).
public enum Said {
    case applied(LiveApplied)
    case refused(LiveRefused)
    case snapshot(LiveSnapshot)
}

/// The wire forms' small parts: every frame a tagged struct, an instance `live` and `key`.
enum LiveWire {
    static func bad(_ what: String) -> FactError { FactError("frame: \(what)") }

    static func instanceFields(_ i: Instance) -> [(String, Value)] { [("live", .text(i.state)), ("key", .text(i.key))] }

    static func tagged(_ t: String, _ fields: [(String, Value)]) -> Value { .record(Record(fields + [("t", .text(t))])) }

    static func instanceOf(_ m: Record) throws -> Instance { Instance(try text(m, "live"), try text(m, "key")) }

    static func get(_ m: Record, _ k: String) throws -> Value {
        guard let v = m[k] else { throw bad("missing \(k)") }
        return v
    }

    static func strct(_ v: Value, _ what: String) throws -> Record {
        guard case .record(let r) = v else { throw bad("\(what) is a struct") }
        return r
    }

    static func args(_ v: Value, _ what: String) throws -> [String: Value] {
        var out = [String: Value]()
        for (k, x) in try strct(v, what).pairs { out[k] = x }
        return out
    }

    static func text(_ m: Record, _ k: String) throws -> String {
        guard case .text(let s) = try get(m, k) else { throw bad("\(k) is text") }
        return s
    }

    static func boolean(_ m: Record, _ k: String) throws -> Bool {
        guard case .bool(let b) = try get(m, k) else { throw bad("\(k) is a bool") }
        return b
    }

    static func int(_ v: Value, _ k: String) throws -> Int64 {
        guard case .int(let n) = v else { throw bad("\(k) is an int") }
        return n
    }

    static func ident(_ m: Record, _ k: String) throws -> Id {
        guard case .id(let i) = try get(m, k) else { throw bad("\(k) is an id") }
        return i
    }
}

/// `resync`'s value: a client asking for an instance whole.
public func resyncValue(_ i: Instance) -> Value { LiveWire.tagged("resync", LiveWire.instanceFields(i)) }

/// `resync`'s instance, from its fields.
public func resyncInstance(_ m: Record) throws -> Instance { try LiveWire.instanceOf(m) }

// ---------------------------------------------------------------------------
// Running

/// The live states of a module, and how each runs (`state::Runner`):
/// through `Eval.runTransition`, interpreted — what every runtime but Rust
/// does, and what Rust does for a state it holds no native code for.
public struct Runner {
    public let closures: [String: LiveClosure]

    public init(_ closures: [String: LiveClosure]) { self.closures = closures }

    /// Every live state of `m`, interpreted.
    public static func ofModule(_ m: Module) -> Runner { Runner(Hash.liveClosures(m)) }

    /// Whether the module has any live state at all.
    public var isEmpty: Bool { closures.isEmpty }

    private func closure(_ state: String) throws -> LiveClosure {
        guard let c = closures[state] else { throw EvalFault.verdict(.refused("no live state \(state)")) }
        return c
    }

    /// The kind of a transition: ordered for a state or a name there is not.
    public func kind(_ state: String, _ transition: String) -> Kind { closures[state]?.kind(transition) ?? .ordered }

    /// The value an instance of `state` starts as.
    public func initial(_ state: String) throws -> Value { try Eval.constant(try closure(state).state.initial) }

    /// The keys of the instances of `state` this author holds.
    public func keys(_ state: String, _ who: Ctx) throws -> [String] { try Eval.scopeKeys(try closure(state), who) }

    /// The hash of an instance's value, ephemeral fields left out.
    public func hash(_ state: String, _ v: Value) -> [UInt8] { closures[state]?.valueHash(v) ?? Sha256.hash(Canon.encode(v)) }

    /// The fields of `state` that are ephemeral.
    public func ephemeral(_ state: String) -> [String] { closures[state]?.ephemeral ?? [] }

    /// §14 `run`: a transition — a hook by its reserved name — over `value`.
    /// Leaving empties the device's slot of every ephemeral field first,
    /// which is the engine's and no hook's to forget; and an ephemeral call's
    /// slot is held to `slotCap`. A verdict or a bug is thrown as an `EvalFault`.
    public func run(_ state: String, _ transition: String, _ value: Value, _ ctx: LiveCtx, _ input: [String: Value]) throws -> Value {
        let c = try closure(state)
        let v = transition == onLeaveName ? Runner.emptySlots(c.ephemeral, value, Runner.deviceOf(input)) : value
        let out = try Eval.runTransition(c, transition, v, ctx, input)
        if c.kind(transition) == .ephemeral, case .record(let r) = out {
            for f in c.ephemeral {
                guard case .record(let slots)? = r[f], let slot = slots[ctx.device] else { continue }
                let n = Canon.encode(slot).count
                if n > slotCap { throw EvalFault.verdict(.refused("\(f): a slot holds at most \(slotCap) bytes, not \(n)")) }
            }
        }
        return out
    }

    // The id of the device a hook is handed.
    private static func deviceOf(_ input: [String: Value]) -> String {
        if case .record(let d)? = input["device"], case .text(let t)? = d["id"] { return t }
        return ""
    }

    // `v` with `device`'s slot of every ephemeral field emptied.
    private static func emptySlots(_ eph: [String], _ v: Value, _ device: String) -> Value {
        guard case .record(var r) = v, !eph.isEmpty else { return v }
        for f in eph {
            guard case .record(var slots)? = r[f] else { continue }
            slots.remove(device)
            r.set(f, .record(slots))
        }
        return .record(r)
    }
}

/// `verdict_text`: a fault as the words a refusal is sent with.
func liveVerdictText(_ e: Error) -> String {
    switch e {
    case EvalFault.verdict(let r): return refusalText(r)
    case EvalFault.bug(let b): return bugText(b)
    default: return "\(e)"
    }
}

// ---------------------------------------------------------------------------
// The client's half

/// An ordered call of this device's the server has not yet answered
/// (`state::Pending`), with what its preview ran under.
public struct PendingCall {
    public let id: Id
    public let transition: String
    public let input: [String: Value]
    public let ctx: LiveCtx
}

/// One instance as a client holds it (`state::Watched`): what the server
/// last confirmed, at its sequence, and the view — the confirmed state with
/// this device's pending calls replayed on top.
public struct WatchedInstance {
    public var confirmed: Value
    public var seq: Int64
    /// A snapshot has arrived on this connection: before one, nothing of the instance applies.
    public var synced: Bool
    /// A `resync` was asked and its snapshot has not arrived.
    public var asked: Bool
    public var pending: [PendingCall]
    public var view: Value
    /// The last counter taken from each device for its ephemeral calls.
    public var counters: [String: Int64]
}

/// What a client's call came to (`state::Called`): its kind, and the frame to send if linked.
public struct Called {
    public let kind: Kind
    public let call: LiveCall
}

/// A live call refused by its own preview (`Refusal`, in the reference's `Result`).
public struct LiveRefusal: Error, CustomStringConvertible {
    public let refusal: Refusal
    public var description: String { refusalText(refusal) }
}

/// §14 A client's live state (`state::Watch`): every instance it has heard of
/// or called on, and the device it declares itself as — nil, and it is no
/// device, subscribes to nothing, and its frames are the bytes they always
/// were.
public struct Watch {
    public let runner: Runner
    public let device: DeviceDecl?
    public private(set) var instances = SortedMap<Instance, WatchedInstance>()
    /// Verdicts the server gave this device's calls, oldest first, until taken.
    public private(set) var refused = [(Id, String)]()
    private var moved = false

    public init(_ runner: Runner, _ device: DeviceDecl?) {
        self.runner = runner
        self.device = device
    }

    /// Something moved since the last ask: a view, a verdict.
    public mutating func takeMoved() -> Bool {
        defer { moved = false }
        return moved
    }

    /// `instance_of`: the instance of `state` this author holds — its scope's first key.
    public func instanceOf(_ state: String, _ who: Ctx) -> Instance? {
        guard let k = (try? runner.keys(state, who))?.first else { return nil }
        return Instance(state, k)
    }

    /// An instance's view: the confirmed state with pending calls on top.
    public func view(_ i: Instance) -> Value? { instances[i]?.view }

    private func deviceId(_ who: Ctx) -> String {
        if let d = device, !d.id.isEmpty { return d.id }
        return who.session
    }

    private mutating func watched(_ i: Instance) throws -> WatchedInstance {
        if let w = instances[i] { return w }
        let v: Value
        do {
            v = try runner.initial(i.state)
        } catch {
            throw LiveRefusal(refusal: .refused(liveVerdictText(error)))
        }
        let w = WatchedInstance(confirmed: v, seq: 0, synced: false, asked: false, pending: [], view: v, counters: [:])
        instances[i] = w
        return w
    }

    /// §14.2 `call`: this device calls a transition, run at once over its view
    /// with `now` its guess at the server's clock. Ordered, it is pending
    /// until the server answers; ephemeral, it is applied and done, `counter`
    /// its place among this device's. A refusal here is the preview's
    /// verdict, thrown as a `LiveRefusal`, and nothing is sent.
    public mutating func call(_ who: Ctx, _ i: Instance, _ transition: String, _ input: [String: Value], _ id: Id, _ now: Int64, _ counter: Int64) throws -> Called {
        let kind = runner.kind(i.state, transition)
        if kind == .ordered && isHook(transition) {
            throw LiveRefusal(refusal: .refused("\(transition) is the server's to run"))
        }
        let ctx = LiveCtx(user: who.user, session: who.session, roles: who.roles, device: deviceId(who), now: now)
        var w = try watched(i)
        let preview: Value
        do {
            preview = try runner.run(i.state, transition, w.view, ctx, input)
        } catch EvalFault.verdict(let r) {
            throw LiveRefusal(refusal: r)
        } catch EvalFault.bug(let b) {
            throw LiveRefusal(refusal: .refused(bugText(b)))
        }
        let call: LiveCall
        switch kind {
        case .ephemeral:
            if let v = try? runner.run(i.state, transition, w.confirmed, ctx, input) { w.confirmed = v }
            w.counters[ctx.device] = counter
            call = LiveCall(i, transition, input, id, counter)
        case .ordered:
            w.pending.append(PendingCall(id: id, transition: transition, input: input, ctx: ctx))
            call = LiveCall(i, transition, input, id, nil)
        }
        w.view = preview
        if kind == .ephemeral { replay(i.state, &w) }
        instances[i] = w
        moved = true
        return Called(kind: kind, call: call)
    }

    /// `connected`: a connection opened — every instance waits for its
    /// snapshot, and every pending call is said again, in the order it was made.
    public mutating func connected() -> [LiveCall] {
        var out = [LiveCall]()
        for i in instances.keys {
            var w = instances[i]!
            w.synced = false
            w.asked = false
            for p in w.pending { out.append(LiveCall(i, p.transition, p.input, p.id, nil)) }
            instances[i] = w
        }
        return out
    }

    /// `heard`: what the server said; the instances to ask a snapshot of — a
    /// gap in the sequence, or a state whose hash is not the server's.
    public mutating func heard(_ said: Said) -> [Instance] {
        switch said {
        case .snapshot(let s):
            if runner.closures[s.instance.state] != nil {
                var w = instances[s.instance] ?? WatchedInstance(confirmed: s.value, seq: s.seq, synced: true, asked: false, pending: [], view: s.value, counters: [:])
                w.confirmed = s.value
                w.seq = s.seq
                w.synced = true
                w.asked = false
                w.counters = s.counters
                replay(s.instance.state, &w)
                instances[s.instance] = w
                moved = true
            }
            return []
        case .refused(let r):
            guard var w = instances[r.instance] else { return [] }
            let before = w.pending.count
            w.pending.removeAll { $0.id == r.id }
            if w.pending.count != before {
                refused.append((r.id, r.reason))
                replay(r.instance.state, &w)
                instances[r.instance] = w
                moved = true
            }
            return []
        case .applied(let a):
            guard var w = instances[a.instance], w.synced else { return [] }
            defer { instances[a.instance] = w }
            switch a.order {
            case .ephemeral(let counter):
                if let last = w.counters[a.device], last >= counter { return [] }
                w.counters[a.device] = counter
                if let v = try? runner.run(a.instance.state, a.transition, w.confirmed, a.ctx, a.input) {
                    w.confirmed = v
                    replay(a.instance.state, &w)
                    moved = true
                }
                return []
            case .ordered(let seq, let id, _, _, _, _, let hash):
                // Already confirmed — the answer to a call pushed again —
                // leaves pending what it answers.
                if seq <= w.seq {
                    if let id = id {
                        let before = w.pending.count
                        w.pending.removeAll { $0.id == id }
                        if w.pending.count != before {
                            replay(a.instance.state, &w)
                            moved = true
                        }
                    }
                    return []
                }
                if seq > w.seq + 1 { return ask(&w, a.instance) }
                // Not the server's state: ask for it whole.
                guard let next = try? runner.run(a.instance.state, a.transition, w.confirmed, a.ctx, a.input), runner.hash(a.instance.state, next) == hash else {
                    return ask(&w, a.instance)
                }
                w.confirmed = next
                w.seq = seq
                if let id = id { w.pending.removeAll { $0.id == id } }
                replay(a.instance.state, &w)
                moved = true
                return []
            }
        }
    }

    /// Verdicts on this device's calls since the last ask.
    public mutating func takeRefused() -> [(Id, String)] {
        defer { refused.removeAll() }
        return refused
    }

    // A snapshot asked for, once until it arrives.
    private func ask(_ w: inout WatchedInstance, _ i: Instance) -> [Instance] {
        if w.asked { return [] }
        w.asked = true
        return [i]
    }

    // The view again: the confirmed state with every pending call replayed on
    // top, in order, under what each was made with; one the state no longer
    // takes is passed over, still pending, until the server answers it.
    private func replay(_ state: String, _ w: inout WatchedInstance) {
        var v = w.confirmed
        for p in w.pending {
            if let x = try? runner.run(state, p.transition, v, p.ctx, p.input) { v = x }
        }
        w.view = v
    }
}
