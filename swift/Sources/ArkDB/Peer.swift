// §11 The peer, as `rust/ark/src/peer.rs` defines it: the log machine every
// peer runs (`Replica`), and the authority role a peer takes for the log it
// sequences (`Authority`) — which a client takes only alone. Both are
// sans-io.
//
// A replica's view is always `replay(confirmed) then replay(pending)`.
// Confirmed state moves only forward; the only thing ever undone is the
// replica's own pending, replayed on top when confirmed entries land. That
// is the rebase. A replica that holds the closure an entry names replays
// the intent; one that does not asks for the facts; when it holds both it
// replays and compares, and a disagreement is recorded in `diverged` and
// resolved in the authority's favour. An intent of this replica's own is
// the exception, because it was already run here: what that run did is
// kept beside it, and confirming it applies that record rather than
// running it again (`docs/plan-perf.md` R2).
//
// One representation differs from the Rust and says nothing different. The
// Rust view shares with the confirmed store every table it has not written
// (`docs/plan-db.md` D7.3); here the view is its own store, copied from the
// confirmed store at open and at every replay — a copy on write, so it costs
// a table's rows only when that table is next written — and moved by every
// change the confirmed store moves by while it shares that state. What a
// view reports, every hash and every frame are the Rust's. The Rust's native
// procedures (`authoring::Procedure`) are an authoring vocabulary's, which a
// phone does not have: an entry is run by a raw write, by the closure its
// hash names, or not at all.

/// An entry and its facts as they wait in a replica's inbox (`peer::Inbox`).
public final class Inbox {
    public var entry: LogEntry?
    public var facts: FactList?
    public init(_ entry: LogEntry? = nil, _ facts: FactList? = nil) {
        self.entry = entry
        self.facts = facts
    }
}

/// What a view is told (`peer::Changes`): the changes to the optimistic
/// store since it last asked — every transition it made, a rebase's
/// included — or that it was replaced whole and must re-hydrate.
public enum Changes {
    /// Oldest first.
    case applied([Change])
    case rebuilt
}

/// What whoever keeps the confirmed store durable is told (§11.1,
/// `peer::Journal`): the changes it moved by per sequence since they last
/// asked, or that it was replaced and must be written whole.
public enum Moved {
    case run([(Seq, FactList)])
    case replaced
}

/// An entry run by hash (`peer::run`): its changes, its verdict, or a bug.
enum Applied {
    case ok([Change])
    case refused(Refusal)
    case bug(EvalError)
}

/// The key a closure is held under: its hash, as lowercase hex.
func hashKey(_ fh: [UInt8]) -> String { Hex.encode(fh) }

/// The entry's author as every run of it after the first sees it (`peer::ctx_of`).
func ctxOf(_ e: LogEntry) -> Ctx { Ctx(e.actor, e.session, roles: e.roles, authority: false) }

/// `bug_text`: a bug, as the Rust writes `format!("bug: {e:?}")`.
public func bugText(_ e: EvalError) -> String { "bug: " + e.description }

/// `run`: an entry's function applied by its hash — a raw write natively
/// (`docs/plan-guards.md` D4), otherwise through the closure the hash names;
/// `nil` when neither is held.
func runEntry(_ schema: Schema, _ bodies: [String: Closure], _ fh: [UInt8], _ ctx: Ctx, _ autos: [String: Value], _ args: [String: Value], _ store: Store) -> Applied? {
    let raw = Raw.of(fh)
    var c: Closure? = nil
    if raw == nil {
        guard let b = bodies[hashKey(fh)] else { return nil }
        c = b
    }
    do {
        let ran = raw != nil ? try Raw.apply(raw!, args, store) : try Eval.applyClosure(schema, c!, ctx, autos, args, store)
        switch ran {
        case .ok(let cs): return .ok(cs)
        case .refused(let r): return .refused(r)
        }
    } catch EvalFault.verdict(let r) {
        return .refused(r)
    } catch EvalFault.bug(let e) {
        return .bug(e)
    } catch {
        return .bug(.typeError("\(error)"))
    }
}

/// `invert`: a change undone, exact because a change carries the whole row
/// on each side of it (`docs/plan-perf.md` R2).
func invert(_ c: Change) -> Change {
    switch c {
    case .add(let t, let r): return .remove(t, r)
    case .remove(let t, let r): return .add(t, r)
    case .edit(let t, let o, let n): return .edit(t, n, o)
    }
}

