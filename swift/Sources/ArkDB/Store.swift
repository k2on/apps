import Foundation

/// A row: every column of its table, by name. Never partial once stored.
public typealias Row = [FieldName: Value]

/// A row's key: the key columns' values in key order, as a hashable
/// wrapper (equality and hashing are structural, through `Value`).
public struct Key: Hashable {
    public let values: [Value]
    public init(_ values: [Value]) { self.values = values }
}

/// §4.1 What a write reports.
public enum Change: Equatable {
    case add(TableName, Row)
    case remove(TableName, Row)
    case edit(TableName, Row, Row) // old, new

    public var table: TableName {
        switch self {
        case .add(let t, _), .remove(let t, _), .edit(let t, _, _): return t
        }
    }
}

/// §4.2 A refusal: a verdict about a write, reached identically by every
/// replica. `text` renders it as `Ark.Store`'s `Show` would, which is what
/// a `Fault.refuse` carries and what a `Reject` frame says.
public enum Refusal: Error, Equatable {
    case noSuchTable(TableName)
    case malformedRow(TableName, String)
    case notNull(TableName, FieldName)
    case uniqueViolation(TableName, [FieldName])
    case missingParent(TableName, FieldName, TableName)
    case stillReferenced(TableName, TableName)
    case refused(String)

    public var text: String {
        func q(_ s: String) -> String { return "\"" + s + "\"" }
        switch self {
        case .noSuchTable(let t): return "NoSuchTable \(q(t))"
        case .malformedRow(let t, let w): return "MalformedRow \(q(t)) \(q(w))"
        case .notNull(let t, let c): return "NotNull \(q(t)) \(q(c))"
        case .uniqueViolation(let t, let cs): return "UniqueViolation \(q(t)) [" + cs.map(q).joined(separator: ",") + "]"
        case .missingParent(let t, let c, let p): return "MissingParent \(q(t)) \(q(c)) \(q(p))"
        case .stillReferenced(let t, let c): return "StillReferenced \(q(t)) \(q(c))"
        case .refused(let s): return s
        }
    }

    public var fault: Fault { return .refuse(text) }
}

/// §4 The store, as generated code sees it (`Ark.Store`, `Ark.Eval` §6.6).
public protocol Store: AnyObject {
    var schema: Schema { get }
    /// The row, or null.
    func get(_ table: TableName, _ key: [Value]) -> Value
    /// Whether a row is there, as a `Value.bool`.
    func exists(_ table: TableName, _ key: [Value]) -> Value
    /// The nodes exactly as `Ark.Eval.select` builds them, as a `Value.list`.
    func select(_ plan: Plan) -> Value
    /// Write a full row; faults with the store's refusal.
    func put(_ table: TableName, _ row: Value) throws
    /// Delete by key; a missing row is a no-op.
    func delete(_ table: TableName, _ key: [Value]) throws
    /// Every row of a table, in key order under the total order.
    func scan(_ table: TableName) -> [Row]
    /// §4.5 Apply a change as a fact, with no constraint check.
    func applyChange(_ ch: Change)
}

/// The store as a value: a map of maps, exactly `Ark.Store`. A class so
/// that generated code can write through it; `clone()` is how the peer
/// keeps a confirmed store apart from the view built on it (the tables are
/// copy-on-write, so a clone costs nothing until one is written).
public final class MemoryStore: Store {
    public let schema: Schema
    public private(set) var tables: [TableName: [Key: Row]]
    /// The changes `put` and `delete` reported through the `Store`
    /// interface since `takeChanges()` was last called, in order.
    public private(set) var reported: [Change] = []

    public init(schema: Schema) {
        self.schema = schema
        self.tables = [:]
    }

    init(schema: Schema, tables: [TableName: [Key: Row]]) {
        self.schema = schema
        self.tables = tables
    }

    public static func empty(_ schema: Schema) -> MemoryStore { return MemoryStore(schema: schema) }

    public func clone() -> MemoryStore { return MemoryStore(schema: schema, tables: tables) }

    /// The tables of the schema, in schema order.
    public var tableNames: [TableName] { return schema.tableNames }

    public func rows(_ t: TableName) -> [Key: Row] { return tables[t] ?? [:] }

    public func getRow(_ t: TableName, _ key: [Value]) -> Row? { return rows(t)[Key(key)] }

