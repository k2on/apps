// Each element's walk id (ark_app::shots::id_of: `budget.row:dining-out`,
// `budget.auto-assign`) as its accessibility identifier, so what names an
// element in a screenshot walk names it to anything outside the app too (a
// UI test, the Accessibility Inspector). Written by the generated screens
// (ui/rust/gen), never by hand; an identifier is never read aloud.
import Foundation

public enum ArkId {
    /// Words as an id's part, as ark_app::shots::slug makes them: lower
    /// case, letters and digits, a dash between words, anything else
    /// dropped; a tag's `/` kept.
    public static func slug(_ s: String) -> String {
        var out = ""
        for c in s {
            if c.isLetter || c.isNumber {
                out += c.lowercased()
            } else if c == "/" {
                out += "/"
            } else if !out.isEmpty && !out.hasSuffix("-") && !out.hasSuffix("/") {
                out += "-"
            }
        }
        return out.trimmingCharacters(in: CharacterSet(charactersIn: "-"))
    }
}
