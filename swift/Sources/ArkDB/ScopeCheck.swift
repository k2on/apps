// `docs/plan-guards.md` D2 What a person holds — the union of a module's
// scopes for one identity — as far as the verifier asks it
// (`rust/ark/src/scope.rs`): the scopes gathered by table, a filter folded
// under a context, the union's columns, and `check`, which holds what a
// client runs to what its person holds for every role set the module
// names; and the one piece of the client's half a client needs, the
// device's schema (`deviceSchema`). What a server does with a union —
// admitting rows, projecting them, filtering facts — is the server's.

// The context a filter is folded under: everything a person is known by, or
// — for the verifier, which asks about every user at once — no user and no login.
struct FoldCtx {
    let user: String?
    let session: String?
    let roles: Set<String>
}

// A filter folded under a context: decided for every row, a predicate over
// the row's columns, or — where the context does not say — unknown.
private enum F {
    case yes
    case no
    case rows(Pred)
    case unknown
}

/// What one person holds of one table: the rows (`nil`: every row) and the columns (`nil`: all).
public struct Held: Hashable {
    public let rows: [Pred]?
    public let columns: [String]?
    public var isWhole: Bool { rows == nil && columns == nil }
}

/// `scope::Scopes`: every scope a module's procedures run, gathered by the table each hold names.
public struct Scopes {
    public let schema: Schema
    public let holds: SortedMap<CodePoints, [Hold]>

    /// Whether the module has any scope at all.
    public var isEmpty: Bool { holds.isEmpty }

    /// The role names the scopes test, in order.
    public func roles() -> SortedSet<CodePoints> {
        var out = SortedSet<CodePoints>()
        for (_, hs) in holds { for h in hs { if let f = h.filter { ScopeCheck.predRoles(f, &out) } } }
        return out
    }

    /// `Scopes::of`: the scopes some procedure lists in its `uses`, each once.
    public static func of(_ m: Module) -> Scopes {
        let used = Set(m.functions.filter { $0.kind.isProcedure }.flatMap { $0.uses })
        var holds = SortedMap<CodePoints, [Hold]>()
        for f in m.functions where f.kind == .scope && used.contains(f.name) {
            for h in f.holds { holds.modify(CodePoints(h.table), default: { [] }) { $0.append(h) } }
        }
        return Scopes(schema: m.schema, holds: holds)
    }
}

public enum ScopeCheck {
    // A context expression, folded: a value where the context decides it.
    static func value(_ e: Expr, _ ctx: FoldCtx) -> Value? {
        switch e {
        case .lit(let v): return v
        case .ctxUser: return ctx.user.map { .text($0) }
        case .ctxSession: return ctx.session.map { .text($0) }
        case .hasRole(let r): return .bool(ctx.roles.contains(r))
        case .op(let o, let args):
            switch o {
            case .not:
                guard args.count == 1, let v = value(args[0], ctx) else { return nil }
                return .bool(v == .bool(false))
            case .and:
                let vs = args.map { value($0, ctx) }
                if vs.contains(where: { $0 == .bool(false) }) { return .bool(false) }
                if vs.allSatisfy({ $0 == .bool(true) }) { return .bool(true) }
                return nil
            case .or:
                let vs = args.map { value($0, ctx) }
                if vs.contains(where: { $0 == .bool(true) }) { return .bool(true) }
                if vs.allSatisfy({ $0 == .bool(false) }) { return .bool(false) }
                return nil
            default: return nil
            }
        case .cmp(let o, let l, let r):
            guard let a = value(l, ctx), let b = value(r, ctx) else { return nil }
            return .bool(cmp(o, a, b))
        default: return nil
        }
    }

