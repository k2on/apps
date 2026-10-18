// §3 Ark IR, as `rust/ark/src/ir/mod.rs` defines it: the program a domain
// is. A module carries a schema, routers, functions and the types of its
// live frames. A function is a procedure on a router (a mutator or a
// query), a middleware (a guard, a provide or a scope) or a helper, with a
// body in a small imperative core over a pure expression language. Spec
// version 4: a query is a `Plan` and nothing else.

/// The version of the specification this runtime implements (`ir::SPEC_VERSION`).
public let SPEC_VERSION: Int64 = 4

/// A local variable, alpha-normalised: the `n`th binding in a function (`ir::Sym`).
public typealias Sym = Int64

/// A block of statements.
public typealias Block = [Stmt]

/// `ir::Module`.
public struct Module: Hashable {
    public var spec: Int64
    public var schema: Schema
    /// In declaration order; a helper may be called only by functions after it.
    public var functions: [Function]
    /// §1.1 The routers: groups of procedures, each naming the middleware it may use.
    public var routers: [Router]
    /// §3.9 The live section: the frame types an app's realtime channel carries, by name.
    public var live: [LiveFrame]
    /// §2.2 The type section (`docs/plan-live.md`): the records and sums a
    /// live state is made of, each by its name, which `Ty.named` refers to.
    /// On the wire `types`, written only when not empty, so a module without
    /// one is the bytes it was.
    public var types: [Named<Ty>]
    /// §3.12 The live states: a typed state machine per scope. `states`,
    /// written only when there is one.
    public var states: [LiveStateDecl]

    public init(spec: Int64, schema: Schema, functions: [Function], routers: [Router], live: [LiveFrame], types: [Named<Ty>] = [], states: [LiveStateDecl] = []) {
        self.spec = spec
        self.schema = schema
        self.functions = functions
        self.routers = routers
        self.live = live
        self.types = types
        self.states = states
    }

    /// `Module::lookup_function`.
    public func lookupFunction(_ name: String) -> Function? { functions.first { $0.name == name } }

    /// `Module::lookup_router`.
    public func lookupRouter(_ name: String) -> Router? { routers.first { $0.name == name } }

    /// `Module::lookup_state`.
    public func lookupState(_ name: String) -> LiveStateDecl? { states.first { $0.name == name } }

    /// `Module::type_map`: the type section as a map, which is how a named type is resolved.
    public func typeMap() -> [String: Ty] {
        var out = [String: Ty]()
        for t in types where out[t.name] == nil { out[t.name] = t.value }
        return out
    }
}

/// §3.12 A live state (`ir::State`, `docs/plan-live.md`): one current value
/// per instance, an instance per scope key, held by the server and never
/// replayed. Its value is of `ty`; it starts as `initial`; `scope` names the
/// instances a person is subscribed to, as a function of who they are; and
/// it moves only by its transitions. The three hooks are transitions the
/// engine runs itself: a device arriving, a device leaving, a snapshot
/// restored at start.
public struct LiveStateDecl: Hashable {
    public var name: String
    public var ty: Ty
    /// A constant expression of `ty`.
    public var initial: Expr
    /// A `Text`, or a `List` of them, of the context alone.
    public var scope: Expr
    public var transitions: [Transition]
    public var onJoin: Transition?
    public var onLeave: Transition?
    public var onRestore: Transition?

    public init(name: String, ty: Ty, initial: Expr, scope: Expr, transitions: [Transition], onJoin: Transition?, onLeave: Transition?, onRestore: Transition?) {
        self.name = name
        self.ty = ty
        self.initial = initial
        self.scope = scope
        self.transitions = transitions
        self.onJoin = onJoin
        self.onLeave = onLeave
        self.onRestore = onRestore
    }

    /// `State::transition`: a transition by name, a hook by its reserved name.
    public func transition(_ name: String) -> Transition? {
        switch name {
        case onJoinName: return onJoin
        case onLeaveName: return onLeave
        case onRestoreName: return onRestore
        default: return transitions.first { $0.name == name }
        }
    }

    /// Every transition and hook: declared ones first, then join, leave, restore.
    public var allTransitions: [Transition] { transitions + [onJoin, onLeave, onRestore].compactMap { $0 } }
}

