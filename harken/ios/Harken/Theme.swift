// harken's look on the phone, over ArkUI's (swift/ArkUI: the spacing, the
// springs, the derived square, the colours): arkui's theme
// (arkui/theme.toml) — the neutrals every Ark app shares, as `.ink`, `.quiet`
// and the rest, and harken's accent, the gold, which `HarkenApp` names
// (`Accent.use(.gold)`) — and the derived square in the gold. Liquid Glass
// brings its own materials: what is here is only what the glass is tinted
// with.
import ArkUI
import SwiftUI
import UIKit

// Space, Springs, the derived square and the colour helpers are ArkUI's
// (swift/ArkUI), any app's; what follows is harken's own.

extension Space {
    /// A record's cover beside its title, as a catalogue's page opens.
    static let record: CGFloat = 132
}

extension Springs {
    /// A sideways drag on the mini player let go past this share of its width, or…
    static let wide: CGFloat = 0.25
    /// …faster than this, in points a second, is a skip. (A speed past which
    /// a thing counts as thrown at all, for its landing, is ArkUI's
    /// `Springs.thrown`.)
    static let flick: CGFloat = 600
}

extension Squares {
    /// The derived square's colours for harken — the gold's, the same as the
    /// desktop's and Android's — set at the app's root (`\.squares`), so every
    /// device draws one album as one square.
    static let harken = AccentSet.gold.squares
}

/// The SF Symbol for a device's kind, as the picker and the device chip draw it.
func symbol(forDevice kind: String, name: String) -> String {
    switch kind {
    case "phone": return "iphone"
    case "speaker": return name.lowercased().contains("tv") ? "tv" : "hifispeaker"
    case "browser": return "globe"
    case "car": return "car"
    default: return "laptopcomputer"
    }
}
