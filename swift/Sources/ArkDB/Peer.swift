import Foundation

/// A verdict against one of this peer's own intents.
public struct Rejection: Equatable {
    public var id: Id
    public var why: Refusal
}

/// Confirmed entries received and not yet applied.
public struct Inbox {
    public var entry: Entry?
    public var facts: Facts?
}

/// What a view is told: the changes to the optimistic store since it last
/// asked, or that it was rebuilt and must re-hydrate.
public enum Changes: Equatable {
    case applied([Change])
    case rebuilt
}

/// How a replica applies an intent through native code — generated code —
/// rather than through the interpreter. `knows` says whether the code has
/// the function a hash names; `apply` runs it — given the entry's author,
/// its autos and arguments and the store to apply against — and answers the
/// outcome, the store passed untouched (`Eval.applyBody` is how one is
/// written over a generated `apply`). A hash the code does not know falls
/// through to the closure the replica holds, so a peer generated with
/// `--only` still replays every other entry by intent. Additive: a replica
/// with no applier is exactly the spec's.
public struct Applier {
    public var knows: (FnHash) -> Bool
    public var apply: (FnHash, Ctx, Args, Args, MemoryStore) throws -> Eval.Outcome
    public init(knows: @escaping (FnHash) -> Bool, apply: @escaping (FnHash, Ctx, Args, Args, MemoryStore) throws -> Eval.Outcome) {
        self.knows = knows; self.apply = apply
    }
}

/// §11 One peer's copy of one scope: a confirmed store at a cursor, the
/// pending intents, and the optimistic view they produce on top. The view
/// is always `replay(confirmed) then replay(pending)`.
///
/// A value type, as in the spec. The stores it holds are treated as
/// immutable: every apply produces a new one, so a copy of a `Replica` is
/// independent of the original.
public struct Replica {
    public var scope: ScopeName
    public var schema: Schema
    /// The closures this peer can run, keyed by the hash an entry names.
    public var bodies: [FnHash: Closure]
    /// The confirmed store: the scope exactly as the authority had it at `cursor`.
    public private(set) var confirmed: MemoryStore
    public private(set) var cursor: Seq
    /// Intents authored here that no verdict has answered, in authoring order.
    public private(set) var pending: [Entry]
    /// The optimistic store: `confirmed` with `pending` replayed.
    public private(set) var view: MemoryStore
    public private(set) var inbox: [Seq: Inbox]
    public private(set) var rejections: [Rejection]
    /// Sequences at which this peer's replay disagreed with the authority.
    public private(set) var diverged: [Seq]
    var rebuilt: Bool
    var changes: [Change] // oldest first
    /// Native code to apply intents through, consulted before `bodies`.
    public var applier: Applier? = nil

    /// §11.1 Open a replica from what was durable; pending replays on top.
    public static func open(_ schema: Schema, _ scope: ScopeName, _ bodies: [FnHash: Closure], _ confirmed: MemoryStore, _ cursor: Seq, _ pending: [Entry], applier: Applier? = nil) -> Replica {
        var r = Replica(scope: scope, schema: schema, bodies: bodies, confirmed: confirmed, cursor: cursor, pending: pending,
                        view: confirmed, inbox: [:], rejections: [], diverged: [], rebuilt: true, changes: [], applier: applier)
        r.replay()
        return r
    }

    /// Whether this replica can apply an entry naming this function, by
    /// native code or by a held closure.
    public func knows(_ fh: FnHash) -> Bool { return canApply(fh) }

    /// Apply a function to a store: through the applier when it knows the
    /// hash, else through the closure held for it; `nil` when neither does.
    func applyFunction(_ fh: FnHash, _ ctx: Ctx, _ autos: Args, _ args: Args, _ st: MemoryStore) throws -> Eval.Outcome? {
        if let a = applier, a.knows(fh) { return try a.apply(fh, ctx, autos, args, st) }
        guard let c = bodies[fh] else { return nil }
        return try Eval.applyClosure(schema, c, ctx, autos, args, st)
    }

    /// Whether either the applier or a held closure can run this function.
    func canApply(_ fh: FnHash) -> Bool {
        return bodies[fh] != nil || (applier?.knows(fh) ?? false)
    }

    /// §11.2 Author an intent: apply it forward into the view, and if it is
    /// not refused, record it as pending. A refusal changes nothing.
    public mutating func mutate(_ id: Id, _ ctx: Ctx, _ fh: FnHash, _ autos: Args, _ args: Args) -> Result<Entry, Refusal> {
        return author(id, ctx, fh, autos, args) { r, st in try r.applyFunction(fh, ctx, autos, args, st) }
    }

