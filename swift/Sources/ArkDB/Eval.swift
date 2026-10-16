// §6 Evaluation: what a domain means (`rust/ark/src/eval.rs`). An
// interpreter of the IR over any `Store`. A read — a query's plan, a
// mutator's `select` — means what `View.pull` answers, and is asked of it
// through `View.read`: that is the one evaluator of plans, and it
// evaluates the expressions inside one through `Scope`, which is this
// file's evaluator with the plan's binders in scope.
//
// A procedure runs as the spec has it: its input checked field by field,
// then each middleware in its `uses` order, then the body — one
// transaction, and a refusal anywhere leaves the store untouched. A
// `Refusal` is a verdict every replica reaches; an `EvalError` is a bug.
// Evaluation order is part of the meaning: `and`/`or` short-circuit left
// to right, `if` and `match` evaluate only the taken arm, list elements
// and call arguments left to right, a struct's fields in field-name order.

/// Who authored an entry (`eval::Ctx`): the user the authority verified,
/// the login, the roles frozen in the entry, and whether this run is the
/// authority's — which alone runs private blocks (D3).
public struct Ctx: Hashable {
    public var user: String
    public var session: String
    /// In code point order, each once.
    public private(set) var roles: [String]
    public var authority: Bool

    public init(_ user: String, _ session: String, roles: [String] = [], authority: Bool = false) {
        self.user = user
        self.session = session
        self.roles = Ctx.sortedRoles(roles)
        self.authority = authority
    }

    static func sortedRoles(_ rs: [String]) -> [String] {
        SortedSet(rs.map(CodePoints.init)).items.map { $0.s }
    }

    /// Whether this is `Ctx.nobody`.
    public var isNobody: Bool { user.isEmpty && session.isEmpty }

    /// The same author, holding these roles.
    public func withRoles(_ roles: [String]) -> Ctx {
        var c = self
        c.roles = Ctx.sortedRoles(roles)
        return c
    }

    /// The same author, run as the authority runs it.
    public func asAuthority(_ authority: Bool) -> Ctx {
        var c = self
        c.authority = authority
        return c
    }

    public func hasRole(_ r: String) -> Bool { roles.contains { textEquals($0, r) } }

    /// §11.2a Who authors before anyone has signed in.
    public static let nobody = Ctx("", "")
}

/// A bug, never a verdict (`eval::EvalError`), as the reference's `{:?}` writes it.
public enum EvalError: Hashable, CustomStringConvertible {
    case unknownFunction(String)
    case wrongKind(String, FnKind)
    case missingArg(String)
    case missingAuto(String)
    case unboundVar(Sym)
    case noSuchField(String)
    case typeError(String)
    case arity(String)
    case noReturn(String)
    case impure(String)
    case unknownTable(String)
    case compositeParentKey(String)
    case notProvided(String)

    public var description: String {
        switch self {
        case .unknownFunction(let n): return "UnknownFunction(\(debugString(n)))"
        case .wrongKind(let n, let k): return "WrongKind(\(debugString(n)), \(k.debugName))"
        case .missingArg(let n): return "MissingArg(\(debugString(n)))"
        case .missingAuto(let n): return "MissingAuto(\(debugString(n)))"
        case .unboundVar(let s): return "UnboundVar(\(s))"
        case .noSuchField(let n): return "NoSuchField(\(debugString(n)))"
        case .typeError(let w): return "TypeError(\(debugString(w)))"
        case .arity(let w): return "Arity(\(debugString(w)))"
        case .noReturn(let n): return "NoReturn(\(debugString(n)))"
        case .impure(let w): return "Impure(\(debugString(w)))"
        case .unknownTable(let t): return "UnknownTable(\(debugString(t)))"
        case .compositeParentKey(let t): return "CompositeParentKey(\(debugString(t)))"
        case .notProvided(let n): return "NotProvided(\(debugString(n)))"
        }
    }
}

/// The two ways a computation stops short (`eval::EvalFault`), thrown.
public enum EvalFault: Error, CustomStringConvertible {
    case verdict(Refusal)
    case bug(EvalError)

    public var description: String {
        switch self {
        case .verdict(let r): return r.show()
        case .bug(let e): return e.description
        }
    }
}

/// What a mutator run came to: its changes, applied, or the verdict that
/// left the store as it was.
public enum Ran: Equatable {
    case ok([Change])
    case refused(Refusal)
}

/// What the form validator says (`eval::Checked`): a message per field that fails, and the values.
public struct Checked {
    public let messages: [(String, String)]
    public let values: [String: Value]
}

// §6.4 A local: one per binding, pushed over the one before.
final class Frame {
    let sym: Sym
    let value: Value
    let up: Frame?
    init(_ sym: Sym, _ value: Value, _ up: Frame?) {
        self.sym = sym
        self.value = value
        self.up = up
    }
}

