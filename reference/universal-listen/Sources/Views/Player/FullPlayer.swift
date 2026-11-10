import SwiftUI

/// The full player. Portrait: artwork, title, scrubber, transport, volume,
/// and a row of Lyrics / output / Up Next; choosing Lyrics or Up Next folds
/// the artwork into a header and fills the middle. Landscape and the Mac:
/// artwork and controls on the left, lyrics or Up Next on the right.
struct FullPlayer: View {
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { geo in
            let wide = geo.size.width > geo.size.height * 1.15 && geo.size.width > 700
            ZStack {
                PlayerBackground(art: player.info?.art)
                if wide {
                    HStack(spacing: 48) {
                        VStack(spacing: 24) {
                            Spacer(minLength: 0)
                            PlayerArtwork(maxSide: min(geo.size.height * 0.5, 420))
                            PlayerControls(showsTitle: true)
                            Spacer(minLength: 0)
                        }
                        .frame(maxWidth: 460)
                        PlayerPanelView(panel: nav.playerPanel == .upNext ? .upNext : .lyrics)
                            .frame(maxWidth: .infinity)
                    }
                    .padding(.horizontal, 56)
                    .padding(.vertical, 32)
                } else {
                    VStack(spacing: 20) {
                        if nav.playerPanel == .none {
                            Spacer(minLength: 0)
                            PlayerArtwork(maxSide: geo.size.width - 64)
                            Spacer(minLength: 0)
                        } else {
                            CompactHeader()
                            PlayerPanelView(panel: nav.playerPanel)
                                .transition(.opacity)
                        }
                        PlayerControls(showsTitle: nav.playerPanel == .none)
                    }
                    .padding(.horizontal, 28)
                    .padding(.top, 8)
                    .padding(.bottom, 12)
                }
            }
            .overlay(alignment: .top) { CloseHandle() }
        }
        .environment(\.colorScheme, .dark)
        #if os(iOS)
        .preferredColorScheme(.dark)
        #endif
        .foregroundStyle(.white)
        .tint(.white)
        .listenAnimation(nav.playerPanel)
        #if os(macOS)
        .onExitCommand { nav.playerExpanded = false }
        #endif
    }
}

private struct CloseHandle: View {
    @Environment(Navigator.self) private var nav

    var body: some View {
        HStack {
            Button("Close", systemImage: "chevron.down") { nav.playerExpanded = false }
                .labelStyle(.iconOnly)
                .font(.title3.weight(.semibold))
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
                .buttonStyle(PressableStyle())
                .keyboardShortcut(.cancelAction)
            Spacer()
        }
        .padding(.horizontal, 12)
        .overlay {
            Capsule().fill(.white.opacity(0.35)).frame(width: 40, height: 5)
                .accessibilityHidden(true)
        }
    }
}

/// The artwork, smaller while paused.
private struct PlayerArtwork: View {
    let maxSide: CGFloat
    @Environment(Player.self) private var player

    var body: some View {
        if let info = player.info {
            Artwork(art: info.art)
                .frame(maxWidth: maxSide, maxHeight: maxSide)
                .scaleEffect(player.isPlaying ? 1 : 0.84)
                .shadow(color: .black.opacity(player.isPlaying ? 0.4 : 0.2), radius: player.isPlaying ? 30 : 14, y: 14)
                .listenAnimation(player.isPlaying)
        }
    }
}

private struct CompactHeader: View {
    @Environment(Player.self) private var player

    var body: some View {
        if let info = player.info {
            HStack(spacing: 14) {
                Artwork(art: info.art).frame(width: 64, height: 64)
                    .shadow(color: .black.opacity(0.3), radius: 8, y: 4)
                VStack(alignment: .leading, spacing: 2) {
                    Text(info.title).font(.headline).lineLimit(1)
                    Text(info.subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                ItemMoreMenu()
            }
            .padding(.top, 44)
        }
    }
}

/// Title, scrubber, transport, volume and the bottom row.
private struct PlayerControls: View {
    let showsTitle: Bool
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        @Bindable var player = player
        VStack(spacing: 22) {
            if showsTitle {
                HStack(alignment: .center) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(player.info?.title ?? "Not Playing").font(.title3.weight(.semibold)).lineLimit(1)
                        Text(player.info?.subtitle ?? "").font(.title3).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer()
                    FavoriteButton()
                    ItemMoreMenu()
                }
            }
            Scrubber(position: $player.position, duration: player.duration)
            TransportButtons(size: .full)
                .frame(maxWidth: .infinity)
            VolumeSlider()
                .tint(.white.opacity(0.85))
                .foregroundStyle(.secondary)
            PlayerBottomRow()
        }
    }
}

/// Lyrics or transcript · Speed (spoken) · Output · Up Next.
private struct PlayerBottomRow: View {
    @Environment(Player.self) private var player
    @Environment(Navigator.self) private var nav

    var body: some View {
        HStack {
            panelToggle(.lyrics, title: player.isSpoken ? "Transcript" : "Lyrics", symbol: "quote.bubble")
            Spacer()
            if player.isSpoken {
                SpeedMenu()
                Spacer()
            }
            OutputButton(style: .player)
            Spacer()
            panelToggle(.upNext, title: "Up Next", symbol: "list.bullet")
        }
        .font(.title3)
        .padding(.horizontal, 24)
    }

    private func panelToggle(_ panel: PlayerPanel, title: String, symbol: String) -> some View {
        let on = nav.playerPanel == panel
        return Button(title, systemImage: symbol) {
            nav.playerPanel = on ? .none : panel
        }
        .labelStyle(.iconOnly)
        .frame(width: 44, height: 44)
        .background(on ? AnyShapeStyle(Color.white.opacity(0.22)) : AnyShapeStyle(Color.clear), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .buttonStyle(PressableStyle())
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityIdentifier("panel.\(panel.rawValue)")
    }
}

/// Playback speed for spoken items: a pop-up menu.
struct SpeedMenu: View {
    @Environment(Player.self) private var player

