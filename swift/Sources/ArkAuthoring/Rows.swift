import Foundation
import ArkDB

// MARK: - §2.5 declarations: rows, scopes, columns

/// A row of a table: a struct of terms, one property per column, with its
/// table's name, its key's shape and its columns said once.
public protocol Row: Term, Codable {
    static var NAME: String { get }
    /// The key's shape: one term, or a tuple of them in key order.
    associatedtype Key
    static func columns() -> Columns<Self>
}

extension Row {
    public static var ty: Ty { return RowSchema.table(Self.self).rowTy }
    public init(repr: Repr) { self = build(Self.self, Projection(base: repr)) }
    public var repr: Repr { return structRepr(take(self)) }
}

/// A scope: a struct of tables, in order, and its name.
public protocol Scope: Decodable {
    static var NAME: String { get }
}

/// What a scope's struct is made of.
protocol AnyTable: Decodable {
    init()
    static var table: ArkDB.Table { get }
}

enum RowSchema {
    static var cache: [ObjectIdentifier: ArkDB.Table] = [:]
    static let lock = NSLock()

    static func table<R: Row>(_ r: R.Type) -> ArkDB.Table {
        lock.lock()
        if let t = cache[ObjectIdentifier(r)] { lock.unlock(); return t }
        lock.unlock()
        let c = R.columns()
        let t = ArkDB.Table(R.NAME, columns: c.cols, key: c.key, indexes: c.indexes, refs: c.refs)
        lock.lock()
        cache[ObjectIdentifier(r)] = t
        lock.unlock()
        return t
    }

    /// A scope's tables, in the order its struct declares them.
    static func scope<S: Scope>(_ s: S.Type) -> ArkDB.Scope {
        let recorder = TableRecorder()
        do {
            _ = try S(from: recorder)
        } catch {
            fatalError("ArkAuthoring: \(S.self) is not a struct of tables: \(error)")
        }
        return ArkDB.Scope(S.NAME, tables: recorder.tables)
    }

    static func make<S: Scope>(_ s: S.Type) -> S {
        do {
            return try S(from: Building(source: ByName { t, _ in t.init(repr: .v(.null)) }))
        } catch {
            fatalError("ArkAuthoring: \(S.self) is not a struct of tables: \(error)")
        }
    }
}

/// Records each table a scope's decoding asks for.
final class TableRecorder: Decoder {
    var tables: [ArkDB.Table] = []
    var codingPath: [CodingKey] { return [] }
    var userInfo: [CodingUserInfoKey: Any] { return [:] }
    func container<Key: CodingKey>(keyedBy type: Key.Type) throws -> KeyedDecodingContainer<Key> {
        return KeyedDecodingContainer(RecorderKeyed<Key>(owner: self))
    }
    func unkeyedContainer() throws -> UnkeyedDecodingContainer { throw CodingFailure(what: "unkeyed") }
    func singleValueContainer() throws -> SingleValueDecodingContainer { throw CodingFailure(what: "single") }
}

struct RecorderKeyed<K: CodingKey>: KeyedDecodingContainerProtocol {
    typealias Key = K
    let owner: TableRecorder
    var codingPath: [CodingKey] { return [] }
    var allKeys: [K] { return [] }
    func contains(_ key: K) -> Swift.Bool { return true }
    func decodeNil(forKey key: K) throws -> Swift.Bool { return false }
    func decode<T: Decodable>(_ type: T.Type, forKey key: K) throws -> T {
        guard let tb = type as? AnyTable.Type else { throw CodingFailure(what: "\(key.stringValue) is not a Table") }
        owner.tables.append(tb.table)
        return tb.init() as! T
    }
    func nestedContainer<NK: CodingKey>(keyedBy type: NK.Type, forKey key: K) throws -> KeyedDecodingContainer<NK> { throw CodingFailure(what: "nested") }
    func nestedUnkeyedContainer(forKey key: K) throws -> UnkeyedDecodingContainer { throw CodingFailure(what: "nested") }
    func superDecoder() throws -> Decoder { throw CodingFailure(what: "super") }
    func superDecoder(forKey key: K) throws -> Decoder { throw CodingFailure(what: "super") }
}

/// A column of a row type, by its IR name: `static let userId = col<Playlist, Text>("user_id")`.
public struct Col<R: Row, V: Term>: AnyColumn {
    public let name: String
    public init(_ name: String) { self.name = name }
}

public typealias col<R: Row, V: Term> = Col<R, V>

/// A column of any row, for the places a list of them is taken.
public protocol AnyColumn {
    var name: String { get }
}