/// `transitions`: whether `chs`, applied in order over `store`, is a
/// transition at every step — an `Add` onto an absent key, a `Remove` or an
/// `Edit` from exactly the row there. Over an overlay.
func transitions(_ schema: Schema, _ store: Store, _ chs: [Change]) -> Bool {
    let over = Overlay(store)
    for c in chs {
        guard let tbl = schema.lookupTable(c.table) else { return false }
        let row = c.new ?? c.old!
        if over.get(c.table, tbl.keyOf(row)) != c.old { return false }
        over.applyChange(c)
    }
    return true
}

/// One peer's copy of the log (`peer::Replica`). The fields are the Rust's,
/// each documented there; `view` is its own store (the file's opening note).
public final class Replica {
    public var schema: Schema
    /// The closures this peer can run, by the hash an entry names (as hex).
    public var bodies: [String: Closure]
    /// The confirmed store: the tables exactly as the authority had them at `cursor`.
    public var confirmed: MemoryStore
    public var cursor: Seq
    /// The log `confirmed` is a prefix of, once the server said (§10, §12.4).
    public var logId: Id? = nil
    /// Intents authored here that no verdict has answered, in authoring order.
    public var pending: [LogEntry]
    /// What each pending intent's run did to `view`, by its id (R2). Never durable.
    public var recorded = [Id: FactList]()
    /// The optimistic store: `confirmed` with `pending` replayed. Never durable.
    public var view: MemoryStore
    /// Confirmed entries received and not yet applied.
    public var inbox = SortedMap<Seq, Inbox>()
    /// Verdicts against this peer's own intents, newest last.
    public var rejections = [(Id, Refusal)]()
    /// Sequences at which this peer's replay disagreed with the authority's facts.
    public var diverged = [Seq]()
    public var rebuilt = true
    public var changes = [Change]()
    public var replaced = true
    /// What the confirmed store moved by since `takeConfirmed` last asked, per sequence.
    public var journal = [(Seq, FactList)]()
    /// The server runs another module than this peer's (§12 `behind`, `docs/plan-db.md` D1).
    public var behind = false
    /// The facts of pending intents this replica holds no closure for, where it knows them (D1).
    public var known = [Id: FactList]()
    /// `docs/plan-guards.md` D2 The union held: the tables held in part, with their columns.
    public var partial: SortedMap<CodePoints, [String]>? = nil
    /// A partial replica's horizon of what it has been told (`upto`).
    public var through: Seq

    init(_ schema: Schema, _ bodies: [String: Closure], _ confirmed: MemoryStore, _ cursor: Seq, _ pending: [LogEntry]) {
        self.schema = schema
        self.bodies = bodies
        self.confirmed = confirmed
        self.cursor = cursor
        self.pending = pending
        self.view = confirmed
        self.through = cursor
        replay()
    }

    /// `Replica::open`: from what was durable, the pending intents replayed on top.
    public static func open(_ schema: Schema, _ bodies: [String: Closure], _ confirmed: MemoryStore, _ cursor: Seq, _ pending: [LogEntry]) -> Replica {
        Replica(schema, bodies, confirmed, cursor, pending)
    }

    /// The union as a plain map, table to held columns.
    public var partialMap: [String: [String]]? {
        guard let p = partial else { return nil }
        var out = [String: [String]]()
        for (k, v) in p { out[k.s] = v }
        return out
    }

    private func run(_ e: LogEntry, _ store: Store) -> Applied? {
        runEntry(schema, bodies, e.fnHash, ctxOf(e), e.autos, e.args, store)
    }

