import Foundation

/// Who authored an entry: the user the authority verified, and the login.
public struct Ctx: Equatable {
    public var user: String
    public var session: String
    public init(user: String, session: String) { self.user = user; self.session = session }
}

/// Arguments (or autos) by name.
public typealias Args = [String: Value]

/// §6 The interpreter of IR closures: what generated code must mean, and
/// what a peer runs for a closure it was sent. Exactly `Ark.Eval`.
///
/// A `Refusal` is a verdict; a `Fault.bug` is a bug — a module the verifier
/// would have refused. The two never mix.
public enum Eval {
    /// What applying a mutator came to: the store after it with the changes
    /// in order, or the verdict (the store untouched).
    public enum Outcome {
        case applied(MemoryStore, [Change])
        case refused(Refusal)
    }

    // Why a block stopped.
    enum Stop: Error {
        case returned(Value?)
        case verdict(Refusal)
        case bug(String)
    }

    struct Env {
        var schema: Schema
        var helpers: [Function]
        var kind: FnKind
        var ctx: Ctx
        var args: Args
        var autos: Args
        var locals: [Sym: Value]
        /// What each provide middleware returned, by its name.
        var provided: [String: Value] = [:]
    }

    final class St {
        var store: MemoryStore
        var changes: [Change] = []
        init(_ s: MemoryStore) { store = s }
    }

    // MARK: §6.1 apply

    /// Find a mutator in a module by name and run its current closure.
    public static func apply(_ m: Module, _ name: String, _ ctx: Ctx, _ autos: Args, _ args: Args, _ st: MemoryStore) throws -> Outcome {
        let fn = try function(m, name)
        return try applyClosure(m.schema, Hash.closure(m, fn), ctx, autos, args, st)
    }

    /// Run a closure against a store. The store passed is never written:
    /// the outcome carries a new one. Throws `Fault.bug` for a bug.
    ///
    /// AUTHORING.md §1.2: decode the input, run its checks (a failing one
    /// is a refusal), run each middleware in the procedure's `uses` order
    /// (a refusal stops there), then the body — all of it one transaction.
    public static func applyClosure(_ sch: Schema, _ c: Closure, _ ctx: Ctx, _ autos: Args, _ args: Args, _ st: MemoryStore) throws -> Outcome {
        let fn = c.fn
        guard fn.kind == .mutator else { throw Fault.bug("WrongKind \(fn.name)") }
        for a in fn.autos where autos[a.name] == nil { throw Fault.bug("MissingAuto \(a.name)") }
        for a in fn.input where args[a.name] == nil { throw Fault.bug("MissingArg \(a.name)") }
        let env0 = Env(schema: sch, helpers: c.helpers, kind: .mutator, ctx: ctx, args: args, autos: autos, locals: [:])
        let s = St(st.clone())
        do {
            let env = try prelude(env0, fn, s)
            _ = try block(env, fn.body, s)
            return .applied(s.store, s.changes)
        } catch Stop.returned {
            return .applied(s.store, s.changes)
        } catch Stop.verdict(let r) {
            return .refused(r)
        } catch Stop.bug(let e) {
            throw Fault.bug(e)
        }
    }

    /// The input's checks and then the middleware, as every procedure runs
    /// them before its body: the environment the body runs in — arguments
    /// normalised, provided values bound.
    static func prelude(_ env: Env, _ fn: Function, _ s: St) throws -> Env {
        var e = env
        e.args = try inputChecks(env, fn, s)
        for name in fn.uses {
            guard let mw = env.helpers.first(where: { $0.name == name }) else { throw Stop.bug("UnknownFunction \(name)") }
            guard mw.kind.isMiddleware else { throw Stop.bug("WrongKind \(name)") }
            let menv = Env(schema: env.schema, helpers: env.helpers, kind: mw.kind, ctx: env.ctx, args: e.args, autos: env.autos, locals: [:])
            do {
                _ = try block(menv, mw.body, s)
                if mw.kind == .provide { throw Stop.bug("NoReturn \(name)") }
            } catch Stop.returned(let v) {
                if mw.kind == .provide {
                    guard let v = v else { throw Stop.bug("NoReturn \(name)") }
                    e.provided[name] = v
                }
            }
        }
        return e
    }

