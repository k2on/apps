import SwiftUI

@main
struct BudgetApp: App {
    @State private var store = SampleData.makeStore()

    var body: some Scene {
        WindowGroup(id: "main") {
            ContentView()
                .environment(store)
                .environment(\.currencyCode, store.currencyCode)
                .appearanceFromSettings()
        }
        #if os(macOS)
        .defaultSize(LaunchOptions.current.windowSize ?? CGSize(width: 1200, height: 780))
        #endif
        .commands {
            BudgetCommands(store: store)
        }

        #if os(macOS)
        // On Mac, settings live in their own window under the app menu
        // (⌘,). On iPhone and iPad they are a sheet from the Budget toolbar.
        Settings {
            SettingsView()
                .environment(store)
                .environment(\.currencyCode, store.currencyCode)
                .appearanceFromSettings()
        }
        #endif
    }
}

/// Light, dark or follow the system, as chosen in Settings, or with
/// `-appearance light|dark` at launch (UserDefaults' argument domain feeds
/// the same key).
enum Appearance: String, CaseIterable, Identifiable {
    case system, light, dark

    var id: Self { self }

    var title: String {
        switch self {
        case .system: "System"
        case .light: "Light"
        case .dark: "Dark"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}

private struct AppearanceModifier: ViewModifier {
    @AppStorage("appearance") private var appearance: Appearance = .system

    func body(content: Content) -> some View {
        content.preferredColorScheme(appearance.colorScheme)
    }
}

extension View {
    func appearanceFromSettings() -> some View {
        modifier(AppearanceModifier())
    }
}
