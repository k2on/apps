// money's look on the phone, over ArkUI's (swift/ArkUI: the spacing, the
// springs, the figures, the colours): arkui's theme (arkui/theme.toml) — the
// neutrals every Ark app shares, as `.ink`, `.quiet` and the rest, and
// money's accent, the green, which `MoneyApp` names (`Accent.use(.green)`)
// and `AccentColor` in the asset catalog holds too. Liquid Glass brings its
// own materials; what is here is only what a figure is tinted with.
import ArkUI
import SwiftUI

extension Tone {
    /// The figure's tint, as ArkUI's pill and hero draw it.
    var tint: Tint {
        switch self {
        case .zero: return .neutral
        case .funded: return .good
        case .short: return .caution
        case .over: return .bad
        case .onCard: return .caution
        }
    }

    /// What the colour means, for VoiceOver.
    var words: String {
        switch self {
        case .zero: return "nothing available"
        case .funded: return "funded"
        case .short: return "underfunded"
        case .over: return "overspent"
        case .onCard: return "overspent on a card"
        }
    }
}

extension Summary {
    /// Ready to Assign's tint: green while there is money to give a job,
    /// red when more was assigned than there is, quiet at nothing.
    var tint: Tint { readyToAssign > 0 ? .good : readyToAssign < 0 ? .bad : .neutral }
}

/// An account's mark: its kind's symbol in its colour on a faint wash of
/// that colour, a rounded square as Settings' tinted rows draw one. The
/// colours are the theme's chart palette (arkui/theme.toml), which the
/// Android app draws its marks in too.
struct AccountMark: View {
    let kind: String
    var side: CGFloat = 32

    var body: some View {
        Tile(symbol: Accounts.symbol(kind), color: AccountMark.color(kind), side: side)
    }

    /// A kind's colour, by its place in the chart palette.
    static func color(_ kind: String) -> Color {
        let c = Color.chart
        switch kind {
        case "checking": return c[1]
        case "savings": return c[0]
        case "cash": return c[5]
        case "credit": return c[3]
        case "loan": return c[2]
        case "investment": return c[4]
        default: return .quiet
        }
    }

    /// A symbol in `color` on a wash of it: an account's mark, or All Accounts'.
    struct Tile: View {
        let symbol: String
        let color: Color
        var side: CGFloat = 32

        var body: some View {
            Image(systemName: symbol)
                .font(.system(size: side * 0.5, weight: .semibold))
                .foregroundStyle(color)
                .frame(width: side, height: side)
                .background(color.opacity(0.16), in: RoundedRectangle(cornerRadius: side * 0.26, style: .continuous))
                .accessibilityHidden(true)
        }
    }
}
