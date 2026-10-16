// `docs/plan-phones.md` S2 A trace, as `rust/ark/src/trace.rs` defines it:
// one client's life in a session, written down as its inputs and its
// outputs, so that this client is tested as a function of what it was given
// — with nothing left to a scheduler, and no server in it: what the server
// said is already the frames. `spec/RUNNERS.md` § trace is the contract;
// `Player` is it as code, as `trace::Player` is in the reference.
//
// The client is `Client` over its `Replica`, with what it makes durable
// (`Durable`) and, alone, its own `Authority`. A step is one input and what
// the client did after it: the frames it queued as canonical bytes, a local
// refusal, what each kept view reports, and its state, written only where
// it moved — every field after an open.

/// A view a trace keeps up to date (`trace::ViewSpec`).
public struct ViewSpec {
    public let query: String
    public let ctx: Ctx
    public let args: [String: Value]
}

/// One input to a client (`trace::Input`).
public enum Input {
    /// The last: §14 the device its `hello` declares, for a client of a module with live state.
    case open(Mode, Bool, String?, DeviceDecl?)
    case reopen
    case connected
    case disconnected
    case mutate(Ctx, String, [UInt8], Id, [String: Value], [String: Value])
    case signIn(Ctx, String?)
    case token(String?)
    case receive([UInt8])
    case settle
    case verify
    case alone
    case join(Ctx)
    case hydrate([ViewSpec])
    /// §14 `Client.call`: a transition of a live instance, by this author,
    /// with the time it guessed for the server's and the id and counter it
    /// drew — frozen, as a mutation's autos are.
    case call(Ctx, Instance, String, [String: Value], Id, Int64, Int64)

    /// `Input::from_value`: an input read back; a `TraceError` names what is not one.
    public static func fromValue(_ v: Value) throws -> Input {
        let m = try strct(v, "an input")
        func get(_ k: String) throws -> Value {
            guard let x = m[k] else { throw TraceError("an input without \(k): \(v.show())") }
            return x
        }
        func token(_ k: String) throws -> String? {
            switch try get(k) {
            case .null: return nil
            case .text(let s): return s
            default: throw TraceError("\(k) is neither text nor null")
            }
        }
        func args(_ k: String) throws -> [String: Value] { dict(try strct(try get(k), k)) }
        guard case .text(let t) = try get("t") else { throw TraceError("an input whose t is not text") }
        switch t {
        case "open":
            guard case .text(let mw) = try get("mode"), let mode = Mode(rawValue: mw) else { throw TraceError("mode is neither whole nor facts") }
            guard case .bool(let cl) = try get("closures") else { throw TraceError("closures is not a bool") }
            var device: DeviceDecl? = nil
            if let d = m["device"] {
                do { device = try DeviceDecl.fromValue(d) } catch { throw TraceError("\(error)") }
            }
            return .open(mode, cl, try token("token"), device)
        case "reopen": return .reopen
        case "connected": return .connected
        case "disconnected": return .disconnected
        case "mutate":
            guard case .text(let fname) = try get("function") else { throw TraceError("function is not text") }
            guard case .bytes(let fh) = try get("fn") else { throw TraceError("fn is not bytes") }
            guard case .id(let id) = try get("id") else { throw TraceError("id is not an id") }
            return .mutate(try ctxOf(try get("ctx")), fname, fh, id, try args("autos"), try args("args"))
        case "sign_in": return .signIn(try ctxOf(try get("ctx")), try token("token"))
        case "token": return .token(try token("token"))
        case "receive":
            guard case .bytes(let b) = try get("frame") else { throw TraceError("frame is not bytes") }
            return .receive(b)
        case "settle": return .settle
        case "verify": return .verify
        case "alone": return .alone
        case "join": return .join(try ctxOf(try get("ctx")))
        case "call":
            guard case .text(let live) = try get("live") else { throw TraceError("live is not text") }
            guard case .text(let key) = try get("key") else { throw TraceError("key is not text") }
            guard case .text(let tr) = try get("transition") else { throw TraceError("transition is not text") }
            guard case .id(let id) = try get("id") else { throw TraceError("id is not an id") }
            guard case .int(let now) = try get("now") else { throw TraceError("now is not an int") }
            guard case .int(let counter) = try get("counter") else { throw TraceError("counter is not an int") }
            return .call(try ctxOf(try get("ctx")), Instance(live, key), tr, try args("input"), id, now, counter)
        case "hydrate":
            guard case .list(let vs) = try get("views") else { throw TraceError("views is not a list") }
            return .hydrate(try vs.map { s in
                let f = try strct(s, "a view")
                guard case .text(let q)? = f["query"] else { throw TraceError("a view's query") }
                guard let c = f["ctx"] else { throw TraceError("a view without ctx") }
                guard let a = f["args"] else { throw TraceError("a view without args") }
                return ViewSpec(query: q, ctx: try ctxOf(c), args: dict(try strct(a, "args")))
            })
        default: throw TraceError("unknown input \(t)")
        }
    }

