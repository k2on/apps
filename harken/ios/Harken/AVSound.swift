// The sound itself: an `AVPlayer` playing one track at a time from the
// server's `/media/…` (range requests, so a seek is a seek), under an audio
// session in the playback category, so it plays with the screen locked, with
// the ring switch on silent and in the background (`UIBackgroundModes:
// audio`). What Android's ExoPlayer and its audio focus are; the queue and
// the rules are [SoundCore]'s and the reconciler's.
//
// **Stopping by itself is said**: an interruption (a call, Siri, another
// app's audio) and the headphones coming out (a route change whose reason is
// the old device unavailable) pause it and tell [SoundCore], which says it
// to the reconciler as a stall — Android's "paused by anything but a press".
// An interruption that ends saying to resume resumes it, as Android's audio
// focus does for a transient loss.
import AVFoundation
import UIKit

/// Everything here is called on the main thread, and calls back on it.
final class AVEngine: NSObject, Engine {
    var onEnd: (() -> Void)?
    var onStall: (() -> Void)?
    var onResume: (() -> Void)?
    var onChange: (() -> Void)?

    private let player = AVPlayer()
    private var observers = [NSKeyValueObservation]()
    private var ended: NSObjectProtocol? = nil
    private var failed: NSObjectProtocol? = nil
    /// Asked to play: `AVPlayer`'s rate is 0 while it waits for its buffer.
    private var wanted = false

    override init() {
        super.init()
        player.automaticallyWaitsToMinimizeStalling = true
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playback, mode: .default)
        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(interrupted(_:)), name: AVAudioSession.interruptionNotification, object: session)
        center.addObserver(self, selector: #selector(routed(_:)), name: AVAudioSession.routeChangeNotification, object: session)
        observers.append(player.observe(\.timeControlStatus, options: [.new]) { [weak self] _, _ in
            DispatchQueue.main.async { self?.onChange?() }
        })
    }

    // MARK: Engine

    func set(_ url: URL?, at ms: Int64, playing: Bool) {
        if let o = ended { NotificationCenter.default.removeObserver(o) }
        if let o = failed { NotificationCenter.default.removeObserver(o) }
        ended = nil
        failed = nil
        guard let url = url else {
            player.replaceCurrentItem(with: nil)
            wanted = false
            return
        }
        let item = AVPlayerItem(asset: AVURLAsset(url: url))
        item.preferredForwardBufferDuration = 30
        ended = NotificationCenter.default.addObserver(forName: AVPlayerItem.didPlayToEndTimeNotification, object: item, queue: .main) { [weak self] _ in
            self?.onEnd?()
        }
        // A stream that cannot be fetched stopped by itself.
        failed = NotificationCenter.default.addObserver(forName: AVPlayerItem.failedToPlayToEndTimeNotification, object: item, queue: .main) { [weak self] _ in
            self?.wanted = false
            self?.onStall?()
        }
        player.replaceCurrentItem(with: item)
        if ms > 0 { player.seek(to: CMTime(value: ms, timescale: 1000), toleranceBefore: .zero, toleranceAfter: .zero) }
        wanted = playing
        if playing { play() } else { player.pause() }
    }

    func play() {
        try? AVAudioSession.sharedInstance().setActive(true)
        wanted = true
        player.play()
    }

    func pause() {
        wanted = false
        player.pause()
    }

    func seek(_ ms: Int64) {
        player.seek(to: CMTime(value: max(ms, 0), timescale: 1000), toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] _ in
            DispatchQueue.main.async { self?.onChange?() }
        }
    }

    var positionMs: Int64 {
        let t = player.currentTime()
        return t.isValid && t.isNumeric ? Int64(t.seconds * 1000) : 0
    }

    var sounding: Bool { player.timeControlStatus == .playing }

    var buffering: Bool { wanted && player.timeControlStatus == .waitingToPlayAtSpecifiedRate }

    var gain: Float {
        get { player.volume }
        set { player.volume = newValue }
    }

    var muted: Bool {
        get { player.isMuted }
        set { player.isMuted = newValue }
    }

    // MARK: stopping by itself

    @objc private func interrupted(_ n: Notification) {
        guard let raw = n.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt, let type = AVAudioSession.InterruptionType(rawValue: raw) else { return }
        let opts = (n.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt).map(AVAudioSession.InterruptionOptions.init) ?? []
        DispatchQueue.main.async {
            switch type {
            case .began:
                guard self.wanted else { return }
                self.wanted = false
                self.onStall?()
            case .ended:
                if opts.contains(.shouldResume) { self.onResume?() }
            @unknown default: break
            }
        }
    }

    @objc private func routed(_ n: Notification) {
        guard let raw = n.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
              AVAudioSession.RouteChangeReason(rawValue: raw) == .oldDeviceUnavailable else { return }
        // The headphones out: paused, as a phone always has, and said.
        DispatchQueue.main.async {
            guard self.wanted else { return }
            self.player.pause()
            self.wanted = false
            self.onStall?()
        }
    }
}
