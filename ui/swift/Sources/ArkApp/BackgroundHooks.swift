// What an app that declares `[background]` in its app.toml hands the
// library (ui/docs/app.md, Background And Notifications), and the app
// delegate that carries it out: the periodic task registered before launch
// finishes, as Apple requires (ark-background's `PhoneBackground`), the
// notifications' events told to the process's one runtime (a push or a
// notice's button a wake, an endpoint the session's push row), and APNs'
// token, its failure and a remote notification handed to ark-notifications'
// `PhoneNotifications`. An app that declares none has no `BackgroundHooks`,
// no delegate adaptor in its `@main`, and nothing of this runs: the app
// runs nothing in the background by default.
//
// The library names no module: the generated `@main` of an app that
// declares `[background]` says which of them it has (ui/prebuild.py).
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

public struct BackgroundHooks {
    /// At launch, before it finishes: the periodic task's handler
    /// registered over the process's runtime (`[background] sync`).
    public var launched: (ArkProcess) -> Void
    /// APNs' device token for this app (`[background] push`).
    public var token: (Data) -> Void
    /// Registering for pushes failed.
    public var failed: (Error) -> Void
    /// A remote notification: a wake of the process's runtime, `done`
    /// told whether there was anything new once it is idle.
    public var received: ([AnyHashable: Any], @escaping (Bool) -> Void) -> Void

    public init(launched: @escaping (ArkProcess) -> Void = { _ in },
                token: @escaping (Data) -> Void = { _ in },
                failed: @escaping (Error) -> Void = { _ in },
                received: @escaping ([AnyHashable: Any], @escaping (Bool) -> Void) -> Void = { _, done in done(false) }) {
        self.launched = launched
        self.token = token
        self.failed = failed
        self.received = received
    }

    /// What this app declares: set by its generated `@main`, before launch.
    nonisolated(unsafe) public static var app: BackgroundHooks? = nil
}

#if canImport(UIKit)
/// The app delegate of an app that declares `[background]`
/// (`@UIApplicationDelegateAdaptor(ArkAppDelegate.self)`).
public final class ArkAppDelegate: NSObject, UIApplicationDelegate {
    public func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        BackgroundHooks.app?.launched(ArkProcess.shared())
        return true
    }

    public func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        BackgroundHooks.app?.token(deviceToken)
    }

    public func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        BackgroundHooks.app?.failed(error)
    }

    public func application(_ application: UIApplication, didReceiveRemoteNotification userInfo: [AnyHashable: Any],
                            fetchCompletionHandler completionHandler: @escaping (UIBackgroundFetchResult) -> Void) {
        guard let bg = BackgroundHooks.app else { return completionHandler(.noData) }
        bg.received(userInfo) { new in completionHandler(new ? .newData : .noData) }
    }
}
#elseif canImport(AppKit)
/// The Mac's app delegate of an app that declares `[background]`
/// (`@NSApplicationDelegateAdaptor(ArkAppDelegate.self)`).
public final class ArkAppDelegate: NSObject, NSApplicationDelegate {
    public func applicationWillFinishLaunching(_ notification: Notification) {
        BackgroundHooks.app?.launched(ArkProcess.shared())
    }

    public func application(_ application: NSApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        BackgroundHooks.app?.token(deviceToken)
    }

    public func application(_ application: NSApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        BackgroundHooks.app?.failed(error)
    }

    public func application(_ application: NSApplication, didReceiveRemoteNotification userInfo: [String: Any]) {
        BackgroundHooks.app?.received(userInfo) { _ in }
    }
}
#endif
