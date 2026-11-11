// Every app on one simulator, both themes, in one run of the test, driven
// as a person would (../../apps.md, ../../apps-ios.sh): what to do is a plan,
// a JSON file on the Mac (`TEST_RUNNER_APP_PLAN`, apps-ios.sh writes it):
//
//   [{"bundle": …, "server"?: …, "user"?: …, "scheme"?: …, "out": <dir>,
//     "orientation"?: "portrait"|"landscape", "signedIn"?: true,
//     "shots": [{"id": …, "args"?: [launch arguments], "route"?: <path>,
//                "steps": [{"tap": label, "role"?: …} | {"type": text} | {"await": label} | …]}]}]
//
// For each theme (the simulator's own appearance, set from here), each
// app in turn: launched with `-shots YES` (ArkUI's `Shots`: no motion, and
// the ready signal), signed in through arkui's onboarding to the server CI
// started, if it is not already, then each shot. **Nothing waits for a
// time.** The app says when its screen is ready (`ark.ready`, labelled
// `ready <generation> <route>`, ui/docs/shots.md); after anything the driver
// does it waits for a newer generation, then takes the screen. A shot is
// opened by its `route` as a link (`<scheme>://<route>`) in the running app
// where it has one, by its launch arguments with a fresh launch where it
// has those, and otherwise from where the last left it; `relaunch` starts
// it again. Every wait has a limit, which only fails the shot.
//
// A tap's label is what the element says or its accessibility identifier;
// `role` ("tab", "button", "field", "text") says which kind when several
// say it; `press` holds it instead (a row's context menu), as does "hold":
// true; `a|b` is whichever is there, and "optional": true passes over a tap
// with nothing to tap. A `wait` is not slept: the step after it, or the
// shot, waits for the app to be ready, as every step does.
//
// Writes <out>/<theme>/<id>.png, and beside each the app's accessibility
// tree as it was taken (<id>.tree.txt), which is what a step can name.
import UIKit
import XCTest

/// One app's part of the plan.
struct Entry: Decodable {
    var bundle: String
    var server: String?
    var user: String?
    var scheme: String?
    var out: String
    var orientation: String?
    var signedIn: Bool?
    var shots: [Shot]
}

/// A shot, as shots.json has it.
struct Shot: Decodable {
    struct Step: Decodable {
        var tap: String?
        /// Held (a long press: a row's context menu), as `tap` finds it.
        var press: String?
        var role: String?
        var `where`: String?
        var unless: String?
        /// `await` in shots.json, which Swift keeps for itself.
        var waitFor: String?
        var type: String?
        var wait: Double?
        var relaunch: Bool?
        /// A tap with nothing to tap passed over.
        var optional: Bool?
        /// A long press, not a tap: what opens a context menu.
        var hold: Bool?

        enum CodingKeys: String, CodingKey {
            case tap, press, role, `where`, unless, type, wait, relaunch, optional, hold
            case waitFor = "await"
        }
    }

    var id: String
    var steps: [Step] = []
    /// The app's launch arguments for this shot (shots.json's `screens`).
    var args: [String] = []
    /// Where it is, as the app's router spells it: opened as a link.
    var route: String?

    enum CodingKeys: String, CodingKey { case id, steps, args, route }

    init(from d: Decoder) throws {
        let c = try d.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        steps = try c.decodeIfPresent([Step].self, forKey: .steps) ?? []
        args = try c.decodeIfPresent([String].self, forKey: .args) ?? []
        route = try c.decodeIfPresent(String.self, forKey: .route)
    }
}

/// The longest a screen may take to say it is ready: a guard, never a pace.
private let readyLimit: TimeInterval = 60

final class Drive: XCTestCase {
    private let env = ProcessInfo.processInfo.environment
    private let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
    /// The launch arguments each running app was launched with.
    private var launched: [String: [String]] = [:]

    func testShots() throws {
        let plan = try JSONDecoder().decode([Entry].self, from: Data(contentsOf: URL(fileURLWithPath: try value("APP_PLAN"))))
        var failed: [String] = []
        for (theme, appearance) in [("light", XCUIDevice.Appearance.light), ("dark", .dark)] {
            XCUIDevice.shared.appearance = appearance
            for entry in plan {
                do {
                    try take(entry, theme: theme)
                } catch {
                    failed.append("\(entry.bundle) (\(theme)): \(error.localizedDescription)")
                }
            }
        }
        XCTAssertTrue(failed.isEmpty, failed.joined(separator: "\n"))
    }

