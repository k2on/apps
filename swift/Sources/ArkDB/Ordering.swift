// §3.12 Which transitions need an order (`rust/ark/src/ir/ordering.rs`,
// `docs/plan-live.md`, "Ordered and ephemeral fields"), by static
// inspection, as `Reads` says what a query reads.
//
// - **ephemeral**: it reads nothing of the state, nothing of the context but
//   `ctx.device`, and writes only the caller's own slot of ephemeral fields
//   (`state.f[ctx.device]`). The server relays it with no sequence and no
//   hash, and the newest value from a device wins.
// - **ordered**: every other one, sequenced per instance.
//
// And one rule binds the ordered kind: it reads no ephemeral field. It may
// write the caller's own slot and pass the rest through. Hooks are ordered.
//
// "Writes only" and "passes through" are read off the shape of what the body
// returns: the state itself, or a struct of every field of the state's
// record, each `state.f` — or, for an ephemeral `f`,
// `set_slot(state.f, ctx.device, v)` — through any `if` whose condition is
// allowed where it stands.

/// The two kinds of transition (`ordering::Kind`), named as a vector names them.
public enum Kind: String, Hashable {
    /// Sequenced per instance; the server's order is every replica's.
    case ordered
    /// Relayed with no order: writes the caller's own ephemeral slots only.
    case ephemeral
}

public enum Ordering {
    /// `classify`: the kind of a declared transition of a state whose resolved
    /// type is `state`, ephemeral markers kept. A hook is always ordered.
    public static func classify(_ state: Ty, _ t: Transition) -> Kind {
        let eph = Types.ephemeralFields(state)
        guard case .structure(let fields) = state, !eph.isEmpty, !isHook(t.name) else { return .ordered }
        let names = fields.names
        let checksQuiet = t.input.flatMap { $0.value.checks }.allSatisfy { c in
            if case .refine(let e, _) = c { return quiet(e) }
            return true
        }
        var wrote = false
        let ok = checksQuiet && t.guards.allSatisfy { quiet($0.e) } && blockEphemeral(t.body, names, eph, &wrote) && wrote
        return ok ? .ephemeral : .ordered
    }

    // Reads nothing of the state and nothing of the context but the device.
    private static func quiet(_ e: Expr) -> Bool {
        !mentions(e) { x in
            switch x {
            case .liveState, .ctxUser, .ctxSession, .ctxNow, .hasRole: return true
            default: return false
            }
        }
    }

    private static func blockEphemeral(_ b: Block, _ names: [String], _ eph: [String], _ wrote: inout Bool) -> Bool {
        for s in b {
            let ok: Bool
            switch s {
            case .ret(let e?): ok = returnsOwnSlots(e, names, eph, &wrote)
            case .ifS(let c, let a, let e): ok = quiet(c) && blockEphemeral(a, names, eph, &wrote) && blockEphemeral(e, names, eph, &wrote)
            case .forS(_, let xs, let body): ok = quiet(xs) && blockEphemeral(body, names, eph, &wrote)
            case .letS(_, let e), .refuse(let e): ok = quiet(e)
            default: ok = false
            }
            if !ok { return false }
        }
        return true
    }

    // What an ephemeral transition may return: the state unchanged, or every
    // field passed through but ephemeral ones written at the caller's slot,
    // through any `if` that reads nothing it may not.
    private static func returnsOwnSlots(_ e: Expr, _ names: [String], _ eph: [String], _ wrote: inout Bool) -> Bool {
        switch e {
        case .liveState: return true
        case .ife(let c, let a, let b):
            return quiet(c) && returnsOwnSlots(a, names, eph, &wrote) && returnsOwnSlots(b, names, eph, &wrote)
        case .structE(let fs):
            guard sameNames(fs.names, names) else { return false }
            for (k, v) in zip(fs.names, fs.exprs) {
                if passedThrough(k, v) { continue }
                guard let x = ownSlotWrite(k, v), eph.contains(k), quiet(x) else { return false }
                wrote = true
            }
            return true
        default: return false
        }
    }

    // Two sorted lists of field names, the same by code point.
    private static func sameNames(_ a: [String], _ b: [String]) -> Bool {
        a.count == b.count && zip(a, b).allSatisfy { textEquals($0.0, $0.1) }
    }

    // `state.k`, as `k` itself.
    private static func passedThrough(_ k: String, _ v: Expr) -> Bool {
        if case .field(.liveState, let f) = v { return f == k }
        return false
    }

    // `set_slot(state.k, ctx.device, x)`: the value written, `x`.
    private static func ownSlotWrite(_ k: String, _ v: Expr) -> Expr? {
        if case .setSlot(let m, .ctxDevice, let x) = v, passedThrough(k, m) { return x }
        return nil
    }

    /// `ordered_reads_ephemeral`: the first ephemeral field an ordered
    /// transition reads, by name, or `"the whole state"` where it uses the
    /// state as a value; nil where it reads none.
    public static func orderedReadsEphemeral(_ state: Ty, _ t: Transition) -> String? {
        let eph = Types.ephemeralFields(state)
        guard !eph.isEmpty, case .structure(let fields) = state else { return nil }
        for i in t.input {
            for c in i.value.checks {
                if case .refine(let e, _) = c, let r = reads(e, eph) { return r }
            }
        }
        for g in t.guards {
            if let r = reads(g.e, eph) { return r }
        }
        return blockReads(t.body, fields.names, eph)
    }

    private static func blockReads(_ b: Block, _ names: [String], _ eph: [String]) -> String? {
        for s in b {
            let r: String?
            switch s {
            case .ret(let e?): r = returnedReads(e, names, eph)
            case .ifS(let c, let a, let e): r = reads(c, eph) ?? blockReads(a, names, eph) ?? blockReads(e, names, eph)
            case .forS(_, let xs, let body): r = reads(xs, eph) ?? blockReads(body, names, eph)
            case .letS(_, let e), .refuse(let e): r = reads(e, eph)
            default: r = nil
            }
            if let r = r { return r }
        }
        return nil
    }

    // A returned value: the state itself is a pass-through; a struct of every
    // field may pass an ephemeral field through or write the caller's slot of
    // it; anything else is read as any expression is.
    private static func returnedReads(_ e: Expr, _ names: [String], _ eph: [String]) -> String? {
        switch e {
        case .liveState: return nil
        case .ife(let c, let a, let b): return reads(c, eph) ?? returnedReads(a, names, eph) ?? returnedReads(b, names, eph)
        case .structE(let fs) where sameNames(fs.names, names):
            for (k, v) in zip(fs.names, fs.exprs) {
                if eph.contains(k) {
                    if passedThrough(k, v) { continue }
                    if let x = ownSlotWrite(k, v) {
                        if let r = reads(x, eph) { return r }
                        continue
                    }
                }
                if let r = reads(v, eph) { return r }
            }
            return nil
        default: return reads(e, eph)
        }
    }

    // The first ephemeral field `e` reads: `state.f` for an ephemeral `f`, or
    // the state used whole.
    private static func reads(_ e: Expr, _ eph: [String]) -> String? {
        switch e {
        case .field(.liveState, let f): return eph.contains(f) ? f : nil
        case .liveState: return "the whole state"
        default:
            for c in children(e) {
                if let r = reads(c, eph) { return r }
            }
            return nil
        }
    }

    // Whether any subexpression of `e` is one `hit` answers for.
    private static func mentions(_ e: Expr, _ hit: (Expr) -> Bool) -> Bool {
        hit(e) || children(e).contains { mentions($0, hit) }
    }
}
