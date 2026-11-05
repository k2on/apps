// Whether things move, said once for every Ark app: the system's Reduce
// Motion, read here and nowhere else, as `\.reducesMotion`. Reduced, what
// slides, springs, scales or flies across (a transition's move, a lift, a
// press's shrink) becomes a short cross-fade with nothing passing where it
// goes: `Springs.calm` for the animation (`Animation.reduced(_:)`,
// `.motion(_:value:)`), `.opacity` for the transition (`.motionTransition`),
// no scale (`PressStyle`, `Reorderable`). What fades still fades, and what
// changes colour still does: those help, and do not move.
//
// The system's own motion (a sheet, the zoom into a page, a tab) follows the
// system's setting by itself; this is only for what the apps draw.
import SwiftUI

public enum Motion {
    /// Animate whatever the system's Reduce Motion says. True for now: the
    /// person these apps are made for has Reduce Motion on and still wants to
    /// see the motion while it is being worked on. Flip it to false to follow
    /// the system's setting, as an app should.
    public static let alwaysAnimate = true

    /// Whether motion is reduced, given the system's setting.
    public static func reduced(system: Bool) -> Bool { alwaysAnimate ? false : system }
}

extension EnvironmentValues {
    /// Whether what moves should cross-fade instead: the system's Reduce
    /// Motion, unless `Motion.alwaysAnimate` keeps the motion on.
    public var reducesMotion: Bool { Motion.reduced(system: accessibilityReduceMotion) }
}

extension View {
    /// `animation` when `value` changes, or under Reduce Motion the short
    /// ease that stands in for it.
    public func motion<V: Equatable>(_ animation: Animation, value: V) -> some View {
        modifier(MotionAnimation(animation: animation, value: value))
    }

    /// `transition` as this view comes and goes, or under Reduce Motion a fade.
    public func motionTransition(_ transition: AnyTransition) -> some View {
        modifier(MotionTransition(transition: transition))
    }

    /// Scaled by `scale`, unless motion is reduced: a press, a lift, a
    /// cover that rests while its sound is paused.
    public func motionScale(_ scale: CGFloat, anchor: UnitPoint = .center) -> some View {
        modifier(MotionScale(scale: scale, anchor: anchor))
    }
}

private struct MotionAnimation<V: Equatable>: ViewModifier {
    let animation: Animation
    let value: V
    @Environment(\.reducesMotion) private var still

    func body(content: Content) -> some View {
        content.animation(animation.reduced(still), value: value)
    }
}

private struct MotionTransition: ViewModifier {
    let transition: AnyTransition
    @Environment(\.reducesMotion) private var still

    func body(content: Content) -> some View {
        content.transition(still ? .opacity : transition)
    }
}

private struct MotionScale: ViewModifier {
    let scale: CGFloat
    let anchor: UnitPoint
    @Environment(\.reducesMotion) private var still

    func body(content: Content) -> some View {
        content.scaleEffect(still ? 1 : scale, anchor: anchor)
    }
}

/// What a card, a cover or a chip does under the finger: the instant it is
/// touched, before it is let go, it gives a little — 0.97 of itself, a
/// touch dimmer — and springs back, critically damped, when the finger
/// lifts or slides off. Under Reduce Motion it only dims.
public struct PressStyle: ButtonStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        Pressed(configuration: configuration)
    }

    private struct Pressed: View {
        let configuration: Configuration
        @Environment(\.reducesMotion) private var still

        var body: some View {
            let down = configuration.isPressed
            configuration.label
                .scaleEffect(down && !still ? 0.97 : 1)
                .opacity(down ? 0.8 : 1)
                // Down at once, so the touch is answered as it lands; back up on a spring.
                .animation(down ? Springs.fade : Springs.size, value: down)
        }
    }
}

/// What a row does under the finger: the instant it is touched it takes the
/// pressed row's colour (`containerHighest`), and lets it go when the finger
/// lifts or slides off. A row the width of the page does not shrink.
public struct RowPressStyle: ButtonStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(configuration.isPressed ? Color.containerHighest : Color.clear)
            .animation(configuration.isPressed ? nil : Springs.fade, value: configuration.isPressed)
    }
}

extension ButtonStyle where Self == PressStyle {
    /// A card's, a cover's or a chip's press: it gives a little under the finger.
    public static var press: PressStyle { PressStyle() }
}

extension ButtonStyle where Self == RowPressStyle {
    /// A row's press: it darkens under the finger.
    public static var pressRow: RowPressStyle { RowPressStyle() }
}
