// The few pieces the screens written from Rust (`ark-ui-gen`) use beside
// ArkUI's parts: a glyph by its shared name that is always one, and a field
// for an amount typed as text, the domain parsing it.
#if os(iOS)
import SwiftUI

extension ArkGlyphs {
    /// The SF Symbol for Lucide's `name`; a question mark for one that is
    /// not shared yet, so a missing name shows rather than crashes.
    public static func sf(_ name: String) -> String {
        symbol(name) ?? "questionmark"
    }
}

/// An amount typed as text: the decimal pad, the figure's font. What it
/// means is the core's to say.
public struct AmountField: View {
    let label: String
    @Binding var text: String

    public init(_ label: String, text: Binding<String>) {
        self.label = label
        _text = text
    }

    public var body: some View {
        TextField(label, text: $text)
            .keyboardType(.decimalPad)
            .monospacedDigit()
    }
}
#endif
