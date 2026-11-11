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

    /// Whether any layer on the screen has an animation on its way to an
    /// end: something a shared spring did not settle (the system's own).
    /// One that repeats for ever (a spinner, the caret's blink) has no end
    /// to wait for, and is not counted.
    private static func moving() -> String? {
        #if canImport(UIKit)
        func any(_ l: CALayer) -> String? {
            let now = l.convertTime(CACurrentMediaTime(), from: nil)
            for key in l.animationKeys() ?? [] {
                guard let a = l.animation(forKey: key) else { continue }
                // No end to wait for: repeating for ever, or lasting for ever
                // (Liquid Glass's lens following what it sits on).
                if a.repeatCount.isInfinite || a.repeatCount >= .greatestFiniteMagnitude || a.repeatDuration.isInfinite || a.duration.isInfinite { continue }
                // Ended, and only kept on the layer (not removed on completion):
                // at its end already.
                let runs = Double(max(a.repeatCount, 1)) * a.duration
                if a.beginTime > 0, now >= a.beginTime + runs + a.timeOffset { continue }
                // Named, for the busy reason: what, on which layer, for how long.
                return "\(type(of: l)).\(key) \(type(of: a)) \(a.duration)s"
            }
            for s in l.sublayers ?? [] { if let m = any(s) { return m } }
            return nil
        }
        for w in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).flatMap(\.windows) {
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
