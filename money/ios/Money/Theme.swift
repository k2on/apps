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

/// An account's mark: its kind's symbol on a tinted rounded square, as Wallet's passes and Settings' rows draw one.
struct AccountMark: View {
    let kind: String
    var side: CGFloat = 32

    var body: some View {
        Image(systemName: Accounts.symbol(kind))
            .font(.system(size: side * 0.5, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: side, height: side)
            .background(AccountMark.color(kind).gradient, in: RoundedRectangle(cornerRadius: side * 0.26, style: .continuous))
            .accessibilityHidden(true)
    }

    static func color(_ kind: String) -> Color {
        switch kind {
        case "checking": return .blue
        case "savings": return .green
        case "cash": return .mint
        case "credit": return .indigo
        case "loan": return .orange
        case "investment": return .purple
        default: return .gray
        }
    }
}
