// Every app on one simulator, both themes, in one run of the test, driven
// as a person would (../../apps.md, ../../apps-ios.sh): what to do is a plan,
// a JSON file on the Mac (`TEST_RUNNER_APP_PLAN`, apps-ios.sh writes it):
//
//   [{"bundle": …, "server"?: …, "user"?: …, "scheme"?: …, "out": <dir>,
//     "orientation"?: "portrait"|"landscape", "signedIn"?: true,
//     "links"?: true,
//     "shots": [{"id": …, "args"?: [a screen's],
//                "route"?: <path>, "fresh"?: true,
//                "steps": [{"tap": label, "role"?: …} | {"type": text} | {"await": label} | …]}]}]
//
// For each theme (the simulator's own appearance, which apps-ios.sh sets
// for a run of its own, `TEST_RUNNER_THEME`; both when none is said), each
// app in turn: launched once with `-shots YES` (ArkUI's `Shots`: no
// motion, and the ready signal), signed in through arkui's onboarding to
// the server CI started, if it is not already, then its shots one after
// another **in that one launch**, each from where the last left the app.
// **Nothing waits for a time.** The app says when its screen is ready
// (`ark.ready`, labelled `ready <generation> <route>`, ui/docs/shots.md);
// after anything the driver does it waits for a newer generation, then
// takes the screen.
//
// A shot is reached, in the running app, by its `args` (a screen's launch
// arguments) as a launch link (`<scheme>://?screen=…`, the browser's
// spelling, which the app's shell takes as a launch: everything it showed
// put back first, ui/docs/shots.md) where the app says it takes them
// (`links`), by its `route` as a link (`<scheme>://<route>`), and by its
// steps. The app is launched again only for a shot that says it needs a
// clean start (`fresh`), or whose `args` the app takes only at launch;
// `relaunch` in a step starts it again. Every wait
// has a limit, which only fails the shot.
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
// Says how long each shot took, and how it was reached (`Drive: …`).
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
    /// The app opens a screen's launch arguments by a link as it runs.
    var links: Bool?
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
    /// The screen's own launch arguments (shots.json's `screens`): a link
    /// in the running app where the app takes them so (`links`).
    var args: [String] = []
    /// Where it is, as the app's router spells it: opened as a link.
    var route: String?
    /// The app launched afresh for it: what an earlier shot left (a song
    /// playing, a sheet) would not do.
    var fresh = false

    enum CodingKeys: String, CodingKey { case id, steps, args, route, fresh }

    init(from d: Decoder) throws {
        let c = try d.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        steps = try c.decodeIfPresent([Step].self, forKey: .steps) ?? []
        args = try c.decodeIfPresent([String].self, forKey: .args) ?? []
        route = try c.decodeIfPresent(String.self, forKey: .route)
        fresh = try c.decodeIfPresent(Bool.self, forKey: .fresh) ?? false
    }
}

/// The longest a screen may take to say it is ready: a guard, never a pace.
private let readyLimit: TimeInterval = 60

final class Drive: XCTestCase {
    private let env = ProcessInfo.processInfo.environment
    private let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
    /// The apps launched in this theme's pass.
    private var launched: Set<String> = []
    /// The screen arguments each running app was last brought to (at its
    /// launch, or by a link since).
    private var screen: [String: [String]] = [:]
    /// How the shot being taken was reached, and what it took: said once
    /// it is taken.
    private var reached = ""

