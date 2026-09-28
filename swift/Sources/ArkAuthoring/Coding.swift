import Foundation
import ArkDB

// Rows, inputs and scopes are ordinary Swift structs whose `Codable`
// conformance the compiler synthesises (`Row`, `Input` and `Scope` refine
// it). These coders are how the vocabulary builds one from a `Repr` and
// takes one apart again, field by field, with no reflection and nothing
// the author writes: a property `userId` is the IR's field `user_id`.

struct CodingFailure: Error { let what: String }

/// What a field of a struct being built is made from.
protocol FieldSource {
    func term(_ t: Term.Type, _ name: FieldName) -> Term
}

/// Every field a projection of one struct-valued `Repr`.
struct Projection: FieldSource {
    let base: Repr
    func term(_ t: Term.Type, _ name: FieldName) -> Term {
        switch base {
        case .e(let e): return t.init(repr: .e(.field(e, name)))
        case .v(.record(let m)): return t.init(repr: .v(m[name] ?? .null))
        case .v: return t.init(repr: .v(zero(t.ty)))
        }
    }
}

/// Every field whatever a function answers for its name.
struct ByName: FieldSource {
    let f: (Term.Type, FieldName) -> Term
    func term(_ t: Term.Type, _ name: FieldName) -> Term { return f(t, name) }
}

struct Building: Decoder {
    let source: FieldSource
    var codingPath: [CodingKey] { return [] }
    var userInfo: [CodingUserInfoKey: Any] { return [:] }
    func container<Key: CodingKey>(keyedBy type: Key.Type) throws -> KeyedDecodingContainer<Key> {
        return KeyedDecodingContainer(BuildingKeyed<Key>(source: source))
    }
    func unkeyedContainer() throws -> UnkeyedDecodingContainer { throw CodingFailure(what: "unkeyed") }
    func singleValueContainer() throws -> SingleValueDecodingContainer { throw CodingFailure(what: "single") }
}

struct BuildingKeyed<K: CodingKey>: KeyedDecodingContainerProtocol {
    typealias Key = K
    let source: FieldSource
    var codingPath: [CodingKey] { return [] }
    var allKeys: [K] { return [] }
    func contains(_ key: K) -> Swift.Bool { return true }
    func decodeNil(forKey key: K) throws -> Swift.Bool { return false }
    func decode<T: Decodable>(_ type: T.Type, forKey key: K) throws -> T {
        guard let tt = type as? Term.Type else { throw CodingFailure(what: "\(key.stringValue) is not a vocabulary type") }
        return source.term(tt, snake(key.stringValue)) as! T
    }
    func decode(_ type: Swift.Bool.Type, forKey key: K) throws -> Swift.Bool { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: String.Type, forKey key: K) throws -> String { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: Double.Type, forKey key: K) throws -> Double { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: Float.Type, forKey key: K) throws -> Float { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: Swift.Int.Type, forKey key: K) throws -> Swift.Int { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: Int8.Type, forKey key: K) throws -> Int8 { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: Int16.Type, forKey key: K) throws -> Int16 { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: Int32.Type, forKey key: K) throws -> Int32 { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: Int64.Type, forKey key: K) throws -> Int64 { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: UInt.Type, forKey key: K) throws -> UInt { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: UInt8.Type, forKey key: K) throws -> UInt8 { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: UInt16.Type, forKey key: K) throws -> UInt16 { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: UInt32.Type, forKey key: K) throws -> UInt32 { throw CodingFailure(what: key.stringValue) }
    func decode(_ type: UInt64.Type, forKey key: K) throws -> UInt64 { throw CodingFailure(what: key.stringValue) }
    func nestedContainer<NK: CodingKey>(keyedBy type: NK.Type, forKey key: K) throws -> KeyedDecodingContainer<NK> { throw CodingFailure(what: "nested") }
    func nestedUnkeyedContainer(forKey key: K) throws -> UnkeyedDecodingContainer { throw CodingFailure(what: "nested") }
    func superDecoder() throws -> Decoder { throw CodingFailure(what: "super") }
    func superDecoder(forKey key: K) throws -> Decoder { throw CodingFailure(what: "super") }
}

