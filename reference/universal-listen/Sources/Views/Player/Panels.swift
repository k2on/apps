import SwiftUI

enum PanelStyle { case player, inspector, popover }

/// Up Next: what plays after this, reorderable, with shuffle and repeat.
struct UpNextList: View {
    let style: PanelStyle
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library

    var body: some View {
        @Bindable var player = player
        List {
            Section {
                ForEach(Array(player.upNext.enumerated()), id: \.offset) { _, item in
                    if let info = library.info(item) {
                        HStack(spacing: 12) {
                            Artwork(art: info.art).frame(width: 40, height: 40)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(info.title).lineLimit(1)
                                Text(info.subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                            }
                            Spacer()
                            Text(Format.clock(info.duration)).font(.subheadline.monospacedDigit()).foregroundStyle(.secondary)
                        }
                        .listRowBackground(Color.clear)
                        .accessibilityElement(children: .combine)
                    }
                }
                .onMove { player.moveInQueue(from: $0, to: $1) }
                .onDelete { player.removeFromQueue(at: $0) }
            } header: {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Up Next").font(.headline).foregroundStyle(.primary)
                        if let output = player.output {
                            Text("Playing on \(output.name)").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Spacer()
                    Toggle(isOn: $player.shuffle) { Label("Shuffle", systemImage: "shuffle") }
                    Toggle(isOn: Binding(get: { player.repeatMode != .off },
                                         set: { player.repeatMode = $0 ? .all : .off })) {
                        Label("Repeat", systemImage: player.repeatMode.symbol)
                    }
                }
                .toggleStyle(.button)
                .labelStyle(.iconOnly)
                .textCase(nil)
            }
            if !player.history.isEmpty && style != .popover {
                Section("History") {
                    ForEach(Array(player.history.suffix(4).reversed().enumerated()), id: \.offset) { _, item in
                        if let info = library.info(item) {
                            HStack(spacing: 12) {
                                Artwork(art: info.art).frame(width: 32, height: 32)
                                Text(info.title).lineLimit(1).foregroundStyle(.secondary)
                            }
                            .listRowBackground(Color.clear)
                        }
                    }
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(style == .player ? .hidden : .automatic)
        .overlay {
            if player.upNext.isEmpty {
                ContentUnavailableView("Nothing Up Next", systemImage: "list.bullet",
                                       description: Text("Choose Play Next on a song or episode to add it here."))
            }
        }
    }
}

/// Lyrics for songs, the transcript for spoken items: the current line
/// bright, tap any line to jump there.
struct LinesView: View {
    let style: PanelStyle
    @Environment(Player.self) private var player
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let lines = player.info?.lines ?? []
        let spoken = player.isSpoken
        let current = player.currentLineID
        if lines.isEmpty {
            ContentUnavailableView(spoken ? "No Transcript" : "Instrumental",
                                   systemImage: spoken ? "text.quote" : "music.note",
                                   description: Text(spoken ? "The server hasn't transcribed this episode yet." : "This track has no lyrics."))
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: spoken ? 18 : 22) {
                        ForEach(lines) { line in
                            Button {
                                player.seek(line.time)
                            } label: {
                                VStack(alignment: .leading, spacing: 4) {
                                    if spoken, let speaker = line.speaker,
                                       line.id == 0 || lines[line.id - 1].speaker != speaker {
                                        Text("\(speaker.uppercased())  \(Format.clock(line.time))")
                                            .font(.caption.weight(.semibold))
                                            .foregroundStyle(.secondary)
                                    }
                                    Text(line.text)
                                        .font(lineFont(spoken: spoken))
                                        .multilineTextAlignment(.leading)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .foregroundStyle(lineStyle(line, current: current))
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .id(line.id)
                            .accessibilityHint("Plays from here")
                        }
                    }
                    .padding(.vertical, style == .player ? 24 : 16)
                    .padding(.horizontal, style == .player ? 4 : 16)
                }
                .scrollIndicators(.hidden)
                .onAppear { if let current { proxy.scrollTo(current, anchor: .center) } }
                .onChange(of: current) { _, new in
                    guard let new else { return }
                    if reduceMotion { proxy.scrollTo(new, anchor: .center) } else {
                        withAnimation(.listen) { proxy.scrollTo(new, anchor: .center) }
                    }
                }
            }
        }
    }

    private func lineFont(spoken: Bool) -> Font {
        switch style {
        case .player: spoken ? .title3.weight(.semibold) : .title.bold()
        case .inspector, .popover: spoken ? .body : .title3.bold()
        }
    }

    private func lineStyle(_ line: TimedLine, current: Int?) -> AnyShapeStyle {
        guard let current else { return AnyShapeStyle(.secondary) }
        if line.id == current { return AnyShapeStyle(.primary) }
        return line.id < current ? AnyShapeStyle(.secondary) : AnyShapeStyle(.tertiary)
    }
}

/// The output button: "Playing on …", opening the household's devices.
struct OutputButton: View {
    enum Style { case toolbar, player }
    let style: Style
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav
    @Environment(Library.self) private var library
    @State private var shown = false