    /// §1.3 Every field's checks in order, then the whole-input
    /// refinements: the normalised arguments, or a verdict.
    static func inputChecks(_ env: Env, _ fn: Function, _ s: St) throws -> Args {
        var args = env.args
        for nf in fn.input {
            guard let v0 = args[nf.name] else { throw Stop.bug("MissingArg \(nf.name)") }
            let r = try Checks.field(nf.name, nf.field, v0, exists: { t, k in s.store.getRow(t, [k]) != nil }, refine: { e, v in
                var a = args
                a[nf.name] = v
                var renv = env
                renv.args = a
                return try bool(try eval(renv, e, s))
            })
            switch r {
            case .ok(let v): args[nf.name] = v
            case .failed(let msg): throw Stop.verdict(.refused(msg))
            }
        }
        for r in fn.refine {
            var renv = env
            renv.args = args
            if !(try bool(try eval(renv, r.expr, s))) { throw Stop.verdict(.refused(r.why ?? Checks.invalid)) }
        }
        return args
    }

    /// §1.3 The form validator: each present field's checks, trim first, and
    /// the normalised values beside the messages (one per field, its first
    /// failure). The whole-input refinements run only when every field is
    /// present, and report under the field name "".
    public static func check(_ sch: Schema, _ c: Closure, _ partial: Args, _ st: MemoryStore, ctx: Ctx = Ctx(user: "", session: "")) throws -> (messages: [(String, String)], values: Args) {
        let fn = c.fn
        let env = Env(schema: sch, helpers: c.helpers, kind: .query, ctx: ctx, args: partial, autos: [:], locals: [:])
        let s = St(st)
        var values = partial
        var messages: [(String, String)] = []
        do {
            for nf in fn.input {
                guard let v0 = partial[nf.name] else { continue }
                let r = try Checks.field(nf.name, nf.field, v0, exists: { t, k in s.store.getRow(t, [k]) != nil }, refine: { e, v in
                    var renv = env
                    renv.args = values
                    renv.args[nf.name] = v
                    return try bool(try eval(renv, e, s))
                })
                switch r {
                case .ok(let v): values[nf.name] = v
                case .failed(let msg): messages.append((nf.name, msg))
                }
            }
            if fn.input.allSatisfy({ partial[$0.name] != nil }) {
                for r in fn.refine {
                    var renv = env
                    renv.args = values
                    if !(try bool(try eval(renv, r.expr, s))) { messages.append(("", r.why ?? Checks.invalid)) }
                }
            }
        } catch Stop.verdict(let r) {
            messages.append(("", r.text))
        } catch Stop.bug(let e) {
            throw Fault.bug(e)
        } catch Stop.returned {
            throw Fault.bug("return inside a check")
        }
        return (messages, values)
    }

    // MARK: §6.2 query

    public static func query(_ m: Module, _ name: String, _ args: Args, _ st: MemoryStore, ctx: Ctx = Ctx(user: "", session: "")) throws -> Value {
        let fn = try function(m, name)
        return try queryClosure(m.schema, Hash.closure(m, fn), args, st, ctx: ctx)
    }

    /// Run a query. A refusal — a failing check, a middleware's, an
    /// overflow — is thrown as `Fault.refuse`; a bug as `Fault.bug`.
    public static func queryClosure(_ sch: Schema, _ c: Closure, _ args: Args, _ st: MemoryStore, ctx: Ctx = Ctx(user: "", session: "")) throws -> Value {
        switch try queryResult(sch, c, args, st, ctx: ctx) {
        case .success(let v): return v
        case .failure(let r): throw r.fault
        }
    }

