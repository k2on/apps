// A page's header, small, in its navigation bar once the header itself has
// scrolled up out of view: Apple's large title handing over to the inline
// one, for a page whose header is more than a title — an album's cover and
// name, a person's face and name, a show's picture, an account. Its art
// (`InlineArt.side`) and its title on one line, in the bar's principal
// place over the bar's own glass; tapped, the page goes back to its top.
// While the header is in view, the bar is as it always was.
//
// Any app's page uses it the same way: the header says how tall it is
// (`.inlineHeaderFold($fold)`), and the scroll view says what goes in the
// bar (`.inlineHeader(title, fold:, onTop:) { art }`). Whether the header
// has gone is read from the scroll view's geometry as it scrolls, so this
// follows the scrolling rather than playing once: past the fold it comes
// in, back over it it goes, and a turn mid-way springs from wherever it had
// got to. The art and the title are one object and move as one — a rise of
// `MotionTokens.rise` and a fade, transform and opacity only — on
// `Springs.move`, critically damped: nothing threw it, so it does not
// overshoot. It leaves the way it came, sinking back toward the header.
// Under Reduce Motion it only fades (`.motion`'s calm ease), with nothing
// travelling, and the way back to the top is the same ease.
//
// Android's twin is arkui's `PinnedBar` (kotlin/arkui).
import SwiftUI

/// The art beside a page's title in its bar.
public enum InlineArt {
    /// Its side.
    public static let side: CGFloat = 26
    /// The corners of a square cover at `side`; a face is round.
    public static let corner: CGFloat = 4
}

#if os(iOS)
@available(iOS 26.0, *)
public struct InlineHeader<Art: View>: View {
    let title: String
    let shown: Bool
    let onTop: () -> Void
    let art: () -> Art
    @Environment(\.reducesMotion) private var still

    public init(_ title: String, shown: Bool, onTop: @escaping () -> Void, @ViewBuilder art: @escaping () -> Art) {
        self.title = title
        self.shown = shown
        self.onTop = onTop
        self.art = art
    }

    public var body: some View {
        Button(action: onTop) {
            HStack(spacing: Space.s) {
                art().frame(width: InlineArt.side, height: InlineArt.side)
                Text(title).font(.headline).lineLimit(1).foregroundStyle(.ink)
            }
        }
        .buttonStyle(.press)
        .opacity(shown ? 1 : 0)
        .offset(y: shown || still ? 0 : MotionTokens.rise)
        .motion(Springs.move, value: shown)
        .allowsHitTesting(shown)
        .accessibilityHidden(!shown)
        .accessibilityHint("Scrolls to the top")
    }
}

@available(iOS 26.0, *)
extension View {
    /// On a page's header: how tall it is, for `inlineHeader`'s `fold`.
    public func inlineHeaderFold(_ fold: Binding<CGFloat>) -> some View {
        onGeometryChange(for: CGFloat.self) { $0.size.height } action: { fold.wrappedValue = $0 }
    }

    /// On a page's scroll view (a `ScrollView` or a `List`): once it has
    /// scrolled `fold` past its rest — its header gone up under the bar —
    /// `art` beside `title` in the navigation bar's principal place, tapped
    /// to go back to the top (`onTop`, the scroll there, animated here).
    public func inlineHeader<Art: View>(_ title: String, fold: CGFloat, onTop: @escaping () -> Void,
                                        @ViewBuilder art: @escaping () -> Art) -> some View {
        modifier(InlineHeaderBar(title: title, fold: fold, onTop: onTop, art: art))
    }
}

@available(iOS 26.0, *)
private struct InlineHeaderBar<Art: View>: ViewModifier {
    let title: String
    let fold: CGFloat
    let onTop: () -> Void
    let art: () -> Art
    @State private var gone = false
    @Environment(\.reducesMotion) private var still

    func body(content: Content) -> some View {
        content
            .onScrollGeometryChange(for: Bool.self) { g in
                g.contentOffset.y + g.contentInsets.top >= fold
            } action: { _, now in
                gone = now
            }
            .toolbar {
                ToolbarItem(placement: .principal) {
                    InlineHeader(title, shown: gone, onTop: {
                        withAnimation(Springs.move.reduced(still)) { onTop() }
                    }, art: art)
                }
                .sharedBackgroundVisibility(.hidden)
            }
    }
}
#endif
