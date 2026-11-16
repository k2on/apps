// The shell's parts (ark_app::shell, ui/docs/design.md): each has the name
// and inputs its Rust builder declares (`Part::call`), so a generated
// shell (`MoneyShellView`, `MoneySettingsView`, `MoneyCommands`) is only
// calls to these. A standard multiplatform SwiftUI app's own pieces —
// `TabView` with `.sidebarAdaptable`, a toolbar by placement,
// `.inspector`, `.searchable`, `.confirmationDialog`, the Settings scene's
// tabs, the menu bar's commands — so the iPhone, the iPad and the Mac each
// get theirs from the size class and the platform, here and nowhere in an
// app. What a section hosts is the app's generated screens by name
// (`\.hostedScreens`), set once by the app library's shell (ArkApp's
// `ArkShell`).
#if canImport(SwiftUI)
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

// MARK: what the app provides

private struct HostedScreensKey: EnvironmentKey {
    static let defaultValue: (String) -> AnyView = { _ in AnyView(EmptyView()) }
}

private struct ShellFooterKey: EnvironmentKey {
    static let defaultValue: AnyView? = nil
}

private struct ShellAccountKey: EnvironmentKey {
    static let defaultValue: AnyView? = nil
}

private struct ShownSectionKey: EnvironmentKey {
    static let defaultValue: String? = nil
}

private struct SectionShownKey: EnvironmentKey {
    static let defaultValue = true
}

extension EnvironmentValues {
    /// The screens a shell hosts, by name: the app's generated screens,
    /// each over its own runtime (the app library's `ArkShell` sets it once).
    public var hostedScreens: (String) -> AnyView {
        get { self[HostedScreensKey.self] }
        set { self[HostedScreensKey.self] = newValue }
    }

    /// The sidebar's foot (who is signed in), which a toolbar carries at
    /// its trailing end where there is no sidebar (the iPhone's tab bar).
    var shellFooter: AnyView? {
        get { self[ShellFooterKey.self] }
        set { self[ShellFooterKey.self] = newValue }
    }

    /// Who is signed in (`ToolbarAccount`): every section's toolbar's last
    /// item where there is no sidebar (the iPhone, an iPad's compact
    /// width); the sidebar's foot holds them where there is one.
    var shellAccount: AnyView? {
        get { self[ShellAccountKey.self] }
        set { self[ShellAccountKey.self] = newValue }
    }

    /// The tag of the section `AdaptiveTabs` shows (a child's parent when
    /// compact); nil outside one.
    var shownSection: String? {
        get { self[ShownSectionKey.self] }
        set { self[ShownSectionKey.self] = newValue }
    }

    /// Whether what is under it is in the section shown now: false in a
    /// tab the person is not looking at (`SectionStack`), true outside
    /// the sections.
    var sectionShown: Bool {
        get { self[SectionShownKey.self] }
        set { self[SectionShownKey.self] = newValue }
    }
}

/// A section's navigation stack, its own: what it pushes is its page's
/// (`pushes`, `split_view`: the app's state), so a section switched away
/// from is found as it was left.
struct SectionStack<Content: View>: View {
    let tag: String
    let content: Content
    @Environment(\.shownSection) private var shown

