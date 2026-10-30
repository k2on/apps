// Figures, as Apple's own apps draw them: an amount in the locale's currency
// (`CentsText`), the one figure a screen is about, large on glass the way
// Wallet draws a card's balance (`HeroFigure`), a figure's state in a tinted
// capsule (`Pill`), an amount typed on the keypad the way Apple Cash takes
// one (`AmountEntry`), and a step back and forth through something ordered —
// a month, a week — with its name between (`Pager`). Nothing of any app:
// the amounts are cents (ArkUICore's `Cents`).
#if os(iOS)
import SwiftUI

/// Cents in the locale's currency format: `$1,234.56`.
public struct CentsText: View {
    let cents: Int64
    let code: String
    let signed: Bool

    /// `signed` writes a plus on what is above nothing, as an inflow reads.
    public init(_ cents: Int64, code: String = CentsText.code, signed: Bool = false) {
        self.cents = cents
        self.code = code
        self.signed = signed
    }

    /// The locale's currency, US dollars when it has none.
    public static var code: String { Locale.current.currency?.identifier ?? "USD" }

    public var body: some View {
        Text(CentsText.string(cents, code: code, signed: signed))
            .monospacedDigit()
    }

    /// The text alone, for a label or an accessibility value.
    public static func string(_ cents: Int64, code: String = CentsText.code, signed: Bool = false) -> String {
        let style = Decimal.FormatStyle.Currency(code: code).sign(strategy: signed ? .always() : .automatic)
        return Cents.decimal(cents).formatted(style)
    }
}

/// What a figure says about itself: the colour of its pill and its text.
public enum Tint: Equatable, Sendable {
    case neutral, good, caution, bad, accent

    public var color: Color {
        switch self {
        case .neutral: return .secondary
        case .good: return .green
        case .caution: return .orange
        case .bad: return .red
        case .accent: return .accentColor
        }
    }
}

/// A figure in a capsule of its tint, as Reminders' and Wallet's badges are.
public struct Pill<Content: View>: View {
    let tint: Tint
    let content: Content

    public init(_ tint: Tint, @ViewBuilder content: () -> Content) {
        self.tint = tint
        self.content = content()
    }

    public var body: some View {
        content
            .font(.subheadline.weight(.semibold))
            .monospacedDigit()
            .foregroundStyle(tint == .neutral ? Color.secondary : tint.color)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(tint == .neutral ? Color.secondary.opacity(0.12) : tint.color.opacity(0.16), in: Capsule())
    }
}

extension Pill where Content == CentsText {
    /// An amount in its capsule.
    public init(cents: Int64, tint: Tint) {
        self.init(tint) { CentsText(cents) }
    }
}

@available(iOS 26.0, *)
/// The one figure a screen is about: its caption, the amount large and
/// rounded, a line under it, on glass tinted by what it says — Wallet's
/// balance, Apple Card's. Tapping it is `action`, when there is one.
public struct HeroFigure<Footer: View>: View {
    let caption: String
    let cents: Int64
    let tint: Tint
    let note: String
    let action: (() -> Void)?
    let footer: Footer

    public init(_ caption: String, cents: Int64, tint: Tint, note: String = "", action: (() -> Void)? = nil, @ViewBuilder footer: () -> Footer) {
        self.caption = caption
        self.cents = cents
        self.tint = tint
        self.note = note
        self.action = action
        self.footer = footer()
    }

    public var body: some View {
        let face = VStack(alignment: .leading, spacing: Space.xs) {
            Text(caption)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.secondary)
                .textCase(.uppercase)
            CentsText(cents)
                .font(.system(size: 44, weight: .bold, design: .rounded))
                .foregroundStyle(tint == .neutral ? Color.primary : tint.color)
                .contentTransition(.numericText(value: Double(cents)))
                .minimumScaleFactor(0.5)
                .lineLimit(1)
            if !note.isEmpty {
                Text(note).font(.footnote).foregroundStyle(.secondary)
            }
            footer.padding(.top, Space.s)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Space.xl)
        .background {
            LinearGradient(colors: [tint.color.opacity(0.22), tint.color.opacity(0.04)], startPoint: .topLeading, endPoint: .bottomTrailing)
                .clipShape(RoundedRectangle(cornerRadius: 28, style: .continuous))
        }
        .glassEffect(.regular, in: RoundedRectangle(cornerRadius: 28, style: .continuous))
        .animation(Springs.move, value: cents)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(caption)
        .accessibilityValue(CentsText.string(cents) + (note.isEmpty ? "" : ", " + note))

