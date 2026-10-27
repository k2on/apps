// harken's look on the phone, over ArkUI's (swift/ArkUI: the spacing, the
// springs, the derived square): the desktop's palette
// (`harken/iced/src/palette.rs`) — the system's neutral greys, which on iOS
// are the system's own colours, and the gold, one for a dark page and one
// for a white one — and the derived square in harken's colours. Liquid Glass
// brings its own materials: what is here is only what the glass is tinted
// with.
import ArkUI
import SwiftUI
import UIKit

extension Color {
    /// The gold: `controlAccentColor`'s slot. A dark page takes a bright leaf
    /// gold; a white one cannot, because text drawn on it has to be legible.
    static let gold = Color(light: 0x9A741A, dark: 0xE9BB45)
}

// Space, Springs, the derived square and the colour helpers are ArkUI's
// (swift/ArkUI), any app's; what follows is harken's own.

extension Space {
    /// A record's cover beside its title, as a catalogue's page opens.
    static let record: CGFloat = 132
}

extension Springs {
    /// A sideways drag on the mini player let go past this share of its width, or…
    static let wide: CGFloat = 0.25
    /// …faster than this, in points a second, is a skip.
    static let flick: CGFloat = 600
}

extension Squares {
    /// The derived square's colours for harken — the desktop's `DARK_ART` and
    /// `LIGHT_ART`, their first stops — set at the app's root (`\.squares`),
    /// so the phone, the desktop and the Android app draw one album as one square.
    static let harken = Squares(
        light: [0xE8D19A, 0xEFDCAE, 0xE2D2AC, 0xF0DFA8, 0xE6D0A0, 0xF3E6BE],
        dark: [0x7A5C15, 0x8A6B22, 0x6B5A2A, 0x8F7327, 0x5E4A18, 0xA08137]
    )
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
