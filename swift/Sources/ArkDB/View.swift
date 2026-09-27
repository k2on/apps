import Foundation

/// §13.1 A filter with its right-hand sides evaluated: one case per `Pred`.
public indirect enum Filter: Equatable {
    case fcmp(FieldName, CmpOp, Value)
    case fin(FieldName, [Value])
    case fall([Filter])
    case fany([Filter])
    case fnot(Filter)
}

public struct ViewRelated: Equatable {
    public var name: FieldName
    public var relation: Relation
    public var plan: ViewPlan
    public init(name: FieldName, relation: Relation, plan: ViewPlan) {
        self.name = name; self.relation = relation; self.plan = plan
    }
}

/// `Plan` with every right-hand side evaluated, to any depth.
public struct ViewPlan: Equatable {
    public var table: TableName
    public var filter: Filter?
    public var order: [OrderBy]
    public var limit: Int?
    public var related: [ViewRelated]
    public init(table: TableName, filter: Filter? = nil, order: [OrderBy] = [], limit: Int? = nil, related: [ViewRelated] = []) {
        self.table = table; self.filter = filter; self.order = order; self.limit = limit; self.related = related
    }

    /// Resolve a plan with an evaluator for its right-hand sides.
    public static func evalPlan(_ p: Plan, _ ev: (Expr) throws -> Value) throws -> ViewPlan {
        let f = try p.filter.map { try evalPred($0, ev) }
        let rels = try p.related.map { ViewRelated(name: $0.name, relation: $0.relation, plan: try evalPlan($0.plan, ev)) }
        return ViewPlan(table: p.table, filter: f, order: p.order, limit: p.limit, related: rels)
    }

    static func evalPred(_ p: Pred, _ ev: (Expr) throws -> Value) throws -> Filter {
        switch p {
        case .pcmp(let c, let op, let e): return .fcmp(c, op, try ev(e))
        case .pin(let c, let es): return .fin(c, try es.map(ev))
        case .pall(let ps): return .fall(try ps.map { try evalPred($0, ev) })
        case .pany(let ps): return .fany(try ps.map { try evalPred($0, ev) })
        case .pnot(let q): return .fnot(try evalPred(q, ev))
        }
    }
}

/// Whether a row passes the filter; no filter admits every row.
public func admits(_ f: Filter?, _ row: Row) -> Bool {
    guard let f = f else { return true }
    func go(_ f: Filter) -> Bool {
        switch f {
        case .fcmp(let c, let op, let v): return Ops.compare(op, row[c] ?? .null, v)
        case .fin(let c, let vs): return vs.contains { Ops.compare(.eq, row[c] ?? .null, $0) }
        case .fall(let fs): return fs.allSatisfy(go)
        case .fany(let fs): return fs.contains(where: go)
        case .fnot(let g): return !go(g)
        }
    }
    return go(f)
}

/// §13.2 The order a view keeps its rows in: the plan's order, then the key
/// ascending, as a total comparison.
public func compareRows(_ tbl: Table, _ cols: [OrderBy], _ a: Row, _ b: Row) -> Int {
    let o = Eval.orderBy(cols, a, b)
    if o != 0 { return o }
    return compareValue(.list(tbl.keyOf(a)), .list(tbl.keyOf(b)))
}

/// A node kept beside the row it was built over.
public struct Node: Equatable {
    public var row: Row
    public var node: Value
}

/// §13.4 What `push` did to the view's list, as positions into the list as
/// it stands when the patch is applied.
public enum Patch: Equatable {
    case insert(at: Int, node: Value)
    case remove(at: Int)
    case update(at: Int, node: Value)

    /// The patch as the vectors write it.
    public var value: Value {
        switch self {
        case .insert(let i, let n): return .record(["t": .text("insert"), "at": .int(Int64(i)), "node": n])
        case .remove(let i): return .record(["t": .text("remove"), "at": .int(Int64(i))])
        case .update(let i, let n): return .record(["t": .text("update"), "at": .int(Int64(i)), "node": n])
        }
    }

