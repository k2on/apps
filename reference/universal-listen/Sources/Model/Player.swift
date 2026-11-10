import SwiftUI

/// Playback: what is playing, where, and what comes next. No audio is
/// produced; the playhead advances on a clock unless the app is launched
/// for screenshots.
@Observable @MainActor
final class Player {
    enum Repeat: String, CaseIterable, Identifiable {
        case off = "Off", all = "All", one = "One"
        var id: String { rawValue }
        var symbol: String { self == .one ? "repeat.1" : "repeat" }
    }

    static let speeds: [Double] = [0.75, 1, 1.25, 1.5, 1.75, 2]

    let library: Library
    var item: PlayItem?
    var upNext: [PlayItem] = []
    var history: [PlayItem] = []
    var isPlaying = true
    var position: TimeInterval = 0
    var volume: Double = 0.62
    var rate: Double = 1
    var shuffle = false
    var repeatMode: Repeat = .off
    var outputID = "living-room"
    /// Bumped on each transport action, for sensory feedback.
    var actionCount = 0

    private var clock: Task<Void, Never>?

    init(library: Library) {
        self.library = library
        load()
        if !Launch.frozen {
            clock = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(for: .seconds(1))
                    self?.tick()
                }
            }
        }
    }

    var info: ItemInfo? { item.flatMap(library.info) }
    var isSpoken: Bool { info?.isSpoken ?? false }
    var output: Device? { library.device(outputID) }
    var duration: TimeInterval { info?.duration ?? 1 }
    var currentTrackID: String? { if case .track(let id) = item { id } else { nil } }
    var currentEpisodeID: String? { if case .episode(let id) = item { id } else { nil } }

    /// The lyric or transcript line at the playhead.
    var currentLineID: Int? {
        guard let lines = info?.lines else { return nil }
        return lines.last { $0.time <= position }?.id
    }

    // MARK: Transport

    func play(_ item: PlayItem, queue: [PlayItem] = []) {
        if let current = self.item { history.append(current) }
        self.item = item
        upNext = queue
        if case .episode(let id) = item, let e = library.episode(id), e.isInProgress {
            position = e.duration * e.progress
        } else {
            position = 0
        }
        rate = isSpoken ? rate : 1
        isPlaying = true
        actionCount += 1
    }

    func play(tracks: [Track], from start: Track? = nil, shuffled: Bool = false) {
        var list = tracks.map { PlayItem.track($0.id) }
        if shuffled { list = list.sorted { Seed.value("\($0)") < Seed.value("\($1)") } }
        let first = start.map { PlayItem.track($0.id) } ?? list.first
        guard let first else { return }
        let rest = list.drop { $0 != first }.dropFirst()
        shuffle = shuffled
        play(first, queue: Array(rest))
    }

    func playNext(_ items: [PlayItem]) { upNext.insert(contentsOf: items, at: 0) }
    func playLater(_ items: [PlayItem]) { upNext.append(contentsOf: items) }

    func togglePlay() {
        guard item != nil else { return }
        isPlaying.toggle()
        actionCount += 1
    }

    func next() {
        guard !upNext.isEmpty else { isPlaying = false; position = 0; return }
        if let current = item { history.append(current) }
        item = upNext.removeFirst()
        position = 0
        actionCount += 1
    }

    func previous() {
        if position > 3 || history.isEmpty { position = 0; return }
        if let current = item { upNext.insert(current, at: 0) }
        item = history.removeLast()
        position = 0
        actionCount += 1
    }

    func skip(_ seconds: TimeInterval) { seek(position + seconds) }

    func seek(_ time: TimeInterval) {
        position = min(max(0, time), duration)
    }

    func nudgeVolume(_ by: Double) { volume = min(1, max(0, volume + by)) }

    func handOff(to deviceID: String) {
        outputID = deviceID
        actionCount += 1
    }

    func removeFromQueue(at offsets: IndexSet) { upNext.remove(atOffsets: offsets) }
    func moveInQueue(from: IndexSet, to: Int) { upNext.move(fromOffsets: from, toOffset: to) }

    private func tick() {
        guard isPlaying, item != nil else { return }
        position += rate
        if position >= duration {
            if repeatMode == .one { position = 0 } else { next() }
        }
    }

    // MARK: Launch state

    private func load() {
        let arg = Launch.playing ?? "track:harbor-lights-2"
        let parts = arg.split(separator: ":", maxSplits: 1).map(String.init)
        let id = parts.count > 1 ? parts[1] : "harbor-lights-2"
        if parts[0] == "episode", let e = library.episode(id), let show = library.show(e.showID) {
            item = .episode(id)
            position = e.isInProgress ? e.duration * e.progress : (e.transcript.dropFirst(5).first?.time ?? 40) + 2
            rate = 1.25
            let more: [PlayItem] = library.episodes(of: show).filter { $0.id != id && $0.progress < 1 }.prefix(4).map { PlayItem.episode($0.id) }
            let extra: [PlayItem] = [PlayItem.episode("vespers-1"), PlayItem.episode("workbench-2")].filter { $0 != PlayItem.episode(id) }
            upNext = more + extra
            history = [.track("clair-de-lune-4")]
        } else if let t = library.track(id), let album = library.album(t.albumID) {
            item = .track(id)
            position = (t.lyrics.dropFirst(6).first?.time ?? 72) + 1
            let rest = album.trackIDs.drop { $0 != id }.dropFirst().map { PlayItem.track($0) }
            let extra: [PlayItem] = [.track("northern-lines-1"), .track("northern-lines-2"), .episode("workbench-1")]
            upNext = Array(rest) + extra
            history = album.trackIDs.prefix { $0 != id }.map { PlayItem.track($0) }
        }
    }
}