// Everything an expression is evaluated in.
final class Env {
    let schema: Schema
    let helpers: [Function]
    let kind: FnKind
    let ctx: Ctx
    let args: [String: Value]
    let autos: [String: Value]
    let provided: [String: Value]
    // A plan node's binders, under every frame.
    let node: [(Sym, Value)]
    let locals: Frame?
    // §13 In a plan's node (and a helper one calls), a sum's fold is judged on its result alone.
    let wide: Bool
    // §3.12 Inside a live state's transition: the state, the time and the device.
    let live: LiveEnv?

    init(_ schema: Schema, _ helpers: [Function], _ kind: FnKind, _ ctx: Ctx, _ args: [String: Value], _ autos: [String: Value], _ provided: [String: Value], _ node: [(Sym, Value)], _ locals: Frame?, _ wide: Bool, _ live: LiveEnv? = nil) {
        self.schema = schema
        self.helpers = helpers
        self.kind = kind
        self.ctx = ctx
        self.args = args
        self.autos = autos
        self.provided = provided
        self.node = node
        self.locals = locals
        self.wide = wide
        self.live = live
    }

    func local(_ x: Sym) -> Value? {
        var at = locals
        while let f = at {
            if f.sym == x { return f.value }
            at = f.up
        }
        var i = node.count - 1
        while i >= 0 {
            if node[i].0 == x { return node[i].1 }
            i -= 1
        }
        return nil
    }

    // This scope with one more local. Binding one leaves the node's sum rule, as the reference's does.
    func under(_ sym: Sym, _ v: Value) -> Env {
        Env(schema, helpers, kind, ctx, args, autos, provided, node, Frame(sym, v, locals), false, live)
    }

    func withFrame(_ f: Frame) -> Env { Env(schema, helpers, kind, ctx, args, autos, provided, node, f, false, live) }

    func copy(kind: FnKind? = nil, args: [String: Value]? = nil, node: [(Sym, Value)]? = nil, locals: Frame?? = nil, wide: Bool? = nil, autos: [String: Value]? = nil, provided: [String: Value]? = nil) -> Env {
        Env(schema, helpers, kind ?? self.kind, ctx, args ?? self.args, autos ?? self.autos, provided ?? self.provided, node ?? self.node, locals ?? self.locals, wide ?? self.wide, live)
    }
}

// §3.12 What a transition reads beside its input and its author.
struct LiveEnv {
    let state: Value
    let now: Int64
    let device: String
}

/// §3.12 Who calls a transition, and when (`eval::LiveCtx`): the author, as
/// an entry's is, the device calling, and the time — the server's when it
/// sequences the call, the device's own guess of the server's when it
/// previews one.
public struct LiveCtx: Hashable {
    public var user: String
    public var session: String
    /// In code point order.
    public var roles: [String]
    public var device: String
    public var now: Int64

    public init(user: String = "", session: String = "", roles: [String] = [], device: String = "", now: Int64 = 0) {
        self.user = user
        self.session = session
        self.roles = roles
        self.device = device
        self.now = now
    }
}

// The store as it stands, the changes made so far, and the private blocks the
// authority's run reached, each with the locals it was written among.
final class St {
    let store: Store
    var changes = [Change]()
    var deferred = [(Block, [(Sym, Value)])]()
    init(_ store: Store) { self.store = store }
}

// The store an expression that may not read runs against: empty, never written.
final class NoStore: Store {
    let schema: Schema
    init(_ schema: Schema) { self.schema = schema }
    func get(_ table: String, _ key: [Value]) -> Row? { nil }
    func scan(_ table: String) -> [Row] { [] }
    func scanWhere(_ table: String, _ keep: Keep) -> [Row] { [] }
    func scanWhereEq(_ table: String, _ eq: [Eq], _ spans: [Span], _ keep: Keep) -> [Row] { [] }
    func scanOrdered(_ table: String, _ eq: [Eq], _ spans: [Span], _ order: [(String, Dir)], _ keep: Keep, _ limit: Int) -> [Row]? { nil }
    func scanWhereText(_ table: String, _ eq: [Eq], _ spans: [Span], _ has: [[(String, String)]], _ keep: Keep) -> [Row] { [] }
    func applyChange(_ change: Change) {}
    func exists(_ table: String, _ key: [Value]) -> Bool { false }
    func digest(_ table: String) -> Digest? { nil }
}

/// §1.3 What the expressions of a plan are evaluated in (`eval::Scope`): the
/// function's helpers, context, arguments and provided values, and the
/// locals bound around the read. A filter is evaluated here; everything a
/// node computes in a `NodeScope` of it.
public final class Scope {
    let outer: Env
    init(_ outer: Env) { self.outer = outer }

    public var schema: Schema { outer.schema }

    /// An expression constant for the read: a filter's right-hand side.
    public func eval(_ e: Expr) throws -> Value { try Eval.pure(outer, e) }

    /// A node's scope: nothing bound yet, no read, and a sum judged on its result.
    public func node() -> NodeScope { NodeScope(outer.copy(kind: .helper, node: [], locals: .some(nil), wide: true)) }

    /// A scope with no locals: a query's, or a read outside any procedure.
    public static func of(_ schema: Schema, _ helpers: [Function], _ ctx: Ctx, _ args: [String: Value], _ provided: [String: Value]) -> Scope {
        Scope(Env(schema, helpers, .helper, ctx, args, [:], provided, [], nil, false))
    }
}

