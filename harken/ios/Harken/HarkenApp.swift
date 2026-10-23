// The iOS peer: SwiftUI over harken's peer — the Rust kernel with harken's
// domain linked in (`HarkenFFI.xcframework`, `harken/ffi`), called through
// the generated types (`Gen/Harken.swift`). One `Model` for the app's life;
// every screen reads it. The onboarding until a session is open, then the
// app (Shell.swift); the sign-in's browser sheet run whenever the model asks
// for one; `harken://auth` links handed to the model; and the app coming
// back to the front said, which is when a sign-in that came back empty may
// go again (Onboarding.swift).
import AuthenticationServices
import SwiftUI
import UIKit

@main
struct HarkenApp: App {
    @StateObject private var model: Model
    @Environment(\.scenePhase) private var phase
    @State private var wasAway = false
    private let nowPlaying: NowPlayingHolder

    init() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let m = Model(shelf: DefaultsShelf(), root: support, deviceName: UIDevice.current.name, engine: AVEngine())
        _model = StateObject(wrappedValue: m)
        nowPlaying = NowPlayingHolder(NowPlaying(model: m))
    }

    var body: some Scene {
        WindowGroup {
            Root()
                .environmentObject(model)
                .tint(.gold)
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

/// The lock screen's end, kept for the app's life.
final class NowPlayingHolder {
    let held: NowPlaying
    init(_ n: NowPlaying) { held = n }
}

/// The onboarding's step, drawn; the app once it is running.
struct Root: View {
    @EnvironmentObject private var model: Model
    @Environment(\.webAuthenticationSession) private var web

    var body: some View {
        Group {
            switch model.step {
            case let .server(typed, checking, error): ServerScreen(typed: typed, checking: checking, error: error)
            case let .signIn(server, phase, error): SignInScreen(server: server, phase: phase, error: error)
            case let .opening(server, _): OpeningScreen(server: server)
            case .running: Shell()
            }
        }
        .animation(Springs.fade, value: stepKind)
        // The sign-in, in the system's browser sheet, back on `harken://auth`.
        .task(id: model.browse) {
            guard let url = model.browse else { return }
            do {
                let back = try await web.authenticate(using: url, callbackURLScheme: Onboarding.scheme)
                model.signInReturned(back, error: nil)
            } catch let e as ASWebAuthenticationSessionError where e.code == .canceledLogin {
                model.signInReturned(nil, error: "The sign-in was closed before it finished.")
            } catch {
                model.signInReturned(nil, error: "Signing in didn't finish: \(error.localizedDescription)")
            }
        }
    }

    private var stepKind: Int {
        switch model.step {
        case .server: return 0
        case .signIn: return 1
        case .opening: return 2
        case .running: return 3
        }
    }
}