    private static func fold(_ p: Pred, _ ctx: FoldCtx) -> F {
        // A comparison of a column is over rows whatever the context.
        func rhs(_ e: Expr) -> Expr { value(e, ctx).map { .lit($0) } ?? e }
        switch p {
        case .cmp(let c, let o, let e): return .rows(.cmp(c, o, rhs(e)))
        case .inP(let c, let xs): return .rows(.inP(c, xs.map(rhs)))
        case .has(let c, let e): return .rows(.has(c, rhs(e)))
        case .when(let e):
            let v = value(e, ctx)
            if v == .bool(true) { return .yes }
            if v == nil { return .unknown }
            return .no
        case .all(let qs):
            var rows = [Pred]()
            var unknown = false
            for q in qs {
                switch fold(q, ctx) {
                case .no: return .no
                case .yes: break
                case .rows(let r): rows.append(r)
                case .unknown: unknown = true
                }
            }
            if unknown { return .unknown }
            if rows.isEmpty { return .yes }
            return rows.count == 1 ? .rows(rows[0]) : .rows(.all(rows))
        case .any(let qs):
            var rows = [Pred]()
            var unknown = false
            for q in qs {
                switch fold(q, ctx) {
                case .yes: return .yes
                case .no: break
                case .rows(let r): rows.append(r)
                case .unknown: unknown = true
                }
            }
            // An unknown alternative beside known rows: the rows are a lower bound.
            if rows.isEmpty && unknown { return .unknown }
            if rows.isEmpty { return .no }
            return rows.count == 1 ? .rows(rows[0]) : .rows(.any(rows))
        case .not(let q):
            switch fold(q, ctx) {
            case .yes: return .no
            case .no: return .yes
            case .rows(let r): return .rows(.not(r))
            case .unknown: return .unknown
            }
        case .exists(let t, let c, let q):
            switch fold(q, ctx) {
            case .no: return .no
            case .yes: return .rows(.exists(t, c, .all([])))
            case .rows(let r): return .rows(.exists(t, c, r))
            case .unknown: return .unknown
            }
        }
    }

    // `union`: the holds naming one table under a context — the rows of every
    // contributing hold, and their columns.
    static func union(_ t: Table, _ hs: [Hold], _ ctx: FoldCtx) -> Held {
        var rows: [Pred]? = []
        var cols = Set<String>()
        var allCols = false
        var any = false
        for h in hs {
            let f = h.filter.map { fold($0, ctx) } ?? .yes
            switch f {
            case .no, .unknown: continue
            case .yes: rows = nil
            case .rows(let p): rows?.append(p)
            }
            any = true
            if case .all = h.columns {
                allCols = true
            } else {
                for c in t.columns where h.columns.keeps(c.name) { cols.insert(c.name) }
            }
        }
        // Nothing contributes: no row, so every column — there is none to show.
        if !any || allCols { return Held(rows: rows, columns: nil) }
        for k in t.key { cols.insert(k) }
        let columns = t.columns.map { $0.name }.filter { cols.contains($0) }
        return Held(rows: rows, columns: columns.count < t.columns.count ? columns : nil)
    }

    static func predRoles(_ p: Pred, _ out: inout SortedSet<CodePoints>) {
        switch p {
        case .when(let e), .cmp(_, _, let e), .has(_, let e): exprRoles(e, &out)
        case .inP(_, let xs): for x in xs { exprRoles(x, &out) }
        case .all(let ps), .any(let ps): for q in ps { predRoles(q, &out) }
        case .not(let q), .exists(_, _, let q): predRoles(q, &out)
        }
    }

    private static func exprRoles(_ e: Expr, _ out: inout SortedSet<CodePoints>) {
        switch e {
        case .hasRole(let r): out.insert(CodePoints(r))
        case .op(_, let args): for x in args { exprRoles(x, &out) }
        case .cmp(_, let l, let r):
            exprRoles(l, &out)
            exprRoles(r, &out)
        default: break
        }
    }

    // Role sets at most this many names make: past it, each role alone and none at all.
    private static let powersetUpTo = 10

