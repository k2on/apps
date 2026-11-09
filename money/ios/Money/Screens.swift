// The screens whose state, rules and layout are Rust's (ui/docs/app.md):
// what their generated SwiftUI (Generated/) reads and sends to. Each holds
// its runtime over the bridge (`AccountsApp`, money_app run behind
// money_ffi) and publishes the record it shows, read again after every
// message and whenever a view it holds moved. Nothing here decides
// anything: a message goes to Rust, and Rust says what the page is now,
// which route to open, and what to ask of the phone (the device modules
// money lists in modules.toml).
import Combine
import Foundation

/// The Accounts tab, running in Rust.
@MainActor
final class AccountsScreen: ObservableObject {
    @Published private(set) var page: AccountsPage
    /// Opens a route on the tab's back stack: the shell's navigator.
    var open: (Route) -> Void = { _ in }
    /// Raises the reconcile sheet for an account: the shell's own, until
    /// reconciling moves into Rust.
    var reconcile: (Id) -> Void = { _ in }

    private let app: AccountsApp
    private let shell: ScreenShell

    /// The tab over `session`; `refresh` asks the bank, `today` is the
    /// session's day (the demo's in the demo).
    init(session: Session, modules: PhoneModules, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        self.shell = shell
        app = AccountsApp(session: session, shell: shell)
        app.useModules(haptics: modules.haptics, clipboard: modules.clipboard, share: modules.share)
        page = app.page()
        shell.opened = { [weak self] r in self?.open(r) }
        shell.reconciled = { [weak self] id in self?.reconcile(id) }
    }

    /// `msg` taken by the app; the page read again.
    func send(_ msg: AccountsMsg) {
        app.send(msg: msg)
        page = app.page()
    }

    /// The page read again: a view it holds moved.
    func reload() {
        page = app.page()
    }
}

/// money's shell running in Rust (money/app/src/shell): its sections,
/// toolbar, inspector, search and commands, which the generated
/// `MoneyShellView`, `MoneySettingsView` and `MoneyCommands` read and send
/// to. It is the app's for its whole life — the Mac's menu bar is made
/// before any session is open — and runs once one is (`connect`); until
/// then it shows Budget and takes nothing. A command only a hosted screen
/// can carry out comes back by name (`native`).
@MainActor
final class MoneyShellScreen: ObservableObject {
    @Published private(set) var page = ShellPage(section: "budget", inspector: false, search: "", showing: false, toReview: nil)
    /// A command the hosted screens carry out: "new-transaction", "refresh", "search <text>"…
    var native: (String) -> Void = { _ in }

    private var app: ShellApp? = nil
    private var shell: ScreenShell? = nil

    /// Running over `session`.
    func connect(session: Session, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        shell.other = { [weak self] name in self?.native(name) }
        self.shell = shell
        let app = ShellApp(session: session, shell: shell)
        self.app = app
        page = app.page()
    }

    /// The session closed: back to Budget, taking nothing.
    func disconnect() {
        app = nil
        shell = nil
    }

    /// `msg` taken by the shell; the page read again.
    func send(_ msg: ShellMsg) {
        guard let app else { return }
        app.send(msg: msg)
        page = app.page()
    }

    /// The page read again: a view it holds moved.
    func reload() {
        if let app { page = app.page() }
    }
}

/// What a screen's runtime asks of the shell, on the thread it sends on
/// (the main one: every message comes from a view).
final class ScreenShell: AppShell, @unchecked Sendable {
    var opened: @MainActor (Route) -> Void = { _ in }
    var reconciled: @MainActor (Id) -> Void = { _ in }
    /// Any other native request: the shell's commands, by name.
    var other: @MainActor (String) -> Void = { _ in }
    private let refresh: () -> Void
    private let day: () -> String

    init(refresh: @escaping () -> Void, today: @escaping () -> String) {
        self.refresh = refresh
        day = today
    }

    func openRoute(route: Route) {
        // On the main queue, after the message that asked: every message comes from a view.
        Task { @MainActor in self.opened(route) }
    }

    func native(name: String) {
        if name == "refresh" {
            refresh()
        } else if name.hasPrefix("reconcile ") {
            let id = Id(String(name.dropFirst("reconcile ".count)))
            Task { @MainActor in self.reconciled(id) }
        } else {
            Task { @MainActor in self.other(name) }
        }
    }

    func nowMs() -> Int64 { Int64((Date().timeIntervalSince1970 * 1000).rounded()) }

    func today() -> String { day() }
}

/// The device modules the phone registers with every screen: the ones
/// money lists in money/modules.toml, whose Swift (modules/<name>/swift)
/// the project compiles in (modules.yml).
struct PhoneModules {
    var haptics: Haptics? = nil
    var clipboard: Clipboard? = nil
    var share: Share? = nil
}