    private static func strct(_ v: Value, _ what: String) throws -> Record {
        guard case .record(let r) = v else { throw TraceError("\(what) is not a struct: \(v.show())") }
        return r
    }

    private static func dict(_ r: Record) -> [String: Value] {
        var out = [String: Value]()
        for (k, v) in r.pairs { out[k] = v }
        return out
    }

    private static func ctxOf(_ v: Value) throws -> Ctx {
        let m = try strct(v, "a ctx")
        func text(_ k: String) throws -> String {
            guard case .text(let s)? = m[k] else { throw TraceError("ctx \(k)") }
            return s
        }
        var roles = [String]()
        switch m["roles"] {
        case nil: break
        case .list(let rs)?:
            roles = try rs.map { r in
                guard case .text(let s) = r else { throw TraceError("a role: \(r.show())") }
                return s
            }
        default: throw TraceError("ctx roles")
        }
        return Ctx(try text("user"), try text("session"), roles: roles)
    }
}

/// A trace that is not one, or a step a client cannot take (`trace.rs`'s `Err(String)`).
public struct TraceError: Error, CustomStringConvertible {
    public let what: String
    public init(_ what: String) { self.what = what }
    public var description: String { what }
}

/// A client that is its own authority (`trace::Alone`): where it forked —
/// the log and cursor it last shared with a server — and the authority over
/// its local history since.
public final class Alone {
    public let authority: Authority
    public let fork: Seq
    public let forkLog: Id?
    public init(_ authority: Authority, _ fork: Seq, _ forkLog: Id?) {
        self.authority = authority
        self.fork = fork
        self.forkLog = forkLog
    }
}

/// Server → alone (`trace::alone`, `docs/plan-alone.md` §1): the fork is
/// where the replica stands; the authority starts from the confirmed store
/// with nothing in its log, over the closures the client holds, and is no
/// server's — its private blocks are not run (D3); the replica holds no
/// named log from here, and nothing of the server's it had not yet applied
/// — the inbox is left behind, or it would be applied as the local
/// history's own next sequence; and what is pending is sequenced, locally.
@discardableResult
public func goAlone(_ c: Client) -> Alone {
    let r = c.replica
    r.inbox.removeAll()
    r.through = r.cursor
    let a = Authority(r.schema, r.bodies)
    a.log = Log(r.cursor, MemoryStore.empty(r.schema), [], nil)
    a.store = r.confirmed.copy()
    let out = Alone(a, r.cursor, r.logId)
    r.logId = nil
    commit(out, c)
    return out
}

/// Alone, everything pending sequenced by the local authority (`trace::commit`).
public func commit(_ a: Alone, _ c: Client) { localCommit(a.authority, c.replica) }

/// Alone → server (`trace::join`): the local history above the fork taken
/// back out of the confirmed store and re-queued as `who`'s — the same ids,
/// functions, autos and arguments (`Replica.forkBack`). The caller connects.
public func join(_ c: Client, _ a: Alone, _ who: Ctx) {
    var facts = [FactList]()
    var requeue = [LogEntry]()
    for (n, ef) in a.authority.log.entries where n > a.fork {
        requeue.append(ef.0.by(who.user, who.session))
        facts.append(ef.1)
    }
    c.replica.forkBack(facts, a.fork, a.forkLog, requeue)
}

/// What a client makes durable, as a runner keeps it (§11.1): told after
/// every step, and what a `reopen` opens from. `KeptStore` is the trace's
/// own (`trace::keep`, `trace::reopen`); a driver's storage is another
/// (`ArkDBClient`'s records), held to the same traces.
public protocol Durable: AnyObject {
    /// A fresh client: nothing durable yet.
    func clear()
    /// After every step: what the confirmed store moved by, with the cursor, log and union.
    func keep(_ c: Client, _ alone: Alone?) throws
    /// `reopen`: the client stopped and opened again from what it made durable.
    func reopen(_ old: Client) throws -> Client
}

