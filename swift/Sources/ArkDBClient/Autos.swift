// The non-determinism a mutator is allowed, drawn once at the originating
// peer and frozen in the entry, as `rust/ark-client/src/autos.rs` draws it:
// from the platform, or from a seed — a test's or a fixture's — by the same
// generator the reference uses, so a seeded Swift peer draws exactly the ids
// and times a seeded Rust peer draws.
import ArkDB
import Foundation

/// Where a peer's autos come from (`autos::Autos`).
public final class Autos {
    private enum Source {
        case system
        case seeded(state: UInt64, now: Int64)
    }

    private var source: Source
    private var rng = SystemRandomNumberGenerator()

    private init(_ s: Source) { source = s }

    /// The platform's randomness and clock.
    public static func system() -> Autos { Autos(.system) }

    /// `Autos::seeded`: a PCG-style multiplier over a 64-bit state, the clock
    /// starting at 1,700,000,000,000 and moving one millisecond per draw.
    public static func seeded(_ seed: Int64) -> Autos {
        Autos(.seeded(state: UInt64(bitPattern: seed) ^ 0x9e37_79b9_7f4a_7c15, now: 1_700_000_000_000))
    }

    private func fill(_ b: inout [UInt8]) {
        switch source {
        case .system:
            for i in 0..<b.count { b[i] = UInt8.random(in: 0...255, using: &rng) }
        case .seeded(var state, let now):
            var at = 0
            while at < b.count {
                state = state &* 6364136223846793005 &+ 1442695040888963407
                let x = state ^ (state >> 33)
                for i in 0..<min(8, b.count - at) { b[at + i] = UInt8(truncatingIfNeeded: x >> (8 * UInt64(i))) }
                at += 8
            }
            source = .seeded(state: state, now: now)
        }
    }

    /// The time, in milliseconds.
    public func nowMs() -> Int64 {
        switch source {
        case .system:
            return Int64((Date().timeIntervalSince1970 * 1000).rounded(.down))
        case .seeded(let state, let now):
            source = .seeded(state: state, now: now + 1)
            return now + 1
        }
    }

    /// `new_id`: a fresh id, laid out as a version 4 UUID.
    public func newId() -> Id {
        var b = [UInt8](repeating: 0, count: 16)
        fill(&b)
        b[6] = (b[6] & 0x0f) | 0x40
        b[8] = (b[8] & 0x3f) | 0x80
        return Id(bytes: b)!
    }

    /// `draw`: the autos a mutator declares, in its order.
    public func draw(_ f: Function) -> [String: Value] {
        var out = [String: Value]()
        for a in f.autos {
            switch a.value {
            case .newId: out[a.name] = .id(newId())
            case .now: out[a.name] = .int(nowMs())
            }
        }
        return out
    }
}
