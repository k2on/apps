// §1.5 A plan kept up to date (`rust/ark/src/view.rs`, "The view",
// "Maintenance" and R9 "The numbers, kept"): `hydrate` pulls it once and
// keeps the entries, indexed by key and by `(node, dependency)`; `pushAll`
// is told the changes of one settle — the store already after all of them
// — rebuilds the entries they touch, once each, and reports what it did to
// the answer as positions (`Patch`). The contract: after any sequence of
// changes the view is a fresh hydrate over the same store, and splicing
// the patches into the old answer gives the new one.
//
// R9 (`docs/plan-perf.md`, `docs/plan-db.md` D4): a related list, or a
// group's members, whose every use in its parent is an aggregate — `len`,
// a sum's `fold`, `min`/`max` of a column, `first`/`last` of a list ordered
// by one column — is kept as those numbers, moved by each change's terms,
// and never pulled whole: a count and a sum as `i128`s (`Wide`; a sum is
// judged on its total, §13), an extreme as the column's value, read again
// through an index when it leaves.

/// What a view's expressions are evaluated in, owned (`view::Env`): the
/// function's helpers, the context, the checked arguments and the provided values.
public struct ViewEnv: Equatable {
    public let helpers: [Function]
    public let ctx: Ctx
    public let args: [String: Value]
    public let provided: [String: Value]

    public init(_ helpers: [Function], _ ctx: Ctx, _ args: [String: Value], _ provided: [String: Value]) {
        self.helpers = helpers
        self.ctx = ctx
        self.args = args
        self.provided = provided
    }

    /// The scope a plan is pulled in.
    public func scope(_ sch: Schema) -> Scope { Scope.of(sch, helpers, ctx, args, provided) }
}

/// §13.4 What `pushAll` did to a view's answer, as positions into the list
/// as it stands (`view::Patch`).
public enum Patch: Equatable {
    /// Puts the node before the element at `at` (`at == len` appends).
    case insert(Int, Value)
    case remove(Int)
    case update(Int, Value)
}

// R9 aggregates -----------------------------------------------------------------

/// R9 An aggregate a plan node's expressions take of one of its lists (`view::Agg`).
public enum Agg: Hashable {
    case count
    /// `init` plus the sum of `f` over the list's nodes, `x` each node.
    case sum(Sym, Expr, Expr)
    /// D4 The least value of a column over the list's rows, `Null` when it is empty.
    case min(String)
    /// D4 The greatest.
    case max(String)

    // The column and the direction an extreme is read in from an index.
    var extreme: (String, Dir)? {
        switch self {
        case .min(let c): return (c, .asc)
        case .max(let c): return (c, .desc)
        default: return nil
        }
    }

    var isCount: Bool {
        if case .count = self { return true }
        return false
    }
}

/// An aggregate and the slot its number is bound at.
public struct Slot: Hashable {
    public let agg: Agg
    public let sym: Sym
}

// How one aggregate use reads its number, which is how it is rewritten.
private enum Use {
    // `len(list)` (no `init`), or a sum's fold (`init + number`).
    case plain(Expr?)
    // D4 `fold(list, init, |acc, x| min(acc, x.c))`, or `max`.
    case foldU(Expr, StdFn)
    // D4 `match first(list) { y => some, None => none }` reading `y.col` alone.
    case pick(Sym, String, Expr, Expr)
}

/// R9 One plan node's expressions as a view evaluates them (`view::Face`):
/// every aggregate use of a kept list replaced by a number the view keeps,
/// at a slot no plan binds (negative).
public struct Face: Hashable {
    public var having: Expr? = nil
    public var project: Expr? = nil
    public var order: [Expr?] = []
    public var members: [Slot]? = nil
    public var ids: [NodeId] = []
    public var kept: [Bool] = []
}

/// R9 A related plan whose list is kept as numbers: the aggregates its parent takes, and its face.
public struct Kept: Hashable {
    public let aggs: [Slot]
    public let face: Face
}

/// R9 What a view keeps as numbers, derived from the plan and the helpers (`view::Shape`).
public struct Shape: Hashable {
    public let root: Face
    public let kept: SortedMap<NodeId, Kept>

    public static func == (a: Shape, b: Shape) -> Bool { a.root == b.root && a.kept == b.kept }
    public func hash(into h: inout Hasher) { h.combine(root) }
}

typealias Ops = [(Dep, Bool)]

// What recognising an aggregate needs beyond the expression: the helpers,
// and the one column each related list is ordered by (D4).
private struct Rec {
    let helpers: [Function]
    let orders: [Sym: (String, Dir)]
}

// The uses of each candidate list: its aggregates, or a list (used otherwise).
private struct Uses {
    enum U {
        case aggs([Agg])
        case list
    }
    var m = [Sym: U]()

    func aggs(_ s: Sym) -> [Agg]? {
        if case .aggs(let a)? = m[s] { return a }
        return nil
    }

    func has(_ s: Sym) -> Bool { m[s] != nil }

    mutating func list(_ s: Sym) { m[s] = .list }

    mutating func add(_ s: Sym, _ a: Agg) {
        if case .aggs(var xs)? = m[s], !xs.contains(a) {
            xs.append(a)
            m[s] = .aggs(xs)
        }
    }
}

enum Shapes {
    /// R9 `shape`: the shape of a plan in a function whose helpers are `helpers`, over a schema.
    static func shape(_ sch: Schema, _ plan: Plan, _ helpers: [Function]) -> Shape {
        var next: Sym = -1
        if let (root, kept) = face(sch, plan, 0, true, helpers, &next) { return Shape(root: root, kept: kept) }
        return Shape(root: Face(), kept: SortedMap())
    }

    // D4 The one column a list is ordered by, when it is.
    private static func oneColumnOrder(_ sch: Schema, _ c: Plan) -> (String, Dir)? {
        guard let tbl = sch.lookupTable(c.table), let first = c.order.first, case .column(let c0) = first.key, c.limit == nil else { return nil }
        let keys = tbl.key.filter { $0 != c0 }
        let rest = Array(c.order.dropFirst())
        let ok = rest.count <= keys.count && zip(rest, keys).allSatisfy { kd, want in kd.dir == .asc && kd.key == .column(want) }
        return ok ? (c0, first.dir) : nil
    }

    // D4 The columns a plan's filter holds equal however it is satisfied, read off the plan.
    private static func filterEqs(_ p: Pred?) -> [String] {
        var out = [String]()
        func go(_ p: Pred) {
            switch p {
            case .cmp(let c, .eq, _): out.append(c)
            case .all(let ps): ps.forEach(go)
            default: break
            }
        }
        if let p = p { go(p) }
        return out
    }

