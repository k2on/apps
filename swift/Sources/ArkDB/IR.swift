import Foundation

/// The version of this specification a module was written against.
public typealias SpecVersion = Int
/// Spec version 2: routers, middleware, input schemas and checks, and the
/// three table writes in place of `put` (spec/AUTHORING.md §1).
public let specVersion: SpecVersion = 2

/// A local variable, alpha-normalised: the `n`th binding in a function.
public typealias Sym = Int
public typealias Block = [Stmt]

public struct Module: Equatable {
    public var spec: SpecVersion
    public var schema: Schema
    /// In declaration order; a helper may be called only by functions after it.
    public var functions: [Function]
    /// §1.1 The routers: each names its scope and the middleware declared
    /// on it, in declaration order.
    public var routers: [Router]
    /// §3.9 The live section: frame types by name.
    public var live: [LiveFrame]
    public init(spec: SpecVersion, schema: Schema, functions: [Function], routers: [Router] = [], live: [LiveFrame]) {
        self.spec = spec; self.schema = schema; self.functions = functions; self.routers = routers; self.live = live
    }

    public func lookupFunction(_ n: String) -> Function? {
        return functions.first { $0.name == n }
    }

    public func lookupRouter(_ n: String) -> Router? {
        return routers.first { $0.name == n }
    }

    /// The middleware a procedure runs, in order: its own `uses`.
    public func middlewareOf(_ fn: Function) -> [Function] {
        return fn.uses.compactMap(lookupFunction)
    }
}

/// §1.1 A router: a name, the scope every procedure on it reads and
/// writes, and the middleware declared on it by function name, in
/// declaration order. A procedure runs its own `uses`, a subsequence.
public struct Router: Equatable {
    public var name: String
    public var scope: ScopeName
    public var uses: [String]
    public init(name: String, scope: ScopeName, uses: [String]) { self.name = name; self.scope = scope; self.uses = uses }
}

public struct LiveFrame: Equatable {
    public var name: String
    public var ty: Ty
    public init(_ name: String, _ ty: Ty) { self.name = name; self.ty = ty }
}

/// §1.2 Mutators and queries are procedures, on a router. A guard runs
/// before the body and may refuse; a provide does the same and returns a
/// value the body reads as `.provided(name)`. Helpers are pure.
public enum FnKind: Equatable {
    case mutator
    case query
    case helper
    case guard_
    case provide

    /// Middleware: a guard or a provide.
    public var isMiddleware: Bool { return self == .guard_ || self == .provide }
    /// A procedure: a mutator or a query, on a router.
    public var isProcedure: Bool { return self == .mutator || self == .query }

    /// The lowercase spelling the wire uses.
    public var wireName: String {
        switch self {
        case .mutator: return "mutator"
        case .query: return "query"
        case .helper: return "helper"
        case .guard_: return "guard"
        case .provide: return "provide"
        }
    }

    public init?(wireName s: String) {
        switch s {
        case "mutator": self = .mutator
        case "query": self = .query
        case "helper": self = .helper
        case "guard": self = .guard_
        case "provide": self = .provide
        default: return nil
        }
    }
}

/// The non-determinism a mutator is allowed, by type.
public enum Auto: Equatable {
    case newId(TableName)
    case now
}

public struct NamedAuto: Equatable {
    public var name: String
    public var auto: Auto
    public init(_ name: String, _ auto: Auto) { self.name = name; self.auto = auto }
}

/// §1.3 A check on one input field. `why` is the message; nil means the
/// default (`Eval.defaultMessage`).
public indirect enum Check: Equatable {
    /// Text: normalise before every later check and before the body.
    case trim
    /// Text: length in code points at least n.
    case minLen(Int, String?)
    case maxLen(Int, String?)
    /// Int: lo <= v <= hi, either bound optional.
    case range(Int?, Int?, String?)
    /// List: at least one element.
    case nonEmpty(String?)
    /// Id: a row with that key exists in the procedure's scope.
    case exists(String?)
    /// Any: the expression, over `.arg(<this field>)`, is true.
    case refine(Expr, String?)
}