    /// One app's shots in `theme`, each written to `entry.out/theme/`.
    private func take(_ entry: Entry, theme: String) throws {
        let app = XCUIApplication(bundleIdentifier: entry.bundle)
        let out = URL(fileURLWithPath: entry.out).appendingPathComponent(theme)
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        switch entry.orientation {
        case "landscape"?: XCUIDevice.shared.orientation = .landscapeLeft
        case "portrait"?: XCUIDevice.shared.orientation = .portrait
        default: break
        }
        // Signed in once, in the first theme, from a launch with no arguments.
        if theme == "light", entry.signedIn != true, let server = entry.server, !server.isEmpty {
            launch(app, entry, args: [])
            try signedIn(app, server: server, user: entry.user ?? "", out: out)
        }
        for shot in entry.shots {
            do {
                try open(shot, in: app, entry: entry, out: out)
                try ready(app)
                try app.screenshot().pngRepresentation.write(to: out.appendingPathComponent("\(shot.id).png"))
                try app.debugDescription.write(to: out.appendingPathComponent("\(shot.id).tree.txt"), atomically: true, encoding: .utf8)
            } catch {
                try? app.debugDescription.write(to: out.appendingPathComponent("stuck.tree.txt"), atomically: true, encoding: .utf8)
                try? app.screenshot().pngRepresentation.write(to: out.appendingPathComponent("stuck.png"))
                throw error
            }
        }
    }

    /// The app at `shot`: by its launch arguments (a fresh launch), by its
    /// route (a link, in the running app), or from where it is; then each
    /// step, each followed by the app's next ready moment.
    private func open(_ shot: Shot, in app: XCUIApplication, entry: Entry, out: URL) throws {
        if let route = shot.route, let url = link(route, scheme: entry.scheme) {
            // Where the router can take it: in the app as it runs, no relaunch.
            if launched[entry.bundle] != shot.args || app.state != .runningForeground { launch(app, entry, args: shot.args) }
            let g = try ready(app)
            app.open(url)
            try ready(app, after: g)
        } else {
            // Opened afresh, as every shot is that names no route: a clean start.
            launch(app, entry, args: shot.args)
        }
        for step in shot.steps {
            let g = try ready(app)
            // A step that is done already: what it opens is on the screen.
            if let done = step.unless, there(app, done).exists { continue }
            if let label = step.waitFor {
                XCTAssertTrue(there(app, label).waitForExistence(timeout: readyLimit), "\(label) came, for \(shot.id)")
            }
            if let label = step.tap ?? step.press {
                let c = find(app, label, step: step)
                if step.optional == true && !c.exists { continue }
                guard c.exists else {
                    throw NSError(domain: "Drive", code: 2, userInfo: [NSLocalizedDescriptionKey: "\(label) is not there for \(shot.id); stuck.tree.txt is what was"])
                }
                if step.press != nil || step.hold == true { c.press(forDuration: 1.2) } else { c.tap() }
                try ready(app, after: g)
            }
            if step.relaunch == true {
                launch(app, entry, args: launched[entry.bundle] ?? [])
            }
            if let text = step.type {
                app.typeText(text)
                try ready(app, after: g)
            }
        }
    }

    /// How a shot's `route` is handed to the app: the one place that says
    /// it, so a router's own spelling (a whole URL, `ark_router::Address`)
    /// is a change here alone. A route that is a URL already is taken as it
    /// is; a path is the app's scheme's.
    private func link(_ route: String, scheme: String?) -> URL? {
        if route.contains("://") { return URL(string: route) }
        guard let scheme, !scheme.isEmpty else { return nil }
        return URL(string: "\(scheme)://\(route)")
    }

    /// `app` launched afresh with `args`, in screenshot mode, and ready.
    private func launch(_ app: XCUIApplication, _ entry: Entry, args: [String]) {
        app.terminate()
        app.launchArguments = ["-shots", "YES"] + args
        app.launch()
        launched[entry.bundle] = args
        _ = try? ready(app)
    }

