// The shell every Ark app on the iPhone, the iPad and the Mac shows once it
// runs, written once (ui/docs/app.md, "The App Library"): the app's shell
// screen's generated view (its sections, toolbars, search, inspectors —
// ArkUI's `AdaptiveTabs` and the rest) hosting the app's other screens'
// generated views by name, each over its own runtime; what goes over every
// section (the app's sheets); Settings (the Mac's window, a sheet elsewhere)
// and Add a Device; who is signed in at the sidebar's foot and their
// account sheet; a page the app opens in Safari's view; the launch's or
// Settings' Light or Dark on every window; and what the app's host asks of
// its shell (`ArkHost.natives()`). An app adds only its names, generated
// from app.toml's `[shell]` (ui/prebuild.py writes `Generated/ArkShell.swift`:
// the holders, which name hosts which view, the scenes).
//
// Each screen's view observes its own holder, and its holder publishes only
// when its page differs (`ArkScreen.reload`); a hosted view observes only
// its slot, which publishes only when a session makes its holder again; and
// the root observes only what it draws itself (Settings up, Add a Device,
// the theme, the page in Safari), each set only when it changed. What the
// shell hands down (`hostedScreens`) is made once, so nothing under it is
// drawn again because the root was (money's own shell drew every hosted
// view again on every change: app-screens 43486700398, about a second a
// step on the iPhone and the iPad).
#if canImport(Combine)
import Combine
#endif
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
import ArkUI
import SwiftUI

/// What is read again whenever the app moved: a screen's holder.
@MainActor
public protocol ArkReloading: AnyObject {
    func reload()
}

extension ArkScreen: ArkReloading {}

/// A slot the shell fills with each session (`ArkSlot`).
@MainActor
protocol ArkSlotted: AnyObject {
    func open(_ open: Bool)
    func reload()
}

/// One screen's holder for the session open, made again with each session
/// and let go while none is: what a hosted view observes, and only for that.
@MainActor
public final class ArkSlot<Holder: ArkReloading>: ObservableObject, ArkSlotted {
    @Published public private(set) var holder: Holder?
    private let make: () -> Holder

    init(_ make: @escaping () -> Holder) { self.make = make }

    func open(_ open: Bool) { holder = open ? make() : nil }

    func reload() { holder?.reload() }
}

/// What the shared shell reads of the app's shell page: each a field
/// app.toml's `[shell]` names, written by ui/prebuild.py.
public struct ArkShellPage<Shell: ArkRuntime> {
    /// The section showing (`budget`, `accounts/<id>`): the ready signal's route.
    let section: (Shell.Page) -> String
    /// Dark, light, or the device's (none).
    let dark: (Shell.Page) -> Bool?
    /// Who is signed in, for the account face and its sheet.
    let household: (Shell.Page) -> Household
    /// A page to show in Safari's view while it is set, and the message that
    /// tells the shell it was closed.
    let browse: (Shell.Page) -> String?
    let closed: (String) -> Shell.Msg

    public init(section: @escaping (Shell.Page) -> String, dark: @escaping (Shell.Page) -> Bool?, household: @escaping (Shell.Page) -> Household,
                browse: @escaping (Shell.Page) -> String?, closed: @escaping (String) -> Shell.Msg) {
        self.section = section
        self.dark = dark
        self.household = household
        self.browse = browse
        self.closed = closed
    }
}

/// The shell over an app's library, for the app's life: its shell screen's
/// holder, every other screen's slot, and what the root draws.
@MainActor
public final class ArkShell<Library: ArkAppHost, Shell: ArkRuntime>: ObservableObject {
    public let app: ArkApp<Library>
    /// The shell's holder, for the app's whole life: the Mac's menu bar is
    /// made before any session is open; it shows its blank page until one runs.
    public let screen: ArkScreen<Shell>
    /// Settings asked for (the Mac's window, a sheet elsewhere).
    @Published public var settings = false
    /// Add a Device's page up, as Settings asked.
    @Published public var addingDevice = false
    /// The theme every window draws in: dark, light, or the device's.
    @Published public private(set) var dark: Bool? = nil
    /// The page shown in Safari's view (a bank's sign-in), while there is one.
    @Published public private(set) var browse: String? = nil
    /// The screens the shell hosts by name, made once (ArkUI's `\.hostedScreens`).
    public internal(set) var hosted: (String) -> AnyView = { _ in AnyView(EmptyView()) }
    /// The hosted name drawn as who is signed in (app.toml's `[shell] account`).
    public let account: String

