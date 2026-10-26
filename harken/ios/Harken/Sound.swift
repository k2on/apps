// This phone's player as harken's reconciler drives it (`PhoneSound`, the
// FFI's callback interface, S3b), and nothing of Apple's but Foundation: the
// queue, where in it, whether it was asked to play, where it is and whether
// it stopped by itself — kept here as one value, read by the reconciler on the
// session's queue — over an [Engine] that makes the sound one track at a
// time on the main thread (AVSound.swift: `AVPlayer` and the audio session).
// What the reconciler tells it is written into the value at once, so the next
// look sees what it has just asked, and then done on the main thread, after
// which the engine is read again and the reconciler poked. The twin of
// Android's `ExoSound` (Playback.kt).
//
// It plays through its queue on its own, as a phone's player does: a track
// that ends is followed by the next — or, alone, by itself or the first
// again, as repeat says — and the reconciler sees the move and says it
// (`advance`). Alone — no session, or the link down — the screens drive it
// directly ([start], [next], …), as Android's screens drive the ExoPlayer
// through its controller; the reconciler then reads what it plays as this
// phone's own session.
import Foundation

/// What makes the sound: one track at a time, on the main thread.
protocol Engine: AnyObject {
    /// Told on the main thread: the track ended by itself.
    var onEnd: (() -> Void)? { get set }
    /// …it stopped by itself — an interruption, the headphones out, a stream that failed.
    var onStall: (() -> Void)? { get set }
    /// …it went on by itself — an interruption over that says to resume.
    var onResume: (() -> Void)? { get set }
    /// …anything about it moved: its rate, its buffer, its item's status.
    var onChange: (() -> Void)? { get set }

    /// Make `url` the track, at `ms`, playing or not; none clears it.
    func set(_ url: URL?, at ms: Int64, playing: Bool)
    func play()
    func pause()
    func seek(_ ms: Int64)
    /// Where in the track, in ms.
    var positionMs: Int64 { get }
    /// Sounding now: asked to play, and not waiting for its buffer.
    var sounding: Bool { get }
    /// Filling a buffer.
    var buffering: Bool { get }
    /// This player's own gain, 0 to 1, and its mute: the session's volume for this phone.
    var gain: Float { get set }
    var muted: Bool { get set }
}

/// The reconciler's player over an [Engine].
final class SoundCore: PhoneSound, @unchecked Sendable {
    private struct Said {
        var queue: [Track] = []
        var index = -1
        var playing = false
        var buffering = false
        var positionMs: Int64 = 0
        /// When the position was read, on this phone's monotonic clock.
        var readAt: Double = 0
        /// Sounding as it was read: the position runs on from it.
        var moving = false
        var ended = false
        var from = ""
        var level: Int64 = 100
        var muted = false
    }

    private let lock = NSLock()
    private var said = Said()
    private var stalled = false
    private var inFlight = 0
    private var wake = 0
    private let engine: Engine

    /// Where a track's bytes are: the server's `/media/…` (`Files.url`).
    var urlOf: (Track) -> URL? = { _ in nil }
    /// Told whenever it moved by itself, so the reconciler steps.
    var poke: () -> Void = {}
    /// Told on the main thread whenever what it plays alone moved, for the screens.
    var moved: () -> Void = {}

    /// Alone only: what the session has not got.
    private(set) var repeatMode: Repeat = .off
    private(set) var shuffled = false

    init(engine: Engine) {
        self.engine = engine
        engine.onEnd = { [weak self] in self?.ended() }
        engine.onStall = { [weak self] in
            guard let self = self else { return }
            self.locked { self.stalled = true }
            self.read()
        }
        engine.onResume = { [weak self] in
            guard let self = self else { return }
            self.locked { self.said.playing = true }
            self.engine.play()
            self.read()
        }
        engine.onChange = { [weak self] in self?.read() }
        said.level = Int64((engine.gain * 100).rounded())
    }

    private func locked<T>(_ f: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return f()
    }

    private static func clock() -> Double { ProcessInfo.processInfo.systemUptime }

