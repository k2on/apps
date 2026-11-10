// The shell's parts (ark_app::shell, ui/docs/design.md): each has the name
// and inputs its Rust builder declares (`Part::call`), so a generated
// shell (`MoneyShellView`, `MoneySettingsView`, `MoneyCommands`) is only
// calls to these. A standard multiplatform SwiftUI app's own pieces —
// `TabView` with `.sidebarAdaptable`, a toolbar by placement,
// `.inspector`, `.searchable`, `.confirmationDialog`, the Settings scene's
// tabs, the menu bar's commands — so the iPhone, the iPad and the Mac each
// get theirs from the size class and the platform, here and nowhere in an
// app. What a section hosts is the app's (`\.hostedScreens`), set once at
// its root.
#if canImport(SwiftUI)
import SwiftUI

// MARK: what the app provides

private struct HostedScreensKey: EnvironmentKey {
    static let defaultValue: (String) -> AnyView = { _ in AnyView(EmptyView()) }
}

private struct SectionPathKey: EnvironmentKey {
    static let defaultValue: (String) -> Binding<NavigationPath>? = { _ in nil }
}

private struct ShellFooterKey: EnvironmentKey {
    static let defaultValue: AnyView? = nil
}

extension EnvironmentValues {
    /// The screens a shell hosts, by name: the app's own (hand-written, or
    /// generated apart), until they move into Rust.
    public var hostedScreens: (String) -> AnyView {
        get { self[HostedScreensKey.self] }
        set { self[HostedScreensKey.self] = newValue }
    }

    /// Each section's back stack, by its tag: the app's, so a link can push
    /// onto it and a section switched away from is found as it was left.
    public var sectionPath: (String) -> Binding<NavigationPath>? {
        get { self[SectionPathKey.self] }
        set { self[SectionPathKey.self] = newValue }
    }

    /// The sidebar's foot (who is signed in), which a toolbar carries at
    /// its trailing end where there is no sidebar (the iPhone's tab bar).
    var shellFooter: AnyView? {
        get { self[ShellFooterKey.self] }
        set { self[ShellFooterKey.self] = newValue }
    }
}

/// A section's navigation stack: the app's back stack for its tag, or one of its own.
struct SectionStack<Content: View>: View {
    let tag: String
    let content: Content
    @Environment(\.sectionPath) private var paths
    @State private var own = NavigationPath()

    var body: some View {
        NavigationStack(path: paths(tag) ?? $own) { content }
    }
}

/// A screen the app draws itself, by name.
public struct HostedScreen: View {
    let name: String
    @Environment(\.hostedScreens) private var screens

    public init(_ name: String) { self.name = name }

    public var body: some View { screens(name) }
}

// MARK: the sections

/// The app's sections: a tab bar on the iPhone, a sidebar on the iPad
/// (the person can turn it into the tab bar) and on the Mac, with
/// `footer` (who is signed in) at the sidebar's foot. A tag `parent/child`
/// is a child of `parent` (an account of Accounts): a place of its own in
/// the sidebar; on a compact width the parent's tab, which pushes it.
public struct AdaptiveTabs<Tabs: TabContent<String>, Footer: View>: View {
    let selection: String
    let onSelect: (String) -> Void
    let tabs: Tabs
    let footer: Footer
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var size
    #endif

    public init(
        selection: String,
        onSelect: @escaping (String) -> Void,
        @TabContentBuilder<String> _ tabs: () -> Tabs,
        @ViewBuilder footer: () -> Footer
    ) {
        self.selection = selection
        self.onSelect = onSelect
        self.tabs = tabs()
        self.footer = footer()
    }

    private var compact: Bool {
        #if os(iOS)
        size == .compact
        #else
        false
        #endif
    }

    /// The tab shown: a child's parent when compact.
    private var shown: String {
        guard compact, let slash = selection.firstIndex(of: "/") else { return selection }
        return String(selection[..<slash])
    }

    public var body: some View {
        TabView(selection: Binding(get: { shown }, set: onSelect)) { tabs }
            .tabViewStyle(.sidebarAdaptable)
            .tabViewSidebarBottomBar { footer }
            .environment(\.shellFooter, compact ? AnyView(footer) : nil)
    }
}

/// One section: its title and glyph, its tag, what waits there, what it shows.
public func SectionTab<Content: View>(
    _ title: String,
    glyph: String,
    tag: String,
    badge: Int64? = nil,
    @ViewBuilder _ content: () -> Content
) -> some TabContent<String> {
    Tab(title, systemImage: ArkGlyphs.sf(glyph), value: tag) { SectionStack(tag: tag, content: content()) }
        .badge(Int(badge ?? 0))
}

