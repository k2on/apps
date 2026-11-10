import SwiftUI

// How the player attaches to the app on each device:
//   iPhone   a mini player above the tab bar; tap for the full player.
//   iPad     the same mini player, plus toolbar buttons for output, lyrics
//            (the inspector) and Up Next (a popover).
//   Mac      the window toolbar is the player, as in Music: transport,
//            the now-playing display, volume, output, lyrics, Up Next.

extension View {
    func playbackToolbar() -> some View { modifier(PlaybackToolbar()) }
}

struct PlaybackToolbar: ViewModifier {
    @Environment(\.isCompact) private var isCompact

    @ViewBuilder func body(content: Content) -> some View {
        content.toolbar {
            #if os(macOS)
            ToolbarItemGroup(placement: .navigation) {
                TransportButtons(size: .toolbar)
            }
            ToolbarItem(placement: .principal) {
                NowPlayingDisplay()
            }
            ToolbarItem(placement: .primaryAction) {
                VolumeSlider(compact: true).frame(width: 110)
            }
            #endif
            if !isCompact {
                ToolbarItemGroup(placement: .primaryAction) {
                    OutputButton(style: .toolbar)
                    LyricsToggle()
                    UpNextButton()
                }
            }
        }
    }
}

/// Mac: the toolbar's centre display — artwork, title, a thin progress line.
struct NowPlayingDisplay: View {
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        HStack(spacing: 10) {
            if let info = player.info {
                Button {
                    nav.playerExpanded = true
                } label: {
                    Artwork(art: info.art).frame(width: 30, height: 30)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Show Full Player")
                VStack(spacing: 2) {
                    Text(info.title).font(.callout.weight(.medium)).lineLimit(1)
                    Text(info.subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    ProgressView(value: player.position, total: player.duration)
                        .progressViewStyle(.linear)
                        .controlSize(.mini)
                        .tint(.secondary)
                }
                .frame(maxWidth: .infinity)
                Text("-" + Format.clock(player.duration - player.position))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.secondary)
            } else {
                Image(systemName: "music.note").foregroundStyle(.tertiary).frame(maxWidth: .infinity)
            }
        }
        .padding(.horizontal, 8)
        .frame(width: 400, height: 38)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Now Playing")
    }
}

/// Previous / play-pause / next, or skip back / play-pause / skip forward for spoken items.
struct TransportButtons: View {
    enum Size { case toolbar, mini, full }
    let size: Size
    @Environment(Player.self) private var player

    var body: some View {
        let spoken = player.isSpoken
        HStack(spacing: spacing) {
            if size != .mini {
                Button(spoken ? "Skip Back 15 Seconds" : "Previous", systemImage: spoken ? "gobackward.15" : "backward.fill") {
                    spoken ? player.skip(-15) : player.previous()
                }
            }
            Button(player.isPlaying ? "Pause" : "Play", systemImage: player.isPlaying ? "pause.fill" : "play.fill") {
                player.togglePlay()
            }
            .font(playFont)
            .contentTransition(.symbolEffect(.replace))
            Button(spoken ? "Skip Forward 30 Seconds" : "Next", systemImage: spoken ? "goforward.30" : "forward.fill") {
                spoken ? player.skip(30) : player.next()
            }
        }
        .labelStyle(.iconOnly)
        .font(font)
        .modifier(TransportStyle(size: size))
        .disabled(player.item == nil)
    }

    private var spacing: CGFloat {
        switch size {
        case .toolbar: 2
        case .mini: 8
        case .full: 44
        }
    }

    private var font: Font {
        switch size {
        case .toolbar: .body
        case .mini: .title3
        case .full: .system(size: 30)
        }
    }

    private var playFont: Font {
        switch size {
        case .toolbar: .title3
        case .mini: .title2
        case .full: .system(size: 46)
        }
    }
}

private struct TransportStyle: ViewModifier {
    let size: TransportButtons.Size
    @ViewBuilder func body(content: Content) -> some View {
        if size == .toolbar {
            content
        } else {
            content.buttonStyle(PressableStyle(scale: size == .full ? 0.82 : 0.88))
        }
    }
}

struct VolumeSlider: View {
    var compact = false
    @Environment(Player.self) private var player

    var body: some View {
        @Bindable var player = player
        Slider(value: $player.volume, in: 0...1) {
            Text("Volume")
        } minimumValueLabel: {
            Image(systemName: "speaker.fill").font(compact ? .caption : .footnote)
        } maximumValueLabel: {
            Image(systemName: "speaker.wave.3.fill").font(compact ? .caption : .footnote)
        }
        .controlSize(compact ? .mini : .regular)
        .accessibilityValue(Text("\(Int(player.volume * 100)) percent"))
    }
}