    // D4 Whether a store keeping the schema's indexes reads the rows `eq`
    // holds equal in `col`'s order through one of them (`Secondary::serves`
    // for one column).
    private static func served(_ tbl: Table, _ eq: [String], _ col: String, _ dir: Dir) -> Bool {
        let lists = (tbl.indexes.map { $0.columns } + tbl.refs.map { [$0.column] }).filter { !$0.isEmpty && $0 != tbl.key }
        func held(_ c: String) -> Bool { eq.contains(c) }
        for cols in lists {
            var n = 0
            while n < cols.count && held(cols[n]) { n += 1 }
            let lead = cols[0..<n]
            let rest = Array(cols[n...])
            if !eq.allSatisfy({ lead.contains($0) }) || held(col) { continue }
            if rest.count == 1 && rest[0] == col { return true }
            if rest.isEmpty && dir == .asc && tbl.key.first(where: { !held($0) }) == col { return true }
        }
        return false
    }

    // D4 Whether every extreme among `aggs` can be kept over `p`'s rows with `pinned` held equal.
    private static func extremesKept(_ sch: Schema, _ p: Plan, _ pinned: [String], _ aggs: [Agg], _ child: Bool) -> Bool {
        if !aggs.contains(where: { $0.extreme != nil }) { return true }
        guard let tbl = sch.lookupTable(p.table) else { return false }
        if child && (p.project != nil || p.having != nil || !p.related.isEmpty) { return false }
        let eq = pinned + filterEqs(p.filter)
        return aggs.compactMap { $0.extreme }.allSatisfy { c, d in tbl.column(c)?.nullable == false && served(tbl, eq, c, d) }
    }

    // A plan's face, and the kept plans beneath it; `nil` when a child plan
    // (`root` false) cannot be all numbers.
    private static func face(_ sch: Schema, _ p: Plan, _ base: NodeId, _ root: Bool, _ helpers: [Function], _ next: inout Sym) -> (Face, SortedMap<NodeId, Kept>)? {
        var isTable = false
        if case .table = p.source { isTable = true }
        if !root && (!p.lookups.isEmpty || p.limit != nil || p.members != nil || !isTable) { return nil }
        var ids = [NodeId]()
        var id = base + p.lookups.count
        for r in p.related {
            ids.append(id)
            id += 1 + View.nodeCount(r.plan)
        }
        // Pass one: every use of every list.
        var cands = Set(p.related.map { $0.sym })
        if let m = p.members, root { cands.insert(m) }
        var uses = Uses()
        for s in cands { uses.m[s] = .aggs([]) }
        var orders = [Sym: (String, Dir)]()
        for r in p.related { if let o = oneColumnOrder(sch, r.plan) { orders[r.sym] = o } }
        let rec = Rec(helpers: helpers, orders: orders)
        var exprs = [p.having, p.project].compactMap { $0 }
        for o in p.order { if case .expr(let e) = o.key { exprs.append(e) } }
        for e in exprs { scan(e, cands, rec, &uses) }
        // D4 An extreme no index serves leaves its list a list.
        for r in p.related {
            guard let aggs = uses.aggs(r.sym) else { continue }
            if !extremesKept(sch, r.plan, r.on.map { $0.column }, aggs, true) { uses.list(r.sym) }
        }
        if let m = p.members, root, case .group(_, let by) = p.source, let aggs = uses.aggs(m) {
            if !extremesKept(sch, p, by, aggs, false) { uses.list(m) }
        }
        // The default node carries every related list whole.
        if p.project == nil { for r in p.related { uses.list(r.sym) } }
        // Members read before the numbers are bound are a list.
        if let m = p.members {
            let early = p.lookups.flatMap { $0.key } + p.related.flatMap { $0.on.map { $0.e } }
            if early.contains(where: { Eval.mentions($0, m) }) { uses.list(m) }
        }
        var kept = SortedMap<NodeId, Kept>()
        var flags = [Bool]()
        var slots = [Sym: [Slot]]()
        func assign(_ aggs: [Agg]) -> [Slot] {
            aggs.map { a in
                let s = next
                next -= a.isCount ? 1 : 2
                return Slot(agg: a, sym: s)
            }
        }
        for (r, rid) in zip(p.related, ids) {
            var child: (Face, SortedMap<NodeId, Kept>)? = nil
            if uses.aggs(r.sym) != nil { child = face(sch, r.plan, rid + 1, false, helpers, &next) }
            if let c = child {
                let aggs = assign(uses.aggs(r.sym) ?? [])
                slots[r.sym] = aggs
                for (k, v) in c.1 { kept[k] = v }
                kept[rid] = Kept(aggs: aggs, face: c.0)
                flags.append(true)
            } else {
                if !root { return nil }
                flags.append(false)
            }
        }
        var members: [Slot]? = nil
        if let m = p.members, root, let aggs = uses.aggs(m) {
            let a = assign(aggs)
            slots[m] = a
            members = a
        }
        func rw(_ e: Expr) -> Expr { rewrite(e, slots, rec) }
        let face = Face(
            having: p.having.map(rw), project: p.project.map(rw),
            order: p.order.map { o -> Expr? in
                if case .expr(let e) = o.key { return rw(e) }
                return nil
            },
            members: members, ids: ids, kept: flags)
        return (face, kept)
    }

    // One use of a list that is an aggregate: the list's symbol, the aggregate, and how it reads it.
    private static func aggregateUse(_ e: Expr, _ cands: Set<Sym>, _ rec: Rec) -> (Sym, Agg, Use)? {
        func listVar(_ es: [Expr]) -> Sym? {
            guard es.count == 1, case .variable(let s) = es[0], cands.contains(s) else { return nil }
            return s
        }
        switch e {
        case .std(.Len, let args):
            if let s = listVar(args) { return (s, .count, .plain(nil)) }
        case .fold(let over, let initE, let acc, let x, let body):
            guard case .variable(let s) = over, cands.contains(s) else { return nil }
            if let (g, c) = extremeStep(body, acc, x) { return (s, g == .Min ? .min(c) : .max(c), .foldU(initE, g)) }
            guard let f = Eval.sumStep(body, acc) else { return nil }
            if closed(f, [x], false) && closed(initE, [], false) { return (s, .sum(x, f, initE), .plain(initE)) }
        // D4 `first`/`last` of a list ordered by one column, read for that column alone.
        case .match(let o, let y, let some, let none):
            if case .std(let fn, let args) = o, fn == .First || fn == .Last {
                guard let s = listVar(args), let (c, d) = rec.orders[s] else { return nil }
                if !onlyField(some, y, c) { return nil }
                let least = (d == .asc) == (fn == .First)
                return (s, least ? .min(c) : .max(c), .pick(y, c, some, none))
            }
        case .call(let fn, let args):
            guard let s = listVar(args) else { return nil }
            guard let h = rec.helpers.first(where: { $0.name == fn && $0.kind == .helper }), let (agg, initE) = helperAgg(h) else { return nil }
            return (s, agg, .plain(initE))
        default: break
        }
        return nil
    }