/// One node's scope: its binders, bound as the plan evaluates them (`eval::NodeScope`).
public final class NodeScope {
    private let env: Env
    private var bound = [(Sym, Value)]()
    init(_ env: Env) { self.env = env }

    public func bind(_ x: Sym, _ v: Value) { bound.append((x, v)) }

    public func eval(_ e: Expr) throws -> Value { try Eval.pure(env.copy(node: bound), e) }
}

private struct Returned {
    let v: Value?
}

public enum Eval {
    static func bug(_ e: EvalError) -> EvalFault { .bug(e) }
    static func verdict(_ r: Refusal) -> EvalFault { .verdict(r) }

    /// §6.1 `apply`: a mutator by name, through its current closure.
    public static func apply(_ m: Module, _ name: String, _ ctx: Ctx, _ autos: [String: Value], _ args: [String: Value], _ store: Store) throws -> Ran {
        guard let f = m.lookupFunction(name) else { throw bug(.unknownFunction(name)) }
        return try applyClosure(m.schema, Hash.closure(m, f), ctx, autos, args, store)
    }

    /// §6.1 `apply_closure`: how an entry replays. The changes, already
    /// applied to the store — or the verdict, the store unchanged; a bug is
    /// thrown as `EvalFault.bug`.
    public static func applyClosure(_ sch: Schema, _ c: Closure, _ ctx: Ctx, _ autos: [String: Value], _ args: [String: Value], _ store: Store) throws -> Ran {
        let f = c.function
        if f.kind != .mutator { throw bug(.wrongKind(f.name, f.kind)) }
        for a in f.autos where autos[a.name] == nil { throw bug(.missingAuto(a.name)) }
        let st = St(Overlay(store))
        do {
            _ = try procedure(st, sch, c, ctx, autos, args)
        } catch EvalFault.verdict(let r) {
            return .refused(r)
        }
        store.applyChanges(st.changes)
        return .ok(st.changes)
    }

    /// §6.2 `query_closure`: a query's value, or a fault (a verdict is shown and recorded nowhere).
    public static func queryClosure(_ sch: Schema, _ c: Closure, _ ctx: Ctx, _ args: [String: Value], _ store: Store) throws -> Value {
        let f = c.function
        if f.kind != .query { throw bug(.wrongKind(f.name, f.kind)) }
        guard let v = try procedure(St(Overlay(store)), sch, c, ctx, [:], args) else { throw bug(.noReturn(f.name)) }
        return v
    }

    /// §6.2 `query_as`: a query by name, as someone.
    public static func queryAs(_ m: Module, _ name: String, _ ctx: Ctx, _ args: [String: Value], _ store: Store) throws -> Value {
        guard let f = m.lookupFunction(name) else { throw bug(.unknownFunction(name)) }
        return try queryClosure(m.schema, Hash.closure(m, f), ctx, args, store)
    }

    /// §1.7 `middleware`: the input checked, then each middleware in `uses`
    /// order, over `store`: the checked input and what each provide
    /// returned. Nothing is written.
    public static func middleware(_ sch: Schema, _ c: Closure, _ ctx: Ctx, _ args: [String: Value], _ store: Store) throws -> ([String: Value], [String: Value]) {
        try preamble(St(Overlay(store)), sch, c, ctx, args)
    }

    // A procedure, whole: preamble, then the body (or the plan), then — at the
    // authority only — the private blocks the body reached.
    private static func procedure(_ st: St, _ sch: Schema, _ c: Closure, _ ctx: Ctx, _ autos: [String: Value], _ args0: [String: Value]) throws -> Value? {
        let f = c.function
        let (args, provided) = try preamble(st, sch, c, ctx, args0)
        let env = Env(sch, c.helpers, f.kind, ctx, args, autos, provided, [], nil, false)
        if f.kind == .query, let plan = f.plan { return .list(try View.read(sch, plan, Scope(env), st.store)) }
        let out = try block(st, env, f.body)?.v
        let deferred = st.deferred
        st.deferred.removeAll()
        for (b, locals) in deferred { _ = try block(st, env.copy(node: locals), b) }
        return out
    }

    // The input checked, then the middleware.
    private static func preamble(_ st: St, _ sch: Schema, _ c: Closure, _ ctx: Ctx, _ args0: [String: Value]) throws -> ([String: Value], [String: Value]) {
        let f = c.function
        let args = try checkedInput(st, sch, c, ctx, args0)
        var provided = [String: Value]()
        for u in f.uses {
            guard let mw = c.helpers.first(where: { $0.name == u }) else { throw bug(.unknownFunction(u)) }
            if !mw.kind.isMiddleware { throw bug(.wrongKind(u, mw.kind)) }
            // D2 A scope runs nothing.
            if mw.kind == .scope { continue }
            let env = Env(sch, c.helpers, mw.kind, ctx, args, [:], provided, [], nil, false)
            let r = try block(st, env, mw.body)
            if mw.kind == .guardFn && (r == nil || r!.v == nil) {
                continue
            } else if mw.kind == .provide, let v = r?.v {
                provided[u] = v
            } else {
                throw bug(.noReturn(u))
            }
        }
        return (args, provided)
    }