/// A relationship from a parent row type to the child rows that
/// reference it: `static let playlistItem = rel<Playlist, PlaylistItem>("playlist_item")`.
public struct Rel<P: Row, C: Row> {
    public let name: String
    public init(_ name: String) { self.name = name }
}

public typealias rel<P: Row, C: Row> = Rel<P, C>

/// §2.5 A table's columns, key, indexes and references, said in order.
public struct Columns<R: Row> {
    var cols: [Column] = []
    var key: [FieldName] = []
    var indexes: [Index] = []
    var refs: [Ref] = []

    public init() {}

    func adding<V: Term>(_ c: Col<R, V>, _ want: (Ty) -> Swift.Bool, _ what: String) -> Columns<R> {
        var me = self
        var t = V.ty
        if case .option(let inner) = t { t = inner }
        precondition(want(t), "ArkAuthoring: \(R.NAME).\(c.name) is not \(what)")
        me.cols.append(Column(c.name, t))
        return me
    }

    public func id<V: Term>(_ c: Col<R, V>) -> Columns<R> { return adding(c, { if case .id = $0 { return true }; return false }, "an id") }
    public func text<V: Term>(_ c: Col<R, V>) -> Columns<R> { return adding(c, { $0 == .text }, "text") }
    public func int<V: Term>(_ c: Col<R, V>) -> Columns<R> { return adding(c, { $0 == .int }, "an int") }
    public func bool<V: Term>(_ c: Col<R, V>) -> Columns<R> { return adding(c, { $0 == .bool }, "a bool") }
    public func bytes<V: Term>(_ c: Col<R, V>) -> Columns<R> { return adding(c, { $0 == .bytes }, "bytes") }

    /// A text column holding one of these variants.
    public func `enum`<V: Term>(_ c: Col<R, V>, _ variants: [String]) -> Columns<R> {
        var me = adding(c, { $0 == .text }, "text")
        me.cols[me.cols.count - 1].ty = .enumOf(variants)
        return me
    }

    /// The column just added may be null.
    public func nullable() -> Columns<R> {
        var me = self
        me.cols[me.cols.count - 1].nullable = true
        return me
    }

    /// The id column just added references a row of `P`.
    public func refs<P: Row>(_ p: P.Type) -> Columns<R> {
        var me = self
        me.refs.append(Ref(me.cols[me.cols.count - 1].name, P.NAME))
        return me
    }

    public func key(_ cs: AnyColumn...) -> Columns<R> {
        var me = self
        me.key = cs.map { $0.name }
        return me
    }

    public func unique(_ cs: AnyColumn...) -> Columns<R> {
        var me = self
        me.indexes.append(Index(cs.map { $0.name }, unique: true))
        return me
    }

    public func index(_ cs: AnyColumn...) -> Columns<R> {
        var me = self
        me.indexes.append(Index(cs.map { $0.name }, unique: false))
        return me
    }
}

// MARK: - §2.3 tables

/// What a mutator's body is: the write (or writes) it made. A write's
/// `.on(...)` says what an insert or upsert matches on.
public final class Effect {
    enum Target {
        case nothing
        case emitted(BlockBox, Swift.Int)
        case pending(PendingWrite)
    }
    let target: Target
    init(_ t: Target) { target = t }
    static var none: Effect { return Effect(.nothing) }

    /// Match on these columns — a unique index — rather than the key.
    @discardableResult
    public func on(_ cols: AnyColumn...) -> Effect {
        let names = cols.map { $0.name }
        switch target {
        case .emitted(let box, let i):
            switch box.stmts[i] {
            case .sInsert(let t, let e, _): box.stmts[i] = .sInsert(t, e, names)
            case .sUpsert(let t, let e, _): box.stmts[i] = .sUpsert(t, e, names)
            default: preconditionFailure("ArkAuthoring: .on after something that is not an insert or an upsert")
            }
        case .pending(let p):
            p.on = names
        case .nothing:
            break
        }
        return self
    }
}

/// A predicate over one row of `R`.
public struct Pred<R: Row> {
    let p: ArkDB.Pred
    public func and(_ b: Pred<R>) -> Pred<R> { return Pred(p: .pall([p, b.p])) }
    public func or(_ b: Pred<R>) -> Pred<R> { return Pred(p: .pany([p, b.p])) }
    public func not() -> Pred<R> { return Pred(p: .pnot(p)) }
}

/// One column of an order.
public struct Order<R: Row> {
    let by: OrderBy
}