    let page: ArkShellPage<Shell>
    private let make: (Library) -> Shell
    private var slots: [ArkSlotted] = []
    private var session: UInt64 = 0
    private var watching: AnyCancellable?

    public init(_ app: ArkApp<Library>, shell make: @escaping (Library) -> Shell, account: String, page: ArkShellPage<Shell>) {
        self.app = app
        self.make = make
        self.account = account
        self.page = page
        screen = ArkScreen(make(app.library))
        watching = app.moved.sink { [weak self] _ in
            // Read on the next turn, after the app's own reading.
            DispatchQueue.main.async { self?.moved() }
        }
    }

    /// A screen's slot, its holder made by `make` with each session.
    public func slot<Holder: ArkReloading>(_ make: @escaping (Library) -> Holder) -> ArkSlot<Holder> {
        let library = app.library
        let s = ArkSlot { make(library) }
        slots.append(s)
        return s
    }

    /// The screens the shell hosts by name: set once, as the app's
    /// generated holders are made.
    public func host(_ hosted: @escaping (String) -> AnyView) {
        self.hosted = hosted
    }

    /// What moved, read again: the holders made again for a new session,
    /// what the host asks of the shell carried out.
    func moved() {
        let began = ProcessInfo.processInfo.systemUptime
        defer { Ready.shared.spent("the shell read again", since: began) }
        let host = app.host
        let now = host.generation()
        if now != session {
            session = now
            screen.attach(make(app.library))
            for s in slots { s.open(now != 0) }
        }
        for s in slots { s.reload() }
        screen.reload()
        let p = screen.page
        // Where the app is, for a screenshot's ready signal: the section.
        Ready.shared.at(route: page.section(p))
        let dark = page.dark(p)
        if dark != self.dark { self.dark = dark }
        let browse = page.browse(p)
        if browse != self.browse { self.browse = browse }
        for ask in host.natives() {
            switch ask {
            case .settings:
                if !settings { settings = true }
            case .addDevice:
                if !addingDevice { addingDevice = true }
            case .reset:
                if settings { settings = false }
                if addingDevice { addingDevice = false }
            // The app host signs out itself: never handed to a shell.
            case .signOut: break
            }
        }
    }
}

/// A screen the shell hosts: its generated view over its slot's holder,
/// while a session is open.
public struct ArkHosted<Holder: ArkReloading & ObservableObject, Content: View>: View {
    @ObservedObject private var slot: ArkSlot<Holder>
    private let content: () -> Content

    public init(_ slot: ArkSlot<Holder>, @ViewBuilder _ content: @escaping () -> Content) {
        self.slot = slot
        self.content = content
    }

    public var body: some View {
        if let h = slot.holder { content().environmentObject(h) }
    }
}

/// A name the shell hosts that is no screen's: who is signed in (the
/// account's name), nothing in Settings' General (the desktop adds there),
/// else an inspector with nothing chosen.
public struct ArkShellPlace<Library: ArkAppHost, Shell: ArkRuntime>: View {
    let shell: ArkShell<Library, Shell>
    let name: String

    public init(_ shell: ArkShell<Library, Shell>, name: String) {
        self.shell = shell
        self.name = name
    }

    public var body: some View {
        if name == shell.account {
            ArkAccountButton(shell: shell, screen: shell.screen)
        } else {
            ContentUnavailableView("Nothing Selected", systemImage: "sidebar.trailing", description: Text("Select a row to see its details here."))
        }
    }
}

/// The app once it runs: its shell screen's view (`main`), what goes over
/// every section (`over`: its sheets), Settings, Add a Device, a page in
/// Safari's view, and the theme on every window.
public struct ArkShellView<Library: ArkAppHost, Shell: ArkRuntime, Main: View, Over: View, Settings: View>: View {
    @ObservedObject private var shell: ArkShell<Library, Shell>
    private let main: Main
    private let over: Over
    private let settingsView: () -> Settings

    public init(_ shell: ArkShell<Library, Shell>, @ViewBuilder main: () -> Main, @ViewBuilder over: () -> Over,
                @ViewBuilder settings: @escaping () -> Settings) {
        self.shell = shell
        self.main = main()
        self.over = over()
        settingsView = settings
    }