/// Sections under a heading, in the sidebar alone (Mail's mailboxes): a
/// `TabSection` hidden from the tab bar, each of its tabs only the
/// sidebar's, so a compact tab bar has no More tab for them.
public func SectionGroup<Tabs: TabContent<String>>(_ title: String, @TabContentBuilder<String> _ tabs: () -> Tabs) -> some TabContent<String> {
    TabSection(title) { tabs() }
        .defaultVisibility(.hidden, for: .tabBar)
        #if os(iOS)
        .tabPlacement(.sidebarOnly)
        #endif
}

// MARK: columns

/// A list beside what it opens: columns while there is room (the
/// detail at the content's side, `LayoutTokens.contentWidth` for the
/// list), a stack when compact (the detail pushed while `showing`,
/// back sends `onBack`). Inside the section's one `NavigationStack`
/// (`SectionTab`), so its toolbar and its search are the section's.
public struct SplitColumns<Content: View, Detail: View>: View {
    let showing: Bool
    let onBack: () -> Void
    let content: Content
    let detail: Detail
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var size
    #endif

    public init(showing: Bool, onBack: @escaping () -> Void, @ViewBuilder _ content: () -> Content, @ViewBuilder detail: () -> Detail) {
        self.showing = showing
        self.onBack = onBack
        self.content = content()
        self.detail = detail()
    }

    private var compact: Bool {
        #if os(iOS)
        size == .compact
        #else
        false
        #endif
    }

    public var body: some View {
        if compact {
            content.navigationDestination(isPresented: Binding(get: { showing }, set: { if !$0 { onBack() } })) { detail }
        } else {
            HStack(spacing: 0) {
                content.frame(width: LayoutTokens.contentWidth)
                Divider()
                detail.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }
}

/// What a list opens, pushed onto the section's stack while `showing`, at
/// every width (`navigationDestination(isPresented:)`); back sends `onBack`.
public struct PushedDetail<Content: View, Detail: View>: View {
    let showing: Bool
    let onBack: () -> Void
    let content: Content
    let detail: Detail

    public init(showing: Bool, onBack: @escaping () -> Void, @ViewBuilder _ content: () -> Content, @ViewBuilder detail: () -> Detail) {
        self.showing = showing
        self.onBack = onBack
        self.content = content()
        self.detail = detail()
    }

    public var body: some View {
        content.navigationDestination(isPresented: Binding(get: { showing }, set: { if !$0 { onBack() } })) { detail }
    }
}

// MARK: the toolbar

/// Where a toolbar's button goes.
public enum BarPlacement {
    case navigation, principal, primary, secondary

    var placement: ToolbarItemPlacement {
        switch self {
        case .navigation: return .navigation
        case .principal: return .principal
        case .primary: return .primaryAction
        case .secondary: return .secondaryAction
        }
    }
}

/// A section's title and toolbar over its content (in the section's
/// navigation stack, `SectionTab`'s).
public struct WithToolbar<Content: View, Items: ToolbarContent>: View {
    let title: String
    let content: Content
    let items: Items

    public init(_ title: String, @ViewBuilder _ content: () -> Content, @ToolbarContentBuilder items: () -> Items) {
        self.title = title
        self.content = content()
        self.items = items()
    }

    @Environment(\.shellFooter) private var footer

    public var body: some View {
        content
            .navigationTitle(title)
            #if os(iOS)
            .toolbarTitleDisplayMode(.automatic)
            #endif
            .toolbar {
                items
                // No sidebar to stand at the foot of: who is signed in sits
                // at the toolbar's end, never over its title.
                if let footer {
                    ToolbarItem(placement: .secondaryAction) { footer }
                }
            }
    }
}

/// A toolbar's button: its glyph, its title (said, and shown where the
/// toolbar shows titles).
public struct BarButton: ToolbarContent {
    let title: String
    let glyph: String
    let placement: BarPlacement
    let enabled: Bool
    let onTap: () -> Void

    public init(_ title: String, glyph: String, placement: BarPlacement, enabled: Bool, onTap: @escaping () -> Void) {
        self.title = title
        self.glyph = glyph
        self.placement = placement
        self.enabled = enabled
        self.onTap = onTap
    }

    public var body: some ToolbarContent {
        ToolbarItem(placement: placement.placement) {
            Button(action: onTap) { Label(title, systemImage: ArkGlyphs.sf(glyph)) }
                .disabled(!enabled)
        }
    }
}

// MARK: search and the inspector

/// `content`, searchable: the toolbar's field on the Mac and the iPad, under
/// the title on the iPhone.
public struct WithSearch<Content: View>: View {
    let prompt: String
    let text: String
    let onChange: (String) -> Void
    let content: Content

    public init(_ prompt: String, text: String, onChange: @escaping (String) -> Void, @ViewBuilder _ content: () -> Content) {
        self.prompt = prompt
        self.text = text
        self.onChange = onChange
        self.content = content()
    }

    public var body: some View {
        content.searchable(text: Binding(get: { text }, set: onChange), prompt: prompt)
    }
}

/// `content` with an inspector at its trailing edge: a column on the Mac
/// and the iPad, a sheet on the iPhone (SwiftUI's own adapting).
public struct WithInspector<Content: View, Inspector: View>: View {
    let title: String
    let isShown: Bool
    let onToggle: (Bool) -> Void
    let content: Content
    let inspector: Inspector

    public init(
        _ title: String,
        isShown: Bool,
        onToggle: @escaping (Bool) -> Void,
        @ViewBuilder _ content: () -> Content,
        @ViewBuilder inspector: () -> Inspector
    ) {
        self.title = title
        self.isShown = isShown
        self.onToggle = onToggle
        self.content = content()
        self.inspector = inspector()
    }

    public var body: some View {
        content
            .inspector(isPresented: Binding(get: { isShown }, set: onToggle)) {
                inspector
                    .navigationTitle(title)
                    .inspectorColumnWidth(min: LayoutTokens.inspectorWidth, ideal: LayoutTokens.inspectorWidth + 40)
            }
            // Its toggle at the toolbar's trailing end, as Apple's apps put it.
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button { onToggle(!isShown) } label: { Label(title, systemImage: "sidebar.trailing") }
                        .help("Show or hide \(title.lowercased()) (⌥⌘I)")
                }
            }
    }
}