    func testShots() throws {
        let plan = try JSONDecoder().decode([Entry].self, from: Data(contentsOf: URL(fileURLWithPath: try value("APP_PLAN"))))
        var failed: [String] = []
        // The simulator's appearance is set by apps-ios.sh (`simctl ui`),
        // one run of this test a theme (`TEST_RUNNER_THEME`): set from here
        // alone, every app's dark pass drew light, byte for byte (money's
        // and harken's iPhone and iPad shots in 43131582873 through
        // 43159033586). Set here too; each shot checks the app drew in it.
        let only = env["THEME"].flatMap { $0.isEmpty ? nil : $0 }
        for (theme, appearance) in [("light", XCUIDevice.Appearance.light), ("dark", .dark)] where only == nil || only == theme {
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
        // Launched once for this theme: its first shot opens it afresh, as
        // the theme was set with the app wherever another left it.
        launched.remove(entry.bundle)
        for shot in entry.shots {
            do {
                let start = Date()
                try open(shot, in: app, entry: entry, out: out)
                let opened = Date()
                try ready(app)
                try drawn(app, in: theme, id: shot.id)
                try noBanner(shot.id)
                let isReady = Date()
                try picture(app, landscape: entry.orientation == "landscape", id: shot.id)
                    .write(to: out.appendingPathComponent("\(shot.id).png"))
                let pictured = Date()
                try app.debugDescription.write(to: out.appendingPathComponent("\(shot.id).tree.txt"), atomically: true, encoding: .utf8)
                let s = { (a: Date, b: Date) in String(format: "%.1fs", b.timeIntervalSince(a)) }
                print("Drive: \(entry.bundle) \(URL(fileURLWithPath: entry.out).lastPathComponent) \(theme) \(shot.id): \(reached), \(shot.steps.count) steps; open \(s(start, opened)), ready \(s(opened, isReady)), picture \(s(isReady, pictured)), tree \(s(pictured, Date())), all \(s(start, Date()))")
            } catch {
                try? app.debugDescription.write(to: out.appendingPathComponent("stuck.tree.txt"), atomically: true, encoding: .utf8)
                try? upright(XCUIScreen.main.screenshot().pngRepresentation, landscape: entry.orientation == "landscape")
                    .write(to: out.appendingPathComponent("stuck.png"))
                throw error
            }
        }
    }

    /// The app at `shot`, in the app as it runs where it can be: launched
    /// afresh only for the theme's first shot, a `fresh` one, or one whose
    /// screen arguments the app takes only at launch; else its route as a
    /// link where it has one (the same launch as its arguments, which the
    /// shell takes as one: shots.json's note), its screen arguments as a
    /// launch link (`links`) where it has none, or from where the last shot
    /// left it. One link, one wait: opening both the arguments and then the
    /// route was two links and two waits a shot (7 to 15 s of "open" in
    /// app-screens 43258586746, where its "ready" was 1.5). Then each step,
    /// each followed by the app's next ready moment.
    private func open(_ shot: Shot, in app: XCUIApplication, entry: Entry, out: URL) throws {
        let links = entry.links == true && entry.scheme?.isEmpty == false
        let relaunch = shot.fresh || !launched.contains(entry.bundle) || app.state != .runningForeground
            || (!links && !shot.args.isEmpty && screen[entry.bundle] != shot.args)
        let routed = shot.route.flatMap { link($0, scheme: entry.scheme) }
        if relaunch {
            launch(app, entry, args: shot.args)
            reached = shot.fresh ? "launched (fresh)" : "launched"
        } else if !shot.args.isEmpty, routed == nil, links, let url = link(query(shot.args), scheme: entry.scheme) {
            // Its screen as a launch link: what the last shot showed put back first.
            let g = try ready(app)
            app.open(url)
            try ready(app, after: g)
            screen[entry.bundle] = shot.args
            reached = "linked \(url.absoluteString)"
        } else {
            reached = "continued"
        }
        // Where the router can take it: in the app as it runs, no relaunch;
        // not again after a launch that took its arguments already.
        if let url = routed, let route = shot.route, !(relaunch && !shot.args.isEmpty) {
            let g = try ready(app)
            app.open(url)
            try ready(app, after: g)
            if !shot.args.isEmpty { screen[entry.bundle] = shot.args }
            reached += ", routed \(route)"
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
                launch(app, entry, args: screen[entry.bundle] ?? [])
            }
            if let text = step.type {
                app.typeText(text)
                try ready(app, after: g)
            }
        }
    }