/// §1.3 An input field: its type and its checks, in order.
public struct Field: Equatable {
    public var ty: Ty
    public var checks: [Check]
    public init(_ ty: Ty, checks: [Check] = []) { self.ty = ty; self.checks = checks }
}

public struct NamedField: Equatable {
    public var name: String
    public var field: Field
    public init(_ name: String, _ field: Field) { self.name = name; self.field = field }
    public init(_ name: String, _ ty: Ty, checks: [Check] = []) { self.name = name; self.field = Field(ty, checks: checks) }
    public var ty: Ty { return field.ty }
}

/// A check over the whole input, after the fields.
public struct Refine: Equatable {
    public var expr: Expr
    public var why: String?
    public init(_ expr: Expr, _ why: String?) { self.expr = expr; self.why = why }
}

public struct Function: Equatable {
    public var name: String
    public var kind: FnKind
    /// The scope a procedure (derived from its router) or a middleware
    /// belongs to; nil for helpers.
    public var scope: ScopeName?
    /// The router a procedure is on; nil for helpers and middleware.
    public var router: String?
    /// The middleware this procedure runs before its body, in order: a
    /// subsequence of its router's `uses`. Empty for anything else.
    public var uses: [String]
    public var autos: [NamedAuto]
    /// The input: for a procedure its fields and checks; for a middleware
    /// the fields of the procedure's input it reads; for a helper its
    /// parameters.
    public var input: [NamedField]
    /// Checks over the whole input, after the fields.
    public var refine: [Refine]
    /// The result type of a query, helper or provide; nil for a mutator or guard.
    public var ret: Ty?
    public var body: Block
    /// The author's names for symbols; not hashed, not required.
    public var names: [Sym: String]
    public init(name: String, kind: FnKind, scope: ScopeName?, router: String? = nil, uses: [String] = [], autos: [NamedAuto], input: [NamedField], refine: [Refine] = [], ret: Ty?, body: Block, names: [Sym: String] = [:]) {
        self.name = name; self.kind = kind; self.scope = scope; self.router = router; self.uses = uses; self.autos = autos
        self.input = input; self.refine = refine; self.ret = ret; self.body = body; self.names = names
    }

    /// The input's field names, in order: a helper's parameters.
    public var inputNames: [String] { return input.map { $0.name } }
}

/// §3.1 Statements. Named after `Ark.IR`'s constructors, since `let`,
/// `if`, `for` and `return` are keywords.
public indirect enum Stmt: Equatable {
    case sLet(Sym, Expr)
    case sIf(Expr, Block, Block)
    case sFor(Sym, Expr, Block)
    /// §1.4 Write the row unless one matches on the columns (the key when empty).
    case sInsert(TableName, Expr, [FieldName])
    /// §1.4 Write the row; if one matches on the columns, keep its key
    /// columns and take the rest from the new row. `sUpsert t e []` is the
    /// old `put`.
    case sUpsert(TableName, Expr, [FieldName])
    /// §1.4 Key; the existing row bound to the symbol; the new row. A
    /// no-op when absent.
    case sUpdate(TableName, [Expr], Sym, Expr)
    case sDelete(TableName, [Expr])
    case sRefuse(Expr)
    case sReturn(Expr?)
}

/// §3.2 Expressions.
public indirect enum Expr: Equatable {
    case lit(Value)
    case arg(String)
    case auto(String)
    case variable(Sym)
    case ctxUser
    case ctxSession
    /// §1.2 What the provide middleware of that name returned.
    case provided(String)
    case field(Expr, FieldName)
    case structOf([FieldName: Expr])
    case list([Expr])
    case some(Expr)
    case none(Ty)
    /// `match e { Some x -> a; None -> b }`
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
    /// `fold xs init (acc, x -> body)`
    case fold(Expr, Expr, Sym, Sym, Expr)
    case select(Plan)
    case get(TableName, [Expr])
    case exists(TableName, [Expr])
}

public enum Op: String, Equatable {
    case add, sub, mul, div, mod, neg, and, or, not
}

/// Comparison under the total order.
public enum CmpOp: String, Equatable {
    case eq, ne, lt, le, gt, ge
}