/// The reserved names of the three hooks, which no declared transition may have.
public let onJoinName = "on_join"
public let onLeaveName = "on_leave"
public let onRestoreName = "on_restore"

/// Whether a name is one of the three hooks'.
public func isHook(_ name: String) -> Bool { name == onJoinName || name == onLeaveName || name == onRestoreName }

/// §3.12 One way a live state moves (`ir::Transition`): its input with the
/// checks a procedure's input has; its guards, each a `Bool` refusing with
/// its message when false; and a body that returns the new state or refuses.
/// It reads no table and draws no auto.
public struct Transition: Hashable {
    public var name: String
    public var input: [Named<Field>]
    public var guards: [Guard]
    public var body: Block

    public init(name: String, input: [Named<Field>], guards: [Guard], body: Block) {
        self.name = name
        self.input = input
        self.guards = guards
        self.body = body
    }
}

/// One guard of a transition: a `Bool`, and the message it refuses with.
public struct Guard: Hashable {
    public let e: Expr
    public let why: String
    public init(e: Expr, why: String) {
        self.e = e
        self.why = why
    }
}

/// §3.12 A hook's `device` (`ir::device_ty`): one device standing in an
/// instance as it declared itself at `Hello`, and the user the server
/// verified it as.
public func deviceTy() -> Ty {
    .structOf([("can_play", .bool), ("id", .text), ("kind", .text), ("name", .text), ("user", .text)])
}

/// One frame type of the live section.
public struct LiveFrame: Hashable {
    public let name: String
    public let ty: Ty
}

/// §1.1 A router: a group of procedures sharing middleware, its `uses` in order.
public struct Router: Hashable {
    public let name: String
    public let uses: [String]
}

/// `ir::FnKind`, spelled on the wire in lowercase.
public enum FnKind: String, Hashable {
    case mutator
    case query
    case helper
    case guardFn = "guard"
    case provide
    /// `docs/plan-guards.md` D2 Middleware that runs nothing: what a person holds.
    case scope

    /// A mutator or a query.
    public var isProcedure: Bool { self == .mutator || self == .query }

    /// A guard, a provide or a scope.
    public var isMiddleware: Bool { self == .guardFn || self == .provide || self == .scope }

    /// As the reference's `{:?}` names it.
    public var debugName: String {
        switch self {
        case .mutator: return "Mutator"
        case .query: return "Query"
        case .helper: return "Helper"
        case .guardFn: return "Guard"
        case .provide: return "Provide"
        case .scope: return "Scope"
        }
    }
}

/// The non-determinism a mutator is allowed, by type; drawn once and frozen in the entry.
public enum Auto: Hashable {
    case newId(String)
    case now
}

/// A name and what it is, in declaration order.
public struct Named<T: Hashable>: Hashable {
    public let name: String
    public let value: T
    public init(_ name: String, _ value: T) {
        self.name = name
        self.value = value
    }
}

/// `ir::Function`. A decoded function carries no author's names; nothing here needs them.
public struct Function: Hashable {
    public var name: String
    public var kind: FnKind
    public var router: String?
    public var uses: [String]
    public var autos: [Named<Auto>]
    public var input: [Named<Field>]
    public var refine: [Refinement]
    public var ret: Ty?
    public var body: Block
    /// §1.4 A query's whole meaning; `nil` for every other kind.
    public var plan: Plan?
    /// `docs/plan-guards.md` D2 A scope's holds; empty for every other kind.
    public var holds: [Hold]
    /// `docs/plan-guards.md` D3 A mutator with a server half.
    public var isPrivate: Bool

    /// `Function::arg_types`.
    public var argTypes: [(String, Ty)] { input.map { ($0.name, $0.value.ty) } }
}

/// A refinement over a procedure's whole input: an expression and its message.
public struct Refinement: Hashable {
    public let e: Expr
    public let why: String?
}

/// `docs/plan-guards.md` D2 What a scope holds of one table.
public struct Hold: Hashable {
    public let table: String
    public let filter: Pred?
    public let columns: Projection
}

/// Which columns of a held row a scope keeps (`ir::Projection`).
public enum Projection: Hashable {
    case all
    case pick([String])
    case exclude([String])

    /// `Projection::keeps`.
    public func keeps(_ column: String) -> Bool {
        switch self {
        case .all: return true
        case .pick(let cs): return cs.contains(column)
        case .exclude(let cs): return !cs.contains(column)
        }
    }
}