    // §1.3 The input, decoded and checked.
    private static func checkedInput(_ st: St, _ sch: Schema, _ c: Closure, _ ctx: Ctx, _ args0: [String: Value]) throws -> [String: Value] {
        let f = c.function
        var args = args0
        for i in f.input where args[i.name] == nil { throw bug(.missingArg(i.name)) }
        for i in f.input {
            let n = i.name
            let v = args[n]!
            let now = args
            let (v2, msg) = try checkField(n, i.value, v, { t, k in st.store.exists(t, [k]) }) { e, x in
                var local = now
                local[n] = x
                let env = Env(sch, c.helpers, .helper, ctx, local, [:], [:], [], nil, false)
                return try pureBool(St(Overlay(MemoryStore.empty(sch))), env, e)
            }
            if let m = msg { throw verdict(.refused(m)) }
            args[n] = v2
        }
        for r in f.refine {
            let env = Env(sch, c.helpers, .helper, ctx, args, [:], [:], [], nil, false)
            if try !pureBool(st, env, r.e) { throw verdict(.refused(r.why ?? "invalid")) }
        }
        return args
    }

    // A check's expression: pure, a Bool.
    private static func pureBool(_ st: St, _ env: Env, _ e: Expr) throws -> Bool {
        let v = try eval(st, env, e)
        guard case .bool(let b) = v else { throw bug(.typeError("a check is a Bool, not \(v.show())")) }
        return b
    }

    /// §1.3 `default_message`: the message a check refuses with when it names none.
    public static func defaultMessage(_ field: String, _ check: Check, _ ty: Ty) -> String {
        switch check {
        case .trim: return "\(field): invalid"
        case .minLen(let n, _): return "\(field): at least \(n) characters"
        case .maxLen(let n, _): return "\(field): at most \(n) characters"
        case .range(let lo, let hi, _):
            if let l = lo, let h = hi { return "\(field): between \(l) and \(h)" }
            if let l = lo { return "\(field): at least \(l)" }
            if let h = hi { return "\(field): at most \(h)" }
            return "\(field): invalid"
        case .nonEmpty: return "\(field): at least one"
        case .exists: return "\(field): no such \(idTable(ty) ?? "row")"
        case .refine: return "\(field): invalid"
        }
    }

    /// `id_table`: the table an id field names, through an option.
    public static func idTable(_ ty: Ty) -> String? {
        switch ty {
        case .id(let t): return t
        case .option(let o): return idTable(o)
        default: return nil
        }
    }

    /// §1.3 `check_field`: one field's checks, in order — the value as
    /// normalised so far (a `trim` rewrites it for every later check and the
    /// body) and the message of the first check that fails. A field of an
    /// option type is checked only when it is `Some`.
    public static func checkField(_ name: String, _ field: Field, _ value: Value, _ exists: (String, Value) -> Bool, _ refine: (Expr, Value) throws -> Bool) throws -> (Value, String?) {
        if value.isNull, case .option = field.ty { return (value, nil) }
        func bad(_ what: String) -> EvalFault { bug(.typeError("\(name): \(what)")) }
        var v = value
        for c in field.checks {
            var ok: Bool
            switch c {
            case .trim:
                guard case .text(let t) = v else { throw bad("trim of a value that is not text") }
                v = .text(Std.trim(t))
                ok = true
            case .minLen(let n, _), .maxLen(let n, _):
                guard case .text(let t) = v else { throw bad("a length check on a value that is not text") }
                let len = Int64(t.unicodeScalars.count)
                if case .minLen = c { ok = len >= n } else { ok = len <= n }
            case .range(let lo, let hi, _):
                guard case .int(let x) = v else { throw bad("a range on a value that is not an int") }
                ok = (lo == nil || x >= lo!) && (hi == nil || x <= hi!)
            case .nonEmpty:
                guard case .list(let xs) = v else { throw bad("non_empty on a value that is not a list") }
                ok = !xs.isEmpty
            case .exists:
                guard let t = idTable(field.ty) else { throw bad("exists on a value that is not an id") }
                ok = exists(t, v)
            case .refine(let e, _):
                ok = try refine(e, v)
            }
            if !ok { return (v, c.given ?? defaultMessage(name, c, field.ty)) }
        }
        return (v, nil)
    }

