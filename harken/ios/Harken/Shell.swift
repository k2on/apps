// harken, once the onboarding has handed it a session: the system's tab
// bar — Home, Library and Search, Search in its own place at the bar's end
// (`Tab(role: .search)`) — in Liquid Glass, which shrinks as a list scrolls
// down (`tabBarMinimizeBehavior(.onScrollDown)`); the mini player on glass
// above it (`tabViewBottomAccessory`), which zooms into the full player and
// back (`matchedTransitionSource` / `navigationTransition(.zoom)`), Android's
// `ExpandingCard`; each tab its own `NavigationStack`, so a tab switch is
// instant and finds the tab as it was left, and the tab already shown,
// tapped again, goes back to its root and then its top (the system's own,
// which Android's `Tabs.kt` built by hand); the pages they open zooming out
// of the cover that was tapped; and the sheets a song's menu raises. This
// file wires; what each screen draws is the screen's.
import ArkUI
import SwiftUI

struct Shell: View {
    @EnvironmentObject private var model: Model
    @State private var tab: Pane = .home
    @State private var libraryTab: LibraryTab = .playlists
    /// Each tab's back stack (ArkUI's `Navigator`), of harken's routes.
    @State private var navs: [Pane: Navigator<Route>] = [.home: Navigator(), .library: Navigator(), .search: Navigator()]
    @Namespace private var zoom

    private func path(_ t: Pane) -> Binding<[Route]> {
        Binding(get: { navs[t]?.path ?? [] }, set: { navs[t, default: Navigator()].path = $0 })
    }

    var body: some View {
        TabView(selection: $tab) {
            Tab("Home", systemImage: "house.fill", value: Pane.home) {
                stack(.home) { HomeView() }
            }
            Tab("Library", systemImage: "square.stack.fill", value: Pane.library) {
                stack(.library) { LibraryView(tab: $libraryTab) }
            }
            Tab(value: Pane.search, role: .search) {
                stack(.search) { SearchView() }
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
        .tabViewBottomAccessory {
            MiniPlayer(open: { model.fullPlayer = true })
                .matchedTransitionSource(id: "player", in: zoom)
        }
        .fullScreenCover(isPresented: $model.fullPlayer) {
            // Its own sheets: the tab view under it is already presenting it.
            FullPlayer(close: { model.fullPlayer = false })
                .modifier(SheetHost())
                .environmentObject(model)
                .navigationTransition(.zoom(sourceID: "player", in: zoom))
                .environment(\.open) { r in
                    model.fullPlayer = false
                    open(r)
                }
        }
        .modifier(SheetHost())
        .environment(\.zoom, zoom)
        .environment(\.showTab) { t, lib in
            if let l = lib { libraryTab = l }
            tab = t
        }
        // A link the app was opened with: its route, navigated to on the tab shown.
        .onChange(of: model.link) {
            guard let url = model.link else { return }
            model.link = nil
            navs[tab, default: Navigator()].handle(url, scheme: AppInfo.harken.scheme, parse: { routeFromPath(path: $0) }, take: model.takes)
        }
        .alert(model.notice ?? "", isPresented: Binding(get: { model.notice != nil }, set: { if !$0 { model.notice = nil } })) {
            Button("OK", role: .cancel) { model.notice = nil }
        }
    }

    /// A tab's stack: its root, and every page it can open. The account is
    /// the root's own, at the end of its large title (`largeTitleBar`).
    private func stack<Root: View>(_ t: Pane, @ViewBuilder root: () -> Root) -> some View {
        NavigationStack(path: path(t)) {
            root()
                // What scrolls under the bars, the top's and the tab bar's
                // with the mini player, fades into them softly, the system's
                // own edge, on every scroll view of the tab.
                .scrollEdges()
                .navigationDestination(for: Route.self) { r in
                    Destination(route: r).zoomed(r, zoom).scrollEdges()
                }
        }
        .environment(\.open) { r in navs[t, default: Navigator()].push(r) }
    }

    private func open(_ r: Route) { navs[tab, default: Navigator()].push(r) }

}

/// Where the sheets rise from: whatever under it asks (`\.present`), one at a
/// time — a sheet asking for another hands over to it.
struct SheetHost: ViewModifier {
    @EnvironmentObject private var model: Model
    @State private var sheet: Sheet? = nil

    func body(content: Content) -> some View {
        content
            .environment(\.present) { s in present(s) }
            .sheet(item: $sheet) { s in
                sheetView(s)
                    .scrollEdges()
                    .environmentObject(model)
                    .environment(\.present) { next in present(next) }
            }
    }

    private func present(_ s: Sheet) {
        if sheet == nil {
            sheet = s
            return
        }
        sheet = nil
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { sheet = s }
    }

    @ViewBuilder private func sheetView(_ s: Sheet) -> some View {
        switch s {
        case .addTo(let t): AddToPlaylistSheet(track: t).presentationDetents([.medium, .large])
        case .newPlaylist(let then): NewPlaylist(then: then).presentationDetents([.height(280)])
        case .queue: QueueSheet().presentationDetents([.medium, .large])
        case .devices: DevicesSheet().presentationDetents([.medium, .large])
        case .follow(let kind): FollowSheet(kind: kind).presentationDetents([.medium, .large])
        case .transcript: TranscriptSheet().presentationDetents([.large])
        }
    }
}

/// One route of a tab's back stack, drawn.
struct Destination: View {
    let route: Route
    @EnvironmentObject private var model: Model

    var body: some View {
        switch route {
        case let .album(key, _): AlbumPage(model: model, id: key)
        case let .artist(n): ArtistPage(model: model, name: n)
        case .playlist:
            // By its id (an older link's by its name), so a renamed one keeps
            // its page; one that is gone is the page's own "gone".
            PlaylistPage(model: model, id: model.playlists.first { route.names(id: $0.id.hex, name: $0.name) }?.id ?? Id(String(repeating: "0", count: 32)))
        case let .work(key, _): WorkPage(model: model, id: key)
        case let .recording(key, _): RecordingPage(model: model, id: key)
        case let .show(key, _): ShowPage(model: model, id: key)
        case .library: SongsList().navigationTitle(route.title())
        case .settings: SettingsView()
        case .diagnostics: DiagnosticsView()
        default: EmptyView()
        }
    }
}

/// A page's view, held for as long as the page is on its stack — not only
/// while it is drawn — so coming back to it finds its rows; let go when it is
/// popped. Made once per page by the page's `@StateObject`.
@MainActor
final class PageHold: ObservableObject {
    private let release: () -> Void

    init(_ model: Model, _ page: Page) {
        model.hold(page)
        release = { [weak model] in model?.release(page) }
    }

    deinit {
        let r = release
        DispatchQueue.main.async { MainActor.assumeIsolated { r() } }
    }
}
