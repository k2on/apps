import SwiftUI

/// The iOS peer: SwiftUI over harken's peer — the Rust kernel with
/// harken's domain linked in (`HarkenFFI.xcframework`, `harken/ffi`), called
/// through the generated types (`Gen/Harken.swift`). One `Model` for the
/// app's life; every screen reads it.
@main
struct HarkenApp: App {
    @StateObject private var model = Model()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(model)
        }
    }
}

struct RootView: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        VStack(spacing: 0) {
            TabView {
                LibraryView()
                    .tabItem { Label("Library", systemImage: "music.note.list") }
                PlaylistsView()
                    .tabItem { Label("Playlists", systemImage: "text.badge.plus") }
                SettingsView()
                    .tabItem { Label("Settings", systemImage: "gearshape") }
            }
            StatusBar()
        }
    }
}

/// One line under everything: where the session stands, and the last note.
struct StatusBar: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(model.linkColor)
                .frame(width: 8, height: 8)
            Text(model.statusLine)
                .font(.caption)
                .lineLimit(1)
            Spacer()
            if let note = model.notice {
                Text(note)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(.bar)
    }
}
