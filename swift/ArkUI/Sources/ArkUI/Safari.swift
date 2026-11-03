// A web page in Safari's own view (`SFSafariViewController`), for a page
// that is somebody else's to finish and never comes back to the app — a
// bank's login on its provider's page, say — where the browser sheet of a
// sign-in (`ASWebAuthenticationSession`) would wait for a redirect that
// never comes. It has Safari's Done, its address bar and its AutoFill;
// presented full screen, as Safari's view expects to be, and closed by
// whoever presents it: on Done (`done`), or when the app learns the page's
// work is over. Nothing of any app.
#if os(iOS)
import SafariServices
import SwiftUI

public struct SafariView: UIViewControllerRepresentable {
    let url: URL
    let tint: Color?
    let done: () -> Void

    /// `url` in Safari's view, its buttons in `tint`; `done` told when its Done is tapped.
    public init(_ url: URL, tint: Color? = nil, done: @escaping () -> Void = {}) {
        self.url = url
        self.tint = tint
        self.done = done
    }

    public func makeUIViewController(context: Context) -> SFSafariViewController {
        let c = SFSafariViewController(url: url)
        c.dismissButtonStyle = .done
        if let tint { c.preferredControlTintColor = UIColor(tint) }
        c.delegate = context.coordinator
        return c
    }

    public func updateUIViewController(_ controller: SFSafariViewController, context: Context) {
        context.coordinator.done = done
    }

    public func makeCoordinator() -> Coordinator { Coordinator(done) }

    public final class Coordinator: NSObject, SFSafariViewControllerDelegate {
        var done: () -> Void
        init(_ done: @escaping () -> Void) { self.done = done }
        public func safariViewControllerDidFinish(_ controller: SFSafariViewController) { done() }
    }
}

extension View {
    /// `url`, whenever there is one, in Safari's view over everything:
    /// `closed` told when its Done is tapped, and setting `url` to none
    /// closes it.
    public func safari(_ url: URL?, tint: Color? = nil, closed: @escaping () -> Void) -> some View {
        fullScreenCover(isPresented: Binding(get: { url != nil }, set: { if !$0 { closed() } })) {
            if let url {
                SafariView(url, tint: tint, done: closed).ignoresSafeArea()
            }
        }
    }
}
#endif
