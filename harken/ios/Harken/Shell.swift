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
import SwiftUI

struct Shell: View {
    @EnvironmentObject private var model: Model
    @State private var tab: Pane = .home
    @State private var libraryTab: LibraryTab = .playlists
    @State private var paths: [Pane: [Route]] = [.home: [], .library: [], .search: []]
    @Namespace private var zoom

    private func path(_ t: Pane) -> Binding<[Route]> {
        Binding(get: { paths[t] ?? [] }, set: { paths[t] = $0 })
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
        .alert(model.notice ?? "", isPresented: Binding(get: { model.notice != nil }, set: { if !$0 { model.notice = nil } })) {
            Button("OK", role: .cancel) { model.notice = nil }
        }
    }

    /// A tab's stack: its root, every page it can open, and the account at the top right.
    private func stack<Root: View>(_ t: Pane, @ViewBuilder root: () -> Root) -> some View {
        NavigationStack(path: path(t)) {
            root()
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) { AccountMenu() }
                }
                .navigationDestination(for: Route.self) { r in
                    Destination(route: r).zoomed(r, zoom)
                }
        }
        .environment(\.open) { r in paths[t, default: []].append(r) }
    }

    private func open(_ r: Route) { paths[tab, default: []].append(r) }

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
        }
    }
}

/// One page of a tab's stack.
struct Destination: View {
    let route: Route
    @EnvironmentObject private var model: Model

    var body: some View {
        switch route {
        case .page(.album(let n)): AlbumPage(model: model, name: n)
        case .page(.artist(let n)): ArtistPage(model: model, name: n)
        case .page(.playlist(let id)): PlaylistPage(model: model, id: id)
        case .page(.work(let id)): WorkPage(model: model, id: id)
        case .page(.recording(let id)): RecordingPage(model: model, id: id)
        case .page(.songs): SongsList().navigationTitle("Songs")
        case .settings: SettingsView()
        case .diagnostics: DiagnosticsView()
        }
    }
}

/// A page's view, held for as long as the page is on its stack — not only
/// while it is drawn — so coming back to it finds its rows; let go when it is
/// popped. Made once per page by the page's `@StateObject`.
@MainActor
final class Holding: ObservableObject {
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
