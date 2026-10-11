// A query held as a list and kept up to date (`rust/ark-client/src/view.rs`).
//
// Hydrate once, then hand every `Changes` to `update` and splice what it
// returns into whatever the screen built from the rows. Every query is
// maintained: a query is a plan, and `Views` keeps a plan's entries, so a
// change costs the entries it touches whatever the size of the list.
//
// A query's middleware — its input checks, guards and provides — runs before
// the hydrate, over the same store, and decides the scope the plan is pulled
// in (§1.7). The view keeps the tables that middleware reads (`Reads`); a
// change to one of them re-runs it before anything is pushed, and a
// different outcome — another provided value, a refusal appearing or
// clearing, or another user signed in — re-hydrates and reports `reset`. A
// rebase is changes like any other; `Changes.rebuilt` — the optimistic store
// replaced whole — is a re-hydrate and `reset`.
import ArkDB

/// What an update did to the list (`view::Update`).
public enum Update: Equatable {
    case unchanged
    /// Apply these, in order, to the list as it stood (`ClientView.splice`).
    case patched([Patch])
    /// The list was read again: take `ClientView.rows` whole.
    case reset
}

/// A maintained query (`ark_client::View`).
public final class ClientView {
    public let name: String
    public let args: [String: Value]
    private var ctx: Ctx
    /// The tables the middleware reads (§1.7).
    private let guards: Set<String>
    // The checked input and the provided values, or the verdict that leaves the list empty.
    private var outcome: Outcome
    private var held: MaintainedView? = nil

    enum Outcome: Equatable {
        case ran([String: Value], [String: Value])
        case refused(Refusal)

        static func == (a: Outcome, b: Outcome) -> Bool {
            switch (a, b) {
            case (.ran(let x, let p), .ran(let y, let q)): return argsEqual(x, y) && argsEqual(p, q)
            case (.refused(let r), .refused(let s)): return refusalText(r) == refusalText(s)
            default: return false
            }
        }
    }

    /// The list, as it stands.
    public private(set) var rows = [Value]()

    /// Whether the view holds the plan's entries: every query's does, except while it refuses.
    public var incremental: Bool { held != nil }

    private init(_ name: String, _ args: [String: Value], _ ctx: Ctx, _ guards: Set<String>, _ outcome: Outcome) {
        self.name = name
        self.args = args
        self.ctx = ctx
        self.guards = guards
        self.outcome = outcome
    }

    static func open(_ peer: Peer, _ name: String, _ args: [String: Value]) throws -> ClientView {
        let c = try closure(peer, name)
        var guards = Set(Reads.reads(c.function))
        for u in c.function.uses {
            // A scope runs nothing on a device (`docs/plan-guards.md` D2):
            // what it reads decides what the authority serves, not whether
            // this view's middleware refuses.
            if let h = c.helpers.first(where: { $0.name == u && $0.kind != .scope }) { guards.formUnion(Reads.reads(h)) }
        }
        let o = try middleware(peer, name, c, args)
        if case .refused(let r) = o { throw Refused(r) }
        let v = ClientView(name, args, peer.ctx, guards, o)
        try v.hydrate(peer, c)
        return v
    }

    /// Run the middleware again and hydrate whole.
    public func rehydrate(_ peer: Peer) throws {
        let c = try ClientView.closure(peer, name)
        ctx = peer.ctx
        outcome = try ClientView.middleware(peer, name, c, args)
        try hydrate(peer, c)
    }

    private func hydrate(_ peer: Peer, _ c: Closure) throws {
        guard case .ran(let a, let p) = outcome else {
            held = nil
            rows = []
            return
        }
        guard let plan = c.function.plan else { throw PeerError("\(name): a query with no plan") }
        let env = ViewEnv(c.helpers, ctx, a, p)
        let v: MaintainedView
        do {
            v = try Views.hydrate(peer.storeSchema, plan, env, peer.store)
        } catch let e as EvalFault {
            throw ClientView.fault(name, e)
        }
        rows = v.rows()
        held = v
    }

    /// Bring the list up to date with what moved: `changes` is what
    /// `Peer.takeChanges` returned, handed to every view in turn.
    public func update(_ peer: Peer, _ changes: Changes) throws -> Update {
        let chs: [Change]
        switch changes {
        case .rebuilt:
            try rehydrate(peer)
            return .reset
        case .applied(let cs): chs = cs
        }
        // §1.7 The middleware first: whoever is signed in, and whatever the
        // tables it reads now say.
        if peer.ctx != ctx || chs.contains(where: { guards.contains($0.table) }) {
            let c = try ClientView.closure(peer, name)
            let o = try ClientView.middleware(peer, name, c, args)
            if peer.ctx != ctx || o != outcome {
                ctx = peer.ctx
                outcome = o
                try hydrate(peer, c)
                return .reset
            }
        }
        guard let v = held, !chs.isEmpty else { return .unchanged }
        let patches: [Patch]
        do {
            patches = try Views.pushAll(peer.storeSchema, peer.store, chs, v)
        } catch is EvalFault {
            // A fault leaves the engine's view as it was, and stale: read it
            // again, and say so.
            try rehydrate(peer)
            return .reset
        }
        if patches.isEmpty { return .unchanged }
        rows = Views.splice(patches, rows)
        return .patched(patches)
    }

    /// Apply patches in order to a list as it stood (`view::splice`): what a
    /// screen holding its own list of the rows — mapped to its own type —
    /// does with `Update.patched`.
    public static func splice<T>(_ xs: inout [T], _ ps: [Patch], _ of: (Value) -> T) {
        for p in ps {
            switch p {
            case .insert(let at, let node): xs.insert(of(node), at: at)
            case .remove(let at): xs.remove(at: at)
            case .update(let at, let node): xs[at] = of(node)
            }
        }
    }

    private static func closure(_ peer: Peer, _ name: String) throws -> Closure {
        let (h, _) = try peer.domain.query(name)
        return peer.domain.closures[Hex.encode(h)]!
    }

    // The middleware over the peer's store, as the peer's user: a verdict is
    // an outcome, a bug is an error.
    private static func middleware(_ peer: Peer, _ name: String, _ c: Closure, _ args: [String: Value]) throws -> Outcome {
        do {
            let (a, p) = try Eval.middleware(peer.storeSchema, c, peer.ctx, args, peer.store)
            return .ran(a, p)
        } catch EvalFault.verdict(let r) {
            return .refused(r)
        } catch EvalFault.bug(let e) {
            throw PeerError("bug: \(name): \(e)")
        }
    }

    private static func fault(_ name: String, _ e: EvalFault) -> Error {
        switch e {
        case .verdict(let r): return Refused(r)
        case .bug(let b): return PeerError("bug: \(name): \(b)")
        }
    }
}

extension Peer {
    /// `view`: a query held as a list and kept up to date (`ClientView`).
    public func view(_ name: String, _ args: [String: Value]) throws -> ClientView { try ClientView.open(self, name, args) }
}
