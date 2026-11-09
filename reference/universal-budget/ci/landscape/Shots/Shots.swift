// Budget's iPad landscape shots. From the environment (TEST_RUNNER_<NAME>):
//   SHOTS_BUNDLE  the installed app's bundle id
//   SHOTS_LIST    lines "<name>|<argument> <argument> …" (no quoting: no
//                 argument may hold a space)
//   SHOTS_OUT     directory on the Mac the PNGs are written to
import XCTest

final class Shots: XCTestCase {
    func testLandscape() throws {
        let env = ProcessInfo.processInfo.environment
        let app = XCUIApplication(bundleIdentifier: env["SHOTS_BUNDLE"]!)
        let out = URL(fileURLWithPath: env["SHOTS_OUT"]!)
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        XCUIDevice.shared.orientation = .landscapeLeft
        for line in env["SHOTS_LIST"]!.split(separator: "\n") {
            let parts = line.split(separator: "|", maxSplits: 1).map(String.init)
            guard parts.count == 2 else { continue }
            app.terminate()
            app.launchArguments = parts[1].split(separator: " ").map(String.init)
            app.launch()
            _ = app.wait(for: .runningForeground, timeout: 10)
            try settled().pngRepresentation.write(to: out.appendingPathComponent("\(parts[0]).png"))
            try? app.debugDescription.write(to: out.appendingPathComponent("\(parts[0]).tree.txt"), atomically: true, encoding: .utf8)
        }
        app.terminate()
        XCUIDevice.shared.orientation = .portrait
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