    // D4 A fold's step that is `min(acc, x.c)` or `max(acc, x.c)`, either way round.
    private static func extremeStep(_ body: Expr, _ acc: Sym, _ x: Sym) -> (StdFn, String)? {
        guard case .std(let fn, let args) = body, fn == .Min || fn == .Max, args.count == 2 else { return nil }
        func fieldOfX(_ e: Expr) -> String? {
            if case .field(let inner, let n) = e, inner == .variable(x) { return n }
            return nil
        }
        if args[0] == .variable(acc), let c = fieldOfX(args[1]) { return (fn, c) }
        if args[1] == .variable(acc), let c = fieldOfX(args[0]) { return (fn, c) }
        return nil
    }

    // D4 Whether every occurrence of `y` in `e` is `y.c`.
    private static func onlyField(_ e: Expr, _ y: Sym, _ c: String) -> Bool {
        if case .field(let inner, let n) = e, inner == .variable(y) { return n == c }
        if case .variable(let s) = e { return s != y }
        return children(e).allSatisfy { onlyField($0, y, c) }
    }

    // A helper whose whole body is an aggregate of its one parameter.
    private static func helperAgg(_ h: Function) -> (Agg, Expr?)? {
        guard h.input.count == 1, h.body.count == 1, case .ret(let be?) = h.body[0] else { return nil }
        let p = h.input[0].name
        func isP(_ e: Expr) -> Bool {
            if case .arg(let n) = e { return n == p }
            return false
        }
        if case .std(.Len, let args) = be, args.count == 1, isP(args[0]) { return (.count, nil) }
        if case .fold(let over, let initE, let acc, let x, let body) = be, isP(over) {
            guard let f = Eval.sumStep(body, acc) else { return nil }
            if closed(f, [x], true) && closed(initE, [], true) { return (.sum(x, f, initE), initE) }
        }
        return nil
    }

    // Pass one: note each use of a candidate list — an aggregate, or anything
    // else, which makes it a list.
    private static func scan(_ e: Expr, _ cands: Set<Sym>, _ rec: Rec, _ uses: inout Uses) {
        if let (s, agg, use) = aggregateUse(e, cands, rec) {
            uses.add(s, agg)
            switch use {
            case .plain(let i): if let i = i { scan(i, cands, rec, &uses) }
            case .foldU(let i, _): scan(i, cands, rec, &uses)
            case .pick(_, _, let some, let none):
                scan(some, cands, rec, &uses)
                scan(none, cands, rec, &uses)
            }
            return
        }
        if case .variable(let s) = e, cands.contains(s) { uses.list(s) }
        for c in children(e) { scan(c, cands, rec, &uses) }
    }

    // Pass two: each aggregate use of a kept list replaced by its number.
    private static func rewrite(_ e: Expr, _ slots: [Sym: [Slot]], _ rec: Rec) -> Expr {
        let kept = Set(slots.keys)
        return mapExpr(e) { x in
            guard let (s, agg, u) = aggregateUse(x, kept, rec), let slot = slots[s]?.first(where: { $0.agg == agg })?.sym else { return nil }
            switch u {
            // The sum's `init` is in its number, which is two slots.
            case .plain(let i):
                if i == nil { return .variable(slot) }
                return .op(.add, [.variable(slot), .variable(slot - 1)])
            case .foldU(let i, let g):
                let initE = rewrite(i, slots, rec)
                let y = slot - 1
                return .match(.variable(slot), y, .std(g, [initE, .variable(y)]), initE)
            case .pick(let y, let col, let some, let none):
                let some2 = mapExpr(some) { z in
                    if case .field(let inner, let n) = z, inner == .variable(y), n == col { return .variable(y) }
                    return nil
                }
                return .match(.variable(slot), y, rewrite(some2, slots, rec), rewrite(none, slots, rec))
            }
        }
    }

    // Whether every symbol free in `e` is one of `allowed` and nothing is read.
    private static func closed(_ e: Expr, _ allowed: [Sym], _ strict: Bool) -> Bool {
        func go(_ e: Expr, _ bound: [Sym]) -> Bool {
            switch e {
            case .variable(let s): return bound.contains(s)
            case .arg, .auto, .provided: return !strict
            case .select, .get, .exists: return false
            case .match(let x, let s, let a, let n): return go(x, bound) && go(n, bound) && go(a, bound + [s])
            case .map(let o, let s, let b), .filter(let o, let s, let b), .any(let o, let s, let b), .all(let o, let s, let b), .sortBy(let o, let s, let b):
                return go(o, bound) && go(b, bound + [s])
            case .fold(let o, let z, let a, let s, let b): return go(o, bound) && go(z, bound) && go(b, bound + [a, s])
            default: return children(e).allSatisfy { go($0, bound) }
            }
        }
        return go(e, allowed)
    }

    // `e` with `f` applied top-down: where `f` answers, its answer replaces the subexpression.
    private static func mapExpr(_ e: Expr, _ f: (Expr) -> Expr?) -> Expr {
        if let r = f(e) { return r }
        return mapChildren(e) { mapExpr($0, f) }
    }
}

// The kept numbers, against a plan, its shape, a scope and a store.
final class Cx {
    let sch: Schema
    let plan: Plan
    let shape: Shape
    let scope: Scope
    let st: Store
    let rel: [Related?]
    var filters = [NodeId: Filter?]()
    var rootFilter: Filter?

    init(_ sch: Schema, _ plan: Plan, _ shape: Shape, _ scope: Scope, _ st: Store) throws {
        self.sch = sch
        self.plan = plan
        self.shape = shape
        self.scope = scope
        self.st = st
        rel = View.nodes(plan).map { n -> Related? in
            if case .related(let r) = n { return r }
            return nil
        }
        for id in shape.kept.keys { filters[id] = .some(try rootFilterOf(related(id).plan)) }
        rootFilter = try rootFilterOf(plan)
    }

    func rootFilterOf(_ p: Plan) throws -> Filter? {
        guard let f = p.filter else { return nil }
        return try View.evalPred(f) { try scope.eval($0) }
    }

    func related(_ id: NodeId) -> Related {
        guard id < rel.count, let r = rel[id] else { fatalError("a kept node is a related plan") }
        return r
    }

    func kept(_ id: NodeId) -> Kept { shape.kept[id]! }

    func filter(_ id: NodeId) -> Filter? { filters[id] ?? nil }
}

// A group's members as the entry is built with them: none, the list, or its numbers.
private enum Members {
    case none
    case list(Value)
    case nums([Num])
}

enum R9 {
    static func intOf(_ v: Value) throws -> Int64 {
        guard case .int(let n) = v else { throw EvalFault.bug(.typeError("an aggregate's term is an Int, not \(v.show())")) }
        return n
    }

    static func boolOf(_ v: Value) throws -> Bool {
        guard case .bool(let b) = v else { throw EvalFault.bug(.typeError("a having is a Bool, not \(v.show())")) }
        return b
    }

    // What `aggs` come to over no node: nothing counted, a sum's `init`, no extreme.
    static func zero(_ cx: Cx, _ aggs: [Slot]) throws -> [Num] {
        try aggs.map { s in
            switch s.agg {
            case .count: return .n(.zero)
            case .sum(_, _, let i): return .n(Wide(try intOf(try cx.scope.node().eval(i))))
            default: return .v(.null)
            }
        }
    }

