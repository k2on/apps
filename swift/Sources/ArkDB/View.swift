// §13 Views: what a plan means, and a plan kept up to date
// (`rust/ark/src/view.rs`).
//
// `pull` is the one evaluator of plans: a query's value, a mutator's
// `select`, a hydrating view are all what it answers. It returns one
// `Entry` per candidate — a source row the filter admits, or a group —
// carrying its key, the dependencies its subtree read (by plan node), its
// order keys, whether `having` admitted it, and its node. `answer` is the
// list a caller sees; `read` is that answer for a caller that keeps no
// entries.

/// A `Lookup` or a `Related` of a plan tree, by its position in a pre-order walk (`view::NodeId`).
public typealias NodeId = Int

/// One node of a plan tree that reads another table (`view::Node`).
public enum Node {
    case lookup(Lookup)
    case related(Related)

    /// The table this node reads.
    public var table: String {
        switch self {
        case .lookup(let l): return l.table
        case .related(let r): return r.plan.table
        }
    }

    /// The dependency value a row of `table` would satisfy: the row's key for
    /// a lookup, the values of the `on` columns for a related plan — each as a list.
    public func dependency(_ sch: Schema, _ row: Row) -> Value {
        switch self {
        case .lookup(let l): return .list(sch.lookupTable(l.table)?.keyOf(row) ?? [])
        case .related(let r): return .list(r.on.map { row[$0.column] ?? .null })
        }
    }
}

/// A dependency: what a node of a plan read, by the node and the value (`view::Dep`).
public struct Dep: Hashable, Comparable {
    public let id: NodeId
    public let on: Value
    public init(_ id: NodeId, _ on: Value) {
        self.id = id
        self.on = on
    }
    public static func < (a: Dep, b: Dep) -> Bool { a.id != b.id ? a.id < b.id : compareValue(a.on, b.on) < 0 }
}

/// What one aggregate a view keeps comes to (`view::Num`): an integer of any
/// size Rust's `i128` holds, or a column's value.
public enum Num: Hashable {
    case n(Wide)
    case v(Value)
}

/// R9 One reader of kept numbers: a child node, by `(node, dependency, key)`;
/// `nil` stands for the entry's own node.
public struct Reader: Hashable {
    public let id: NodeId
    public let on: Value
    public let key: [Value]
}

/// R9 One child node of a kept plan whose own lists are kept (`view::Kid`).
public struct Kid: Hashable {
    public var row: Value
    public var admitted: Bool
    public var node: Value
    public var subs: [Value]
}

/// R9 What one entry keeps of a kept related plan for one `(node,
/// dependency)` (`view::Held`): the numbers, the child nodes themselves when
/// the child plan has kept lists of its own, and who reads them.
public final class KeptNums: Equatable {
    public var nums: [Num]
    public var kids: SortedMap<[Value], Kid>
    public var parents: Set<Reader?>

    init(_ nums: [Num], _ kids: SortedMap<[Value], Kid>, _ parents: Set<Reader?>) {
        self.nums = nums
        self.kids = kids
        self.parents = parents
    }

    public static func == (a: KeptNums, b: KeptNums) -> Bool {
        a.nums == b.nums && a.kids == b.kids && a.parents == b.parents
    }
}

/// The numbers an entry keeps, by `(node, dependency)` (`view::HeldMap`).
public final class HeldMap: Equatable {
    public var map = SortedMap<Dep, KeptNums>()
    public init() {}

    public subscript(d: Dep) -> KeptNums? {
        get { map[d] }
        set { map[d] = newValue }
    }

    public var keys: [Dep] { map.keys }

    public static func == (a: HeldMap, b: HeldMap) -> Bool {
        a.map.keys == b.map.keys && zip(a.map.values, b.map.values).allSatisfy { $0 == $1 }
    }
}