extension Col {
    func pcmp(_ o: CmpOp, _ v: V) -> Pred<R> { return Pred(p: .pcmp(name, o, lift(v.repr))) }
    public func eq(_ v: V) -> Pred<R> { return pcmp(.eq, v) }
    public func ne(_ v: V) -> Pred<R> { return pcmp(.ne, v) }
    public func lt(_ v: V) -> Pred<R> { return pcmp(.lt, v) }
    public func le(_ v: V) -> Pred<R> { return pcmp(.le, v) }
    public func gt(_ v: V) -> Pred<R> { return pcmp(.gt, v) }
    public func ge(_ v: V) -> Pred<R> { return pcmp(.ge, v) }

    /// The column's value is one of these.
    public func isIn(_ vs: List<V>) -> Pred<R> {
        switch vs.repr {
        case .e(.list(let items)): return Pred(p: .pin(name, items))
        case .v(.list(let items)): return Pred(p: .pin(name, items.map { .lit($0) }))
        default: preconditionFailure("ArkAuthoring: isIn takes a list written out, as the IR's `pin` does")
        }
    }

    public func asc() -> Order<R> { return Order(by: OrderBy(name, .asc)) }
    public func desc() -> Order<R> { return Order(by: OrderBy(name, .desc)) }
}

/// A select being built: `filter`, `orderBy`, `limit`, `with`, then `all`
/// or `first`.
public struct Query<R: Row> {
    var plan: Plan

    public func filter(_ p: Pred<R>) -> Query<R> {
        var q = self
        q.plan.filter = q.plan.filter.map { .pall([$0, p.p]) } ?? p.p
        return q
    }

    public func orderBy(_ os: Order<R>...) -> Query<R> {
        var q = self
        q.plan.order += os.map { $0.by }
        return q
    }

    public func limit(_ n: Swift.Int) -> Query<R> {
        var q = self
        q.plan.limit = n
        return q
    }

    /// Hang each row's children of a relationship beneath it, as a field
    /// of the relationship's name.
    public func with<C: Row>(_ r: Rel<R, C>) -> Query<R> {
        var q = self
        let child = RowSchema.table(C.self)
        guard let ref = child.refs.first(where: { $0.table == R.NAME }) else {
            preconditionFailure("ArkAuthoring: \(C.NAME) does not reference \(R.NAME)")
        }
        q.plan.related.append(Related(name: r.name, relation: Relation(parent: R.NAME, child: C.NAME, column: ref.column), plan: Plan(table: C.NAME)))
        return q
    }

    /// Every row the plan admits: `SLet s (ESelect plan)`.
    public func all() -> List<R> {
        return read(.select(plan)) { n in n.store.select(plan) }
    }

    /// The first row: `SLet s (ESelect plan{limit = 1})`, then
    /// `SLet t (EStd First [EVar s])`; the value is `EVar t` (as
    /// `Ark.Demo` lowers it).
    public func first() -> Opt<R> {
        var p = plan
        p.limit = 1
        let rows: List<R> = read(.select(p)) { n in n.store.select(p) }
        switch Ambient.mode {
        case .emit(let em): return Opt(repr: .e(em.read(.std(.first, [lift(rows.repr)]))))
        case .native: return rows.first()
        }
    }
}

/// A read: a `let` at once under Emit; the store's answer under Native,
/// after any pending write.
func read<T: Term>(_ e: Expr, _ native: (Native) -> Value) -> T {
    switch Ambient.mode {
    case .emit(let em): return T(repr: .e(em.read(e)))
    case .native(let n):
        n.flush()
        if n.stopped { return T(repr: .v(zero(T.ty))) }
        return T(repr: .v(native(n)))
    }
}

/// A write, both ways.
func write(_ stmt: @autoclosure () -> Stmt, _ native: (Native) -> Void) -> Effect {
    switch Ambient.mode {
    case .emit(let em):
        em.append(stmt())
        return Effect(.emitted(em.current, em.current.stmts.count - 1))
    case .native(let n):
        n.flush()
        if n.stopped { return .none }
        if !n.writable { n.bug("Impure write outside a mutator"); return .none }
        native(n)
        if let p = n.pending { return Effect(.pending(p)) }
        return .none
    }
}

/// A table of a scope: `db.playlist`.
public struct Table<R: Row>: AnyTable {
    public init() {}
    public init(from decoder: Decoder) throws { self.init() }
    static var table: ArkDB.Table { return RowSchema.table(R.self) }

    var query: Query<R> { return Query(plan: Plan(table: R.NAME)) }