// MARK: a question and a notice

/// One action of a `ConfirmDialog`.
public struct DialogAction: Identifiable {
    public let title: String
    public let destructive: Bool
    public let onTap: () -> Void
    public var id: String { title }

    public init(_ title: String, destructive: Bool, onTap: @escaping () -> Void) {
        self.title = title
        self.destructive = destructive
        self.onTap = onTap
    }
}

/// A question before something that cannot be undone: present while it is in the view.
public struct ConfirmDialog: View {
    let title: String
    let message: String
    let actions: [DialogAction]
    let onCancel: () -> Void

    public init(_ title: String, message: String, actions: [DialogAction], onCancel: @escaping () -> Void) {
        self.title = title
        self.message = message
        self.actions = actions
        self.onCancel = onCancel
    }

    public var body: some View {
        Color.clear.frame(width: 0, height: 0)
            .confirmationDialog(title, isPresented: Binding(get: { true }, set: { if !$0 { onCancel() } }), titleVisibility: .visible) {
                ForEach(actions) { a in
                    Button(a.title, role: a.destructive ? .destructive : nil, action: a.onTap)
                }
                Button("Cancel", role: .cancel, action: onCancel)
            } message: {
                Text(message)
            }
    }
}

/// Something the person has to know, and OK: present while it is in the view.
public struct NoticeAlert: View {
    let title: String
    let message: String
    let onOk: () -> Void

    public init(_ title: String, message: String, onOk: @escaping () -> Void) {
        self.title = title
        self.message = message
        self.onOk = onOk
    }

    public var body: some View {
        Color.clear.frame(width: 0, height: 0)
            .alert(title, isPresented: Binding(get: { true }, set: { if !$0 { onOk() } })) {
                Button("OK", action: onOk)
            } message: {
                Text(message)
            }
    }
}

// MARK: Settings

/// The Settings scene's panes: a window of tabs on the Mac (the one
/// shown by its title, `selection`), a list of screens on the iPhone and
/// the iPad.
public struct SettingsPanes<Panes: View>: View {
    let selection: String
    let onSelect: (String) -> Void
    let panes: Panes

    public init(selection: String, onSelect: @escaping (String) -> Void, @ViewBuilder _ panes: () -> Panes) {
        self.selection = selection
        self.onSelect = onSelect
        self.panes = panes()
    }