/// §1.5 One candidate of a plan: a source row the filter admits, or a
/// non-empty group (`view::Entry`).
public struct Entry: Equatable {
    public var key: [Value]
    /// What the subtree read, at every depth, in order.
    public var deps: [Dep]
    /// The order keys' values, in the plan's order.
    public var order: [Value]
    public var admitted: Bool
    /// The node's value; `Null` when not admitted.
    public var node: Value
    /// R9 The numbers kept for each kept related plan beneath it, at every
    /// depth, by `(node, dependency)`; empty for an entry `pull` made.
    public var held: HeldMap
    /// R9 A group's members as numbers, one per aggregate of `Face.members`; empty when a list.
    public var members: [Num]

    public init(_ key: [Value], _ deps: [Dep], _ order: [Value], _ admitted: Bool, _ node: Value, _ held: HeldMap = HeldMap(), _ members: [Num] = []) {
        self.key = key
        self.deps = deps
        self.order = order
        self.admitted = admitted
        self.node = node
        self.held = held
        self.members = members
    }
}

// A filter with its right-hand sides evaluated: one constructor per `Pred`.
indirect enum Filter {
    case cmp(String, CmpOp, Value)
    case inF(String, [Value])
    case all([Filter])
    case any([Filter])
    case not(Filter)
    // D4 The needle, folded once; `nil` for one that is not text, which admits nothing.
    case has(String, String?)
}

// What an entry is built over: a row of the plan's table, or a group's key as
// the struct of its `by` columns.
struct Cand {
    let value: Row
    // A column, as `Value::field` reads one: a bug when it is missing.
    func field(_ c: String) throws -> Value {
        guard let v = value[c] else { throw EvalFault.bug(.noSuchField(c)) }
        return v
    }
}

public enum View {
    /// Every lookup and related node of a plan tree, by `NodeId` (`view::nodes`).
    public static func nodes(_ plan: Plan) -> [Node] {
        var out = [Node]()
        func walk(_ p: Plan) {
            for l in p.lookups { out.append(.lookup(l)) }
            for r in p.related {
                out.append(.related(r))
                walk(r.plan)
            }
        }
        walk(plan)
        return out
    }

    /// How many lookup and related nodes a plan tree has.
    public static func nodeCount(_ plan: Plan) -> Int {
        plan.lookups.count + plan.related.reduce(0) { $0 + 1 + nodeCount($1.plan) }
    }

    static func limitOf(_ plan: Plan) -> Int {
        guard let n = plan.limit else { return Int.max }
        return n <= 0 ? 0 : Int(n)
    }

    /// §1.3 `pull`: every candidate of a plan over a store, ordered.
    public static func pull(_ sch: Schema, _ plan: Plan, _ scope: Scope, _ st: Store) throws -> [Entry] {
        try pullAt(sch, plan, 0, [], scope, st)
    }

    /// `answer`: the admitted entries' nodes, in order, cut to the limit.
    public static func answer(_ plan: Plan, _ entries: [Entry]) -> [Value] {
        let lim = limitOf(plan)
        var out = [Value]()
        for e in entries where e.admitted {
            if out.count >= lim { break }
            out.append(e.node)
        }
        return out
    }

    /// §1.4 `read`: the answer of a pull, for a caller that keeps no entries.
    /// A bare plan's rows are read through the store's indexes in order where
    /// one serves (R1), and otherwise read and sorted.
    public static func read(_ sch: Schema, _ plan: Plan, _ scope: Scope, _ st: Store) throws -> [Value] {
        if !plan.isBare { return answer(plan, try pull(sch, plan, scope, st)) }
        let (tbl, filter) = try source(sch, plan, [], scope)
        let order: [(String, Dir)] = plan.order.map { o in
            guard case .column(let c) = o.key else { fatalError("a bare plan orders by columns") }
            return (c, o.dir)
        }
        let lim = limitOf(plan)
        if lim == 0 { return [] }
        let eq = equalities(filter)
        let sp = spans(filter)
        // A text search is read through its text index and then sorted.
        let searched = needles(filter)?.contains { b in b.contains { !trigrams($0.1).isEmpty } } == true
        if !searched, let rows = st.scanOrdered(plan.table, eq, sp, order, { admits(filter, $0) }, lim) {
            return rows.map { .record($0) }
        }
        let rows = fetch(st, plan.table, filter).sorted { Rules.compareRows(tbl, order, $0, $1) < 0 }
        return (rows.count > lim ? Array(rows[0..<lim]) : rows).map { .record($0) }
    }