    /// §11.2 `mutate`: an intent applied forward into the optimistic store
    /// and, if it is not refused, recorded as pending; the entry, or the
    /// verdict, which changes nothing.
    public func mutate(_ id: Id, _ ctx: Ctx, _ fh: [UInt8], _ autos: [String: Value], _ args: [String: Value]) -> (LogEntry?, Refusal?) {
        if let raw = Raw.of(fh) { return (nil, .forbidden(Raw.tableOf(raw, args))) }
        switch runEntry(schema, bodies, fh, ctx, autos, args, Overlay(view)) {
        case nil: return (nil, .refused(unknownFunction(fh)))
        case .bug(let e)?: return (nil, .refused(bugText(e)))
        case .refused(let r)?: return (nil, r)
        case .ok(let cs)?:
            let e = LogEntry(id, ctx.user, ctx.session, ctx.roles, fh, args, autos)
            view.applyChanges(cs)
            if recorded.count > pending.count {
                let live = Set(pending.map { $0.id })
                recorded = recorded.filter { live.contains($0.key) }
            }
            recorded[e.id] = cs
            pending.append(e)
            changes.append(contentsOf: cs)
            return (e, nil)
        }
    }

    /// `can_apply`: a closure is held for the hash, or it is a raw write.
    public func canApply(_ fh: [UInt8]) -> Bool { bodies[hashKey(fh)] != nil || Raw.isRaw(fh) }

    /// D3 `is_private`: an entry naming this hash is taken by the authority's facts.
    public func isPrivate(_ fh: [UInt8]) -> Bool { bodies[hashKey(fh)].map(Hash.isPrivate) == true }

    /// §11.3 `receive`: placed in the inbox, applied by the next `settle` (R8).
    public func receive(_ n: Seq, _ e: LogEntry) {
        if n <= cursor { return }
        inbox.modify(n, default: { Inbox() }) { $0.entry = e }
    }

    /// `receive_with`: an entry and its facts together, as a batch delivers them.
    public func receiveWith(_ n: Seq, _ e: LogEntry, _ f: FactList) {
        if n <= cursor { return }
        inbox[n] = Inbox(e, f)
    }

    /// `receive_batch`: a page handed in whole, as its own pump.
    public func receiveBatch(_ items: [(Seq, LogEntry, FactList?)]) {
        for (n, e, f) in items {
            if let f = f { receiveWith(n, e, f) } else { receive(n, e) }
        }
        settle()
    }

    /// `receive_facts`: the facts of an entry, at its sequence.
    public func receiveFacts(_ n: Seq, _ f: FactList) {
        if n <= cursor { return }
        inbox.modify(n, default: { Inbox() }) { $0.facts = f }
    }

    /// §11.4 `ack`: this peer's own intent was sequenced at `n`, placed as if
    /// it had arrived; one at or below the cursor is confirmed by the store
    /// already holding it, and dropped.
    public func ack(_ id: Id, _ n: Seq) {
        guard let at = pending.firstIndex(where: { $0.id == id }) else { return }
        if n <= cursor {
            let undo = pending[at...].map { $0.id }
            pending.remove(at: at)
            rebase(undo, [], at)
            return
        }
        receive(n, pending[at])
    }

    /// §11.2b `sign_in`: every pending intent authored as nobody becomes
    /// `who`'s, and the view is rebased by changes from the first rewritten.
    public func signIn(_ who: Ctx) {
        func nobody(_ e: LogEntry) -> Bool { e.actor.isEmpty && e.session.isEmpty }
        guard let from = pending.firstIndex(where: nobody) else { return }
        let undo = pending[from...].map { $0.id }
        for i in from..<pending.count where nobody(pending[i]) { pending[i] = pending[i].by(who.user, who.session) }
        rebase(undo, [], from)
    }

    /// §11.5 `reject`: a verdict against an intent still pending — dropped,
    /// the verdict kept, and the view rebased from it.
    public func reject(_ id: Id, _ why: Refusal) {
        guard let at = pending.firstIndex(where: { $0.id == id }) else { return }
        rejections.append((id, why))
        let undo = pending[at...].map { $0.id }
        pending.remove(at: at)
        rebase(undo, [], at)
    }

