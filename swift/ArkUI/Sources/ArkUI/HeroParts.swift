// A page about somebody, the way a record shop's catalogue opens one — the
// iPhone's twin of `rust/arkui/src/hero.rs` (`hero::banner`, `heading`,
// `entry`, `more`) and of Android's `HeroBanner`, `HeroHeading`,
// `HeroEntry` and `HeroMore`: their wide picture across the top with their
// name set on it, the page under it in sections each under a bold heading,
// a listing's entries over their hairlines, and the line that opens the
// rest of a list.
#if os(iOS)
import SwiftUI
import UIKit

/// Somebody's banner: their wide picture (`image`, once the app has it)
/// across the page, cropped to fill, run up to the top of the screen under
/// the status bar and that much taller, so what shows below the bar is the
/// shape `shape(width:)` says; `kind` in small type and their name in bold
/// on its lower edge, over a shade that keeps both legible on a white
/// collar or a white wig, and a lighter one at its top under the bar and
/// the back button. Until the picture arrives the ground is drawn, so the
/// name is already where it will be.
public struct HeroBanner: View {
    let image: UIImage?
    let kind: String
    let name: String

    public init(image: UIImage?, kind: String, name: String) {
        self.image = image
        self.kind = kind
        self.name = name
    }

    /// How tall a banner `width` wide is below the status bar: its own shape
    /// (2400×933) across a phone would be a strip, so a phone crops it to a
    /// taller frame, never past the desktop's tallest (`hero::TALLEST`,
    /// Android's `HeroBanner.height`).
    public static func shape(width: CGFloat) -> CGFloat {
        min(max(max(width / (2400 / 933), width * 0.82), 200), 486)
    }

    /// The status bar's height: the top of the window's own safe area, which
    /// is the bar's and not the navigation bar's.
    public static var statusBar: CGFloat { phone?.safeAreaInsets.top ?? 0 }

    /// Its whole height, the status bar it runs under included.
    public static var height: CGFloat { shape(width: phone?.bounds.width ?? 393) + statusBar }

    /// The window the app is drawn in, as UIKit has it.
    private static var phone: UIWindow? {
        UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first?.keyWindow
    }

    public var body: some View {
        let top = HeroBanner.statusBar
        // Words on a picture are white over a shade of the theme's scrim,
        // whatever the theme: the desktop's banner draws them the same.
        Color(uiColor: .secondarySystemBackground)
            .frame(maxWidth: .infinity)
            .frame(height: HeroBanner.height)
            .overlay {
                if let i = image { Image(uiImage: i).resizable().scaledToFill().accessibilityHidden(true) }
            }
            .overlay {
                LinearGradient(stops: [.init(color: Color.clear, location: 0.45), .init(color: Color.scrim.opacity(0.6), location: 1)],
                               startPoint: .top, endPoint: .bottom)
            }
            // Under the status bar's icons and the back button: a little shade.
            .overlay(alignment: .top) {
                LinearGradient(colors: [Color.scrim.opacity(0.35), Color.clear], startPoint: .top, endPoint: .bottom)
                    .frame(height: top + 72)
                    .allowsHitTesting(false)
            }
            .overlay(alignment: .bottomLeading) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(kind).font(.caption.weight(.semibold)).foregroundStyle(Color.white.opacity(0.85))
                    Text(name).font(.largeTitle.bold()).foregroundStyle(Color.white)
                        .lineLimit(3).minimumScaleFactor(0.6)
                }
                .padding(.horizontal, Space.page + 4)
                .padding(.bottom, Space.xl)
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(.isHeader)
            }
            .clipped()
    }
}

/// A section's heading on a page about somebody, bold.
public struct HeroHeading: View {
    let title: String

    public init(_ title: String) {
        self.title = title
    }

    public var body: some View {
        Text(title).font(.title2.weight(.semibold))
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Space.page)
            .padding(.top, Space.xl + 4)
            .padding(.bottom, Space.xs)
            .accessibilityAddTraits(.isHeader)
    }
}

/// One entry of a listing over its hairline: `picture` (a cover) at its
/// left when it has one, a line of small type over its title (`over`, who
/// it is by), a line under, a count at its right, and a chevron. The whole
/// entry opens it.
public struct HeroEntry<Picture: View>: View {
    let over: String
    let title: String
    let under: String
    let count: String
    let onOpen: () -> Void
    let picture: Picture

    public init(over: String = "", title: String, under: String = "", count: String = "", onOpen: @escaping () -> Void,
                @ViewBuilder picture: () -> Picture) {
        self.over = over
        self.title = title
        self.under = under
        self.count = count
        self.onOpen = onOpen
        self.picture = picture()
    }

    public var body: some View {
        Button(action: onOpen) {
            VStack(spacing: 0) {
                HStack(spacing: Space.m) {
                    picture
                    VStack(alignment: .leading, spacing: 2) {
                        if !over.isEmpty { Text(over).font(.caption2.weight(.semibold)).foregroundStyle(.quiet) }
                        Text(title).font(.body).foregroundStyle(.ink).multilineTextAlignment(.leading)
                        if !under.isEmpty { Text(under).font(.footnote).foregroundStyle(.quiet).lineLimit(2) }
                    }
                    Spacer(minLength: Space.s)
                    if !count.isEmpty { Text(count).font(.subheadline).foregroundStyle(.quiet).monospacedDigit() }
                    Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.outline)
                }
                .padding(.vertical, Space.m - 2)
                Divider()
            }
            // Inside the press, so a pressed row darkens the page's width.
            .padding(.horizontal, Space.page)
            .contentShape(Rectangle())
        }
        .buttonStyle(.pressRow)
    }
}

/// The line under a list that opens the rest of it — "See All 40 Works" —
/// in the accent, its chevron where an entry's is.
public struct HeroMore: View {
    let title: String
    let action: () -> Void

    public init(_ title: String, action: @escaping () -> Void) {
        self.title = title
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack {
                Text(title).foregroundStyle(Color.accentText)
                Spacer()
                Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.outline)
            }
            .padding(.vertical, Space.m)
            .padding(.horizontal, Space.page)
            .contentShape(Rectangle())
        }
        .buttonStyle(.pressRow)
    }
}
#endif
