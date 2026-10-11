// §4 The store, as `rust/ark/src/store.rs` defines it: an ordered
// key-value backend under get, scan, put, delete and the commit a batch of
// them makes. Not-null, uniqueness and references are enforced here,
// identically everywhere, as deterministic refusals; an index changes
// nothing but what a read costs.
//
// A row is a `Record` of every column (§4). The Rust lays a row out
// positionally behind its table's names (`docs/plan-perf.md` R11), which is
// representation: "a row laid out as the table's" is, here, a record whose
// fields are exactly the table's columns (`Table.holds`), and a row of no
// table is any other record.

/// §4.1 What a write reports (`store::Change`).
public enum Change: Hashable {
    case add(String, Row)
    case remove(String, Row)
    /// Old, new.
    case edit(String, Row, Row)

    public var table: String {
        switch self {
        case .add(let t, _), .remove(let t, _), .edit(let t, _, _): return t
        }
    }

    /// The row before, if the change had one.
    public var old: Row? {
        switch self {
        case .add: return nil
        case .remove(_, let r): return r
        case .edit(_, let o, _): return o
        }
    }

    /// The row after, if the change leaves one.
    public var new: Row? {
        switch self {
        case .add(_, let r): return r
        case .remove: return nil
        case .edit(_, _, let n): return n
        }
    }
}

/// §4.2 A refusal: a verdict about the write, reached identically by every
/// replica (`store::Refusal`).
public enum Refusal: Hashable, Error {
    case noSuchTable(String)
    case malformedRow(String, String)
    case notNull(String, String)
    case uniqueViolation(String, [String])
    case missingParent(String, String, String)
    case stillReferenced(String, String)
    /// An explicit `refuse`, or a checked arithmetic fault.
    case refused(String)
    /// Not this login's to do, naming the table it would have written (`docs/plan-guards.md` D4).
    case forbidden(String)

    /// `impl Display for Refusal`: the text a server puts in a `reject` frame.
    public func show() -> String {
        switch self {
        case .noSuchTable(let t): return "NoSuchTable \(debugString(t))"
        case .malformedRow(let t, let w): return "MalformedRow \(debugString(t)) \(debugString(w))"
        case .notNull(let t, let c): return "NotNull \(debugString(t)) \(debugString(c))"
        case .uniqueViolation(let t, let cs): return "UniqueViolation \(debugString(t)) \(debugStrings(cs))"
        case .missingParent(let t, let c, let p): return "MissingParent \(debugString(t)) \(debugString(c)) \(debugString(p))"
        case .stillReferenced(let t, let c): return "StillReferenced \(debugString(t)) \(debugString(c))"
        case .refused(let w): return "Refused \(debugString(w))"
        case .forbidden(let t): return "Forbidden \(debugString(t))"
        }
    }
}

/// A bound of a span, under the total order (`std::ops::Bound`).
public enum Bound: Hashable {
    case unbounded
    case included(Value)
    case excluded(Value)

    var value: Value? {
        switch self {
        case .unbounded: return nil
        case .included(let v), .excluded(let v): return v
        }
    }
}

/// One column a read's filter holds between two bounds (`store::Span`,
/// `docs/plan-perf.md` R6). A hint, as the equalities are: `keep` still decides.
public struct Span: Hashable {
    public var column: String
    public var lo: Bound
    public var hi: Bound
    public init(column: String, lo: Bound, hi: Bound) {
        self.column = column
        self.lo = lo
        self.hi = hi
    }
}

/// A column held equal to a value.
public typealias Eq = (column: String, value: Value)

/// A row filter, as the store is handed one.
public typealias Keep = (Row) -> Bool

/// What a write did: a change (or none), or the refusal that stopped it.
public enum Written {
    case ok(Change?)
    case refused(Refusal)
}

/// The store's interface (`store::Store`): a backend implements the
/// primitives, and the constraints and the reads are given. `scan` returns
/// rows in key order under the total order, and a table with no rows is
/// empty rather than absent.
public protocol Store: AnyObject {
    var schema: Schema { get }

    /// The row under a key, if any.
    func get(_ table: String, _ key: [Value]) -> Row?

    /// Every row of a table, in key order.
    func scan(_ table: String) -> [Row]

    /// The rows `keep` admits, in key order.
    func scanWhere(_ table: String, _ keep: Keep) -> [Row]

    /// `scan_where_eq`: told which columns `keep` holds equal and which between bounds (hints).
    func scanWhereEq(_ table: String, _ eq: [Eq], _ spans: [Span], _ keep: Keep) -> [Row]