    /// The screen as the app is laid out, checked. On its side, not the
    /// app's own screenshot: XCUITest crops that from the screen's picture,
    /// which the simulator gives in its own portrait frame with the app
    /// drawn a quarter turn round, by the app's landscape frame, so it was a
    /// landscape canvas holding the turned picture's first 2064 rows and
    /// 688 black columns (every iPad landscape PNG of 43131582873, which
    /// the canvas's width let through). The whole screen is taken and
    /// turned upright instead, and the shot fails unless the app's window
    /// is wider than tall, the picture is too and of the window's shape, and
    /// what is past the picture's height is not all black.
    private func picture(_ app: XCUIApplication, landscape: Bool, id: String) throws -> Data {
        guard landscape else { return app.screenshot().pngRepresentation }
        // The turn XCUIDevice was asked for, waited for (a guard, not a pace).
        var window = app.windows.firstMatch.frame
        let turnBy = Date().addingTimeInterval(10)
        while window.width <= window.height, Date() < turnBy {
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
            window = app.windows.firstMatch.frame
        }
        let png = upright(XCUIScreen.main.screenshot().pngRepresentation, landscape: true)
        func fail(_ why: String) -> NSError {
            NSError(domain: "Drive", code: 7, userInfo: [NSLocalizedDescriptionKey: "\(id) is not on its side: \(why)"])
        }
        guard window.width > window.height else {
            throw fail("the app's window is \(window.width)x\(window.height)")
        }
        guard let cg = UIImage(data: png)?.cgImage else { throw fail("no picture") }
        let (w, h) = (cg.width, cg.height)
        guard w > h else { throw fail("the picture is \(w)x\(h)") }
        guard abs(CGFloat(w) * window.height - CGFloat(h) * window.width) <= CGFloat(h) * window.width * 0.01 else {
            throw fail("the picture is \(w)x\(h), the window \(window.width)x\(window.height)")
        }
        if allBlack(cg, from: h) { throw fail("its columns past \(h) are all black, a portrait picture in a landscape canvas") }
        return png
    }

    /// Whether every pixel of `cg` from column `x` on is black: the band a
    /// portrait picture leaves in a landscape canvas. A shot on its side
    /// has the status bar's battery there, in either theme.
    private func allBlack(_ cg: CGImage, from x: Int) -> Bool {
        let (w, h) = (cg.width - x, cg.height)
        guard w > 0, let strip = cg.cropping(to: CGRect(x: x, y: 0, width: w, height: h)) else { return false }
        var px = [UInt8](repeating: 0, count: w * h * 4)
        let drawn = px.withUnsafeMutableBytes { buf -> Bool in
            guard let ctx = CGContext(data: buf.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                      space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return false }
            ctx.draw(strip, in: CGRect(x: 0, y: 0, width: w, height: h))
            return true
        }
        guard drawn else { return false }
        for i in stride(from: 0, to: px.count, by: 4) where px[i] > 8 || px[i + 1] > 8 || px[i + 2] > 8 { return false }
        return true
    }