    var body: some View {
        NavigationStack { content }
            .environment(\.sectionShown, shown.map { $0 == tag } ?? true)
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
/// `footer` (who is signed in) at the sidebar's foot, and `account` (an
/// `ToolbarAccount`) the last item of every section's toolbar where there
/// is no sidebar: who is signed in has one place on the screen. A tag
/// `parent/child` is a child of `parent` (an account of Accounts): a place
/// of its own in the sidebar; on a compact width the parent's tab, which
/// pushes it. With an accessory (`bar`, `full`: the now-playing bar and
/// the full player), the bar stands over the tab bar and zooms into the
/// whole screen (`expandingCard`) while `expanded`, there only while
/// `showing`.
public struct AdaptiveTabs<Tabs: TabContent<String>, Footer: View, Account: View, Bar: View, Full: View>: View {
    let selection: String
    let onSelect: (String) -> Void
    let tabs: Tabs
    let footer: Footer
    let account: Account
    let accessory: AccessoryWiring?
    let bar: Bar
    let full: Full
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var size
    #endif

    /// The sections with the now-playing accessory.
    public init(
        selection: String,
        onSelect: @escaping (String) -> Void,
        showing: Bool,
        expanded: Bool,
        onOpen: @escaping () -> Void,
        onClose: @escaping () -> Void,
        onNext: (() -> Void)? = nil,
        onPrevious: (() -> Void)? = nil,
        @TabContentBuilder<String> _ tabs: () -> Tabs,
        @ViewBuilder footer: () -> Footer,
        @ViewBuilder account: () -> Account,
        @ViewBuilder bar: () -> Bar,
        @ViewBuilder full: () -> Full
    ) {
        self.selection = selection
        self.onSelect = onSelect
        self.tabs = tabs()
        self.footer = footer()
        self.account = account()
        self.accessory = AccessoryWiring(showing: showing, expanded: expanded, onOpen: onOpen, onClose: onClose, onNext: onNext, onPrevious: onPrevious)
        self.bar = bar()
        self.full = full()
    }

    /// Whether an account item was given: the toolbars carry it short of a
    /// sidebar, and the footer stays at the sidebar's foot.
    private var hasAccount: Bool { Account.self != EmptyView.self }

    /// Whether a footer was given: where there is a sidebar it is who is
    /// signed in, and the toolbars carry no account item beside it.
    private var hasFooter: Bool { Footer.self != EmptyView.self }

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
            .nowPlaying(accessory, bar: bar, full: full)
            .environment(\.shellFooter, compact && !hasAccount ? AnyView(footer) : nil)
            .environment(\.shellAccount, hasAccount && (compact || !hasFooter) ? AnyView(account) : nil)
            .environment(\.shownSection, shown)
    }
}

extension AdaptiveTabs where Bar == EmptyView, Full == EmptyView {
    /// The sections, with no accessory.
    public init(
        selection: String,
        onSelect: @escaping (String) -> Void,
        @TabContentBuilder<String> _ tabs: () -> Tabs,
        @ViewBuilder footer: () -> Footer,
        @ViewBuilder account: () -> Account
    ) {
        self.selection = selection
        self.onSelect = onSelect
        self.tabs = tabs()
        self.footer = footer()
        self.account = account()
        self.accessory = nil
        self.bar = EmptyView()
        self.full = EmptyView()
    }
}

extension AdaptiveTabs where Account == EmptyView, Bar == EmptyView, Full == EmptyView {
    /// The sections with no account item in their toolbars.
    public init(
        selection: String,
        onSelect: @escaping (String) -> Void,
        @TabContentBuilder<String> _ tabs: () -> Tabs,
        @ViewBuilder footer: () -> Footer
    ) {
        self.init(selection: selection, onSelect: onSelect, tabs, footer: footer, account: { EmptyView() })
    }
}

/// What the now-playing accessory is told and says (`AdaptiveTabs`): whether
/// its bar is there and whether it has grown, and what opening it, putting
/// it back and swiping its bar sideways send.
public struct AccessoryWiring {
    let showing: Bool
    let expanded: Bool
    let onOpen: () -> Void
    let onClose: () -> Void
    let onNext: (() -> Void)?
    let onPrevious: (() -> Void)?
}

extension View {
    /// The now-playing accessory over the sections, when there is one: on
    /// the iPhone and the iPad the tab view's bottom accessory that zooms
    /// into the whole screen (`expandingCard`), its bar skipped by a
    /// sideways swipe; on the Mac the bar at the window's foot, which
    /// raises the full player as a sheet, the system's.
    @ViewBuilder
    func nowPlaying<Bar: View, Full: View>(_ a: AccessoryWiring?, bar: Bar, full: Full) -> some View {
        if let a {
            let open = Binding(get: { a.showing && a.expanded }, set: { $0 ? a.onOpen() : a.onClose() })
            #if os(iOS)
            self.expandingCard(isPresented: open, isEnabled: a.showing) {
                SkippingBar(onNext: a.onNext, onPrevious: a.onPrevious) { bar }
            } expanded: {
                AccessoryFull(onClose: a.onClose) { full }
            }
            #else
            self.safeAreaInset(edge: .bottom) {
                if a.showing {
                    Button(action: a.onOpen) { bar.contentShape(Rectangle()) }
                        .buttonStyle(.pressRow)
                        .padding(Space.s)
                        .background(.bar)
                }
            }
            .sheet(isPresented: open) { AccessoryFull(onClose: a.onClose) { full } }
            #endif
        } else {
            self
        }
    }
}

/// What the accessory grew into: its close, round on glass at the top's
/// leading end (the swipe down that closes it is the system's too), and
/// what it holds under it, at the page's margins, scrolling when it is
/// taller than the screen.
struct AccessoryFull<Content: View>: View {
    let onClose: () -> Void
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Button(action: onClose) { Image(systemName: "chevron.down").frame(width: 24, height: 24) }
                    .buttonStyle(.glass)
                    .buttonBorderShape(.circle)
                    .controlSize(.large)
                    .accessibilityLabel("Close the player")
                Spacer()
            }
            .padding(.horizontal, Space.page)
            .padding(.vertical, Space.s)
            ScrollView {
                content()
                    .padding(.horizontal, Space.page)
                    .padding(.bottom, Space.xl)
            }
        }
    }
}

