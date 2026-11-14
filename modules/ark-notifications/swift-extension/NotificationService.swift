// ark-notifications' Notification Service Extension (README.md): generated
// into an app's project only when its app.toml says `[background]
// service-extension = true`, as a target of its own.
//
// A separate process by Apple's design: the system starts it, alone, for
// each alert push marked `mutable-content`, gives it about 30 seconds and
// about 24 MB, and shows whatever it hands back. A push is a wake with the
// app's generic words ("New activity in Money") and nothing of its data, so
// this is where those words are swapped for the device's own: what the
// app's runtime last worked out from its replica for a notice and left in
// their App Group (`group.<the app's bundle id>`, the key `ark.notice`: a
// JSON object with `title` and `body`). With nothing left, the generic
// words stand. It links nothing of the app's: not its library, not ArkUI,
// so it cannot reach the app's process, runtime or replica, and never
// syncs itself (README.md says what that would take).
import Foundation
import UserNotifications

final class NotificationService: UNNotificationServiceExtension {
    private var handler: ((UNNotificationContent) -> Void)?
    private var content: UNMutableNotificationContent?

    override func didReceive(_ request: UNNotificationRequest, withContentHandler handler: @escaping (UNNotificationContent) -> Void) {
        self.handler = handler
        let content = (request.content.mutableCopy() as? UNMutableNotificationContent) ?? UNMutableNotificationContent()
        self.content = content
        if request.content.userInfo["ark"] != nil, let notice = Self.left() {
            content.title = notice.title
            content.body = notice.body
        }
        handler(content)
    }

    /// The system's time is up: what there is, shown.
    override func serviceExtensionTimeWillExpire() {
        if let handler, let content { handler(content) }
    }

    /// The App Group the app and this share: `group.<the app's bundle id>`,
    /// this one's id being the app's with `.notices` after it.
    static var group: String {
        let mine = Bundle.main.bundleIdentifier ?? ""
        let app = mine.hasSuffix(".notices") ? String(mine.dropLast(".notices".count)) : mine
        return "group.\(app)"
    }

    /// What the app's runtime left for the next push: its title and body.
    static func left() -> (title: String, body: String)? {
        guard let text = UserDefaults(suiteName: group)?.string(forKey: "ark.notice"),
              let any = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: String],
              let title = any["title"], let body = any["body"] else { return nil }
        return (title, body)
    }
}
