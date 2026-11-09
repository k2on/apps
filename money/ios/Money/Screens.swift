// The screens whose state, rules and layout are Rust's (docs/ark-app.md):
// what their generated SwiftUI (Generated/) reads and sends to. Each holds
// its runtime over the bridge (`AccountsApp`, money_app run behind
// money_ffi) and publishes the record it shows, read again after every
// message and whenever a view it holds moved. Nothing here decides
// anything: a message goes to Rust, and Rust says what the page is now,
// which route to open, and what to ask of the phone (Modules.swift).
import Combine
import Foundation

/// The Accounts tab, running in Rust.
@MainActor
final class AccountsScreen: ObservableObject {
    @Published private(set) var page: AccountsPage
    /// Opens a route on the tab's back stack: the shell's navigator.
    var open: (Route) -> Void = { _ in }

    private let app: AccountsApp
    private let shell: ScreenShell

    /// The tab over `session`; `refresh` asks the bank, `nowMs` is the
    /// time (the demo's day in the demo).
    init(session: Session, modules: PhoneModules, refresh: @escaping () -> Void, nowMs: @escaping () -> Int64) {
        let shell = ScreenShell(refresh: refresh, nowMs: nowMs)
        self.shell = shell
        app = AccountsApp(session: session, shell: shell)
        app.useModules(haptics: modules.haptics, clipboard: modules.clipboard, share: modules.share, camera: modules.camera)
        page = app.page()
        shell.opened = { [weak self] r in self?.open(r) }
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

/// What a screen's runtime asks of the shell, on the thread it sends on
/// (the main one: every message comes from a view).
final class ScreenShell: AppShell, @unchecked Sendable {
    var opened: @MainActor (Route) -> Void = { _ in }
    private let refresh: () -> Void
    private let now: () -> Int64

    init(refresh: @escaping () -> Void, nowMs: @escaping () -> Int64) {
        self.refresh = refresh
        now = nowMs
    }

    func open(route: Route) {
        MainActor.assumeIsolated { opened(route) }
    }

    func native(name: String) {
        if name == "refresh" { refresh() }
    }

    func nowMs() -> Int64 { now() }
}

/// The device modules the phone registers with every screen.
struct PhoneModules {
    var haptics: Haptics? = nil
    var clipboard: Clipboard? = nil
    var share: Share? = nil
    var camera: Camera? = nil
}