    /// `scan_ordered` (R1): the first `limit` rows `keep` admits in `order` —
    /// each column under its direction, then the key ascending — read from an
    /// index that holds them so; `nil` when none does.
    func scanOrdered(_ table: String, _ eq: [Eq], _ spans: [Span], _ order: [(String, Dir)], _ keep: Keep, _ limit: Int) -> [Row]?

    /// `docs/plan-db.md` D4 `scan_where_text`: told also which text columns the
    /// filter holds to contain which (folded) needles, as a disjunction of
    /// conjunctions. A hint; `keep` still decides.
    func scanWhereText(_ table: String, _ eq: [Eq], _ spans: [Span], _ has: [[(String, String)]], _ keep: Keep) -> [Row]

    /// §4.5 Apply a change as a fact: raw, unjudged. An unknown table is ignored.
    func applyChange(_ change: Change)

    func exists(_ table: String, _ key: [Value]) -> Bool

    /// §8.1 The digest this store keeps for a table, if it keeps one.
    func digest(_ table: String) -> Digest?
}

extension Store {
    public func applyChanges(_ changes: [Change]) { for c in changes { applyChange(c) } }

    /// §4.3 `put`: completed, judged, and written. A new key is an `Add`, an equal row nothing.
    public func put(_ table: String, _ row: Row) -> Written {
        let w = Rules.judgePut(self, table, row)
        if case .ok(let c?) = w { applyChange(c) }
        return w
    }

    /// §4.4 `delete` by key: a missing row is nothing; a referenced one a refusal.
    public func delete(_ table: String, _ key: [Value]) -> Written {
        let w = Rules.judgeDelete(self, table, key)
        if case .ok(let c?) = w { applyChange(c) }
        return w
    }

    /// The row under a key as a value, or `Null`.
    public func getValue(_ table: String, _ key: [Value]) -> Value { get(table, key).map { .record($0) } ?? .null }

    /// Every table's rows, as the vectors write a store: table name to the list of rows.
    public func storeValue() -> Value {
        .record(Record(schema.tables.map { t in (t.name, Value.list(scan(t.name).map { .record($0) })) }))
    }
}

/// The rules every backend judges a write by (`store.rs`, "The rules").
public enum Rules {
    /// `complete`: every nullable column the row left out, as `Null`. A row
    /// that names a column the table lacks or leaves out one it requires
    /// stays as it is otherwise, for `judgePut` to refuse.
    public static func complete(_ tbl: Table, _ row: Row) -> Row {
        if tbl.holds(row) { return row }
        let missing = tbl.columns.filter { $0.nullable && !row.has($0.name) }
        if missing.isEmpty { return row }
        var r = row
        for c in missing { r.set(c.name, .null) }
        return r
    }

    /// `row_for`: a struct written to a table, as the table's row — laid out,
    /// its omitted nullable columns `Null`, when it names only columns and
    /// leaves out only nullable ones; as it is otherwise.
    public static func rowFor(_ st: Store, _ table: String, _ m: Row) -> Row {
        guard let tbl = st.schema.lookupTable(table) else { return m }
        let fits = m.names.allSatisfy { tbl.column($0) != nil } && tbl.columns.allSatisfy { $0.nullable || m.has($0.name) }
        return fits ? complete(tbl, m) : m
    }

    /// `project_row` (§12 behind): a fact's row laid out as `tbl`'s — a column
    /// the table lacks dropped, a nullable column the row lacks `Null` — or a
    /// refusal for a required column it lacks.
    public static func projectRow(_ tbl: Table, _ row: Row) -> Result<Row, Refusal> {
        if tbl.holds(row) { return .success(row) }
        var out = [(String, Value)]()
        for c in tbl.columns {
            if let v = row[c.name] {
                out.append((c.name, v))
            } else if c.nullable {
                out.append((c.name, .null))
            } else {
                return .failure(.malformedRow(tbl.name, "\(c.name) is missing"))
            }
        }
        return .success(Record(out))
    }

