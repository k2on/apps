// §2 The schema (`rust/ark/src/schema.rs`): which tables exist, their
// columns and key, which indexes are unique, which columns reference which
// table, and which text columns are searched through a trigram index. A
// value in the module, not DDL; one set of tables and one log.

/// §2.1 The static types of the IR (`schema.rs`, `Ty`).
public indirect enum Ty: Hashable, CustomStringConvertible {
    case bool
    case int
    case text
    case bytes
    case id(String)
    case enumeration([String])
    case option(Ty)
    case list(Ty)
    /// Fields in code point order of their names.
    case structure(TyFields)

    /// A struct type from fields in any order.
    public static func structOf(_ fields: [(String, Ty)]) -> Ty { .structure(TyFields(fields)) }

    /// `is_scalar`: the types a column may have, before nullability.
    public var isScalar: Bool {
        switch self {
        case .bool, .int, .text, .bytes, .id, .enumeration: return true
        default: return false
        }
    }

    public var structFields: TyFields? {
        if case .structure(let f) = self { return f }
        return nil
    }

    /// As Rust's `{:?}` writes it.
    public var description: String {
        switch self {
        case .bool: return "Bool"
        case .int: return "Int"
        case .text: return "Text"
        case .bytes: return "Bytes"
        case .id(let t): return "Id(\(debugString(t)))"
        case .enumeration(let vs): return "Enum(\(debugStrings(vs)))"
        case .option(let t): return "Option(\(t))"
        case .list(let t): return "List(\(t))"
        case .structure(let f):
            return "Struct({" + zip(f.names, f.types).map { debugString($0.0) + ": " + $0.1.description }.joined(separator: ", ") + "})"
        }
    }
}

/// A struct type's fields, in code point order of their names.
public struct TyFields: Hashable {
    public private(set) var names: [String]
    public private(set) var types: [Ty]

    public init(_ fields: [(String, Ty)]) {
        let sorted = fields.enumerated().sorted { a, b in
            let c = compareText(a.element.0, b.element.0)
            return c != 0 ? c < 0 : a.offset < b.offset
        }
        var ns = [String]()
        var ts = [Ty]()
        for (_, f) in sorted {
            if let l = ns.last, textEquals(l, f.0) {
                ts[ts.count - 1] = f.1
            } else {
                ns.append(f.0)
                ts.append(f.1)
            }
        }
        names = ns
        types = ts
    }

    public var count: Int { names.count }

    public subscript(name: String) -> Ty? {
        guard let i = names.firstIndex(where: { textEquals($0, name) }) else { return nil }
        return types[i]
    }

    public func has(_ name: String) -> Bool { self[name] != nil }

    public var pairs: [(String, Ty)] { Array(zip(names, types)) }

    /// These fields with one more, or one replaced.
    public func with(_ name: String, _ t: Ty) -> TyFields {
        var ps = pairs
        if let i = ps.firstIndex(where: { textEquals($0.0, name) }) { ps[i].1 = t } else { ps.append((name, t)) }
        return TyFields(ps)
    }
}

/// A column; a nullable column's values are `Null` or a value of `ty`.
public struct Column: Hashable {
    public let name: String
    public let ty: Ty
    public let nullable: Bool

    public init(_ name: String, _ ty: Ty, _ nullable: Bool) {
        self.name = name
        self.ty = ty
        self.nullable = nullable
    }

    /// `Column::column_ty`: the static type, nullability included.
    public var columnTy: Ty { nullable ? .option(ty) : ty }
}

/// A uniqueness constraint when `unique`, a statement of intent otherwise.
public struct Index: Hashable {
    public let columns: [String]
    public let unique: Bool
    public init(_ columns: [String], _ unique: Bool) {
        self.columns = columns
        self.unique = unique
    }
}

/// `column REFERENCES table(key)`.
public struct Ref: Hashable {
    public let column: String
    public let table: String
    public init(_ column: String, _ table: String) {
        self.column = column
        self.table = table
    }
}

/// Ascending or descending, for an order.
public enum Dir: Hashable {
    case asc
    case desc
}

/// A table (`schema.rs`, `Table`). `text` is `docs/plan-db.md` D4's text
/// indexes: each a text column whose folded value's trigrams name the rows
/// that hold them, kept beside `indexes` as the Rust keeps them (on the wire
/// an `index` node of kind `text`, after the others).
public final class Table: Hashable {
    public let name: String
    public let columns: [Column]
    public let key: [String]
    public let indexes: [Index]
    public let refs: [Ref]
    public let text: [String]
    let byName: [String: Int]

    public init(_ name: String, _ columns: [Column], _ key: [String], _ indexes: [Index], _ refs: [Ref], _ text: [String] = []) {
        self.name = name
        self.columns = columns
        self.key = key
        self.indexes = indexes
        self.refs = refs
        self.text = text
        var m = [String: Int]()
        for (i, c) in columns.enumerated() where m[c.name] == nil { m[c.name] = i }
        byName = m
    }

    public static func == (a: Table, b: Table) -> Bool {
        a === b || (a.name == b.name && a.columns == b.columns && a.key == b.key && a.indexes == b.indexes && a.refs == b.refs && a.text == b.text)
    }

    public func hash(into h: inout Hasher) {
        h.combine(name)
        h.combine(columns)
    }

    /// `Table::column`.
    public func column(_ name: String) -> Column? { byName[name].map { columns[$0] } }

    /// `Table::row_ty`: a struct of every column.
    public var rowTy: Ty { .structOf(columns.map { ($0.name, $0.columnTy) }) }

