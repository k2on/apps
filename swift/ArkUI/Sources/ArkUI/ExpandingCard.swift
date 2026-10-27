// Android's `ExpandingCard` (ui/ExpandingCard.kt), as iOS draws it: a bar on
// glass above the tab bar (`tabViewBottomAccessory`) that zooms into a whole
// screen and back (`matchedTransitionSource` / `navigationTransition(.zoom)`).
// The system does the motion, a finger on the screen included; this is only
// the wiring, so every app's bar and screen are the same pair.
#if os(iOS)
import SwiftUI

@available(iOS 26.0, *)
extension View {
    /// On a `TabView`: `bar` above its tab bar, which opens `expanded` over
    /// the whole screen, zooming out of the bar, while `isPresented` is true;
    /// it zooms back into the bar when `isPresented` goes false or is swiped
    /// down.
    public func expandingCard<Bar: View, Expanded: View>(
        isPresented: Binding<Bool>,
        @ViewBuilder bar: () -> Bar,
        @ViewBuilder expanded: @escaping () -> Expanded
    ) -> some View {
        modifier(ExpandingCard(isPresented: isPresented, bar: bar(), expanded: expanded))
    }
}

@available(iOS 26.0, *)
private struct ExpandingCard<Bar: View, Expanded: View>: ViewModifier {
    @Binding var isPresented: Bool
    let bar: Bar
    let expanded: () -> Expanded
    @Namespace private var zoom

    func body(content: Content) -> some View {
        content
            .tabViewBottomAccessory {
                Button { isPresented = true } label: { bar.contentShape(Rectangle()) }
                    .buttonStyle(.plain)
                    .matchedTransitionSource(id: "card", in: zoom)
            }
            .fullScreenCover(isPresented: $isPresented) {
                expanded().navigationTransition(.zoom(sourceID: "card", in: zoom))
            }
    }
}
#endif