    static func wide(_ n: Num) throws -> Wide {
        guard case .n(let w) = n else { throw EvalFault.bug(.typeError("a count or a sum at \(n)")) }
        return w
    }

    // One admitted node's terms put in.
    static func putIn(_ aggs: [Slot], _ nums: inout [Num], _ terms: [Num]) throws {
        for i in aggs.indices {
            let n = nums[i]
            let t = terms[i]
            if let ex = aggs[i].agg.extreme {
                guard case .v(let nv) = n, case .v(let tv) = t else { throw EvalFault.bug(.typeError("an extreme \(n) by \(t)")) }
                let o = compareValue(tv, nv)
                let beyond = ex.1 == .asc ? o < 0 : o > 0
                if nv.isNull || beyond { nums[i] = t }
            } else {
                nums[i] = .n(try wide(n) + wide(t))
            }
        }
    }

    // One admitted node's terms taken out; whether an extreme was taken with it.
    static func takeOut(_ aggs: [Slot], _ nums: inout [Num], _ terms: [Num]) throws -> Bool {
        var stale = false
        for i in aggs.indices {
            let n = nums[i]
            if aggs[i].agg.extreme == nil {
                nums[i] = .n(try wide(n) - wide(terms[i]))
            } else if case .v(let nv) = n {
                stale = stale || nv.isNull || n == terms[i]
            } else {
                throw EvalFault.bug(.typeError("an extreme at \(n)"))
            }
        }
        return stale
    }

    // The numbers bound for a node to read, at their slots: a sum as two
    // clamped `Int`s whose checked sum is the total when it is one.
    static func bindNums(_ node: NodeScope, _ aggs: [Slot], _ nums: [Num]) {
        for (s, n) in zip(aggs, nums) {
            switch (s.agg, n) {
            case (.sum, .n(let w)):
                let first = w.clamped
                node.bind(s.sym, .int(first))
                node.bind(s.sym - 1, .int((w - Wide(first)).clamped))
            case (_, .v(let v)): node.bind(s.sym, v)
            case (_, .n(let w)): node.bind(s.sym, .int(w.clamped))
            }
        }
    }

    static func column(_ node: Value, _ c: String) -> Value { node.recordValue?[c] ?? .null }

    // The terms one admitted node adds to the aggregates.
    static func terms(_ cx: Cx, _ aggs: [Slot], _ node: Value) throws -> [Num] {
        try aggs.map { s in
            switch s.agg {
            case .count: return .n(.one)
            case .sum(let x, let f, _):
                let n = cx.scope.node()
                n.bind(x, node)
                return .n(Wide(try intOf(try n.eval(f))))
            case .min(let c), .max(let c): return .v(column(node, c))
            }
        }
    }

    // A related plan's pins for one dependency.
    static func pinsOf(_ r: Related, _ on: Value) -> [(String, Value)] {
        guard case .list(let xs) = on else { return [] }
        return Array(zip(r.on.map { $0.column }, xs))
    }

    // D4 Every extreme among `aggs` read again from the store, which is final.
    static func extremes(_ cx: Cx, _ plan: Plan, _ pins: [(String, Value)], _ aggs: [Slot], _ nums: inout [Num]) throws {
        if !aggs.contains(where: { $0.agg.extreme != nil }) { return }
        let (_, filter) = try View.source(cx.sch, plan, pins, cx.scope)
        let eq = View.equalities(filter)
        let sp = View.spans(filter)
        for i in aggs.indices {
            guard let (c, d) = aggs[i].agg.extreme else { continue }
            var row: Row?
            if let ordered = cx.st.scanOrdered(plan.table, eq, sp, [(c, d)], { View.admits(filter, $0) }, 1) {
                row = ordered.first
            } else {
                for b in View.fetch(cx.st, plan.table, filter) {
                    guard let a = row else {
                        row = b
                        continue
                    }
                    let o = compareValue(b[c] ?? .null, a[c] ?? .null)
                    if (d == .asc && o < 0) || (d == .desc && o > 0) { row = b }
                }
            }
            nums[i] = .v(row?[c] ?? .null)
        }
    }

    // A child row of a kept plan with no related plans: its node, when its having admits it.
    static func rowNode(_ cx: Cx, _ id: NodeId, _ row: Value) throws -> Value? {
        let c = cx.related(id).plan
        let face = cx.kept(id).face
        let node = cx.scope.node()
        if let r = c.row { node.bind(r, row) }
        if let h = face.having, try !boolOf(try node.eval(h)) { return nil }
        if let p = face.project { return try node.eval(p) }
        return row
    }

    // The terms a child row adds to its parent's numbers: none when the filter or the having refuses it.
    static func rowTerms(_ cx: Cx, _ id: NodeId, _ row: Row) throws -> [Num]? {
        if !View.admits(cx.filter(id), row) { return nil }
        guard let node = try rowNode(cx, id, .record(row)) else { return nil }
        return try terms(cx, cx.kept(id).aggs, node)
    }

    // A child node of a kept plan whose lists are kept: its having and node over its numbers.
    static func kidNode(_ cx: Cx, _ id: NodeId, _ row: Value, _ subs: [Value], _ held: HeldMap) throws -> (Bool, Value) {
        let c = cx.related(id).plan
        let face = cx.kept(id).face
        let node = cx.scope.node()
        if let r = c.row { node.bind(r, row) }
        for (jid, on) in zip(face.ids, subs) { bindNums(node, cx.kept(jid).aggs, held[Dep(jid, on)]!.nums) }
        let admitted = try face.having.map { try boolOf(try node.eval($0)) } ?? true
        let value: Value
        if !admitted {
            value = .null
        } else if let p = face.project {
            value = try node.eval(p)
        } else {
            value = row
        }
        return (admitted, value)
    }

    static func kidTerms(_ cx: Cx, _ id: NodeId, _ kid: Kid) throws -> [Num]? {
        kid.admitted ? try terms(cx, cx.kept(id).aggs, kid.node) : nil
    }

    // A child row of a kept plan whose lists are kept, as a node.
    static func buildKid(_ cx: Cx, _ id: NodeId, _ on: Value, _ key: [Value], _ row: Value, _ held: HeldMap, _ ops: inout Ops) throws -> Kid {
        let c = cx.related(id).plan
        let face = cx.kept(id).face
        var subs = [Value]()
        for (r, jid) in zip(c.related, face.ids) {
            let node = cx.scope.node()
            if let rs = c.row { node.bind(rs, row) }
            let sub = Value.list(try r.on.map { try node.eval($0.e) })
            try ensure(cx, jid, sub, Reader(id: id, on: on, key: key), held, &ops)
            subs.append(sub)
        }
        let (admitted, node) = try kidNode(cx, id, row, subs, held)
        return Kid(row: row, admitted: admitted, node: node, subs: subs)
    }