#if os(iOS)
/// The bar, skipped by a sideways drag: it follows the finger one to one
/// once the drag is more across than down; let go, where it was going —
/// how far it was pulled and its speed projected at the snappy rate
/// (`Momentum.snappy`), since it decides an act — past `MotionTokens.skip`
/// of the bar's width, or as fast as a flick, sends `onNext` (to the left)
/// or `onPrevious` (to the right); then it springs back from the finger's
/// speed (`Springs.released`), the momentum's overshoot only when it was
/// flicked, a fade's settle when motion is reduced. Without either, the
/// bar alone.
struct SkippingBar<Content: View>: View {
    let onNext: (() -> Void)?
    let onPrevious: (() -> Void)?
    @ViewBuilder let content: () -> Content
    @State private var swipe: CGFloat = 0
    @Environment(\.reducesMotion) private var still

    var body: some View {
        if let onNext, let onPrevious {
            GeometryReader { geo in
                content()
                    .offset(x: swipe)
                    .frame(width: geo.size.width, height: geo.size.height)
                    .contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 12).onChanged { v in
                        if abs(v.translation.width) > abs(v.translation.height) { swipe = v.translation.width }
                    }.onEnded { v in
                        let wide = CGFloat(MotionTokens.skip) * geo.size.width
                        let rest = swipe + CGFloat(Momentum.project(Double(v.velocity.width), rate: Momentum.snappy))
                        if rest < -wide || v.velocity.width < -Springs.thrown { onNext() }
                        else if rest > wide || v.velocity.width > Springs.thrown { onPrevious() }
                        let back = still ? Springs.calm : Springs.released(velocity: Double(v.velocity.width), from: Double(swipe), to: 0)
                        withAnimation(back) { swipe = 0 }
                    })
            }
        } else {
            content()
        }
    }
}
#endif

/// Who is signed in, a toolbar's last item (`AdaptiveTabs`' `account`): their
/// picture (`picture`, a whole URL), or their initial on the accent while
/// there is none (`Face`, through the shared URL cache), opening their
/// account (`onTap`: money's Settings › General). VoiceOver hears "Account"
/// and their name.
public struct ToolbarAccount: View {
    let name: String
    let picture: String
    let onTap: () -> Void

