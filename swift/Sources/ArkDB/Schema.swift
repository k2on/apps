import Foundation

/// §2.1 The static types of the IR, as `Ark.Schema.Ty`. `enum` and
/// `struct` are Swift keywords, so those two cases are `enumOf` and
/// `structOf`.
public indirect enum Ty: Equatable {
    case bool
    case int
    case text
    case bytes
    case id(TableName)
    case enumOf([String])
    case option(Ty)
    case list(Ty)
    case structOf([FieldName: Ty])

    /// The types a column may have (before nullability).
    public var isScalar: Bool {
        switch self {
        case .bool, .int, .text, .bytes, .id, .enumOf: return true
        default: return false
        }
    }
}

public struct Column: Equatable {
    public var name: FieldName
    public var ty: Ty
    public var nullable: Bool
    public init(_ name: FieldName, _ ty: Ty, nullable: Bool = false) {
        self.name = name; self.ty = ty; self.nullable = nullable
    }
    /// The static type, nullability included.
    public var columnTy: Ty { return nullable ? .option(ty) : ty }
}

public struct Index: Equatable {
    public var columns: [FieldName]
    public var unique: Bool
    public init(_ columns: [FieldName], unique: Bool) { self.columns = columns; self.unique = unique }
}

/// `column REFERENCES table(key)`.
public struct Ref: Equatable {
    public var column: FieldName
    public var table: TableName
    public init(_ column: FieldName, _ table: TableName) { self.column = column; self.table = table }
}

public struct Table: Equatable {
    public var name: TableName
    public var columns: [Column]
    public var key: [FieldName]
    public var indexes: [Index]
    public var refs: [Ref]
    public init(_ name: TableName, columns: [Column], key: [FieldName], indexes: [Index] = [], refs: [Ref] = []) {
        self.name = name; self.columns = columns; self.key = key; self.indexes = indexes; self.refs = refs
    }

    public func column(_ n: FieldName) -> Column? {
        return columns.first { $0.name == n }
    }

    /// The type of a whole row: a struct of every column.
    public var rowTy: Ty {
        var m: [FieldName: Ty] = [:]
        for c in columns { m[c.name] = c.columnTy }
        return .structOf(m)
    }

    /// The types of the key columns, in key order.
    public var keyTy: [Ty] {
        return key.compactMap { column($0)?.columnTy }
    }

    /// The key of a row: the key columns' values in key order; a column the
    /// row lacks reads as null.
    public func keyOf(_ row: Row) -> [Value] {
        return key.map { row[$0] ?? .null }
    }
}

/// §2 The schema: the module's one set of tables, in declaration order —
/// which is also the order a state hash walks them. There is one log, so
/// every reference is checked and any function may read any table.
public struct Schema: Equatable {
    public var tables: [Table]
    public init(tables: [Table]) { self.tables = tables }

    public func lookupTable(_ n: TableName) -> Table? {
        return tables.first { $0.name == n }
    }

    /// Every table, in schema order.
    public var tableNames: [TableName] {
        return tables.map { $0.name }
    }

    /// §2.2 Every relationship: one per reference.
    public var relations: [Relation] {
        var out: [Relation] = []
        for t in tables { for r in t.refs { out.append(Relation(parent: r.table, child: t.name, column: r.column)) } }
        return out
    }

    /// §2.3 Well-formedness, as `Ark.Schema.checkSchema`: each complaint
    /// names the `SchemaError` constructor and its table and column. A
    /// module whose schema draws any is refused by the verifier.
    public func problems() -> [String] {
        var out: [String] = []
        func dups(_ xs: [String]) -> [String] {
            var n: [String: Int] = [:]
            for x in xs { n[x, default: 0] += 1 }
            return n.filter { $0.value > 1 }.keys.sorted { compareText($0, $1) < 0 }
        }
        let twice = Set(dups(tables.map { $0.name }))
        for t in tables where twice.contains(t.name) { out.append("DuplicateTable \(t.name)") }
        for t in tables {
            let n = t.name
            for c in dups(t.columns.map { $0.name }) { out.append("DuplicateColumn \(n) \(c)") }
            if t.key.isEmpty { out.append("NoKey \(n)") }
            for k in t.key where t.column(k) == nil { out.append("UnknownKeyColumn \(n) \(k)") }
            for k in t.key where t.column(k)?.nullable == true { out.append("NullableKey \(n) \(k)") }
            for c in t.columns where !c.ty.isScalar { out.append("NonScalarColumn \(n) \(c.name)") }
            for ix in t.indexes { for c in ix.columns where t.column(c) == nil { out.append("UnknownIndexColumn \(n) \(c)") } }
            for r in t.refs {
                guard let c = t.column(r.column) else { out.append("UnknownRefColumn \(n) \(r.column)"); continue }
                guard let p = lookupTable(r.table) else { out.append("UnknownRefTable \(n) \(r.table)"); continue }
                let pk = p.key.compactMap { p.column($0)?.ty }
                guard pk.count == 1, p.key.count == 1 else { out.append("RefToCompositeKey \(n) \(r.table)"); continue }
                var want = pk[0]
                if case .id = want { want = .id(r.table) }
                if c.ty != want { out.append("RefTypeMismatch \(n) \(r.column)") }
                if case .id(let of) = c.ty, of != r.table { out.append("IdNamesWrongTable \(n) \(r.column)") }
            }
            // An id column is its own table's key or a reference: an id
            // naming a table with no reference is a declaration the store
            // could not hold to.
            for c in t.columns {
                guard case .id(let of) = c.ty else { continue }
                if t.refs.contains(where: { $0.column == c.name }) { continue }
                if of == n && t.key.contains(c.name) { continue }
                out.append("IdColumnWithoutRef \(n) \(c.name)")
            }
        }
        return out
    }

    /// The relationships reaching down from a table.
    public func childrenOf(_ parent: TableName) -> [Relation] {
        return relations.filter { $0.parent == parent }
    }

    /// The relationships reaching up from a table.
    public func parentOf(_ child: TableName) -> [Relation] {
        return relations.filter { $0.child == child }
    }
}

/// A reference `child.column REFERENCES parent`, as two relationships.
public struct Relation: Equatable {
    public var parent: TableName
    public var child: TableName
    /// The child column that holds the parent's key.
    public var column: FieldName
    public init(parent: TableName, child: TableName, column: FieldName) {
        self.parent = parent; self.child = child; self.column = column
    }
}

/// Ascending or descending, for an order.
public enum Dir: Equatable {
    case asc
    case desc
}
