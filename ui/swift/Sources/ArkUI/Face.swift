// Who is signed in, as a face: their initials on the accent, and their
// picture over them once it has arrived. One face for every app — money's
// account face and harken's draw this, the same size in the same place —
// so a person looks the same whichever app they opened. The picture
// (`Pictures.url`, the server's `/auth/picture`) is fetched once a launch
// and kept; it answers anybody, so no login goes with it.
import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// How large the account's face is, at the end of a tab's title, in every app.
public let faceSide: CGFloat = 32

/// Who is signed in, round, `side` across: their picture (`picture`, a
/// whole URL, "" for none) once it has loaded, cropped to fill, over their
/// initials on the accent. It grows with the person's text size, its
/// initials with it, up to half as large again.
public struct Face: View {
    let name: String
    let picture: String
    var side: CGFloat
    @ScaledMetric(relativeTo: .body) private var grows: CGFloat = 1

    public init(name: String, picture: String, side: CGFloat = faceSide) {
        self.name = name
        self.picture = picture
        self.side = side
    }

    public var body: some View {
        let side = self.side * min(grows, 1.5)
        return ZStack {
            Circle().fill(Color.accent)
            Text(letters)
                .font(.system(size: side * 0.4, weight: .semibold))
                .foregroundStyle(Color.onAccent)
            if let url = URL(string: picture), !picture.isEmpty {
                FacePicture(url: url)
            }
        }
        .frame(width: side, height: side)
        .clipShape(Circle())
    }

    // The initials, or the name's first character when it has no letters ("?").
    private var letters: String {
        let l = Pictures.initials(name)
        return l.isEmpty ? String(name.prefix(1)) : l
    }
}

/// A face's picture, fetched once a launch and kept: every toolbar's face
/// (a tab's each) is the same picture, and fetching it again for each, as
/// `AsyncImage` did, held each new tab's screen for a second or two
/// (app-screens 43318181211's walk, "face …/auth/picture").
private struct FacePicture: View {
    let url: URL
    @State private var image: Image? = nil
    @State private var done = false

    @MainActor private static var kept: [URL: Image] = [:]

    var body: some View {
        ZStack { (image ?? Self.kept[url])?.resizable().scaledToFill() }
            // Not ready for a screenshot until it has come, or failed.
            .readyWhile(Self.kept[url] == nil && !done, "face \(url.absoluteString)")
            .task(id: url) {
                if let k = Self.kept[url] {
                    image = k
                    done = true
                    return
                }
                if let r = try? await URLSession.shared.data(from: url), let i = Self.image(r.0) {
                    Self.kept[url] = i
                    image = i
                }
                done = true
            }
    }

    private static func image(_ data: Data) -> Image? {
        #if canImport(UIKit)
        return UIImage(data: data).map { Image(uiImage: $0) }
        #elseif canImport(AppKit)
        return NSImage(data: data).map { Image(nsImage: $0) }
        #else
        return nil
        #endif
    }
}

extension AsyncImagePhase {
    /// Whether it is still on its way: neither come nor failed.
    var isLoading: Bool {
        if case .empty = self { return true }
        return false
    }
}