    public func get(_ table: TableName, _ key: [Value]) -> Value {
        if let r = getRow(table, key) { return .record(r) }
        return .null
    }

    public func exists(_ table: TableName, _ key: [Value]) -> Value {
        return .bool(getRow(table, key) != nil)
    }

    /// Every row of a table, in key order under the total order.
    public func scan(_ table: TableName) -> [Row] {
        let rs = rows(table)
        return rs.keys.sorted { compareValue(.list($0.values), .list($1.values)) < 0 }.map { rs[$0]! }
    }

    public func select(_ plan: Plan) -> Value {
        do {
            return .list(try Eval.select(schema: schema, store: self, plan: plan) { e in
                if case .lit(let v) = e { return v }
                throw Fault.bug("select: a plan handed to the store must carry literal right-hand sides")
            })
        } catch {
            fatalError("MemoryStore.select: \(error)")
        }
    }

    public func put(_ table: TableName, _ row: Value) throws {
        guard case .record(let r) = row else { throw Fault.bug("put: expected Struct, got \(row.brief)") }
        switch tryPut(table, r) {
        case .failure(let why): throw why.fault
        case .success(let ch): if let c = ch { reported.append(c) }
        }
    }

    public func delete(_ table: TableName, _ key: [Value]) throws {
        switch tryDelete(table, key) {
        case .failure(let why): throw why.fault
        case .success(let ch): if let c = ch { reported.append(c) }
        }
    }

    public func takeReported() -> [Change] {
        let r = reported
        reported = []
        return r
    }

    /// §4.3 Write a full row: the table must exist; the row is completed
    /// (every omitted nullable column becomes null) and must then be exactly
    /// the table's columns with values of their types; no non-nullable
    /// column may be null; every unique index must stay unique against every
    /// other row; every reference must find its parent. A new key is an
    /// `add`; the same row again is no change at all; otherwise an `edit`.
    public func tryPut(_ tn: TableName, _ row0: Row) -> Result<Change?, Refusal> {
        guard let tbl = schema.lookupTable(tn) else { return .failure(.noSuchTable(tn)) }
        let row = MemoryStore.complete(tbl, row0)
        if case .failure(let r) = MemoryStore.wellTyped(tbl, row) { return .failure(r) }
        let k = Key(tbl.keyOf(row))
        let here = rows(tn)
        var others = here
        others.removeValue(forKey: k)
        for ix in tbl.indexes where ix.unique {
            if case .failure(let r) = MemoryStore.unique(tbl, others, row, ix) { return .failure(r) }
        }
        for ref in tbl.refs {
            if case .failure(let r) = parentExists(tbl, row, ref) { return .failure(r) }
        }
        if let old = here[k] {
            if old == row { return .success(nil) }
            tables[tn, default: [:]][k] = row
            return .success(.edit(tn, old, row))
        }
        tables[tn, default: [:]][k] = row
        return .success(.add(tn, row))
    }

    /// §4.4 Delete by key: a missing row is a no-op; a row another row still
    /// references is a refusal.
    public func tryDelete(_ tn: TableName, _ key: [Value]) -> Result<Change?, Refusal> {
        guard let tbl = schema.lookupTable(tn) else { return .failure(.noSuchTable(tn)) }
        guard let row = getRow(tn, key) else { return .success(nil) }
        for rel in schema.childrenOf(tn) {
            if case .failure(let r) = noChild(tbl, key, rel) { return .failure(r) }
        }
        tables[tn]?.removeValue(forKey: Key(key))
        return .success(.remove(tn, row))
    }

    /// Fill in every nullable column the row left out, as null.
    static func complete(_ tbl: Table, _ row: Row) -> Row {
        var r = row
        for c in tbl.columns where c.nullable && r[c.name] == nil { r[c.name] = .null }
        return r
    }

    static func wellTyped(_ tbl: Table, _ row: Row) -> Result<Void, Refusal> {
        let want = tbl.columns.map { $0.name }
        let have = Set(row.keys)
        if have != Set(want) {
            let haveList = have.sorted { compareText($0, $1) < 0 }.map { "\"" + $0 + "\"" }.joined(separator: ",")
            let wantList = want.map { "\"" + $0 + "\"" }.joined(separator: ",")
            return .failure(.malformedRow(tbl.name, "columns [\(haveList)] are not [\(wantList)]"))
        }
        for c in tbl.columns {
            guard let v = row[c.name] else { return .failure(.malformedRow(tbl.name, c.name)) }
            if v.isNull() {
                if c.nullable { continue }
                return .failure(.notNull(tbl.name, c.name))
            }
            if !ofType(c.ty, v) { return .failure(.malformedRow(tbl.name, c.name + " has the wrong type")) }
        }
        return .success(())
    }

