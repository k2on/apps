// A screenshot's root (ui/docs/shots.md), for `Shots.on` (`-shots YES`):
//
// - **Motion is at its end at once.** Every shared spring and transition
//   (`Springs`, `.motion`, `.motionTransition`, `.press`) is
//   `Springs.instant`; under the root no transaction animates; UIKit's own
//   animations (a push, a sheet, the keyboard) are off. A screen is never
//   caught on its way.
// - **The ready signal is drawn** (ArkUICore's `Ready`): an accessibility
//   element, `ark.ready`, labelled `ready <generation> <route>`, there
//   only while the screen is ready. It is ticked once a frame by a display
//   link while it waits, told what of the screen still moves (an animation
//   with an end on any layer), and told of every touch, which may change
//   what is drawn.
// - **What is not the app's stays still:** the caret is clear.
//
// Off, `.arkShots()` is nothing.
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

extension View {
    /// The root of an app, for a screenshot: see above. Nothing at all
    /// unless `Shots.on`.
    public func arkShots() -> some View {
        modifier(ShotsRoot())
    }

    /// Not ready while `loading` (a picture, a cover): `what` names it.
    public func readyWhile(_ loading: Bool, _ what: String) -> some View {
        modifier(ReadyWhile(loading: loading, what: what))
    }
}

/// `Ready`, as SwiftUI watches it.
@MainActor
private final class Signal: ObservableObject {
    static let shared = Signal()
    @Published private(set) var label: String? = nil
    @Published private(set) var ready = false
    #if canImport(UIKit)
    private var link: CADisplayLink?
    #endif

    private init() {
        let r = Ready.shared
        r.onChange = { [weak self] in
            self?.label = r.label
            self?.ready = r.ready
        }
        r.moving = Signal.moving
        r.ticking = { [weak self] on in self?.tick(on) }
    }

    private func tick(_ on: Bool) {
        #if canImport(UIKit)
        link?.invalidate()
        link = nil
        guard on else { return }
        let l = CADisplayLink(target: Frame(), selector: #selector(Frame.frame))
        l.add(to: .main, forMode: .common)
        link = l
        #endif
    }

    /// When each animation seen on a layer's key began (`moving`), and the
    /// keys whose animation began again: repeating ones.
    private static var begun: [String: CFTimeInterval] = [:]
    private static var repeating: Set<String> = []

    /// Whether any layer on the screen has an animation on its way to an
    /// end: something a shared spring did not settle (the system's own).
    /// One that repeats (for ever, or added afresh each time it ends: a
    /// spinner, the caret's blink) or turns pictures over in place has no
    /// end to wait for, and is not counted.
    private static func moving() -> String? {
        #if canImport(UIKit)
        func any(_ l: CALayer) -> String? {
            // What is not drawn moves nothing on the screen: a hidden or
            // clear layer, and all under it.
            if l.isHidden || l.opacity == 0 { return nil }
            let now = l.convertTime(CACurrentMediaTime(), from: nil)
            for key in l.animationKeys() ?? [] {
                guard let a = l.animation(forKey: key) else { continue }
                // No end to wait for: repeating for ever, or lasting for ever
                // (Liquid Glass's lens following what it sits on).
                if a.repeatCount.isInfinite || a.repeatCount >= .greatestFiniteMagnitude || a.repeatDuration.isInfinite || a.duration.isInfinite { continue }
                // A sequence of pictures (`contents`: a spinner's frames, a
                // symbol's effect), which turns over in place and moves
                // nothing: never waited for.
                if (a as? CAPropertyAnimation)?.keyPath == "contents" { continue }
                // Ended, and only kept on the layer (not removed on completion):
                // at its end already.
                let runs = Double(max(a.repeatCount, 1)) * a.duration
                if a.beginTime > 0, now >= a.beginTime + runs + a.timeOffset { continue }
                // The system's material settling into a new theme (Liquid
                // Glass's backdrop, 0.25 s, which the window's speed does
                // not reach: it held the dark switch 0.44 s in app-screens
                // 43327637228): at its end at once, as everything is in
                // shots mode. Its model value is already where it ends.
                if "\(type(of: l))".contains("Backdrop") {
                    l.removeAnimation(forKey: key)
                    continue
                }
                // Begun again, on the same layer and key, since it was last
                // seen: one that repeats by being added afresh each time it
                // ends (money's 0.8 s keyframes in #102's runs), which has no
                // end either.
                let slot = "\(ObjectIdentifier(l).hashValue).\(key)"
                if repeating.contains(slot) { continue }
                if let was = begun[slot], was != a.beginTime { repeating.insert(slot); continue }
                begun[slot] = a.beginTime
                // Named, for the busy reason: what, on which layer (and the
                // view it draws, and that view's parent), for how long.
                let view = l.delegate as? UIView
                let whose = [view, view?.superview].compactMap { $0 }.map { "\(type(of: $0))" }.joined(separator: " in ")
                return "\(type(of: l)).\(key) of \(whose.isEmpty ? "-" : whose) \(type(of: a)) \(a.duration)s repeat \(a.repeatCount) begin \(a.beginTime) now \(now)"
            }
            for s in l.sublayers ?? [] { if let m = any(s) { return m } }
            return nil
        }
        for w in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).flatMap(\.windows) {
            // Every window, a dialog's and a menu's made since the root's
            // first: the system's own Core Animation at a hundred times
            // its speed (`still()`), each time the screen is looked at.
            if w.layer.speed < 100 { w.layer.speed = 100 }
            if let m = any(w.layer) { return m }
        }
        return nil
        #else
        return nil
        #endif
    }
}