    /// The screen's picture turned the way the device is held: on its side
    /// (landscape left), the simulator's screen is in its own portrait
    /// frame with the app drawn a quarter turn round, and it is turned a
    /// quarter turn counter-clockwise here (the direction checked on
    /// 43131582873's PNGs: their status bar reads upright so).
    private func upright(_ png: Data, landscape: Bool) -> Data {
        guard landscape, let cg = UIImage(data: png)?.cgImage, cg.height > cg.width else { return png }
        let (w, h) = (CGFloat(cg.width), CGFloat(cg.height))
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let turned = UIGraphicsImageRenderer(size: CGSize(width: h, height: w), format: format).image { ctx in
            // (x, y) to (y, w - x): the top edge to the left, read upward.
            ctx.cgContext.translateBy(x: 0, y: w)
            ctx.cgContext.rotate(by: -.pi / 2)
            UIImage(cgImage: cg).draw(in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        return turned.pngData() ?? png
    }

    /// No system banner over the app when it is taken: a notification
    /// SpringBoard shows on its own (run 43083062235: "Ready for Apple
    /// Intelligence" over money's accounts) swiped away and waited out, a
    /// few times; one that stays fails the shot rather than be kept in it.
    private func noBanner(_ id: String) throws {
        let banner = springboard.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH 'NotificationShortLook'")).firstMatch
        for _ in 0..<3 where banner.exists {
            print("Drive: a system banner over \(id) (\(banner.label)); put away")
            banner.swipeUp()
            _ = banner.waitForNonExistence(timeout: 10)
        }
        if banner.exists {
            throw NSError(domain: "Drive", code: 6, userInfo: [NSLocalizedDescriptionKey: "a system banner stayed over \(id): \(banner.label)"])
        }
    }

    /// How a shot's `route` is handed to the app: the one place that says
    /// it, so a router's own spelling (a whole URL, `ark_router::Address`)
    /// is a change here alone. A route that is a URL already is taken as it
    /// is; a path is the app's scheme's.
    private func link(_ route: String, scheme: String?) -> URL? {
        if route.contains("://") { return URL(string: route) }
        // A launch link's query alone: the scheme's, with no path (`money://?screen=…`).
        guard let scheme, !scheme.isEmpty else { return nil }
        return URL(string: "\(scheme)://\(route)")
    }

    /// A screen's launch arguments as a link's query, the browser's
    /// spelling (`-screen category:dining -inspector 1` is
    /// `?screen=category%3Adining&inspector=1`), which ArkUI reads back
    /// (`Navigator.launchArgs`).
    private func query(_ args: [String]) -> String {
        var items: [URLQueryItem] = []
        var i = 0
        while i + 1 < args.count {
            items.append(URLQueryItem(name: String(args[i].drop { $0 == "-" }), value: args[i + 1]))
            i += 2
        }
        var c = URLComponents()
        c.queryItems = items
        // A colon is a URL's own; written out, so every reader reads one spelling.
        return "?" + (c.percentEncodedQuery ?? "").replacingOccurrences(of: ":", with: "%3A")
    }

    /// `app` launched afresh with `args`, in screenshot mode, and ready.
    private func launch(_ app: XCUIApplication, _ entry: Entry, args: [String]) {
        // From the home screen, so the status bar has no way back to the app
        // that was in front before (a "◂ Money" over harken's shots, run
        // 43083062235's dark pass, and over each app's first in 42977527249).
        // Home first, then the app stopped: a menu a shot left open (money's
        // Auto-Assign) or a song playing goes with the app to the
        // background, where the system puts the menu away and nothing is
        // drawn, rather than the app being stopped from the middle of it
        // (XCUITest's "Failed to get matching snapshots: Timed out while
        // evaluating UI query" ended a pass at the launch after each in
        // app-screens 43258586746).
        XCUIDevice.shared.press(.home)
        app.terminate()
        app.launchArguments = ["-shots", "YES"] + args
        app.launch()
        launched.insert(entry.bundle)
        screen[entry.bundle] = args
        _ = try? ready(app)
    }

    /// The theme the app says its window draws in (its ready signal's
    /// value, ArkUI's `Shots`) is the pass's: a dark pass that drew light
    /// fails here, not in a picture that looks like the light one.
    private func drawn(_ app: XCUIApplication, in theme: String, id: String) throws {
        let said = signal(app).value as? String
        guard said != theme else { return }
        throw NSError(domain: "Drive", code: 4, userInfo: [NSLocalizedDescriptionKey: "\(id) is drawn in \(said ?? "no theme it says"), not \(theme)"])
    }

    /// The app's ready signal at a generation after `after` (any, if nil),
    /// once it is there: its generation. Waited for as XCUITest waits for an
    /// element, up to `readyLimit`, which only fails.
    @discardableResult
    private func ready(_ app: XCUIApplication, after: Int? = nil) throws -> Int {
        let mark = signal(app)
        let until = Date().addingTimeInterval(readyLimit)
        while Date() < until {
            guard mark.waitForExistence(timeout: until.timeIntervalSinceNow) else { break }
            let label = mark.label
            let g = Int(label.split(separator: " ").dropFirst().first ?? "") ?? -1
            if g > (after ?? -1) { return g }
            // Ready, but at the generation before: wait for it to change.
            let changed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false OR label != %@", label), object: mark)
            _ = XCTWaiter.wait(for: [changed], timeout: max(until.timeIntervalSinceNow, 0))
        }
        throw NSError(domain: "Drive", code: 3, userInfo: [NSLocalizedDescriptionKey: "the app did not say it was ready (after generation \(after ?? -1)) within \(Int(readyLimit)) s"])
    }

    /// The app's ready signal: ArkUI's `Shots` draws it as a `Text`
    /// (ui/swift/Sources/ArkUI/Shots.swift), so a static text by its
    /// identifier, not every element of the tree searched for it, as
    /// `descendants(matching: .any)` did several times a shot.
    private func signal(_ app: XCUIApplication) -> XCUIElement {
        app.staticTexts.matching(identifier: "ark.ready").firstMatch
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