    /// §11.2 with the body supplied: the same authoring, applied by native
    /// code the caller hands over (a generated mutator run through
    /// `Eval.applyBody`) rather than by what the replica holds.
    public mutating func mutateWith(_ id: Id, _ ctx: Ctx, _ fh: FnHash, _ autos: Args, _ args: Args, _ apply: (MemoryStore) throws -> Eval.Outcome) -> Result<Entry, Refusal> {
        return author(id, ctx, fh, autos, args) { _, st in try apply(st) }
    }

    mutating func author(_ id: Id, _ ctx: Ctx, _ fh: FnHash, _ autos: Args, _ args: Args, _ run: (Replica, MemoryStore) throws -> Eval.Outcome?) -> Result<Entry, Refusal> {
        do {
            guard let outcome = try run(self, view) else { return .failure(.refused("unknown function " + Hex.encode(fh))) }
            switch outcome {
            case .refused(let why): return .failure(why)
            case .applied(let v, let chs):
                let e = Entry(id: id, actor: ctx.user, session: ctx.session, fn: fh, args: args, autos: autos)
                view = v
                pending.append(e)
                changes.append(contentsOf: chs)
                return .success(e)
            }
        } catch {
            return .failure(.refused("bug: \(error)"))
        }
    }

    /// §11.3 A confirmed entry arrives, at its sequence.
    public mutating func receive(_ n: Seq, _ e: Entry) {
        if n <= cursor { return }
        var ib = inbox[n] ?? Inbox(entry: nil, facts: nil)
        ib.entry = e
        inbox[n] = ib
        advance()
    }

    /// An entry and its facts arrive together.
    public mutating func receiveWith(_ n: Seq, _ e: Entry, _ f: Facts) {
        if n <= cursor { return }
        inbox[n] = Inbox(entry: e, facts: f)
        advance()
    }

    /// The facts of an entry arrive, at its sequence.
    public mutating func receiveFacts(_ n: Seq, _ f: Facts) {
        if n <= cursor { return }
        var ib = inbox[n] ?? Inbox(entry: nil, facts: nil)
        ib.facts = f
        inbox[n] = ib
        advance()
    }

    /// §11.4 This peer's own intent was sequenced at `n`.
    public mutating func ack(_ i: Id, _ n: Seq) {
        guard let e = pending.first(where: { $0.id == i }) else { return }
        receive(n, e)
    }

    /// §11.5 A verdict against this peer's own intent: dropped, kept for
    /// the app to show, and the view rebuilt without it.
    public mutating func reject(_ i: Id, _ why: Refusal) {
        pending.removeAll { $0.id == i }
        rejections.append(Rejection(id: i, why: why))
        replay()
    }

    /// The sequences the replica is waiting on facts for.
    public var needs: [Seq] {
        return inbox.keys.sorted().filter { n in
            guard let e = inbox[n]?.entry, inbox[n]?.facts == nil else { return false }
            return !canApply(e.fn) || diverged.contains(n)
        }
    }

    /// Try the inbox again.
    public mutating func retry() { advance() }

    /// What a view is told, and the slate wiped.
    public mutating func takeChanges() -> Changes {
        let r: Changes = rebuilt ? .rebuilt : .applied(changes)
        rebuilt = false
        changes = []
        return r
    }

    /// This replica's claim: its cursor and the hash of its confirmed state.
    public func verifyAt() -> (Seq, [UInt8]) {
        return (cursor, Hash.stateHash(confirmed))
    }

    // MARK: §11.6 advancing

    mutating func advance() {
        let pendingBefore = pending
        var acc: [Change] = []
        var moved = false
        var others = false
        while true {
            let n = cursor + 1
            guard let ib = inbox[n], let e = ib.entry, let (st2, chs, div) = applyOne(n, e, ib.facts) else { break }
            let ownNext = pending.first.map { $0.id == e.id } ?? false
            confirmed = st2
            cursor = n
            inbox[n] = nil
            pending.removeAll { $0.id == e.id }
            if div { diverged.append(n) }
            acc.append(contentsOf: chs)
            moved = true
            others = others || !ownNext || div
        }
        if !moved { return }
        if pendingBefore.isEmpty {
            view = confirmed
            changes.append(contentsOf: acc)
        } else if !others {
            if pending.isEmpty { view = confirmed }
        } else {
            replay()
        }
    }