    /// Run a query: `Either Refusal Value`, a bug thrown as `Fault.bug`.
    public static func queryResult(_ sch: Schema, _ c: Closure, _ args: Args, _ st: MemoryStore, ctx: Ctx = Ctx(user: "", session: "")) throws -> Result<Value, Refusal> {
        let fn = c.fn
        guard fn.kind == .query else { throw Fault.bug("WrongKind \(fn.name)") }
        for a in fn.input where args[a.name] == nil { throw Fault.bug("MissingArg \(a.name)") }
        let env0 = Env(schema: sch, helpers: c.helpers, kind: .query, ctx: ctx, args: args, autos: [:], locals: [:])
        let s = St(st)
        do {
            let env = try prelude(env0, fn, s)
            _ = try block(env, fn.body, s)
            throw Fault.bug("NoReturn \(fn.name)")
        } catch Stop.returned(let v) {
            guard let v = v else { throw Fault.bug("NoReturn \(fn.name)") }
            return .success(v)
        } catch Stop.verdict(let r) {
            return .failure(r)
        } catch Stop.bug(let e) {
            throw Fault.bug(e)
        }
    }

    /// Run a helper on its arguments, with no store at all.
    public static func evalHelper(_ m: Module, _ name: String, _ vals: [Value]) throws -> Value {
        let fn = try function(m, name)
        let c = Hash.closure(m, fn)
        let env = Env(schema: m.schema, helpers: c.helpers, kind: .helper, ctx: Ctx(user: "", session: ""), args: [:], autos: [:], locals: [:])
        let s = St(MemoryStore(schema: m.schema))
        do {
            return try call(env, fn, vals, s)
        } catch Stop.returned {
            throw Fault.bug("NoReturn \(name)")
        } catch Stop.verdict(let r) {
            throw r.fault
        } catch Stop.bug(let e) {
            throw Fault.bug(e)
        }
    }

    static func function(_ m: Module, _ name: String) throws -> Function {
        guard let fn = m.lookupFunction(name) else { throw Fault.bug("UnknownFunction \(name)") }
        return fn
    }

    // MARK: §6.3 statements

    static func block(_ env: Env, _ stmts: Block, _ s: St) throws -> Env {
        var e = env
        for st in stmts { e = try exec(e, st, s) }
        return e
    }

    static func exec(_ env: Env, _ stmt: Stmt, _ s: St) throws -> Env {
        switch stmt {
        case .sLet(let x, let e):
            let v = try eval(env, e, s)
            return bind(x, v, env)
        case .sIf(let c, let yes, let no):
            let b = try bool(try eval(env, c, s))
            _ = try block(env, b ? yes : no, s)
            return env
        case .sFor(let x, let xs, let body):
            let vs = try list(try eval(env, xs, s))
            for v in vs { _ = try block(bind(x, v, env), body, s) }
            return env
        case .sInsert(let t, let e, let on):
            try mutating(env)
            let row = try structOf(try eval(env, e, s))
            try record(s, s.store.tryInsert(t, row, on: on))
            return env
        case .sUpsert(let t, let e, let on):
            try mutating(env)
            let row = try structOf(try eval(env, e, s))
            try record(s, s.store.tryUpsert(t, row, on: on))
            return env
        case .sUpdate(let t, let ks, let x, let e):
            try mutating(env)
            var key: [Value] = []
            for k in ks { key.append(try eval(env, k, s)) }
            guard let old = s.store.getRow(t, key) else { return env }
            let row = try structOf(try eval(bind(x, .record(old), env), e, s))
            try record(s, s.store.tryUpdate(t, key, row))
            return env
        case .sDelete(let t, let ks):
            try mutating(env)
            var key: [Value] = []
            for k in ks { key.append(try eval(env, k, s)) }
            try record(s, s.store.tryDelete(t, key))
            return env
        case .sRefuse(let e):
            guard env.kind != .helper else { throw Stop.bug("Impure refuse inside a helper") }
            let t = try text(try eval(env, e, s))
            throw Stop.verdict(.refused(t))
        case .sReturn(let me):
            var v: Value? = nil
            if let e = me { v = try eval(env, e, s) }
            throw Stop.returned(v)
        }
    }