#if canImport(UIKit)
/// The display link's target: a frame drawn, said to `Ready`.
private final class Frame: NSObject {
    @objc func frame() { MainActor.assumeIsolated { Ready.shared.frame() } }
}

/// Every touch on a window, seen and let through: each may change the
/// screen, so each says so (`Ready.changed`).
private final class Touched: UIGestureRecognizer, UIGestureRecognizerDelegate {
    init() {
        super.init(target: nil, action: nil)
        cancelsTouchesInView = false
        delaysTouchesBegan = false
        delaysTouchesEnded = false
        delegate = self
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        MainActor.assumeIsolated { Ready.shared.changed() }
        state = .failed
    }

    func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
}
#endif

private struct ShotsRoot: ViewModifier {
    @ObservedObject private var signal = Signal.shared
    /// The theme and the size class: a new theme (the driver's) or a turn
    /// on its side changes everything drawn.
    @Environment(\.colorScheme) private var scheme
    @Environment(\.horizontalSizeClass) private var width
    @Environment(\.verticalSizeClass) private var height

    func body(content: Content) -> some View {
        if Shots.on {
            content
                .transaction { t in
                    t.animation = nil
                    t.disablesAnimations = true
                }
                .overlay(alignment: .topLeading) {
                    if let label = signal.label {
                        // A point of text no one sees, there only to be found:
                        // `ark.ready` while ready, `ark.busy` (and why) while not.
                        Text(label)
                            .font(.system(size: 1))
                            .foregroundStyle(.clear)
                            .frame(width: 1, height: 1)
                            .clipped()
                            .allowsHitTesting(false)
                            .accessibilityIdentifier(signal.ready ? "ark.ready" : "ark.busy")
                            .accessibilityLabel(label)
                            // The theme it draws in, as the window has it: the
                            // driver's dark pass checks it is dark.
                            .accessibilityValue(scheme == .dark ? "dark" : "light")
                    }
                }
                .onAppear(perform: ShotsRoot.still)
                .onChange(of: scheme) { Ready.shared.changed() }
                .onChange(of: width) { Ready.shared.changed() }
                .onChange(of: height) { Ready.shared.changed() }
                .onGeometryChange(for: CGSize.self, of: { $0.size }) { _ in Ready.shared.changed() }
        } else {
            content
        }
    }

