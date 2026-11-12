// What every Ark app on the iPhone, the iPad and the Mac is underneath its
// screens, written once: the app host (ark-ffi's `ArkHost`, which runs the
// first run every app shares, opens the session over the process's one
// runtime and watches its link), held for the app's life and attached to
// the process's runtime while the app runs; the browser sign-in in the
// system's shared session (`ASWebAuthenticationSession`); the app's links;
// the app coming back to the front; the camera's "Scan a Code"; the notice
// said once; and the screenshots' ready signal.
//
// An app's part is generated (ui/prebuild.py, from app.toml), and tiny: a
// `@main` naming the app's library (`MoneyHost(launch:store:)`, with the
// secure store), its accent, and the scene it draws; the holders of its
// screens (`ArkScreen`). It replaces each app's own model object owning the
// sign-in machine, a session and a snapshot (money's `Model.swift` and
// `Bridge.swift`): the state is Rust's.
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
import ArkUI
#if canImport(AuthenticationServices)
import AuthenticationServices
#endif
import SwiftUI

/// An app's library, as the app holds it: the host every app has, and its
/// first run's pages. Every app's bridge exports one (`MoneyHost`).
public protocol ArkAppHost: AnyObject {
    func host() -> ArkHost
    func welcome() -> WelcomeApp
}

extension Launch {
    /// How this launch reads: the app's directory (Application Support),
    /// its version, `-demo YES` (or a day, `-demo 2026-11-18`: the demo's
    /// household for this launch, whatever sign-in is remembered — the
    /// screenshots'), `-shots YES`, and its arguments.
    public static func current(scan: Bool) -> Launch {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let demo = UserDefaults.standard.string(forKey: "demo")
        let i = Bundle.main.infoDictionary
        let version = "\(i?["CFBundleShortVersionString"] as? String ?? "?") (\(i?["CFBundleVersion"] as? String ?? "?"))"
        return Launch(root: support.path, version: version, demo: demo != nil, demoDay: demo.flatMap { $0.count == 10 ? $0 : nil },
                      shots: Shots.on, scan: scan, args: ProcessInfo.processInfo.arguments)
    }
}

/// The app host as SwiftUI reads it, for the app's life.
@MainActor
public final class ArkApp<Library: ArkAppHost>: ObservableObject {
    /// The app's library (`MoneyHost`): its screens' runtimes, its own asks.
    public let library: Library
    public let host: ArkHost
    /// The first run's pages, as the generated `WelcomeView` reads them.
    public let welcome: ArkScreen<WelcomeApp>
    /// Where the first run stands.
    @Published public private(set) var step: Step
    /// The app itself shows: a session or the demo, no page of the first
    /// run over it.
    @Published public private(set) var showsApp = false
    /// "Scan a Code" asked: the camera's code scanner up.
    @Published public var scanning = false
    /// What the app says once: a refusal, the bank's news.
    @Published public var notice: String? = nil
    /// The sign-in to open in the browser's sheet, once.
    @Published public var browse: URL? = nil
    /// Grows whenever anything the app shows moved: its screens read again.
    @Published public private(set) var moves: UInt64 = 0

    private let teller = Teller()
    /// This app's UI attached to the process's runtime: online while it runs.
    private let attached = ArkProcess.shared().attach()

    public init(_ library: Library) {
        self.library = library
        host = library.host()
        welcome = ArkScreen(library.welcome())
        step = host.step()
        teller.app = { [weak self] in self?.refresh() }
        teller.browse = { [weak self] url in self?.browse = URL(string: url) }
        // A screenshot's: ready once nothing is on its way, at the version drawn.
        var drawn: UInt64 = 0
        let host = host
        Ready.shared.probe {
            let now = host.version()
            if now != drawn {
                drawn = now
                Ready.shared.changed()
                return false
            }
            return host.idle()
        }
        host.start(shell: teller)
    }

    private func refresh() {
        step = host.step()
        showsApp = host.showsApp()
        if host.scanning() { scanning = true }
        if let n = host.takeNotice() { notice = n }
        welcome.reload()
        moves &+= 1
        Ready.shared.changed()
    }

    /// A link the system opened the app with.
    public func open(url: URL) {
        Ready.shared.changed()
        host.openUrl(url: url.absoluteString)
    }

    /// What the browser's sign-in came back with, or why not (none: closed).
    public func returned(_ url: URL?, error: String?) {
        browse = nil
        host.returned(url: url?.absoluteString, error: error)
    }

    /// What the code scanner read (none: closed).
    public func scanned(_ text: String?) {
        scanning = false
        host.scanned(text: text)
    }