    /// `judge_put`: what `put` would report, without writing.
    public static func judgePut(_ st: Store, _ tn: String, _ row0: Row) -> Written {
        guard let tbl = st.schema.lookupTable(tn) else { return .refused(.noSuchTable(tn)) }
        let row = complete(tbl, row0)
        if let r = wellTyped(tbl, row) { return .refused(r) }
        let k = tbl.keyOf(row)
        for ix in tbl.indexes where ix.unique {
            if let r = unique(st, tbl, k, row, ix) { return .refused(r) }
        }
        for r in tbl.refs {
            if let x = parentExists(st, tbl, row, r) { return .refused(x) }
        }
        guard let old = st.get(tn, k) else { return .ok(.add(tn, row)) }
        if old == row { return .ok(nil) }
        return .ok(.edit(tn, old, row))
    }

    /// `matching`: the row a write's column list matches (the key when empty), if any.
    public static func matching(_ st: Store, _ tbl: Table, _ row: Row, _ on: [String]) -> Row? {
        if on.isEmpty { return st.get(tbl.name, tbl.keyOf(row)) }
        let want: [Eq] = on.map { ($0, row[$0] ?? .null) }
        if want.contains(where: { $0.value.isNull }) { return nil }
        return st.scanWhereEq(tbl.name, want, []) { r in want.allSatisfy { r[$0.column] == $0.value } }.first
    }

    /// §1.4 `insert`: the row, unless one matches on the columns.
    public static func insert(_ st: Store, _ tn: String, _ row0: Row, _ on: [String]) -> Written {
        guard let tbl = st.schema.lookupTable(tn) else { return .refused(.noSuchTable(tn)) }
        let row = complete(tbl, row0)
        if matching(st, tbl, row, on) != nil { return .ok(nil) }
        return st.put(tn, row)
    }

    /// §1.4 `upsert`: the row; if one matches on the columns, its key columns kept.
    public static func upsert(_ st: Store, _ tn: String, _ row0: Row, _ on: [String]) -> Written {
        guard let tbl = st.schema.lookupTable(tn) else { return .refused(.noSuchTable(tn)) }
        var row = complete(tbl, row0)
        if let old = matching(st, tbl, row, on) {
            for k in tbl.key {
                if let v = old[k], row.has(k) { row.set(k, v) }
            }
        }
        return st.put(tn, row)
    }

    /// §1.4 `update`, once the new row is computed: it must keep the key.
    public static func update(_ st: Store, _ tn: String, _ key: [Value], _ row0: Row) -> Written {
        guard let tbl = st.schema.lookupTable(tn) else { return .refused(.noSuchTable(tn)) }
        let row = complete(tbl, row0)
        if tbl.keyOf(row) != key { return .refused(.malformedRow(tn, "update changed the key")) }
        return st.put(tn, row)
    }

    /// `judge_delete`: what `delete` would report, without writing.
    public static func judgeDelete(_ st: Store, _ tn: String, _ k: [Value]) -> Written {
        guard let tbl = st.schema.lookupTable(tn) else { return .refused(.noSuchTable(tn)) }
        guard let row = st.get(tn, k) else { return .ok(nil) }
        for rel in st.schema.childrenOf(tn) where k.count == 1 {
            let kv = k[0]
            let held = st.scanWhereEq(rel.child, [(rel.column, kv)], []) { $0[rel.column] == kv }
            if !held.isEmpty { return .refused(.stillReferenced(tbl.name, rel.child)) }
        }
        return .ok(.remove(tn, row))
    }

    // A row is exactly the table's columns, each of the column's type.
    private static func wellTyped(_ tbl: Table, _ row: Row) -> Refusal? {
        if !tbl.holds(row) {
            let have = row.names
            let want = tbl.columns.map { $0.name }
            return .malformedRow(tbl.name, "columns \(debugStrings(have)) are not \(debugStrings(want))")
        }
        for c in tbl.columns {
            let v = row[c.name]!
            if v.isNull {
                if !c.nullable { return .notNull(tbl.name, c.name) }
            } else if !ofType(c.ty, v) {
                return .malformedRow(tbl.name, "\(c.name) has the wrong type")
            }
        }
        return nil
    }

    /// `of_type`: whether a value inhabits a scalar type. Ids are untyped at run time.
    public static func ofType(_ t: Ty, _ v: Value) -> Bool {
        switch (t, v) {
        case (.bool, .bool), (.int, .int), (.text, .text), (.bytes, .bytes), (.id, .id): return true
        case (.enumeration(let vs), .text(let s)): return vs.contains { textEquals($0, s) }
        case (.option(let o), _): return v.isNull || ofType(o, v)
        default: return false
        }
    }