    /// `Table::key_ty`: the key columns' types, in key order.
    public var keyTy: [Ty] { key.compactMap { column($0)?.columnTy } }

    /// `Table::key_of`: the key columns' values in key order; a missing column reads as `Null`.
    @inline(__always)
    public func keyOf(_ row: Row) -> [Value] { key.map { row[$0] ?? .null } }

    /// Whether a row is laid out as this table's rows are: exactly its columns (`Row::is_of`).
    public func holds(_ row: Row) -> Bool {
        if row.count != columns.count { return false }
        for c in columns where !row.has(c.name) { return false }
        return true
    }
}

/// The tables, in declaration order — which is also the order a state hash walks them.
public struct Schema: Hashable {
    public let tables: [Table]

    public init(_ tables: [Table]) { self.tables = tables }

    /// `Schema::empty`.
    public static let empty = Schema([])

    /// `Schema::lookup_table` (the first of a name, as `find` is).
    public func lookupTable(_ name: String) -> Table? { tables.first { $0.name == name } }

    /// §2.2 Every relationship in the schema (`Schema::relations`).
    public func relations() -> [Relation] {
        tables.flatMap { t in t.refs.map { Relation(parent: $0.table, child: t.name, column: $0.column) } }
    }

    /// `Schema::children_of`: the relationships reaching down from a table.
    public func childrenOf(_ parent: String) -> [Relation] { relations().filter { $0.parent == parent } }

    /// `Schema::parent_of`: the relationships reaching up from a table.
    public func parentOf(_ child: String) -> [Relation] { relations().filter { $0.child == child } }
}

/// §2.2 `child.column REFERENCES parent`.
public struct Relation: Hashable {
    public let parent: String
    public let child: String
    public let column: String
}

/// §2.3 Why a schema is not well formed (`schema.rs`, `SchemaError`).
public enum SchemaError: Hashable {
    case duplicateTable(String)
    case duplicateColumn(String, String)
    case noKey(String)
    case unknownKeyColumn(String, String)
    case nullableKey(String, String)
    case nonScalarColumn(String, String)
    case unknownIndexColumn(String, String)
    case unknownRefColumn(String, String)
    case unknownRefTable(String, String)
    case refToCompositeKey(String, String)
    case refTypeMismatch(String, String, Ty, Ty)
    case idColumnWithoutRef(String, String)
    case idNamesWrongTable(String, String)
    case textIndexNotText(String, String)
}

private func dups(_ names: [String]) -> [String] {
    var counts = SortedMap<CodePoints, Int>()
    for n in names { counts.modify(CodePoints(n), default: { 0 }) { $0 += 1 } }
    return counts.entries.filter { $0.value > 1 }.map { $0.key.s }
}

/// `check_schema`: every rule a schema breaks, in the spec's order.
public func checkSchema(_ sch: Schema) -> [SchemaError] {
    var errs = [SchemaError]()
    let dupTables = dups(sch.tables.map { $0.name })
    for t in sch.tables where dupTables.contains(t.name) { errs.append(.duplicateTable(t.name)) }
    for t in sch.tables { perTable(sch, t, &errs) }
    return errs
}

private func perTable(_ sch: Schema, _ t: Table, _ errs: inout [SchemaError]) {
    let tn = t.name
    for c in dups(t.columns.map { $0.name }) { errs.append(.duplicateColumn(tn, c)) }
    if t.key.isEmpty { errs.append(.noKey(tn)) }
    for k in t.key where t.column(k) == nil { errs.append(.unknownKeyColumn(tn, k)) }
    for k in t.key where t.column(k)?.nullable == true { errs.append(.nullableKey(tn, k)) }
    for c in t.columns where !c.ty.isScalar { errs.append(.nonScalarColumn(tn, c.name)) }
    for ix in t.indexes {
        for c in ix.columns where t.column(c) == nil { errs.append(.unknownIndexColumn(tn, c)) }
    }
    for (i, c) in t.text.enumerated() {
        if let col = t.column(c) {
            if col.ty != .text || t.text[0..<i].contains(c) { errs.append(.textIndexNotText(tn, c)) }
        } else {
            errs.append(.unknownIndexColumn(tn, c))
        }
    }
    for r in t.refs { perRef(sch, t, r, &errs) }
    for c in t.columns {
        if case .id(let target) = c.ty {
            let isRef = t.refs.contains { $0.column == c.name }
            let isOwnKey = target == t.name && t.key.contains(c.name)
            if !isRef && !isOwnKey { errs.append(.idColumnWithoutRef(tn, c.name)) }
        }
    }
}

private func perRef(_ sch: Schema, _ t: Table, _ r: Ref, _ errs: inout [SchemaError]) {
    guard let c = t.column(r.column) else {
        errs.append(.unknownRefColumn(t.name, r.column))
        return
    }
    guard let p = sch.lookupTable(r.table) else {
        errs.append(.unknownRefTable(t.name, r.table))
        return
    }
    let keyTy = p.keyTy
    if keyTy.count == 1 {
        var want = keyTy[0]
        if case .id = keyTy[0] { want = .id(r.table) }
        if c.ty != want { errs.append(.refTypeMismatch(t.name, r.column, want, c.ty)) }
        if case .id(let target) = c.ty, target != r.table { errs.append(.idNamesWrongTable(t.name, r.column)) }
    } else {
        errs.append(.refToCompositeKey(t.name, r.table))
    }
}
