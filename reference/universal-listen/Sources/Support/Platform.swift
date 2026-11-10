import SwiftUI

extension EnvironmentValues {
    /// True in a compact-width iOS scene (iPhone, narrow iPad multitasking).
    /// The one width fact the app uses; the Mac is never compact.
    @Entry var isCompact: Bool = false
}

struct CompactReader: ViewModifier {
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    @ViewBuilder func body(content: Content) -> some View {
        content.environment(\.isCompact, sizeClass == .compact)
    }
    #else
    @ViewBuilder func body(content: Content) -> some View { content }
    #endif
}

extension View {
    @ViewBuilder func inlineTitle() -> some View {
        #if os(iOS)
        navigationBarTitleDisplayMode(.inline)
        #else
        self
        #endif
    }

    @ViewBuilder func largeTitle() -> some View {
        #if os(iOS)
        navigationBarTitleDisplayMode(.large)
        #else
        self
        #endif
    }

    /// Grouped list on iOS (inset grouped), the sidebar-adjacent inset style on the Mac.
    @ViewBuilder func groupedList() -> some View {
        #if os(iOS)
        listStyle(.insetGrouped)
        #else
        listStyle(.inset)
        #endif
    }
}

enum Format {
    static func clock(_ t: TimeInterval) -> String {
        let s = max(0, Int(t.rounded()))
        if s >= 3600 { return String(format: "%d:%02d:%02d", s / 3600, (s % 3600) / 60, s % 60) }
        return String(format: "%d:%02d", s / 60, s % 60)
    }

    static func minutes(_ t: TimeInterval) -> String {
        let m = Int((t / 60).rounded())
        if m >= 60 { return m % 60 == 0 ? "\(m / 60) hr" : "\(m / 60) hr \(m % 60) min" }
        return "\(max(1, m)) min"
    }

    static func remaining(_ e: Episode) -> String {
        if e.progress >= 1 { return "Played" }
        if e.progress > 0 { return "\(minutes(e.duration * (1 - e.progress))) left" }
        return minutes(e.duration)
    }

    static func day(_ date: Date) -> String {
        let cal = Calendar(identifier: .gregorian)
        let days = cal.dateComponents([.day], from: cal.startOfDay(for: date), to: cal.startOfDay(for: Launch.now)).day ?? 0
        switch days {
        case 0: return "Today"
        case 1: return "Yesterday"
        case 2..<7: return date.formatted(.dateTime.weekday(.wide))
        default: return date.formatted(.dateTime.month(.abbreviated).day())
        }
    }

    static func total(_ tracks: [Track]) -> String {
        let n = tracks.count
        return "\(n) \(n == 1 ? "song" : "songs"), \(minutes(tracks.reduce(0) { $0 + $1.duration }))"
    }

    static func rate(_ r: Double) -> String {
        r == r.rounded() ? "\(Int(r))×" : "\(r.formatted(.number.precision(.fractionLength(0...2))))×"
    }
}

/// A press that settles with a spring, or without motion when reduced.
struct PressableStyle: ButtonStyle {
    var scale: CGFloat = 0.9
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .contentShape(Rectangle())
            .scaleEffect(configuration.isPressed && !reduceMotion ? scale : 1)
            .opacity(configuration.isPressed ? 0.7 : 1)
            .animation(reduceMotion ? nil : .spring(response: 0.25, dampingFraction: 0.7), value: configuration.isPressed)
    }
}

extension Animation {
    /// The app's one transition curve for panels and the player.
    static let listen = Animation.spring(response: 0.42, dampingFraction: 0.86)
}

extension View {
    /// Animates `value` with the app's spring, or not at all with Reduce Motion.
    func listenAnimation<V: Equatable>(_ value: V) -> some View { modifier(ListenAnimation(value: value)) }
}

private struct ListenAnimation<V: Equatable>: ViewModifier {
    let value: V
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ViewBuilder func body(content: Content) -> some View {
        content.animation(reduceMotion ? .easeInOut(duration: 0.15) : .listen, value: value)
    }
}

// MARK: Carrying the model into detached content

/// The app's model objects, handed on explicitly. Content SwiftUI hosts
/// apart from the view tree — Mac table cells, popovers, menus — does not
/// always inherit the environment's Observable objects.
struct ListenModel: ViewModifier {
    let library: Library
    let player: Player
    let nav: Navigator

    func body(content: Content) -> some View {
        content.environment(library).environment(player).environment(nav)
    }
}

/// A context menu whose items see the model.
private struct ListenContextMenu<Items: View>: ViewModifier {
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav
    let items: () -> Items

    func body(content: Content) -> some View {
        content.contextMenu {
            items().modifier(ListenModel(library: library, player: player, nav: nav))
        }
    }
}

extension View {
    func listenContextMenu<M: View>(@ViewBuilder _ items: @escaping () -> M) -> some View {
        modifier(ListenContextMenu(items: items))
    }
}

/// A pull-down menu whose items see the model.
struct ListenMenu<Items: View, MenuLabel: View>: View {
    @ViewBuilder let items: () -> Items
    @ViewBuilder let label: () -> MenuLabel
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        Menu {
            items().modifier(ListenModel(library: library, player: player, nav: nav))
        } label: {
            label()
        }
    }
}