    /// `compare_rows`: each column under its direction, then the key ascending.
    public static func compareRows(_ tbl: Table, _ order: [(String, Dir)], _ a: Row, _ b: Row) -> Int {
        for (c, d) in order {
            let o = compareValue(a[c] ?? .null, b[c] ?? .null)
            let od = d == .desc ? -o : o
            if od != 0 { return od }
        }
        for k in tbl.key {
            let o = compareValue(a[k] ?? .null, b[k] ?? .null)
            if o != 0 { return o }
        }
        return 0
    }

    // No other row holds the index's columns equal to this row's.
    private static func unique(_ st: Store, _ tbl: Table, _ k: [Value], _ row: Row, _ ix: Index) -> Refusal? {
        let mine: [Eq] = ix.columns.map { ($0, row[$0] ?? .null) }
        // A NULL equals nothing, itself included.
        if mine.contains(where: { $0.value.isNull }) { return nil }
        let clash = st.scanWhereEq(tbl.name, mine, []) { r in tbl.keyOf(r) != k && mine.allSatisfy { r[$0.column] == $0.value } }
        return clash.isEmpty ? nil : .uniqueViolation(tbl.name, ix.columns)
    }

    private static func parentExists(_ st: Store, _ tbl: Table, _ row: Row, _ r: Ref) -> Refusal? {
        guard let v = row[r.column] else { return .malformedRow(tbl.name, r.column) }
        if v.isNull { return nil }
        return st.exists(r.table, [v]) ? nil : .missingParent(tbl.name, r.column, r.table)
    }
}

/// `docs/plan-db.md` D4 A text, folded as `lower` folds it: each character to
/// its simple lowercase mapping under the pinned Unicode tables, one for one
/// (`store::fold`).
public func fold(_ s: String) -> String {
    var ascii = true
    for b in s.utf8 where b >= 0x80 || (b >= 65 && b <= 90) {
        ascii = false
        break
    }
    if ascii { return s }
    var out = String.UnicodeScalarView()
    for u in s.unicodeScalars { out.append(Unicode.Scalar(UnicodeTables.toLowerSimple(u.value))!) }
    return String(out)
}

/// Whether a text holds another, code point for code point (a substring of
/// UTF-8 bytes, which for valid UTF-8 is the same thing) — never Foundation's
/// `contains`, which matches canonical equivalents.
public func textContains(_ s: String, _ needle: String) -> Bool { textIndex(s, needle) != nil }

/// The byte offset at which `needle` first occurs in `s`, by UTF-8 bytes.
public func textIndex(_ s: String, _ needle: String) -> Int? {
    let a = Array(s.utf8)
    let n = Array(needle.utf8)
    if n.isEmpty { return 0 }
    if n.count > a.count { return nil }
    var i = 0
    let first = n[0]
    while i + n.count <= a.count {
        if a[i] == first {
            var j = 1
            while j < n.count && a[i + j] == n[j] { j += 1 }
            if j == n.count { return i }
        }
        i += 1
    }
    return nil
}

/// D4 The trigrams of a folded text: every run of three characters (not bytes), once each.
public func trigrams(_ folded: String) -> SortedSet<CodePoints> {
    let cps = Array(folded.unicodeScalars)
    var out = SortedSet<CodePoints>()
    if cps.count < 3 { return out }
    for i in 0...(cps.count - 3) {
        var v = String.UnicodeScalarView()
        v.append(cps[i])
        v.append(cps[i + 1])
        v.append(cps[i + 2])
        out.insert(CodePoints(String(v)))
    }
    return out
}

// One secondary index: the keys of the rows under each value of its columns.
// The Rust posts ordinals (D7.2); keys answer the same reads, in key order.
private struct Secondary {
    let columns: [String]
    var rows = SortedMap<[Value], SortedSet<[Value]>>()

    func keyOf(_ row: Row) -> [Value] { columns.map { row[$0] ?? .null } }

    mutating func set(_ key: [Value], _ old: Row?, _ new: Row?) {
        let was = old.map(keyOf)
        let now = new.map(keyOf)
        if was == now { return }
        if let w = was {
            let empty = rows.modifyIfPresent(w) { list -> Bool in
                list.remove(key)
                return list.isEmpty
            }
            if empty == true { rows.remove(w) }
        }
        if let n = now { rows.modify(n, default: { SortedSet() }) { _ = $0.insert(key) } }
    }

