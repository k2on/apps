import SwiftUI

/// The Mac's Settings window: a pane per tab.
struct SettingsWindow: View {
    @Environment(Navigator.self) private var nav

    var body: some View {
        @Bindable var nav = nav
        TabView(selection: $nav.settingsPane) {
            ForEach(SettingsPane.allCases) { pane in
                SettingsPaneView(pane: pane)
                    .tabItem { Label(pane.title, systemImage: pane.symbol) }
                    .tag(pane)
            }
        }
        .frame(width: 560, height: 520)
    }
}

/// iPhone and iPad: Settings as a sheet, a list of the same panes.
struct SettingsSheet: View {
    @Environment(Navigator.self) private var nav
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        @Bindable var nav = nav
        NavigationStack(path: $nav.settingsPath) {
            List {
                if let me = library.member(library.meID) {
                    Section {
                        HStack(spacing: 14) {
                            Avatar(member: me, size: 52)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(me.name).font(.title3.weight(.semibold))
                                Text("Owner · Household on listen.home.arpa").font(.subheadline).foregroundStyle(.secondary)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                }
                Section {
                    ForEach(SettingsPane.allCases) { pane in
                        NavigationLink(value: pane) {
                            Label(pane.title, systemImage: pane.symbol)
                        }
                    }
                }
            }
            .navigationTitle("Settings")
            .inlineTitle()
            .navigationDestination(for: SettingsPane.self) { pane in
                SettingsPaneView(pane: pane).navigationTitle(pane.title).inlineTitle()
            }
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
    }
}

struct SettingsPaneView: View {
    let pane: SettingsPane

    var body: some View {
        switch pane {
        case .server: ServerSettings()
        case .household: HouseholdSettings()
        case .playback: PlaybackSettings()
        case .downloads: DownloadSettings()
        case .appearance: AppearanceSettings()
        }
    }
}

private struct ServerSettings: View {
    @Environment(Library.self) private var library
    @AppStorage("server") private var server = "listen.home.arpa"
    @State private var confirmSignOut = false

    var body: some View {
        Form {
            Section {
                TextField("Address", text: $server)
                LabeledContent("Status") {
                    Label("Connected", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                }
                LabeledContent("Version", value: "Listen Server 3.2")
            } header: {
                Text("Home Server")
            }
            Section("Library") {
                LabeledContent("Music", value: "\(library.tracks.count) songs · \(library.albums.count) albums")
                LabeledContent("Podcasts & Sermons", value: "\(library.shows.count) shows · \(library.episodes.count) episodes")
                LabeledContent("Last Scan", value: "Today, 4:12 AM")
                Button("Rescan Library") {}
            }
            Section {
                Button("Sign Out of Server…", role: .destructive) { confirmSignOut = true }
            }
        }
        .formStyle(.grouped)
        .confirmationDialog("Sign out of listen.home.arpa?", isPresented: $confirmSignOut, titleVisibility: .visible) {
            Button("Sign Out", role: .destructive) {}
        } message: {
            Text("Downloads stay on this device until you sign in again.")
        }
    }
}

private struct HouseholdSettings: View {
    @Environment(Library.self) private var library
    @AppStorage("allowHandoff") private var allowHandoff = true
    @AppStorage("shareActivity") private var shareActivity = true
    @AppStorage("leoExplicit") private var leoExplicit = false

    var body: some View {
        Form {
            Section("Members") {
                ForEach(library.members) { m in
                    HStack(spacing: 12) {
                        Avatar(member: m, size: 32)
                        Text(m.name)
                        Spacer()
                        Text(m.role).foregroundStyle(.secondary)
                    }
                }
                Button("Invite Member…", systemImage: "person.badge.plus") {}
            }
            Section {
                Toggle("Others can hand off to my devices", isOn: $allowHandoff)
                Toggle("Show what I'm listening to", isOn: $shareActivity)
            } footer: {
                Text("Everyone in the household sees which speakers are in use.")
            }
            Section("Leo") {
                Toggle("Allow explicit content", isOn: $leoExplicit)
            }
        }
        .formStyle(.grouped)
    }
}

private struct PlaybackSettings: View {
    @AppStorage("crossfade") private var crossfade = true
    @AppStorage("crossfadeSeconds") private var crossfadeSeconds = 4.0
    @AppStorage("soundCheck") private var soundCheck = true
    @AppStorage("skipBack") private var skipBack = 15
    @AppStorage("skipForward") private var skipForward = 30
    @AppStorage("spokenRate") private var spokenRate = 1.25
    @AppStorage("continueUpNext") private var continueUpNext = true

    var body: some View {
        Form {
            Section("Music") {
                Toggle("Crossfade Songs", isOn: $crossfade)
                if crossfade {
                    LabeledContent("Crossfade") {
                        Slider(value: $crossfadeSeconds, in: 1...12, step: 1) { Text("Seconds") }
                            .frame(maxWidth: 220)
                        Text("\(Int(crossfadeSeconds)) s").monospacedDigit().foregroundStyle(.secondary)
                    }
                }
                Toggle("Sound Check", isOn: $soundCheck)
            }
            Section("Podcasts & Sermons") {
                Picker("Skip Back", selection: $skipBack) {
                    ForEach([10, 15, 30], id: \.self) { Text("\($0) seconds").tag($0) }
                }
                Picker("Skip Forward", selection: $skipForward) {
                    ForEach([15, 30, 45, 60], id: \.self) { Text("\($0) seconds").tag($0) }
                }
                Picker("Default Speed", selection: $spokenRate) {
                    ForEach(Player.speeds, id: \.self) { Text(Format.rate($0)).tag($0) }
                }
            }
            Section {
                Toggle("Continue with Up Next", isOn: $continueUpNext)
            } footer: {
                Text("When the queue ends, keep playing similar music from the library.")
            }
        }
        .formStyle(.grouped)
    }
}

private struct DownloadSettings: View {
    @AppStorage("autoDownload") private var autoDownload = true
    @AppStorage("keep") private var keep = "5"
    @AppStorage("removePlayed") private var removePlayed = true
    @AppStorage("cellular") private var cellular = false

    var body: some View {
        Form {
            Section("Episodes") {
                Toggle("Download New Episodes", isOn: $autoDownload)
                Picker("Keep", selection: $keep) {
                    Text("Latest 1").tag("1")
                    Text("Latest 5").tag("5")
                    Text("Latest 10").tag("10")
                    Text("All").tag("all")
                }
                Toggle("Remove Played Downloads", isOn: $removePlayed)
            }
            Section {
                Toggle("Download over Cellular", isOn: $cellular)
            }
            Section("Storage") {
                Gauge(value: 3.1, in: 0...64) {
                    Text("Downloads")
                } currentValueLabel: {
                    Text("3.1 GB")
                }
                LabeledContent("Episodes", value: "2.4 GB")
                LabeledContent("Music", value: "0.7 GB")
            }
        }
        .formStyle(.grouped)
    }
}

private struct AppearanceSettings: View {
    @AppStorage("appearance") private var appearance = "system"
    @AppStorage("showPlays") private var showPlays = true
    @AppStorage("transcriptSize") private var transcriptSize = "Large"

    var body: some View {
        Form {
            Section {
                Picker("Appearance", selection: $appearance) {
                    Text("System").tag("system")
                    Text("Light").tag("light")
                    Text("Dark").tag("dark")
                }
                .pickerStyle(.segmented)
            }
            Section("Library") {
                Toggle("Show Play Counts", isOn: $showPlays)
            }
            Section("Lyrics & Transcripts") {
                Picker("Text Size", selection: $transcriptSize) {
                    ForEach(["Standard", "Large", "Extra Large"], id: \.self) { Text($0) }
                }
            }
        }
        .formStyle(.grouped)
    }
}
