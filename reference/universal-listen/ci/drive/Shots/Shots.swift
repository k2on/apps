// Listen's driven shots: the ones simctl/open cannot take alone. A UI test
// turns the iPad on its side, and opens menus, context menus and submenus
// by acting on the installed app, then writes the screen. From the
// environment (TEST_RUNNER_<NAME>):
//   SHOTS_BUNDLE       the installed app's bundle id
//   SHOTS_LIST         lines "<name>|<argument> …|<step> …" (no argument may hold a space)
//   SHOTS_OUT          directory on the Mac the PNGs are written to
//   SHOTS_ORIENTATION  portrait (default) or landscape (iOS)
// A step is "<verb>:<query>", the query an accessibility identifier, label
// or title, with "_" for a space:
//   tap      tap (click on the Mac)
//   context  long-press (right-click on the Mac): a context menu
//   hover    open a submenu: hover on the Mac, tap on iOS
//   wait     sleep that many seconds
import XCTest

final class Shots: XCTestCase {
    func testShots() throws {
        let env = ProcessInfo.processInfo.environment
        let app = XCUIApplication(bundleIdentifier: env["SHOTS_BUNDLE"]!)
        let out = URL(fileURLWithPath: env["SHOTS_OUT"]!)
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        #if os(iOS)
        XCUIDevice.shared.orientation = env["SHOTS_ORIENTATION"] == "landscape" ? .landscapeLeft : .portrait
        #endif
        for line in env["SHOTS_LIST"]!.split(separator: "\n") {
            let parts = line.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
            guard parts.count >= 2 else { continue }
            app.terminate()
            app.launchArguments = parts[1].split(separator: " ").map(String.init)
            app.launch()
            _ = app.wait(for: .runningForeground, timeout: 15)
            var failed: [String] = []
            if parts.count > 2, !parts[2].isEmpty {
                sleep(3)
                for step in parts[2].split(separator: " ").map(String.init) {
                    if !perform(step, in: app) { failed.append(step) }
                    sleep(1)
                }
            }
            try settled().pngRepresentation.write(to: out.appendingPathComponent("\(parts[0]).png"))
            if !failed.isEmpty {
                print("SHOT \(parts[0]): steps failed: \(failed.joined(separator: " "))")
                try? ("failed: \(failed)\n\n" + app.debugDescription)
                    .write(to: out.appendingPathComponent("\(parts[0]).tree.txt"), atomically: true, encoding: .utf8)
            }
        }
        app.terminate()
        #if os(iOS)
        XCUIDevice.shared.orientation = .portrait
        #endif
    }

    private func element(_ query: String, in app: XCUIApplication) -> XCUIElement {
        let q = query.replacingOccurrences(of: "_", with: " ")
        let p = NSPredicate(format: "identifier == %@ OR label == %@ OR title == %@", q, q, q)
        return app.descendants(matching: .any).matching(p).firstMatch
    }

    private func perform(_ step: String, in app: XCUIApplication) -> Bool {
        let kv = step.split(separator: ":", maxSplits: 1).map(String.init)
        guard kv.count == 2 else { return false }
        if kv[0] == "wait" {
            sleep(UInt32(kv[1]) ?? 1)
            return true
        }
        let e = element(kv[1], in: app)
        guard e.waitForExistence(timeout: 8) else { return false }
        switch kv[0] {
        case "tap":
            #if os(macOS)
            e.click()
            #else
            e.tap()
            #endif
        case "context":
            #if os(macOS)
            e.rightClick()
            #else
            e.press(forDuration: 1.2)
            #endif
        case "hover":
            #if os(macOS)
            e.hover()
            #else
            e.tap()
            #endif
        default:
            return false
        }
        return true
    }

    /// Two seconds, then the same picture three times half a second apart
    /// (or whatever is there after eight seconds).
    private func settled(limit: Double = 8) -> XCUIScreenshot {
        sleep(2)
        var last = XCUIScreen.main.screenshot()
        var same = 0
        let until = Date().addingTimeInterval(limit - 2)
        while Date() < until {
            usleep(500_000)
            let now = XCUIScreen.main.screenshot()
            same = now.pngRepresentation == last.pngRepresentation ? same + 1 : 0
            last = now
            if same == 2 { break }
        }
        return last
    }
}