    /// One entry against the confirmed store: by intent when the closure is
    /// held, by facts otherwise; both when both are present, comparing them.
    func applyOne(_ n: Seq, _ e: Entry, _ mf: Facts?) -> (MemoryStore, [Change], Bool)? {
        if canApply(e.fn), !diverged.contains(n) {
            // A bug thrown here (`nil`) is treated as the closure disagreeing.
            let outcome = (try? applyFunction(e.fn, Ctx(user: e.actor, session: e.session), e.autos, e.args, confirmed)) ?? nil
            if case .applied(let st2, let chs)? = outcome {
                if let f = mf, f != chs { return (confirmed.applying(f), f, true) }
                return (st2, chs, false)
            }
            if let f = mf { return (confirmed.applying(f), f, true) }
            return nil
        }
        if let f = mf { return (confirmed.applying(f), f, false) }
        return nil
    }

    /// Rebuild the view: the confirmed store, then every pending intent in
    /// order; one that is now refused is dropped and recorded.
    mutating func replay() {
        view = confirmed
        rebuilt = true
        changes = []
        var kept: [Entry] = []
        for e in pending {
            do {
                guard let outcome = try applyFunction(e.fn, Ctx(user: e.actor, session: e.session), e.autos, e.args, view) else {
                    rejections.append(Rejection(id: e.id, why: .refused("no closure for a pending intent")))
                    continue
                }
                switch outcome {
                case .applied(let v, _):
                    view = v
                    kept.append(e)
                case .refused(let why):
                    rejections.append(Rejection(id: e.id, why: why))
                }
            } catch {
                rejections.append(Rejection(id: e.id, why: .refused("bug: \(error)")))
            }
        }
        pending = kept
    }
}

// MARK: - An authority

/// The answer to a pushed intent.
public enum Sequenced {
    case appended(Seq, Facts)
    case duplicate(Seq)
    case rejected(Refusal)
}

/// The peer that sequences a scope.
public struct Authority {
    public var scope: ScopeName
    public var schema: Schema
    /// Every closure ever accepted for this scope, by hash.
    public var bodies: [FnHash: Closure]
    public private(set) var log: Log
    /// The state at the head of the log.
    public private(set) var store: MemoryStore

    public init(_ schema: Schema, _ scope: ScopeName, _ bodies: [FnHash: Closure]) {
        self.schema = schema
        self.scope = scope
        self.bodies = bodies
        self.log = Log(schema: schema)
        self.store = MemoryStore(schema: schema)
    }

    /// §3.10 An authority resuming from a snapshot: the log stands on it —
    /// its sequence is the horizon and the head, so the next entry is
    /// `seq + 1` — and the state at the head is the snapshot's rows. This is
    /// how a peer that is its own authority reopens without replaying from
    /// zero: the snapshot is the replica's confirmed store at its cursor.
    public init(_ schema: Schema, _ scope: ScopeName, _ bodies: [FnHash: Closure], from snapshot: Snapshot) {
        self.schema = schema
        self.scope = scope
        self.bodies = bodies
        var l = Log(schema: schema)
        l.base = snapshot
        self.log = l
        self.store = snapshot.store.clone()
    }

    /// §11.7 Sequence an intent: dedupe by id, apply to the head state, and
    /// append with the facts.
    public mutating func sequenceEntry(_ e: Entry) -> Sequenced {
        if let n = log.seqOf(e.id) { return .duplicate(n) }
        guard let c = bodies[e.fn] else { return .rejected(.refused("unknown function " + Hex.encode(e.fn))) }
        do {
            switch try Eval.applyClosure(schema, c, Ctx(user: e.actor, session: e.session), e.autos, e.args, store) {
            case .refused(let why): return .rejected(why)
            case .applied(let st2, let facts):
                let n = log.append(e, facts)
                store = st2
                return .appended(n, facts)
            }
        } catch {
            return .rejected(.refused("bug: \(error)"))
        }
    }

    /// What a peer at a cursor is sent.
    public func page(_ cursor: Seq, _ limit: Int) -> Page {
        return log.entriesAfter(cursor, limit)
    }

    /// Move the horizon; false if the sequence is not retained.
    public mutating func compact(_ n: Seq) -> Bool {
        return log.compactTo(n)
    }

    /// Drop every closure that neither the current module nor a retained
    /// entry names.
    public mutating func retire(current: Set<FnHash>) {
        let keep = current.union(log.namedHashes)
        bodies = bodies.filter { keep.contains($0.key) }
    }
}

/// §11.9 A peer that is its own authority: everything pending is
/// sequenced, and every answer delivered back, in order.
public func localCommit(_ a: inout Authority, _ r: inout Replica) {
    for e in r.pending {
        switch a.sequenceEntry(e) {
        case .appended(let n, let facts):
            r.receiveFacts(n, facts)
            r.ack(e.id, n)
        case .duplicate(let n):
            r.ack(e.id, n)
        case .rejected(let why):
            r.reject(e.id, why)
        }
    }
}
