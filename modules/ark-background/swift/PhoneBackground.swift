// ark-background on the iPhone, the iPad and the Mac (README.md): the
// `NativeBackground` protocol UniFFI writes.
//
// - iPhone, iPad: a `BGAppRefreshTask` of the identifier `<bundle id>.sync`,
//   which ui/prebuild.py lists in Info.plist (`BGTaskSchedulerPermittedIdentifiers`,
//   with the `fetch` background mode) only for an app whose app.toml says
//   `[background] sync`. Registered at launch (`register(process:)`, before
//   launch finishes, as Apple requires), and asked for again after each run.
// - Mac: an `NSBackgroundActivityScheduler` repeating at the interval.
//
// Off until the app asks (`set`), and the app asks only once the person
// turned it on; what was asked is kept in the app's defaults. Each run is a
// wake of the process's one runtime (`ArkProcess.wake(.sync, …)`), and the
// task is finished when the runtime is idle or the system's time is up.
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
#if canImport(BackgroundTasks) && os(iOS)
import BackgroundTasks
#endif

public final class PhoneBackground: NativeBackground, @unchecked Sendable {
    public static let shared = PhoneBackground()
    private let key = "ark.background.every"
    private var process: ArkProcess?
    #if os(macOS)
    private var scheduler: NSBackgroundActivityScheduler?
    #endif

    private init() {}

    static var identifier: String { (Bundle.main.bundleIdentifier ?? "app") + ".sync" }

    /// What was asked, in seconds; none, nothing.
    private var every: UInt32? {
        let v = UserDefaults.standard.integer(forKey: key)
        return v > 0 ? UInt32(v) : nil
    }

    /// At launch, before it finishes: the task's handler registered (it
    /// wakes `process`), and what was asked before asked again.
    public func register(process: ArkProcess) {
        self.process = process
        #if os(iOS)
        BGTaskScheduler.shared.register(forTaskWithIdentifier: Self.identifier, using: nil) { [weak self] task in
            self?.run(task)
        }
        #endif
        if let every { arm(every) }
    }

    public func set(everyS: UInt32?) {
        UserDefaults.standard.set(Int(everyS ?? 0), forKey: key)
        if let everyS { arm(everyS) } else { disarm() }
    }

    public func wanted(reply: WantedReply) {
        reply.wanted(everyS: every)
    }

    private func arm(_ every: UInt32) {
        #if os(iOS)
        let r = BGAppRefreshTaskRequest(identifier: Self.identifier)
        r.earliestBeginDate = Date(timeIntervalSinceNow: TimeInterval(max(every, 900)))
        try? BGTaskScheduler.shared.submit(r)
        #elseif os(macOS)
        disarm()
        let s = NSBackgroundActivityScheduler(identifier: Self.identifier)
        s.repeats = true
        s.interval = TimeInterval(max(every, 900))
        s.schedule { [weak self] done in
            self?.process?.wake(reason: .sync, budgetMs: 25_000, done: Finished { _ in done(.finished) })
        }
        scheduler = s
        #endif
    }

    private func disarm() {
        #if os(iOS)
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: Self.identifier)
        #elseif os(macOS)
        scheduler?.invalidate()
        scheduler = nil
        #endif
    }

    #if os(iOS)
    private func run(_ task: BGTask) {
        // The next one asked for first: a run cut short still leaves one.
        if let every { arm(every) }
        guard let process else { return task.setTaskCompleted(success: false) }
        let finished = Finished { idle in task.setTaskCompleted(success: idle) }
        task.expirationHandler = { finished.finished(idle: false) }
        process.wake(reason: .sync, budgetMs: 25_000, done: finished)
    }
    #endif
}

/// A wake's end handed to a closure, once.
final class Finished: WakeDone, @unchecked Sendable {
    private var then: ((Bool) -> Void)?
    private let lock = NSLock()
    init(_ then: @escaping (Bool) -> Void) { self.then = then }
    func finished(idle: Bool) {
        lock.lock()
        let t = then
        then = nil
        lock.unlock()
        t?(idle)
    }
}
