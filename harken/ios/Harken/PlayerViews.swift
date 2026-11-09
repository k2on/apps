// What is playing, two ways — the mini player on glass above the tab bar
// (the tab view's bottom accessory) and the full player it zooms into — and
// the queue, and an episode's transcript as it is said. Both draw one [PlayerState]; neither knows whether the sound is
// here or on another device of the account but by what that state says
// (`on`). The position is counted from the state's clock as it is drawn
// (`TimelineView`), so only the hairline and the scrubber move with it.
// Android's `PlayerScreens.kt` and `PlayerSheet.kt`: a sideways drag on the
// bar skips — left the next track, right the one before — and the words
// spring back from where the finger left them at its speed; a tap opens
// it; the full player says where the sound is and opens the picker, draws
// the volume of whatever plays, and draws shuffle and repeat only alone,
// since the session has neither.
import ArkUI
import SwiftUI

/// The bar: the cover, the title and who made it — or where it plays — play
/// or pause and next, and the progress along its foot.
struct MiniPlayer: View {
    let open: () -> Void
    @EnvironmentObject private var model: Model
    @Environment(\.tabViewBottomAccessoryPlacement) private var placement
    @Environment(\.reducesMotion) private var still
    @State private var swipe: CGFloat = 0

    var body: some View {
        let s = model.player
        let compact = placement == .inline
        GeometryReader { geo in
            HStack(spacing: Space.m) {
                if let now = s.now {
                    // The cover and the words open the player, and give under
                    // the finger as it lands (`.press`); the buttons beside
                    // them are their own, so a tap on one does not open it.
                    Button(action: open) {
                        HStack(spacing: Space.m) {
                            CoverView(seed: now.seed, url: now.art, side: compact ? 28 : 36, corner: 6)
                                .frame(width: compact ? 28 : 36, height: compact ? 28 : 36)
                            VStack(alignment: .leading, spacing: 0) {
                                Text(now.title).font(.subheadline.weight(.semibold)).lineLimit(1)
                                if !compact {
                                    Text(s.under).font(.caption).foregroundStyle(s.on.map { !$0.here } == true ? Color.accentText : .quiet).lineLimit(1)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .offset(x: swipe)
                        }
                        .frame(maxHeight: .infinity)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.press)
                    .accessibilityHint("Opens the player")
                    Button { model.toggle() } label: {
                        Image(systemName: s.playing ? "pause.fill" : "play.fill").contentTransition(.symbolEffect(.replace)).frame(width: 32, height: 32)
                    }
                    .accessibilityLabel(s.playing ? "Pause" : "Play")
                    if !compact && model.jogs {
                        Button { model.jog(Model.jogForward) } label: { Image(systemName: "goforward.30").frame(width: 32, height: 32) }
                            .accessibilityLabel("Forward 30 seconds")
                    } else if !compact {
                        Button { model.next() } label: { Image(systemName: "forward.end.fill").frame(width: 32, height: 32) }
                            .disabled(!s.hasNext)
                            .accessibilityLabel("Next")
                    }
                } else {
                    Image(systemName: "music.note").foregroundStyle(.quiet).frame(width: 36)
                    Text("Not Playing").font(.subheadline).foregroundStyle(.quiet).frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            // The buttons in the words' colour, not the tint: as Android's bar.
            .foregroundStyle(.ink)
            .padding(.horizontal, Space.m)
            .frame(maxHeight: .infinity)
            .overlay(alignment: .bottom) { if s.now != nil && !compact { Hairline().padding(.horizontal, Space.l) } }
            .contentShape(Rectangle())
            // A sideways drag skips: where it would come to rest, projected
            // snappily (`Momentum.snappy`, as a reorder's drop is) since it
            // decides an act rather than a place, past a share of the width,
            // or let go as fast as a throw (`Springs.flick`); then it springs
            // back, starting at the finger's speed, with the momentum's
            // overshoot only when it was flicked.
            .gesture(DragGesture(minimumDistance: 12).onChanged { v in
                if abs(v.translation.width) > abs(v.translation.height) { swipe = v.translation.width }
            }.onEnded { v in
                let wide = Springs.wide * geo.size.width
                let rest = swipe + CGFloat(Momentum.project(Double(v.velocity.width), rate: Momentum.snappy))
                if rest < -wide || v.velocity.width < -Springs.flick { model.next() }
                else if rest > wide || v.velocity.width > Springs.flick { model.previous() }
                let back = still ? Springs.calm : Springs.released(velocity: Double(v.velocity.width), from: Double(swipe), to: 0)
                withAnimation(back) { swipe = 0 }
            })
        }
        .accessibilityElement(children: .contain)
    }
}

/// The progress, as a hairline along the bar's foot: the one part of the bar that reads the position.
struct Hairline: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        let s = model.player
        TimelineView(.periodic(from: .now, by: 0.5)) { t in
            let at = s.position(Int64(t.date.timeIntervalSince1970 * 1000))
            let done = s.durationMs > 0 ? min(max(Double(at) / Double(s.durationMs), 0), 1) : 0
            GeometryReader { g in
                Capsule().fill(Color.edge)
                    .overlay(alignment: .leading) { Capsule().fill(Color.accent).frame(width: g.size.width * done) }
            }
            .frame(height: 2)
        }
    }
}

/// The full player: the cover, the title and who made it, a scrubber with
/// the time gone and the time left, the transport on glass, the volume, and
/// where it plays and what plays next — and, for an episode whose feed
/// times its words, its transcript.
struct FullPlayer: View {
    let close: () -> Void
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present
    @Environment(\.open) private var open
    @Environment(\.colorScheme) private var scheme
    @Namespace private var glass

    var body: some View {
        let s = model.player
        let cues = s.now.flatMap { model.transcripts[Id($0.id)] } ?? []
        // Spaced as Android's player (PlayerScreens.kt, `FullFace`): room
        // only where it is meant (after the title, above the volume and the
        // chips), the rest the cover's, so the cover is the page's width as
        // Android's is, not what is left once every gap has had its share.
        VStack(spacing: 0) {
            HStack {
                // Round, 44 points, as Android's: the glass a circle about its glyph.
                Button(action: close) { Image(systemName: "chevron.down").frame(width: 24, height: 24) }
                    .buttonStyle(.glass)
                    .buttonBorderShape(.circle)
                    .controlSize(.large)
                    .accessibilityLabel("Close the player")
                Spacer()
                Text(s.now.map { $0.album.isEmpty ? "Now Playing" : $0.album } ?? "Now Playing")
                    .font(.footnote.weight(.semibold)).foregroundStyle(.quiet).lineLimit(1)
                Spacer()
                if let t = track {
                    Menu { TrackActions(track: t, from: nil) } label: { Image(systemName: "ellipsis").frame(width: 24, height: 24) }
                        .buttonStyle(.glass)
                        .buttonBorderShape(.circle)
                        .controlSize(.large)
                        .accessibilityLabel("More")
                } else {
                    Color.clear.frame(width: 44, height: 44)
                }
            }
            Spacer(minLength: 0)
            if let now = s.now {
                CoverView(seed: now.seed, url: now.art, side: 360, corner: 16)
                    .aspectRatio(1, contentMode: .fit)
                    // The room first, as wide as the page if the height
                    // allows, and the spacers what is left: otherwise the
                    // three share it and the cover is a third of it.
                    .layoutPriority(1)
                    .shadow(color: .black.opacity(0.25), radius: 24, y: 12)
                    .motionScale(s.playing ? 1 : MotionTokens.pausedScale)
                    .motion(Springs.move, value: s.playing)
            }
            Spacer(minLength: 0)
            // The title opens its album and the name its person's page, as
            // the desktop's bar and Android's player do; opening one closes
            // the player (`Shell`).
            VStack(alignment: .leading, spacing: 2) {
                Button { if let n = s.now, let a = model.album(playing: n) { open(.album(key: a.id, title: a.name)) } } label: {
                    Text(s.now?.title ?? "Nothing playing").font(.title2.bold()).lineLimit(2).multilineTextAlignment(.leading)
                }
                .buttonStyle(.plain)
                // An episode's name is its show's, and the show is its page.
                Button {
                    guard let a = s.now?.artist, !a.isEmpty else { return }
                    let show = model.shows.first { $0.title == a }
                    open(show != nil && model.artist(named: a) == nil ? .show(key: show!.id, title: show!.title) : .artist(name: a))
                } label: {
                    Text(s.now?.artist ?? "").font(.title3).foregroundStyle(Color.accentText).lineLimit(1)
                }
                .buttonStyle(.plain)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            PlayerScrubber()
                .padding(.top, Space.l)
            Transport()
            if let v = s.volume {
                VolumeSlider(level: v.level, muted: v.muted, about: s.on?.name ?? "this phone",
                             onLevel: { model.volume($0) }, onDone: { model.volumeDone() }, onMute: { model.mute() })
                    .padding(.top, Space.s)
            }
            GlassEffectContainer(spacing: Space.s) {
                HStack(spacing: Space.s) {
                    if let on = s.on {
                        Button { present(.devices) } label: {
                            Label(on.label, systemImage: symbol(forDevice: on.kind, name: on.name)).lineLimit(1)
                        }
                        .buttonStyle(.glass)
                        .tint(on.here && !on.away ? Color.accent : nil)
                        .glassEffectID("device", in: glass)
                    }
                    Button { present(.queue) } label: {
                        Label(s.upNext.first.map { "Up next: \($0.title)" } ?? "Nothing up next", systemImage: "list.bullet").lineLimit(1)
                    }
                    .buttonStyle(.glass)
                    .glassEffectID("queue", in: glass)
                    // An episode whose feed times its words: them, as they are said.
                    if !cues.isEmpty {
                        Button { present(.transcript) } label: { Image(systemName: "quote.bubble") }
                            .buttonStyle(.glass)
                            .glassEffectID("transcript", in: glass)
                            .accessibilityLabel("Transcript")
                    }
                }
                .font(.subheadline.weight(.semibold))
            }
            .padding(.top, Space.l)
            .motion(Springs.move, value: s.on != nil)
            .motion(Springs.move, value: cues.isEmpty)
        }
        .task(id: s.now?.id) { if let n = s.now { model.loadTranscript(Id(n.id)) } }
        .padding(.horizontal, Space.xl)
        .padding(.vertical, Space.s)
        .background {
            if let now = s.now {
                // Square, as every cover, made big enough to cover the screen.
                CoverView(seed: now.seed, url: now.art, side: 64, corner: 0)
                    .aspectRatio(1, contentMode: .fill)
                    .blur(radius: 80)
                    // The theme's wash (ui/theme.toml): faint enough that the
                    // title, the album and the gold read over a pale cover.
                    .opacity(Tokens.washOpacity(dark: scheme == .dark))
                    .ignoresSafeArea()
            }
        }
        .modifier(PullToClose(close: close))
    }

    /// The library's row for what plays, for its menu.
    // What plays, as its row: a song's, an episode's or a sermon's, or one
    // made of what plays — so More is there whatever plays.
    private var track: LibraryEntry? { model.playingEntry }
}

/// The full player pulled down to close it. It follows the finger one to
/// one — down as far as it is pulled, up rubber-banded (`Momentum.band`),
/// there being nothing above it — and let go it closes, judged by where the
/// pull would come to rest (`Momentum`), so a short flick closes it as a
/// long drag does, and a drag that is going back up does not. Otherwise it
/// springs back from where it was let go at the finger's speed
/// (`Springs.released`); a pull caught again on its way back takes over at
/// once. Its own view, so the pull's every frame redraws the offset and not
/// the player.
private struct PullToClose: ViewModifier {
    let close: () -> Void
    @Environment(\.reducesMotion) private var still
    /// How far the player is drawn down by the finger.
    @State private var pull: CGFloat = 0
    /// How tall the player is: the room a pull up is rubber-banded in.
    @State private var tall: CGFloat = 800
    /// How far down a drag must be going to close the player: where it would come to rest.
    static let closes: CGFloat = 120

    func body(content: Content) -> some View {
        content
            .offset(y: pull)
            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { tall = $0 }
            .gesture(DragGesture(minimumDistance: 12).onChanged { v in
                let y = v.translation.height
                // Vertical once it is under way, so a sideways drag is not a pull.
                guard pull != 0 || abs(y) > abs(v.translation.width) else { return }
                pull = y >= 0 ? y : CGFloat(Momentum.band(Double(y), limit: Double(tall)))
            }.onEnded { v in
                let down = v.translation.height
                let rest = down + CGFloat(Momentum.project(Double(v.velocity.height)))
                if rest > Self.closes && v.velocity.height >= 0 && abs(v.translation.width) < down {
                    close()
                } else {
                    let back = still ? Springs.calm : Springs.released(velocity: Double(v.velocity.height), from: Double(pull), to: 0)
                    withAnimation(back) { pull = 0 }
                }
            })
    }
}

/// A scrubber that follows the finger while it is dragged, another device's
/// while one is dragging it, and the player otherwise — ArkUI's `Scrubber`,
/// under the chapter it is in. Dragged, it says where to the session's
/// other devices, and seeks on release.
struct PlayerScrubber: View {
    @EnvironmentObject private var model: Model
    @State private var dragging: Double? = nil

    var body: some View {
        let s = model.player
        let total = Double(max(s.durationMs, 1))
        let chapters = s.now.flatMap { model.chapters[Id($0.id)] } ?? []
        TimelineView(.periodic(from: .now, by: 0.5)) { t in
            let at = dragging ?? Double(s.scrubbing ?? s.position(Int64(t.date.timeIntervalSince1970 * 1000))) / total
            VStack(spacing: 2) {
                // The chapter it is in, for an episode that has them.
                if let c = ChaptersEntry.at(chapters, Int64(at * total)), !c.title.isEmpty {
                    Text(c.title).font(.subheadline).foregroundStyle(.quiet).lineLimit(1)
                }
                Scrubber(value: at, enabled: s.durationMs > 0, label: "Position",
                         left: { formatClock(ms: Int64($0 * total)) },
                         right: { "-" + formatClock(ms: max(s.durationMs - Int64($0 * total), 0)) },
                         onValue: { v in
                             dragging = v
                             model.scrub(Int64(v * total))
                         },
                         onDone: { d in
                             model.seek(Int64(d * total))
                             dragging = nil
                         })
            }
        }
        .task(id: s.now?.id) { if let n = s.now { model.loadChapters([Id(n.id)]) } }
    }
}

/// Previous, play or pause and next, with shuffle and repeat at the ends
/// while this phone is its own session — on glass, in one container, so
/// they read as one control.
struct Transport: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        let s = model.player
        GlassEffectContainer(spacing: Space.l) {
            HStack {
                if s.alone {
                    Button { model.toggleShuffle() } label: { Image(systemName: "shuffle") }
                        .foregroundStyle(s.shuffle ? Color.accentText : .quiet)
                        .accessibilityLabel(s.shuffle ? "Shuffle is on" : "Shuffle is off")
                } else {
                    Color.clear.frame(width: 44)
                }
                Spacer()
                // An episode skips back and on a few seconds; a song goes a track.
                if model.jogs {
                    Button { model.jog(-Model.jogBack) } label: { Image(systemName: "gobackward.15").font(.title) }
                        .accessibilityLabel("Back 15 seconds")
                } else {
                    // |◀ and ▶|, as Android's: a track back and on.
                    Button { model.previous() } label: { Image(systemName: "backward.end.fill").font(.title) }
                        .accessibilityLabel("Previous")
                }
                Spacer()
                Button { model.toggle() } label: {
                    Image(systemName: s.playing ? "pause.fill" : "play.fill")
                        .font(.largeTitle)
                        .contentTransition(.symbolEffect(.replace))
                        .frame(width: 72, height: 72)
                }
                .buttonStyle(.glassProminent)
                .buttonBorderShape(.circle)
                .tint(Color.accent)
                .accessibilityLabel(s.playing ? "Pause" : "Play")
                Spacer()
                if model.jogs {
                    Button { model.jog(Model.jogForward) } label: { Image(systemName: "goforward.30").font(.title) }
                        .accessibilityLabel("Forward 30 seconds")
                } else {
                    Button { model.next() } label: { Image(systemName: "forward.end.fill").font(.title) }
                        .accessibilityLabel("Next")
                }
                Spacer()
                if s.alone {
                    Button { model.cycleRepeat() } label: { Image(systemName: s.repeatMode == .one ? "repeat.1" : "repeat") }
                        .foregroundStyle(s.repeatMode != .off ? Color.accentText : .quiet)
                        .accessibilityLabel(s.repeatMode == .off ? "Repeat is off" : (s.repeatMode == .all ? "Repeating the queue" : "Repeating this song"))
                } else {
                    Color.clear.frame(width: 44)
                }
            }
            .foregroundStyle(.ink)
        }
    }
}

/// The queue: what is playing, and what plays after it in the order it will
/// — dragged into another order by a row's handle, the rest sliding out of
/// its way (ArkUI's Reorderable), taken out from a row's menu, or tapped to
/// play from.
struct QueueSheet: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        let s = model.player
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: Space.s) {
                    if let now = s.now {
                        heading("Now Playing")
                        QueueRow(p: now, current: true).padding(.horizontal, Space.l)
                    }
                    heading(s.shuffle ? "Up Next, Shuffled" : "Up Next").padding(.top, Space.m)
                    if s.upNext.isEmpty {
                        Text("Nothing after this one.").foregroundStyle(.quiet).padding(.horizontal, Space.l)
                    }
                    Reorderable(Self.keyed(s.upNext), key: \.key, onMove: { model.moveUpNext(from: $0, to: $1) }) { row, handle in
                        HStack(spacing: Space.s) {
                            Button { model.jump(row.p.index) } label: {
                                QueueRow(p: row.p, current: false).contentShape(Rectangle())
                            }
                            .buttonStyle(.pressRow)
                            handle
                        }
                        .padding(.leading, Space.l)
                        .padding(.vertical, Space.xs)
                        .contextMenu {
                            Button("Remove from Queue", systemImage: "minus.circle", role: .destructive) {
                                if let i = s.upNext.firstIndex(where: { $0.index == row.p.index }) { model.removeUpNext([i]) }
                            }
                        }
                    }
                }
                .padding(.vertical, Space.m)
            }
            .navigationTitle("Queue")
            .navigationBarTitleDisplayMode(.inline)
        }
    }

    private func heading(_ title: String) -> some View {
        Text(title).font(.headline).padding(.horizontal, Space.l)
    }

    /// A row of what plays next and its key: its track, and how many times
    /// that track is queued before it, so a track queued twice is two rows and
    /// a row keeps its key as it moves.
    struct Keyed {
        let key: String
        let p: Playing
    }

    static func keyed(_ upNext: [Playing]) -> [Keyed] {
        var seen: [String: Int] = [:]
        return upNext.map { p in
            seen[p.id, default: 0] += 1
            return Keyed(key: "\(p.id)#\(seen[p.id]!)", p: p)
        }
    }
}