/// §1.3 One field of a procedure's input.
public struct Field: Hashable {
    public let ty: Ty
    public let checks: [Check]
}

/// §1.3 A check on one field; `why` is the message, `nil` for the default
/// (`eval::default_message`).
public enum Check: Hashable, CustomStringConvertible {
    case trim
    case minLen(Int64, String?)
    case maxLen(Int64, String?)
    case range(Int64?, Int64?, String?)
    case nonEmpty(String?)
    case exists(String?)
    case refine(Expr, String?)

    /// The message the author gave, if any (`eval::message`).
    public var given: String? {
        switch self {
        case .trim: return nil
        case .minLen(_, let w), .maxLen(_, let w), .range(_, _, let w), .nonEmpty(let w), .exists(let w), .refine(_, let w):
            return w
        }
    }

    public var description: String {
        switch self {
        case .trim: return "Trim"
        case .minLen(let n, let w): return "MinLen(\(n), \(String(describing: w)))"
        case .maxLen(let n, let w): return "MaxLen(\(n), \(String(describing: w)))"
        case .range(let lo, let hi, let w): return "Range(\(String(describing: lo)), \(String(describing: hi)), \(String(describing: w)))"
        case .nonEmpty(let w): return "NonEmpty(\(String(describing: w)))"
        case .exists(let w): return "Exists(\(String(describing: w)))"
        case .refine(_, let w): return "Refine(\(String(describing: w)))"
        }
    }
}

/// §3.1 Statements (`ir::Stmt`).
public indirect enum Stmt: Hashable {
    case letS(Sym, Expr)
    case ifS(Expr, Block, Block)
    case forS(Sym, Expr, Block)
    case insert(String, Expr, [String])
    case upsert(String, Expr, [String])
    case update(String, [Expr], Sym, Expr)
    case delete(String, [Expr])
    case refuse(Expr)
    case ret(Expr?)
    /// `docs/plan-guards.md` D3 A mutator's server half, run only by the authority, after the body.
    case privateS(Block)
}

/// `ir::strip`: every private block taken out, wherever it is nested, `private` kept.
public func strip(_ f: Function) -> Function {
    if !f.isPrivate { return f }
    func block(_ b: Block) -> Block {
        b.compactMap { s -> Stmt? in
            switch s {
            case .privateS: return nil
            case .ifS(let c, let a, let e): return .ifS(c, block(a), block(e))
            case .forS(let x, let o, let body): return .forS(x, o, block(body))
            default: return s
            }
        }
    }
    var g = f
    g.body = block(f.body)
    return g
}

/// `ir::strip_module`: the module a client loads.
public func stripModule(_ m: Module) -> Module {
    var n = m
    n.functions = m.functions.map(strip)
    return n
}

/// `ir::has_private`: whether a block holds a private block, at any depth.
public func hasPrivate(_ b: Block) -> Bool {
    b.contains { s in
        switch s {
        case .privateS: return true
        case .ifS(_, let a, let e): return hasPrivate(a) || hasPrivate(e)
        case .forS(_, _, let body): return hasPrivate(body)
        default: return false
        }
    }
}

/// A struct expression's fields, in code point order of their names, which
/// is the order they are evaluated in.
public struct ExprFields: Hashable {
    public private(set) var names: [String]
    public private(set) var exprs: [Expr]

    public init(_ fields: [(String, Expr)]) {
        let sorted = fields.enumerated().sorted { a, b in
            let c = compareText(a.element.0, b.element.0)
            return c != 0 ? c < 0 : a.offset < b.offset
        }
        var ns = [String]()
        var es = [Expr]()
        for (_, f) in sorted {
            if let l = ns.last, textEquals(l, f.0) {
                es[es.count - 1] = f.1
            } else {
                ns.append(f.0)
                es.append(f.1)
            }
        }
        names = ns
        exprs = es
    }

    init(sorted names: [String], _ exprs: [Expr]) {
        self.names = names
        self.exprs = exprs
    }

    public var count: Int { names.count }

    public func map(_ f: (Expr) throws -> Expr) rethrows -> ExprFields {
        ExprFields(sorted: names, try exprs.map(f))
    }
}

