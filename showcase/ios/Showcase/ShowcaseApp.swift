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
// every frame it composes. Each move is taken with time slowed `-slow`
// times for the frameworks that animate (`move`, .github/screens/timewarp.c),
// and the workflow speeds it back up. A shot says when it has settled
// (dev.arkui.showcase.ready) and when it is done (.done), which the
// workflow waits for.
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

/// How many times slower than life a recording's moves are taken (`-slow`),
/// by timewarp.c when it is loaded.
let slow = ShowcaseApp.argument("-slow").flatMap(Double.init) ?? 1

/// `then`, `seconds` from now, at life's speed.
func after(_ seconds: Double, _ then: @escaping () -> Void) {
    DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: then)
}

/// A Darwin notification, which the workflow (through `simctl spawn
/// notifyutil`) and timewarp.c hear.
func say(_ name: String) {
    CFNotificationCenterPostNotification(
        CFNotificationCenterGetDarwinNotifyCenter(), CFNotificationName("dev.arkui.showcase.\(name)" as CFString),
        nil, nil, true)
}

/// `change`, given `seconds` of life to play out in, with time slowed for
/// it alone (timewarp.c, told just before); then `then`, at life's speed.
func move(for seconds: Double, _ change: @escaping () -> Void, then: @escaping () -> Void = {}) {
    say("slow")
    after(0.05) {
        change()
        after(seconds * slow) {
            say("fast")
            after(0.05, then)
        }
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
                DispatchQueue.main.async {
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
    @State private var warm = false

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
            .onAppear { if !warm { after(1.5) { open = false } } }
            .onDisappear {
                if !warm {
                    warm = true
                    after(1) { say("ready") }
                }
            }
        }
        // Opened and closed once as it appears, off the recording, at life's
        // speed: the first zoom in a fresh simulator pays for setting itself
        // up and draws half the frames of the next, and is slow enough that
        // a close sent too soon after it is ignored, so each step waits for
        // the one before it to be seen (above), however slow the runner.
        // The zoom is the system's, slowed for a recording with the rest
        // (timewarp.c).
        .onAppear { after(0.5) { open = true } }
        .onChange(of: go.told) {
            after(0.4) {
                move(for: 0.9, { open = true }) {
                    after(0.9) {
                        move(for: 0.8, { open = false }) { after(0.4) { say("done") } }
                    }
                }
            }
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
                .onAppear { after(1.5) { say("ready") } }
                .onChange(of: go.told) {
                    // Thirty rows down: a row is its picture and the gap under it.
                    let down = 30 * (Space.thumb + Space.m)
                    after(0.4) {
                        move(for: 0.6, { glide(from: 0, to: down) }) {
                            after(0.6) {
                                move(for: 0.6, { glide(from: down, to: 0) }) { after(0.4) { say("done") } }
                            }
                        }
                    }
                }
            }
        }

        /// From `a` to `b` on the spring, a step each tick, by Core
        /// Animation's clock (which a recording slows, timewarp.c).
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