    /// Whether a value inhabits a scalar type; any sixteen bytes inhabit
    /// any id type.
    static func ofType(_ t: Ty, _ v: Value) -> Bool {
        switch (t, v) {
        case (.bool, .bool): return true
        case (.int, .int): return true
        case (.text, .text): return true
        case (.bytes, .bytes): return true
        case (.id, .id): return true
        case (.enumOf(let vs), .text(let x)): return vs.contains { compareText($0, x) == 0 }
        case (.option, .null): return true
        case (.option(let t2), _): return ofType(t2, v)
        default: return false
        }
    }

    /// A NULL is not equal to anything, itself included, so two rows both
    /// NULL in a unique column do not clash.
    static func unique(_ tbl: Table, _ others: [Key: Row], _ row: Row, _ ix: Index) -> Result<Void, Refusal> {
        let mine = ix.columns.map { row[$0] }
        let allPresentNonNull = mine.allSatisfy { $0 != nil && !$0!.isNull() }
        if !allPresentNonNull { return .success(()) }
        for r in others.values {
            let theirs = ix.columns.map { r[$0] }
            var same = true
            for (a, b) in zip(theirs, mine) {
                switch (a, b) {
                case (nil, nil): break
                case (let x?, let y?): if x != y { same = false }
                default: same = false
                }
                if !same { break }
            }
            if same { return .failure(.uniqueViolation(tbl.name, ix.columns)) }
        }
        return .success(())
    }

    func parentExists(_ tbl: Table, _ row: Row, _ r: Ref) -> Result<Void, Refusal> {
        guard let v = row[r.column] else { return .failure(.malformedRow(tbl.name, r.column)) }
        if v.isNull() { return .success(()) }
        if getRow(r.table, [v]) != nil { return .success(()) }
        return .failure(.missingParent(tbl.name, r.column, r.table))
    }

    func noChild(_ tbl: Table, _ k: [Value], _ rel: Relation) -> Result<Void, Refusal> {
        guard k.count == 1 else { return .success(()) }
        let kv = k[0]
        for r in rows(rel.child).values {
            if let c = r[rel.column], c == kv { return .failure(.stillReferenced(tbl.name, rel.child)) }
        }
        return .success(())
    }

    /// §4.5 A fact is not re-judged: the row goes in (or out) as recorded.
    public func applyChange(_ ch: Change) {
        switch ch {
        case .add(let t, let row), .edit(let t, _, let row):
            guard let tbl = schema.lookupTable(t) else { return }
            tables[t, default: [:]][Key(tbl.keyOf(row))] = row
        case .remove(let t, let row):
            guard let tbl = schema.lookupTable(t) else { return }
            tables[t]?.removeValue(forKey: Key(tbl.keyOf(row)))
        }
    }

    public func applyChanges(_ chs: [Change]) {
        for ch in chs { applyChange(ch) }
    }

    /// A copy with the changes applied, the store itself untouched.
    public func applying(_ chs: [Change]) -> MemoryStore {
        let s = clone()
        s.applyChanges(chs)
        return s
    }

    /// Every table of the schema with its rows in key order: the shape the
    /// vectors write a store as, and what the state hash is taken over.
    public func asValue() -> Value {
        var m: [String: Value] = [:]
        for t in tableNames { m[t] = .list(scan(t).map { .record($0) }) }
        return .record(m)
    }

    /// The rows of two stores over one schema, together; the left wins a table.
    public func merge(_ other: MemoryStore) -> MemoryStore {
        var ts = tables
        for (t, rs) in other.tables {
            var mine = ts[t] ?? [:]
            for (k, r) in rs where mine[k] == nil { mine[k] = r }
            ts[t] = mine
        }
        return MemoryStore(schema: schema, tables: ts)
    }

    /// Whether two stores hold the same rows.
    public func sameRows(as other: MemoryStore) -> Bool {
        for t in Set(tables.keys).union(other.tables.keys) {
            if rows(t) != other.rows(t) { return false }
        }
        return true
    }
}
