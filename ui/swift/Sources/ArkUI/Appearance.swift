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
// sidebar's and the detail's columns, each `UITab`'s) is brought into the
// style at once, its bar drawn again, so that nothing switches later.
//
// A bar's style is given it, the app's, and to each view in it, not left
// to come down from the window: on the iPad, after a switch while the app
// ran, money's navigation bars kept the old theme whole (their large
// titles white on the light page, the glass of their items dark) while the
// navigation controller's view they are in had the new one, as did its
// parent, and nothing gave the bar a style of its own (app-screens
// 43972335223, and 43982815086's trace above each bar); drawing it again,
// its traits asked for again and the bar put down and up did not move it,
// only a column's change did (a section's inspector put up). Given to the
// bar alone, the bar took it and the glass of its items, under it, still
// did not (43988749425), nor the iPhone's tab bar's selection under its
// platter. The walk says, for a bar still in the other style, each view
// above it, whose it is and any style given it.
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
        // now, and its navigation bar given the style and drawn again.
        for vc in controllers(under: w.rootViewController) {
            vc.updateTraitsIfNeeded()
            if vc.isViewLoaded, vc.view.window == nil {
                vc.view.updateTraitsIfNeeded()
                redraw(vc.view)
            }
            if let n = vc as? UINavigationController {
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
        // A bar, and what it holds, does not take its style from the view
        // it is in (the file's head): each view of it is given the app's.
        if v is UITabBar || v is UINavigationBar || v is UIToolbar {
            give(v)
            v.setNeedsLayout()
            v.setNeedsDisplay()
        }
        for s in v.subviews { redraw(s) }
    }

    /// `v` and every view in it given the app's style.
    private static func give(_ v: UIView) {
        if v.overrideUserInterfaceStyle != style { v.overrideUserInterfaceStyle = style }
        for s in v.subviews { give(s) }
        v.updateTraitsIfNeeded()
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

    /// What in a bar on the screen is not in the style asked for (none
    /// asked: nothing): each view whose traits say the other style or that
    /// has a style of its own, and each label whose colour, as its traits
    /// resolve it, is the other theme's. A walk logs it beside a picture.
    public static func stale() -> String? {
        guard let dark = Self.dark else { return nil }
        let want: UIUserInterfaceStyle = dark ? .dark : .light
        var out: [String] = []
        func walk(_ v: UIView, _ bar: String) {
            if v.isHidden || v.alpha == 0 { return }
            var why: [String] = []
            if v.traitCollection.userInterfaceStyle != want { why.append("traits \(name(v.traitCollection.userInterfaceStyle))") }
            if v.overrideUserInterfaceStyle != .unspecified, v.overrideUserInterfaceStyle != want { why.append("own \(name(v.overrideUserInterfaceStyle))") }
            if v.traitOverrides.contains(UITraitUserInterfaceStyle.self), v.traitOverrides.userInterfaceStyle != want { why.append("trait override \(name(v.traitOverrides.userInterfaceStyle))") }
            if let l = v as? UILabel, let c = l.textColor {
                var white: CGFloat = 0
                c.resolvedColor(with: l.traitCollection).getWhite(&white, alpha: nil)
                if (white > 0.5) != dark { why.append("label \"\(l.text ?? "")\" \(String(format: "%.2f", white))") }
            }
            if !why.isEmpty, out.count < 24 { out.append("\(bar) \(type(of: v)): \(why.joined(separator: ", "))") }
            for s in v.subviews { walk(s, bar) }
        }
        // Where a bar's style comes from, the first time one is the other's:
        // each view above it up to the first in the style asked, and each
        // controller whose view that is, with any style given it.
        var traced = false
        func trace(_ bar: UIView) {
            traced = true
            var v: UIView? = bar.superview
            while let x = v {
                var line = "  above: \(type(of: x)) traits \(name(x.traitCollection.userInterfaceStyle))"
                if let own = ownStyle(x) { line += ", own \(own)" }
                if let vc = x.next as? UIViewController {
                    line += ", the view of \(type(of: vc))"
                    if vc.overrideUserInterfaceStyle != .unspecified { line += " own \(name(vc.overrideUserInterfaceStyle))" }
                    if vc.traitOverrides.contains(UITraitUserInterfaceStyle.self) { line += " trait override \(name(vc.traitOverrides.userInterfaceStyle))" }
                    if let p = vc.parent {
                        line += " under \(type(of: p))"
                        if let given = p.overrideTraitCollection(forChild: vc), given.userInterfaceStyle != .unspecified { line += " given \(name(given.userInterfaceStyle))" }
                    } else {
                        line += " with no parent"
                    }
                }
                out.append(line)
                if x.traitCollection.userInterfaceStyle == want { break }
                v = x.superview
            }
        }
        func bars(_ v: UIView) {
            if v.isHidden { return }
            if v is UITabBar || v is UINavigationBar || v is UIToolbar {
                let before = out.count
                walk(v, "\(type(of: v))")
                if out.count > before, !traced, v.traitCollection.userInterfaceStyle != want { trace(v) }
                return
            }
            for s in v.subviews { bars(s) }
        }
        for scene in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }) {
            for w in scene.windows { bars(w) }
        }
        return out.isEmpty ? nil : out.joined(separator: "\n")
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