    /// What of the system moves, stilled once: UIKit's animations and the
    /// caret; and every touch said to `Ready`.
    private static func still() {
        #if canImport(UIKit)
        UIView.setAnimationsEnabled(false)
        UITextField.appearance().tintColor = .clear
        UITextView.appearance().tintColor = .clear
        for scene in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }) {
            for window in scene.windows where !(window.gestureRecognizers ?? []).contains(where: { $0 is Touched }) {
                window.addGestureRecognizer(Touched())
                // What the system still animates by Core Animation itself,
                // which `setAnimationsEnabled` does not reach (a tab bar's
                // selection, Liquid Glass settling, a navigation bar's
                // title): over in a hundredth of its time, so the ready
                // signal is not held by motion nobody sees in a picture.
                window.layer.speed = 100
            }
        }
        #endif
        Ready.shared.changed()
    }
}

private struct ReadyWhile: ViewModifier {
    let loading: Bool
    let what: String
    @State private var said = false

    func body(content: Content) -> some View {
        content
            .onAppear { say(loading) }
            .onChange(of: loading) { _, now in say(now) }
            .onDisappear { say(false) }
    }

    private func say(_ now: Bool) {
        guard Shots.on, now != said else { return }
        said = now
        if now { Ready.shared.begin(what) } else { Ready.shared.end(what) }
    }
}

#if canImport(UIKit)
/// The app's own windows, as a screenshot walk inside the app takes them
/// (ui/docs/shots.md, "In the App"): drawn by the app itself, every window
/// of its scenes in their order (what a sheet, a menu or a popover is up
/// in), at the screen's scale. The status bar is the system's, not the
/// app's, and is not in it, as Android's walk leaves it out too.
@MainActor
public enum ShotsWindow {
    private static var windows: [UIWindow] {
        UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .filter { !$0.isHidden && $0.alpha > 0 }
            .sorted { $0.windowLevel < $1.windowLevel }
    }

    /// How each window was drawn into the last picture, lowest first: its
    /// kind, its level, and whether it drew its hierarchy (else its layer
    /// was rendered): what a walk logs beside a picture of more than one.
    public private(set) static var drawn: [String] = []

    /// `w` drawn into `ctx` where it is on the screen: its hierarchy as the
    /// screen has it now (the ready signal said everything is committed),
    /// else after the next update, else its layer rendered; whether its
    /// hierarchy drew.
    private static func draw(_ w: UIWindow, in ctx: CGContext) -> Bool {
        if w.drawHierarchy(in: w.frame, afterScreenUpdates: false) { return true }
        if w.drawHierarchy(in: w.frame, afterScreenUpdates: true) { return true }
        ctx.saveGState()
        ctx.translateBy(x: w.frame.minX, y: w.frame.minY)
        w.layer.render(in: ctx)
        ctx.restoreGState()
        return false
    }

    /// The windows drawn as the screen shows them: every window of the
    /// app's scenes from the lowest level up (the app's, then an alert's,
    /// a menu's, a confirmation dialog's, each with what is presented in
    /// it), one over another.
    public static func picture() -> UIImage? {
        picture(of: windows)
    }

    private static func picture(of ws: [UIWindow]) -> UIImage? {
        guard let main = ws.first(where: \.isKeyWindow) ?? ws.first else { return nil }
        let format = UIGraphicsImageRendererFormat()
        format.scale = main.screen.scale
        format.opaque = true
        let size = main.windowScene?.coordinateSpace.bounds.size ?? main.bounds.size
        var how: [String] = []
        let image = UIGraphicsImageRenderer(size: size, format: format).image { ctx in
            UIColor.black.setFill()
            ctx.fill(CGRect(origin: .zero, size: size))
            for w in ws {
                let ok = draw(w, in: ctx.cgContext)
                how.append("\(type(of: w)) \(Int(w.windowLevel.rawValue)) \(ok ? "drawn" : "rendered")")
            }
        }
        drawn = how
        return image
    }

    /// Whether anything is presented over the app's own screen: a view
    /// controller presented in a window (a sheet, an alert, a confirmation
    /// dialog, a popover), or a window over the app's (an alert's, a
    /// menu's).
    public static var presenting: Bool {
        let ws = windows
        if ws.contains(where: { $0.rootViewController?.presentedViewController != nil }) { return true }
        let base = ws.first?.windowLevel ?? .normal
        return ws.contains { $0.windowLevel > base && $0.rootViewController != nil }
    }