/// §3.2 Expressions (`ir::Expr`).
public indirect enum Expr: Hashable {
    case lit(Value)
    case arg(String)
    case auto(String)
    case variable(Sym)
    case ctxUser
    case ctxSession
    /// `docs/plan-guards.md` D1 Whether the author holds the role.
    case hasRole(String)
    case provided(String)
    case field(Expr, String)
    case structE(ExprFields)
    case listE([Expr])
    case some(Expr)
    case none(Ty)
    case match(Expr, Sym, Expr, Expr)
    case ife(Expr, Expr, Expr)
    case op(Op, [Expr])
    case cmp(CmpOp, Expr, Expr)
    case call(String, [Expr])
    case std(StdFn, [Expr])
    case map(Expr, Sym, Expr)
    case filter(Expr, Sym, Expr)
    case any(Expr, Sym, Expr)
    case all(Expr, Sym, Expr)
    case sortBy(Expr, Sym, Expr)
    case fold(Expr, Expr, Sym, Sym, Expr)
    case select(Plan)
    case get(String, [Expr])
    case exists(String, [Expr])
    /// §2.2 A value of a sum: the variant of the sum `ty`, with these fields;
    /// its value is the struct of the fields with `t` the variant's name.
    /// `{"t":"variant","ty","name","fields"}`.
    case variant(Ty, String, ExprFields)
    /// §2.2 A sum taken apart: one arm per variant, in the sum's variant
    /// order, each binding the variant's record to its symbol.
    /// `{"t":"case","e","arms":[{"t":"arm","name","sym","e"}]}`.
    case caseOf(Expr, [Arm])
    /// §3.12 The state a transition runs over. `{"t":"live_state"}`.
    case liveState
    /// §3.12 The time a transition runs at, in ms since the epoch. `{"t":"ctx_now"}`.
    case ctxNow
    /// §3.12 The id of the device calling. `{"t":"ctx_device"}`.
    case ctxDevice
    /// §2.2 One device's slot of a `PerDevice`, an option. `{"t":"slot","e","key"}`.
    case slot(Expr, Expr)
    /// §2.2 A `PerDevice` with one slot set, or emptied by none. `{"t":"set_slot","e","key","v"}`.
    case setSlot(Expr, Expr, Expr)
    /// §2.2 A `PerDevice`'s filled slots as `{device, value}` records in id order. `{"t":"slots","e"}`.
    case slots(Expr)
}

/// One arm of a `case`: the variant it takes, the symbol its record is bound to, and its body.
public struct Arm: Hashable {
    public var name: String
    public var sym: Sym
    public var e: Expr
    public init(name: String, sym: Sym, e: Expr) {
        self.name = name
        self.sym = sym
        self.e = e
    }
}

/// Arithmetic and boolean operators; integer arithmetic is checked (`ir::Op`).
public enum Op: String, Hashable {
    case add, sub, mul, div, mod, neg, and, or, not

    /// As the reference's `{:?}` names it.
    public var debugName: String {
        switch self {
        case .add: return "Add"
        case .sub: return "Sub"
        case .mul: return "Mul"
        case .div: return "Div"
        case .mod: return "Mod"
        case .neg: return "Neg"
        case .and: return "And"
        case .or: return "Or"
        case .not: return "Not"
        }
    }
}

/// Comparison under the total order (`ir::CmpOp`).
public enum CmpOp: String, Hashable {
    case eq, ne, lt, le, gt, ge
}

/// §1.3 A plan: a tree of reads, what a query is, what `select` pulls and
/// what a view maintains (`ir::Plan`). Each node is evaluated in this order:
/// the source rows the filter admits; `row` (and `members`) bound; the
/// lookups in order; the related plans; `having`; `project`; the order keys.
public struct Plan: Hashable {
    public var source: Source
    public var filter: Pred?
    public var row: Sym?
    public var members: Sym?
    public var lookups: [Lookup]
    public var related: [Related]
    public var having: Expr?
    public var project: Expr?
    public var order: [OrderKey]
    public var limit: Int64?

    public init(source: Source, filter: Pred?, row: Sym?, members: Sym?, lookups: [Lookup], related: [Related], having: Expr?, project: Expr?, order: [OrderKey], limit: Int64?) {
        self.source = source
        self.filter = filter
        self.row = row
        self.members = members
        self.lookups = lookups
        self.related = related
        self.having = having
        self.project = project
        self.order = order
        self.limit = limit
    }