    /// What a patch means to whoever holds a copy of the list.
    public static func splice(_ ps: [Patch], _ xs: [Value]) -> [Value] {
        var vs = xs
        for p in ps {
            switch p {
            case .insert(let i, let v): vs.insert(v, at: i)
            case .remove(let i): vs.remove(at: i)
            case .update(let i, let v): vs[i] = v
            }
        }
        return vs
    }
}

/// §13.3 A maintained plan: the plan and its nodes, in the plan's order.
public struct View: Equatable {
    public var plan: ViewPlan
    public var nodes: [Node]

    /// Pull everything: what `select` answers for the plan, as a view.
    public static func hydrate(_ sch: Schema, _ vp: ViewPlan, _ st: Store) -> View {
        return View(plan: vp, nodes: pull(sch, vp, st))
    }

    /// A rebase rolled the store back: hydrate again.
    public mutating func rebuild(_ sch: Schema, _ st: Store) {
        self = View.hydrate(sch, plan, st)
    }

    /// The current nodes, in order.
    public var rows: [Value] { return nodes.map { $0.node } }

    static func pull(_ sch: Schema, _ vp: ViewPlan, _ st: Store) -> [Node] {
        guard let tbl = sch.lookupTable(vp.table) else { return [] }
        let admitted = st.scan(vp.table).filter { admits(vp.filter, $0) }
        let ordered = stableSort(admitted) { compareRows(tbl, vp.order, $0, $1) }
        let taken = vp.limit.map { Array(ordered.prefix(Swift.max(0, $0))) } ?? ordered
        return taken.map { Node(row: $0, node: nodeOf(sch, vp, st, tbl, $0)) }
    }

    /// A node over a row: the columns, plus one field per relationship.
    static func nodeOf(_ sch: Schema, _ vp: ViewPlan, _ st: Store, _ tbl: Table, _ row: Row) -> Value {
        let pk = parentKey(tbl, row)
        var m = row
        for r in vp.related {
            let pin = Filter.fcmp(r.relation.column, .eq, pk)
            var child = r.plan
            child.filter = child.filter.map { .fall([pin, $0]) } ?? pin
            m[r.name] = .list(pull(sch, child, st).map { $0.node })
        }
        return .record(m)
    }

    static func parentKey(_ tbl: Table, _ row: Row) -> Value {
        let ks = tbl.keyOf(row)
        return ks.count == 1 ? ks[0] : .list(ks)
    }

    /// §13.5 A change arrives; the store is already at the new state.
    public mutating func push(_ sch: Schema, _ st: Store, _ ch: Change) -> [Patch] {
        guard let tbl = sch.lookupTable(plan.table) else { return [] }
        var ps: [Patch] = []
        if ch.table == plan.table { ps += pushTop(sch, st, tbl, ch) }
        ps += pushBelow(sch, st, tbl, ch)
        return ps
    }