        if let action {
            Button(action: action) { face.contentShape(Rectangle()) }
                .buttonStyle(.plain)
                .accessibilityAddTraits(.isButton)
        } else {
            face
        }
    }
}

@available(iOS 26.0, *)
extension HeroFigure where Footer == EmptyView {
    public init(_ caption: String, cents: Int64, tint: Tint, note: String = "", action: (() -> Void)? = nil) {
        self.init(caption, cents: cents, tint: tint, note: note, action: action) { EmptyView() }
    }
}

/// An amount typed the way Apple Cash takes one: digits from the right, the
/// figure large in the middle, the system keypad under it. Its value is
/// whole cents, never below nothing; `prompt` is said under it.
public struct AmountEntry: View {
    @Binding var cents: Int64
    let prompt: String
    let tint: Color
    @FocusState private var focused: Bool
    @State private var typed = ""

    public init(cents: Binding<Int64>, prompt: String = "", tint: Color = .primary) {
        _cents = cents
        self.prompt = prompt
        self.tint = tint
    }

    public var body: some View {
        VStack(spacing: Space.s) {
            CentsText(cents)
                .font(.system(size: 56, weight: .bold, design: .rounded))
                .foregroundStyle(cents == 0 ? Color.secondary : tint)
                .contentTransition(.numericText(value: Double(cents)))
                .minimumScaleFactor(0.4)
                .lineLimit(1)
                .animation(Springs.move, value: cents)
            if !prompt.isEmpty {
                Text(prompt).font(.footnote).foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity)
        .contentShape(Rectangle())
        .onTapGesture { focused = true }
        // The field the keypad types into: unseen, its digits read as cents.
        .background {
            TextField("", text: $typed)
                .keyboardType(.numberPad)
                .focused($focused)
                .opacity(0.011)
                .frame(width: 1, height: 1)
                .accessibilityHidden(true)
                .onChange(of: typed) { _, now in
                    let c = AmountEntry.cents(now)
                    if c != cents { cents = c }
                }
        }
        // Set from outside (a suggestion tapped): the keypad goes on from there.
        .onChange(of: cents) { _, now in
            if AmountEntry.cents(typed) != now { typed = now == 0 ? "" : String(now) }
        }
        .onAppear {
            typed = cents == 0 ? "" : String(cents)
            focused = true
        }
        .sensoryFeedback(.selection, trigger: cents)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(prompt.isEmpty ? "Amount" : prompt)
        .accessibilityValue(CentsText.string(cents))
        .accessibilityAdjustableAction { dir in
            switch dir {
            case .increment: cents += 100
            case .decrement: cents = max(0, cents - 100)
            @unknown default: break
            }
            typed = cents == 0 ? "" : String(cents)
        }
    }

    /// The keypad's digits read as cents.
    static func cents(_ typed: String) -> Int64 {
        var c: Int64 = 0
        for d in typed where d.isASCII && d.isNumber { c = Cents.typing(c, digit: Int(String(d)) ?? 0) }
        return c
    }
}

@available(iOS 26.0, *)
/// A step back and forth through something ordered, its name between the
/// chevrons, and a way back to where it started when it is elsewhere — the
/// way Calendar and Fitness page a month.
public struct Pager: View {
    let title: String
    let back: () -> Void
    let forward: () -> Void
    let home: (() -> Void)?
    let homeLabel: String
    @State private var steps = 0

    /// `home`, when given, is offered as `homeLabel` ("Today").
    public init(_ title: String, homeLabel: String = "Today", home: (() -> Void)? = nil, back: @escaping () -> Void, forward: @escaping () -> Void) {
        self.title = title
        self.back = back
        self.forward = forward
        self.home = home
        self.homeLabel = homeLabel
    }

    public var body: some View {
        HStack(spacing: Space.s) {
            Button { steps -= 1; back() } label: { Image(systemName: "chevron.left") }
                .accessibilityLabel("Previous")
            Text(title)
                .font(.headline)
                .contentTransition(.numericText(value: Double(steps)))
                .frame(minWidth: 150)
                .animation(Springs.move, value: title)
            Button { steps += 1; forward() } label: { Image(systemName: "chevron.right") }
                .accessibilityLabel("Next")
            if let home {
                Button(homeLabel, action: home)
                    .font(.subheadline.weight(.semibold))
                    .transition(.opacity.combined(with: .scale))
            }
        }
        .buttonStyle(.glass)
        .sensoryFeedback(.selection, trigger: steps)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(title)
    }
}
#endif
