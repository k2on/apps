// The screens whose state, rules and layout are Rust's (ui/docs/app.md):
// what their generated SwiftUI (Generated/) reads and sends to. Each holds
// its runtime over the bridge (`AccountsApp`, money_app run behind
// money_ffi) and publishes the record it shows, read again after every
// message and whenever a view it holds moved. Nothing here decides
// anything: a message goes to Rust, and Rust says what the page is now,
// which route to open, and what to ask of the phone (the device modules
// money lists in modules.toml).
import ArkUICore
import Combine
import Foundation

/// The first run's page every Ark app shares, as its generated WelcomeView
/// reads it: the model's (`FirstRunApp`), a press sent back to it.
@MainActor
final class FirstRunScreen: ObservableObject {
    @Published var page = WelcomePage(intro: nil, server: nil, signIn: nil, opening: nil, confirm: nil)
    var press: (WelcomeMsg) -> Void = { _ in }

    func send(_ m: WelcomeMsg) { press(m) }
}

/// The Accounts tab, running in Rust.
@MainActor
final class AccountsScreen: ObservableObject {
    @Published private(set) var page: AccountsPage
    /// Opens a route on the tab's back stack: the shell's navigator.
    var open: (Route) -> Void = { _ in }
    /// What only the shell does: a sheet (Reconcile).
    var request: (MoneyRequest) -> Void = { _ in }

    private let app: AccountsApp
    private let shell: ScreenShell

    /// The tab over `session`; `refresh` asks the bank, `today` is the
    /// session's day (the demo's in the demo).
    init(session: Session, devices: Devices?, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        self.shell = shell
        app = AccountsApp(session: session, shell: shell)
        if let devices { app.useDevices(devices: devices) }
        page = app.page()
        shell.opened = { [weak self] r in self?.open(r) }
        shell.requested = { [weak self] r in self?.request(r) }
    }

    /// `msg` taken by the app; the page read again.
    func send(_ msg: AccountsMsg) {
        app.send(msg: msg)
        page = app.page()
    }

    /// Nothing left to come (`Runtime::is_idle`), and the number that grows
    /// with every message: the screenshot drivers' ready signal.
    var isIdle: Bool { app.isIdle() }
    var version: UInt64 { app.version() }

    /// The page read again: a view it holds moved.
    func reload() {
        page = app.page()
    }
}

/// The Budget tab, running in Rust (money/app/src/budget): its month the
/// model's, told as it pages.
@MainActor
final class BudgetScreen: ObservableObject {
    @Published private(set) var page: BudgetPage
    /// Opens a route (a category's month) on the tab's back stack, or in the inspector.
    var open: (Route) -> Void = { _ in }

    private let app: BudgetApp
    private let shell: ScreenShell

    init(session: Session, devices: Devices?, month: String, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        self.shell = shell
        app = BudgetApp(session: session, shell: shell)
        if let devices { app.useDevices(devices: devices) }
        app.send(msg: .month(month: month))
        page = app.page()
        shell.opened = { [weak self] r in self?.open(r) }
    }

    /// `msg` taken by the app; the page read again.
    func send(_ msg: BudgetMsg) {
        app.send(msg: msg)
        page = app.page()
    }

    /// Nothing left to come (`Runtime::is_idle`), and the number that grows
    /// with every message: the screenshot drivers' ready signal.
    var isIdle: Bool { app.isIdle() }
    var version: UInt64 { app.version() }

    /// The page read again: a view it holds moved.
    func reload() {
        page = app.page()
    }
}

/// The Transactions tab, running in Rust (money/app/src/transactions): To
/// Review and All, its month and search the model's and the shell's.
@MainActor
final class TransactionsScreen: ObservableObject {
    @Published private(set) var page: TransactionsPage
    /// What only the shell does: a transaction's details or its form.
    var request: (MoneyRequest) -> Void = { _ in }

    private let app: TransactionsApp
    private let shell: ScreenShell

    init(session: Session, devices: Devices?, month: String, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        self.shell = shell
        app = TransactionsApp(session: session, shell: shell)
        if let devices { app.useDevices(devices: devices) }
        app.send(msg: .month(month: month))
        page = app.page()
        shell.requested = { [weak self] r in self?.request(r) }
    }

    func send(_ msg: TransactionsMsg) {
        app.send(msg: msg)
        page = app.page()
    }

    var isIdle: Bool { app.isIdle() }
    var version: UInt64 { app.version() }

    func reload() {
        page = app.page()
    }
}

/// A register, running in Rust (money/app/src/register): an account's
/// transactions or every account's, its search the shell's. One for each
/// register shown: the shell's account, All Accounts pushed.
@MainActor
final class RegisterScreen: ObservableObject {
    @Published private(set) var page: RegisterPage
    /// What only the shell does: a transaction's details, a sheet.
    var request: (MoneyRequest) -> Void = { _ in }

    private let app: RegisterApp
    private let shell: ScreenShell

    init(session: Session, devices: Devices?, account: String?, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        self.shell = shell
        app = RegisterApp(session: session, shell: shell)
        if let devices { app.useDevices(devices: devices) }
        app.send(msg: .show(account: account))
        page = app.page()
        shell.requested = { [weak self] r in self?.request(r) }
    }

    func send(_ msg: RegisterMsg) {
        app.send(msg: msg)
        page = app.page()
    }

