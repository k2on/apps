// What a cold simulator does once, done before a screenshot walk starts
// timing (ui/docs/shots.md, "In the App"): the first alert's labels made
// data detectors and the language identifier (about 2.3 s in the iPad's
// profile), the first text field the system's text effects window and its
// feature flags (about 1.5 s), and the asset catalog's lookups and PNG
// decoding; each had landed inside a shot, the iPhone's account and the
// iPad's Accounts waiting 10 to 15 s on one block of the main thread
// (app-screens 43486700398). Here they are done once, after the first
// screen is up and before its picture, and the walk's log says how long it
// took; the screen is waited for again after. A walk's
// launch alone: nothing of it runs for anybody using the app.
#if canImport(UIKit)
import Foundation
#if canImport(NaturalLanguage)
import NaturalLanguage
#endif
import UIKit

@MainActor
enum ShotsWarm {
    /// The warm-up, and what it did, said: its parts and their times.
    static func up() async -> String {
        var parts: [String] = []
        func timed(_ name: String, _ f: () async -> String?) async {
            let t = Date()
            let said = await f()
            parts.append("\(name) \(Int(Date().timeIntervalSince(t) * 1000)) ms" + (said.map { " (\($0))" } ?? ""))
        }
        await timed("an alert with a text field") { await alert() }
        await timed("data detectors") { detectors() }
        await timed("the language identifier") { language() }
        await timed("images") { images() }
        return parts.joined(separator: ", ")
    }

    /// An alert with a text field, presented over what shows and put away
    /// again before anything is taken: the alert's labels, the field's text
    /// effects window and the keyboard's machinery made once. Each wait is a
    /// guard of two seconds, never a hang.
    private static func alert() async -> String? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        guard let window = scenes.flatMap(\.windows).first(where: \.isKeyWindow) ?? scenes.first?.windows.first,
              var top = window.rootViewController else { return "no window" }
        while let p = top.presentedViewController { top = p }
        let alert = UIAlertController(title: "Ready", message: "Every dollar given a job, 1 Jan, 555-0100.", preferredStyle: .alert)
        alert.addTextField { $0.placeholder = "Name" }
        alert.addAction(UIAlertAction(title: "OK", style: .default))
        top.present(alert, animated: false)
        var waits = 0
        while alert.viewIfLoaded?.window == nil && waits < 80 {
            try? await Task.sleep(nanoseconds: 25_000_000)
            waits += 1
        }
        let shown = alert.viewIfLoaded?.window != nil
        alert.textFields?.first?.becomeFirstResponder()
        alert.dismiss(animated: false)
        waits = 0
        while alert.presentingViewController != nil && waits < 80 {
            try? await Task.sleep(nanoseconds: 25_000_000)
            waits += 1
        }
        window.endEditing(true)
        return shown ? nil : "not shown"
    }

    /// The data detectors a label's text makes, made and run once.
    private static func detectors() -> String? {
        let types: NSTextCheckingResult.CheckingType = [.link, .phoneNumber, .date, .address]
        guard let d = try? NSDataDetector(types: types.rawValue) else { return "none" }
        let text = "Call 555-0100 on 1 January at 1 Main Street, or see https://example.com."
        return "\(d.matches(in: text, range: NSRange(text.startIndex..., in: text)).count) found"
    }

    /// The language identifier a label asks of its text, asked once.
    private static func language() -> String? {
        #if canImport(NaturalLanguage)
        let r = NLLanguageRecognizer()
        r.processString("Ready to assign: every dollar given a job.")
        return r.dominantLanguage?.rawValue
        #else
        return nil
        #endif
    }

    /// The app's own images decoded once (its bundle's PNGs and JPEGs) and
    /// the symbol catalog opened, as the screens will want them. Never the
    /// app icon by name: UIKit throws on it (app-screens 43604961847).
    private static func images() -> String? {
        var n = 0
        // Not the app icon's files (the system's, read by the home screen).
        let paths = ["png", "jpg", "jpeg"].flatMap { Bundle.main.paths(forResourcesOfType: $0, inDirectory: nil) }
            .filter { !($0 as NSString).lastPathComponent.hasPrefix("AppIcon") }
        for p in paths {
            if let i = UIImage(contentsOfFile: p)?.preparingForDisplay() {
                _ = i.size
                n += 1
            }
        }
        for s in ["chevron.left", "chevron.right", "plus", "magnifyingglass", "sidebar.trailing", "building.columns.fill"] {
            if UIImage(systemName: s) != nil { n += 1 }
        }
        return "\(n)"
    }
}
#endif
