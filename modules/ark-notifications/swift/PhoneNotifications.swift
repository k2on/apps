// ark-notifications on the iPhone, the iPad and the Mac
// (modules/ark-notifications/README.md): the `NativeNotifications` protocol
// UniFFI writes, over
//
// - `UNUserNotificationCenter`: a notice now, or at its time (a time
//   trigger), its id the request's so a second replaces the first; its
//   channel the thread it is grouped in; its buttons a category of their
//   own, each a background action, so pressing one never opens the app;
// - APNs: `registerForRemoteNotifications`, the token handed back by the
//   app delegate (`registered(token:)`, `failed(error:)`) with the bundle id
//   as the topic and the build's environment, and a push received in the
//   background handed on (`received(_:done:)`). A push is a wake and
//   nothing of the app's data: the server's generic words on screen, and
//   the runtime syncing and saying what came from its own rows.
//
// A tap opens the notice's route through the app's links: `<scheme>://<path>`,
// the scheme the app's Info.plist declares, opened as any link is, so the
// app's `onOpenURL` resolves it. The tap, a button and a push are each told
// to the process's listener; the process's runtime makes a wake-up of a
// button and a push, and says when it is done with it (`settle`).
//
// One per process (`shared`), made at launch by the app delegate so a tap
// that launched the app is not missed: the center's delegate must be set
// before launch finishes.
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
import UserNotifications
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

public final class PhoneNotifications: NSObject, NativeNotifications, UNUserNotificationCenterDelegate, @unchecked Sendable {
    /// The process's one.
    public static let shared = PhoneNotifications()

    private let center = UNUserNotificationCenter.current()
    private let lock = NSLock()
    private var listener: NotificationListener?
    /// What was told before a listener was set (a tap that launched the app).
    private var early = [NotificationEvent]()
    private var categories = Set<String>()

    /// How the process's runtime says it is done with a wake-up: called with
    /// the most it may take and what to do then — the process's own
    /// (`ArkProcess.settle`), unless a test sets another.
    public var settle: (_ budget: TimeInterval, _ done: @escaping () -> Void) -> Void = { budget, done in
        ArkProcess.shared().settle(budgetMs: UInt64(budget * 1000), done: Settled(done))
    }

    override private init() {
        super.init()
        center.delegate = self
    }

    // MARK: NativeNotifications

    public func schedule(notice: Notice) {
        let content = UNMutableNotificationContent()
        content.title = notice.title
        content.body = notice.body
        content.sound = .default
        var info: [String: Any] = ["ark.id": notice.id]
        if let opens = notice.opens { info["ark.opens"] = opens }
        content.userInfo = info
        if let channel = notice.channel { content.threadIdentifier = channel }
        if !notice.actions.isEmpty { content.categoryIdentifier = category(notice.actions) }
        var trigger: UNNotificationTrigger?
        if let at = notice.atMs {
            let wait = Double(at) / 1000 - Date().timeIntervalSince1970
            if wait > 1 { trigger = UNTimeIntervalNotificationTrigger(timeInterval: wait, repeats: false) }
        }
        center.add(UNNotificationRequest(identifier: notice.id, content: content, trigger: trigger))
        // Left for the Notification Service Extension, where the app has
        // one: the words the next wake is shown with in place of the
        // generic ones (swift-extension/).
        if let group = Bundle.main.bundleIdentifier.flatMap({ UserDefaults(suiteName: "group.\($0)") }),
           let json = try? JSONSerialization.data(withJSONObject: ["title": notice.title, "body": notice.body]) {
            group.set(String(data: json, encoding: .utf8), forKey: "ark.notice")
        }
    }

    public func cancel(id: String) {
        center.removePendingNotificationRequests(withIdentifiers: [id])
        center.removeDeliveredNotifications(withIdentifiers: [id])
    }

    public func authorize(reply: AuthorizeReply) {
        center.requestAuthorization(options: [.alert, .sound, .badge]) { granted, error in
            if let error, !granted { return reply.failed(why: error.localizedDescription) }
            reply.allowed(allowed: granted)
        }
    }

    public func registerPush() {
        DispatchQueue.main.async {
            #if canImport(UIKit)
            UIApplication.shared.registerForRemoteNotifications()
            #elseif canImport(AppKit)
            NSApplication.shared.registerForRemoteNotifications()
            #endif
        }
    }

    public func unregisterPush() {
        DispatchQueue.main.async {
            #if canImport(UIKit)
            UIApplication.shared.unregisterForRemoteNotifications()
            #elseif canImport(AppKit)
            NSApplication.shared.unregisterForRemoteNotifications()
            #endif
        }
        tell(.unregistered)
    }

    public func setListener(listener: NotificationListener?) {
        lock.lock()
        self.listener = listener
        let told = listener == nil ? [] : early
        if listener != nil { early = [] }
        lock.unlock()
        for e in told { listener?.heard(event: e) }
    }