    /// The app's ready signal at a generation after `after` (any, if nil),
    /// once it is there: its generation. Waited for as XCUITest waits for an
    /// element, up to `readyLimit`, which only fails.
    @discardableResult
    private func ready(_ app: XCUIApplication, after: Int? = nil) throws -> Int {
        let signal = app.descendants(matching: .any)["ark.ready"]
        let until = Date().addingTimeInterval(readyLimit)
        while Date() < until {
            guard signal.waitForExistence(timeout: until.timeIntervalSinceNow) else { break }
            let label = signal.label
            let g = Int(label.split(separator: " ").dropFirst().first ?? "") ?? -1
            if g > (after ?? -1) { return g }
            // Ready, but at the generation before: wait for it to change.
            let changed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false OR label != %@", label), object: signal)
            _ = XCTWaiter.wait(for: [changed], timeout: max(until.timeIntervalSinceNow, 0))
        }
        throw NSError(domain: "Drive", code: 3, userInfo: [NSLocalizedDescriptionKey: "the app did not say it was ready (after generation \(after ?? -1)) within \(Int(readyLimit)) s"])
    }

    /// What says `label` (`a|b`: either), or has it as its identifier.
    private func there(_ app: XCUIApplication, _ label: String) -> XCUIElement {
        let each = label.split(separator: "|").map(String.init)
        return app.descendants(matching: .any).matching(NSPredicate(format: "label IN %@ OR identifier IN %@", each, each)).firstMatch
    }

