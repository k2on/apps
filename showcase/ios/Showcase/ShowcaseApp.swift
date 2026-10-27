// ArkUI's components on their own, on a real simulator: one shot by name
// (`-showcase art`), with placeholder content and ArkUI's neutral colours.
//
// The shots are the docs' (`docs/site/src/components.rs`, those it says iOS
// has); the screens workflow (.github/workflows/screens.yaml) captures each a
// few seconds after launch, in the theme it set. A recording's shot is
// opened and left to settle first, then told to move by a Darwin
// notification (`notifyutil -p dev.arkui.showcase.go`, sent through
// `simctl spawn`), so the launch is never in the recording. With `-slow 8`
// it moves eight times slower than life, its pauses too, and the workflow
// speeds the recording back up: life's timing, with eight times the frames
// the simulator could draw.
import ArkUI
import SwiftUI
import UIKit

@main
struct ShowcaseApp: App {
    @StateObject private var go = Go()

    var body: some Scene {
        WindowGroup { Screen(shot: Self.shot).environmentObject(go) }
    }

    /// The shot this launch was asked for: `art` when none was.
    static var shot: String { argument("-showcase") ?? "art" }

    static func argument(_ name: String) -> String? {
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
        return args[i + 1]
    }
}

/// How many times slower than life a recording's shot moves (`-slow`): its
/// animations and its pauses alike.
let slow = ShowcaseApp.argument("-slow").flatMap(Double.init) ?? 1

/// `seconds` of life, slowed.
func later(_ seconds: Double, _ then: @escaping () -> Void) {
    DispatchQueue.main.asyncAfter(deadline: .now() + seconds * slow, execute: then)
}

/// The system's own animations, the zoom of a transition included, slowed
/// as SwiftUI's are by `.speed`: Core Animation's clock for the whole window.
func slowTheWindow() {
    for case let scene as UIWindowScene in UIApplication.shared.connectedScenes {
        for window in scene.windows { window.layer.speed = Float(1 / slow) }
    }
}

/// Whether the workflow has said to move: Android's `--ez go true`.
final class Go: ObservableObject {
    static let name = "dev.arkui.showcase.go"
    @Published var told = false

    init() {
        CFNotificationCenterAddObserver(
            CFNotificationCenterGetDarwinNotifyCenter(),
            Unmanaged.passUnretained(self).toOpaque(),
            { _, me, _, _, _ in
                guard let me else { return }
                let go = Unmanaged<Go>.fromOpaque(me).takeUnretainedValue()
                DispatchQueue.main.async { go.told = true }
            },
            Go.name as CFString,
            nil,
            .deliverImmediately
        )
    }
}

/// Places, which mean nothing to any app in particular.
private let places = ["Harbour", "Orchard", "Meadow", "Summit", "Delta", "Lagoon", "Canyon", "Glacier", "Prairie", "Fjord", "Dune", "Grove"]

/// One shot, by the id the docs give it.
struct Screen: View {
    let shot: String

    var body: some View {
        switch shot {
        case "art":
            ScrollView {
                VStack(alignment: .leading, spacing: Space.m) {
                    Text("Squares").font(.title2.bold())
                    LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: Space.m), count: 3), spacing: Space.m) {
                        ForEach(places, id: \.self) { p in
                            Square(seed: p)
                                .aspectRatio(1, contentMode: .fit)
                                .clipShape(RoundedRectangle(cornerRadius: Space.corner))
                        }
                    }
                }
                .padding(Space.page)
            }
        case "expanding-card":
            if #available(iOS 26.0, *) { ExpandingCardShot() } else { Text("Needs iOS 26") }
        case "scroll-to-top":
            ScrollToTopShot()
        default:
            Text("No showcase for \(shot)")
        }
    }
}

/// A place in a list: its square and its name.
private struct Row: View {
    let name: String

    var body: some View {
        HStack(spacing: Space.m) {
            Square(seed: name)
                .frame(width: Space.thumb, height: Space.thumb)
                .clipShape(RoundedRectangle(cornerRadius: Space.corner))
            VStack(alignment: .leading) {
                Text(name)
                Text("A place").font(.footnote).foregroundStyle(.secondary)
            }
        }
    }
}

/// A bar above the tab bar zooming into the whole screen, held, and back.
@available(iOS 26.0, *)
private struct ExpandingCardShot: View {
    @EnvironmentObject private var go: Go
    @State private var open = false

    var body: some View {
        TabView {
            Tab("A page", systemImage: "square.grid.2x2") {
                NavigationStack {
                    List(places.prefix(8), id: \.self) { Row(name: $0) }
                        .listStyle(.plain)
                        .navigationTitle("A page")
                }
            }
            Tab("Another", systemImage: "circle") { Text("Another page") }
        }
        .expandingCard(isPresented: $open) {
            HStack(spacing: Space.m) {
                Square(seed: "Harbour")
                    .frame(width: 32, height: 32)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                Text("A card").font(.subheadline.weight(.semibold))
                Spacer()
            }
            .padding(.horizontal, Space.l)
        } expanded: {
            VStack(alignment: .leading, spacing: Space.l) {
                Square(seed: "Harbour")
                    .aspectRatio(1, contentMode: .fit)
                    .clipShape(RoundedRectangle(cornerRadius: Space.corner))
                Text("A card").font(.title2.bold())
                Text("Its bar's bounds to the screen's, and back.")
                Spacer()
            }
            .padding(Space.xl)
        }
        .onChange(of: go.told) {
            slowTheWindow()
            later(0.4) { open = true }
            later(2.2) { open = false }
        }
    }
}

/// A long list scrolled down, then taken back to its top: what the shown
/// tab, tapped again, does on iOS. A ScrollView, as `scrollTo` animates
/// there and not in a List.
private struct ScrollToTopShot: View {
    @EnvironmentObject private var go: Go
    private let rows = (1...60).map { "\(places[$0 % places.count]) \($0)" }

    var body: some View {
        NavigationStack {
            ScrollViewReader { list in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: Space.m) {
                        ForEach(rows, id: \.self) { Row(name: $0) }
                    }
                    .padding(.horizontal, Space.page)
                }
                .navigationTitle("A page")
                .onChange(of: go.told) {
                    let move = Springs.move.speed(1 / slow)
                    later(0.4) { withAnimation(move) { list.scrollTo(rows[30], anchor: .top) } }
                    later(1.6) { withAnimation(move) { list.scrollTo(rows[0], anchor: .top) } }
                }
            }
        }
    }
}