/// Build a struct of terms from a field source.
func build<T: Decodable>(_ t: T.Type, _ source: FieldSource) -> T {
    do {
        return try T(from: Building(source: source))
    } catch {
        fatalError("ArkAuthoring: \(T.self) is not made of vocabulary types: \(error)")
    }
}

/// The fields of a struct of terms, by IR name, in declaration order.
final class Taking: Encoder {
    var fields: [(FieldName, Term)] = []
    var codingPath: [CodingKey] { return [] }
    var userInfo: [CodingUserInfoKey: Any] { return [:] }
    func container<Key: CodingKey>(keyedBy type: Key.Type) -> KeyedEncodingContainer<Key> {
        return KeyedEncodingContainer(TakingKeyed<Key>(owner: self))
    }
    func unkeyedContainer() -> UnkeyedEncodingContainer { fatalError("ArkAuthoring: unkeyed") }
    func singleValueContainer() -> SingleValueEncodingContainer { fatalError("ArkAuthoring: single value") }
}

struct TakingKeyed<K: CodingKey>: KeyedEncodingContainerProtocol {
    typealias Key = K
    let owner: Taking
    var codingPath: [CodingKey] { return [] }
    mutating func encode<T: Encodable>(_ value: T, forKey key: K) throws {
        guard let t = value as? Term else { throw CodingFailure(what: "\(key.stringValue) is not a vocabulary type") }
        owner.fields.append((snake(key.stringValue), t))
    }
    mutating func encodeNil(forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: Swift.Bool, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: String, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: Double, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: Float, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: Swift.Int, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: Int8, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: Int16, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: Int32, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: Int64, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: UInt, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: UInt8, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: UInt16, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: UInt32, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func encode(_ value: UInt64, forKey key: K) throws { throw CodingFailure(what: key.stringValue) }
    mutating func nestedContainer<NK: CodingKey>(keyedBy keyType: NK.Type, forKey key: K) -> KeyedEncodingContainer<NK> { fatalError("ArkAuthoring: nested") }
    mutating func nestedUnkeyedContainer(forKey key: K) -> UnkeyedEncodingContainer { fatalError("ArkAuthoring: nested") }
    mutating func superEncoder() -> Encoder { fatalError("ArkAuthoring: super") }
    mutating func superEncoder(forKey key: K) -> Encoder { fatalError("ArkAuthoring: super") }
}

/// Take a struct of terms apart.
func take<T: Encodable>(_ x: T) -> [(FieldName, Term)] {
    let t = Taking()
    do {
        try x.encode(to: t)
    } catch {
        fatalError("ArkAuthoring: \(T.self) is not made of vocabulary types: \(error)")
    }
    return t.fields
}

/// A struct of terms as one `Repr`: under Emit an `EStruct` — or, when
/// every field is a projection of one expression, that expression, which
/// is what a row read and passed on unchanged was; under Native a record.
func structRepr(_ fields: [(FieldName, Term)]) -> Repr {
    guard let mode = Ambient.current else {
        var m: [FieldName: Value] = [:]
        for (name, t) in fields { m[name] = t.value ?? .null }
        return .v(.record(m))
    }
    switch mode {
    case .emit:
        var base: Expr? = nil
        var whole = !fields.isEmpty
        for (name, t) in fields {
            guard case .e(.field(let b, let f)) = t.repr, f == name, base == nil || base == b else { whole = false; break }
            base = b
        }
        if whole, let b = base { return .e(b) }
        var m: [FieldName: Expr] = [:]
        for (name, t) in fields { m[name] = lift(t.repr) }
        return .e(.structOf(m))
    case .native(let n):
        var m: [FieldName: Value] = [:]
        for (name, t) in fields { m[name] = n.value(t.repr) }
        return .v(.record(m))
    }
}