    // MARK: read, on the main thread

    // What the engine says now, kept; then a reconcile. Not while a command is
    // on its way: what it says meanwhile is from before it.
    private func read() {
        let busy = locked { inFlight > 0 }
        if busy { return }
        let ms = engine.positionMs
        let sounding = engine.sounding
        let buffering = engine.buffering
        locked {
            said.positionMs = max(ms, 0)
            said.readAt = SoundCore.clock()
            said.moving = sounding
            said.buffering = buffering
        }
        poke()
        moved()
    }

    // Told, on the main thread, in order; then read.
    private func command(_ f: @escaping () -> Void) {
        locked { inFlight += 1 }
        DispatchQueue.main.async {
            f()
            self.locked { self.inFlight -= 1 }
            self.read()
        }
    }

    private var position: Int64 {
        let s = said
        if !s.moving { return s.positionMs }
        return s.positionMs + Int64((SoundCore.clock() - s.readAt) * 1000)
    }

    // A track that ended by itself: the next, as repeat says; or the queue's end.
    private func ended() {
        let (q, i, r) = locked { (said.queue, said.index, repeatMode) }
        if r == .one {
            engine.seek(0)
            engine.play()
        } else if i + 1 < q.count {
            locked {
                said.index = i + 1
                said.positionMs = 0
                said.readAt = SoundCore.clock()
            }
            engine.set(urlOf(q[i + 1]), at: 0, playing: true)
        } else if r == .all && !q.isEmpty {
            locked {
                said.index = 0
                said.positionMs = 0
            }
            engine.set(urlOf(q[0]), at: 0, playing: true)
        } else {
            locked {
                said.ended = true
                said.moving = false
            }
        }
        read()
    }

    // MARK: PhoneSound, from the session's queue

    func state() -> SoundState {
        locked {
            SoundState(queue: said.queue.map { $0.id.hex }, index: Int64(said.index), playing: said.playing, buffering: said.buffering,
                       positionMs: position, ended: said.ended, from: said.from)
        }
    }

    func takeStalled() -> Bool {
        locked {
            let s = stalled
            stalled = false
            return s
        }
    }

    func tracks() -> String { locked { J.wlist(said.queue) { $0.json } } }

    func load(queue: String, at: Int64, positionMs: Int64, playing: Bool, from: String) {
        let tracks = J.list(J.parse(queue)) { Track(json: $0) }
        let i = Int(at)
        locked {
            said.queue = tracks
            said.index = i
            said.playing = playing
            said.positionMs = positionMs
            said.readAt = SoundCore.clock()
            said.moving = false
            said.ended = false
            said.from = from
            said.buffering = true
        }
        let url = i >= 0 && i < tracks.count ? urlOf(tracks[i]) : nil
        command { self.engine.set(url, at: positionMs, playing: playing) }
    }

    func jump(at: Int64, positionMs: Int64) {
        let i = Int(at)
        let (q, playing) = locked { () -> ([Track], Bool) in
            said.index = i
            said.positionMs = positionMs
            said.readAt = SoundCore.clock()
            said.moving = false
            said.ended = false
            return (said.queue, said.playing)
        }
        let url = i >= 0 && i < q.count ? urlOf(q[i]) : nil
        command { self.engine.set(url, at: positionMs, playing: playing) }
    }

    func resume() {
        let restart = locked { () -> (URL?, Int64)? in
            said.playing = true
            // At the queue's end: from its last track's start again.
            guard said.ended, said.index >= 0, said.index < said.queue.count else { return nil }
            said.ended = false
            said.positionMs = 0
            return (urlOf(said.queue[said.index]), 0)
        }
        command {
            if let r = restart { self.engine.set(r.0, at: r.1, playing: true) } else { self.engine.play() }
        }
    }

    func pause() {
        locked {
            said.positionMs = position
            said.readAt = SoundCore.clock()
            said.playing = false
            said.moving = false
        }
        command { self.engine.pause() }
    }