    /// Where an entry sorts, as one list: its order keys, then its key (`place`).
    public static func place(_ e: Entry) -> [Value] { e.order + e.key }

    /// `compare_place`: `compareEntries` of the entry a place was taken from, and `e`.
    public static func comparePlace(_ plan: Plan, _ place: [Value], _ e: Entry) -> Int {
        let n = Swift.min(plan.order.count, place.count)
        for (i, kd) in plan.order.enumerated() {
            let o = compareValue(i < place.count ? place[i] : .null, i < e.order.count ? e.order[i] : .null)
            let od = kd.dir == .desc ? -o : o
            if od != 0 { return od }
        }
        return compareKeys(Array(place[n...]), e.key)
    }

    /// §1.5 `compare_entries`: each order key under its direction, then the entries' keys.
    public static func compareEntries(_ plan: Plan, _ a: Entry, _ b: Entry) -> Int {
        for (i, kd) in plan.order.enumerated() {
            let o = compareValue(i < a.order.count ? a.order[i] : .null, i < b.order.count ? b.order[i] : .null)
            let od = kd.dir == .desc ? -o : o
            if od != 0 { return od }
        }
        return compareKeys(a.key, b.key)
    }

    // A plan whose node ids start at `base`, its rows pinned by `pins`.
    static func pullAt(_ sch: Schema, _ plan: Plan, _ base: NodeId, _ pins: [(String, Value)], _ scope: Scope, _ st: Store) throws -> [Entry] {
        let (tbl, filter) = try source(sch, plan, pins, scope)
        let rows = fetch(st, plan.table, filter)
        var out = [Entry]()
        switch plan.source {
        case .table:
            for row in rows { out.append(try entry(sch, plan, base, scope, st, tbl.keyOf(row), Cand(value: row), nil)) }
        case .group(_, let by):
            var groups = SortedMap<[Value], [Value]>()
            let sorted = rows.sorted { compareKeys(tbl.keyOf($0), tbl.keyOf($1)) < 0 }
            for row in sorted { groups.modify(groupOf(by, row), default: { [] }) { $0.append(.record(row)) } }
            for (k, members) in groups {
                out.append(try entry(sch, plan, base, scope, st, k, Cand(value: groupKey(by, k)), .list(members)))
            }
        }
        out = stableSort(out) { compareEntries(plan, $0, $1) }
        return out
    }

    /// A sort that keeps equal elements in their order, as the reference's `sort_by` does.
    static func stableSort<T>(_ xs: [T], _ cmp: (T, T) -> Int) -> [T] {
        xs.enumerated().sorted { a, b in
            let c = cmp(a.element, b.element)
            return c != 0 ? c < 0 : a.offset < b.offset
        }.map { $0.element }
    }

    static func groupOf(_ by: [String], _ row: Row) -> [Value] { by.map { row[$0] ?? .null } }

    static func groupKey(_ by: [String], _ k: [Value]) -> Row { Record(Array(zip(by, k))) }

    // The rows a filter admits, in key order, through whatever indexes serve it.
    static func fetch(_ st: Store, _ table: String, _ filter: Filter?) -> [Row] {
        let eq = equalities(filter)
        let sp = spans(filter)
        guard let has = needles(filter) else { return st.scanWhereEq(table, eq, sp) { admits(filter, $0) } }
        return st.scanWhereText(table, eq, sp, has) { admits(filter, $0) }
    }