    public init(name: String, picture: String, onTap: @escaping () -> Void) {
        self.name = name
        self.picture = picture
        self.onTap = onTap
    }

    public var body: some View {
        Button(action: onTap) {
            Face(name: name.isEmpty ? "?" : name, picture: picture, side: ToolbarAccount.side)
        }
        .accessibilityLabel(name.isEmpty ? "Account" : "Account, \(name)")
    }

    /// The face's side in a toolbar: a bar button's glyph's height.
    public static let side: CGFloat = 28
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
/// sidebar's. On the iPhone, which never shows the sidebar, the section
/// is not there at all: its tabs, hidden from the bar only, still counted
/// toward the five a bar shows, and made a More tab of the rest (the
/// iPhone's tabs are then Android's: the sections alone, each child
/// pushed by its parent).
@MainActor
public func SectionGroup<Tabs: TabContent<String>>(_ title: String, @TabContentBuilder<String> _ tabs: () -> Tabs) -> some TabContent<String> {
    TabSection(title) { tabs() }
        .defaultVisibility(.hidden, for: .tabBar)
        #if os(iOS)
        .tabPlacement(.sidebarOnly)
        .hidden(SidebarOnly.phone)
        #endif
}

#if os(iOS)
/// Whether this is an iPhone: where a sidebar-only section is not there.
@MainActor
enum SidebarOnly {
    static var phone: Bool { UIDevice.current.userInterfaceIdiom == .phone }
}
#endif

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

/// A toolbar with no items of its own (a `toolbar(title, content, ())` in
/// Rust, harken's sections): the generated view writes `items: { }`, which
/// SwiftUI's builder has no block for. An item group of nothing, which
/// draws nothing.
extension ToolbarContentBuilder {
    public static func buildBlock() -> ToolbarItemGroup<EmptyView> {
        ToolbarItemGroup(placement: .automatic) { EmptyView() }
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
    @Environment(\.shellAccount) private var account

    public var body: some View {
        content
            .navigationTitle(title)
            #if os(iOS)
            // Large on the iPad as on the iPhone (the reference's month over
            // its budget); `.automatic` was inline beside the top tab bar.
            .toolbarTitleDisplayMode(.large)
            #endif
            .toolbar {
                items
                // No sidebar to stand at the foot of: who is signed in sits
                // at the toolbar's end, never over its title.
                if let footer {
                    ToolbarItem(placement: .secondaryAction) { footer }
                }
                // Who is signed in, after the section's own items, at every
                // width (the overflow menu and an inspector's toggle stay
                // at the end, as SwiftUI places them; Android's bar the same).
                if let account {
                    ToolbarItem(placement: .primaryAction) { account }
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
    /// Whether its section is the one shown: an inspector is the shown
    /// section's alone. On the iPhone it is a sheet, and every section's
    /// asked for it at once while it was open: one in a tab not shown,
    /// made already, came up over that tab when it was next shown, and
    /// told the app it was open (app-screens 43467277726: the dark walk's
    /// Transactions had "Nothing Selected" over it, and a row then opened
    /// the inspector, not its form).
    @Environment(\.sectionShown) private var sectionShown

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
            .inspector(isPresented: Binding(get: { isShown && sectionShown }, set: { if sectionShown { onToggle($0) } })) {
                // Its title is a heading inside it, never a navigation
                // title: a stack nested in the section's is folded into it,
                // and its title then named the page (an account's register
                // read "Transaction Details" on the iPhone, the iPad's
                // Budget "Category Details" with nothing inspected).
                VStack(alignment: .leading, spacing: 0) {
                    Text(title)
                        .font(.headline)
                        .accessibilityAddTraits(.isHeader)
                        .padding(.horizontal, Space.l)
                        .padding(.vertical, Space.s)
                    inspector.frame(maxHeight: .infinity)
                }
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
        // A stack of its own, so a pane's page can push another (a
        // connected bank's, from Connected Banks) as it does elsewhere.
        NavigationStack { content }
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