    static func record(_ s: St, _ r: Result<Change?, Refusal>) throws {
        switch r {
        case .failure(let why): throw Stop.verdict(why)
        case .success(let ch): if let c = ch { s.changes.append(c) }
        }
    }

    static func mutating(_ env: Env) throws {
        guard env.kind == .mutator else { throw Stop.bug("Impure write outside a mutator") }
    }

    static func reading(_ env: Env) throws {
        guard env.kind != .helper else { throw Stop.bug("Impure read inside a helper") }
    }

    static func bind(_ x: Sym, _ v: Value, _ env: Env) -> Env {
        var e = env
        e.locals[x] = v
        return e
    }

    // MARK: §6.4 expressions

    static func eval(_ env: Env, _ expr: Expr, _ s: St) throws -> Value {
        switch expr {
        case .lit(let v): return v
        case .arg(let a):
            guard let v = env.args[a] else { throw Stop.bug("MissingArg \(a)") }
            return v
        case .auto(let a):
            guard let v = env.autos[a] else { throw Stop.bug("MissingAuto \(a)") }
            return v
        case .variable(let x):
            guard let v = env.locals[x] else { throw Stop.bug("UnboundVar \(x)") }
            return v
        case .ctxUser: return .text(env.ctx.user)
        case .ctxSession: return .text(env.ctx.session)
        case .provided(let n):
            guard let v = env.provided[n] else { throw Stop.bug("NotProvided \(n)") }
            return v
        case .field(let e, let f):
            let m = try structOf(try eval(env, e, s))
            guard let v = m[f] else { throw Stop.bug("NoSuchField \(f)") }
            return v
        case .structOf(let fs):
            // Fields are evaluated in field-name order.
            var m: [String: Value] = [:]
            for k in Encode.sortedFieldNamesOf(fs) { m[k] = try eval(env, fs[k]!, s) }
            return .record(m)
        case .list(let es):
            var out: [Value] = []
            for e in es { out.append(try eval(env, e, s)) }
            return .list(out)
        case .some(let e): return try eval(env, e, s)
        case .none: return .null
        case .match(let e, let x, let some, let none):
            let v = try eval(env, e, s)
            if v.isNull() { return try eval(env, none, s) }
            return try eval(bind(x, v, env), some, s)
        case .ife(let c, let a, let b):
            let t = try bool(try eval(env, c, s))
            return try eval(env, t ? a : b, s)
        case .op(let op, let es):
            switch (op, es.count) {
            case (.and, 2):
                let x = try bool(try eval(env, es[0], s))
                return x ? try eval(env, es[1], s) : .bool(false)
            case (.or, 2):
                let x = try bool(try eval(env, es[0], s))
                return x ? .bool(true) : try eval(env, es[1], s)
            case (.not, 1):
                return .bool(!(try bool(try eval(env, es[0], s))))
            case (.neg, 1):
                let n = try int(try eval(env, es[0], s))
                if n == Int64.min { throw Stop.verdict(.refused("integer overflow")) }
                return .int(-n)
            case (.add, 2), (.sub, 2), (.mul, 2), (.div, 2), (.mod, 2):
                let x = try int(try eval(env, es[0], s))
                let y = try int(try eval(env, es[1], s))
                do {
                    return try Ops.arith(op, .int(x), .int(y))
                } catch Fault.refuse(let t) {
                    throw Stop.verdict(.refused(t))
                }
            default:
                throw Stop.bug("Arity \(op.rawValue)")
            }
        case .cmp(let op, let a, let b):
            let x = try eval(env, a, s)
            let y = try eval(env, b, s)
            return .bool(Ops.compare(op, x, y))
        case .call(let name, let es):
            var vals: [Value] = []
            for e in es { vals.append(try eval(env, e, s)) }
            guard let fn = env.helpers.first(where: { $0.name == name }) else { throw Stop.bug("UnknownFunction \(name)") }
            guard fn.kind == .helper else { throw Stop.bug("WrongKind \(name)") }
            return try call(env, fn, vals, s)
        case .std(let f, let es):
            var vals: [Value] = []
            for e in es { vals.append(try eval(env, e, s)) }
            do {
                return try Std.call(f, vals)
            } catch Fault.refuse(let t) {
                throw Stop.verdict(.refused(t))
            } catch Fault.bug(let t) {
                throw Stop.bug(t)
            }
        case .map(let xs, let x, let body):
            let vs = try list(try eval(env, xs, s))
            var out: [Value] = []
            for v in vs { out.append(try eval(bind(x, v, env), body, s)) }
            return .list(out)
        case .filter(let xs, let x, let body):
            let vs = try list(try eval(env, xs, s))
            var out: [Value] = []
            for v in vs { if try bool(try eval(bind(x, v, env), body, s)) { out.append(v) } }
            return .list(out)
        case .any(let xs, let x, let body):
            let vs = try list(try eval(env, xs, s))
            var r = false
            for v in vs { if try bool(try eval(bind(x, v, env), body, s)) { r = true } }
            return .bool(r)
        case .all(let xs, let x, let body):
            let vs = try list(try eval(env, xs, s))
            var r = true
            for v in vs { if !(try bool(try eval(bind(x, v, env), body, s))) { r = false } }
            return .bool(r)
        case .sortBy(let xs, let x, let key):
            let vs = try list(try eval(env, xs, s))
            var keyed: [(Value, Value)] = []
            for v in vs { keyed.append((v, try eval(bind(x, v, env), key, s))) }
            return .list(stableSort(keyed) { compareValue($0.1, $1.1) }.map { $0.0 })
        case .fold(let xs, let z, let acc, let x, let body):
            let vs = try list(try eval(env, xs, s))
            var a = try eval(env, z, s)
            for v in vs { a = try eval(bind(acc, a, bind(x, v, env)), body, s) }
            return a
        case .select(let p):
            try reading(env)
            do {
                return .list(try select(schema: env.schema, store: s.store, plan: p) { try eval(env, $0, s) })
            } catch Fault.bug(let t) {
                throw Stop.bug(t)
            }
        case .get(let t, let ks):
            try reading(env)
            var key: [Value] = []
            for k in ks { key.append(try eval(env, k, s)) }
            return s.store.get(t, key)
        case .exists(let t, let ks):
            try reading(env)
            var key: [Value] = []
            for k in ks { key.append(try eval(env, k, s)) }
            return s.store.exists(t, key)
        }
    }

