// Navigating, by the same names on every platform: `push`, `replace`,
// `back`, `reset` and `handle` — this one over a `NavigationStack`'s path,
// Android's over Compose's back stack (ui/kotlin), the desktop's over its
// address bar's history (ui/rust/arkui's `navigator`). A route is the app's
// domain's `Route` (`harken_domain::routes`, `money_domain::routes`), whose
// one spelling is its path; the back stack is a list of them.
//
// A value, so a tab's stack is `@State` and a `NavigationStack(path:)`
// binds straight to its `path`: the system's swipe back and its own back
// button pop it as they always did.
import Foundation

public struct Navigator<Route: Hashable>: Equatable {
    /// The back stack above the root, the root first.
    public var path: [Route]

    public init(_ path: [Route] = []) {
        self.path = path
    }

    /// Navigate to `route`, a new entry on the back stack.
    public mutating func push(_ route: Route) {
        path.append(route)
    }

    /// Navigate to `route` in place of the top entry (at the root, pushed).
    public mutating func replace(_ route: Route) {
        if path.isEmpty { path = [route] } else { path[path.count - 1] = route }
    }

    /// Back one entry; at the root, nowhere.
    public mutating func back() {
        if !path.isEmpty { path.removeLast() }
    }

    /// Start again at the root, or at `route` above it.
    public mutating func reset(_ route: Route? = nil) {
        path = route.map { [$0] } ?? []
    }

    /// A link the app was opened with (`harken://album/Water%20Music`): the
    /// route `parse` reads from its path is offered to `take` first — a
    /// sign-in link is the onboarding's, not a page — and navigated to if
    /// `take` does not take it. `false` for a link of another scheme, or one
    /// whose path names nothing here (the sign-in's own `auth?code=…`).
    @discardableResult
    public mutating func handle(_ url: URL, scheme: String, parse: (String) -> Route?, take: (Route) -> Bool = { _ in false }) -> Bool {
        guard let route = Navigator.route(of: url, scheme: scheme, parse: parse) else { return false }
        if !take(route) { push(route) }
        return true
    }

    /// The route a link of `scheme` names: its path is everything after
    /// `scheme://`, query and all.
    public static func route(of url: URL, scheme: String, parse: (String) -> Route?) -> Route? {
        let link = url.absoluteString
        let prefix = scheme + "://"
        guard url.scheme == scheme, link.hasPrefix(prefix) else { return nil }
        return parse(String(link.dropFirst(prefix.count)))
    }
}