    // The positions of the buckets whose values begin with `prefix` and,
    // given a span on the next column, lie between its bounds. `prefix ++
    // [v, []]` is above every bucket continuing `prefix ++ [v]` (a list
    // outranks every scalar).
    func under(_ prefix: [Value], _ span: Span?) -> Range<Int> {
        func at(_ v: Value?, _ top: Bool) -> [Value] {
            var k = prefix
            if let v = v { k.append(v) }
            if top { k.append(.list([])) }
            return k
        }
        let lo = span?.lo ?? .unbounded
        let hi = span?.hi ?? .unbounded
        var l: [Value]
        var lIncl: Bool
        switch lo {
        case .unbounded: (l, lIncl) = (at(nil, false), true)
        case .included(let v): (l, lIncl) = (at(v, false), true)
        case .excluded(let v): (l, lIncl) = (at(v, true), false)
        }
        var h: [Value]
        switch hi {
        case .unbounded: h = at(nil, true)
        case .included(let v): h = at(v, true)
        case .excluded(let v): h = at(v, false)
        }
        if compareKeys(l, h) >= 0 { return 0..<0 }
        let start = lIncl ? rows.lowerBound(l) : rows.upperBound(l)
        let end = rows.lowerBound(h)
        return start < end ? start..<end : start..<start
    }

    // R1 Whether this index holds a read's rows in the read's order: how many
    // leading columns the read holds equal, and which way to walk it.
    func serves(_ tbl: Table, _ eq: [Eq], _ order: [(String, Dir)]) -> (Int, Dir)? {
        func isEq(_ c: String) -> Bool { eq.contains { $0.column == c } }
        var n = 0
        while n < columns.count && isEq(columns[n]) { n += 1 }
        let lead = columns[0..<n]
        let rest = Array(columns[n...])
        if !eq.allSatisfy({ lead.contains($0.column) }) { return nil }
        var ord = order.filter { !isEq($0.0) }.makeIterator()
        var dir: Dir? = nil
        for c in rest {
            guard let (o, d) = ord.next() else { return nil }
            if o != c || (dir != nil && dir != d) { return nil }
            dir = d
        }
        var tail = tbl.key.filter { !isEq($0) && !rest.contains($0) }.makeIterator()
        while let (o, d) = ord.next() {
            guard d == .asc, let t = tail.next(), t == o else { return nil }
        }
        return (n, dir ?? .asc)
    }
}

// D4 A text index: for each trigram of one text column's folded value, the
// keys of the rows whose value holds it.
private struct Trigrams {
    let column: String
    var postings = [CodePoints: SortedSet<[Value]>]()

    func of(_ row: Row?) -> SortedSet<CodePoints> {
        if case .text(let s)? = row?[column] { return trigrams(fold(s)) }
        return SortedSet()
    }

    mutating func set(_ key: [Value], _ old: Row?, _ new: Row?) {
        if old?[column] == new?[column] && (old != nil) == (new != nil) { return }
        let was = of(old)
        let now = of(new)
        for g in was where !now.contains(g) {
            if postings[g] != nil {
                postings[g]!.remove(key)
                if postings[g]!.isEmpty { postings[g] = nil }
            }
        }
        for g in now where !was.contains(g) { postings[g, default: SortedSet()].insert(key) }
    }
}

// One table: its rows by key, its secondaries and text indexes, and its
// digest (§8.1), which moves with every row written.
private final class TableState {
    var rows: SortedMap<[Value], Row>
    var indexes: [Secondary]
    var texts: [Trigrams]
    var digest: Digest

    init(rows: SortedMap<[Value], Row>, indexes: [Secondary], texts: [Trigrams], digest: Digest) {
        self.rows = rows
        self.indexes = indexes
        self.texts = texts
        self.digest = digest
    }

    func set(_ t: String, _ k: [Value], _ row: Row?) {
        let old: Row?
        if let r = row {
            old = rows.put(k, r)
        } else {
            guard let o = rows.remove(k) else { return }
            old = o
        }
        if old != row {
            if let o = old { digest.sub(leaf(t, o)) }
            if let r = row { digest.add(leaf(t, r)) }
        }
        for i in texts.indices { texts[i].set(k, old, row) }
        for i in indexes.indices { indexes[i].set(k, old, row) }
    }

    func copy() -> TableState { TableState(rows: rows, indexes: indexes, texts: texts, digest: digest) }

    static func of(_ tbl: Table) -> TableState {
        var seen = Set<[String]>()
        let lists = tbl.indexes.map { $0.columns } + tbl.refs.map { [$0.column] }
        let indexes = lists.filter { !$0.isEmpty && $0 != tbl.key && seen.insert($0).inserted }.map { Secondary(columns: $0) }
        return TableState(rows: SortedMap(), indexes: indexes, texts: tbl.text.map { Trigrams(column: $0) }, digest: Digest())
    }
}