public struct OrderBy: Equatable {
    public var column: FieldName
    public var dir: Dir
    public init(_ column: FieldName, _ dir: Dir) { self.column = column; self.dir = dir }
}

/// §3.3 A plan: what `select` pulls and what a view maintains.
public struct Plan: Equatable {
    public var table: TableName
    public var filter: Pred?
    public var order: [OrderBy]
    public var limit: Int?
    public var related: [Related]
    public init(table: TableName, filter: Pred? = nil, order: [OrderBy] = [], limit: Int? = nil, related: [Related] = []) {
        self.table = table; self.filter = filter; self.order = order; self.limit = limit; self.related = related
    }
}

public struct Related: Equatable {
    public var name: FieldName
    public var relation: Relation
    public var plan: Plan
    public init(name: FieldName, relation: Relation, plan: Plan) {
        self.name = name; self.relation = relation; self.plan = plan
    }
}

/// A filter over one row; the right-hand sides may not mention the row.
/// Cases are named as the wire spells them.
public indirect enum Pred: Equatable {
    case pcmp(FieldName, CmpOp, Expr)
    case pin(FieldName, [Expr])
    case pall([Pred])
    case pany([Pred])
    case pnot(Pred)
}

/// §3.4 The standard library, by name. The raw value is the Haskell
/// constructor's spelling, which is how the wire names one.
public enum StdFn: String, CaseIterable, Equatable {
    case trim = "Trim"
    case isEmpty = "IsEmpty"
    case concat = "Concat"
    case lower = "Lower"
    case isAlnum = "IsAlnum"
    case chars = "Chars"
    case textLen = "TextLen"
    case startsWith = "StartsWith"
    case splitOnce = "SplitOnce"
    case textOfInt = "TextOfInt"
    case hex = "Hex"
    case min = "Min"
    case max = "Max"
    case clamp = "Clamp"
    case abs = "Abs"
    case fnv1a64 = "Fnv1a64"
    case sha256 = "Sha256"
    case idOfText = "IdOfText"
    case textOfId = "TextOfId"
    case nilId = "NilId"
    case utf8 = "Utf8"
    case first = "First"
    case last = "Last"
    case len = "Len"
    case contains = "Contains"
    case reverse = "Reverse"
    case isSome = "IsSome"
    case unwrapOr = "UnwrapOr"
    /// AUTHORING.md §6: the option's value, or the refusal "unwrapped none".
    case unwrap = "Unwrap"

    /// How many arguments each function takes.
    public var arity: Int {
        switch self {
        case .startsWith, .splitOnce, .min, .max, .contains, .unwrapOr: return 2
        case .clamp: return 3
        case .nilId: return 0
        default: return 1
        }
    }
}

// MARK: - Builders over the IR (right-hand sides are values)

extension Plan {
    public static func from(_ table: TableName) -> Plan { return Plan(table: table) }

    public func filter(_ p: Pred) -> Plan {
        var q = self; q.filter = p; return q
    }

    public func orderBy(_ column: FieldName, _ dir: Dir) -> Plan {
        var q = self; q.order.append(OrderBy(column, dir)); return q
    }

    public func limit(_ n: Int) -> Plan {
        var q = self; q.limit = n; return q
    }

    public func related(_ name: FieldName, _ parent: TableName, _ child: TableName, _ column: FieldName, _ childPlan: Plan) -> Plan {
        var q = self
        q.related.append(Related(name: name, relation: Relation(parent: parent, child: child, column: column), plan: childPlan))
        return q
    }
}

extension Pred {
    /// Right-hand sides are values, already evaluated.
    public static func cmp(_ column: FieldName, _ op: CmpOp, _ v: Value) -> Pred { return .pcmp(column, op, .lit(v)) }
    public static func inList(_ column: FieldName, _ vs: [Value]) -> Pred { return .pin(column, vs.map { .lit($0) }) }
    public static func all(_ ps: [Pred]) -> Pred { return .pall(ps) }
    public static func any(_ ps: [Pred]) -> Pred { return .pany(ps) }
    public static func not(_ p: Pred) -> Pred { return .pnot(p) }
}