    // The numbers of `(id, on)`, read by `parent`: kept if some reader holds
    // them already, pulled from the store otherwise.
    static func ensure(_ cx: Cx, _ id: NodeId, _ on: Value, _ parent: Reader?, _ held: HeldMap, _ ops: inout Ops) throws {
        let k = Dep(id, on)
        if let h = held[k] {
            h.parents.insert(parent)
            return
        }
        let r = cx.related(id)
        let c = r.plan
        let kept = cx.kept(id)
        let pins = pinsOf(r, on)
        let h = KeptNums(try zero(cx, kept.aggs), SortedMap(), [parent])
        if c.related.isEmpty && kept.aggs.allSatisfy({ $0.agg.extreme != nil }) {
            // D4 Extremes alone: one indexed read each, not the list.
            try extremes(cx, c, pins, kept.aggs, &h.nums)
        } else {
            let (tbl, filter) = try View.source(cx.sch, c, pins, cx.scope)
            for row in View.fetch(cx.st, c.table, filter) {
                if c.related.isEmpty {
                    if let n = try rowNode(cx, id, .record(row)) { try putIn(kept.aggs, &h.nums, try terms(cx, kept.aggs, n)) }
                } else {
                    let key = tbl.keyOf(row)
                    let kid = try buildKid(cx, id, on, key, .record(row), held, &ops)
                    if let t = try kidTerms(cx, id, kid) { try putIn(kept.aggs, &h.nums, t) }
                    h.kids[key] = kid
                }
            }
        }
        ops.append((k, true))
        held[k] = h
    }

    // `parent` stops reading `k`; numbers nobody reads are let go, and what their children read with them.
    static func release(_ cx: Cx, _ k: Dep, _ parent: Reader?, _ held: HeldMap, _ ops: inout Ops) {
        guard let h = held[k] else { return }
        h.parents.remove(parent)
        if !h.parents.isEmpty { return }
        held[k] = nil
        let ids = cx.kept(k.id).face.ids
        for (key, kid) in h.kids {
            let me = Reader(id: k.id, on: k.on, key: key)
            for (jid, sub) in zip(ids, kid.subs) { release(cx, Dep(jid, sub), me, held, &ops) }
        }
        ops.append((k, false))
    }
}

// R9 A change to a table a kept plan reads, as one entry's numbers see it.
private struct Hit {
    let id: NodeId
    let on: Value
    let old: Row?
    let new: Row?
}

// What a held number is waiting for in a sweep.
private final class Dirt {
    var before: [Num]? = nil
    var reread = SortedSet<[Value]>()
    var reeval = SortedSet<[Value]>()
    var stale = false
}

// R9's group bookkeeping: what the changes did to one group's members (`view::Moves`).
private final class Moves {
    var arrived = SortedSet<[Value]>()
    var left = SortedSet<[Value]>()

    func has(_ base: SortedSet<[Value]>?, _ k: [Value]) -> Bool { arrived.contains(k) || (!left.contains(k) && base?.contains(k) == true) }

    // `k` leaves; whether it was a member.
    func leave(_ base: SortedSet<[Value]>?, _ k: [Value]) -> Bool {
        if !has(base, k) { return false }
        if !arrived.remove(k) { left.insert(k) }
        return true
    }

    // `k` arrives; whether it was not a member.
    func arrive(_ base: SortedSet<[Value]>?, _ k: [Value]) -> Bool {
        if has(base, k) { return false }
        if !left.remove(k) { arrived.insert(k) }
        return true
    }

    func size(_ base: SortedSet<[Value]>?) -> Int { (base?.count ?? 0) + arrived.count - left.count }

    func keys(_ base: SortedSet<[Value]>?) -> [[Value]] {
        var out = SortedSet<[Value]>()
        if let b = base { for k in b where !left.contains(k) { out.insert(k) } }
        out.formUnion(arrived)
        return out.items
    }
}

// R9, D4 One touched group's kept member numbers.
private final class Track {
    var nums: [Num]?
    var stale: Bool
    init(_ nums: [Num]?, _ stale: Bool) {
        self.nums = nums
        self.stale = stale
    }
}

// What the changes touch: the keys to rebuild, each with its hits; the group moves; the group sums.
private final class Touched {
    var keys = SortedMap<[Value], [Hit]>()
    var groups = SortedMap<[Value], Moves>()
    var sums = SortedMap<[Value], Track>()
}

/// §1.5 A plan kept up to date (`view::View`): the plan, its environment, its
/// shape, and one `Entry` per candidate, with the two indexes that route a change.
public final class MaintainedView: Equatable {
    public let plan: Plan
    public let env: ViewEnv
    public let shape: Shape
    // The admitted entries in answer order, not cut to the limit, each as its place.
    var entries: [[Value]]
    var byKey: SortedMap<[Value], Entry>
    var byDep: SortedMap<Dep, SortedSet<[Value]>>
    var groups: SortedMap<[Value], SortedSet<[Value]>>

    init(_ plan: Plan, _ env: ViewEnv, _ shape: Shape, _ entries: [[Value]], _ byKey: SortedMap<[Value], Entry>, _ byDep: SortedMap<Dep, SortedSet<[Value]>>, _ groups: SortedMap<[Value], SortedSet<[Value]>>) {
        self.plan = plan
        self.env = env
        self.shape = shape
        self.entries = entries
        self.byKey = byKey
        self.byDep = byDep
        self.groups = groups
    }

    /// The answer as it stands: the admitted entries' nodes in order, cut to the limit.
    public func rows() -> [Value] { (0..<Swift.min(entries.count, limit())).map(nodeAt) }

    public static func == (a: MaintainedView, b: MaintainedView) -> Bool {
        a.plan == b.plan && a.env == b.env && a.shape == b.shape && a.entries == b.entries && a.byKey == b.byKey && a.byDep == b.byDep && a.groups == b.groups
    }

    private func limit() -> Int { View.limitOf(plan) }

    private func keyAt(_ i: Int) -> [Value] { Array(entries[i][plan.order.count...]) }

    private func nodeAt(_ i: Int) -> Value { byKey[keyAt(i)]!.node }

    private func positionIn(_ from: Int, _ to: Int, _ e: Entry) -> Int {
        var lo = from
        var hi = to
        while lo < hi {
            let mid = (lo + hi) >> 1
            if View.comparePlace(plan, entries[mid], e) < 0 { lo = mid + 1 } else { hi = mid }
        }
        return lo
    }

    private func fitsAt(_ p: Int, _ e: Entry) -> Bool {
        let before = p == 0 || View.comparePlace(plan, entries[p - 1], e) < 0
        let after = p + 1 >= entries.count || View.comparePlace(plan, entries[p + 1], e) > 0
        return before && after
    }

    // Every dependency an entry recorded, its kept numbers' included.
    func index(_ e: Entry) {
        indexDeps(e)
        for d in e.held.keys { byDep.modify(d, default: { SortedSet() }) { _ = $0.insert(e.key) } }
    }