/// Opens the inspector on Lyrics (or Transcript for spoken items).
struct LyricsToggle: View {
    @Environment(Navigator.self) private var nav
    @Environment(Player.self) private var player

    var body: some View {
        let on = nav.inspectorShown && nav.inspectorPane == .lyrics
        Toggle(isOn: Binding(get: { on }, set: { show in
            nav.inspectorPane = .lyrics
            nav.inspectorShown = show
        })) {
            Label(player.isSpoken ? "Transcript" : "Lyrics", systemImage: "quote.bubble")
        }
        .toggleStyle(.button)
        .help(player.isSpoken ? "Show Transcript" : "Show Lyrics")
    }
}

/// Up Next, as a popover off the toolbar.
struct UpNextButton: View {
    @Environment(Navigator.self) private var nav

    var body: some View {
        @Bindable var nav = nav
        Button("Up Next", systemImage: "list.bullet") { nav.upNextPopover.toggle() }
            .help("Up Next")
            .accessibilityIdentifier("button.upnext")
            .popover(isPresented: $nav.upNextPopover, arrowEdge: .bottom) {
                UpNextList(style: .popover)
                    .frame(minWidth: 340, idealWidth: 360, minHeight: 420, idealHeight: 520)
                    .presentationCompactAdaptation(.popover)
            }
    }
}

/// The inspector: Up Next or Lyrics/Transcript, beside the content (iPad, Mac).
struct PlayerInspector: ViewModifier {
    @Environment(Navigator.self) private var nav
    @Environment(Player.self) private var player

    @ViewBuilder func body(content: Content) -> some View {
        @Bindable var nav = nav
        content.inspector(isPresented: $nav.inspectorShown) {
            VStack(spacing: 0) {
                Picker("Show", selection: $nav.inspectorPane) {
                    Text("Up Next").tag(InspectorPane.upNext)
                    Text(player.isSpoken ? "Transcript" : "Lyrics").tag(InspectorPane.lyrics)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(12)
                Divider()
                switch nav.inspectorPane {
                case .upNext: UpNextList(style: .inspector)
                case .lyrics: LinesView(style: .inspector)
                }
            }
            .inspectorColumnWidth(min: 280, ideal: 330, max: 440)
        }
    }
}

/// iPhone and iPad: the mini player, as the tab view's bottom accessory
/// (iOS 26), or above the content before it.
struct MiniPlayerAccessory: ViewModifier {
    @ViewBuilder func body(content: Content) -> some View {
        #if os(iOS)
        if #available(iOS 26.0, *) {
            content
                .tabViewBottomAccessory { MiniPlayer() }
                .tabBarMinimizeBehavior(.onScrollDown)
        } else {
            content.safeAreaInset(edge: .bottom) {
                MiniPlayer()
                    .padding(.vertical, 8)
                    .background(.bar, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .padding(.horizontal, 12)
                    .padding(.bottom, 56)
            }
        }
        #else
        content
        #endif
    }
}

struct MiniPlayer: View {
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        HStack(spacing: 12) {
            if let info = player.info {
                Artwork(art: info.art).frame(width: 32, height: 32)
                VStack(alignment: .leading, spacing: 0) {
                    Text(info.title).font(.subheadline.weight(.medium)).lineLimit(1)
                    Text(player.output.map { "Playing on \($0.name)" } ?? info.subtitle)
                        .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                TransportButtons(size: .mini)
                    .foregroundStyle(.primary)
            } else {
                Text("Not Playing").foregroundStyle(.secondary)
                Spacer()
            }
        }
        .padding(.horizontal, 14)
        .contentShape(Rectangle())
        .onTapGesture { nav.playerExpanded = true }
        .accessibilityElement(children: .contain)
        .accessibilityAction(named: "Show Full Player") { nav.playerExpanded = true }
        .accessibilityIdentifier("mini-player")
    }
}

/// The full player: a cover over the whole screen on iPhone and iPad, and
/// over the window's content on the Mac (the toolbar stays).
struct FullPlayerPresentation: ViewModifier {
    @Environment(Navigator.self) private var nav

    @ViewBuilder func body(content: Content) -> some View {
        @Bindable var nav = nav
        #if os(iOS)
        content.fullScreenCover(isPresented: $nav.playerExpanded) { FullPlayer() }
        #else
        content.overlay {
            if nav.playerExpanded {
                FullPlayer()
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .listenAnimation(nav.playerExpanded)
        #endif
    }
}
