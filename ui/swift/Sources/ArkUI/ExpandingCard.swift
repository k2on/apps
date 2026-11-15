// Android's `ExpandingCard` (ui/ExpandingCard.kt), as iOS draws it: a bar on
// glass above the tab bar (`tabViewBottomAccessory`) that zooms into a whole
// screen and back (`matchedTransitionSource` / `navigationTransition(.zoom)`).
// The system does the motion, a finger on the screen included — the pull
// down that closes it projects and hands off its speed by itself, and under
// Reduce Motion the zoom is a cross-fade by itself, the system's setting
// being the one it reads — and the bar is on the system's glass, which
// frosts under Reduce Transparency by itself. This is only the wiring, so
// every app's bar and screen are the same pair.
#if os(iOS)
import SwiftUI

@available(iOS 26.0, *)
extension View {
    /// On a `TabView`: `bar` above its tab bar, which opens `expanded` over
    /// the whole screen, zooming out of the bar, while `isPresented` is true;
    /// it zooms back into the bar when `isPresented` goes false or is swiped
    /// down.
    ///
    /// While `isEnabled` is false there is no bar (iOS 26.1's
    /// `tabViewBottomAccessory(isEnabled:)`; before it, the bar is there
    /// and empty, and does not open).
    public func expandingCard<Bar: View, Expanded: View>(
        isPresented: Binding<Bool>,
        isEnabled: Bool = true,
        @ViewBuilder bar: () -> Bar,
        @ViewBuilder expanded: @escaping () -> Expanded
    ) -> some View {
        modifier(ExpandingCard(isPresented: isPresented, isEnabled: isEnabled, bar: bar(), expanded: expanded))
    }
}

@available(iOS 26.0, *)
private struct ExpandingCard<Bar: View, Expanded: View>: ViewModifier {
    @Binding var isPresented: Bool
    let isEnabled: Bool
    let bar: Bar
    let expanded: () -> Expanded
    @Namespace private var zoom

    func body(content: Content) -> some View {
        accessory(content)
            .fullScreenCover(isPresented: $isPresented) {
                expanded().navigationTransition(.zoom(sourceID: "card", in: zoom))
            }
    }

    /// The bar, pressed as one to open it; the buttons on it are their own.
    /// Plain, not `.press`: the system's glass answers the touch, and a give
    /// would shrink the very bar the zoom grows out of.
    private var face: some View {
        Button { isPresented = true } label: { bar.contentShape(Rectangle()) }
            .buttonStyle(.plain)
            .matchedTransitionSource(id: "card", in: zoom)
            .accessibilityHint("Opens the player")
    }

    @ViewBuilder
    private func accessory(_ content: Content) -> some View {
        #if compiler(>=6.2.1)
        if #available(iOS 26.1, *) {
            content.tabViewBottomAccessory(isEnabled: isEnabled) { face }
        } else {
            content.tabViewBottomAccessory { if isEnabled { face } }
        }
        #else
        content.tabViewBottomAccessory { if isEnabled { face } }
        #endif
    }
}
#endif
