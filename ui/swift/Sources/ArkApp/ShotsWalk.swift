// A screenshot walk, taken inside the app (ui/docs/shots.md, "In the
// App"): a launch with `-shots YES` that finds a walk's plan in the app's
// own directory, on a simulator, walks it and photographs itself, then
// exits. The script that wrote the plan (.github/screens/apps-ios.sh)
// takes the pictures from the app's container.
//
// The plan, `<Application Support>/shots/plan.json`:
//
//   {"orientation": "portrait" | "landscape",
//    "shots": [{"id": "welcome", "steps": []},
//              {"id": "budget", "steps": [{"tap": "Get Started"}, …]}, …]}
//
// `"theme": "dark"` in it: an app that draws one theme whatever the
// system's, each screen taken once and written as both.
//
// Each shot's steps are taken in Rust (`ArkHost.shotsStep`, ark_app::shots):
// each reaches what it names in the views on the screen now and sends the
// message a tap on it sends. After each the walk waits for the app's own
// ready signal (`Ready`), then the next. Then the screen is taken in the
// theme it is in, the theme switched, and taken again; the next shot
// starts in the theme this one ended in. Each picture lands in
// `shots/out/<theme>/<id>.png`, what a step can name beside it
// (`<id>.tree.txt`), each shot's times in `shots/out/walk.log`, and
// `shots/out/done` (or `failed`, why) once it is over.
import ArkUI
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
#if canImport(UIKit)
import UIKit
#endif

@MainActor
final class ShotsWalk {
    /// How long one step waits for what it names, and for the screen to be
    /// ready after it: guards, which fail the walk, never a wait.
    private static let find = 20.0
    private static let settle = 60.0

    /// The plan this launch walks: a screenshot's launch, on a simulator,
    /// with a plan in the app's own directory (`root`, Application
    /// Support), which only the app and the simulator's own tools write.
    static func planned(root: String) -> URL? {
        #if targetEnvironment(simulator)
        guard Shots.on else { return nil }
        let plan = URL(fileURLWithPath: root).appendingPathComponent("shots/plan.json")
        return FileManager.default.fileExists(atPath: plan.path) ? plan : nil
        #else
        return nil
        #endif
    }

    private let host: ArkHost
    private let plan: URL
    private let out: URL
    private var log = ""

    init(host: ArkHost, plan: URL) {
        self.host = host
        self.plan = plan
        out = plan.deletingLastPathComponent().appendingPathComponent("out")
    }

    /// The walk, then the process ended: it is the walk's alone.
    func run() async {
        try? FileManager.default.removeItem(at: out)
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let failed: String?
        do {
            try await walk()
            failed = nil
        } catch let e as Stop {
            failed = e.why
        } catch {
            failed = "\(error)"
        }
        say(failed.map { "failed: \($0)" } ?? "done")
        try? log.write(to: out.appendingPathComponent("walk.log"), atomically: true, encoding: .utf8)
        try? (failed ?? "").write(to: out.appendingPathComponent(failed == nil ? "done" : "failed"), atomically: true, encoding: .utf8)
        exit(failed == nil ? 0 : 1)
    }

    private struct Stop: Error { let why: String }

    private func say(_ line: String) {
        print("Shots: \(line)")
        log += line + "\n"
    }

    private func walk() async throws {
        #if canImport(UIKit)
        let data = try Data(contentsOf: plan)
        guard let p = try JSONSerialization.jsonObject(with: data) as? [String: Any], let shots = p["shots"] as? [[String: Any]] else {
            throw Stop(why: "\(plan.path) is no plan")
        }
        // The first screen ready, light, the way the plan holds the device.
        let one = p["theme"] as? String
        var dark = false
        ShotsWindow.theme(dark: dark)
        if p["orientation"] as? String == "landscape" { ShotsWindow.orient(landscape: true) }
        try await settled("the first screen")
        for shot in shots {
            guard let id = shot["id"] as? String else { throw Stop(why: "a shot with no id") }
            let steps = shot["steps"] as? [[String: Any]] ?? []
            var t = Date()
            for step in steps { try await take(step, id) }
            let stepped = Date().timeIntervalSince(t)
            let tree = host.shotsTree()
            var times: [String] = []
            var first: Data? = nil
            for n in 0..<(one == nil ? 2 : 1) {
                if n == 1 {
                    // The other theme, as the system's switch would: no relaunch.
                    t = Date()
                    dark.toggle()
                    ShotsWindow.theme(dark: dark)
                    try await settled("\(id) in \(dark ? "dark" : "light")")
                    times.append("switched \(ms(t))")
                }
                t = Date()
                guard let png = ShotsWindow.picture() else { throw Stop(why: "\(id): no window to draw") }
                if let f = first, f == png { throw Stop(why: "\(id): dark and light drew the same picture") }
                first = png
                let theme = dark ? "dark" : "light"
                for t in one == nil ? [theme] : ["light", "dark"] {
                    let dir = out.appendingPathComponent(t)
                    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                    try png.write(to: dir.appendingPathComponent("\(id).png"))
                    try tree.write(to: dir.appendingPathComponent("\(id).tree.txt"), atomically: true, encoding: .utf8)
                }
                times.append("\(theme) picture \(ms(t))")
            }
            say("\(id): \(steps.count) steps \(Int(stepped * 1000)) ms, \(times.joined(separator: ", "))")
        }
        #else
        throw Stop(why: "a walk is the iPhone's and the iPad's")
        #endif
    }

    /// One step, taken once what it names is on the screen, then the screen
    /// waited for.
    private func take(_ step: [String: Any], _ id: String) async throws {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: step, options: [.sortedKeys]), as: UTF8.self)
        let until = Date().addingTimeInterval(Self.find)
        while true {
            let g = Ready.shared.generation
            switch host.shotsStep(step: json) {
            case .done:
                Ready.shared.changed()
                try await settled("\(id) after \(json)")
                return
            case let .notYet(labels):
                if Date() > until {
                    throw Stop(why: "\(id): nothing on the screen for \(json) in \(Int(Self.find)) s; there was:\n\(labels)")
                }
                // Looked for again once the screen has changed (or a moment on).
                _ = await Ready.shared.untilReady(after: g, within: 0.5)
            case let .failed(why):
                throw Stop(why: "\(id): \(json): \(why)")
            }
        }
    }

    /// Until the app says its screen is ready at a newer generation.
    private func settled(_ what: String) async throws {
        let g = Ready.shared.generation
        Ready.shared.changed()
        guard await Ready.shared.untilReady(after: g, within: Self.settle) else {
            throw Stop(why: "\(what): not ready in \(Int(Self.settle)) s (\(Ready.shared.why))")
        }
    }

    private func ms(_ since: Date) -> String { "\(Int(Date().timeIntervalSince(since) * 1000)) ms" }
}
