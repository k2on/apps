// A page's header, small, in its navigation bar once the header itself has
// scrolled up out of view: Apple's large title handing over to the inline
// one, for a page whose header is more than a title — an album's cover and
// name, a person's face and name, a show's picture. Its art, 26 points, and
// its title on one line, in the bar's principal place over the bar's own
// glass; tapped, the page goes back to its top. While the header is in
// view, the bar is as it always was.
//
// The page says whether the header has gone, from where its scroll view is
// (`shown`), so this follows the scrolling rather than playing once: past
// the fold it comes in, back over it it goes, and a turn mid-way springs
// from wherever it had got to. The art and the title are one object and
// move as one — a rise of a few points and a fade, transform and opacity
// only — on `Springs.move`, critically damped: nothing threw it, so it does
// not overshoot. It leaves the way it came, sinking back toward the header.
// Under Reduce Motion it only fades (`.motion`'s calm ease), with nothing
// travelling.
//
// Android's twin is arkui's `PinnedBar` (kotlin/arkui).
import SwiftUI

#if os(iOS)
@available(iOS 26.0, *)
public struct InlineHeader<Art: View>: View {
    /// The art's side in the bar.
    public static var side: CGFloat { 26 }
    /// How far it rises as it comes in: a nudge, not a journey.
    public static var rise: CGFloat { 8 }

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
            HStack(spacing: 8) {
                art().frame(width: Self.side, height: Self.side)
                Text(title).font(.headline).lineLimit(1)
            }
        }
        .buttonStyle(.plain)
        .opacity(shown ? 1 : 0)
        .offset(y: shown || still ? 0 : Self.rise)
        .motion(Springs.move, value: shown)
        .allowsHitTesting(shown)
        .accessibilityHidden(!shown)
        .accessibilityHint("Scrolls to the top")
    }
}

@available(iOS 26.0, *)
extension View {
    /// The page's header, small, in the navigation bar's principal place
    /// while `shown` (the header has scrolled up out of view): `art` beside
    /// `title`, tapped to go back to the top (`onTop`).
    public func inlineHeader<Art: View>(_ title: String, shown: Bool, onTop: @escaping () -> Void,
                                        @ViewBuilder art: @escaping () -> Art) -> some View {
        toolbar {
            ToolbarItem(placement: .principal) {
                InlineHeader(title, shown: shown, onTop: onTop, art: art)
            }
            .sharedBackgroundVisibility(.hidden)
        }
    }
}
#endif