    var body: some View {
        @Bindable var player = player
        Menu {
            Picker("Playback Speed", selection: $player.rate) {
                ForEach(Player.speeds, id: \.self) { Text(Format.rate($0)).tag($0) }
            }
            Divider()
            Button("Skip Silences", systemImage: "waveform.badge.minus") {}
        } label: {
            Text(Format.rate(player.rate))
                .font(.callout.weight(.semibold).monospacedDigit())
                .padding(.horizontal, 10)
                .frame(height: 30)
                .background(.white.opacity(0.18), in: Capsule())
        }
        .menuIndicator(.hidden)
        .buttonStyle(.plain)
        .accessibilityLabel("Playback Speed")
        .accessibilityValue(Format.rate(player.rate))
        .accessibilityIdentifier("menu.speed")
    }
}

private struct FavoriteButton: View {
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library

    var body: some View {
        if let id = player.currentTrackID, let t = library.track(id) {
            Button(t.isFavorite ? "Undo Favorite" : "Favorite", systemImage: t.isFavorite ? "star.fill" : "star") {
                library.toggleFavorite(id)
            }
            .labelStyle(.iconOnly)
            .font(.title3)
            .frame(width: 36, height: 36)
            .background(.white.opacity(0.15), in: Circle())
            .buttonStyle(PressableStyle())
            .contentTransition(.symbolEffect(.replace))
            .sensoryFeedback(.selection, trigger: t.isFavorite)
        }
    }
}

/// The current item's More menu (same actions as its context menu).
private struct ItemMoreMenu: View {
    @Environment(Player.self) private var player
    @Environment(Library.self) private var library
    @Environment(Navigator.self) private var nav

    var body: some View {
        ListenMenu {
            if let id = player.currentTrackID, let t = library.track(id) {
                TrackMenuItems(tracks: [t])
            } else if let id = player.currentEpisodeID, let e = library.episode(id) {
                EpisodeMenuItems(episode: e)
            }
        } label: {
            Image(systemName: "ellipsis")
                .font(.body.weight(.bold))
                .frame(width: 36, height: 36)
                .background(.white.opacity(0.15), in: Circle())
        }
        .menuIndicator(.hidden)
        .buttonStyle(.plain)
        .accessibilityLabel("More")
        .accessibilityIdentifier("menu.more-current")
        .onChange(of: nav.paths) { nav.playerExpanded = false }
    }
}

/// A blurred field of the artwork's colours behind the full player.
private struct PlayerBackground: View {
    let art: Art?

    var body: some View {
        let a = art ?? Art(hue: 0.6, hue2: 0.7, symbol: "music.note", style: .blobs)
        MeshGradient(width: 3, height: 3, points: [
            [0, 0], [0.5, 0], [1, 0],
            [0, 0.5], [0.6, 0.45], [1, 0.5],
            [0, 1], [0.5, 1], [1, 1],
        ], colors: [
            Color(hue: a.hue, saturation: 0.55, brightness: 0.55), Color(hue: a.hue, saturation: 0.6, brightness: 0.42), Color(hue: a.hue2, saturation: 0.6, brightness: 0.40),
            Color(hue: a.hue2, saturation: 0.55, brightness: 0.36), Color(hue: a.hue, saturation: 0.5, brightness: 0.34), Color(hue: a.hue2, saturation: 0.65, brightness: 0.28),
            Color(hue: a.hue2, saturation: 0.7, brightness: 0.18), Color(hue: a.hue, saturation: 0.6, brightness: 0.16), Color(hue: a.hue2, saturation: 0.7, brightness: 0.14),
        ])
        .ignoresSafeArea()
    }
}

/// The scrubber: a capsule that thickens while dragged; elapsed and remaining below.
struct Scrubber: View {
    @Binding var position: TimeInterval
    let duration: TimeInterval
    @State private var dragging: TimeInterval?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let shown = dragging ?? position
        VStack(spacing: 6) {
            GeometryReader { geo in
                let fraction = duration > 0 ? min(1, max(0, shown / duration)) : 0
                ZStack(alignment: .leading) {
                    Capsule().fill(.white.opacity(0.22))
                    Capsule().fill(.white.opacity(dragging == nil ? 0.75 : 1))
                        .frame(width: max(0, geo.size.width * fraction))
                }
                .frame(height: dragging == nil ? 7 : 12)
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { v in dragging = min(1, max(0, v.location.x / geo.size.width)) * duration }
                        .onEnded { _ in
                            if let d = dragging { position = d }
                            dragging = nil
                        }
                )
                .animation(reduceMotion ? nil : .spring(response: 0.3, dampingFraction: 0.8), value: dragging == nil)
            }
            .frame(height: 22)
            HStack {
                Text(Format.clock(shown))
                Spacer()
                Text("-" + Format.clock(duration - shown))
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.secondary)
        }
        .accessibilityElement()
        .accessibilityLabel("Playback position")
        .accessibilityValue("\(Format.clock(position)) of \(Format.clock(duration))")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: position = min(duration, position + 15)
            case .decrement: position = max(0, position - 15)
            @unknown default: break
            }
        }
    }
}

/// The middle of the portrait player, or the right of the wide one.
private struct PlayerPanelView: View {
    let panel: PlayerPanel

    var body: some View {
        switch panel {
        case .upNext: UpNextList(style: .player)
        default: LinesView(style: .player)
        }
    }
}