// The values `eq` holds `columns` equal to, in the columns' order.
private func held(_ columns: [String], _ eq: [Eq]) -> [Value] {
    columns.map { c in eq.first { $0.column == c }?.value ?? .null }
}

// The span on the column after an index's first `n`, if any.
private func bounding(_ columns: [String], _ n: Int, _ spans: [Span]) -> Span? {
    guard n < columns.count else { return nil }
    let c = columns[n]
    return spans.first { $0.column == c }
}

// The key `eq` names, when it holds every key column equal.
private func keyHeld(_ tbl: Table, _ eq: [Eq]) -> [Value]? {
    let all = !tbl.key.isEmpty && tbl.key.allSatisfy { c in eq.contains { $0.column == c } }
    return all ? held(tbl.key, eq) : nil
}

/// The spec's store (`store::MemoryStore`): the rows of every table by key,
/// an index per declared index and per reference column, a trigram index per
/// text index, and each table's digest. The indexes are derived and say
/// nothing the rows do not: two stores are equal when their rows are.
public final class MemoryStore: Store, Equatable, CustomStringConvertible {
    public let schema: Schema
    private var tables: [String: TableState]

    private init(_ schema: Schema, _ tables: [String: TableState]) {
        self.schema = schema
        self.tables = tables
    }

    /// `MemoryStore::empty`: one empty table per table of the schema.
    public static func empty(_ schema: Schema) -> MemoryStore {
        var tables = [String: TableState]()
        for t in schema.tables where tables[t.name] == nil { tables[t.name] = TableState.of(t) }
        return MemoryStore(schema, tables)
    }

    /// `MemoryStore::from_value`: a store from the vectors' shape, each row applied raw.
    public static func fromValue(_ schema: Schema, _ v: Value) -> MemoryStore {
        let st = empty(schema)
        if case .record(let r) = v {
            for (t, rows) in r.pairs {
                guard case .list(let xs) = rows else { continue }
                for x in xs { if case .record(let row) = x { st.applyChange(.add(t, row)) } }
            }
        }
        return st
    }

    /// A second store with the same rows, written independently.
    public func copy() -> MemoryStore { MemoryStore(schema, tables.mapValues { $0.copy() }) }

    public static func == (a: MemoryStore, b: MemoryStore) -> Bool {
        if a.schema != b.schema || Set(a.tables.keys) != Set(b.tables.keys) { return false }
        for (t, st) in a.tables where st.rows != b.tables[t]!.rows { return false }
        return true
    }

    public var description: String { "MemoryStore(" + storeValue().show() + ")" }

    /// The rows of a table, by key.
    public func rows(_ table: String) -> SortedMap<[Value], Row> { tables[table]?.rows ?? SortedMap() }

    /// Whether every table is empty.
    public var isEmpty: Bool { tables.values.allSatisfy { $0.rows.isEmpty } }

    public func get(_ table: String, _ key: [Value]) -> Row? { tables[table]?.rows[key] }

    public func exists(_ table: String, _ key: [Value]) -> Bool { tables[table]?.rows.contains(key) == true }

    public func scan(_ table: String) -> [Row] { tables[table]?.rows.values ?? [] }

    public func scanWhere(_ table: String, _ keep: Keep) -> [Row] { tables[table]?.rows.values.filter(keep) ?? [] }

    // The index to read `eq` through, the values to look up in it, and a span.
    private func lookup(_ st: TableState, _ eq: [Eq], _ spans: [Span]) -> (Int, [Value], Span?)? {
        func isHeld(_ c: String) -> Bool { eq.contains { $0.column == c } }
        var best: (Int, [Value], Span?)? = nil
        var bestSize = Int.max
        for (i, ix) in st.indexes.enumerated() where ix.columns.allSatisfy(isHeld) {
            let vals = held(ix.columns, eq)
            let size = ix.rows[vals]?.count ?? 0
            if size < bestSize {
                best = (i, vals, nil)
                bestSize = size
            }
        }
        if best != nil { return best }
        var most = -1
        for (i, ix) in st.indexes.enumerated() {
            var n = 0
            while n < ix.columns.count && isHeld(ix.columns[n]) { n += 1 }
            let span = bounding(ix.columns, n, spans)
            if n == 0 && span == nil { continue }
            let score = n + (span != nil ? 1 : 0)
            if score > most {
                most = score
                best = (i, held(Array(ix.columns[0..<n]), eq), span)
            }
        }
        return best
    }

