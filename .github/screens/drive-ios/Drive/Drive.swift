// An installed app driven as a person would, for its screenshots
// (../../apps.md, ../../app-ios.sh), the same for every app: signed in
// through arkui's onboarding to the server CI started, if it is not
// already, then each shot: the app opened, its steps done, the screen
// written as a PNG. What to do comes from the environment, as app-ios.sh
// passes it (`TEST_RUNNER_<NAME>` to xcodebuild):
//
//   APP_BUNDLE  the app's bundle id
//   APP_SERVER  the server's address, typed into the first page; unset, none
//   APP_USER    the name the server's dev sign-in page is given
//   APP_SHOTS   the shots, as the app's screens/shots.json has them:
//               [{"id": …, "steps": [{"tap": label, "role"?: …} | {"type": text} | {"wait": seconds}]}]
//               A tap's label is what the element says or its accessibility
//               identifier; `role` ("tab", "button", "field", "text") says
//               which kind it is when several say it.
//   APP_OUT     the directory the PNGs are written to, on the Mac; beside each,
//               the app's accessibility tree as it was taken (<id>.tree.txt),
//               which is what a step can name
import XCTest

/// A shot, as shots.json has it.
struct Shot: Decodable {
    struct Step: Decodable {
        var tap: String?
        var role: String?
        var `where`: String?
        var unless: String?
        var type: String?
        var wait: Double?
    }

    var id: String
    var steps: [Step] = []

    enum CodingKeys: String, CodingKey { case id, steps }

    init(from d: Decoder) throws {
        let c = try d.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        steps = try c.decodeIfPresent([Step].self, forKey: .steps) ?? []
    }
}

final class Drive: XCTestCase {
    private let env = ProcessInfo.processInfo.environment
    private let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")

    func testShots() throws {
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: try value("APP_BUNDLE"))
        let out = URL(fileURLWithPath: try value("APP_OUT"))
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let shots = try JSONDecoder().decode([Shot].self, from: Data(try value("APP_SHOTS").utf8))

        app.terminate()
        app.launch()
        if let server = env["APP_SERVER"], !server.isEmpty {
            // The server's page: its one text field, whatever an app labels it.
            let field = app.textFields.firstMatch
            if field.waitForExistence(timeout: 20) {
                do {
                    try signIn(app, field: field, server: server, user: try value("APP_USER"))
                } catch {
                    // What the app and the system showed instead, to see why.
                    let seen = "== the app\n\(app.debugDescription)\n\n== SpringBoard\n\(springboard.debugDescription)"
                    try? seen.write(to: out.appendingPathComponent("stuck.tree.txt"), atomically: true, encoding: .utf8)
                    throw error
                }
                XCTAssertTrue(field.waitForNonExistence(timeout: 60), "signed in, past the server's page")
                sleep(3)
            }
        }

        for shot in shots {
            app.terminate()
            app.launch()
            sleep(2)
            for step in shot.steps {
                // A step that is done already: what it opens is on the screen.
                if let done = step.unless, app.descendants(matching: .any).matching(NSPredicate(format: "label == %@ OR identifier == %@", done, done)).firstMatch.waitForExistence(timeout: 2) {
                    continue
                }
                if let label = step.tap {
                    let c = step.where == "bottom" ? lowest(app, label) : control(app, label, role: step.role)
                    if !c.waitForExistence(timeout: 30) {
                        try? app.debugDescription.write(to: out.appendingPathComponent("stuck.tree.txt"), atomically: true, encoding: .utf8)
                        XCTFail("\(label) is not there for \(shot.id); stuck.tree.txt is what was")
                        return
                    }
                    c.tap()
                }
                if let text = step.type { app.typeText(text) }
                if let seconds = step.wait { sleep(UInt32(seconds)) }
            }
            sleep(3)
            try XCUIScreen.main.screenshot().pngRepresentation.write(to: out.appendingPathComponent("\(shot.id).png"))
            try app.debugDescription.write(to: out.appendingPathComponent("\(shot.id).tree.txt"), atomically: true, encoding: .utf8)
        }
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
        field.typeText(server + "\n")
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
        // The server's page, in the system's web sheet. It is drawn by
        // Safari's view service, a process of its own, and nothing of that
        // process is touched: tapping its element makes XCUITest bring it up
        // as an app, which takes the sheet away and the sign-in with it (the
        // screen goes black). The page puts its field in focus itself
        // (`autofocus`), so the name is typed as the app's keyboard input,
        // then sent with Go; a field the app's own hierarchy shows is tapped
        // first.
        let seen = Date().addingTimeInterval(30)
        let field = app.webViews.textFields.firstMatch
        while Date() < seen && !field.exists && !app.keyboards.firstMatch.exists { sleep(1) }
        sleep(2)
        if field.exists && field.isHittable { field.tap() }
        app.typeText(user + "\n")
    }

    /// Of every element that says `label`, the lowest on the screen, once
    /// one is there: a mini player saying the title a row above it says.
    private func lowest(_ app: XCUIApplication, _ label: String) -> XCUIElement {
        let all = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@ OR identifier == %@", label, label))
        _ = all.firstMatch.waitForExistence(timeout: 30)
        return all.allElementsBoundByIndex.filter { $0.isHittable }.max { $0.frame.maxY < $1.frame.maxY } ?? all.firstMatch
    }

    /// What says `label`, or has it as its identifier: of the `role` asked
    /// for when one is; else a tab, else a button, else any element.
    private func control(_ app: XCUIApplication, _ label: String, role: String?) -> XCUIElement {
        switch role {
        case "tab": return app.tabBars.buttons[label]
        case "button": return app.buttons[label].firstMatch
        case "field": return app.textFields[label].firstMatch
        case "text": return app.staticTexts[label].firstMatch
        default:
            let tab = app.tabBars.buttons[label]
            if tab.exists { return tab }
            let button = app.buttons[label].firstMatch
            return button.exists ? button : app.descendants(matching: .any)[label].firstMatch
        }
    }

    private func value(_ name: String) throws -> String {
        guard let v = env[name], !v.isEmpty else { throw NSError(domain: "Drive", code: 1, userInfo: [NSLocalizedDescriptionKey: "\(name) is not set"]) }
        return v
    }
}