    /// §1.3 `check`, the form validator: each field present through its
    /// checks, `trim` normalising; the refinements over the whole input only
    /// when every field is present and passed. Nothing is refused and nothing
    /// is written.
    public static func check(_ sch: Schema, _ c: Closure, _ ctx: Ctx, _ partial: [String: Value], _ store: Store) throws -> Checked {
        let f = c.function
        var messages = [(String, String)]()
        var values = [String: Value]()
        var all = true
        for i in f.input {
            let n = i.name
            guard let v = partial[n] else {
                all = false
                continue
            }
            let snapshot = values
            let refine = { (e: Expr, x: Value) throws -> Bool in
                var local = snapshot
                for (k, pv) in partial where snapshot[k] == nil { local[k] = pv }
                local[n] = x
                let env = Env(sch, c.helpers, .helper, ctx, local, [:], [:], [], nil, false)
                return try pureBool(St(Overlay(MemoryStore.empty(sch))), env, e)
            }
            do {
                let (v2, msg) = try checkField(n, i.value, v, { t, k in store.exists(t, [k]) }, refine)
                if let m = msg {
                    all = false
                    messages.append((n, m))
                }
                values[n] = v2
            } catch EvalFault.verdict(let r) {
                all = false
                messages.append((n, refusalText(r)))
            }
        }
        if all {
            for r in f.refine {
                let env = Env(sch, c.helpers, .helper, ctx, values, [:], [:], [], nil, false)
                do {
                    if try !pureBool(St(Overlay(MemoryStore.empty(sch))), env, r.e) { messages.append(("", r.why ?? "invalid")) }
                } catch EvalFault.verdict(let x) {
                    messages.append(("", refusalText(x)))
                }
            }
        }
        return Checked(messages: messages, values: values)
    }

    /// `refusal_text`: the message of a `refuse`, otherwise the refusal shown.
    /// §3.12 `run_transition`: a transition of a live state by name — a hook
    /// by its reserved name — over `state`: its input held to its declared
    /// types and checked as a procedure's is, its guards in order, its body;
    /// and the new state held to the state's type. A verdict is a refusal
    /// every runtime reaches, in the same words. A hook the state does not
    /// declare leaves the state as it was.
    public static func runTransition(_ c: LiveClosure, _ name: String, _ state: Value, _ ctx: LiveCtx, _ input: [String: Value]) throws -> Value {
        func refused(_ t: String) -> EvalFault { verdict(.refused(t)) }
        guard let t = c.state.transition(name) else {
            if isHook(name) { return state }
            throw refused("\(c.state.name): no transition \(name)")
        }
        // The input, held to its types: a call crosses a boundary.
        for k in input.keys.sorted(by: { compareText($0, $1) < 0 }) where !t.input.contains(where: { $0.name == k }) {
            throw refused("\(k): not an input of \(name)")
        }
        var args = [String: Value]()
        for i in t.input {
            guard let v = input[i.name] else { throw refused("\(i.name): missing") }
            if let why = Types.whyNot(c.types, i.value.ty, v) { throw refused("\(i.name): \(why)") }
            args[i.name] = v
        }
        let schema = Schema([])
        let author = Ctx(ctx.user, ctx.session, roles: ctx.roles)
        let live = LiveEnv(state: state, now: ctx.now, device: ctx.device)
        func env(_ a: [String: Value]) -> Env { Env(schema, c.helpers, .provide, author, a, [:], [:], [], nil, false, live) }
        // The checks, each over its own field as it stands.
        for i in t.input {
            let n = i.name
            let local = args
            let (v2, msg) = try checkField(n, i.value, args[n]!, { _, _ in false }) { e, x in
                var own = local
                own[n] = x
                return try pureBool(St(NoStore(schema)), env(own), e)
            }
            if let m = msg { throw refused(m) }
            args[n] = v2
        }
        let st = St(NoStore(schema))
        let e = env(args)
        for g in t.guards where try !pureBool(st, e, g.e) { throw refused(g.why) }
        guard let out = try block(st, e, t.body)?.v else { throw bug(.noReturn(name)) }
        if let why = Types.whyNot(c.types, c.state.ty, out) {
            throw refused("\(name): the state it makes is not a \(c.state.name): \(why)")
        }
        return out
    }

    /// §3.12 `constant`: a constant expression's value — a live state's init.
    public static func constant(_ e: Expr) throws -> Value {
        try pure(Env(Schema([]), [], .helper, .nobody, [:], [:], [:], [], nil, false), e)
    }

    /// §3.12 `scope_keys`: the keys of the instances `ctx` holds of a live
    /// state, a text or a list of them, in the order the scope gives them,
    /// without repeats.
    public static func scopeKeys(_ c: LiveClosure, _ ctx: Ctx) throws -> [String] {
        let env = Env(Schema([]), c.helpers, .helper, ctx, [:], [:], [:], [], nil, false)
        var out = [String]()
        func add(_ v: Value) throws {
            guard case .text(let t) = v else { throw bug(.typeError("a scope key is text, not \(v.show())")) }
            if !out.contains(where: { textEquals($0, t) }) { out.append(t) }
        }
        let v = try pure(env, c.state.scope)
        if case .list(let xs) = v { for x in xs { try add(x) } } else { try add(v) }
        return out
    }

    public static func refusalText(_ r: Refusal) -> String {
        if case .refused(let w) = r { return w }
        return r.show()
    }

    /// §6.6 `select_plan`: a plan whose right-hand sides are already values, pulled over a store.
    public static func selectPlan(_ sch: Schema, _ plan: Plan, _ store: Store) throws -> [Value] {
        try View.read(sch, plan, Scope.of(sch, [], .nobody, [:], [:]), store)
    }