    /// Call a helper: a fresh environment of its arguments alone.
    static func call(_ env: Env, _ fn: Function, _ vals: [Value], _ s: St) throws -> Value {
        guard vals.count == fn.input.count else { throw Stop.bug("Arity \(fn.name)") }
        var args: Args = [:]
        for (a, v) in zip(fn.input, vals) { args[a.name] = v }
        let env2 = Env(schema: env.schema, helpers: env.helpers, kind: .helper, ctx: env.ctx, args: args, autos: [:], locals: [:])
        do {
            _ = try block(env2, fn.body, s)
            throw Stop.bug("NoReturn \(fn.name)")
        } catch Stop.returned(let v) {
            guard let v = v else { throw Stop.bug("NoReturn \(fn.name)") }
            return v
        }
    }

    // MARK: §6.6 select

    /// Pull a plan: scan, keep what the filter admits, sort stably by the
    /// order, take the limit, and hang each relationship's rows beneath.
    /// The right-hand sides are evaluated once, before the scan, by `rhs`.
    /// Generic over any `Store`, through `scan`. Throws `Fault.bug` for a
    /// table the schema lacks or a composite parent key.
    public static func select(schema: Schema, store: Store, plan p: Plan, rhs: (Expr) throws -> Value) throws -> [Value] {
        guard let tbl = schema.lookupTable(p.table) else { throw Fault.bug("UnknownTable \(p.table)") }
        let keep = try predicate(p.filter, rhs)
        let admitted = store.scan(p.table).filter(keep)
        let ordered = stableSort(admitted) { orderBy(p.order, $0, $1) }
        let taken = p.limit.map { Array(ordered.prefix(Swift.max(0, $0))) } ?? ordered
        var out: [Value] = []
        for row in taken { out.append(try attach(schema: schema, store: store, tbl, p.related, row, rhs)) }
        return out
    }