    public func filter(_ p: Pred<R>) -> Query<R> { return query.filter(p) }
    public func orderBy(_ os: Order<R>...) -> Query<R> { var q = query; q.plan.order = os.map { $0.by }; return q }
    public func limit(_ n: Swift.Int) -> Query<R> { return query.limit(n) }
    public func with<C: Row>(_ r: Rel<R, C>) -> Query<R> { return query.with(r) }
    public func all() -> List<R> { return query.all() }
    public func first() -> Opt<R> { return query.first() }

    func getBy(_ key: [Repr]) -> Opt<R> {
        return read(.get(R.NAME, key.map(lift))) { n in n.store.get(R.NAME, key.map(n.value)) }
    }

    func existsBy(_ key: [Repr]) -> Bool {
        return read(.exists(R.NAME, key.map(lift))) { n in n.store.exists(R.NAME, key.map(n.value)) }
    }

    func deleteBy(_ key: [Repr]) -> Effect {
        return write(.sDelete(R.NAME, key.map(lift))) { n in n.record(n.store.tryDelete(R.NAME, key.map(n.value))) }
    }

    func updateBy(_ key: [Repr], _ f: (R) -> R) -> Effect {
        switch Ambient.mode {
        case .emit(let em):
            let x = em.fresh()
            let new = f(R(repr: .e(.variable(x))))
            em.append(.sUpdate(R.NAME, key.map(lift), x, lift(new.repr)))
            return Effect(.emitted(em.current, em.current.stmts.count - 1))
        case .native(let n):
            n.flush()
            if n.stopped { return .none }
            if !n.writable { n.bug("Impure write outside a mutator"); return .none }
            guard let old = n.store.getRow(R.NAME, key.map(n.value)) else { return .none }
            let new = f(R(repr: .v(.record(old))))
            if n.stopped { return .none }
            guard case .record(let row) = n.value(new.repr) else { n.bug("update: not a row"); return .none }
            n.record(n.store.tryUpdate(R.NAME, key.map(n.value), row))
            return .none
        }
    }

    /// Write the row unless one matches (on the key, or `.on(...)`).
    @discardableResult
    public func insert(_ row: R) -> Effect { return put(.insert, row) }

    /// Write the row; one that matches (on the key, or `.on(...)`) keeps its
    /// key and takes the rest.
    @discardableResult
    public func upsert(_ row: R) -> Effect { return put(.upsert, row) }

    func put(_ k: PendingWrite.Kind, _ row: R) -> Effect {
        let r = row.repr
        return write(k == .insert ? .sInsert(R.NAME, lift(r), []) : .sUpsert(R.NAME, lift(r), [])) { n in
            guard case .record(let m) = n.value(r) else { n.bug("TypeError expected Struct"); return }
            n.pending = PendingWrite(k, R.NAME, m)
        }
    }
}

// The key is positional and typed by the row's `Key`, so a wrong order
// does not compile.
extension Table {
    public func get<A: Term>(_ a: A) -> Opt<R> where R.Key == A { return getBy([a.repr]) }
    public func get<A: Term, B: Term>(_ a: A, _ b: B) -> Opt<R> where R.Key == (A, B) { return getBy([a.repr, b.repr]) }
    public func get<A: Term, B: Term, C: Term>(_ a: A, _ b: B, _ c: C) -> Opt<R> where R.Key == (A, B, C) { return getBy([a.repr, b.repr, c.repr]) }

    public func exists<A: Term>(_ a: A) -> Bool where R.Key == A { return existsBy([a.repr]) }
    public func exists<A: Term, B: Term>(_ a: A, _ b: B) -> Bool where R.Key == (A, B) { return existsBy([a.repr, b.repr]) }
    public func exists<A: Term, B: Term, C: Term>(_ a: A, _ b: B, _ c: C) -> Bool where R.Key == (A, B, C) { return existsBy([a.repr, b.repr, c.repr]) }

    @discardableResult
    public func delete<A: Term>(_ a: A) -> Effect where R.Key == A { return deleteBy([a.repr]) }
    @discardableResult
    public func delete<A: Term, B: Term>(_ a: A, _ b: B) -> Effect where R.Key == (A, B) { return deleteBy([a.repr, b.repr]) }
    @discardableResult
    public func delete<A: Term, B: Term, C: Term>(_ a: A, _ b: B, _ c: C) -> Effect where R.Key == (A, B, C) { return deleteBy([a.repr, b.repr, c.repr]) }

    @discardableResult
    public func update<A: Term>(_ a: A, _ f: (R) -> R) -> Effect where R.Key == A { return updateBy([a.repr], f) }
    @discardableResult
    public func update<A: Term, B: Term>(_ a: A, _ b: B, _ f: (R) -> R) -> Effect where R.Key == (A, B) { return updateBy([a.repr, b.repr], f) }
}
