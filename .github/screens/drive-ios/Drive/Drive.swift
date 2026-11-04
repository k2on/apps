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
//               [{"id": …, "steps": [{"tap": label} | {"type": text} | {"wait": seconds}]}]
//   APP_OUT     the directory the PNGs are written to, on the Mac
import XCTest

/// A shot, as shots.json has it.
struct Shot: Decodable {
    struct Step: Decodable {
        var tap: String?
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
                try signIn(app, field: field, server: server, user: try value("APP_USER"))
                XCTAssertTrue(field.waitForNonExistence(timeout: 60), "signed in, past the server's page")
                sleep(3)
            }
        }

        for shot in shots {
            app.terminate()
            app.launch()
            sleep(2)
            for step in shot.steps {
                if let label = step.tap {
                    let c = control(app, label)
                    XCTAssertTrue(c.waitForExistence(timeout: 30), "\(label) is there for \(shot.id)")
                    c.tap()
                }
                if let text = step.type { app.typeText(text) }
                if let seconds = step.wait { sleep(UInt32(seconds)) }
            }
            sleep(3)
            try XCUIScreen.main.screenshot().pngRepresentation.write(to: out.appendingPathComponent("\(shot.id).png"))
        }
    }

    /// The address typed and checked; the system's "wants to use … to sign
    /// in" allowed; the server's dev sign-in page given a name and sent.
    private func signIn(_ app: XCUIApplication, field: XCUIElement, server: String, user: String) throws {
        field.tap()
        field.typeText(server)
        app.buttons["Continue"].tap()
        let allow = springboard.buttons["Continue"]
        if allow.waitForExistence(timeout: 30) { allow.tap() }
        let name = try first(timeout: 30, [
            app.webViews.textFields.firstMatch,
            app.textFields["a name"],
            springboard.webViews.textFields.firstMatch,
        ])
        name.tap()
        name.typeText(user + "\n")
    }

    /// A tab, or any button, that says `label`.
    private func control(_ app: XCUIApplication, _ label: String) -> XCUIElement {
        let tab = app.tabBars.buttons[label]
        return tab.exists ? tab : app.buttons[label].firstMatch
    }

    private func first(timeout: TimeInterval, _ candidates: [XCUIElement]) throws -> XCUIElement {
        let until = Date().addingTimeInterval(timeout)
        while Date() < until {
            if let e = candidates.first(where: { $0.exists }) { return e }
            sleep(1)
        }
        throw NSError(domain: "Drive", code: 2, userInfo: [NSLocalizedDescriptionKey: "no sign-in page appeared"])
    }

    private func value(_ name: String) throws -> String {
        guard let v = env[name], !v.isEmpty else { throw NSError(domain: "Drive", code: 1, userInfo: [NSLocalizedDescriptionKey: "\(name) is not set"]) }
        return v
    }
}
