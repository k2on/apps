// Figures, as Apple's own apps draw them: an amount in the locale's currency
// (`CentsText`) and one something else sets, with its mark (`MarkedCents`),
// the one figure a screen is about, large on glass the way
// Wallet draws a card's balance (`HeroFigure`), a figure's state in a tinted
// capsule (`Pill`), an amount typed on the keypad the way Apple Cash takes
// one (`AmountEntry`), and a step back and forth through something ordered —
// a month, a week — with its name between (`Pager`). Nothing of any app:
// the amounts are cents (ArkUICore's `Cents`).
#if canImport(SwiftUI)
import SwiftUI

/// Cents as the locale writes money, `$1,234.56`, by the app's library
/// (`Cents.write`).
public struct CentsText: View {
    let cents: Int64
    let signed: Bool

    /// `signed` writes a plus on what is above nothing, as an inflow reads.
    public init(_ cents: Int64, signed: Bool = false) {
        self.cents = cents
        self.signed = signed
    }

    public var body: some View {
        Text(CentsText.string(cents, signed: signed))
            .monospacedDigit()
    }

    /// The text alone, for a label or an accessibility value.
    public static func string(_ cents: Int64, signed: Bool = false) -> String {
        Cents.write(cents, signed)
    }
}

/// An amount nobody types here — something else sets it — with a small
/// mark before it (an SF Symbol: `repeat`, `function`, `lock`), read aloud
/// as the amount and then `says`.
public struct MarkedCents: View {
    let cents: Int64
    let mark: String
    let says: String

    public init(_ cents: Int64, mark: String, says: String) {
        self.cents = cents
        self.mark = mark
        self.says = says
    }

    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Image(systemName: mark)
                .imageScale(.small)
                .foregroundStyle(.quiet)
            CentsText(cents)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(CentsText.string(cents) + ", " + says)
    }
}

/// What a figure says about itself: the colour of its pill and its text, the
/// theme's signals (ui/theme.toml), each legible on the page. The same
/// five as Android's `Tint` and the desktop's `arkui::parts::Tint`.
public enum Tint: Equatable, Sendable {
    case neutral, positive, caution, negative, accent

    public var color: Color {
        switch self {
        case .neutral: return .quiet
        case .positive: return .positive
        case .caution: return .caution
        case .negative: return .error
        case .accent: return .accentText
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
            .foregroundStyle(tint == .neutral ? Color.quiet : tint.color)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(tint == .neutral ? Color.quiet.opacity(0.12) : tint.color.opacity(0.16), in: Capsule())
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
                .foregroundStyle(.quiet)
            CentsText(cents)
                .scaledFont(44, weight: .bold, design: .rounded)
                .foregroundStyle(tint == .neutral ? Color.ink : tint.color)
                .contentTransition(.numericText(value: Double(cents)))
                .minimumScaleFactor(0.5)
                .lineLimit(1)
            if !note.isEmpty {
                Text(note).font(.footnote).foregroundStyle(.quiet)
            }
            footer.padding(.top, Space.s)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Space.xl)
        .background {
            LinearGradient(colors: [tint.color.opacity(0.22), tint.color.opacity(0.04)], startPoint: .topLeading, endPoint: .bottomTrailing)
                .clipShape(RoundedRectangle(cornerRadius: 28, style: .continuous))
        }
        .surface(in: RoundedRectangle(cornerRadius: 28, style: .continuous))
        .motion(Springs.move, value: cents)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(caption)
        .accessibilityValue(CentsText.string(cents) + (note.isEmpty ? "" : ", " + note))

        if let action {
            Button(action: action) { face.contentShape(Rectangle()) }
                .buttonStyle(.press)
                .accessibilityAddTraits(.isButton)
        } else {
            face
        }
    }
}

@available(iOS 26.0, *)
extension HeroFigure where Footer == EmptyView {
    /// The figure alone, nothing under it and nothing to tap.
    public init(_ caption: String, cents: Int64, tint: Tint, note: String = "") {
        self.init(caption, cents: cents, tint: tint, note: note, action: nil) { EmptyView() }
    }
}

/// An amount typed the way Apple Cash takes one: digits from the right, the
/// figure large in the middle, the system keypad under it. Its value is
/// whole cents, never below nothing; `prompt` is said under it.
public struct AmountEntry: View {
    @Binding var cents: Int64
    let prompt: String
    let tint: Color
    /// Room above and below the figure: a form's row gives it `Space.m`.
    let inset: CGFloat
    @FocusState private var focused: Bool
    @State private var typed = ""