    mutating func pushTop(_ sch: Schema, _ st: Store, _ tbl: Table, _ ch: Change) -> [Patch] {
        let vp = plan
        let before = nodes
        let keep: (Row) -> Bool = { admits(vp.filter, $0) }
        let key: (Row) -> [Value] = { tbl.keyOf($0) }
        let order: (Row, Row) -> Int = { compareRows(tbl, vp.order, $0, $1) }
        func position(_ row: Row) -> Int? {
            let k = key(row)
            return before.firstIndex { key($0.row) == k }
        }
        func insertPos(_ row: Row, _ ns: [Node]) -> Int {
            var j = 0
            while j < ns.count && order(ns[j].row, row) < 0 { j += 1 }
            return j
        }
        func build(_ row: Row) -> Node { return Node(row: row, node: View.nodeOf(sch, vp, st, tbl, row)) }
        let full = vp.limit != nil && vp.limit! == before.count
        /// The first admitted row beyond the bound.
        func refill(_ ns: [Node]) -> Node? {
            let bound = ns.last?.row
            let candidates = st.scan(vp.table).filter { r in keep(r) && (bound == nil || order(bound!, r) < 0) }
            guard let r = stableSort(candidates, order).first else { return nil }
            return build(r)
        }
        func add(_ row: Row) -> [Patch] {
            if !keep(row) { return [] }
            let j = insertPos(row, before)
            if let lim = vp.limit, j >= lim { return [] }
            let n = build(row)
            var ns = before
            ns.insert(n, at: j)
            if let lim = vp.limit, ns.count > lim {
                nodes = Array(ns.prefix(lim))
                return [.insert(at: j, node: n.node), .remove(at: lim)]
            }
            nodes = ns
            return [.insert(at: j, node: n.node)]
        }
        func removeAt(_ i: Int) -> [Patch] {
            var ns = before
            ns.remove(at: i)
            if full, let n = refill(ns) {
                nodes = ns + [n]
                return [.remove(at: i), .insert(at: ns.count, node: n.node)]
            }
            nodes = ns
            return [.remove(at: i)]
        }
        func edit(_ i: Int, _ new: Row) -> [Patch] {
            var ns = before
            ns.remove(at: i)
            let j = insertPos(new, ns)
            let n = build(new)
            let hidden: Node? = (full && j == ns.count) ? refill(ns) : nil
            if let h = hidden, key(h.row) != key(new) {
                nodes = ns + [h]
                return [.remove(at: i), .insert(at: j, node: h.node)]
            }
            ns.insert(n, at: j)
            nodes = ns
            if j == i { return [.update(at: i, node: n.node)] }
            return [.remove(at: i), .insert(at: j, node: n.node)]
        }
        switch ch {
        case .add(_, let row): return add(row)
        case .remove(_, let row):
            guard let i = position(row) else { return [] }
            return removeAt(i)
        case .edit(_, let old, let new):
            switch (position(old), keep(new)) {
            case (nil, false): return []
            case (nil, true): return add(new)
            case (let i?, false): return removeAt(i)
            case (let i?, true): return edit(i, new)
            }
        }
    }

    /// §13.6 A change beneath the plan: an update of every parent node it
    /// could have moved.
    mutating func pushBelow(_ sch: Schema, _ st: Store, _ tbl: Table, _ ch: Change) -> [Patch] {
        let vp = plan
        let t = ch.table
        let direct = vp.related.filter { $0.relation.child == t }.map { $0.relation.column }
        func descendants(_ c: ViewPlan) -> [TableName] {
            return c.related.map { $0.plan.table } + c.related.flatMap { descendants($0.plan) }
        }
        let deeper = vp.related.flatMap { descendants($0.plan) }.contains(t)
        if direct.isEmpty && !deeper { return [] }
        var joins = Set<Value>()
        for col in direct { for row in changedRows(ch) { if let v = row[col] { joins.insert(v) } } }
        var ps: [Patch] = []
        for i in nodes.indices {
            let row = nodes[i].row
            let affected = deeper || joins.contains(View.parentKey(tbl, row))
            if !affected { continue }
            let new = View.nodeOf(sch, vp, st, tbl, row)
            if new != nodes[i].node {
                nodes[i].node = new
                ps.append(.update(at: i, node: new))
            }
        }
        return ps
    }

    func changedRows(_ ch: Change) -> [Row] {
        switch ch {
        case .add(_, let r), .remove(_, let r): return [r]
        case .edit(_, let o, let n): return [o, n]
        }
    }

    /// §13.7 The contract: indistinguishable from a view hydrated now.
    public static func contract(_ sch: Schema, _ vp: ViewPlan, _ st: Store, _ view: View) -> Bool {
        return view == hydrate(sch, vp, st)
    }
}
