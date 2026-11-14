// A screen's runtime as its generated SwiftUI reads it, for every Ark app:
// the record it shows (`page`, read again after every message and whenever
// its app says something moved) and where a press goes (`send`). Nothing
// here decides anything: a message goes to Rust, and Rust says what the page
// is now. An app's generated holders are this over each of its screens'
// runtimes (`typealias AccountsScreen = ArkScreen<AccountsApp>`, written by
// ui/prebuild.py from app.toml's `[screens]`); the first run's is too.
//
// What waits for the page to be drawn (`Command::AfterDraw`: an inspector
// put away before a launch moves a section) is taken once SwiftUI has drawn
// the page just read: a few frames on, as a task on the main actor may run
// before the update it follows is committed.
import ArkUI
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
#if canImport(Combine)
import Combine
#endif

/// A screen's runtime, as the bridge exports every one of them.
public protocol ArkRuntime: AnyObject {
    associatedtype Page
    associatedtype Msg
    func page() -> Page
    func send(msg: Msg)
    func version() -> UInt64
    func isIdle() -> Bool
    func drawn() -> Bool
    func awaitsDraw() -> Bool
}

/// The first run's pages are a screen like any other.
extension WelcomeApp: ArkRuntime {}

@MainActor
public final class ArkScreen<Runtime: ArkRuntime>: ObservableObject {
    /// What the screen shows now.
    @Published public private(set) var page: Runtime.Page
    /// The runtime, while there is one: a screen made before its app runs
    /// (the Mac's menu bar, made with the app) shows `blank` and keeps what
    /// it is sent until one is attached.
    public private(set) var runtime: Runtime?
    private var early: [Runtime.Msg] = []
    private let blank: Runtime.Page?

    /// The screen over `runtime`.
    public init(_ runtime: Runtime) {
        self.runtime = runtime
        page = runtime.page()
        blank = nil
    }

    /// A screen with no runtime yet, showing `blank`.
    public init(blank: Runtime.Page) {
        runtime = nil
        page = blank
        self.blank = blank
    }

    /// Running over `runtime` (none: back to blank, keeping nothing): what
    /// it was sent before it ran taken first, in order.
    public func attach(_ runtime: Runtime?) {
        guard runtime !== self.runtime else { return }
        self.runtime = runtime
        guard let runtime else {
            if let blank { page = blank }
            return
        }
        let waiting = early
        early = []
        for m in waiting { runtime.send(msg: m) }
        page = runtime.page()
        afterDraw()
    }

    /// `msg` taken by the runtime; the page read again.
    public func send(_ msg: Runtime.Msg) {
        let began = ProcessInfo.processInfo.systemUptime
        defer { Ready.shared.spent("\(Runtime.self)'s page", since: began) }
        guard let runtime else { return early.append(msg) }
        runtime.send(msg: msg)
        page = runtime.page()
        afterDraw()
    }

    /// The page read again: something it holds moved. Every screen of an
    /// app is read again whenever anything of the app moved, most of them
    /// the same as they were: only one that differs is told to its views,
    /// which are drawn again only then (a page told the same drew every
    /// screen of the app again on each change: app-screens 43451267924,
    /// one to eight seconds a step on the iPhone and the iPad).
    public func reload() {
        let began = ProcessInfo.processInfo.systemUptime
        defer { Ready.shared.spent("\(Runtime.self)'s page", since: began) }
        guard let runtime else { return }
        let now = runtime.page()
        if !Same.values(page, now) { page = now }
        afterDraw()
    }

    /// Nothing left to come (`Runtime::is_idle`), and the number that grows
    /// with every message: the screenshot drivers' ready signal.
    public var isIdle: Bool { runtime?.isIdle() ?? true }
    public var version: UInt64 { runtime?.version() ?? 0 }

    private func afterDraw() {
        guard let runtime, runtime.awaitsDraw() else { return }
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(50))
            guard let self, let runtime = self.runtime else { return }
            if runtime.drawn() {
                let now = runtime.page()
                if !Same.values(self.page, now) { self.page = now }
            }
            self.afterDraw()
        }
    }
}

/// Whether two values are the same, when their type can say (a page's
/// record is `Equatable`, as UniFFI writes every record): else never.
enum Same {
    static func values<T>(_ a: T, _ b: T) -> Bool {
        guard let a = a as? any Equatable else { return false }
        return equal(a, b)
    }

    private static func equal<E: Equatable>(_ a: E, _ b: Any) -> Bool {
        guard let b = b as? E else { return false }
        return a == b
    }
}
