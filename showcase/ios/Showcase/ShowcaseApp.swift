// ArkUI's components on their own, on a real simulator: one shot by name
// (`-showcase art`), with placeholder content and ArkUI's neutral colours.
//
// The shots are the docs' (`docs/site/src/components.rs`, those it says iOS
// has); the screens workflow (.github/workflows/screens.yaml) captures each a
// few seconds after launch, in the theme it set. A recording's shot is
// opened and left to settle first, then told to move by a Darwin
// notification (`notifyutil -p dev.arkui.showcase.go`, sent through
// `simctl spawn`), so the launch is never in the recording, which
// .github/screens/record.swift takes from the simulator's framebuffer,
// every frame the app draws, at life's speed.
import ArkUI
import QuartzCore
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

/// `then`, `seconds` from now.
func later(_ seconds: Double, _ then: @escaping () -> Void) {
    DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: then)
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
                DispatchQueue.main.async {
                    // A probe's (probe.yaml): the windows' layer speed, as
                    // told to go, for every shot.
                    if let speed = ShowcaseApp.argument("-layerSpeed").flatMap(Float.init) {
                        for case let scene as UIWindowScene in UIApplication.shared.connectedScenes {
                            for window in scene.windows { window.layer.speed = speed }
                        }
                    }
                    go.told = true
                    Frames.shared.count()
                }
            },
            Go.name as CFString,
            nil,
            .deliverImmediately
        )
    }
}

/// How many frames the app draws a second once told to go, written to
/// Documents/frames.log in its container, which ios.sh prints beside what
/// the recording caught: which of the two keeps a recording short of frames.
final class Frames: NSObject {
    static let shared = Frames()
    private var link: CADisplayLink?
    private var second: CFTimeInterval = 0
    private var frames = 0
    private var longest: CFTimeInterval = 0
    private var last: CFTimeInterval = 0

    func count() {
        guard link == nil else { return }
        try? FileManager.default.removeItem(at: Self.log)
        let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    static let log = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("frames.log")

    private func say(_ line: String) {
        let data = Data((line + "\n").utf8)
        if let file = try? FileHandle(forWritingTo: Self.log) {
            file.seekToEndOfFile()
            file.write(data)
            try? file.close()
        } else {
            try? data.write(to: Self.log)
        }
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        if last > 0 { longest = max(longest, now - last) }
        last = now
        if second == 0 { second = now }
        frames += 1
        if now - second >= 1 {
            say("app drew \(frames) frames in \(String(format: "%.2f", now - second))s, longest gap \(Int(longest * 1000)) ms")
            (second, frames, longest) = (now, 0, 0)
        }
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
        case "clocks":
            ClocksShot()
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
        // The zoom is the system's: the bar opens, holds, and closes.
        .onChange(of: go.told) {
            later(0.4) { open = true }
            later(2.2) { open = false }
        }
    }
}

/// A long list scrolled down, then taken back to its top: what the shown
/// tab, tapped again, does on iOS. The offset is set once a frame along
/// Springs.move's own curve, rather than left to an animated `scrollTo`
/// (a List's jumps; a ScrollView's jumped here too), so every frame the
/// simulator draws is a place along the way.
private struct ScrollToTopShot: View {
    var body: some View {
        if #available(iOS 18.0, *) { Stepped() } else { Text("Needs iOS 18") }
    }

    @available(iOS 18.0, *)
    private struct Stepped: View {
        @EnvironmentObject private var go: Go
        @State private var position = ScrollPosition(edge: .top)
        private let rows = (1...60).map { "\(places[$0 % places.count]) \($0)" }

        var body: some View {
            NavigationStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: Space.m) {
                        ForEach(rows, id: \.self) { Row(name: $0) }
                    }
                    .padding(.horizontal, Space.page)
                }
                .scrollPosition($position)
                .navigationTitle("A page")
                .onChange(of: go.told) {
                    // Thirty rows down: a row is its picture and the gap under it.
                    let down = 30 * (Space.thumb + Space.m)
                    later(0.4) { glide(from: 0, to: down) }
                    later(1.6) { glide(from: down, to: 0) }
                }
            }
        }

        /// From `a` to `b` on the spring, a step each tick.
        private func glide(from a: CGFloat, to b: CGFloat) {
            let start = CACurrentMediaTime()
            Timer.scheduledTimer(withTimeInterval: 1.0 / 120, repeats: true) { timer in
                let t = CACurrentMediaTime() - start
                let done = t >= 0.6
                position.scrollTo(y: a + (b - a) * CGFloat(done ? 1 : sprung(t)))
                if done { timer.invalidate() }
            }
        }
    }
}

/// Where Springs.move is at `t` seconds, from 0 to 1: a unit mass on
/// Springs.stiffness, damped at 0.88 of critical, as SwiftUI's
/// `spring(response:dampingFraction:)` it is built from.
func sprung(_ t: Double) -> Double {
    let zeta = 0.88
    let omega = Springs.stiffness.squareRoot()
    let damped = omega * (1 - zeta * zeta).squareRoot()
    return 1 - exp(-zeta * omega * t) * (cos(damped * t) + zeta * omega / damped * sin(damped * t))
}

/// A probe (.github/workflows/probe.yaml), not a shot of the docs: three
/// squares each crossing the screen in one second once told to go, moved by
/// three clocks: UIKit's (UIView.animate), SwiftUI's (withAnimation), and
/// the render server's (a CABasicAnimation). Recorded with each way of
/// slowing a simulator's animations, it says which clock each slows.
private struct ClocksShot: View {
    @EnvironmentObject private var go: Go
    @State private var across = false

    var body: some View {
        GeometryReader { g in
            VStack(alignment: .leading, spacing: 120) {
                Moved(by: .uikit, go: go.told).frame(height: 80)
                Color.green.frame(width: 80, height: 80).offset(x: across ? g.size.width - 80 : 0)
                Moved(by: .coreAnimation, go: go.told).frame(height: 80)
            }
            .padding(.top, 160)
        }
        .onChange(of: go.told) {
            withAnimation(.linear(duration: 1)) { across = true }
        }
    }

    private struct Moved: UIViewRepresentable {
        enum Clock { case uikit, coreAnimation }
        let by: Clock
        let go: Bool

        func makeUIView(context: Context) -> UIView {
            let row = UIView()
            let square = UIView(frame: CGRect(x: 0, y: 0, width: 80, height: 80))
            square.backgroundColor = by == .uikit ? .systemRed : .systemBlue
            square.tag = 1
            row.addSubview(square)
            return row
        }

        func updateUIView(_ row: UIView, context: Context) {
            guard go, let square = row.viewWithTag(1), square.frame.minX == 0, square.layer.animationKeys() == nil else { return }
            let to = row.bounds.width - 80
            switch by {
            case .uikit:
                UIView.animate(withDuration: 1, delay: 0, options: .curveLinear) { square.frame.origin.x = to }
            case .coreAnimation:
                let move = CABasicAnimation(keyPath: "position.x")
                move.fromValue = square.layer.position.x
                move.toValue = to + 40
                move.duration = 1
                move.timingFunction = CAMediaTimingFunction(name: .linear)
                move.fillMode = .forwards
                move.isRemovedOnCompletion = false
                square.layer.add(move, forKey: "across")
            }
        }
    }
}
