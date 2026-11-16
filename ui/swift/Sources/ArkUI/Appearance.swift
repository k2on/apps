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
// And not only what is on the screen: every view controller of the window
// (each tab's navigation stack, each page under the top of a stack, the
// sidebar's and the detail's columns) is brought into the style at once,
// its bar drawn again, so that nothing switches later, when it comes on
// the screen. A tab or a page that was off the screen at the switch took
// the new style only as it appeared, and its navigation bar laid its large
// title out in the middle of that: drawn over the page's first rows
// (harken's Library on the iPhone and the iPad, app-screens 43796348626)
// or in the old theme's colour, white on the light page under a toolbar
// drawn dark (money's account, budget and review on the iPad on its side).
// Each tab is its own `UITab`'s controller, off the tab bar controller's
// children while another is shown: money's budget and account kept the
// dark ones (app-screens 43926273208), review, the one shown, did not.
// The traits are applied (`updateTraitsIfNeeded`) before the bars are
// drawn again, so a bar's colours are resolved in the new style, not the
// old.
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
        w.updateTraitsIfNeeded()
        // Every controller's, off the screen too (above): its traits taken
        // now, and its navigation bar drawn again in them.
        for vc in controllers(under: w.rootViewController) {
            vc.updateTraitsIfNeeded()
            if vc.isViewLoaded, vc.view.window == nil {
                vc.view.updateTraitsIfNeeded()
                redraw(vc.view)
            }
            if let n = vc as? UINavigationController {
                n.navigationBar.updateTraitsIfNeeded()
                redraw(n.navigationBar)
                n.navigationBar.layoutIfNeeded()
            }
        }
        redraw(w)
        w.setNeedsLayout()
        w.layoutIfNeeded()
    }

    /// `root` and every controller under it, each once: its children, each
    /// tab's (shown or not), each column's, each page of a stack, and what
    /// is presented over it.
    private static func controllers(under root: UIViewController?) -> [UIViewController] {
        var out: [UIViewController] = []
        var seen = Set<ObjectIdentifier>()
        func walk(_ vc: UIViewController?) {
            guard let vc, seen.insert(ObjectIdentifier(vc)).inserted else { return }
            out.append(vc)
            for c in vc.children { walk(c) }
            for c in (vc as? UITabBarController)?.viewControllers ?? [] { walk(c) }
            // A tab bar of `UITab`s (iOS 18's, SwiftUI's `sidebarAdaptable`
            // tabs among them) lists only the shown tab's controller among
            // its children and none in `viewControllers`: each tab's, and
            // each of a group's (money's accounts under Accounts), is its
            // `UITab`'s.
            if #available(iOS 18.0, *), let t = vc as? UITabBarController {
                func tabs(_ ts: [UITab]) {
                    for tab in ts {
                        walk(tab.viewController)
                        if let g = tab as? UITabGroup { tabs(g.children) }
                    }
                }
                tabs(t.tabs)
            }
            for c in (vc as? UISplitViewController)?.viewControllers ?? [] { walk(c) }
            for c in (vc as? UINavigationController)?.viewControllers ?? [] { walk(c) }
            walk(vc.presentedViewController)
        }
        walk(root)
        return out
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

    /// The style `v` is given of its own, by either of UIKit's ways (its
    /// override, its trait overrides), if any.
    private static func ownStyle(_ v: UIView) -> String? {
        if v.overrideUserInterfaceStyle != .unspecified { return name(v.overrideUserInterfaceStyle) }
        if v.traitOverrides.contains(UITraitUserInterfaceStyle.self) { return "\(name(v.traitOverrides.userInterfaceStyle)) (trait override)" }
        return nil
    }

    private static func name(_ s: UIUserInterfaceStyle) -> String {
        switch s {
        case .dark: "dark"
        case .light: "light"
        default: "unspecified"
        }
    }

    /// Each bar of every window, as the system has it now: its kind, the
    /// style its traits say, and each view in it with a style of its own
    /// or a material: what a walk says when a bar is drawn in the other
    /// theme.
    public static func describeBars() -> String {
        var out: [String] = []
        func walk(_ v: UIView, _ depth: Int, _ inBar: Bool) {
            let bar = v is UITabBar || v is UINavigationBar || v is UIToolbar
            if bar || inBar {
                let own = ownStyle(v).map { " own \($0)" } ?? ""
                let effect = (v as? UIVisualEffectView).map { " effect \(String(describing: $0.effect.map { type(of: $0) }))" } ?? ""
                if bar || !own.isEmpty || !effect.isEmpty || depth < 3 {
                    out.append("\(String(repeating: " ", count: depth))\(type(of: v)) traits \(name(v.traitCollection.userInterfaceStyle))\(own)\(effect)\(v.isHidden ? " hidden" : "")")
                }
            }
            for s in v.subviews { walk(s, bar || inBar ? depth + 1 : 0, bar || inBar) }
        }
        for scene in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }) {
            for w in scene.windows {
                out.append("\(type(of: w)) own \(name(w.overrideUserInterfaceStyle)) traits \(name(w.traitCollection.userInterfaceStyle))")
                walk(w, 0, false)
            }
        }
        return out.joined(separator: "\n")
    }
}
#endif