    var isIdle: Bool { app.isIdle() }
    var version: UInt64 { app.version() }

    func reload() {
        page = app.page()
    }
}

/// Insights, running in Rust (money/app/src/insights): its month the model's.
@MainActor
final class InsightsScreen: ObservableObject {
    @Published private(set) var page: InsightsPage

    private let app: InsightsApp
    private let shell: ScreenShell

    init(session: Session, month: String, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        self.shell = shell
        app = InsightsApp(session: session, shell: shell)
        app.send(msg: .month(month: month))
        page = app.page()
    }

    func send(_ msg: InsightsMsg) {
        app.send(msg: msg)
        page = app.page()
    }

    var isIdle: Bool { app.isIdle() }
    var version: UInt64 { app.version() }

    func reload() {
        page = app.page()
    }
}

/// The sheets over the app, running in Rust (money/app/src/sheets): a
/// transaction's form and its receipt, a category given, an account
/// reconciled — whoever
/// raised it (`MoneyRequest.sheet`).
@MainActor
final class SheetsScreen: ObservableObject {
    @Published private(set) var page: SheetsPage

    private let app: SheetsApp
    private let shell: ScreenShell

    init(session: Session, devices: Devices?, month: String, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        self.shell = shell
        app = SheetsApp(session: session, shell: shell)
        if let devices { app.useDevices(devices: devices) }
        app.send(msg: .month(month: month, today: today()))
        page = app.page()
    }

    func send(_ msg: SheetsMsg) {
        app.send(msg: msg)
        page = app.page()
    }

    var isIdle: Bool { app.isIdle() }
    var version: UInt64 { app.version() }

    func reload() {
        page = app.page()
    }

    /// The camera or the photo picker answered (a receipt's page): what came of it taken.
    func answered() {
        if app.drain() { page = app.page() }
    }
}

/// money's shell running in Rust (money/app/src/shell): its sections,
/// toolbar, inspector, search and commands, which the generated
/// `MoneyShellView`, `MoneySettingsView` and `MoneyCommands` read and send
/// to. It is the app's for its whole life — the Mac's menu bar is made
/// before any session is open — and runs once one is (`connect`); until
/// then it shows Budget and keeps what it is sent (the launch's place, the
/// month), which it takes as soon as it runs: a remembered session opens
/// after the shell has appeared and sent them. A command only a hosted
/// screen can carry out comes back typed (`request`).
@MainActor
final class MoneyShellScreen: ObservableObject {
    @Published private(set) var page = shellBlank()
    {
        // Where the app is, for a screenshot's ready signal: the section.
        didSet { Ready.shared.at(route: page.section) }
    }
    /// What the hosted screens carry out (`MoneyRequest`): a new
    /// transaction, the month moved, a message for the Budget's runtime…
    var request: (MoneyRequest) -> Void = { _ in }

    private var app: ShellApp? = nil
    private var shell: ScreenShell? = nil
    /// What it was sent before it ran, in order: taken by `connect`.
    private var early: [ShellMsg] = []

    /// Running over `session`.
    func connect(session: Session, refresh: @escaping () -> Void, today: @escaping () -> String) {
        let shell = ScreenShell(refresh: refresh, today: today)
        shell.requested = { [weak self] r in self?.request(r) }
        self.shell = shell
        let app = ShellApp(session: session, shell: shell)
        self.app = app
        let waiting = early
        early = []
        for msg in waiting { app.send(msg: msg) }
        page = app.page()
        afterDraw()
    }

    /// The session closed: back to Budget, taking nothing.
    func disconnect() {
        app = nil
        shell = nil
    }

    /// `msg` taken by the shell; the page read again.
    func send(_ msg: ShellMsg) {
        guard let app else { return early.append(msg) }
        app.send(msg: msg)
        page = app.page()
        afterDraw()
    }

    /// What waits for the page to be drawn (`Command::AfterDraw`: the
    /// inspector put away before a launch moves the section), taken once
    /// SwiftUI has drawn the page just read: a few frames on, as a task on
    /// the main actor may run before the update it follows is committed.
    private func afterDraw() {
        guard let app, app.awaitsDraw() else { return }
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(50))
            guard let self, let app = self.app else { return }
            if app.drawn() { self.page = app.page() }
            self.afterDraw()
        }
    }

    /// Nothing left to come, and the number that grows with every message.
    var isIdle: Bool { app?.isIdle() ?? false }
    var version: UInt64 { app?.version() ?? 0 }
    /// For a screenshot's ready signal (ArkUI's `Ready`): idle, or not
    /// running at all (signed out: nothing of its own to come).
    var quiet: Bool { app == nil || isIdle }

    /// The page read again: a view it holds moved.
    func reload() {
        if let app { page = app.page() }
    }
}

/// What a screen's runtime asks of the shell, on the thread it sends on
/// (the main one: every message comes from a view).
final class ScreenShell: AppShell, @unchecked Sendable {
    var opened: @MainActor (Route) -> Void = { _ in }
    /// Any request but the bank sync: the shell's commands, a reconcile.
    var requested: @MainActor (MoneyRequest) -> Void = { _ in }
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

    func request(request: MoneyRequest) {
        if case .refresh = request {
            refresh()
        } else {
            Task { @MainActor in self.requested(request) }
        }
    }

    func nowMs() -> Int64 { Int64((Date().timeIntervalSince1970 * 1000).rounded()) }

    func today() -> String { day() }
}