    /// Every window drawn dark, or light, whatever the device's appearance:
    /// what the system's switch does to an app, and what follows it (a
    /// sheet, a menu, the system's own controls), its bars and their glass
    /// drawn again (ArkAppearance).
    public static func theme(dark: Bool) {
        ArkAppearance.force(dark: dark)
        Ready.shared.changed()
    }

    /// How light the app's bars are drawn (0 black, 1 white): the middle
    /// of the tab bar, or else of the top navigation bar, in its own window
    /// drawn alone; nil with none on the screen. `image` is the picture,
    /// whose windows' drawing (`drawn`) it keeps. A dark picture's
    /// bars are dark and a light one's light: a bar the other way kept its
    /// old theme (ArkAppearance).
    public static func barLightness(in _: UIImage) -> (bar: String, lightness: Double)? {
        func bars(_ v: UIView, _ out: inout [UIView]) {
            if v.isHidden || v.alpha == 0 { return }
            if v is UITabBar || v is UINavigationBar { out.append(v) }
            for s in v.subviews { bars(s, &out) }
        }
        var found: [UIView] = []
        for w in windows { bars(w, &found) }
        let sized = found.filter { $0.bounds.width > 40 && $0.bounds.height > 20 && $0.window != nil }
        // Its own window alone, so what is over it (a dialog's dim, a
        // sheet) says nothing of the bar's theme.
        guard let bar = sized.first(where: { $0 is UITabBar }) ?? sized.first, let w = bar.window,
              let alone = picture(of: [w]), let cg = alone.cgImage else { return nil }
        let r = bar.convert(bar.bounds, to: w).offsetBy(dx: w.frame.minX, dy: w.frame.minY)
        // Its middle: half its height, its width but the ends.
        let band = CGRect(x: r.minX + r.width * 0.15, y: r.midY - r.height * 0.25, width: r.width * 0.7, height: r.height * 0.5)
        let s = alone.scale
        let px = CGRect(x: band.minX * s, y: band.minY * s, width: band.width * s, height: band.height * s).integral
            .intersection(CGRect(x: 0, y: 0, width: cg.width, height: cg.height))
        guard px.width >= 1, px.height >= 1, let crop = cg.cropping(to: px) else { return nil }
        // Drawn small into eight-bit grey: the median of its points.
        let (cw, ch) = (min(64, Int(px.width)), min(16, Int(px.height)))
        var grey = [UInt8](repeating: 0, count: cw * ch)
        let drew = grey.withUnsafeMutableBytes { buf -> Bool in
            guard let ctx = CGContext(data: buf.baseAddress, width: cw, height: ch, bitsPerComponent: 8, bytesPerRow: cw,
                                      space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return false }
            ctx.draw(crop, in: CGRect(x: 0, y: 0, width: cw, height: ch))
            return true
        }
        guard drew else { return nil }
        let sorted = grey.sorted()
        return ("\(type(of: bar))", Double(sorted[sorted.count / 2]) / 255)
    }

    /// Every scene turned on its side (`landscape`), or upright: as the
    /// device turned. What the scene said if it would not (its error).
    public static func orient(landscape: Bool, refused: @escaping (String) -> Void = { _ in }) {
        for scene in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }) {
            scene.windows.first?.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations()
            scene.requestGeometryUpdate(.iOS(interfaceOrientations: landscape ? .landscapeRight : .portrait)) { e in
                refused("\(e.localizedDescription)")
            }
        }
        Ready.shared.changed()
    }

    /// The device itself told it is turned, as the simulator's own Rotate
    /// does: for a scene that would not turn by its geometry alone.
    public static func turnDevice(landscape: Bool) {
        UIDevice.current.setValue((landscape ? UIInterfaceOrientation.landscapeRight : .portrait).rawValue, forKey: "orientation")
        UIViewController.attemptRotationToDeviceOrientation()
        Ready.shared.changed()
    }

    /// Whether the app's scene is wider than tall now.
    public static var wide: Bool {
        guard let w = windows.first else { return false }
        let b = w.windowScene?.coordinateSpace.bounds ?? w.bounds
        return b.width > b.height
    }
}
#endif