    /// The table the plan reads.
    public var table: String { source.table }

    /// `Plan::is_v3_shaped`: the shape a mutator's read may have.
    public var isV3Shaped: Bool {
        guard case .table = source, members == nil, lookups.isEmpty, having == nil, project == nil else { return false }
        for o in order {
            if case .expr = o.key { return false }
        }
        for r in related {
            guard r.on.count == 1, let row = row, case .field(let e, _) = r.on[0].e, e == .variable(row), r.plan.isV3Shaped else { return false }
        }
        return true
    }

    /// `Plan::is_bare`: v3-shaped with nothing beneath.
    public var isBare: Bool { isV3Shaped && related.isEmpty }

    /// `Plan::from`: every row of a table, in key order.
    public static func from(_ table: String) -> Plan {
        Plan(source: .table(table), filter: nil, row: nil, members: nil, lookups: [], related: [], having: nil, project: nil, order: [], limit: nil)
    }
}

/// Where a plan's nodes come from.
public enum Source: Hashable {
    case table(String)
    case group(String, [String])

    public var table: String {
        switch self {
        case .table(let t), .group(let t, _): return t
        }
    }
}

/// A row by key from another table, bound to `sym` as an option.
public struct Lookup: Hashable {
    public var name: String
    public var sym: Sym
    public var table: String
    public var key: [Expr]
    public init(name: String, sym: Sym, table: String, key: [Expr]) {
        self.name = name
        self.sym = sym
        self.table = table
        self.key = key
    }
}

/// One `child.column = expr` of a related plan.
public struct On: Hashable {
    public let column: String
    public let e: Expr
    public init(column: String, e: Expr) {
        self.column = column
        self.e = e
    }
}

/// A child plan beneath each node, bound to `sym` as the list of its nodes.
public struct Related: Hashable {
    public var name: String
    public var sym: Sym
    public var on: [On]
    public var plan: Plan
    public init(name: String, sym: Sym, on: [On], plan: Plan) {
        self.name = name
        self.sym = sym
        self.on = on
        self.plan = plan
    }
}

/// One key of an order: a column of the source row, or an expression over the node's binders.
public enum Key: Hashable {
    case column(String)
    case expr(Expr)
}

/// A key and its direction.
public struct OrderKey: Hashable {
    public var key: Key
    public var dir: Dir
    public init(_ key: Key, _ dir: Dir) {
        self.key = key
        self.dir = dir
    }
}

/// A filter over one row; the right-hand sides may not mention the row (`ir::Pred`).
public indirect enum Pred: Hashable {
    case cmp(String, CmpOp, Expr)
    case inP(String, [Expr])
    case all([Pred])
    case any([Pred])
    case not(Pred)
    /// `docs/plan-db.md` D4 The text column holds the needle, both sides folded.
    case has(String, Expr)
    /// `docs/plan-guards.md` D2 A scope's leaf: a `Bool` that reads no row.
    case when(Expr)
    /// `docs/plan-guards.md` D2 A scope's one reach into another table.
    case exists(String, String, Pred)
}

/// §3.4 The standard library, by name, spelled as the constructors' `show` (`ir::StdFn`).
public enum StdFn: String, Hashable, CaseIterable {
    case Trim, IsEmpty, Concat, Lower, IsAlnum, Chars, TextLen, StartsWith, SplitOnce, TextOfInt, Hex, Min, Max, Clamp, Abs, Fnv1a64, Sha256, IdOfText, TextOfId, NilId, Utf8, First, Last, Len, Contains, Reverse
    /// §5.1 The list editors: `ListConcat(a, b)`, `Insert(xs, i, x)`,
    /// `RemoveAt(xs, i)`, `Move(xs, from, to)`, `Slice(xs, start, end)`.
    case ListConcat, Insert, RemoveAt, Move, Slice
    case IsSome, UnwrapOr, Unwrap

    public var arity: Int {
        switch self {
        case .NilId: return 0
        case .StartsWith, .SplitOnce, .Min, .Max, .Contains, .UnwrapOr, .ListConcat, .RemoveAt: return 2
        case .Clamp, .Insert, .Move, .Slice: return 3
        default: return 1
        }
    }
}
