// A page of an app's first run, any Ark app's: its mark on glass, a title,
// a sentence, and what to do — the server's address, signing in, the
// replica on its way. The app gives the symbol and the colour; harken's
// onboarding draws the same page with its own copy.
#if os(iOS)
import SwiftUI

@available(iOS 26.0, *)
public struct WelcomePage<Content: View>: View {
    let symbol: String
    let tint: Color
    let title: String
    let sentence: String
    let content: Content

    public init(symbol: String, tint: Color, title: String, sentence: String, @ViewBuilder content: () -> Content) {
        self.symbol = symbol
        self.tint = tint
        self.title = title
        self.sentence = sentence
        self.content = content()
    }

    public var body: some View {
        ScrollView {
            VStack(spacing: Space.s) {
                Image(systemName: symbol)
                    .font(.system(size: 44, weight: .semibold))
                    .foregroundStyle(tint)
                    .frame(width: 88, height: 88)
                    .glassEffect(.regular.tint(tint.opacity(0.15)), in: RoundedRectangle(cornerRadius: 24, style: .continuous))
                    .padding(.bottom, Space.l)
                    .accessibilityHidden(true)
                Text(title).font(.largeTitle.bold()).multilineTextAlignment(.center)
                    .accessibilityAddTraits(.isHeader)
                Text(sentence).font(.body).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    .padding(.bottom, Space.xxl)
                content
            }
            .frame(maxWidth: 420)
            .padding(.horizontal, Space.xxl)
            .padding(.vertical, Space.xl)
            .frame(maxWidth: .infinity)
        }
        .scrollBounceBehavior(.basedOnSize)
        .safeAreaPadding(.top, 60)
    }
}

/// A sentence that rises over the bottom of a screen on glass and goes by
/// itself: what an app says once — a refusal, what a sync brought.
@available(iOS 26.0, *)
extension View {
    public func toast(_ text: Binding<String?>, seconds: Double = 2.5) -> some View {
        modifier(Toast(text: text, seconds: seconds))
    }
}

@available(iOS 26.0, *)
private struct Toast: ViewModifier {
    @Binding var text: String?
    let seconds: Double

    func body(content: Content) -> some View {
        content.overlay(alignment: .top) {
            if let t = text {
                Text(t)
                    .font(.subheadline.weight(.medium))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, Space.l)
                    .padding(.vertical, Space.m)
                    .glassEffect(.regular, in: Capsule())
                    .padding(.horizontal, Space.xl)
                    .padding(.top, Space.s)
                    .transition(.move(edge: .top).combined(with: .opacity))
                    .onTapGesture { text = nil }
                    .task(id: t) {
                        try? await Task.sleep(for: .seconds(seconds))
                        if text == t { text = nil }
                    }
                    .accessibilityAddTraits(.updatesFrequently)
            }
        }
        .animation(Springs.move, value: text)
    }
}
#endif