    // A plan's table, and its filter and its pins as one filter, evaluated.
    static func source(_ sch: Schema, _ plan: Plan, _ pins: [(String, Value)], _ scope: Scope) throws -> (Table, Filter?) {
        guard let tbl = sch.lookupTable(plan.table) else { throw EvalFault.bug(.unknownTable(plan.table)) }
        var all = [Filter]()
        for (c, v) in pins { all.append(.cmp(c, .eq, v)) }
        if let f = plan.filter { all.append(try evalPred(f) { try scope.eval($0) }) }
        switch all.count {
        case 0: return (tbl, nil)
        case 1: return (tbl, all[0])
        default: return (tbl, .all(all))
        }
    }

    // One candidate, pulled: its lookups, its related plans, its having, its
    // node and its order keys, in that order.
    static func entry(_ sch: Schema, _ plan: Plan, _ base: NodeId, _ scope: Scope, _ st: Store, _ key: [Value], _ row: Cand, _ members: Value?) throws -> Entry {
        var deps = [Dep]()
        // A bare plan binds nothing: its node is the row.
        if plan.isBare {
            let order = try plan.order.map { try orderKey($0.key, row, nil) }
            return Entry(key, deps, order, true, .record(row.value))
        }
        let node = scope.node()
        if let r = plan.row { node.bind(r, .record(row.value)) }
        if let m = plan.members, let ms = members { node.bind(m, ms) }
        var id = base
        for l in plan.lookups {
            let k = try l.key.map { try node.eval($0) }
            try lookUp(node, l, st, k, id, &deps)
            id += 1
        }
        var fields: Row? = plan.project == nil ? row.value : nil
        for r in plan.related {
            let on = try r.on.map { ($0.column, try node.eval($0.e)) }
            deps.append(Dep(id, .list(on.map { $0.1 })))
            let kids = try pullAt(sch, r.plan, id + 1, on, scope, st)
            for kid in kids { deps.append(contentsOf: kid.deps) }
            let list = Value.list(answer(r.plan, kids))
            fields?.set(r.name, list)
            node.bind(r.sym, list)
            id += 1 + nodeCount(r.plan)
        }
        var admitted = true
        if let h = plan.having {
            let v = try node.eval(h)
            guard case .bool(let b) = v else { throw EvalFault.bug(.typeError("a having is a Bool, not \(v.show())")) }
            admitted = b
        }
        let value: Value
        if !admitted {
            value = .null
        } else if let p = plan.project {
            value = try node.eval(p)
        } else {
            value = .record(fields ?? Record())
        }
        let order = try plan.order.map { try orderKey($0.key, row, node) }
        return Entry(key, deps, order, admitted, value)
    }

    // A lookup, bound: the row under its key, or `Null` — read, and recorded as
    // a dependency, only when no part of the key is `Null`.
    private static func lookUp(_ node: NodeScope, _ l: Lookup, _ st: Store, _ k: [Value], _ id: NodeId, _ deps: inout [Dep]) throws {
        if k.contains(where: { $0.isNull }) {
            node.bind(l.sym, .null)
            return
        }
        node.bind(l.sym, st.get(l.table, k).map { .record($0) } ?? .null)
        deps.append(Dep(id, .list(k)))
    }

    private static func orderKey(_ k: Key, _ row: Cand, _ node: NodeScope?) throws -> Value {
        switch k {
        case .column(let c): return try row.field(c)
        case .expr(let e):
            guard let n = node else { throw EvalFault.bug(.typeError("an expression order key on a bare plan")) }
            return try n.eval(e)
        }
    }

    // The filter, evaluated --------------------------------------------------

    static func evalPred(_ p: Pred, _ ev: (Expr) throws -> Value) throws -> Filter {
        switch p {
        case .cmp(let c, let o, let e): return .cmp(c, o, try ev(e))
        case .inP(let c, let xs): return .inF(c, try xs.map(ev))
        case .all(let ps): return .all(try ps.map { try evalPred($0, ev) })
        case .any(let ps): return .any(try ps.map { try evalPred($0, ev) })
        case .not(let q): return .not(try evalPred(q, ev))
        case .has(let c, let e): return .has(c, try ev(e).textValue.map(fold))
        // D2 A scope's leaves, refused in a plan by the verifier: `When` is the
        // constant it evaluates to, and `Exists`, which needs a store, admits nothing.
        case .when(let e): return try ev(e) == .bool(true) ? .all([]) : .any([])
        case .exists: return .any([])
        }
    }

