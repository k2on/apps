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
/// (the person can turn it into the tab bar) and on the Mac.
public struct AdaptiveTabs<Tabs: TabContent<String>>: View {
    let selection: String
    let onSelect: (String) -> Void
    let tabs: Tabs

    public init(selection: String, onSelect: @escaping (String) -> Void, @TabContentBuilder<String> _ tabs: () -> Tabs) {
        self.selection = selection
        self.onSelect = onSelect
        self.tabs = tabs()
    }

    public var body: some View {
        TabView(selection: Binding(get: { selection }, set: onSelect)) { tabs }
            .tabViewStyle(.sidebarAdaptable)
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

    public var body: some View {
        content
            .navigationTitle(title)
            .toolbar { items }
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
        content.inspector(isPresented: Binding(get: { isShown }, set: onToggle)) {
            inspector
                .navigationTitle(title)
                .inspectorColumnWidth(ideal: LayoutTokens.inspectorWidth)
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

/// The Settings scene's panes: a window of tabs on the Mac, a list of
/// screens on the iPhone and the iPad.
public struct SettingsPanes<Panes: View>: View {
    let panes: Panes

    public init(@ViewBuilder _ panes: () -> Panes) { self.panes = panes() }

    public var body: some View {
        #if os(macOS)
        TabView { panes }
            .frame(minWidth: 480, minHeight: 320)
        #else
        NavigationStack {
            List { panes }
                .navigationTitle("Settings")
        }
        #endif
    }
}

/// One pane of Settings: a tab on the Mac, a row that opens it elsewhere.
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
