// A slider with no handle: a capsule track, filled from its start to where
// the value is, as the system's player draws its scrubber and its volume.
// Held, the track thickens a little (`Springs.size`), so it is plain that it
// is held; a touch puts the value where it touches and a drag follows the
// finger. Said to VoiceOver as an adjustable value. Android's twin is
// arkui's `Bar` (kotlin/arkui).
import SwiftUI

public struct Bar: View {
    /// How thick the track is at rest, and held; and the height the finger has.
    public static let thickness: CGFloat = 6
    public static let held: CGFloat = 10
    public static let touch: CGFloat = 28

    let value: Double
    let fill: Color
    let track: Color
    let enabled: Bool
    let onValue: (Double) -> Void
    let onDone: () -> Void
    @State private var holding = false

    /// A handleless slider over `0...1`. `onValue` is said for every value the
    /// finger passes, and `onDone` when it lets go.
    public init(value: Double, fill: Color = Color.accent, track: Color = Color.quiet.opacity(0.25), enabled: Bool = true,
                onValue: @escaping (Double) -> Void, onDone: @escaping () -> Void = {}) {
        self.value = value
        self.fill = fill
        self.track = track
        self.enabled = enabled
        self.onValue = onValue
        self.onDone = onDone
    }

    public var body: some View {
        let shown = min(max(value, 0), 1)
        GeometryReader { g in
            let thick = holding ? Bar.held : Bar.thickness
            ZStack(alignment: .leading) {
                Capsule().fill(track)
                // At least as wide as it is thick, so a start reads as a dot rather than nothing.
                Capsule().fill(fill).frame(width: shown > 0 ? max(g.size.width * shown, thick) : 0)
            }
            .frame(height: thick)
            .frame(maxHeight: .infinity)
            .animation(Springs.size, value: holding)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { v in
                guard enabled else { return }
                holding = true
                onValue(min(max(v.location.x / max(g.size.width, 1), 0), 1))
            }.onEnded { _ in
                guard enabled else { return }
                holding = false
                onDone()
            })
        }
        .frame(height: Bar.touch)
        .opacity(enabled ? 1 : 0.5)
        .accessibilityElement()
        .accessibilityValue(Text("\(Int((shown * 100).rounded())) percent"))
        .accessibilityAdjustableAction { d in
            guard enabled else { return }
            switch d {
            case .increment: onValue(min(shown + 0.05, 1)); onDone()
            case .decrement: onValue(max(shown - 0.05, 0)); onDone()
            @unknown default: break
            }
        }
    }
}
