// money's scenes over the app library (ui/swift's ArkApp): the generated
// `@main` (ui/prebuild.py, from app.toml) holds money's library
// (`MoneyHost`) and draws these. The window is the first run every Ark app
// shares until a session (or the demo) is open, then money's shell
// (Shell.swift); the Mac's menu bar is the shell's commands and its Settings
// window the shell's panes, written from Rust. What money's own screens
// read is `MoneyScreens`: the holders of the runtimes money's host made over
// the session, read again whenever the app says something moved, and what
// the host asks of this shell (a page pushed, Settings, Add a Device).
import ArkUI
import Combine
import SwiftUI

struct MoneyScenes: Scene {
    @ObservedObject var app: ArkApp<MoneyHost>
    @StateObject private var screens: MoneyScreens

    init(app: ArkApp<MoneyHost>) {
        self.app = app
        _screens = StateObject(wrappedValue: MoneyScreens(app))
    }

    var body: some Scene {
        WindowGroup {
            ArkRoot(app) { WelcomeView() } content: { Shell() }
                .environmentObject(app)
                .environmentObject(screens)
                .tint(Color.accent)
                // The derived square in the green (ArkUI's are neutral).
                .environment(\.squares, .green)
                // A screenshot's motion and ready signal (ArkUI's `Shots`).
                .arkShots()
        }
        // The size the launch asks for (the Mac's screenshots: 1440×900).
        .launchWindowSize()
        // The menu bar's commands, written from Rust (Generated/MoneyCommands.swift):
        // the Mac's menus, the iPad's hardware keyboard.
        .commands { MoneyCommands(screen: screens.shell) }
        // Settings: the Mac's Settings window (⌘,), written from Rust
        // (Generated/MoneySettingsView.swift); on the iPhone and the iPad,
        // the account sheet's Settings.
        ArkSettings {
            MoneySettingsView()
                .moneySettings(app, screens)
                .tint(Color.accent)
        }
    }
}

/// money's screens as SwiftUI holds them: the runtimes its host made over
/// the session (`MoneyHost`), each in the app library's `ArkScreen`, made
/// again with each session and read again whenever anything moved.
@MainActor
final class MoneyScreens: ObservableObject {
    let app: ArkApp<MoneyHost>
    /// The shell's, for the app's whole life: the Mac's menu bar is made
    /// before any session is open; it shows its blank page until one runs.
    let shell: MoneyShellScreen
    @Published private(set) var accounts: AccountsScreen? = nil
    @Published private(set) var budget: BudgetScreen? = nil
    @Published private(set) var transactions: TransactionsScreen? = nil
    @Published private(set) var insights: InsightsScreen? = nil
    @Published private(set) var sheets: SheetsScreen? = nil
    /// Settings asked for (the Mac's window, a sheet elsewhere).
    @Published var settings = false
    /// Add a Device's page up, as Settings asked.
    @Published var addingDevice = false
    /// Grows with every reading: a register reads again on it.
    @Published private(set) var moves: UInt64 = 0
    /// A page the host asked to push on the section's stack, and the stacks
    /// put away: Shell.swift's.
    var push: (Route) -> Void = { _ in }
    var reset: () -> Void = {}

    private var session: UInt64 = 0
    private var watching: AnyCancellable?

    init(_ app: ArkApp<MoneyHost>) {
        self.app = app
        shell = MoneyShellScreen(app.library.shell())
        Numbers.install()
        watching = app.$moves.sink { [weak self] _ in
            // After the change is published: read on the next turn.
            DispatchQueue.main.async { self?.moved() }
        }
    }

    /// What moved, read again: the holders made again for a new session.
    func moved() {
        let lib = app.library
        let now = lib.generation()
        if now != session {
            session = now
            shell.attach(lib.shell())
            let open = now != 0
            accounts = open ? AccountsScreen(lib.accounts()) : nil
            budget = open ? BudgetScreen(lib.budget()) : nil
            transactions = open ? TransactionsScreen(lib.transactions()) : nil
            insights = open ? InsightsScreen(lib.insights()) : nil
            sheets = open ? SheetsScreen(lib.sheets()) : nil
        }
        accounts?.reload()
        budget?.reload()
        transactions?.reload()
        insights?.reload()
        sheets?.reload()
        shell.reload()
        moves &+= 1
        // Where the app is, for a screenshot's ready signal: the section.
        Ready.shared.at(route: shell.page.section)
        for native in lib.natives() {
            switch native {
            case let .open(route): push(route)
            case .settings: settings = true
            case .addDevice: addingDevice = true
            case .reset:
                settings = false
                reset()
            // The app library signs out itself: never handed to a shell.
            case .signOut: break
            }
        }
    }

    /// A register's runtime, for as long as it shows: the account of this
    /// hex id's, or every account's for none.
    func register(_ account: String?) -> RegisterScreen? {
        app.library.register(first: .show(account: account)).map { RegisterScreen($0) }
    }
}
