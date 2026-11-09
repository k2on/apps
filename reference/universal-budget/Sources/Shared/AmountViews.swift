import SwiftUI

extension EnvironmentValues {
    /// The household's currency. Formatting follows the person's locale
    /// (grouping, decimal mark, symbol position); the currency is a setting.
    @Entry var currencyCode: String = "USD"
}

extension Decimal {
    func formatted(currency code: String, signed: Bool = false) -> String {
        signed
            ? formatted(.currency(code: code).sign(strategy: .always()))
            : formatted(.currency(code: code))
    }
}

/// An amount of money. Always tabular digits, so columns line up and
/// numbers don't jiggle as they change.
struct AmountText: View {
    enum Style {
        /// "−$45.00" / "$1,200.00"
        case plain
        /// "+$1,200.00" in green for money coming in; outflows stay primary.
        /// Colour is never the only signal: the sign is always shown.
        case signed
    }

    var amount: Decimal
    var style: Style = .plain

    @Environment(\.currencyCode) private var currencyCode

    init(_ amount: Decimal, style: Style = .plain) {
        self.amount = amount
        self.style = style
    }

    var body: some View {
        let text = Text(amount.formatted(currency: currencyCode, signed: style == .signed && amount > 0))
            .monospacedDigit()
        if style == .signed && amount > 0 {
            text.foregroundStyle(.green)
        } else {
            text
        }
    }
}

extension CategoryStatus {
    /// Red for overspent, orange for underfunded, green when there is money
    /// to spend, grey when there's nothing.
    var tint: Color {
        switch self {
        case .overspent: .red
        case .underfunded: .orange
        case .funded, .positive: .green
        case .zero: .secondary
        }
    }

    var systemImage: String? {
        switch self {
        case .overspent: "exclamationmark.triangle.fill"
        case .underfunded: "clock.fill"
        default: nil
        }
    }

    func message(currency code: String) -> String? {
        switch self {
        case .overspent(let amount): "Overspent by \(amount.formatted(currency: code))"
        case .underfunded(let amount): "\(amount.formatted(currency: code)) more needed this month"
        case .funded: "Funded"
        case .zero, .positive: nil
        }
    }
}

/// The available balance of a category, as a capsule: the number people
/// look at most in an envelope budget.
struct AvailableBadge: View {
    var amount: Decimal
    var status: CategoryStatus

    @Environment(\.currencyCode) private var currencyCode

    var body: some View {
        HStack(spacing: 4) {
            if let symbol = status.systemImage {
                Image(systemName: symbol)
                    .imageScale(.small)
            }
            AmountText(amount)
        }
        .font(.callout.weight(.semibold))
        .foregroundStyle(foreground)
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(background, in: .capsule)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Available")
        .accessibilityValue([amount.formatted(currency: currencyCode), status.message(currency: currencyCode)]
            .compactMap { $0 }.joined(separator: ", "))
    }

    private var foreground: Color {
        switch status {
        case .overspent: .white
        case .zero: .secondary
        default: status.tint
        }
    }

    private var background: Color {
        switch status {
        case .overspent: .red
        case .zero: .secondary.opacity(0.12)
        default: status.tint.opacity(0.16)
        }
    }
}

/// A slim bar showing progress towards a category's goal.
struct GoalProgressBar: View {
    var progress: Double
    var status: CategoryStatus

    var body: some View {
        ProgressView(value: progress)
            .progressViewStyle(.linear)
            .tint(status.isAttentionNeeded ? status.tint : .green)
            .accessibilityLabel("Goal progress")
            .accessibilityValue(progress.formatted(.percent.precision(.fractionLength(0))))
    }
}
