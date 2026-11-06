// The iOS app: SwiftUI over money's peer — the Rust kernel with money's
// domain linked in (`MoneyFFI.xcframework`, `money/ffi`), called through the
// generated types (`Gen/Money.swift`). One `Model` for the app's life; every
// screen reads it. The first run until a session (or the demo) is open,
// then the tabs (Shell.swift); the sign-in's browser sheet run whenever the
// model asks for one; `money://auth` links handed to the model; and the app
// coming back to the front said, which dials the server and is when a
// sign-in that came back empty may go again.
import ArkUI
import AuthenticationServices
import SwiftUI

@main
struct MoneyApp: App {
    @StateObject private var model: Model
    /// The views the screens hold (`.holding`), told what moved by the model.
    @State private var watched: Watched<MoneyView>
    @Environment(\.scenePhase) private var phase
    @State private var wasAway = false

    init() {
        // money's accent, the green, before the first view draws it.
        Accent.use(.green)
        // Money written as every client writes it, in the phone's locale.
        Numbers.install()
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let model = Model(shelf: DefaultsShelf(), root: support)
        let watched = Watched<MoneyView> { [weak model] v in model?.hold(v) }
        model.onMoved = { [weak watched] views in watched?.moved(views) }
        _model = StateObject(wrappedValue: model)
        _watched = State(initialValue: watched)
    }

    var body: some Scene {
        WindowGroup {
            Root()
                .environmentObject(model)
                .environment(watched)
                .tint(Color.accent)
                // The derived square in the green (ArkUI's are neutral).
                .environment(\.squares, .green)
                .onOpenURL { model.open(url: $0) }
        }
        .onChange(of: phase) { _, now in
            if now == .background { wasAway = true }
            if now == .active && wasAway {
                wasAway = false
                model.send(.foreground)
            }
        }
    }
}

/// The first run's step, drawn; the app once it is running.
struct Root: View {
    @EnvironmentObject private var model: Model
    @Environment(\.webAuthenticationSession) private var web

    var body: some View {
        SwiftUI.Group {
            switch model.step {
            case let .server(typed, checking, error): ServerScreen(typed: typed, checking: checking, error: error)
            case let .signIn(server, phase, error): SignInScreen(server: server, phase: phase, error: error)
            case let .opening(server, _): OpeningScreen(server: server)
            case let .confirm(_, _, error): ConfirmScreen(error: error)
            case .running, .demo: Shell()
            }
        }
        .animation(Springs.fade, value: stepKind)
        .toast(model.step.isRunning ? .constant(nil) : $model.notice)
        // The sign-in, in the system's browser sheet, back on `money://auth`.
        .task(id: model.browse) {
            guard let url = model.browse else { return }
            do {
                let back = try await web.authenticate(using: url, callbackURLScheme: AppInfo.money.scheme)
                model.signInReturned(back, error: nil)
            } catch let e as ASWebAuthenticationSessionError where e.code == .canceledLogin {
                model.signInReturned(nil, error: nil)
            } catch {
                model.signInReturned(nil, error: error.localizedDescription)
            }
        }
    }

    private var stepKind: Int {
        switch model.step {
        case .server: return 0
        case .signIn: return 1
        case .opening: return 2
        case .running, .demo: return 3
        case .confirm: return 4
        }
    }
}

extension Step {
    /// The tabs are up, and say what the app says themselves.
    var isRunning: Bool {
        switch self {
        case .running, .demo: return true
        default: return false
        }
    }
}
