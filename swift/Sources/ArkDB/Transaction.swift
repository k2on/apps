import Foundation

/// The store as generated code sees `db` in a mutator: a transaction over a
/// base store that records what it changed, so a generated body runs as one
/// unit — the base is untouched until the caller takes `commit()`, and a
/// verdict drops the overlay and reports nothing, exactly as
/// `Eval.applyClosure` rolls back. The Swift counterpart of the Rust
/// runtime's `ark::gen::Db` / `run_mutator`.
///
/// The working copy is a `MemoryStore` clone, which is copy-on-write, so a
/// transaction that refuses costs nothing beyond what it read.
public final class TransactionStore: Store {
    public let schema: Schema
    /// The store the transaction started from; never written.
    public let base: MemoryStore
    let work: MemoryStore
    /// What has been changed so far, in order.
    public private(set) var changes: [Change] = []
    /// The last refusal the store gave, so that the structured verdict is
    /// recoverable from the `Fault.refuse` a generated body propagates.
    public private(set) var lastRefusal: Refusal?

    public init(base: MemoryStore) {
        self.schema = base.schema
        self.base = base
        self.work = base.clone()
    }

    public func get(_ table: TableName, _ key: [Value]) -> Value { return work.get(table, key) }
    public func exists(_ table: TableName, _ key: [Value]) -> Value { return work.exists(table, key) }
    public func select(_ plan: Plan) -> Value { return work.select(plan) }
    public func scan(_ table: TableName) -> [Row] { return work.scan(table) }

    public func put(_ table: TableName, _ row: Value) throws {
        guard case .record(let r) = row else { throw Fault.bug("put: expected Struct, got \(row.brief)") }
        switch work.tryPut(table, r) {
        case .failure(let why):
            lastRefusal = why
            throw why.fault
        case .success(let ch):
            if let c = ch { changes.append(c) }
        }
    }

    public func delete(_ table: TableName, _ key: [Value]) throws {
        switch work.tryDelete(table, key) {
        case .failure(let why):
            lastRefusal = why
            throw why.fault
        case .success(let ch):
            if let c = ch { changes.append(c) }
        }
    }

    /// A fact applied inside a transaction is recorded like any other change.
    public func applyChange(_ ch: Change) {
        work.applyChange(ch)
        changes.append(ch)
    }

    /// The verdict a fault a generated body threw stands for: the store's
    /// own structured refusal when the fault is the one it raised, otherwise
    /// `refused(text)`; a `Fault.bug` is the bug, as a fault.
    public func verdict(_ f: Fault) -> Result<Refusal, Fault> {
        switch f {
        case .refuse(let text):
            if let r = lastRefusal, r.text == text { return .success(r) }
            return .success(.refused(text))
        case .bug:
            return .failure(f)
        }
    }

    /// The store after the transaction: the working copy. The base is as
    /// it was, so a caller that does not take this has committed nothing.
    public func commit() -> MemoryStore { return work }
}

extension Eval {
    /// Run a native body — generated code — as one transaction over a store:
    /// the shape `applyClosure` has for a body that is code rather than IR.
    /// The store passed is never written; the outcome carries a new one.
    /// A `Fault.refuse` is the verdict, a `Fault.bug` is rethrown, and any
    /// other error is a bug too.
    public static func applyBody(_ st: MemoryStore, _ body: (Store) throws -> Void) throws -> Outcome {
        let tx = TransactionStore(base: st)
        do {
            try body(tx)
            return .applied(tx.commit(), tx.changes)
        } catch let f as Fault {
            switch tx.verdict(f) {
            case .success(let r): return .refused(r)
            case .failure(let bug): throw bug
            }
        } catch {
            throw Fault.bug("\(error)")
        }
    }
}