    var body: some View {
        Button {
            presented.wrappedValue.toggle()
        } label: {
            Label(player.output.map { "Playing on \($0.name)" } ?? "Output",
                  systemImage: player.outputID == "this" ? "airplayaudio" : (player.output?.symbol ?? "airplayaudio"))
        }
        .labelStyle(.iconOnly)
        .frame(minWidth: 44, minHeight: 32)
        .help(player.output.map { "Playing on \($0.name)" } ?? "Output")
        .accessibilityIdentifier("button.output")
        .popover(isPresented: presented, arrowEdge: style == .player ? .bottom : .top) {
            OutputPicker()
                .frame(minWidth: 320, idealWidth: 340)
                .modifier(ListenModel(library: library, player: player, nav: nav))
                .presentationDetents([.medium, .large])
        }
        .onChange(of: nav.outputPopover, initial: true) { _, open in
            // The launch's `-popover output` opens the player's when the player is shown.
            if style == .player, open, nav.playerExpanded {
                nav.outputPopover = false
                shown = true
            }
        }
    }

    /// The toolbar's is the Navigator's (a launch can open it); the player's is its own.
    private var presented: Binding<Bool> {
        if style == .toolbar {
            return Binding(get: { nav.outputPopover && !nav.playerExpanded }, set: { nav.outputPopover = $0 })
        }
        return $shown
    }
}

/// Every speaker and device in the house, and who is using it.
struct OutputPicker: View {
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav

    var body: some View {
        @Bindable var player = player
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Playing On").font(.headline)
                if let info = player.info {
                    Text(info.title).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .padding([.horizontal, .top], 16)
            .padding(.bottom, 8)
            List {
                ForEach(library.devices) { device in
                    let user = library.members.first { $0.activity?.deviceID == device.id && $0.activity?.isPlaying == true }
                    Button {
                        if user != nil {
                            nav.dialog = .handOff(device.id)
                        } else {
                            player.handOff(to: device.id)
                        }
                    } label: {
                        HStack(spacing: 12) {
                            Image(systemName: device.symbol)
                                .font(.title3)
                                .frame(width: 32)
                                .foregroundStyle(player.outputID == device.id ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                            VStack(alignment: .leading, spacing: 1) {
                                Text(device.name)
                                Text(user.map { "\($0.name) is listening" } ?? device.room)
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            if player.outputID == device.id {
                                Image(systemName: "checkmark.circle.fill").foregroundStyle(.tint)
                                    .accessibilityLabel("Selected")
                            } else {
                                Image(systemName: "circle").foregroundStyle(.tertiary)
                            }
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(player.outputID == device.id ? .isSelected : [])
                }
            }
            .listStyle(.plain)
            .frame(minHeight: 300)
            Divider()
            VolumeSlider().padding(16)
        }
        .sensoryFeedback(.selection, trigger: player.outputID)
        .modifier(HandOffDialog())
    }
}

/// "Kitchen is in use": take it over or play on both.
struct HandOffDialog: ViewModifier {
    @Environment(Navigator.self) private var nav
    @Environment(Library.self) private var library
    @Environment(Player.self) private var player

    @ViewBuilder func body(content: Content) -> some View {
        let deviceID: String? = if case .handOff(let d) = nav.dialog { d } else { nil }
        let device = deviceID.flatMap(library.device)
        let user = library.members.first { $0.activity?.deviceID == deviceID }
        content.confirmationDialog(
            "Play on \(device?.name ?? "this device")?",
            isPresented: Binding(get: { device != nil }, set: { if !$0 { nav.dialog = nil } }),
            titleVisibility: .visible,
            presenting: device
        ) { d in
            Button("Move Playback to \(d.name)") { player.handOff(to: d.id) }
            Button("Play on Both") { player.handOff(to: d.id) }
            Button("Cancel", role: .cancel) {}
        } message: { d in
            if let user, let activity = user.activity, let info = library.info(activity.item) {
                Text("\(user.name) is listening to “\(info.title)” on \(d.name). Their playback will pause.")
            } else {
                Text("What you're playing will move to \(d.name).")
            }
        }
    }
}
