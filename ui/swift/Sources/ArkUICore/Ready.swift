// Screenshots that never depend on time (ui/docs/shots.md): whether this
// launch is one (`Shots.on`), and whether the screen is ready to be taken
// (`Ready`). Foundation only, so an app's model calls it and still
// type-checks on Linux; ArkUI's `.arkShots()` draws it, ticks it on each
// frame and says what of the screen still moves.
import Foundation

public enum Shots {
    /// `-shots YES` among the launch arguments (the argument domain): a
    /// screenshot, or a test of one. Every shared spring is then at its end
    /// at once (ArkUI's `Springs`), and the ready signal is shown.
    public static let on: Bool = UserDefaults.standard.bool(forKey: "shots")
}

/// Whether the screen is what it will stay. Ready when nothing the app said
/// it waits for is still to come (`begin`/`end`: a picture loading;
/// `probe`: the runtime idle, the peer settled), nothing on the screen is
/// on its way anywhere (`moving`), and a frame has been drawn since. Then
/// the driver finds `ark.ready`, labelled `ready <generation> <route>`;
/// anything that may change what is drawn (a touch, a link, a route, a
/// snapshot from the peer) takes it away and grows the generation, and the
/// driver waits for a newer one. Nothing here waits for a time.
@MainActor
public final class Ready {
    public static let shared = Ready()

    /// Each thing the app waits for, by name, and how many of it.
    private var waiting: [String: Int] = [:]
    /// Whether the app's own work is done, each asked on every frame.
    private var probes: [() -> Bool] = []
    /// Frames drawn since everything was done.
    private var quiet = 0
    /// Grows each time the screen may change.
    public private(set) var generation = 0
    /// Whether the screen is ready, at `generation`.
    public private(set) var ready = false
    /// Where the app is (a route's path, a section), as it last said.
    public private(set) var route = ""
    /// Told whenever `ready`, `generation` or `route` changes: the root
    /// that draws the signal.
    public var onChange: (() -> Void)?
    /// Asked to start (true) or stop (false) calling `frame()` once a frame.
    public var ticking: ((Bool) -> Void)?
    /// Whether anything on the screen is still on its way to an end.
    public var moving: () -> Bool = { false }
    private var ticks = false

    private init() {}

    /// `what` is on its way: not ready until as many `end(what)`.
    public func begin(_ what: String) {
        guard Shots.on else { return }
        waiting[what, default: 0] += 1
        changed()
    }

    /// `what` has come (or failed).
    public func end(_ what: String) {
        guard Shots.on, let n = waiting[what] else { return }
        waiting[what] = n > 1 ? n - 1 : nil
        changed()
    }

    /// `idle` asked on each frame, until it says so, whenever the screen
    /// may have changed.
    public func probe(_ idle: @escaping () -> Bool) {
        guard Shots.on else { return }
        probes.append(idle)
        changed()
    }

    /// The app is at `route` now.
    public func at(route: String) {
        guard Shots.on, route != self.route else { return }
        self.route = route
        changed()
    }

    /// What is drawn may have changed: not ready, a newer generation to
    /// come, every frame checked until it is.
    public func changed() {
        guard Shots.on else { return }
        ready = false
        generation += 1
        quiet = 0
        if !ticks {
            ticks = true
            ticking?(true)
        }
        onChange?()
    }

    /// One frame drawn: ready once everything is done and a frame has been
    /// drawn so (what is done is then on the screen, not only in a view).
    public func frame() {
        guard waiting.isEmpty, probes.allSatisfy({ $0() }), !moving() else {
            quiet = 0
            return
        }
        quiet += 1
        guard quiet > 1, !ready else { return }
        ready = true
        ticks = false
        ticking?(false)
        onChange?()
    }

    /// What it says to the driver: `ready <generation> <route>`.
    public var label: String { "ready \(generation) \(route)" }
}