    // An expression that reads nothing, over a store that holds nothing.
    static func pure(_ env: Env, _ e: Expr) throws -> Value { try eval(St(NoStore(env.schema)), env, e) }

    // §6.3 Statements -------------------------------------------------------

    // A `let` binds for the rest of its block and no further.
    private static func block(_ st: St, _ env0: Env, _ blk: Block) throws -> Returned? {
        var env = env0
        for s in blk {
            if case .letS(let x, let e) = s {
                env = env.under(x, try eval(st, env, e))
                continue
            }
            if let r = try exec(st, env, s) { return r }
        }
        return nil
    }

    private static func exec(_ st: St, _ env: Env, _ s: Stmt) throws -> Returned? {
        switch s {
        case .letS: fatalError("a block binds its lets")
        case .ifS(let c, let a, let b):
            return try block(st, env, try bool(try eval(st, env, c)) ? a : b)
        case .forS(let x, let o, let body):
            for v in try list(try eval(st, env, o)) {
                if let r = try block(st, env.under(x, v), body) { return r }
            }
        case .insert(let t, let row, let on):
            try mutating(env)
            let r = Rules.rowFor(st.store, t, try strct(try eval(st, env, row)))
            try wrote(st, Rules.insert(st.store, t, r, on))
        case .upsert(let t, let row, let on):
            try mutating(env)
            let r = Rules.rowFor(st.store, t, try strct(try eval(st, env, row)))
            try wrote(st, Rules.upsert(st.store, t, r, on))
        case .update(let t, let k, let x, let row):
            try mutating(env)
            let key = try evalMany(st, env, k)
            if let old = st.store.get(t, key) {
                let r = Rules.rowFor(st.store, t, try strct(try eval(st, env.under(x, .record(old)), row)))
                try wrote(st, Rules.update(st.store, t, key, r))
            }
        case .delete(let t, let k):
            try mutating(env)
            try wrote(st, st.store.delete(t, try evalMany(st, env, k)))
        case .refuse(let e):
            if env.kind == .helper { throw bug(.impure("refuse inside a helper")) }
            throw verdict(.refused(try text(try eval(st, env, e))))
        case .ret(let e):
            return Returned(v: try e.map { try eval(st, env, $0) })
        // D3 Deferred, at the authority; nothing anywhere else.
        case .privateS(let body):
            if env.ctx.authority { st.deferred.append((body, captured(env))) }
        }
        return nil
    }

    // The locals in scope, outermost first: what a private block runs over.
    private static func captured(_ env: Env) -> [(Sym, Value)] {
        var frames = [(Sym, Value)]()
        var at = env.locals
        while let f = at {
            frames.append((f.sym, f.value))
            at = f.up
        }
        return env.node + frames.reversed()
    }

    private static func wrote(_ st: St, _ w: Written) throws {
        switch w {
        case .refused(let r): throw verdict(r)
        case .ok(let c): if let c = c { st.changes.append(c) }
        }
    }

    private static func mutating(_ env: Env) throws {
        if env.kind != .mutator { throw bug(.impure("write outside a mutator")) }
    }

    private static func reading(_ env: Env) throws {
        if env.kind == .helper { throw bug(.impure("read inside a helper")) }
    }

    // §6.4 Expressions --------------------------------------------------------

