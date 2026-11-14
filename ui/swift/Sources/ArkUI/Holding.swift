// A core's views as a screen holds them (`ark_peer::store` behind an app's
// `Session`): a screen holds the view it shows while it is on screen, and
// reads it again whenever the core says it moved — the helper every Ark app
// on iOS draws its views with, as `watched.watch(view) { read }` is on
// Android and the store itself is in iced.
//
//     RegisterView(account: id)
//         .holding(MoneyView.register(account: id))
//
// Nothing here splices, counts or keeps rows: the core holds each view by
// count, keeps its rows spliced and says which views moved; this turns that
// into SwiftUI's observation. The app puts one `Watched` in the environment
// (`.environmentObject(watched)`), its `hold` the session's.
//
// An `ObservableObject`, not `@Observable`: the Observation macros need the
// compiler's plugin server, which a sandboxed nix build cannot run.
import Combine
import SwiftUI

/// An app's views by key `V` (its `MoneyView`, its `HarkenView`): `hold` is
/// its session's hold, and `moved` is told — on the main actor — which views
/// moved, which whatever reads them through `read` is drawn again for.
@MainActor
public final class Watched<V: Hashable>: ObservableObject {
    @Published private var ticks = [V: Int]()
    private let holdOf: (V) -> AnyObject?

    public init(hold: @escaping (V) -> AnyObject?) {
        holdOf = hold
    }

    /// `views` moved: whatever reads one of them is drawn again.
    public func moved(_ views: [V]) {
        for v in views { ticks[v, default: 0] += 1 }
    }

    /// `view` held until the answer is let go; what reads it drawn again,
    /// now that it has rows.
    public func hold(_ view: V) -> AnyObject? {
        let held = holdOf(view)
        ticks[view, default: 0] += 1
        return held
    }

    /// `read` of `view`, drawn again whenever `view` moves.
    public func read<T>(_ view: V, _ read: () -> T) -> T {
        _ = ticks[view]
        return read()
    }
}

/// `view` held while the modified view is on screen; none holds nothing.
private struct Holding<V: Hashable>: ViewModifier {
    let view: V?
    @EnvironmentObject private var watched: Watched<V>
    @State private var held: AnyObject?

    func body(content: Content) -> some View {
        content
            .onAppear { held = view.flatMap { watched.hold($0) } }
            .onDisappear { held = nil }
            .onChange(of: view) { _, now in held = now.flatMap { watched.hold($0) } }
    }
}

extension View {
    /// `view` held — opened and kept by the core — while this is on screen,
    /// through the `Watched` in the environment; none holds nothing.
    public func holding<V: Hashable>(_ view: V?) -> some View {
        modifier(Holding(view: view))
    }
}