/// `sim::Kept` and `trace::keep`: the confirmed store moved only by
/// `Replica.takeConfirmed`, the cursor, the log and the union beside it; the
/// pending intents durable as the replica's own list.
public final class KeptStore: Durable {
    private var store: MemoryStore? = nil
    private var cursor: Seq = 0
    private var logId: Id? = nil
    private var partial: [String: [String]]? = nil

    public init() {}

    public func clear() { store = nil }

    public func keep(_ c: Client, _ alone: Alone?) {
        let r = c.replica
        if store == nil { store = r.confirmed.copy() }
        switch r.takeConfirmed() {
        case .replaced: store = r.confirmed.copy()
        case .run(let run): for (_, f) in run { store!.applyChanges(f) }
        }
        cursor = r.cursor
        logId = r.logId
        partial = r.partialMap
    }

    public func reopen(_ old: Client) throws -> Client {
        guard let st = store else { throw TraceError("a reopen with nothing durable") }
        return reopenFrom(old, st.copy(), cursor, logId, partial, old.replica.pending)
    }
}

/// `trace::reopen`: a client stopped and opened again from what it made
/// durable — the replica over the store at its cursor, under the device's
/// schema when it held a union (D2), with its log, its union, a page horizon
/// at the cursor, the pending intents, and the closures it held; the client
/// over it with the same mode and token, and the module's schema. Unlinked.
public func reopenFrom(_ old: Client, _ store: MemoryStore, _ cursor: Seq, _ logId: Id?, _ partial: [String: [String]]?, _ pending: [LogEntry]) -> Client {
    let r = Replica.open(store.schema, old.replica.bodies, store, cursor, pending)
    r.logId = logId
    if let p = partial {
        var m = SortedMap<CodePoints, [String]>()
        for (k, v) in p { m[CodePoints(k)] = v }
        r.partial = m
    }
    r.through = cursor
    let c = Client.open(r, old.mode, old.token).withSchema(old.schema)
    // §14 Live state is about now and nothing of it is durable: a client
    // started again holds none of it, and is told it whole when it next connects.
    guard let w = old.live else { return c }
    return c.withLive(Watch(w.runner, w.device))
}

/// `client_bodies`: the module's closures as a client holds them — private blocks stripped (D3).
public func clientBodies(_ bodies: [String: Closure]) -> [String: Closure] { bodies.mapValues(Hash.stripped) }

private final class Watched {
    let spec: ViewSpec
    var view: MaintainedView?
    init(_ spec: ViewSpec, _ view: MaintainedView?) {
        self.spec = spec
        self.view = view
    }
}

/// What a client's step produced, read off it (`trace::Observer`): the views
/// the trace keeps and the state the last step wrote, which is what lets a
/// step write only what moved.
public final class Observer {
    private var queries = [String: Closure]()
    private var views = [Watched]()
    private var events = [Value]()
    private var last = [String: Value]()

    public init(_ bodies: [String: Closure]) {
        for c in bodies.values where c.function.kind == .query { queries[c.function.name] = c }
    }

    /// The client was opened: the next step writes every state field.
    public func reset() { last.removeAll() }

    private func refused(_ why: String) -> Value { .record([("refused", .text(why))]) }

    private func faultText(_ e: Error) -> String {
        switch e {
        case EvalFault.verdict(let r): return refusalText(r)
        case EvalFault.bug(let b): return bugText(b)
        default: return "bug: \(e)"
        }
    }

    // A view read over `st`: what it holds, and what it reports.
    private func openView(_ spec: ViewSpec, _ st: MemoryStore) -> (MaintainedView?, Value) {
        let sch = st.schema
        guard let c = queries[spec.query] else { return (nil, refused("no query \(spec.query)")) }
        guard let plan = c.function.plan else { return (nil, refused("\(spec.query) has no plan")) }
        do {
            let (args, provided) = try Eval.middleware(sch, c, spec.ctx, spec.args, st)
            let v = try Views.hydrate(sch, plan, ViewEnv(c.helpers, spec.ctx, args, provided), st)
            return (v, .record([("rows", .list(v.rows()))]))
        } catch {
            return (nil, refused(faultText(error)))
        }
    }

