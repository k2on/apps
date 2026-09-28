import Foundation
import ArkDB

// The two backends behind one API (AUTHORING.md §3). A body never learns
// which it runs under: every operation asks the ambient context. Under
// `Emit` every value holds an expression and every read, write and branch
// is recorded as IR; under `Native` every value holds a `Value` and the
// operations run at once against a transaction over the store.
//
// Inside this module the vocabulary's `Bool`, `Int` and `Id` shadow the
// standard library's and ArkDB's, so every host type is written qualified:
// `Swift.Bool`, `Swift.Int`, `ArkDB.Id`.

/// What a vocabulary value holds: an expression (Emit) or a value (Native).
public enum Repr {
    case e(Expr)
    case v(Value)
}

/// A block being recorded; a write's `Effect` keeps a reference to it so
/// that `.on(...)` can amend the statement just appended.
final class BlockBox {
    var stmts: [Stmt] = []
}

/// Module-wide state while emitting: the helpers met so far, in the order
/// they must precede their first caller.
final class Collector {
    var helpers: [String: Function] = [:]
    var ready: [Function] = []
}

/// Emit: records one function.
final class Emitter {
    let collector: Collector
    let kind: FnKind
    var nextSym = 0
    var blocks: [BlockBox] = [BlockBox()]
    var autos: [NamedAuto] = []
    /// No host exposes its binders' names, so none are recorded: the
    /// printer derives canonical ones (AUTHORING.md §6).
    let names: [Sym: String] = [:]

    init(_ collector: Collector, _ kind: FnKind) {
        self.collector = collector
        self.kind = kind
    }

    func fresh() -> Sym {
        let s = nextSym
        nextSym += 1
        return s
    }

    var current: BlockBox { return blocks[blocks.count - 1] }

    func append(_ s: Stmt) { current.stmts.append(s) }

    /// A read is a `let` at once; the value is its symbol.
    func read(_ e: Expr) -> Expr {
        let s = fresh()
        append(.sLet(s, e))
        return .variable(s)
    }

    /// Run a closure with a fresh block and hand back what it recorded.
    func record(_ body: () -> Void) -> Block {
        blocks.append(BlockBox())
        body()
        return blocks.removeLast().stmts
    }

    func auto(_ name: String, _ a: Auto) {
        precondition(!autos.contains { $0.name == name }, "ctx: the auto \"\(name)\" is drawn twice")
        autos.append(NamedAuto(name, a))
    }
}

/// A write not yet made: kept until the next operation so that `.on(...)`
/// can still say what it matches on.
final class PendingWrite {
    enum Kind { case insert, upsert }
    let kind: Kind
    let table: TableName
    let row: ArkDB.Row
    var on: [FieldName] = []
    init(_ kind: Kind, _ table: TableName, _ row: ArkDB.Row) { self.kind = kind; self.table = table; self.row = row }
}

/// Native: runs one procedure against a working copy of a store.
final class Native {
    var store: MemoryStore
    var changes: [Change] = []
    let ctx: ArkDB.Ctx
    let autos: Args
    let writable: Swift.Bool
    /// The verdict (or bug) that stopped the body. Once set, every further
    /// operation is inert and answers a default of its type: the store is
    /// never touched again and the runner reports this.
    var stop: Stop?
    var pending: PendingWrite?

    enum Stop {
        case refused(Refusal)
        case bug(String)
    }

    init(_ store: MemoryStore, _ ctx: ArkDB.Ctx, _ autos: Args, writable: Swift.Bool) {
        self.store = store
        self.ctx = ctx
        self.autos = autos
        self.writable = writable
    }

    var stopped: Swift.Bool { return stop != nil }

    func refuse(_ r: Refusal) { if stop == nil { stop = .refused(r) } }
    func bug(_ s: String) { if stop == nil { stop = .bug(s) } }

    func record(_ r: Result<Change?, Refusal>) {
        switch r {
        case .failure(let why): refuse(why)
        case .success(let ch): if let c = ch { changes.append(c) }
        }
    }

    /// Make the pending write, if any.
    func flush() {
        guard let p = pending else { return }
        pending = nil
        if stopped { return }
        switch p.kind {
        case .insert: record(store.tryInsert(p.table, p.row, on: p.on))
        case .upsert: record(store.tryUpsert(p.table, p.row, on: p.on))
        }
    }

    /// A value a native operation needs; a bug if it is an expression.
    func value(_ r: Repr) -> Value {
        switch r {
        case .v(let v): return v
        case .e: bug("an expression reached a native operation"); return .null
        }
    }
}

enum Mode {
    case emit(Emitter)
    case native(Native)
}

/// The ambient context, per thread, as a stack.
enum Ambient {
    final class Box { var stack: [Mode] = [] }
    static let key = "arkdb.authoring.ambient"

    static var box: Box {
        let d = Thread.current.threadDictionary
        if let b = d[key] as? Box { return b }
        let b = Box()
        d[key] = b
        return b
    }

    static var current: Mode? { return box.stack.last }

    static var mode: Mode {
        guard let m = box.stack.last else {
            fatalError("ArkAuthoring: the vocabulary was used outside a module's emit() or a procedure's run")
        }
        return m
    }

    static func with<T>(_ m: Mode, _ body: () throws -> T) rethrows -> T {
        let b = box
        b.stack.append(m)
        defer { b.stack.removeLast() }
        return try body()
    }

    static var emitter: Emitter? {
        if case .emit(let e) = mode { return e }
        return nil
    }
}

func lift(_ r: Repr) -> Expr {
    switch r {
    case .e(let e): return e
    case .v(let v): return .lit(v)
    }
}

/// A value of a type that stands in for one a stopped body can no longer
/// compute: the zero of its type.
func zero(_ t: Ty) -> Value {
    switch t {
    case .bool: return .bool(false)
    case .int: return .int(0)
    case .text: return .text("")
    case .bytes: return .bytes([])
    case .id: return .id(ArkDB.Id.nil_)
    case .enumOf(let vs): return .text(vs.first ?? "")
    case .option: return .null
    case .list: return .list([])
    case .structOf(let fs): return .record(fs.mapValues(zero))
    }
}

/// One operation, both ways: the expression under Emit; the value under
/// Native, where a refusal (an overflow, `unwrap` of none) stops the body.
func op<T: Term>(_ e: @autoclosure () -> Expr, _ native: (Native) throws -> Value) -> T {
    switch Ambient.mode {
    case .emit: return T(repr: .e(e()))
    case .native(let n):
        if n.stopped { return T(repr: .v(zero(T.ty))) }
        do {
            return T(repr: .v(try native(n)))
        } catch Fault.refuse(let t) {
            n.refuse(.refused(t))
        } catch Fault.bug(let t) {
            n.bug(t)
        } catch {
            n.bug("\(error)")
        }
        return T(repr: .v(zero(T.ty)))
    }
}

/// A standard function over terms.
func std<T: Term>(_ f: StdFn, _ args: [Repr]) -> T {
    return op(.std(f, args.map(lift))) { n in try Std.call(f, args.map(n.value)) }
}

/// `lowerCamel` to `snake_case`: how a Swift name becomes the IR's.
func snake(_ s: String) -> String {
    var out = ""
    for c in s {
        if c.isUppercase {
            if !out.isEmpty { out.append("_") }
            out.append(contentsOf: c.lowercased())
        } else {
            out.append(c)
        }
    }
    return out
}