    private func byKey(_ tbl: Table?, _ rows: [Row]) -> [Row] {
        guard let t = tbl, rows.count > 1 else { return rows }
        return rows.sorted { Rules.compareRows(t, [], $0, $1) < 0 }
    }

    public func scanWhereEq(_ table: String, _ eq: [Eq], _ spans: [Span], _ keep: Keep) -> [Row] {
        let tbl = schema.lookupTable(table)
        guard let st = tables[table] else { return [] }
        if let t = tbl, let k = keyHeld(t, eq) {
            guard let r = st.rows[k], keep(r) else { return [] }
            return [r]
        }
        guard let (i, vals, span) = lookup(st, eq, spans) else { return scanWhere(table, keep) }
        let ix = st.indexes[i]
        var out = [Row]()
        if vals.count == ix.columns.count {
            guard let keys = ix.rows[vals] else { return [] }
            for k in keys {
                let r = st.rows[k]!
                if keep(r) { out.append(r) }
            }
        } else {
            for p in ix.under(vals, span) {
                for k in ix.rows.values[p] {
                    let r = st.rows[k]!
                    if keep(r) { out.append(r) }
                }
            }
        }
        return byKey(tbl, out)
    }

    public func scanOrdered(_ table: String, _ eq: [Eq], _ spans: [Span], _ order: [(String, Dir)], _ keep: Keep, _ limit: Int) -> [Row]? {
        guard let tbl = schema.lookupTable(table), let st = tables[table] else { return nil }
        var found: (Int, Int, Dir)? = nil
        for (i, ix) in st.indexes.enumerated() {
            if let s = ix.serves(tbl, eq, order) {
                found = (i, s.0, s.1)
                break
            }
        }
        guard let (i, n, dir) = found else { return nil }
        let ix = st.indexes[i]
        var out = [Row]()
        if limit == 0 || st.rows.isEmpty { return out }
        let range = ix.under(held(Array(ix.columns[0..<n]), eq), bounding(ix.columns, n, spans))
        let positions: [Int] = dir == .asc ? Array(range) : Array(range.reversed())
        for p in positions {
            for k in ix.rows.values[p] {
                let r = st.rows[k]!
                if keep(r) {
                    out.append(r)
                    if out.count == limit { return out }
                }
            }
        }
        return out
    }

    public func applyChange(_ change: Change) {
        guard let tbl = schema.lookupTable(change.table), let st = tables[change.table] else { return }
        switch change {
        case .add(let t, let r): st.set(t, tbl.keyOf(r), r)
        case .edit(let t, _, let n): st.set(t, tbl.keyOf(n), n)
        case .remove(let t, let r): st.set(t, tbl.keyOf(r), nil)
        }
    }

    public func digest(_ table: String) -> Digest? { tables[table]?.digest }

    // D4 The keys a text index says may hold the needles: per branch, the
    // postings of every trigram of every needle on an indexed column,
    // intersected; the branches together. `nil` when a branch has none.
    private func textKeys(_ st: TableState, _ has: [[(String, String)]]) -> SortedSet<[Value]>? {
        if st.texts.isEmpty { return nil }
        var out = SortedSet<[Value]>()
        for branch in has {
            var lists = [SortedSet<[Value]>?]()
            for (c, needle) in branch {
                guard let ix = st.texts.first(where: { $0.column == c }) else { continue }
                for g in trigrams(needle) { lists.append(ix.postings[g]) }
            }
            if lists.isEmpty { return nil }
            if lists.contains(where: { $0 == nil }) { continue }
            let sorted = lists.map { $0! }.enumerated().sorted { $0.element.count != $1.element.count ? $0.element.count < $1.element.count : $0.offset < $1.offset }.map { $0.element }
            for k in sorted[0] where sorted.allSatisfy({ $0.contains(k) }) { out.insert(k) }
        }
        return out
    }

    public func scanWhereText(_ table: String, _ eq: [Eq], _ spans: [Span], _ has: [[(String, String)]], _ keep: Keep) -> [Row] {
        let tbl = schema.lookupTable(table)
        let wholeKey = tbl != nil && keyHeld(tbl!, eq) != nil
        let st = tables[table]
        let keys = st != nil && !wholeKey ? textKeys(st!, has) : nil
        guard let s = st, let ks = keys else { return scanWhereEq(table, eq, spans, keep) }
        return ks.compactMap { k in s.rows[k].flatMap { keep($0) ? $0 : nil } }
    }
}

