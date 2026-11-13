// The app's appearance over the device's: Settings' Light or Dark, a
// launch's `-appearance`, or a screenshot walk's theme (ui/docs/shots.md).
//
// On the iPhone and the iPad it is every window's own style, of every
// scene, and of every window made after (an alert's, a menu's, a
// confirmation dialog's), not SwiftUI's `preferredColorScheme`: iOS 26's
// Liquid Glass bars kept the theme they were made in when the style
// changed under them while the app ran (app-screens 43318181211: the dark
// iPhone's tab bar light on `account` and `insights`, each taken just
// after a switch). So each window's bars and their glass are drawn again
// when it changes: each bar's appearance handed back to it, each
// material's effect set afresh, and the window laid out.
//
// On the Mac, SwiftUI's own `preferredColorScheme`.
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

extension View {
    /// The app drawn `dark` (true), light (false), or as the device is (nil).
    public func arkAppearance(_ dark: Bool?) -> some View {
        modifier(AppearanceRoot(dark: dark))
    }
}

private struct AppearanceRoot: ViewModifier {
    let dark: Bool?

    @ViewBuilder func body(content: Content) -> some View {
        #if canImport(UIKit)
        content
            .onAppear { ArkAppearance.app(dark: dark) }
            .onChange(of: dark) { _, now in ArkAppearance.app(dark: now) }
        #else
        switch dark {
        case true?: content.preferredColorScheme(.dark)
        case false?: content.preferredColorScheme(.light)
        case nil: content
        }
        #endif
    }
}

#if canImport(UIKit)
/// What every window of the app is drawn in: the app's own choice, under a
/// screenshot walk's while one is taking it.
@MainActor
public enum ArkAppearance {
    /// The app's: dark, light, or the device's (nil).
    private static var chosen: Bool? = nil
    /// A screenshot walk's theme, over the app's while it is set.
    private static var forced: Bool? = nil
    private static var watching: NSObjectProtocol?

    /// The app's own choice (Settings, a launch's `-appearance`).
    public static func app(dark: Bool?) {
        chosen = dark
        apply()
    }

    /// A screenshot walk's theme, whatever the app's (none: the app's).
    public static func force(dark: Bool?) {
        forced = dark
        apply()
    }

    /// Whether the windows are asked to be dark now (nil: the device's).
    public static var dark: Bool? { forced ?? chosen }

    private static var style: UIUserInterfaceStyle {
        dark.map { $0 ? .dark : .light } ?? .unspecified
    }

    /// Every window of every scene in the style; and every window shown
    /// after, as it is.
    private static func apply() {
        if watching == nil {
            // Seen on the main queue: every window styled again (the one
            // just shown among them).
            watching = NotificationCenter.default.addObserver(forName: UIWindow.didBecomeVisibleNotification, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { apply() }
            }
        }
        for scene in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }) {
            for w in scene.windows { restyle(w) }
        }
    }

    private static func restyle(_ w: UIWindow) {
        let want = style
        guard w.overrideUserInterfaceStyle != want else { return }
        w.overrideUserInterfaceStyle = want
        redraw(w)
        w.setNeedsLayout()
        w.layoutIfNeeded()
    }

    /// What draws a material or a bar under `v`, told to draw again.
    private static func redraw(_ v: UIView) {
        if let e = v as? UIVisualEffectView {
            let effect = e.effect
            e.effect = nil
            e.effect = effect
        }
        if let t = v as? UITabBar {
            t.standardAppearance = t.standardAppearance
            if let a = t.scrollEdgeAppearance { t.scrollEdgeAppearance = a }
        }
        if let n = v as? UINavigationBar {
            n.standardAppearance = n.standardAppearance
            if let a = n.scrollEdgeAppearance { n.scrollEdgeAppearance = a }
            if let a = n.compactAppearance { n.compactAppearance = a }
        }
        if let t = v as? UIToolbar {
            t.standardAppearance = t.standardAppearance
            if let a = t.scrollEdgeAppearance { t.scrollEdgeAppearance = a }
        }
        if v is UITabBar || v is UINavigationBar || v is UIToolbar {
            v.setNeedsLayout()
            v.setNeedsDisplay()
        }
        for s in v.subviews { redraw(s) }
    }
}
#endif