/// What plays, word for word, as Apple Podcasts' transcript and Apple
/// Music's lyrics have it: every line its feed's captions time, the one
/// being said in the full colour and kept in the middle, those said already
/// dimmed, and who speaks named where that changes. A line tapped plays from
/// it. Scrolled by hand, it stops following the sound — until Follow Along
/// is pressed, or the line playing comes down to the middle of what shows.
/// The position is the player's, counted as the scrubber counts it. A line
/// held asks who its voice is, where voices were told apart.
struct TranscriptSheet: View {
    @EnvironmentObject private var model: Model
    @Environment(\.reducesMotion) private var still
    /// The list keeps the line playing in its middle.
    @State private var following = true
    /// A finger is on the list, or it is still coasting from one.
    @State private var handled = false
    /// The lines that show, by their place.
    @State private var showing = [Int]()
    /// The line whose voice is being named, and the name typed.
    @State private var naming: TranscriptEntry?
    @State private var typed = ""

    var body: some View {
        let s = model.player
        let cues = s.now.flatMap { model.transcripts[Id($0.id)] } ?? []
        NavigationStack {
            Group {
                if cues.isEmpty {
                    ContentUnavailableView("No Transcript", systemImage: "quote.bubble",
                                           description: Text("Its feed has no transcript timed to it."))
                } else {
                    ScrollViewReader { proxy in
                        TimelineView(.periodic(from: .now, by: 0.5)) { t in
                            let at = TranscriptEntry.at(cues, s.scrubbing ?? s.position(Int64(t.date.timeIntervalSince1970 * 1000)))
                            ScrollView {
                                LazyVStack(alignment: .leading, spacing: Space.l) {
                                    ForEach(cues.indices, id: \.self) { i in
                                        Button {
                                            model.seek(cues[i].startMs)
                                            following = true
                                        } label: {
                                            TranscriptLine(cue: cues[i], named: TranscriptEntry.names(cues, i), when: TranscriptLine.When.of(i, at))
                                        }
                                        .buttonStyle(.plain)
                                        .accessibilityHint("Plays from here")
                                        .contextMenu {
                                            if !cues[i].voice.isEmpty {
                                                Button("Who Is This?", systemImage: "person.crop.circle") {
                                                    typed = cues[i].speaker == cues[i].voice ? "" : cues[i].speaker
                                                    naming = cues[i]
                                                }
                                            }
                                        }
                                        .id(i)
                                    }
                                }
                                .scrollTargetLayout()
                                .padding(.horizontal, Space.xl)
                                .padding(.vertical, Space.xxl)
                                .motion(Springs.move, value: at)
                            }
                            .onScrollPhaseChange { _, phase in
                                handled = phase == .tracking || phase == .interacting || phase == .decelerating
                                if phase == .interacting { following = false }
                            }
                            .onScrollTargetVisibilityChange(idType: Int.self) { ids in showing = ids }
                            // The line moved on: followed, it is scrolled to the
                            // middle; let go, it is followed again once it has
                            // caught up with where the hand left the list.
                            .onChange(of: at) { before, now in
                                if !following && !handled && TranscriptEntry.caughtUp(from: before, to: now, showing: showing) { following = true }
                                guard following, let i = now else { return }
                                withAnimation(Springs.move.reduced(still)) { proxy.scrollTo(i, anchor: .center) }
                            }
                            .onAppear { if let i = at { proxy.scrollTo(i, anchor: .center) } }
                            .overlay(alignment: .bottom) {
                                if !following {
                                    Button {
                                        following = true
                                        if let i = at { withAnimation(Springs.move.reduced(still)) { proxy.scrollTo(i, anchor: .center) } }
                                    } label: {
                                        Label("Follow Along", systemImage: "waveform")
                                    }
                                    .buttonStyle(.glassProminent)
                                    .tint(Color.accent)
                                    .padding(.bottom, Space.l)
                                    .motionTransition(.move(edge: .bottom).combined(with: .opacity))
                                }
                            }
                            .motion(Springs.move, value: following)
                        }
                    }
                }
            }
            .background {
                if let now = s.now {
                    CoverView(seed: now.seed, url: now.art, side: 64, corner: 0)
                        .blur(radius: 80)
                        .opacity(0.3)
                        .ignoresSafeArea()
                }
            }
            .navigationTitle("Transcript")
            .navigationBarTitleDisplayMode(.inline)
            .alert("Who Is This?", isPresented: Binding(get: { naming != nil }, set: { if !$0 { naming = nil } }), presenting: naming) { line in
                let media = s.now.map { Id($0.id) }
                let name = { (who: String) in if let m = media { model.nameVoice(m, line.voice, who) } }
                ForEach(media.flatMap { model.people[$0] } ?? [], id: \.name) { p in
                    Button(p.name) { name(p.name) }
                }
                TextField("Name", text: $typed)
                Button("Name") { name(typed) }.disabled(typed.trimmingCharacters(in: .whitespaces).isEmpty)
                Button("Nobody", role: .destructive) { name("") }
                Button("Cancel", role: .cancel) {}
            } message: { line in
                Text("\u{201C}\(line.text)\u{201D} Named once, it is them in every episode they are heard in.")
            }
        }
        .task(id: s.now?.id) {
            if let n = s.now {
                model.loadTranscript(Id(n.id))
                model.loadPeople([Id(n.id)])
            }
        }
    }
}