/// An optimistic overlay over a base store (`store::Overlay`): `(table, key)
/// -> row or none`, consulted first on every read, the base untouched until
/// the caller commits the changes.
public final class Overlay: Store {
    private let base: Store
    private var writes = [String: SortedMap<[Value], Row?>]()

    public init(_ base: Store) { self.base = base }

    public var schema: Schema { base.schema }

    private func written(_ table: String) -> SortedMap<[Value], Row?>? {
        guard let w = writes[table], !w.isEmpty else { return nil }
        return w
    }

    public func get(_ table: String, _ key: [Value]) -> Row? {
        if let w = writes[table], let i = w.index(of: key) { return w.values[i] }
        return base.get(table, key)
    }

    public func exists(_ table: String, _ key: [Value]) -> Bool {
        if let w = writes[table], let i = w.index(of: key) { return w.values[i] != nil }
        return base.exists(table, key)
    }

    public func scan(_ table: String) -> [Row] {
        guard let tbl = schema.lookupTable(table) else { return [] }
        guard let ws = written(table) else { return base.scan(table) }
        var merged = SortedMap<[Value], Row>()
        for r in base.scan(table) { merged[tbl.keyOf(r)] = r }
        for (k, w) in ws {
            if let r = w { merged[k] = r } else { merged.remove(k) }
        }
        return merged.values
    }

    public func scanWhere(_ table: String, _ keep: Keep) -> [Row] { scanWhereEq(table, [], [], keep) }

    public func scanWhereEq(_ table: String, _ eq: [Eq], _ spans: [Span], _ keep: Keep) -> [Row] {
        scanWhereText(table, eq, spans, [], keep)
    }

    public func scanWhereText(_ table: String, _ eq: [Eq], _ spans: [Span], _ has: [[(String, String)]], _ keep: Keep) -> [Row] {
        guard let tbl = schema.lookupTable(table) else { return [] }
        if let k = keyHeld(tbl, eq) {
            guard let r = get(table, k), keep(r) else { return [] }
            return [r]
        }
        func fromBase(_ keep: Keep) -> [Row] {
            has.isEmpty ? base.scanWhereEq(table, eq, spans, keep) : base.scanWhereText(table, eq, spans, has, keep)
        }
        guard let ws = written(table) else { return fromBase(keep) }
        var merged = SortedMap<[Value], Row>()
        for r in fromBase({ r in !ws.contains(tbl.keyOf(r)) && keep(r) }) { merged[tbl.keyOf(r)] = r }
        for (key, w) in ws {
            if let r = w, keep(r) { merged[key] = r }
        }
        return merged.values
    }

    public func scanOrdered(_ table: String, _ eq: [Eq], _ spans: [Span], _ order: [(String, Dir)], _ keep: Keep, _ limit: Int) -> [Row]? {
        guard let tbl = schema.lookupTable(table) else { return nil }
        guard let ws = written(table) else { return base.scanOrdered(table, eq, spans, order, keep, limit) }
        guard var rows = base.scanOrdered(table, eq, spans, order, { r in !ws.contains(tbl.keyOf(r)) && keep(r) }, limit) else { return nil }
        for w in ws.values { if let r = w, keep(r) { rows.append(r) } }
        rows.sort { Rules.compareRows(tbl, order, $0, $1) < 0 }
        return rows.count > limit ? Array(rows[0..<limit]) : rows
    }

    public func applyChange(_ change: Change) {
        guard let tbl = schema.lookupTable(change.table) else { return }
        switch change {
        case .add(let t, let r): writes[t, default: SortedMap()].put(tbl.keyOf(r), r)
        case .edit(let t, _, let n): writes[t, default: SortedMap()].put(tbl.keyOf(n), n)
        case .remove(let t, let r): writes[t, default: SortedMap()].put(tbl.keyOf(r), nil)
        }
    }

    public func digest(_ table: String) -> Digest? {
        guard var d = base.digest(table) else { return nil }
        guard let ws = writes[table] else { return d }
        for (k, w) in ws {
            let old = base.get(table, k)
            if old == w { continue }
            if let o = old { d.sub(leaf(table, o)) }
            if let n = w { d.add(leaf(table, n)) }
        }
        return d
    }
}