    static func eval(_ st: St, _ env: Env, _ e: Expr) throws -> Value {
        switch e {
        case .lit(let v): return v
        case .arg(let n):
            guard let v = env.args[n] else { throw bug(.missingArg(n)) }
            return v
        case .auto(let n):
            guard let v = env.autos[n] else { throw bug(.missingAuto(n)) }
            return v
        case .variable(let x):
            guard let v = env.local(x) else { throw bug(.unboundVar(x)) }
            return v
        case .provided(let f):
            guard let v = env.provided[f] else { throw bug(.notProvided(f)) }
            return v
        case .field(let x, let name):
            let v = try eval(st, env, x)
            guard case .record(let r) = v else { throw typeError("Struct", v) }
            guard let fv = r[name] else { throw bug(.noSuchField(name)) }
            return fv
        case .get(let t, let k):
            try reading(env)
            return st.store.get(t, try evalMany(st, env, k)).map { .record($0) } ?? .null
        case .ctxUser: return .text(env.ctx.user)
        case .ctxSession: return .text(env.ctx.session)
        case .hasRole(let r): return .bool(env.ctx.hasRole(r))
        case .structE(let fs):
            var vs = [Value]()
            vs.reserveCapacity(fs.count)
            for x in fs.exprs { vs.append(try eval(st, env, x)) }
            return .record(Record(sorted: fs.names, vs))
        case .listE(let xs): return .list(try evalMany(st, env, xs))
        case .some(let x): return try eval(st, env, x)
        case .none: return .null
        case .match(let x, let s, let a, let n):
            let v = try eval(st, env, x)
            if v.isNull { return try eval(st, env, n) }
            return try eval(st, env.under(s, v), a)
        case .ife(let c, let a, let n):
            return try eval(st, env, try bool(try eval(st, env, c)) ? a : n)
        case .op(let o, let args): return try op(st, env, o, args)
        case .cmp(let o, let l, let r):
            let x = try eval(st, env, l)
            let y = try eval(st, env, r)
            return .bool(cmp(o, x, y))
        case .call(let fn, let args):
            let vals = try args.map { try eval(st, env, $0) }
            guard let f = env.helpers.first(where: { $0.name == fn }) else { throw bug(.unknownFunction(fn)) }
            if f.kind != .helper { throw bug(.wrongKind(fn, f.kind)) }
            return try call(st, env, f, vals)
        case .std(let fn, let args):
            let vals = try args.map { try eval(st, env, $0) }
            do {
                return try Std.apply(fn, vals)
            } catch StdError.fault(let why) {
                throw verdict(.refused(why))
            } catch StdError.arity(let f, let n) {
                throw bug(.arity("\(f.rawValue)/\(n)"))
            } catch StdError.typeMismatch(let f) {
                throw bug(.typeError(f.rawValue))
            }
        case .map(let o, let s, let b):
            return .list(try list(try eval(st, env, o)).map { try eval(st, env.under(s, $0), b) })
        case .filter(let o, let s, let b):
            let vs = try list(try eval(st, env, o))
            let keep = try vs.map { try bool(try eval(st, env.under(s, $0), b)) }
            return .list(vs.enumerated().filter { keep[$0.offset] }.map { $0.element })
        // Every element is evaluated; the result is the disjunction (conjunction).
        case .any(let o, let s, let b):
            var acc = false
            for v in try list(try eval(st, env, o)) { acc = try bool(try eval(st, env.under(s, v), b)) || acc }
            return .bool(acc)
        case .all(let o, let s, let b):
            var acc = true
            for v in try list(try eval(st, env, o)) { acc = try bool(try eval(st, env.under(s, v), b)) && acc }
            return .bool(acc)
        // Stable, under the total order of the key.
        case .sortBy(let o, let s, let k):
            let vs = try list(try eval(st, env, o))
            let keys = try vs.map { try eval(st, env.under(s, $0), k) }
            let idx = vs.indices.sorted { a, b in
                let c = compareValue(keys[a], keys[b])
                return c != 0 ? c < 0 : a < b
            }
            return .list(idx.map { vs[$0] })
        case .fold(let o, let z, let acc, let s, let b): return try fold(st, env, o, z, acc, s, b)
        case .select(let p):
            try reading(env)
            return .list(try View.read(env.schema, p, Scope(env), st.store))
        case .liveState:
            guard let l = env.live else { throw bug(.typeError("the state outside a transition")) }
            return l.state
        case .ctxNow:
            guard let l = env.live else { throw bug(.typeError("ctx.now outside a transition")) }
            return .int(l.now)
        case .ctxDevice:
            guard let l = env.live else { throw bug(.typeError("ctx.device outside a transition")) }
            return .text(l.device)
        // §2.2 A sum's value: its fields, and its variant's name under `t`.
        case .variant(_, let n, let fs):
            var vs = [Value]()
            for x in fs.exprs { vs.append(try eval(st, env, x)) }
            var r = Record(sorted: fs.names, vs)
            r.set(Types.tag, .text(n))
            return .record(r)
        // The arm the value's tag names, over the variant's record.
        case .caseOf(let x, let arms):
            var r = try strct(try eval(st, env, x))
            guard case .text(let tag)? = r[Types.tag] else { throw bug(.typeError("a sum's tag, not \(r[Types.tag]?.show() ?? "none")")) }
            guard let arm = arms.first(where: { textEquals($0.name, tag) }) else { throw bug(.typeError("no arm for the variant \(tag)")) }
            r.remove(Types.tag)
            return try eval(st, env.under(arm.sym, .record(r)), arm.e)
        // §2.2 A device's slot, none where it holds nothing.
        case .slot(let m, let k):
            let pd = try eval(st, env, m)
            let key = try text(try eval(st, env, k))
            guard case .record(let r) = pd else { throw typeError("a map of devices", pd) }
            return r[key] ?? .null
        case .setSlot(let m, let k, let v):
            var r = try strct(try eval(st, env, m))
            let key = try text(try eval(st, env, k))
            let x = try eval(st, env, v)
            if x.isNull { r.remove(key) } else { r.set(key, x) }
            return .record(r)
        case .slots(let m):
            let r = try strct(try eval(st, env, m))
            return .list(r.pairs.map { .record(Record([("device", .text($0.0)), ("value", $0.1)])) })
        case .exists(let t, let k):
            try reading(env)
            return .bool(st.store.exists(t, try evalMany(st, env, k)))
        }
    }

    private static func op(_ st: St, _ env: Env, _ o: Op, _ es: [Expr]) throws -> Value {
        switch (o, es.count) {
        case (.and, 2):
            return try bool(try eval(st, env, es[0])) ? try eval(st, env, es[1]) : .bool(false)
        case (.or, 2):
            return try bool(try eval(st, env, es[0])) ? .bool(true) : try eval(st, env, es[1])
        case (.not, 1): return .bool(!(try bool(try eval(st, env, es[0]))))
        case (.neg, 1):
            let n = try int(try eval(st, env, es[0]))
            if n == Int64.min { throw verdict(.refused("integer overflow")) }
            return .int(-n)
        case (.add, 2), (.sub, 2), (.mul, 2), (.div, 2), (.mod, 2):
            let x = try int(try eval(st, env, es[0]))
            let y = try int(try eval(st, env, es[1]))
            switch arith(o, x, y) {
            case .success(let r): return .int(r)
            case .failure(let w): throw verdict(.refused(w.why))
            }
        default: throw bug(.arity(o.debugName))
        }
    }