    private func indexDeps(_ e: Entry) {
        for d in e.deps { byDep.modify(d, default: { SortedSet() }) { _ = $0.insert(e.key) } }
    }

    private func drop(_ d: Dep, _ key: [Value]) {
        let empty = byDep.modifyIfPresent(d) { ks -> Bool in
            ks.remove(key)
            return ks.isEmpty
        }
        if empty == true { byDep.remove(d) }
    }

    private func unindex(_ e: Entry) { for d in e.deps { drop(d, e.key) } }

    // What the kept numbers of the entry under `key` came to hold or let go, in order.
    func applyOps(_ key: [Value], _ ops: Ops) {
        for (d, holds) in ops {
            if holds { byDep.modify(d, default: { SortedSet() }) { _ = $0.insert(key) } } else { drop(d, key) }
        }
    }

    // §1.5, 3–4 One entry's new state against its old, as patches against the
    // window the client holds (`View::settle`).
    func settle(_ key: [Value], _ new: Entry?, _ out: inout [Patch]) {
        let lim = limit()
        let old = byKey.remove(key)
        var was: Int? = nil
        if let o = old, o.admitted { was = positionIn(0, entries.count, o) }
        let sameDeps = old != nil && new != nil && old!.deps == new!.deps
        if let o = old, !sameDeps { unindex(o) }
        var now: Int? = nil
        if let n = new, n.admitted {
            if let w = was {
                if fitsAt(w, n) {
                    now = w
                } else if w > 0 && View.comparePlace(plan, entries[w - 1], n) > 0 {
                    now = positionIn(0, w, n)
                } else {
                    now = positionIn(w + 1, entries.count, n) - 1
                }
            } else {
                now = positionIn(0, entries.count, n)
            }
        }
        let changed = old == nil || new == nil || old!.node != new!.node
        switch (was, now) {
        case (let w?, nil): entries.remove(at: w)
        case (nil, let n?): entries.insert([], at: n)
        case (let w?, let n?) where w != n:
            let x = entries.remove(at: w)
            entries.insert(x, at: n)
        default: break
        }
        if let n = now, let ne = new {
            let k = plan.order.count
            let slot = entries[n]
            if slot.count != k + ne.key.count || Array(slot[0..<k]) != ne.order { entries[n] = View.place(ne) }
        }
        if let ne = new {
            if !sameDeps { indexDeps(ne) }
            byKey[key] = ne
        }
        switch (was, now) {
        case (nil, nil): break
        case (let p?, nil):
            if p < lim {
                out.append(.remove(p))
                if entries.count >= lim { out.append(.insert(lim - 1, nodeAt(lim - 1))) }
            }
        case (nil, let q?):
            if q < lim {
                out.append(.insert(q, nodeAt(q)))
                if entries.count > lim { out.append(.remove(lim)) }
            }
        case (let p?, let q?):
            if p == q {
                if p < lim && changed { out.append(.update(p, nodeAt(p))) }
            } else if p < lim && q < lim {
                out.append(.remove(p))
                out.append(.insert(q, nodeAt(q)))
            } else if p < lim {
                out.append(.remove(p))
                out.append(.insert(lim - 1, nodeAt(lim - 1)))
            } else if q < lim {
                out.append(.insert(q, nodeAt(q)))
                out.append(.remove(lim))
            }
        }
    }
}

/// §1.5 Hydrate, push, and the contract (`view.rs`).
public enum Views {
    /// R9 `shape`: what a view of `plan` keeps as numbers.
    public static func shape(_ sch: Schema, _ plan: Plan, _ helpers: [Function]) -> Shape { Shapes.shape(sch, plan, helpers) }

    /// §1.5 `hydrate`: pull everything, and keep it.
    public static func hydrate(_ sch: Schema, _ plan: Plan, _ env: ViewEnv, _ st: Store) throws -> MaintainedView {
        let shape = Shapes.shape(sch, plan, env.helpers)
        let scope = env.scope(sch)
        let pulled = try pullRoot(try Cx(sch, plan, shape, scope, st))
        var groups = SortedMap<[Value], SortedSet<[Value]>>()
        if case .group(_, let by) = plan.source {
            let (tbl, filter) = try View.source(sch, plan, [], scope)
            for r in View.fetch(st, plan.table, filter) { groups.modify(View.groupOf(by, r), default: { SortedSet() }) { _ = $0.insert(tbl.keyOf(r)) } }
        }
        let v = MaintainedView(plan, env, shape, [], SortedMap(), SortedMap(), groups)
        for e in pulled {
            if e.admitted { v.entries.append(View.place(e)) }
            v.index(e)
            v.byKey[e.key] = e
        }
        return v
    }

    /// §1.6 `rebuild`: hydrate again, in the same environment.
    public static func rebuild(_ sch: Schema, _ st: Store, _ view: MaintainedView) throws -> MaintainedView {
        try hydrate(sch, view.plan, view.env, st)
    }

    /// `contract`: a maintained view is right when it is indistinguishable from one hydrated now.
    public static func contract(_ sch: Schema, _ st: Store, _ view: MaintainedView) -> Bool {
        guard let fresh = try? rebuild(sch, st, view) else { return false }
        return fresh == view
    }

    // A group's members as numbers, from its rows.
    private static func memberNums(_ cx: Cx, _ aggs: [Slot], _ rows: [Value]) throws -> [Num] {
        var nums = try R9.zero(cx, aggs)
        for r in rows { try R9.putIn(aggs, &nums, try R9.terms(cx, aggs, r)) }
        return nums
    }