    /// `fork_back`, alone → server (`docs/plan-alone.md` §1): the local
    /// history since the fork taken back out of the confirmed store and
    /// re-queued ahead of what is pending; the view told every transition.
    public func forkBack(_ local: [FactList], _ fork: Seq, _ logId: Id?, _ requeue: [LogEntry]) {
        let order = pending.map { $0.id }
        let whole = order.allSatisfy { recorded[$0] != nil }
        for (e, f) in zip(requeue, local) where !canApply(e.fnHash) { known[e.id] = f }
        var told = [Change]()
        if whole {
            for id in order.reversed() {
                let rec = recorded.removeValue(forKey: id) ?? []
                for c in rec.reversed() {
                    let back = invert(c)
                    view.applyChange(back)
                    told.append(back)
                }
            }
        }
        for facts in local.reversed() {
            for c in facts.reversed() {
                let back = invert(c)
                confirmed.applyChange(back)
                if whole { view.applyChange(back) }
                told.append(back)
            }
        }
        cursor = fork
        self.logId = logId
        inbox.removeAll()
        journal.removeAll()
        replaced = true
        let queued = Set(requeue.map { $0.id })
        let rest = pending.filter { !queued.contains($0.id) }
        pending = requeue + rest
        if !whole { return replay() }
        told.append(contentsOf: runPending(0))
        changes.append(contentsOf: told)
    }

    /// `needs`: the sequences waiting on facts. A partial replica asks for none (D2).
    public func needs() -> [Seq] {
        if partial != nil { return [] }
        return inbox.entries.filter { n, ib in
            guard let e = ib.entry, ib.facts == nil else { return false }
            return !canApply(e.fnHash) || diverged.contains(n) || isPrivate(e.fnHash)
        }.map { $0.key }
    }

    /// §11.6 `settle`: the inbox applied, once per pump (R8).
    public func settle() { advance() }

    /// `take_changes`: what a view is told, and the slate wiped.
    public func takeChanges() -> Changes {
        let out: Changes = rebuilt ? .rebuilt : .applied(changes)
        rebuilt = false
        changes.removeAll()
        return out
    }

    /// `take_confirmed`: what the confirmed store has done since the last ask.
    public func takeConfirmed() -> Moved {
        let run = journal
        journal.removeAll()
        if replaced {
            replaced = false
            return .replaced
        }
        return .run(run)
    }

    /// `verify_at`: the cursor and the hash of the confirmed state there.
    public func verifyAt() -> (Seq, [UInt8]) { (cursor, Hash.stateHash(confirmed)) }

    // §11.6 `advance`: apply from the inbox in order for as long as the next
    // entry can be applied; then decide what the view owes.
    private func advance() {
        let next = partial != nil ? through > cursor : inbox.contains(cursor + 1)
        if !next { return }
        let order = pending.map { $0.id }
        let (acc, moved, others) = partial != nil ? confirmPartial() : confirmInbox()
        if !moved { return }
        if order.isEmpty {
            land(acc)
            changes.append(contentsOf: acc)
        } else if !others {
            for id in order[0..<(order.count - pending.count)] { recorded[id] = nil }
        } else {
            rebase(order, acc, 0)
        }
    }

    // `confirm_inbox`: the advance's first half for a whole replica.
    private func confirmInbox() -> ([Change], Bool, Bool) {
        var acc = [Change]()
        var moved = false
        var others = false
        while true {
            let n = cursor + 1
            guard let ib = inbox[n], let e = ib.entry else { break }
            let mf = ib.facts
            let ownNext = pending.first == e
            let previewed = canApply(e.fnHash)
            let priv = isPrivate(e.fnHash)
            let rec = recorded[e.id]
            var byRecord: (FactList, Bool)? = nil
            if let r = rec, ownNext, !others, previewed {
                let f = mf.map(here)
                if let ff = f, priv {
                    byRecord = (ff, false)
                } else if f == nil && priv {
                    byRecord = nil
                } else if let ff = f, ff != r {
                    byRecord = (ff, true)
                } else {
                    byRecord = (r, false)
                }
            }
            let corrected = priv && ownNext && byRecord != nil && recorded[e.id] != byRecord!.0
            guard let applied = byRecord ?? applyOne(n, e, mf) else {
                if mf == nil && canApply(e.fnHash) && !priv && !diverged.contains(n) { diverged.append(n) }
                break
            }
            let (chs, div) = applied
            confirmed.applyChanges(chs)
            journal.append((n, chs))
            cursor = n
            inbox.remove(n)
            if ownNext { pending.removeFirst() } else { pending.removeAll { $0.id == e.id } }
            known[e.id] = nil
            if div { diverged.append(n) }
            acc.append(contentsOf: chs)
            moved = true
            others = others || !ownNext || div || !previewed || corrected
        }
        return (acc, moved, others)
    }