    static func attach(schema: Schema, store: Store, _ tbl: Table, _ rels: [Related], _ row: Row, _ rhs: (Expr) throws -> Value) throws -> Value {
        let k = tbl.keyOf(row)
        let pk: Value
        if k.count == 1 {
            pk = k[0]
        } else if rels.isEmpty {
            pk = .null
        } else {
            throw Fault.bug("CompositeParentKey \(tbl.name)")
        }
        var node = row
        for r in rels {
            let pin = Pred.pcmp(r.relation.column, .eq, .lit(pk))
            var child = r.plan
            child.filter = child.filter.map { .pall([pin, $0]) } ?? pin
            node[r.name] = .list(try select(schema: schema, store: store, plan: child, rhs: rhs))
        }
        return .record(node)
    }

    static func predicate(_ p: Pred?, _ rhs: (Expr) throws -> Value) throws -> (Row) -> Bool {
        guard let p = p else { return { _ in true } }
        switch p {
        case .pcmp(let c, let op, let e):
            let v = try rhs(e)
            return { row in Ops.compare(op, row[c] ?? .null, v) }
        case .pin(let c, let es):
            var vs: [Value] = []
            for e in es { vs.append(try rhs(e)) }
            return { row in vs.contains { Ops.compare(.eq, row[c] ?? .null, $0) } }
        case .pall(let ps):
            var fs: [(Row) -> Bool] = []
            for q in ps { fs.append(try predicate(q, rhs)) }
            return { row in fs.allSatisfy { $0(row) } }
        case .pany(let ps):
            var fs: [(Row) -> Bool] = []
            for q in ps { fs.append(try predicate(q, rhs)) }
            return { row in fs.contains { $0(row) } }
        case .pnot(let q):
            let f = try predicate(q, rhs)
            return { row in !f(row) }
        }
    }

    /// The plan's order alone; ties keep scan (key) order under a stable sort.
    public static func orderBy(_ cols: [OrderBy], _ a: Row, _ b: Row) -> Int {
        for c in cols {
            let o = compareValue(a[c.column] ?? .null, b[c.column] ?? .null)
            if o != 0 { return c.dir == .asc ? o : -o }
        }
        return 0
    }

    // MARK: coercions: a bug when the verifier's type does not hold

    static func bool(_ v: Value) throws -> Bool {
        if case .bool(let b) = v { return b }
        throw Stop.bug("TypeError expected Bool, got \(v.brief)")
    }

    static func int(_ v: Value) throws -> Int64 {
        if case .int(let n) = v { return n }
        throw Stop.bug("TypeError expected Int, got \(v.brief)")
    }

    static func text(_ v: Value) throws -> String {
        if case .text(let t) = v { return t }
        throw Stop.bug("TypeError expected Text, got \(v.brief)")
    }

    static func list(_ v: Value) throws -> [Value] {
        if case .list(let xs) = v { return xs }
        throw Stop.bug("TypeError expected List, got \(v.brief)")
    }

    static func structOf(_ v: Value) throws -> Row {
        if case .record(let m) = v { return m }
        throw Stop.bug("TypeError expected Struct, got \(v.brief)")
    }
}
