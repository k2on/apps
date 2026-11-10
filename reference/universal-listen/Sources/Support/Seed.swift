import Foundation

/// Deterministic numbers from strings (FNV-1a), so every launch has the same library.
enum Seed {
    static func value(_ s: String) -> UInt64 {
        var h: UInt64 = 0xcbf2_9ce4_8422_2325
        for b in s.utf8 {
            h ^= UInt64(b)
            h = h &* 0x0000_0100_0000_01b3
        }
        return h
    }

    static func unit(_ s: String) -> Double { Double(value(s) % 10_000) / 10_000 }

    static func int(_ s: String, _ range: ClosedRange<Int>) -> Int {
        range.lowerBound + Int(value(s) % UInt64(range.count))
    }
}

/// The launch arguments the screenshots use (SCREENS.md). Each is a
/// `-key value` pair, which UserDefaults reads in its argument domain.
enum Launch {
    static let defaults = UserDefaults.standard
    static func string(_ key: String) -> String? { defaults.string(forKey: key) }

    static var screen: String? { string("screen") }
    static var inspector: String? { string("inspector") }
    static var playerExpanded: Bool { string("player") == "expanded" }
    static var panel: String? { string("panel") }
    static var playing: String? { string("playing") }
    static var popover: String? { string("popover") }
    static var dialog: String? { string("dialog") }
    static var sheet: String? { string("sheet") }
    static var searchActive: Bool { defaults.bool(forKey: "searchActive") }
    static var settingsTab: String? { string("settingsTab") }
    /// Screenshots want a still picture: no clock moves the playhead.
    static var frozen: Bool { screen != nil || defaults.bool(forKey: "frozen") }

    static var windowSize: CGSize {
        let parts = (string("window") ?? "1280x820").split(separator: "x").compactMap { Double($0) }
        return parts.count == 2 ? CGSize(width: parts[0], height: parts[1]) : CGSize(width: 1280, height: 820)
    }

    /// "Now" is fixed: Tuesday 10 November 2026, 9:41.
    static let now: Date = {
        var c = DateComponents()
        c.year = 2026; c.month = 11; c.day = 10; c.hour = 9; c.minute = 41
        return Calendar(identifier: .gregorian).date(from: c) ?? Date(timeIntervalSince1970: 1_794_300_000)
    }()

    static func daysAgo(_ n: Int) -> Date { now.addingTimeInterval(-Double(n) * 86_400) }
}
