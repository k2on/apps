// A screenshot walk, taken inside the app (ui/docs/shots.md, "In the
// App"): a launch with `-shots YES` that finds a walk's plan in the app's
// own directory, on a simulator, walks it and photographs itself, then
// exits. The script that wrote the plan (.github/screens/apps-ios.sh)
// takes the pictures from the app's container.
//
// The plan, `<Application Support>/shots/plan.json`:
//
//   {"orientations": ["portrait", "landscape"] (the iPad's; none: upright),
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
// starts in the theme this one ended in; an iPad's turns on its side in
// the app too, one switch at a time. Each picture lands in
// `shots/out/<theme>/<id>.png` (`out/<portrait|landscape>/<theme>/` when
// it turns), what a step can name beside it
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
    nonisolated static func planned(root: String) -> URL? {
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
    /// Whether the screen Rust says is up has a sheet, a dialog or an alert
    /// in it (its tree's `(modal)`): the screen is not ready until the
    /// platform presents it.
    private var modal = false

    init(host: ArkHost, plan: URL) {
        self.host = host
        self.plan = plan
        out = plan.deletingLastPathComponent().appendingPathComponent("out")
    }

    /// The walk, then the process ended: it is the walk's alone.
    func run() async {
        #if canImport(UIKit)
        Ready.shared.probe { [weak self] in
            guard let self, self.modal, !ShotsWindow.presenting else { return nil }
            return "a sheet or a dialog the app shows, not presented yet"
        }
        #endif
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
        // The ways a screen is taken, in the order the walks go, each one
        // switch from the last: light, dark; and on the iPad (`orientations`)
        // then dark on its side, light on its side. A one-theme app's
        // (`theme`): one way (a way up each on the iPad).
        let one = p["theme"] as? String
        let turns = (p["orientations"] as? [String] ?? []).count > 1
        var ways: [(dark: Bool, landscape: Bool)] = one == nil ? [(false, false), (true, false)] : [(false, false)]
        if turns { ways = one == nil ? [(false, false), (true, false), (true, true), (false, true)] : [(false, false), (false, true)] }
        var at = (dark: false, landscape: false)
        ShotsWindow.theme(dark: false)
        try await settled("the first screen")
        // To another way: the one thing that differs switched, the screen waited for.
        func go(_ to: (dark: Bool, landscape: Bool)) async throws -> String {
            let t = Date()
            if to.dark != at.dark { ShotsWindow.theme(dark: to.dark) }
            if to.landscape != at.landscape {
                var why = ""
                ShotsWindow.orient(landscape: to.landscape) { why = $0 }
                // Until the scene is the way it was asked (its geometry is
                // the system's to change); the device told as well if not.
                var tries = 0
                while ShotsWindow.wide != to.landscape && tries < 100 {
                    if tries == 30 { ShotsWindow.turnDevice(landscape: to.landscape) }
                    try await Task.sleep(nanoseconds: 50_000_000)
                    tries += 1
                }
                if ShotsWindow.wide != to.landscape { throw Stop(why: "the scene did not turn \(to.landscape ? "on its side" : "upright"): \(why)") }
            }
            at = to
            try await settled("\(name(at))")
            return ms(t)
        }
        var drawn: [String: [String: Data]] = [:]
        func shoot(_ id: String, _ tree: String) throws -> String {
            let t = Date()
            guard let image = ShotsWindow.picture(), let png = image.pngData() else { throw Stop(why: "\(id): no window to draw") }
            let theme = at.dark ? "dark" : "light", way = at.landscape ? "landscape" : "portrait"
            // The theme really switched: the bars drawn in it, not left in
            // the last (iOS 26's glass kept its old one, 43318181211). A
            // one-theme app's are its own.
            var bar = ""
            if one == nil, let b = ShotsWindow.barLightness(in: image) {
                bar = String(format: ", %@ %.2f", b.bar, b.lightness)
                if (b.lightness < 0.5) != at.dark {
                    throw Stop(why: "\(id) \(name(at)): its \(b.bar) is drawn \(b.lightness < 0.5 ? "dark" : "light") (\(String(format: "%.2f", b.lightness))): the theme did not switch")
                }
            }
            drawn[id, default: [:]][name(at)] = png
            for th in one == nil ? [theme] : ["light", "dark"] {
                let dir = turns ? out.appendingPathComponent(way).appendingPathComponent(th) : out.appendingPathComponent(th)
                try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                try png.write(to: dir.appendingPathComponent("\(id).png"))
                try tree.write(to: dir.appendingPathComponent("\(id).tree.txt"), atomically: true, encoding: .utf8)
            }
            return ms(t) + bar
        }
        // The shots before the sign-in's (its shot says `signIn`): the first
        // run's pages, each taken every way at the start, then back to the
        // first way.
        let first = shots.firstIndex { $0["signIn"] as? Bool == true } ?? 0
        for shot in shots[..<first] {
            guard let id = shot["id"] as? String else { throw Stop(why: "a shot with no id") }
            for step in shot["steps"] as? [[String: Any]] ?? [] { _ = try await take(step, id) }
            let tree = host.shotsTree()
            var times: [String] = []
            for (k, w) in ways.enumerated() {
                if k > 0 {
                    let switched = try await go(w)
                    times.append("switched \(switched)")
                }
                let took = try shoot(id, tree)
                times.append("\(name(at)) picture \(took)")
            }
            say("\(id): \(times.joined(separator: ", "))")
        }
        if at != ways[0] {
            let back = try await go(ways[0])
            say("back to \(name(ways[0])) \(back)")
        }
        // Then the rest, walked whole once a way: the sign-in's steps on the
        // first walk alone; before each walk after, the app put back where
        // the walk starts (`ArkHost.shotsReset`: what shows put away, the
        // first section), then the next way switched.
        for (k, w) in ways.enumerated() {
            let walkBegan = Date()
            if k > 0 {
                let r = Date()
                switch host.shotsReset() {
                case .done: break
                case let .failed(why), let .notYet(why): throw Stop(why: "reset: \(why)")
                }
                modal = host.shotsTree().contains(" (modal)")
                try await settled("reset")
                let reset = ms(r)
                let switched = try await go(w)
                say("reset \(reset), switched \(switched)")
            }
            for shot in shots[first...] {
                guard let id = shot["id"] as? String else { throw Stop(why: "a shot with no id") }
                let steps = (k > 0 && shot["signIn"] as? Bool == true) ? [] : shot["steps"] as? [[String: Any]] ?? []
                let t = Date()
                var each: [String] = []
                for step in steps { each.append(try await take(step, id)) }
                let stepped = ms(t)
                let tree = host.shotsTree()
                let picture = try shoot(id, tree)
                say("\(id) \(name(at)): \(steps.count) steps \(stepped) [\(each.joined(separator: "; "))], picture \(picture)")
                // Within a walk, two shots of one picture: a step that went nowhere.
                if let same = shots[first...].compactMap({ $0["id"] as? String }).first(where: { $0 != id && drawn[$0]?[name(at)] == drawn[id]?[name(at)] }) {
                    throw Stop(why: "\(id) and \(same) drew the same picture \(name(at))")
                }
            }
            say("the walk \(name(at)): \(ms(walkBegan))")
        }
        // Each screen's ways apart: one the same as another did not switch.
        for (id, byWay) in drawn {
            let ws = byWay.keys.sorted()
            for (a, x) in ws.enumerated() { for y in ws[(a + 1)...] where byWay[x] == byWay[y] {
                throw Stop(why: "\(id): \(x) and \(y) drew the same picture")
            } }
        }
        #else
        throw Stop(why: "a walk is the iPhone's and the iPad's")
        #endif
    }

    /// One step, then the screen waited for: its times, Rust's and the
    /// ready signal's. What it names not there at a ready screen fails at
    /// once (it will not come); while the screen is on its way (a sign-in,
    /// a sync) it is looked for again each time the screen is ready anew.
    private func take(_ step: [String: Any], _ id: String) async throws -> String {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: step, options: [.sortedKeys]), as: UTF8.self)
        let until = Date().addingTimeInterval(Self.find)
        let began = Date()
        while true {
            let g = Ready.shared.generation
            let r = Date()
            let took = host.shotsStep(step: json)
            let rust = ms(r)
            switch took {
            case .done:
                let w = Date()
                modal = host.shotsTree().contains(" (modal)")
                try await settled("\(id) after \(json)")
                return "\(json) \(rust) + ready \(ms(w))"
            case let .notYet(labels):
                if (Ready.shared.ready && Ready.shared.generation == g) || Date() > until {
                    throw Stop(why: "\(id): nothing on the ready screen is \(json) (\(ms(began))); there was:\n\(labels)")
                }
                _ = await Ready.shared.untilReady(after: g, within: Self.find)
            case let .failed(why):
                throw Stop(why: "\(id): \(json): \(why)")
            }
        }
    }

    /// Until the app says its screen is ready at a newer generation; what
    /// held it, each with when it began, logged.
    private func settled(_ what: String) async throws {
        let g = Ready.shared.generation
        let t = Date()
        Ready.shared.trace()
        Ready.shared.changed()
        guard await Ready.shared.untilReady(after: g, within: Self.settle) else {
            throw Stop(why: "\(what): not ready in \(Int(Self.settle)) s (\(Ready.shared.why))")
        }
        let held = Ready.shared.held.map { "\(Int($0.at * 1000)) ms \($0.why)" }
        let f = Ready.shared.frames
        if !held.isEmpty || f.longest > 100 {
            say("  \(what): ready in \(ms(t)), \(f.count) frames, the longest \(f.longest) ms apart; held by: \(held.joined(separator: " | "))")
        }
    }

    private func name(_ w: (dark: Bool, landscape: Bool)) -> String {
        "\(w.dark ? "dark" : "light")\(w.landscape ? " landscape" : "")"
    }

    private func ms(_ since: Date) -> String { "\(Int(Date().timeIntervalSince(since) * 1000)) ms" }
}