    public init(cents: Binding<Int64>, prompt: String = "", tint: Color = .ink, inset: CGFloat = 0) {
        _cents = cents
        self.prompt = prompt
        self.tint = tint
        self.inset = inset
    }

    public var body: some View {
        VStack(spacing: Space.s) {
            CentsText(cents)
                .scaledFont(56, weight: .bold, design: .rounded)
                .foregroundStyle(cents == 0 ? Color.quiet : tint)
                .contentTransition(.numericText(value: Double(cents)))
                .minimumScaleFactor(0.4)
                .lineLimit(1)
                .motion(Springs.move, value: cents)
            if !prompt.isEmpty {
                Text(prompt).font(.footnote).foregroundStyle(.quiet)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, inset)
        .contentShape(Rectangle())
        .onTapGesture { focused = true }
        // The field the keypad types into: unseen, its digits read as cents.
        .background {
            TextField("", text: $typed)
                .keypad(.number)
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
            // Not in a screenshot (ui/docs/shots.md): the keypad would cover
            // half of what is taken; a shot that types taps the field first.
            if !Shots.on { focused = true }
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
    let backLabel: String
    let forwardLabel: String
    @State private var steps = 0
    /// Which way the last step went: forward brings the next name in from
    /// the right, back from the left; home, whose way is not known here, fades.
    @State private var way: Edge? = nil

    /// `home`, when given, is offered as `homeLabel` ("Today"); VoiceOver
    /// hears the chevrons as `backLabel` and `forwardLabel`.
    public init(_ title: String, homeLabel: String = "Today", home: (() -> Void)? = nil, backLabel: String = "Previous", forwardLabel: String = "Next",
                back: @escaping () -> Void, forward: @escaping () -> Void) {
        self.title = title
        self.back = back
        self.forward = forward
        self.home = home
        self.homeLabel = homeLabel
        self.backLabel = backLabel
        self.forwardLabel = forwardLabel
    }

    public var body: some View {
        HStack(spacing: Space.s) {
            Button { steps -= 1; go(.leading, back) } label: { Image(systemName: "chevron.left") }
                .accessibilityLabel(backLabel)
            ZStack {
                // A new name for each title, so a step slides the old one out
                // and the new one in rather than changing one in place; a
                // step taken mid-slide turns the spring where it is drawn.
                Text(title)
                    .font(.headline)
                    .lineLimit(1)
                    .id(title)
                    .motionTransition(Self.slide(way))
            }
            .frame(minWidth: Self.width)
            .clipped()
            .motion(Springs.move, value: title)
            Button { steps += 1; go(.trailing, forward) } label: { Image(systemName: "chevron.right") }
                .accessibilityLabel(forwardLabel)
            if let home {
                Button(homeLabel) { steps += 1; go(nil, home) }
                    .font(.subheadline.weight(.semibold))
                    .motionTransition(.enter)
            }
        }
        .buttonStyle(.glass)
        // "Today" comes and goes with the step that leaves or reaches home.
        .motion(Springs.size, value: home == nil)
        .sensoryFeedback(.selection, trigger: steps)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(title)
    }

    /// A step `to` that side. The way is set before the step, so the name
    /// that leaves is drawn knowing it as well as the one that comes
    /// (ui/docs/motion.md, Choreography): a view that goes takes the transition
    /// it was last drawn with, so when the way turns, the step waits the one
    /// frame it takes to draw the old name with the new way.
    private func go(_ to: Edge?, _ step: @escaping () -> Void) {
        if way == to { step() } else {
            way = to
            DispatchQueue.main.async(execute: step)
        }
    }

    /// The least room the name has between the chevrons.
    static let width: CGFloat = 150

    /// The name coming in from `way`'s side and leaving by the other, as
    /// Android's Stepper (`Transitions.stepped`): fading as it slides the
    /// shared shift (`MotionTokens.shift`) of its width, enough to say which
    /// way it went. With no way, a cross-fade.
    static func slide(_ way: Edge?) -> AnyTransition {
        guard let way else { return .opacity }
        let by = CGFloat(MotionTokens.shift) * width * (way == .trailing ? 1 : -1)
        return .asymmetric(insertion: .offset(x: by).combined(with: .opacity),
                           removal: .offset(x: -by).combined(with: .opacity))
    }
}
#endif