    // §1.5, 3 and R9 One candidate of the root plan, built over the numbers `held` keeps (`entry_at`).
    private static func entryAt(_ cx: Cx, _ key: [Value], _ row: Cand, _ members: Members, _ held: HeldMap, _ ops: inout Ops) throws -> Entry {
        let plan = cx.plan
        let face = cx.shape.root
        var deps = [Dep]()
        if plan.isBare {
            let order = try plan.order.map { o -> Value in
                guard case .column(let c) = o.key else { throw EvalFault.bug(.typeError("a bare plan orders by columns")) }
                return try row.field(c)
            }
            return Entry(key, deps, order, true, .record(row.value), held, [])
        }
        let node = cx.scope.node()
        if let r = plan.row { node.bind(r, .record(row.value)) }
        var nums = [Num]()
        switch members {
        case .none: break
        case .list(let v): if let m = plan.members { node.bind(m, v) }
        case .nums(let ns):
            R9.bindNums(node, face.members ?? [], ns)
            nums = ns
        }
        var id = 0
        for l in plan.lookups {
            let k = try l.key.map { try node.eval($0) }
            if k.contains(where: { $0.isNull }) {
                node.bind(l.sym, .null)
            } else {
                node.bind(l.sym, cx.st.get(l.table, k).map { .record($0) } ?? .null)
                deps.append(Dep(id, .list(k)))
            }
            id += 1
        }
        var fields: Row? = plan.project == nil ? row.value : nil
        for (j, r) in plan.related.enumerated() {
            let on = try r.on.map { ($0.column, try node.eval($0.e)) }
            if face.kept[j] {
                let dep = Value.list(on.map { $0.1 })
                try R9.ensure(cx, id, dep, nil, held, &ops)
                let stale = held.keys.filter { $0.id == id && $0.on != dep }
                for k in stale { R9.release(cx, k, nil, held, &ops) }
                R9.bindNums(node, cx.kept(id).aggs, held[Dep(id, dep)]!.nums)
            } else {
                deps.append(Dep(id, .list(on.map { $0.1 })))
                let kids = try View.pullAt(cx.sch, r.plan, id + 1, on, cx.scope, cx.st)
                for kid in kids { deps.append(contentsOf: kid.deps) }
                let list = Value.list(View.answer(r.plan, kids))
                fields?.set(r.name, list)
                node.bind(r.sym, list)
            }
            id += 1 + View.nodeCount(r.plan)
        }
        let admitted = try face.having.map { try R9.boolOf(try node.eval($0)) } ?? true
        let value: Value
        if !admitted {
            value = .null
        } else if let p = face.project {
            value = try node.eval(p)
        } else {
            value = .record(fields ?? Record())
        }
        var order = [Value]()
        for (kd, e) in zip(plan.order, face.order) {
            switch kd.key {
            case .column(let c): order.append(try row.field(c))
            case .expr:
                guard let fe = e else { throw EvalFault.bug(.typeError("an expression order key with no face")) }
                order.append(try node.eval(fe))
            }
        }
        return Entry(key, deps, order, admitted, value, held, nums)
    }

    // §1.5 and R9 Every candidate of the root plan, pulled with its shape.
    private static func pullRoot(_ cx: Cx) throws -> [Entry] {
        let plan = cx.plan
        let (tbl, filter) = try View.source(cx.sch, plan, [], cx.scope)
        let rows = View.fetch(cx.st, plan.table, filter)
        var out = [Entry]()
        var ops = Ops()
        switch plan.source {
        case .table:
            for row in rows { out.append(try entryAt(cx, tbl.keyOf(row), Cand(value: row), .none, HeldMap(), &ops)) }
        case .group(_, let by):
            var groups = SortedMap<[Value], [Value]>()
            for row in rows.sorted(by: { compareKeys(tbl.keyOf($0), tbl.keyOf($1)) < 0 }) {
                groups.modify(View.groupOf(by, row), default: { [] }) { $0.append(.record(row)) }
            }
            for (k, members) in groups {
                let m: Members
                if let aggs = cx.shape.root.members { m = .nums(try memberNums(cx, aggs, members)) } else { m = .list(.list(members)) }
                out.append(try entryAt(cx, k, Cand(value: View.groupKey(by, k)), m, HeldMap(), &ops))
            }
        }
        return View.stableSort(out) { View.compareEntries(plan, $0, $1) }
    }

    /// §1.5 `push_all`: the changes of one settle pushed through the view, `st`
    /// already the store after all of them. Every entry a change touches is
    /// rebuilt from `st` once — its kept numbers moved by the change's terms
    /// rather than recounted — and settled against the old one in key order
    /// into patches. A fault leaves the view stale: `rebuild` it.
    public static func pushAll(_ sch: Schema, _ st: Store, _ changes: [Change], _ view: MaintainedView) throws -> [Patch] {
        let scope = view.env.scope(sch)
        let cx = try Cx(sch, view.plan, view.shape, scope, st)
        let t = try touched(cx, changes, view)
        var rebuilt = [([Value], Entry?, Ops)]()
        for (k, hits) in t.keys {
            let old = view.byKey[k]
            let held = old?.held ?? HeldMap()
            // The held numbers move to the rebuilt entry; the old one keeps none.
            if var o = old {
                o.held = HeldMap()
                view.byKey[k] = o
            }
            var ops = Ops()
            try sweep(cx, held, hits, &ops)
            rebuilt.append((k, try rootOf(cx, k, held, t, view.groups, view.byKey[k], &ops), ops))
        }
        for (g, m) in t.groups {
            var members = view.groups[g] ?? SortedSet()
            members.subtract(m.left)
            members.formUnion(m.arrived)
            view.groups[g] = members.isEmpty ? nil : members
        }
        var out = [Patch]()
        for (k, e, ops) in rebuilt {
            view.settle(k, e, &out)
            view.applyOps(k, ops)
        }
        return out
    }

    // §1.5, 1–2 What the changes touch.
    private static func touched(_ cx: Cx, _ changes: [Change], _ view: MaintainedView) throws -> Touched {
        let plan = cx.plan
        guard let tbl = cx.sch.lookupTable(plan.table) else { throw EvalFault.bug(.unknownTable(plan.table)) }
        let ns = View.nodes(plan)
        let t = Touched()
        var memberAggs: [Slot]? = nil
        if let aggs = cx.shape.root.members, aggs.contains(where: { !$0.agg.isCount }) { memberAggs = aggs }
        for ch in changes {
            let old = ch.old
            let new = ch.new
            if ch.table == plan.table {
                switch plan.source {
                case .table:
                    for r in [old, new].compactMap({ $0 }) { t.keys.modify(tbl.keyOf(r), default: { [] }) { _ in } }
                // In change order, so an edit within a group, out of one and
                // into another, leaves the members right — and a kept sum
                // moved by each row that really left or arrived.
                case .group(_, let by):
                    for (r0, arrives) in [(old, false), (new, true)] {
                        guard let r = r0 else { continue }
                        let g = View.groupOf(by, r)
                        let base = view.groups[g]
                        let members = t.groups.modify(g, default: { Moves() }) { $0 }
                        let moved = !arrives ? members.leave(base, tbl.keyOf(r)) : (View.admits(cx.rootFilter, r) && members.arrive(base, tbl.keyOf(r)))
                        if moved, let ma = memberAggs {
                            let terms = try R9.terms(cx, ma, .record(r))
                            let tr = t.sums.modify(g, default: {
                                var nums: [Num]? = nil
                                if let e = view.byKey[g], e.members.count == ma.count { nums = e.members }
                                return Track(nums, false)
                            }) { $0 }
                            if tr.nums != nil {
                                if arrives { try R9.putIn(ma, &tr.nums!, terms) } else { tr.stale = try R9.takeOut(ma, &tr.nums!, terms) || tr.stale }
                            }
                        }
                        t.keys.modify(g, default: { [] }) { _ in }
                    }
                }
            }
            for (id, n) in ns.enumerated() where n.table == ch.table {
                let kept = cx.shape.kept.contains(id)
                let dOld = old.map { n.dependency(cx.sch, $0) }
                let dNew = new.map { n.dependency(cx.sch, $0) }
                let both = dOld != nil && dOld == dNew
                for (d0, isOld) in [(dOld, true), (dNew, false)] {
                    guard let d = d0, !(both && !isOld) else { continue }
                    guard let ks = view.byDep[Dep(id, d)] else { continue }
                    for k in ks {
                        t.keys.modify(k, default: { [] }) { hits in
                            if kept { hits.append(Hit(id: id, on: d, old: isOld ? old : nil, new: (!isOld || both) ? new : nil)) }
                        }
                    }
                }
            }
        }
        return t
    }

