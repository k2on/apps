import SwiftUI

@main
struct ListenApp: App {
    @State private var library: Library
    @State private var player: Player
    @State private var nav = Navigator()
    @AppStorage("appearance") private var appearance = "system"

    init() {
        let library = Library()
        _library = State(initialValue: library)
        _player = State(initialValue: Player(library: library))
    }

    private var scheme: ColorScheme? {
        switch appearance {
        case "light": .light
        case "dark": .dark
        default: nil
        }
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .modifier(CompactReader())
                .environment(library)
                .environment(player)
                .environment(nav)
                .preferredColorScheme(scheme)
                #if os(macOS)
                .frame(minWidth: 960, minHeight: 600)
                #endif
        }
        #if os(macOS)
        .defaultSize(width: Launch.windowSize.width, height: Launch.windowSize.height)
        #endif
        .commands {
            SidebarCommands()
            InspectorCommands()
            ListenCommands(player: player, nav: nav)
        }

        #if os(macOS)
        Settings {
            SettingsWindow()
                .environment(library)
                .environment(player)
                .environment(nav)
                .preferredColorScheme(scheme)
        }
        #endif
    }
}

/// The menu bar (Mac; iPad with a keyboard): a Controls menu like Music's,
/// search on ⌘F, and the player and Up Next in View.
struct ListenCommands: Commands {
    let player: Player
    let nav: Navigator

    var body: some Commands {
        CommandGroup(replacing: .textEditing) {
            Button("Find") {
                nav.tab = .search
                nav.searchPresented = true
            }
            .keyboardShortcut("f")
        }
        CommandGroup(after: .sidebar) {
            Button(nav.playerExpanded ? "Exit Full Player" : "Full Player") { nav.playerExpanded.toggle() }
                .keyboardShortcut("f", modifiers: [.command, .shift])
            Button(nav.inspectorShown && nav.inspectorPane == .lyrics ? "Hide Lyrics" : "Show Lyrics") {
                toggleInspector(.lyrics)
            }
            .keyboardShortcut("l", modifiers: [.command, .option])
            Button(nav.inspectorShown && nav.inspectorPane == .upNext ? "Hide Up Next" : "Show Up Next") {
                toggleInspector(.upNext)
            }
            .keyboardShortcut("u", modifiers: [.command, .option])
            Divider()
        }
        CommandMenu("Controls") {
            Button(player.isPlaying ? "Pause" : "Play") { player.togglePlay() }
                .keyboardShortcut(.return, modifiers: .command)
            Button("Next") { player.next() }
                .keyboardShortcut(.rightArrow, modifiers: .command)
            Button("Previous") { player.previous() }
                .keyboardShortcut(.leftArrow, modifiers: .command)
            Button("Skip Back 15 Seconds") { player.skip(-15) }
                .keyboardShortcut(.leftArrow, modifiers: [.command, .option])
            Button("Skip Forward 30 Seconds") { player.skip(30) }
                .keyboardShortcut(.rightArrow, modifiers: [.command, .option])
            Divider()
            Button("Go to Current Item") {
                if let route = player.info?.route { nav.go(route) }
            }
            .keyboardShortcut("l")
            Divider()
            Button("Increase Volume") { player.nudgeVolume(0.1) }
                .keyboardShortcut(.upArrow, modifiers: .command)
            Button("Decrease Volume") { player.nudgeVolume(-0.1) }
                .keyboardShortcut(.downArrow, modifiers: .command)
            Divider()
            Toggle("Shuffle", isOn: Binding(get: { player.shuffle }, set: { player.shuffle = $0 }))
                .keyboardShortcut("s", modifiers: [.command, .option])
            Picker("Repeat", selection: Binding(get: { player.repeatMode }, set: { player.repeatMode = $0 })) {
                ForEach(Player.Repeat.allCases) { Text($0.rawValue).tag($0) }
            }
            Picker("Playback Speed", selection: Binding(get: { player.rate }, set: { player.rate = $0 })) {
                ForEach(Player.speeds, id: \.self) { Text(Format.rate($0)).tag($0) }
            }
            .disabled(!player.isSpoken)
        }
    }

    private func toggleInspector(_ pane: InspectorPane) {
        if nav.inspectorShown && nav.inspectorPane == pane {
            nav.inspectorShown = false
        } else {
            nav.inspectorPane = pane
            nav.inspectorShown = true
        }
    }
}
