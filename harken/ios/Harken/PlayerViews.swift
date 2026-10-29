// What is playing, two ways — the mini player on glass above the tab bar
// (the tab view's bottom accessory) and the full player it zooms into — and
// the queue, and an episode's transcript as it is said. Both draw one [PlayerState]; neither knows whether the sound is
// here or on another device of the account but by what that state says
// (`on`). The position is counted from the state's clock as it is drawn
// (`TimelineView`), so only the hairline and the scrubber move with it.
// Android's `PlayerScreens.kt` and `PlayerSheet.kt`: a sideways drag on the
// bar skips — left the next track, right the one before — and a tap opens
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
    @State private var swipe: CGFloat = 0

    var body: some View {
        let s = model.player
        let compact = placement == .inline
        GeometryReader { geo in
            HStack(spacing: Space.m) {
                if let now = s.now {
                    CoverView(seed: now.seed, url: now.art, side: compact ? 28 : 36, corner: 6)
                        .frame(width: compact ? 28 : 36, height: compact ? 28 : 36)
                    VStack(alignment: .leading, spacing: 0) {
                        Text(now.title).font(.subheadline.weight(.semibold)).lineLimit(1)
                        if !compact {
                            Text(s.under).font(.caption).foregroundStyle(s.on.map { !$0.here } == true ? Color.gold : .secondary).lineLimit(1)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .offset(x: swipe)
                    Button { model.toggle() } label: {
                        Image(systemName: s.playing ? "pause.fill" : "play.fill").contentTransition(.symbolEffect(.replace)).frame(width: 32, height: 32)
                    }
                    .accessibilityLabel(s.playing ? "Pause" : "Play")
                    if !compact && model.jogs {
                        Button { model.jog(Model.jogForward) } label: { Image(systemName: "goforward.30").frame(width: 32, height: 32) }
                            .accessibilityLabel("Forward 30 seconds")
                    } else if !compact {
                        Button { model.next() } label: { Image(systemName: "forward.fill").frame(width: 32, height: 32) }
                            .disabled(!s.hasNext)
                            .accessibilityLabel("Next")
                    }
                } else {
                    Image(systemName: "music.note").foregroundStyle(.secondary).frame(width: 36)
                    Text("Not Playing").font(.subheadline).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(.horizontal, Space.m)
            .frame(maxHeight: .infinity)
            .overlay(alignment: .bottom) { if s.now != nil && !compact { Hairline().padding(.horizontal, Space.l) } }
            .contentShape(Rectangle())
            .onTapGesture { if s.now != nil { open() } }
            // A sideways drag skips: far enough, or fast enough; then it springs back.
            .gesture(DragGesture(minimumDistance: 12).onChanged { v in
                if abs(v.translation.width) > abs(v.translation.height) { swipe = v.translation.width }
            }.onEnded { v in
                let wide = Springs.wide * geo.size.width
                let fast = v.predictedEndTranslation.width - v.translation.width
                if swipe < -wide || fast < -Springs.flick * 0.25 { model.next() }
                else if swipe > wide || fast > Springs.flick * 0.25 { model.previous() }
                withAnimation(Springs.move) { swipe = 0 }
            })
        }
        .accessibilityElement(children: .contain)
        .accessibilityAction(named: "Open the player") { open() }
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
                Capsule().fill(.quaternary)
                    .overlay(alignment: .leading) { Capsule().fill(Color.gold).frame(width: g.size.width * done) }
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
    @Namespace private var glass

    var body: some View {
        let s = model.player
        let cues = s.now.flatMap { model.transcripts[Id($0.id)] } ?? []
        VStack(spacing: Space.l) {
            HStack {
                Button(action: close) { Image(systemName: "chevron.down") }
                    .buttonStyle(.glass)
                    .accessibilityLabel("Close the player")
                Spacer()
                Text(s.now.map { $0.album.isEmpty ? "Now Playing" : $0.album } ?? "Now Playing")
                    .font(.footnote.weight(.semibold)).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
                if let t = track {
                    Menu { TrackActions(track: t, from: nil) } label: { Image(systemName: "ellipsis") }
                        .buttonStyle(.glass)
                        .accessibilityLabel("More")
                } else {
                    Color.clear.frame(width: 44, height: 44)
                }
            }
            Spacer(minLength: 0)
            if let now = s.now {
                CoverView(seed: now.seed, url: now.art, side: 360, corner: 16)
                    .aspectRatio(1, contentMode: .fit)
                    .shadow(color: .black.opacity(0.25), radius: 24, y: 12)
                    .scaleEffect(s.playing ? 1 : 0.9)
                    .animation(Springs.move, value: s.playing)
            }
            Spacer(minLength: 0)
            // The title opens its album and the name its person's page, as
            // the desktop's bar and Android's player do; opening one closes
            // the player (`Shell`).
            VStack(alignment: .leading, spacing: 2) {
                Button { if let n = s.now, let a = model.album(playing: n) { open(.page(.album(a.id))) } } label: {
                    Text(s.now?.title ?? "Nothing playing").font(.title2.bold()).lineLimit(2).multilineTextAlignment(.leading)
                }
                .buttonStyle(.plain)
                // An episode's name is its show's, and the show is its page.
                Button {
                    guard let a = s.now?.artist, !a.isEmpty else { return }
                    let show = model.shows.first { $0.title == a }
                    open(.page(show != nil && model.artist(named: a) == nil ? .show(show!.id) : .artist(a)))
                } label: {
                    Text(s.now?.artist ?? "").font(.title3).foregroundStyle(Color.gold).lineLimit(1)
                }
                .buttonStyle(.plain)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Scrubber()
            Transport()
            if let v = s.volume {
                VolumeSlider(level: v.level, muted: v.muted, about: s.on?.name ?? "this phone",
                             onLevel: { model.volume($0) }, onDone: { model.volumeDone() }, onMute: { model.mute() })
            }
            GlassEffectContainer(spacing: Space.s) {
                HStack(spacing: Space.s) {
                    if let on = s.on {
                        Button { present(.devices) } label: {
                            Label(on.label, systemImage: symbol(forDevice: on.kind, name: on.name)).lineLimit(1)
                        }
                        .buttonStyle(.glass)
                        .tint(on.here && !on.away ? .gold : nil)
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
            .animation(Springs.move, value: s.on != nil)
            .animation(Springs.move, value: cues.isEmpty)
        }
        .task(id: s.now?.id) { if let n = s.now { model.loadTranscript(Id(n.id)) } }
        .padding(.horizontal, Space.xl)
        .padding(.vertical, Space.s)
        .background {
            if let now = s.now {
                CoverView(seed: now.seed, url: now.art, side: 64, corner: 0)
                    .blur(radius: 80)
                    .opacity(0.45)
                    .ignoresSafeArea()
            }
        }
        .gesture(DragGesture(minimumDistance: 40).onEnded { v in
            if v.translation.height > 120 && abs(v.translation.width) < v.translation.height { close() }
        })
    }

    /// The library's row for what plays, for its menu.
    private var track: LibraryEntry? {
        guard let id = model.player.now?.id else { return nil }
        return model.tracks.first { $0.id.hex == id }
    }
}

/// A scrubber that follows the finger while it is dragged, another device's
/// while one is dragging it, and the player otherwise. Dragged, it says where
/// to the session's other devices, and seeks on release.
struct Scrubber: View {
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
                    Text(c.title).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
                Slider(value: Binding(get: { min(max(at, 0), 1) }, set: { v in
                    dragging = v
                    model.scrub(Int64(v * total))
                }), in: 0...1, onEditingChanged: { editing in
                    if !editing, let d = dragging {
                        model.seek(Int64(d * total))
                        dragging = nil
                    }
                })
                .disabled(s.durationMs <= 0)
                .tint(.gold)
                HStack {
                    Text(Spell.clock(Int64(at * total)))
                    Spacer()
                    Text("-" + Spell.clock(max(s.durationMs - Int64(at * total), 0)))
                }
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
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
                        .foregroundStyle(s.shuffle ? Color.gold : .secondary)
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
                    Button { model.previous() } label: { Image(systemName: "backward.fill").font(.title) }
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
                .tint(.gold)
                .accessibilityLabel(s.playing ? "Pause" : "Play")
                Spacer()
                if model.jogs {
                    Button { model.jog(Model.jogForward) } label: { Image(systemName: "goforward.30").font(.title) }
                        .accessibilityLabel("Forward 30 seconds")
                } else {
                    Button { model.next() } label: { Image(systemName: "forward.fill").font(.title) }
                        .accessibilityLabel("Next")
                }
                Spacer()
                if s.alone {
                    Button { model.cycleRepeat() } label: { Image(systemName: s.repeatMode == .one ? "repeat.1" : "repeat") }
                        .foregroundStyle(s.repeatMode != .off ? Color.gold : .secondary)
                        .accessibilityLabel(s.repeatMode == .off ? "Repeat is off" : (s.repeatMode == .all ? "Repeating the queue" : "Repeating this song"))
                } else {
                    Color.clear.frame(width: 44)
                }
            }
            .foregroundStyle(.primary)
        }
    }
}

/// A volume: its mute, then its slider. Under the finger it is where the
/// finger is; anywhere else it is where the level it is handed says, and a
/// level that arrives from elsewhere is sprung to (`Springs.move`) rather than
/// jumped to.
struct VolumeSlider: View {
    let level: Int
    let muted: Bool
    let about: String
    let onLevel: (Int) -> Void
    let onDone: () -> Void
    let onMute: () -> Void
    @State private var shown: Double = 0
    @State private var dragging = false

    var body: some View {
        HStack(spacing: Space.s) {
            Button(action: onMute) { Image(systemName: muted ? "speaker.slash.fill" : "speaker.wave.2.fill").frame(width: 28) }
                .foregroundStyle(muted ? .secondary : .primary)
                .accessibilityLabel(muted ? "Unmute \(about)" : "Mute \(about)")
            Slider(value: Binding(get: { shown }, set: { v in
                dragging = true
                shown = v
                onLevel(Int((v * 100).rounded()))
            }), in: 0...1, onEditingChanged: { editing in
                if !editing {
                    dragging = false
                    onDone()
                }
            })
            .tint(muted ? .secondary : .gold)
            .accessibilityLabel("Volume of \(about)")
        }
        .onAppear { shown = Double(level) / 100 }
        .onChange(of: level) { _, l in if !dragging { withAnimation(Springs.move) { shown = Double(l) / 100 } } }
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
                        Text("Nothing after this one.").foregroundStyle(.secondary).padding(.horizontal, Space.l)
                    }
                    Reorderable(Self.keyed(s.upNext), key: \.key, onMove: { model.moveUpNext(from: $0, to: $1) }) { row, handle in
                        HStack(spacing: Space.s) {
                            QueueRow(p: row.p, current: false)
                                .contentShape(Rectangle())
                                .onTapGesture { model.jump(row.p.index) }
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
                                .animation(Springs.move, value: at)
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
                                withAnimation(.smooth) { proxy.scrollTo(i, anchor: .center) }
                            }
                            .onAppear { if let i = at { proxy.scrollTo(i, anchor: .center) } }
                            .overlay(alignment: .bottom) {
                                if !following {
                                    Button {
                                        following = true
                                        if let i = at { withAnimation(.smooth) { proxy.scrollTo(i, anchor: .center) } }
                                    } label: {
                                        Label("Follow Along", systemImage: "waveform")
                                    }
                                    .buttonStyle(.glassProminent)
                                    .tint(.gold)
                                    .padding(.bottom, Space.l)
                                    .transition(.move(edge: .bottom).combined(with: .opacity))
                                }
                            }
                            .animation(Springs.move, value: following)
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
        let ink: HierarchicalShapeStyle = when == .saying ? .primary : when == .coming ? .secondary : .tertiary
        VStack(alignment: .leading, spacing: Space.xs) {
            if named {
                Text(cue.speaker.uppercased()).font(.caption.weight(.semibold)).foregroundStyle(Color.gold)
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
                Text(p.title).foregroundStyle(current ? Color.gold : .primary).lineLimit(1)
                Text(p.artist).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            if current { Image(systemName: "waveform").foregroundStyle(Color.gold).accessibilityLabel("Playing") }
        }
    }
}