    /// These views in place of any held, hydrated over `st`.
    public func hydrate(_ specs: [ViewSpec], _ st: MemoryStore) {
        views.removeAll()
        events.removeAll()
        for spec in specs {
            let (view, ev) = openView(spec, st)
            views.append(Watched(spec, view))
            events.append(ev)
        }
    }

    /// `Observer::step`: the step's outputs.
    public func step(_ c: Client, _ frames: [ClientMsg], _ changes: Changes, _ refusal: Refusal?) -> Value {
        var out = [(String, Value)]()
        if !frames.isEmpty { out.append(("frames", .list(frames.map { .bytes($0.encode()) }))) }
        if let r = refusal { out.append(("refused", .text(refusalText(r)))) }
        let st = c.replica.view
        while events.count < views.count { events.append(.null) }
        while events.count > views.count { events.removeLast() }
        switch changes {
        case .rebuilt:
            for k in views.indices {
                let (view, ev) = openView(views[k].spec, st)
                views[k].view = view
                events[k] = ev
            }
        case .applied(let chs):
            if !chs.isEmpty {
                let sch = st.schema
                for k in views.indices {
                    guard let v = views[k].view else { continue }
                    do {
                        events[k] = .record([("patches", .list(try Views.pushAll(sch, st, chs, v).map(Views.patchValue)))])
                    } catch {
                        views[k].view = nil
                        events[k] = refused(faultText(error))
                    }
                }
            }
        }
        if events.contains(where: { !$0.isNull }) { out.append(("views", .list(events))) }
        events.removeAll()
        for (k, v) in state(c) where last[k] != v {
            out.append((k, v))
            last[k] = v
        }
        return .record(out)
    }
}

/// `trace::state`: the state a step is held to, field by field.
public func state(_ c: Client) -> [(String, Value)] {
    let r = c.replica
    var union: Value = .null
    if let p = r.partial { union = .record(Record(p.entries.map { ($0.key.s, Value.list($0.value.map { .text($0) })) })) }
    return [
        ("cursor", .int(r.cursor)),
        ("log", r.logId.map { .id($0) } ?? .null),
        ("pending", .list(r.pending.map { .id($0.id) })),
        ("rejections", .list(r.rejections.map { .record([("id", .id($0.0)), ("reason", .text(refusalText($0.1)))]) })),
        ("diverged", .list(r.diverged.map { .int($0) })),
        ("agreed", .list(c.agreed.map { .record([("seq", .int($0.0)), ("ok", $0.1.map { .bool($0) } ?? .null)]) })),
        ("denied", c.denied.map { .text($0) } ?? .null),
        ("union", union),
        ("hash", .bytes(Hash.stateHash(r.confirmed))),
        ("view_hash", .bytes(Hash.stateHash(r.view))),
    ] + liveState(c)
}

// §14 A client holding live state is held to it too: each instance's
// sequence, whether a snapshot has come on this connection, the hash of what
// is confirmed, its view whole and the ids still pending; and the server's
// verdicts on its calls. Absent for every client that holds none.
private func liveState(_ c: Client) -> [(String, Value)] {
    guard let w = c.live else { return [] }
    let instances: [Value] = w.instances.entries.map { e in
        .record(Record([
            ("live", .text(e.key.state)),
            ("key", .text(e.key.key)),
            ("seq", .int(e.value.seq)),
            ("synced", .bool(e.value.synced)),
            ("hash", .bytes(w.runner.hash(e.key.state, e.value.confirmed))),
            ("view", e.value.view),
            ("pending", .list(e.value.pending.map { .id($0.id) })),
        ]))
    }
    let refused: [Value] = w.refused.map { .record(Record([("id", .id($0.0)), ("reason", .text($0.1))])) }
    return [("live", .list(instances)), ("live_refused", .list(refused))]
}

/// A client driven by nothing but a trace's inputs (`trace::Player`): what
/// a runner in any language is. `durable` is what it makes durable — the
/// trace's own copy by default.
public final class Player {
    private let moduleSchema: Schema
    private let live: Runner
    private let bodies: [String: Closure]
    private let observer: Observer
    private let durable: Durable
    private var client: Client? = nil
    private var alone: Alone? = nil