    func seek(ms: Int64) {
        locked {
            said.positionMs = ms
            said.readAt = SoundCore.clock()
            said.ended = false
        }
        command { self.engine.seek(ms) }
    }

    func wakeIn(ms: Int64) {
        let n = locked { () -> Int in
            wake += 1
            return wake
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(Int(max(ms, 0)))) {
            if self.locked({ self.wake }) == n { self.poke() }
        }
    }

    /// The session's volume for this phone: its player's own gain, which
    /// another device of the account may move — the system's volume is the
    /// person's, and no app may set it (`AVAudioSession.outputVolume` is read-only).
    func volume() -> Loudness? { locked { Loudness(level: said.level, muted: said.muted) } }

    func setVolume(level: Int64, muted: Bool) {
        locked {
            said.level = min(max(level, 0), 100)
            said.muted = muted
        }
        let gain = Float(min(max(level, 0), 100)) / 100
        DispatchQueue.main.async {
            self.engine.gain = gain
            self.engine.muted = muted
        }
    }

    // MARK: alone, from the screens, on the main thread

    /// What plays now, alone: the queue, where, and where from.
    var queue: (tracks: [Track], index: Int) { locked { (said.queue, said.index) } }

    /// Play `queue` from `at`, from the start: a track pressed, alone.
    func start(_ queue: [Track], at: Int, from: String, shuffled: Bool, positionMs: Int64 = 0) {
        self.shuffled = shuffled
        load(queue: J.wlist(queue) { $0.json }, at: Int64(at), positionMs: positionMs, playing: true, from: from)
    }

    func toggle() {
        let playing = locked { said.playing && !said.ended }
        if playing { pause() } else { resume() }
    }

    func next() {
        let (q, i) = queue
        if i + 1 < q.count { jump(at: Int64(i + 1), positionMs: 0) }
        else if repeatMode != .off, !q.isEmpty { jump(at: 0, positionMs: 0) }
    }

    /// The one before — or, three seconds into this one, its start.
    func previous() {
        let (_, i) = queue
        let at = locked { position }
        if at > 3000 || i <= 0 { seek(ms: 0) } else { jump(at: Int64(i - 1), positionMs: 0) }
    }

    func jumpTo(_ index: Int) {
        let (q, _) = queue
        if index >= 0 && index < q.count { jump(at: Int64(index), positionMs: 0) }
    }

    /// Put `tracks` right after what plays — or at the end — or play them, when nothing does.
    func insert(_ tracks: [Track], next: Bool, from: String) {
        let empty = locked { () -> Bool in
            if said.queue.isEmpty || said.index < 0 { return true }
            let at = next ? said.index + 1 : said.queue.count
            said.queue.insert(contentsOf: tracks, at: min(at, said.queue.count))
            return false
        }
        if empty { start(tracks, at: 0, from: from, shuffled: false) } else { read() }
    }

    func remove(_ index: Int) {
        let gone = locked { () -> Bool in
            guard index >= 0 && index < said.queue.count, index != said.index else { return false }
            said.queue.remove(at: index)
            if index < said.index { said.index -= 1 }
            return true
        }
        if gone { read() }
    }

    func move(from: Int, to: Int) {
        let done = locked { () -> Bool in
            guard from >= 0, from < said.queue.count, to >= 0, to < said.queue.count, from != to else { return false }
            let t = said.queue.remove(at: from)
            said.queue.insert(t, at: to)
            if from == said.index { said.index = to }
            else if from < said.index && to >= said.index { said.index -= 1 }
            else if from > said.index && to <= said.index { said.index += 1 }
            return true
        }
        if done { read() }
    }

    /// What is still to come shuffled — in the player's own queue, so what is drawn is what plays.
    func shuffle() {
        shuffled.toggle()
        if shuffled {
            locked {
                let from = said.index + 1
                if from < said.queue.count - 1 { said.queue.replaceSubrange(from..., with: said.queue[from...].shuffled()) }
            }
        }
        read()
    }

    /// Off, all, one, off.
    func cycleRepeat() {
        repeatMode = repeatMode.next
        moved()
    }
}
