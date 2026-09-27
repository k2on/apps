import Foundation

/// A fault, with two constructors that are never conflated: `refuse` is a
/// verdict a mutator reaches deterministically (an explicit refuse, a
/// constraint, an overflow), `bug` is a bug — a module the verifier would
/// have refused, or a generator that disagrees with `Ark.Eval`.
public enum Fault: Error, Equatable {
    case refuse(String)
    case bug(String)

    /// `throw Fault.refuse(Value.text("…"))`, as generated code spells it.
    public static func refuse(_ v: Value) -> Fault {
        if case .text(let t) = v { return .refuse(t) }
        return .bug("refuse: expected Text, got \(v.brief)")
    }

    public static func bug(_ v: Value) -> Fault { return .bug(v.brief) }

    /// The text either constructor carries.
    public var message: String {
        switch self {
        case .refuse(let t): return t
        case .bug(let t): return t
        }
    }
}