    // D4 The text searches a filter holds however it is satisfied, as a
    // disjunction of conjunctions of (column, folded needle). Past sixteen
    // branches it says nothing, which is only a wider read.
    static func needles(_ f: Filter?) -> [[(String, String)]]? {
        let most = 16
        func go(_ f: Filter) -> [[(String, String)]]? {
            switch f {
            case .has(let c, let n):
                guard let n = n else { return nil }
                return [[(c, n)]]
            case .all(let fs):
                var out: [[(String, String)]]? = nil
                for g in fs.compactMap(go) {
                    guard let bs = out else {
                        out = g
                        continue
                    }
                    var prod = [[(String, String)]]()
                    for b in bs { for h in g { prod.append(b + h) } }
                    if prod.count > most { return bs }
                    out = prod
                }
                return out
            case .any(let fs):
                let parts = fs.map(go)
                if parts.contains(where: { $0 == nil }) { return nil }
                let bs = parts.flatMap { $0! }
                return bs.count <= most ? bs : nil
            default: return nil
            }
        }
        guard let f = f, let r = go(f), !r.isEmpty else { return nil }
        return r
    }

    // The columns a filter holds equal to a value however it is satisfied.
    static func equalities(_ f: Filter?) -> [Eq] {
        var out = [Eq]()
        func go(_ f: Filter) {
            switch f {
            case .cmp(let c, .eq, let v): out.append((c, v))
            case .all(let fs): fs.forEach(go)
            default: break
            }
        }
        if let f = f { go(f) }
        return out
    }

    // The columns a filter holds between bounds, one span per column with the
    // tightest bound of each side (R6).
    static func spans(_ f: Filter?) -> [Span] {
        var out = [Span]()
        // Whether `new` narrows past `old`, `inward` the direction a bound moves to narrow.
        func tighter(_ new: Bound, _ old: Bound, _ inward: Int) -> Bool {
            guard let o = old.value else { return true }
            guard let n = new.value else { return false }
            let by = compareValue(n, o).signum()
            if by == inward { return true }
            if by == 0, case .excluded = new { return true }
            return false
        }
        func go(_ f: Filter) {
            switch f {
            case .cmp(let c, let op, let v) where op == .ge || op == .gt || op == .le || op == .lt:
                var i = out.firstIndex { $0.column == c } ?? -1
                if i < 0 {
                    out.append(Span(column: c, lo: .unbounded, hi: .unbounded))
                    i = out.count - 1
                }
                if op == .ge || op == .gt {
                    let new: Bound = op == .ge ? .included(v) : .excluded(v)
                    if tighter(new, out[i].lo, 1) { out[i].lo = new }
                } else {
                    let new: Bound = op == .le ? .included(v) : .excluded(v)
                    if tighter(new, out[i].hi, -1) { out[i].hi = new }
                }
            case .all(let fs): fs.forEach(go)
            default: break
            }
        }
        if let f = f { go(f) }
        return out
    }

    // Whether a row passes the filter; no filter admits every row. A column
    // the row lacks reads as `Null`.
    static func admits(_ f: Filter?, _ row: Row) -> Bool {
        func go(_ f: Filter) -> Bool {
            switch f {
            case .cmp(let c, let o, let v): return cmp(o, row[c] ?? .null, v)
            case .inF(let c, let vs):
                let x = row[c] ?? .null
                return vs.contains { x == $0 }
            case .all(let fs): return fs.allSatisfy(go)
            case .any(let fs): return fs.contains(where: go)
            case .not(let g): return !go(g)
            // D4 Both sides folded; a `Null` holds nothing, and every text the empty needle.
            case .has(let c, let needle):
                guard case .text(let s)? = row[c], let n = needle else { return false }
                if n.isEmpty { return true }
                return textContains(fold(s), n)
            }
        }
        guard let f = f else { return true }
        return go(f)
    }
}