    public init(_ m: Module, _ durable: Durable = KeptStore()) {
        moduleSchema = m.schema
        live = Runner.ofModule(m)
        var bs = [String: Closure]()
        for (k, v) in Hash.closures(m) { bs[k] = v }
        bodies = bs
        observer = Observer(bs)
        self.durable = durable
    }

    /// The client as it stands, once opened.
    public var current: Client? { client }

    /// One input, and what the client did after it.
    public func step(_ input: Input) throws -> Value {
        var refusal: Refusal? = nil
        if case .open(let mode, let closures, let token, let device) = input {
            let bs = closures ? clientBodies(bodies) : [:]
            let r = Replica.open(moduleSchema, bs, MemoryStore.empty(moduleSchema), 0, [])
            let c = Client.open(r, mode, token)
            // §14 A client of a module with live state that declares a device
            // holds it, as that device. One that declares none holds none and
            // reports none (spec/RUNNERS.md, `live`), however much live state
            // its module has.
            if !live.isEmpty, let d = device { _ = c.withLive(Watch(live, d)) }
            client = c
            durable.clear()
            alone = nil
            observer.reset()
        }
        guard var c = client else { throw TraceError("an input before the client was opened") }
        switch input {
        case .open: break
        case .reopen:
            c = try durable.reopen(c)
            client = c
            observer.reset()
        case .connected: c.connected()
        case .disconnected: c.disconnected()
        case .mutate(let ctx, _, let fh, let id, let autos, let args):
            refusal = c.mutate(id, ctx, fh, autos, args).1
            if let a = alone { commit(a, c) }
        case .signIn(let ctx, let token): c.signIn(ctx, token)
        case .token(let t): c.token = t
        case .receive(let frame):
            switch try ServerMsg.decodeFor(frame, c.schema) {
            case .snap(let s): c.recvSnapshot(s)
            case .msg(let m): c.recv(m)
            }
        case .settle: c.settle()
        case .verify: c.verifyAll()
        case .alone: alone = goAlone(c)
        case .join(let who):
            guard let a = alone else { throw TraceError("a join while not alone") }
            alone = nil
            join(c, a, who)
        case .hydrate(let specs): observer.hydrate(specs, c.replica.view)
        case .call(let ctx, let i, let transition, let input, let id, let now, let counter):
            do {
                _ = try c.call(ctx, i, transition, input, id, now, counter)
            } catch let e as LiveRefusal {
                refusal = e.refusal
            }
        }
        let frames = c.takeOutgoing()
        let changes = c.replica.takeChanges()
        try durable.keep(c, alone)
        return observer.step(c, frames, changes, refusal)
    }
}

/// `trace::replay`: every step replayed into a fresh client and its outputs
/// held to the recorded ones, value for value — every frame byte for byte.
/// The first step that differs is named, with each field that does, as a `TraceError`.
public func replay(_ m: Module, _ steps: [(Value, Value)], _ durable: Durable = KeptStore()) throws {
    let p = Player(m, durable)
    for (k, step) in steps.enumerated() {
        let (i, want) = step
        let t = i.recordValue?["t"]?.textValue ?? "?"
        let input: Input
        do {
            input = try Input.fromValue(i)
        } catch {
            throw TraceError("step \(k): \(error)")
        }
        let got: Value
        do {
            got = try p.step(input)
        } catch let e as TraceError {
            throw TraceError("step \(k) (\(t)): \(e.what)")
        } catch let e as FactError {
            throw TraceError("step \(k) (\(t)): \(e.what)")
        } catch let e as DecodeError {
            throw TraceError("step \(k) (\(t)): decoding a frame: \(e)")
        }
        if got != want { throw TraceError("step \(k) (\(t)): \(differs(want, got))") }
    }
}

/// `trace::differs`: the fields of two outputs that differ, each with both sides.
public func differs(_ want: Value, _ got: Value) -> String {
    let w = want.recordValue ?? Record()
    let g = got.recordValue ?? Record()
    var keys = SortedSet<CodePoints>()
    for k in w.names { keys.insert(CodePoints(k)) }
    for k in g.names { keys.insert(CodePoints(k)) }
    func show(_ v: Value?) -> String { v.map { String($0.show().prefix(400)) } ?? "absent" }
    return keys.items.map { $0.s }.filter { w[$0] != g[$0] }.map { "\($0): recorded \(show(w[$0])), replayed \(show(g[$0]))" }.joined(separator: "; ")
}