    // `confirm_partial`: every sequence up to `through`, by its facts (D2).
    private func confirmPartial() -> ([Change], Bool, Bool) {
        var acc = [Change]()
        var moved = false
        var others = false
        while cursor < through {
            let n = cursor + 1
            let ib = inbox.remove(n) ?? Inbox()
            let chs = ib.facts ?? []
            let e = ib.entry
            let ownNext = e != nil && pending.first == e
            let agrees = ownNext && !others && canApply(e!.fnHash) && recorded[e!.id] == chs
            var dropped = false
            if let e = e {
                let before = pending.count
                pending.removeAll { $0.id == e.id }
                dropped = pending.count < before
                known[e.id] = nil
            }
            confirmed.applyChanges(chs)
            journal.append((n, chs))
            cursor = n
            moved = true
            if !agrees && (dropped || !chs.isEmpty) { others = true }
            acc.append(contentsOf: chs)
        }
        return (acc, moved, others)
    }

    // `apply_one`: one entry against the confirmed store — by intent when the
    // closure is held, by facts otherwise; both when both, compared.
    private func applyOne(_ n: Seq, _ e: LogEntry, _ mf0: FactList?) -> (FactList, Bool)? {
        let mf = mf0.map(here)
        if isPrivate(e.fnHash) { return mf.map { ($0, false) } }
        if canApply(e.fnHash) && !diverged.contains(n) {
            if case .ok(let cs)? = run(e, Overlay(confirmed)) {
                if let f = mf, f != cs { return (f, true) }
                return (cs, false)
            }
            return mf.map { ($0, true) }
        }
        return mf.map { ($0, false) }
    }

    // `here`: the authority's facts as this replica holds them (D1).
    private func here(_ f: FactList) -> FactList {
        func short(_ c: Change) -> Bool {
            guard let tbl = schema.lookupTable(c.table) else { return false }
            let rows = [c.old, c.new].compactMap { $0 }
            return rows.contains { r in r.count < tbl.columns.count && r.names.allSatisfy { tbl.column($0) != nil } }
        }
        if !behind && !f.contains(where: short) { return f }
        var out = [Change]()
        for c in f {
            if !behind && !short(c) {
                out.append(c)
                continue
            }
            switch project(schema, c) {
            case .of(let p): out.append(p)
            case .none: break
            case .refused: out.append(c)
            }
        }
        return out
    }

    // `replay`: the view rebuilt whole from the confirmed store.
    private func replay() {
        view = confirmed.copy()
        rebuilt = true
        changes.removeAll()
        recorded.removeAll()
        _ = runPending(0)
    }

    // §R2 `rebase`, by changes: the records undone newest first, what landed
    // applied, then `pending[from..]` run again.
    private func rebase(_ undo: [Id], _ landed: [Change], _ from: Int) {
        if undo.contains(where: { recorded[$0] == nil }) { return replay() }
        var told = [Change]()
        for id in undo.reversed() {
            let rec = recorded.removeValue(forKey: id) ?? []
            for c in rec.reversed() {
                let back = invert(c)
                view.applyChange(back)
                told.append(back)
            }
        }
        land(landed)
        told.append(contentsOf: landed)
        told.append(contentsOf: runPending(from))
        changes.append(contentsOf: told)
    }

    // `land`: what landed on the confirmed store, applied to the view.
    private func land(_ landed: [Change]) { view.applyChanges(landed) }

    // `run_pending`: `pending[from..]` run over the view in order, each
    // recorded; one now refused is dropped and its reason kept.
    private func runPending(_ from: Int) -> [Change] {
        let rest = Array(pending[from...])
        pending.removeSubrange(from...)
        var moved = [Change]()
        for e in rest {
            switch run(e, Overlay(view)) {
            case nil:
                let k = known[e.id]
                let chs = k != nil && transitions(schema, view, k!) ? k! : []
                view.applyChanges(chs)
                moved.append(contentsOf: chs)
                recorded[e.id] = chs
                pending.append(e)
            case .ok(let cs)?:
                view.applyChanges(cs)
                moved.append(contentsOf: cs)
                recorded[e.id] = cs
                pending.append(e)
            case .refused(let r)?: rejections.append((e.id, r))
            case .bug(let b)?: rejections.append((e.id, .refused(bugText(b))))
            }
        }
        return moved
    }
}

/// What `store::project` makes of a fact under a schema.
enum Projected {
    case of(Change)
    case none
    case refused(Refusal)
}