    // §13 In a plan's node a sum is arithmetic over the integers: init and every
    // term added wide, the total checked once (`docs/plan-db.md` D4).
    private static func fold(_ st: St, _ env: Env, _ o: Expr, _ z: Expr, _ acc: Sym, _ s: Sym, _ body: Expr) throws -> Value {
        let vs = try list(try eval(st, env, o))
        var a = try eval(st, env, z)
        if env.wide, let f = sumStep(body, acc), case .int(let a0) = a {
            var total = Wide(a0)
            for v in vs { total = total + Wide(try int(try eval(st, env.under(s, v), f))) }
            if !total.fits { throw verdict(.refused("integer overflow")) }
            return .int(total.clamped)
        }
        for v in vs { a = try eval(st, env.withFrame(Frame(s, v, Frame(acc, a, env.locals))), body) }
        return a
    }

    /// §13 `sum_step`: the term of a fold whose step is `acc + f` or `f + acc`,
    /// `f` not reading the accumulator — the shape whose result alone is
    /// judged in a plan's node.
    public static func sumStep(_ body: Expr, _ acc: Sym) -> Expr? {
        guard case .op(.add, let args) = body, args.count == 2 else { return nil }
        let l = args[0]
        let r = args[1]
        if l == .variable(acc) && !mentions(r, acc) { return r }
        if r == .variable(acc) && !mentions(l, acc) { return l }
        return nil
    }

    /// Whether `x` occurs in `e` (a plan inside a `Select` is not looked into).
    public static func mentions(_ e: Expr, _ x: Sym) -> Bool {
        if case .variable(let y) = e { return y == x }
        return children(e).contains { mentions($0, x) }
    }

    private static func evalMany(_ st: St, _ env: Env, _ es: [Expr]) throws -> [Value] {
        try es.map { try eval(st, env, $0) }
    }

    // Call a helper: its arguments bound as a fresh environment; helpers never
    // see the caller's locals, autos, arguments or provided values.
    private static func call(_ st: St, _ env: Env, _ f: Function, _ vals: [Value]) throws -> Value {
        if vals.count != f.input.count { throw bug(.arity(f.name)) }
        var args = [String: Value]()
        for (p, v) in zip(f.input, vals) { args[p.name] = v }
        let env2 = Env(env.schema, env.helpers, .helper, env.ctx, args, [:], [:], [], nil, env.wide)
        guard let v = try block(st, env2, f.body)?.v else { throw bug(.noReturn(f.name)) }
        return v
    }

    /// A checked arithmetic fault's text.
    public struct Fault: Error {
        public let why: String
    }

    /// §6.5 `arith`: checked arithmetic — the result, or the fault's text.
    /// Division truncates toward zero and the remainder takes the dividend's
    /// sign; `MIN / -1` and `MIN % -1` overflow.
    public static func arith(_ o: Op, _ x: Int64, _ y: Int64) -> Result<Int64, Fault> {
        let overflow = Result<Int64, Fault>.failure(Fault(why: "integer overflow"))
        switch o {
        case .add:
            let (r, o) = x.addingReportingOverflow(y)
            return o ? overflow : .success(r)
        case .sub:
            let (r, o) = x.subtractingReportingOverflow(y)
            return o ? overflow : .success(r)
        case .mul:
            let (r, o) = x.multipliedReportingOverflow(by: y)
            return o ? overflow : .success(r)
        case .div, .mod:
            if y == 0 { return .failure(Fault(why: "division by zero")) }
            if x == Int64.min && y == -1 { return overflow }
            return .success(o == .div ? x / y : x % y)
        default:
            return .failure(Fault(why: "not an arithmetic operator"))
        }
    }

    // Coercions: a bug when the verifier's type does not hold.

    private static func typeError(_ want: String, _ v: Value) -> EvalFault {
        bug(.typeError("expected \(want), got \(String(v.show().prefix(60)))"))
    }

    private static func bool(_ v: Value) throws -> Bool {
        guard case .bool(let b) = v else { throw typeError("Bool", v) }
        return b
    }

    private static func int(_ v: Value) throws -> Int64 {
        guard case .int(let n) = v else { throw typeError("Int", v) }
        return n
    }

    private static func text(_ v: Value) throws -> String {
        guard case .text(let s) = v else { throw typeError("Text", v) }
        return s
    }

    private static func list(_ v: Value) throws -> [Value] {
        guard case .list(let xs) = v else { throw typeError("List", v) }
        return xs
    }

    private static func strct(_ v: Value) throws -> Row {
        guard case .record(let r) = v else { throw typeError("Struct", v) }
        return r
    }
}