    // MARK: What the app delegate hands on

    /// APNs' device token for this app.
    public func registered(token: Data) {
        let hex = token.map { String(format: "%02x", $0) }.joined()
        let topic = Bundle.main.bundleIdentifier ?? ""
        tell(.registered(endpoint: .apns(token: hex, topic: topic, sandbox: Self.development)))
    }

    public func failed(error: Error) {
        tell(.failed(why: error.localizedDescription))
    }

    /// A push the system woke the app for (`content-available`, or an alert
    /// while the app runs): a wake and nothing else, told, and `done` once
    /// the runtime has synced and said what came, or the system's half a
    /// minute is nearly up.
    public func received(_ userInfo: [AnyHashable: Any], done: @escaping (_ newData: Bool) -> Void) {
        guard Self.wake(userInfo) else { return done(false) }
        tell(.pushed)
        settle(25) { done(true) }
    }

    // MARK: UNUserNotificationCenterDelegate

    /// A notice while the app is in front is shown all the same.
    public func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list, .sound])
    }

    public func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        let request = response.notification.request
        let info = request.content.userInfo
        let id = (info["ark.id"] as? String) ?? request.identifier
        // A wake's own notice (the app's generic words) opens the app as it was.
        let opens = info["ark.opens"] as? String
        switch response.actionIdentifier {
        case UNNotificationDefaultActionIdentifier:
            if let opens { Self.open(opens) }
            tell(.opened(id: id, opens: opens))
            completionHandler()
        case UNNotificationDismissActionIdentifier:
            completionHandler()
        default:
            tell(.action(notice: id, action: response.actionIdentifier))
            settle(25) { completionHandler() }
        }
    }

    // MARK: Inside

    /// `e` told to the listener the app's runtime set; with none, to the
    /// process itself (`ArkProcess.notices`: a push or a button a wake, an
    /// endpoint the session's push row), and a tap kept until there is one.
    private func tell(_ e: NotificationEvent) {
        lock.lock()
        let l = listener
        var opened = false
        if case .opened = e { opened = true }
        if l == nil && opened { early.append(e) }
        lock.unlock()
        if let l { return l.heard(event: e) }
        if !opened { ArkProcess.shared().notices().heard(event: e) }
    }

    /// The category of a notice's buttons: one per set, registered once.
    private func category(_ actions: [NoticeAction]) -> String {
        let id = "ark." + actions.map(\.id).joined(separator: ".")
        lock.lock()
        let fresh = categories.insert(id).inserted
        let all = categories
        lock.unlock()
        if fresh {
            center.getNotificationCategories { [center] known in
                var set = known.filter { !$0.identifier.hasPrefix("ark.") || all.contains($0.identifier) }
                set.insert(UNNotificationCategory(
                    identifier: id,
                    actions: actions.map { UNNotificationAction(identifier: $0.id, title: $0.title, options: []) },
                    intentIdentifiers: []))
                center.setNotificationCategories(set)
            }
        }
        return id
    }

    /// Whether a push is an Ark server's wake: its `ark` key names an app.
    static func wake(_ info: [AnyHashable: Any]) -> Bool {
        ((info["ark"] as? [String: Any])?["wake"] as? String) != nil
    }

    /// `path` opened through the app's own links: `<scheme>://<path>`.
    static func open(_ path: String) {
        let types = Bundle.main.object(forInfoDictionaryKey: "CFBundleURLTypes") as? [[String: Any]]
        guard let scheme = (types?.first?["CFBundleURLSchemes"] as? [String])?.first,
              let url = URL(string: "\(scheme)://\(path.drop(while: { $0 == "/" }))") else { return }
        DispatchQueue.main.async {
            #if canImport(UIKit)
            UIApplication.shared.open(url)
            #elseif canImport(AppKit)
            NSWorkspace.shared.open(url)
            #endif
        }
    }

    /// Whether this build's pushes go through APNs' development environment:
    /// its provisioning profile says `aps-environment` `development`. A build
    /// with no profile (the App Store's) is production's.
    static let development: Bool = {
        #if canImport(UIKit)
        let profile = Bundle.main.url(forResource: "embedded", withExtension: "mobileprovision")
        #else
        let profile = Bundle.main.bundleURL.appendingPathComponent("Contents/embedded.provisionprofile")
        #endif
        guard let url = profile, let data = try? Data(contentsOf: url),
              let text = String(data: data, encoding: .isoLatin1),
              let key = text.range(of: "aps-environment</key>") else { return false }
        return text[key.upperBound...].prefix(80).contains("development")
    }()
}

/// A settle's end handed to a closure.
final class Settled: WakeDone, @unchecked Sendable {
    private let then: () -> Void
    init(_ then: @escaping () -> Void) { self.then = then }
    func finished(idle: Bool) { then() }
}