/// One line of a transcript: who says it, where that changes, over what is
/// said — in the full colour while it is said, dimmer still to come, and
/// dimmest once said.
private struct TranscriptLine: View {
    enum When {
        case said, saying, coming

        /// Line `i` against the line playing, `at`: before it any line is still to come.
        static func of(_ i: Int, _ at: Int?) -> When {
            guard let at = at else { return .coming }
            return i < at ? .said : i == at ? .saying : .coming
        }
    }

    let cue: TranscriptEntry
    let named: Bool
    let when: When

    var body: some View {
        // What is said now in ink, what is coming quieter, and what was said as
        // the disabled colour: gone by, not to be read again.
        let ink: Color = when == .saying ? .ink : when == .coming ? .quiet : .disabled
        VStack(alignment: .leading, spacing: Space.xs) {
            if named {
                Text(cue.speaker).font(.caption.weight(.semibold)).foregroundStyle(Color.accentText)
                    .opacity(when == .said ? 0.6 : 1)
            }
            Text(cue.text).font(.title3.weight(.semibold)).foregroundStyle(ink)
                .multilineTextAlignment(.leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

private struct QueueRow: View {
    let p: Playing
    let current: Bool

    var body: some View {
        HStack(spacing: Space.m) {
            CoverView(seed: p.seed, url: p.art, side: 40, corner: 4).frame(width: 40, height: 40)
            VStack(alignment: .leading) {
                Text(p.title).foregroundStyle(current ? Color.accentText : .ink).lineLimit(1)
                Text(p.artist).font(.caption).foregroundStyle(.quiet).lineLimit(1)
            }
            Spacer()
            if current { Image(systemName: "waveform").foregroundStyle(Color.accentText).accessibilityLabel("Playing") }
        }
    }
}