    /// `scope::check`: for every role set the module names, the columns each
    /// client-run part of each procedure names must be held — or
    /// `Complaint.notHeld` — unless a guard of the procedure refuses on every
    /// path for that set.
    static func check(_ m: Module, _ named: [String: Set<TableColumn>]) -> [VerifyError] {
        let scopes = Scopes.of(m)
        if scopes.isEmpty { return [] }
        var nameSet = scopes.roles()
        for f in m.functions where f.kind == .guardFn { stmtsRoles(f.body, &nameSet) }
        let names = nameSet.items.map { $0.s }
        var sets = [[String]]()
        if names.count <= powersetUpTo {
            for bits in 0..<(1 << names.count) {
                sets.append(names.enumerated().filter { bits & (1 << $0.offset) != 0 }.map { $0.element })
            }
        } else {
            sets = [[]] + names.map { [$0] }
        }
        let unions = sets.map { roles -> [String: Set<String>?] in
            let ctx = FoldCtx(user: nil, session: nil, roles: Set(roles))
            var out = [String: Set<String>?]()
            for t in m.schema.tables where out[t.name] == nil {
                if let hs = scopes.holds[CodePoints(t.name)] {
                    out[t.name] = .some(union(t, hs, ctx).columns.map { Set($0) })
                } else {
                    out[t.name] = .some(nil)
                }
            }
            return out
        }
        var out = [VerifyError]()
        for p in m.functions where p.kind.isProcedure {
            // What a client runs is the stripped procedure (D3).
            let c = Hash.closure(m, strip(p))
            let parts = ([c.function] + c.helpers).filter { $0.kind != .scope }
            let guards = p.uses.compactMap { m.lookupFunction($0) }.filter { $0.kind == .guardFn }
            var wanted = SortedSet<TableColumn>()
            for f in parts { if let n = named[f.name] { wanted.formUnion(n) } }
            var reported = Set<TableColumn>()
            for (roles, held) in zip(sets, unions) {
                if guards.contains(where: { refuses($0.body, Set(roles)) }) { continue }
                for tc in wanted {
                    // A table no scope holds is held whole; so is one whose holds keep every column.
                    var ok = true
                    if let entry = held[tc.table], let cols = entry { ok = cols.contains(tc.column) }
                    if !ok && reported.insert(tc).inserted {
                        out.append(.inFunction(p.name, .notHeld(tc.table, tc.column, roles)))
                    }
                }
            }
        }
        return out
    }

    private static func stmtsRoles(_ b: Block, _ out: inout SortedSet<CodePoints>) {
        for s in b {
            switch s {
            case .ifS(let c, let a, let e):
                exprRoles(c, &out)
                stmtsRoles(a, &out)
                stmtsRoles(e, &out)
            case .letS(_, let e), .refuse(let e): exprRoles(e, &out)
            default: break
            }
        }
    }

    // Whether a guard's body refuses on every path once its `has_role`s are folded for `roles`.
    private static func refuses(_ b: Block, _ roles: Set<String>) -> Bool {
        let ctx = FoldCtx(user: nil, session: nil, roles: roles)
        for s in b {
            switch s {
            case .refuse: return true
            case .ifS(let c, let a, let e):
                let v = value(c, ctx)
                if v == .bool(true) {
                    if refuses(a, roles) { return true }
                } else if v == .bool(false) {
                    if refuses(e, roles) { return true }
                } else if v == nil {
                    if refuses(a, roles) && refuses(e, roles) { return true }
                }
            case .ret: return false
            default: break
            }
        }
        return false
    }
}

/// `scope::device_schema`: the schema a device has, from the module's and
/// the tables held in part with their columns (what a partial snapshot says)
/// — each such table narrowed to its held columns, its indexes and text
/// indexes to those over them, and every table's references to those whose
/// column is held and whose parent is held whole: a device never refuses a
/// write for a parent it cannot see, and the authority checks every reference.
public func deviceSchema(_ full: Schema, _ held: [String: [String]]) -> Schema {
    if held.isEmpty { return full }
    func whole(_ t: String) -> Bool { held[t] == nil }
    return Schema(full.tables.map { t in
        let cols = held[t.name]
        func keeps(_ c: String) -> Bool { cols == nil || cols!.contains(c) }
        let narrowed = cols != nil && cols!.count < t.columns.count
        let refsMove = t.refs.contains { !whole($0.table) }
        if !narrowed && !refsMove { return t }
        return Table(t.name, t.columns.filter { keeps($0.name) }, t.key, t.indexes.filter { $0.columns.allSatisfy(keeps) }, t.refs.filter { keeps($0.column) && whole($0.table) }, t.text.filter(keeps))
    })
}
