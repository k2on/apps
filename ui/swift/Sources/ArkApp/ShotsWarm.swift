// What a cold simulator does once, done before a screenshot walk starts
// timing (ui/docs/shots.md, "In the App"): the first alert's labels made
// data detectors and the language identifier (about 2.3 s in the iPad's
// profile), the first text field the system's text effects window and its
// feature flags (about 1.5 s), and the asset catalog's lookups and PNG
// decoding; each had landed inside a shot, the iPhone's account and the
// iPad's Accounts waiting 10 to 15 s on one block of the main thread
// (app-screens 43486700398). Here they are done once, out of sight and out
// of every picture, and the walk's log says how long it took. A walk's
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

    /// An alert with a text field, presented and put away in a window of its
    /// own under the app's (never drawn: a picture is taken after it has
    /// gone, and the window with it): the alert's labels, the field's text
    /// effects window and the keyboard's machinery made once.
    private static func alert() async -> String? {
        guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first else { return "no scene" }
        let window = UIWindow(windowScene: scene)
        window.windowLevel = UIWindow.Level(rawValue: UIWindow.Level.normal.rawValue - 1)
        window.rootViewController = UIViewController()
        window.isHidden = false
        defer {
            window.endEditing(true)
            window.isHidden = true
            window.rootViewController = nil
        }
        let alert = UIAlertController(title: "Ready", message: "Every dollar given a job, 1 Jan, 555-0100.", preferredStyle: .alert)
        alert.addTextField { $0.placeholder = "Name" }
        alert.addAction(UIAlertAction(title: "OK", style: .default))
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            window.rootViewController?.present(alert, animated: false) { done.resume() }
        }
        alert.textFields?.first?.becomeFirstResponder()
        alert.textFields?.first?.resignFirstResponder()
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            alert.dismiss(animated: false) { done.resume() }
        }
        return nil
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

    /// The app's own images decoded once (its bundle's PNGs and JPEGs, its
    /// icon) and the symbol catalog opened, as the screens will want them.
    private static func images() -> String? {
        var n = 0
        let paths = ["png", "jpg", "jpeg"].flatMap { Bundle.main.paths(forResourcesOfType: $0, inDirectory: nil) }
        for p in paths {
            if let i = UIImage(contentsOfFile: p)?.preparingForDisplay() {
                _ = i.size
                n += 1
            }
        }
        for name in ["AppIcon", "AccentColor"] {
            if let i = UIImage(named: name)?.preparingForDisplay() {
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
