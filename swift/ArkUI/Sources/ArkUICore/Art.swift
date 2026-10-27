// The derived square's arithmetic: which colour a name is drawn in. The same
// as the desktop's (`arkui::art::hash`) and Android's (`ui/Art.kt`), so one
// name is one square on every device.

/// FNV-1a over a name's UTF-16, and the pick it makes.
public enum Art {
    public static func hash(_ text: String) -> UInt32 {
        var h: UInt32 = 0x811c_9dc5
        for unit in text.utf16 {
            h ^= UInt32(unit)
            h = h &* 0x0100_0193
        }
        return h
    }

    /// Which of `count` colours `seed` draws as.
    public static func pick(_ count: Int, _ seed: String) -> Int { Int(hash(seed) % UInt32(max(count, 1))) }
}

/// The colours the derived square picks from — each its gradient's first
/// stop, as `0xRRGGBB` — for a white page and for a dark one. An app gives
/// its own (SwiftUI's `\.squares`); until it does, ark-ui's neutral greys,
/// Android's `ArkTheme.NeutralLight` and `NeutralDark`.
public struct Squares: Equatable, Sendable {
    public var light: [UInt32]
    public var dark: [UInt32]

    public init(light: [UInt32], dark: [UInt32]) {
        self.light = light
        self.dark = dark
    }

    public static let neutral = Squares(light: [0xE4E4E6, 0xDDE2EA, 0xE6E1DC], dark: [0x5A5A5E, 0x4E5866, 0x605850])

    /// The colours for one side.
    public func of(dark: Bool) -> [UInt32] { dark ? self.dark : light }
}
