// money's look on the phone, over ArkUI's (ui/swift: the spacing, the
// springs, the figures, the colours): arkui's theme (ui/theme.toml) — the
// neutrals every Ark app shares, as `.ink`, `.quiet` and the rest, and
// money's accent, the green, which `MoneyApp` names (`Accent.use(.green)`)
// and `AccentColor` in the asset catalog holds too. Liquid Glass brings its
// own materials; what is here is only what a figure is tinted with.
import ArkUI
import SwiftUI

extension Tone {
    /// The core's tone in ArkUI's tint, as its pill and hero draw it:
    /// money's one map, Android's and the desktop's the same.
    var tint: Tint {
        switch self {
        case .neutral: return .neutral
        case .positive: return .positive
        case .short, .onCard: return .caution
        case .overspent: return .negative
        }
    }
}

/// The SF Symbol an account of a kind is drawn with.
enum Accounts {
    static func symbol(_ kind: String) -> String {
        switch kind {
        case "checking": return "building.columns.fill"
        case "savings": return "dollarsign.circle.fill"
        case "cash": return "banknote.fill"
        case "credit": return "creditcard.fill"
        case "loan": return "house.fill"
        case "investment": return "chart.line.uptrend.xyaxis"
        default: return "building.columns.fill"
        }
    }
}

/// Now, as the domain counts time: milliseconds since 1970.
func nowMs(_ date: Date = Date()) -> Int64 { Int64((date.timeIntervalSince1970 * 1000).rounded()) }

extension Summary {
    /// Ready to Assign's tint: green while there is money to give a job,
    /// red when more was assigned than there is, quiet at nothing.
    var tint: Tint { tone().tint }
}

/// An account's mark: its kind's symbol in its colour on a faint wash of
/// that colour, a rounded square as Settings' tinted rows draw one (ArkUI's
/// `Tile`). The
/// colours are the theme's chart palette (ui/theme.toml), which the
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
}