    public var body: some View {
        #if os(macOS)
        TabView(selection: Binding(get: { selection }, set: onSelect)) { panes }
            .frame(minWidth: 520, minHeight: 360)
        #else
        NavigationStack {
            List { panes }
                .navigationTitle("Settings")
        }
        #endif
    }
}

/// One pane of Settings: a tab on the Mac, tagged by its title; a row that
/// opens it elsewhere.
public struct SettingsPane<Content: View>: View {
    let title: String
    let glyph: String
    let content: Content

    public init(_ title: String, glyph: String, @ViewBuilder _ content: () -> Content) {
        self.title = title
        self.glyph = glyph
        self.content = content()
    }

    public var body: some View {
        #if os(macOS)
        content
            .padding()
            .tabItem { Label(title, systemImage: ArkGlyphs.sf(glyph)) }
            .tag(title)
        #else
        NavigationLink {
            content.navigationTitle(title)
        } label: {
            Label(title, systemImage: ArkGlyphs.sf(glyph))
        }
        #endif
    }
}

// MARK: the Settings scene

/// The app's Settings: the Mac's Settings window (⌘,), and a window the
/// iPad can open (`openWindow(id: "settings")`); the iPhone reaches its
/// Settings inside the app.
public struct ArkSettings<Content: View>: Scene {
    let content: () -> Content

    public init(@ViewBuilder _ content: @escaping () -> Content) { self.content = content }

    public var body: some Scene {
        #if os(macOS)
        Settings { content() }
        #else
        WindowGroup(id: "settings") { content() }
        #endif
    }
}

extension View {
    /// The app's Settings shown while `shown` is: the Mac's Settings window
    /// (opened, and `shown` put back), a sheet on the iPhone and the iPad,
    /// as the HIG has Settings in an app that has no window for it.
    public func settingsShown<Content: View>(_ shown: Binding<Bool>, @ViewBuilder content: @escaping () -> Content) -> some View {
        modifier(SettingsShown(shown: shown, content: content))
    }
}

private struct SettingsShown<Panes: View>: ViewModifier {
    @Binding var shown: Bool
    let content: () -> Panes
    #if os(macOS)
    @Environment(\.openSettings) private var openSettings
    #endif

    func body(content view: Content) -> some View {
        #if os(macOS)
        view.onChange(of: shown) {
            if shown {
                openSettings()
                shown = false
            }
        }
        #else
        view.sheet(isPresented: $shown) {
            NavigationStack { content() }
        }
        #endif
    }
}

extension Scene {
    /// The window's first size, as its launch's `-window 1440x900` says
    /// (the screenshots'), else the system's; the iPhone has none to set.
    public func launchWindowSize() -> some Scene {
        let said = UserDefaults.standard.string(forKey: "window")?.split(separator: "x").compactMap { Double($0) } ?? []
        return defaultSize(width: said.count == 2 ? said[0] : 1180, height: said.count == 2 ? said[1] : 760)
    }
}

// MARK: commands

/// Where a menu of commands goes in the menu bar.
public enum MenuPlace {
    case file, view, go
}

/// A menu of the app's commands: File's after New, View's after the
/// sidebar's, Go a menu of its own. On the iPad, the hardware keyboard's
/// shortcuts and the menu its ⌘ held shows.
public struct ArkCommands<Content: View>: Commands {
    let place: MenuPlace
    let content: Content

    public init(_ place: MenuPlace, @ViewBuilder _ content: () -> Content) {
        self.place = place
        self.content = content()
    }

    public var body: some Commands {
        if place == .go {
            CommandMenu("Go") { content }
        } else {
            CommandGroup(after: place == .file ? .newItem : .sidebar) { content }
        }
    }
}

/// One command: its title, its key with ⌘ (and ⇧ or ⌥), what it does.
public struct CommandButton: View {
    let title: String
    let key: String?
    let modifiers: EventModifiers
    let action: () -> Void

    public init(_ title: String, key: String, shift: Bool, option: Bool, action: @escaping () -> Void) {
        self.title = title
        self.key = key
        var m: EventModifiers = .command
        if shift { m.insert(.shift) }
        if option { m.insert(.option) }
        modifiers = m
        self.action = action
    }

    public init(_ title: String, action: @escaping () -> Void) {
        self.title = title
        key = nil
        modifiers = []
        self.action = action
    }

    public var body: some View {
        if let key, let c = key.first {
            Button(title, action: action).keyboardShortcut(KeyEquivalent(c), modifiers: modifiers)
        } else {
            Button(title, action: action)
        }
    }
}
#endif
