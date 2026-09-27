import Foundation

public typealias ScopeName = String

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

public struct Scope: Equatable {
    public var name: ScopeName
    public var tables: [Table]
    public init(_ name: ScopeName, tables: [Table]) { self.name = name; self.tables = tables }
}

public struct Schema: Equatable {
    public var scopes: [Scope]
    public init(scopes: [Scope]) { self.scopes = scopes }

    public func lookupTable(_ n: TableName) -> Table? {
        for sc in scopes { for t in sc.tables where t.name == n { return t } }
        return nil
    }

    /// The scope a table is in.
    public func tableScope(_ n: TableName) -> ScopeName? {
        return scopes.first { $0.tables.contains { $0.name == n } }?.name
    }

    public func scopeOf(_ n: ScopeName) -> Scope? {
        return scopes.first { $0.name == n }
    }

    /// Every table, in schema order.
    public var tableNames: [TableName] {
        return scopes.flatMap { $0.tables.map { $0.name } }
    }

    /// §2.2 Every relationship: one per reference.
    public var relations: [Relation] {
        var out: [Relation] = []
        for sc in scopes { for t in sc.tables { for r in t.refs { out.append(Relation(parent: r.table, child: t.name, column: r.column)) } } }
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