    /// The app's own content is in the window: what the launch's arguments
    /// open (a sheet, Settings) is opened now. Before the Mac's main window
    /// was on the screen, a sheet or Settings window asked for kept it from
    /// ever showing (app-screens 43159033586).
    public func shown() { host.shown() }

    /// Back in front after it was away.
    public func foreground() { host.foreground() }

    /// Signed out, or out of the demo.
    public func signOut() { host.signOut() }

    /// "Add a Device": a join link another device signs in with, made as
    /// this login at its server, off the main thread; or why not.
    public func deviceLink() async -> DeviceLinkShown {
        let host = host
        return await Task.detached {
            do {
                let d = try host.deviceLink()
                return DeviceLinkShown(link: d.link, size: Int(d.qr.size), dark: d.qr.dark, error: nil)
            } catch let Refusal.Refused(why) {
                return DeviceLinkShown(link: nil, size: 0, dark: [], error: why)
            } catch {
                return DeviceLinkShown(link: nil, size: 0, dark: [], error: "\(error)")
            }
        }.value
    }

    /// The pages of what the app is show over it (their route asked).
    public var introShown: Bool { welcome.page.intro != nil }
}

/// What the host tells the app, from any thread: on the main one.
private final class Teller: HostShell, @unchecked Sendable {
    var app: @MainActor () -> Void = {}
    var browse: @MainActor (String) -> Void = { _ in }
    private var queued = false
    private let lock = NSLock()

    func changed() {
        lock.lock()
        let first = !queued
        queued = true
        lock.unlock()
        guard first else { return }
        DispatchQueue.main.async {
            self.lock.lock()
            self.queued = false
            self.lock.unlock()
            MainActor.assumeIsolated { self.app() }
        }
    }

    func browse(url: String) {
        DispatchQueue.main.async { MainActor.assumeIsolated { self.browse(url) } }
    }
}

/// The first run's pages until the app itself shows, the app after; the
/// sign-in's sheet, the code scanner, the app's links and its phase, and
/// what it says once: every Ark app's root.
public struct ArkRoot<Library: ArkAppHost, Welcome: View, Content: View>: View {
    @ObservedObject private var app: ArkApp<Library>
    private let welcome: () -> Welcome
    private let content: () -> Content
    @Environment(\.scenePhase) private var phase
    #if canImport(AuthenticationServices)
    @Environment(\.webAuthenticationSession) private var web
    #endif
    @State private var away = false

    public init(_ app: ArkApp<Library>, @ViewBuilder welcome: @escaping () -> Welcome, @ViewBuilder content: @escaping () -> Content) {
        self.app = app
        self.welcome = welcome
        self.content = content
    }

    public var body: some View {
        SwiftUI.Group {
            if app.showsApp && !app.introShown {
                content()
                    // The launch's screen opened over it from now, as each
                    // app's own shell sent the launch once its tabs appeared.
                    .onAppear { app.shown() }
            } else {
                welcome().environmentObject(app.welcome)
            }
        }
        .animation(Springs.fade, value: app.showsApp)
        // "Scan a Code": another device's join code, asked about before it is used.
        .sheet(isPresented: $app.scanning, onDismiss: { if app.scanning { app.scanned(nil) } }) {
            QrScanner { text in app.scanned(text) }.ignoresSafeArea()
        }
        .toast($app.notice)
        .onOpenURL { app.open(url: $0) }
        .onChange(of: phase) { _, now in
            if now == .background { away = true }
            if now == .active && away {
                away = false
                app.foreground()
            }
        }
        // The sign-in, in the system's browser sheet, back on the app's scheme.
        .task(id: app.browse) {
            #if canImport(AuthenticationServices)
            guard let url = app.browse else { return }
            let scheme = ArkRootScheme.of(url)
            do {
                // The shared session, never an ephemeral one: the provider's cookie is
                // what makes signing in to the next app one tap.
                let back = try await web.authenticate(using: url, callbackURLScheme: scheme, preferredBrowserSession: .shared)
                app.returned(back, error: nil)
            } catch let e as ASWebAuthenticationSessionError where e.code == .canceledLogin {
                app.returned(nil, error: nil)
            } catch {
                app.returned(nil, error: error.localizedDescription)
            }
            #endif
        }
    }
}

/// The scheme a sign-in comes back on: its `redirect`'s, `<scheme>://auth`.
enum ArkRootScheme {
    static func of(_ url: URL) -> String {
        let redirect = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "redirect" }?.value ?? ""
        return redirect.components(separatedBy: "://").first ?? ""
    }
}