    /// Signed in through the server's page, if the app shows it, and past
    /// arkui's "Sign In".
    private func signedIn(_ app: XCUIApplication, server: String, user: String, out: URL) throws {
        // The server's page: its one text field, whatever an app labels it.
        let field = app.textFields.firstMatch
        guard field.waitForExistence(timeout: 20) else { return }
        do {
            try signIn(app, field: field, server: server, user: user)
        } catch {
            let seen = "== the app\n\(app.debugDescription)\n\n== SpringBoard\n\(springboard.debugDescription)"
            try? seen.write(to: out.appendingPathComponent("stuck.tree.txt"), atomically: true, encoding: .utf8)
            throw error
        }
        // Signed in once the app is past both the server's page and its
        // "Sign In" page, the system's "wants to use … to sign in" allowed
        // again if it asks.
        let signing = app.staticTexts["Sign In"]
        let allow = springboard.buttons["Continue"]
        let until = Date().addingTimeInterval(90)
        while Date() < until && (field.exists || signing.exists) {
            if allow.exists { allow.tap() }
            // An expectation is waited on once: a new one each time round.
            let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: field)
            _ = XCTWaiter.wait(for: [gone], timeout: 1)
        }
        if field.exists || signing.exists {
            let seen = "== the app\n\(app.debugDescription)\n\n== SpringBoard\n\(springboard.debugDescription)"
            try? seen.write(to: out.appendingPathComponent("stuck.tree.txt"), atomically: true, encoding: .utf8)
            throw NSError(domain: "Drive", code: 4, userInfo: [NSLocalizedDescriptionKey: "not signed in after 90 seconds; stuck.tree.txt is what was there"])
        }
        try ready(app)
    }

    /// The address typed and checked; the system's "wants to use … to sign
    /// in" allowed; the server's dev sign-in page given a name and sent. The
    /// page is in the system's web sheet, which another process draws
    /// (Safari's view service), so it is looked for there as well as in the
    /// app; and an app that waits for a press to open it gets one.
    private func signIn(_ app: XCUIApplication, field: XCUIElement, server: String, user: String) throws {
        field.tap()
        // Sent with the keyboard's Go, as the page sends it (`onSubmit`): a tap
        // on Continue while the keyboard is up can go to the keyboard's
        // leaving rather than to the button. Continue only if, a few seconds
        // on, the page has still not moved.
        // Typed, then read back, and only then sent: Go straight after the
        // last key can reach a page whose field has not yet taken that key
        // (harken's, which says each to its model, lost the last digit of
        // the port). Typed again, cleared first, until the field says it.
        for _ in 0..<3 {
            field.typeText(server)
            sleep(1)
            if (field.value as? String) == server { break }
            let had = (field.value as? String)?.count ?? 0
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: had + 4))
        }
        field.typeText("\n")
        sleep(4)
        let next = app.buttons["Continue"]
        if field.exists && next.exists && next.isEnabled && next.isHittable { next.tap() }
        let allow = springboard.buttons["Continue"]
        let open = app.buttons["Sign In"]
        let until = Date().addingTimeInterval(30)
        while Date() < until && !allow.exists {
            if open.exists && open.isHittable { open.tap() }
            sleep(1)
        }
        if allow.exists { allow.tap() }
        // The server signs in its own user (ARK_DEV_USER, app-server.sh) and
        // the sheet comes straight back: the server's page is gone.
        if field.waitForNonExistence(timeout: 30) { return }
        // A server that asks instead: its page, in the system's web sheet. It is drawn by
        // Safari's view service, a process of its own, and nothing of that
        // process is touched: tapping its element makes XCUITest bring it up
        // as an app, which takes the sheet away and the sign-in with it (the
        // screen goes black). The page puts its field in focus itself
        // (`autofocus`), so the name is typed as the app's keyboard input,
        // then sent with Go; a field the app's own hierarchy shows is tapped
        // first.
        let name = app.webViews.textFields.firstMatch
        guard name.waitForExistence(timeout: 10) else {
            throw NSError(domain: "Drive", code: 5, userInfo: [NSLocalizedDescriptionKey: "the server's page did not go, and no page asking for a name came"])
        }
        if name.isHittable { name.tap() }
        app.typeText(user + "\n")
    }

    /// What a tap names on the screen as it is, ready (`a|b`: whichever is
    /// there): one that can be touched first, else one that is there, for
    /// XCUITest's tap to scroll to as it would. Not waited for: the screen
    /// said it was ready, so what it shows is all there is.
    private func find(_ app: XCUIApplication, _ label: String, step: Shot.Step) -> XCUIElement {
        let each = label.split(separator: "|").map(String.init)
        var there: XCUIElement?
        for one in each {
            let c = step.where == "bottom" ? lowest(app, one) : control(app, one, role: step.role)
            if c.exists {
                if c.isHittable { return c }
                there = there ?? c
            }
        }
        return there ?? app.descendants(matching: .any)[each[0]].firstMatch
    }

    /// Of every element that says `label`, the lowest on the screen, once
    /// one is there: a mini player saying the title a row above it says.
    private func lowest(_ app: XCUIApplication, _ label: String) -> XCUIElement {
        let all = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@ OR identifier == %@", label, label))
        return all.allElementsBoundByIndex.filter { $0.isHittable }.max { $0.frame.maxY < $1.frame.maxY } ?? all.firstMatch
    }

    /// What says `label`, or has it as its identifier: of the `role` asked
    /// for when one is; else a tab, else a button, else any element.
    private func control(_ app: XCUIApplication, _ label: String, role: String?) -> XCUIElement {
        switch role {
        case "tab":
            // An iPad's tabs are a bar at the top, or a sidebar, which may not
            // be a tab bar to XCUITest: then the button that says it. The
            // iPhone's, a tab bar's alone, as ever.
            let tab = app.tabBars.buttons[label]
            guard UIDevice.current.userInterfaceIdiom == .pad, !tab.exists else { return tab }
            return app.buttons[label].firstMatch
        case "button": return app.buttons[label].firstMatch
        case "field": return app.textFields[label].firstMatch
        case "text": return app.staticTexts[label].firstMatch
        default:
            let tab = app.tabBars.buttons[label]
            if tab.exists { return tab }
            // Of several, one that can be touched: the full player's Pause, not
            // the mini player's under it.
            let buttons = app.buttons.matching(NSPredicate(format: "label == %@ OR identifier == %@", label, label))
            if let b = buttons.allElementsBoundByIndex.first(where: { $0.isHittable }) { return b }
            let button = buttons.firstMatch
            return button.exists ? button : app.descendants(matching: .any)[label].firstMatch
        }
    }

    private func value(_ name: String) throws -> String {
        guard let v = env[name], !v.isEmpty else { throw NSError(domain: "Drive", code: 1, userInfo: [NSLocalizedDescriptionKey: "\(name) is not set"]) }
        return v
    }
}