    // §1.13, R9 An entry's kept numbers brought up to the store, from the
    // hits its dependencies took, deepest first (`sweep`).
    private static func sweep(_ cx: Cx, _ held: HeldMap, _ hits: [Hit], _ ops: inout Ops) throws {
        var dirty = SortedMap<Dep, Dirt>()
        for hit in hits {
            let k = Dep(hit.id, hit.on)
            guard let h = held[k] else { continue }
            let d = dirty.modify(k, default: { Dirt() }) { $0 }
            if d.before == nil { d.before = h.nums }
            let c = cx.related(hit.id).plan
            if c.related.isEmpty {
                let aggs = cx.kept(hit.id).aggs
                if let o = hit.old, let terms = try R9.rowTerms(cx, hit.id, o) { d.stale = try R9.takeOut(aggs, &h.nums, terms) || d.stale }
                if let n = hit.new, let terms = try R9.rowTerms(cx, hit.id, n) { try R9.putIn(aggs, &h.nums, terms) }
            } else {
                guard let tbl = cx.sch.lookupTable(c.table) else { throw EvalFault.bug(.unknownTable(c.table)) }
                for row in [hit.old, hit.new].compactMap({ $0 }) { d.reread.insert(tbl.keyOf(row)) }
            }
        }
        while let (k, d) = dirty.popLast() {
            guard let h = held[k] else { continue }
            held[k] = nil
            let before = d.before ?? h.nums
            let id = k.id
            let on = k.on
            let r = cx.related(id)
            let ids = cx.kept(id).face.ids
            let aggs = cx.kept(id).aggs
            if d.stale { try R9.extremes(cx, r.plan, R9.pinsOf(r, on), aggs, &h.nums) }
            for key in d.reread {
                let old = h.kids.remove(key)
                var row: Row? = nil
                if let x = cx.st.get(r.plan.table, key), View.admits(cx.filter(id), x), Node.related(r).dependency(cx.sch, x) == on { row = x }
                let new = try row.map { try R9.buildKid(cx, id, on, key, .record($0), held, &ops) }
                if let o = old {
                    let me = Reader(id: id, on: on, key: key)
                    for (j, pair) in zip(ids, o.subs).enumerated() {
                        let (jid, sub) = pair
                        if new == nil || new!.subs[j] != sub { R9.release(cx, Dep(jid, sub), me, held, &ops) }
                    }
                    if let terms = try R9.kidTerms(cx, id, o) { _ = try R9.takeOut(aggs, &h.nums, terms) }
                }
                if let n = new {
                    if let terms = try R9.kidTerms(cx, id, n) { try R9.putIn(aggs, &h.nums, terms) }
                    h.kids[key] = n
                }
            }
            for key in d.reeval where !d.reread.contains(key) {
                guard let kid = h.kids[key] else { continue }
                let (admitted, node) = try R9.kidNode(cx, id, kid.row, kid.subs, held)
                if admitted == kid.admitted && node == kid.node { continue }
                let oldTerms = try R9.kidTerms(cx, id, kid)
                var now = kid
                now.admitted = admitted
                now.node = node
                h.kids[key] = now
                let newTerms = try R9.kidTerms(cx, id, now)
                if let o = oldTerms { _ = try R9.takeOut(aggs, &h.nums, o) }
                if let n = newTerms { try R9.putIn(aggs, &h.nums, n) }
            }
            if h.nums != before {
                for p in h.parents { if let p = p { dirty.modify(Dep(p.id, p.on), default: { Dirt() }) { $0 }.reeval.insert(p.key) } }
            }
            held[k] = h
        }
    }

    // §1.5, 1 and R9 The entry under `k` as the store now decides it: gone,
    // new, or rebuilt over the numbers `held` keeps.
    private static func rootOf(_ cx: Cx, _ k: [Value], _ held: HeldMap, _ t: Touched, _ viewGroups: SortedMap<[Value], SortedSet<[Value]>>, _ old: Entry?, _ ops: inout Ops) throws -> Entry? {
        let plan = cx.plan
        func gone() -> Entry? {
            for d in held.keys { ops.append((d, false)) }
            return nil
        }
        switch plan.source {
        case .table:
            if let row = cx.st.get(plan.table, k), View.admits(cx.rootFilter, row) {
                return try entryAt(cx, k, Cand(value: row), .none, held, &ops)
            }
            return gone()
        case .group(_, let by):
            let base = viewGroups[k]
            let moves = t.groups[k]
            let size = moves?.size(base) ?? (base?.count ?? 0)
            if size == 0 { return gone() }
            func rows() -> [Value] {
                let keys = moves?.keys(base) ?? base?.items ?? []
                return keys.compactMap { cx.st.get(plan.table, $0).map { .record($0) } }
            }
            let members: Members
            if let aggs = cx.shape.root.members {
                let track = t.sums[k]
                var moved: [Num]? = nil
                if let o = old, track == nil, o.members.count == aggs.count {
                    moved = o.members
                } else if let tr = track, var nums = tr.nums {
                    if tr.stale { try R9.extremes(cx, plan, Array(zip(by, k)), aggs, &nums) }
                    moved = nums
                }
                if var m = moved {
                    for i in aggs.indices where aggs[i].agg.isCount { m[i] = .n(Wide(Int64(size))) }
                    members = .nums(m)
                } else if aggs.allSatisfy({ $0.agg.isCount }) {
                    members = .nums(Array(repeating: .n(Wide(Int64(size))), count: aggs.count))
                } else {
                    members = .nums(try memberNums(cx, aggs, rows()))
                }
            } else {
                members = .list(.list(rows()))
            }
            return try entryAt(cx, k, Cand(value: View.groupKey(by, k)), members, held, &ops)
        }
    }

    /// Apply each patch in order: the definition a client's list is held to (`splice`).
    public static func splice(_ ps: [Patch], _ xs: [Value]) -> [Value] {
        var vs = xs
        for p in ps {
            switch p {
            case .insert(let at, let n): vs.insert(n, at: Swift.min(at, vs.count))
            case .remove(let at): if at < vs.count { vs.remove(at: at) }
            case .update(let at, let n): if at < vs.count { vs[at] = n }
            }
        }
        return vs
    }

    /// A patch as the vectors write one: `{t: "insert", at, node}`, `{t:
    /// "remove", at}`, `{t: "update", at, node}`.
    public static func patchValue(_ p: Patch) -> Value {
        switch p {
        case .insert(let at, let n): return .record([("t", .text("insert")), ("at", .int(Int64(at))), ("node", n)])
        case .remove(let at): return .record([("t", .text("remove")), ("at", .int(Int64(at)))])
        case .update(let at, let n): return .record([("t", .text("update")), ("at", .int(Int64(at))), ("node", n)])
        }
    }
}