/// `store::project`: a fact laid out as `schema`'s table — dropped where the
/// schema lacks the table, refused where a row lacks a column the table requires.
func project(_ schema: Schema, _ c: Change) -> Projected {
    guard let tbl = schema.lookupTable(c.table) else { return .none }
    func p(_ r: Row) throws -> Row { try Rules.projectRow(tbl, r).get() }
    do {
        switch c {
        case .add(let t, let r): return .of(.add(t, try p(r)))
        case .remove(let t, let r): return .of(.remove(t, try p(r)))
        case .edit(let t, let o, let n): return .of(.edit(t, try p(o), try p(n)))
        }
    } catch let r as Refusal {
        return .refused(r)
    } catch {
        return .none
    }
}

/// The answer to a pushed intent (`peer::Sequenced`).
public enum Sequenced {
    case appended(Seq, FactList)
    case duplicate(Seq)
    case rejected(Refusal)
}

/// The peer that sequences the log (`peer::Authority`), as a client is one:
/// alone, over its own closures, a private block never run (`isPrivate`
/// false, `docs/plan-guards.md` D3) and a raw write never authored (D4).
public final class Authority {
    public let schema: Schema
    public var bodies: [String: Closure]
    public var log: Log
    /// The state at the head of the log.
    public var store: MemoryStore
    /// D3 This authority runs private blocks: a server's does, and a client's never.
    public var runsPrivate = false

    public init(_ schema: Schema, _ bodies: [String: Closure]) {
        self.schema = schema
        self.bodies = bodies
        log = Log.empty(schema)
        store = MemoryStore.empty(schema)
    }

    /// `can_apply`.
    public func canApply(_ fh: [UInt8]) -> Bool { bodies[hashKey(fh)] != nil || Raw.isRaw(fh) }

    /// D3 `is_private`.
    public func isPrivate(_ fh: [UInt8]) -> Bool { bodies[hashKey(fh)].map(Hash.isPrivate) == true }

    /// §11.7 `sequence_entry`: deduped by id, applied to the head, appended with the facts.
    public func sequenceEntry(_ e: LogEntry) -> Sequenced {
        if let n = log.seqOf(e.id) { return .duplicate(n) }
        if let raw = Raw.of(e.fnHash) { return .rejected(.forbidden(Raw.tableOf(raw, e.args))) }
        switch runEntry(schema, bodies, e.fnHash, ctxOf(e).asAuthority(runsPrivate), e.autos, e.args, Overlay(store)) {
        case nil: return .rejected(.refused(unknownFunction(e.fnHash)))
        case .bug(let b)?: return .rejected(.refused(bugText(b)))
        case .refused(let r)?: return .rejected(r)
        case .ok(let cs)?:
            let n = log.append(e, cs)
            store.applyChanges(cs)
            return .appended(n, cs)
        }
    }

    /// §R2 `append_as`: an intent this process already ran, appended with the
    /// changes that run produced, without running it again — a peer alone's
    /// authority with its replica's record.
    public func appendAs(_ e: LogEntry, _ facts: FactList) -> Sequenced {
        if let n = log.seqOf(e.id) { return .duplicate(n) }
        store.applyChanges(facts)
        return .appended(log.append(e, facts), facts)
    }
}

/// §11.9 `local_commit`: a peer that is its own authority — everything
/// pending sequenced, by the replica's record where the authority's head is
/// the replica's cursor and the intent the first pending, and every answer
/// delivered back, settled per intent.
public func localCommit(_ a: Authority, _ r: Replica) {
    for e in r.pending {
        var recorded: FactList? = nil
        if let rec = r.recorded[e.id], a.log.headSeq() == r.cursor, r.pending.first == e { recorded = rec }
        let out: Sequenced
        if !a.canApply(e.fnHash) {
            out = .rejected(.refused("no closure, and no authority to run it"))
        } else if let rec = recorded {
            out = a.appendAs(e, rec)
        } else {
            out = a.sequenceEntry(e)
        }
        switch out {
        case .appended(let n, let f):
            r.receiveFacts(n, f)
            r.ack(e.id, n)
        case .duplicate(let n): r.ack(e.id, n)
        case .rejected(let why): r.reject(e.id, why)
        }
        r.settle()
    }
}