    public var body: some View {
        main
            .environmentObject(shell.screen)
            .environment(\.hostedScreens, shell.hosted)
            // The sheets any screen raises.
            .background { over }
            // The launch's `-appearance` or Settings' Light or Dark, over the
            // device's (none): every window's, bars and glass drawn again
            // (ArkUI's `arkAppearance`).
            .arkAppearance(shell.dark)
            .settingsShown($shell.settings) {
                settingsView().arkSettings(shell)
            }
            // A page the app opens (a bank's sign-in), as soon as its server
            // answers with it; closed by its Done, or once the app is done
            // with it.
            .safari(shell.browse.flatMap { URL(string: $0) }, tint: Color.accent) {
                if let url = shell.browse { shell.screen.send(shell.page.closed(url)) }
            }
    }
}

/// Settings over the shell's screen: nothing hosted in it (only the
/// desktop adds to General), and Add a Device's page when it is asked.
private struct ArkSettingsFrame<Library: ArkAppHost, Shell: ArkRuntime>: ViewModifier {
    @ObservedObject var shell: ArkShell<Library, Shell>

    func body(content: Content) -> some View {
        content
            .environmentObject(shell.screen)
            .environment(\.hostedScreens, ArkSettingsFrame.nothing)
            .sheet(isPresented: $shell.addingDevice) {
                let app = shell.app
                NavigationStack {
                    AddDeviceView { await app.deviceLink() }
                        .toolbar { Button("Done") { shell.addingDevice = false } }
                }
            }
    }

    private static var nothing: (String) -> AnyView { { _ in AnyView(EmptyView()) } }
}

extension View {
    /// The app's Settings view as the shell shows it, in its own scene or a sheet.
    public func arkSettings<Library: ArkAppHost, Shell: ArkRuntime>(_ shell: ArkShell<Library, Shell>) -> some View {
        modifier(ArkSettingsFrame(shell: shell))
    }

    /// Every Ark app's window: its accent, its derived squares, and a
    /// screenshot's motion and ready signal (ArkUI's `Shots`).
    public func arkWindow(squares: Squares) -> some View {
        tint(Color.accent)
            .environment(\.squares, squares)
            .arkShots()
    }
}

/// Who is using the app, at the sidebar's foot (ArkUI's `AccountFace`): their
/// face, marked while something wants somebody in Settings, raising the
/// account sheet — who and where they are, what wants a look, Settings, and
/// the way out. What its rows ask for is done once the sheet has gone.
struct ArkAccountButton<Library: ArkAppHost, Shell: ArkRuntime>: View {
    let shell: ArkShell<Library, Shell>
    @ObservedObject var screen: ArkScreen<Shell>
    @State private var showing = false
    /// What the sheet's row asked for, done once the sheet has gone.
    @State private var then: Then? = nil

    private enum Then { case settings, signOut }

    var body: some View {
        let h = shell.page.household(screen.page)
        let wanting = Int(h.wanting)
        AccountFace(name: h.who, picture: h.picture, attention: wanting > 0 ? h.wantingSaid : nil) { showing = true }
            .sheet(isPresented: $showing, onDismiss: after) {
                AccountSheet(name: h.who, picture: h.picture, detail: h.detail, server: h.server, linked: h.linked,
                             signOut: h.demo ? "Leave the Demo" : "Sign Out",
                             confirm: h.demo ? "Leave the Demo?" : "Sign Out?",
                             onSettings: { leave(.settings) },
                             onSignOut: { leave(.signOut) }) {
                    if wanting > 0 {
                        Section {
                            Button { leave(.settings) } label: {
                                HStack {
                                    Label {
                                        Text(h.wantingTitle)
                                    } icon: {
                                        Image(systemName: "exclamationmark.circle.fill").foregroundStyle(Color.error)
                                    }
                                    Spacer()
                                    Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.quiet)
                                }
                                .contentShape(Rectangle())
                            }
                            .foregroundStyle(.ink)
                        }
                    }
                }
                .presentationDetents([.medium, .large])
            }
    }

    private func leave(_ t: Then) {
        then = t
        showing = false
    }

    private func after() {
        let t = then
        then = nil
        switch t {
        case .settings?: shell.settings = true
        case .signOut?: shell.app.signOut()
        case nil: break
        }
    }
}
