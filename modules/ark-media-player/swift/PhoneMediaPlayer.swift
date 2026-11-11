// ark-media-player on the iPhone, the iPad and the Mac
// (modules/ark-media-player/README.md): the `NativeMediaPlayer` protocol
// UniFFI writes, over
//
// - `AVPlayer`, one item at a time, from a file or a stream (range
//   requests, so a seek is a seek);
// - the audio session (iPhone and iPad) in the playback category for
//   long-form audio, so it plays with the screen locked, the ring switch on
//   silent and in the background (`UIBackgroundModes: audio`, module.toml),
//   and its interruptions (a call, Siri, an alarm) and route changes (the
//   headphones out) said;
// - `MPNowPlayingInfoCenter`, what the lock screen, Control Center, the
//   Dynamic Island, a paired Apple Watch's Now Playing and CarPlay's
//   now-playing screen show; and `MPRemoteCommandCenter`, whose commands —
//   theirs, and a headset's — are each said as a `Remote`, never done here.
//   Its picture is always one: the artwork, or — with none, or none to be
//   had — ArkUI's derived square of the app's `placeholder` seed, in the
//   palette the app gave (`squares`), the dark side's, as the card is drawn
//   over a dimmed wallpaper. Never an empty tile.
//
// Every call returns at once and is done on the main queue; every event is
// told from it. An app that lists `media-player` in its modules.toml
// compiles this file in and hands one to its bridge, which registers it as
// the port (`ark_media_player::phone::port`).
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif
import ArkUI
import AVFoundation
import MediaPlayer
import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

public final class PhoneMediaPlayer: NSObject, NativeMediaPlayer, @unchecked Sendable {
    private let player = AVPlayer()
    private var listener: MediaListener?
    private var info: NowPlaying?
    private var controls = MediaControls(hasNext: false, hasPrevious: false, skipForwardS: 0, skipBackS: 0, canSeek: false, canRate: false, canAdd: false)
    /// The rate it plays at when it plays.
    private var rate: Float = 1
    /// Asked to play: `AVPlayer`'s rate is 0 while it waits for its buffer.
    private var wanted = false
    private var durationMs: Int64 = 0
    private var observers = [NSKeyValueObservation]()
    private var itemObservers = [NSKeyValueObservation]()
    private var itemNotes = [NSObjectProtocol]()
    private var ticker: Any?
    private var commands = [(MPRemoteCommand, Any)]()
    /// What the artwork shown was made of: its address and the placeholder's seed.
    private var artworkOf: (url: String?, seed: String?)?
    private var artwork: MPMediaItemArtwork?
    /// The derived square's palette, as the app's screens draw it.
    private let squares: Squares
    /// What a car or a watch was given to browse, by parent: kept for CarPlay (README.md).
    private var children = [String: [BrowseItem]]()

    /// A player whose placeholder artwork is drawn in `squares`, the app's palette.
    public init(squares: Squares = .neutral) {
        self.squares = squares
        super.init()
        player.automaticallyWaitsToMinimizeStalling = true
        DispatchQueue.main.async { self.setUp() }
    }

    deinit {
        for (c, t) in commands { c.removeTarget(t) }
        if let t = ticker { player.removeTimeObserver(t) }
        NotificationCenter.default.removeObserver(self)
    }

    private func tell(_ e: MediaEvent) {
        listener?.heard(event: e)
    }

    private func setUp() {
        #if os(iOS)
        let session = AVAudioSession.sharedInstance()
        // Long-form audio: music, a podcast, a book; routed as such (AirPlay).
        try? session.setCategory(.playback, mode: .default, policy: .longFormAudio)
        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(interrupted(_:)), name: AVAudioSession.interruptionNotification, object: session)
        center.addObserver(self, selector: #selector(routed(_:)), name: AVAudioSession.routeChangeNotification, object: session)
        #endif
        observers.append(player.observe(\.timeControlStatus, options: [.new]) { [weak self] p, _ in
            DispatchQueue.main.async { self?.statusChanged(p.timeControlStatus) }
        })
        ticker = player.addPeriodicTimeObserver(forInterval: CMTime(value: 1, timescale: 2), queue: .main) { [weak self] t in
            guard let self = self, self.player.timeControlStatus == .playing, t.isNumeric else { return }
            self.tell(.position(atMs: Int64(t.seconds * 1000)))
        }
        handleCommands()
    }

    private var positionMs: Int64 {
        let t = player.currentTime()
        return t.isValid && t.isNumeric ? Int64(max(t.seconds, 0) * 1000) : 0
    }

    private func statusChanged(_ s: AVPlayer.TimeControlStatus) {
        switch s {
        case .playing: tell(.playing)
        case .waitingToPlayAtSpecifiedRate: if wanted { tell(.waiting) }
        case .paused: tell(.paused)
        @unknown default: break
        }
        showInfo()
    }

    // MARK: NativeMediaPlayer

    public func load(url: String, atMs: Int64) {
        DispatchQueue.main.async {
            self.forgetItem()
            self.wanted = false
            self.durationMs = 0
            guard let u = URL(string: url) else {
                self.tell(.failed(why: "not an address: \(url)"))
                return
            }
            let item = AVPlayerItem(asset: AVURLAsset(url: u))
            item.preferredForwardBufferDuration = 30
            self.itemObservers.append(item.observe(\.status, options: [.new]) { [weak self] i, _ in
                DispatchQueue.main.async { self?.itemStatus(i) }
            })
            let center = NotificationCenter.default
            self.itemNotes.append(center.addObserver(forName: AVPlayerItem.didPlayToEndTimeNotification, object: item, queue: .main) { [weak self] _ in
                self?.wanted = false
                self?.tell(.ended)
            })
            self.itemNotes.append(center.addObserver(forName: AVPlayerItem.failedToPlayToEndTimeNotification, object: item, queue: .main) { [weak self] n in
                self?.wanted = false
                let e = n.userInfo?[AVPlayerItemFailedToPlayToEndTimeErrorKey] as? Error
                self?.tell(.failed(why: e?.localizedDescription ?? "the stream stopped"))
            })
            self.player.replaceCurrentItem(with: item)
            if atMs > 0 { self.player.seek(to: CMTime(value: atMs, timescale: 1000), toleranceBefore: .zero, toleranceAfter: .zero) }
            self.player.pause()
        }
    }

    private func forgetItem() {
        itemObservers.removeAll()
        for n in itemNotes { NotificationCenter.default.removeObserver(n) }
        itemNotes.removeAll()
    }

    private func itemStatus(_ i: AVPlayerItem) {
        guard i === player.currentItem else { return }
        switch i.status {
        case .readyToPlay:
            let d = i.duration
            durationMs = d.isNumeric ? Int64(d.seconds * 1000) : 0
            tell(.loaded(durationMs: durationMs))
            showInfo()
        case .failed:
            wanted = false
            tell(.failed(why: i.error?.localizedDescription ?? "the sound could not be loaded"))
        default: break
        }
    }

    public func play() {
        DispatchQueue.main.async {
            #if os(iOS)
            try? AVAudioSession.sharedInstance().setActive(true)
            #endif
            self.wanted = true
            self.player.playImmediately(atRate: self.rate)
        }
    }

    public func pause() {
        DispatchQueue.main.async {
            self.wanted = false
            self.player.pause()
        }
    }

    public func seek(atMs: Int64) {
        DispatchQueue.main.async {
            self.player.seek(to: CMTime(value: max(atMs, 0), timescale: 1000), toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] _ in
                DispatchQueue.main.async {
                    guard let self = self else { return }
                    self.tell(.position(atMs: self.positionMs))
                    self.showInfo()
                }
            }
        }
    }

    public func setRate(rate: Float) {
        DispatchQueue.main.async {
            self.rate = rate > 0 ? rate : 1
            if #available(iOS 16, macOS 13, *) { self.player.defaultRate = self.rate }
            if self.wanted { self.player.rate = self.rate }
            self.showInfo()
        }
    }

    public func setVolume(volume: Float) {
        DispatchQueue.main.async { self.player.volume = min(max(volume, 0), 1) }
    }

    public func stop() {
        DispatchQueue.main.async {
            self.wanted = false
            self.forgetItem()
            self.player.replaceCurrentItem(with: nil)
            #if os(iOS)
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
            #endif
            self.showInfo()
        }
    }

    public func setNowPlaying(info: NowPlaying?) {
        DispatchQueue.main.async {
            self.info = info
            self.showInfo()
        }
    }

    public func setControls(controls: MediaControls) {
        DispatchQueue.main.async {
            self.controls = controls
            self.enableCommands()
        }
    }

    public func setChildren(parent: String, items: [BrowseItem]) {
        DispatchQueue.main.async { self.children[parent] = items }
    }

    public func setListener(listener: MediaListener?) {
        DispatchQueue.main.async { self.listener = listener }
    }

    // MARK: what the system shows

    private func showInfo() {
        let center = MPNowPlayingInfoCenter.default()
        guard let i = info else {
            center.nowPlayingInfo = nil
            #if os(macOS)
            center.playbackState = .stopped
            #endif
            return
        }
        // Its own sound says where it is; a sound made elsewhere is as the app says.
        let own = player.currentItem != nil
        let sounding = own ? player.timeControlStatus == .playing : i.rate > 0
        let at = own ? positionMs : i.positionMs
        let duration = own && durationMs > 0 ? durationMs : i.durationMs
        var d: [String: Any] = [
            MPMediaItemPropertyTitle: i.title,
            MPMediaItemPropertyArtist: i.artist,
            MPMediaItemPropertyAlbumTitle: i.album,
            MPMediaItemPropertyPlaybackDuration: Double(duration) / 1000,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: Double(at) / 1000,
            MPNowPlayingInfoPropertyPlaybackRate: sounding ? Double(own ? rate : i.rate) : 0.0,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
        ]
        let made = artworkOf.map { $0.url == i.artwork && $0.seed == i.placeholder } ?? false
        if let a = artwork, made { d[MPMediaItemPropertyArtwork] = a }
        center.nowPlayingInfo = d
        #if os(macOS)
        center.playbackState = sounding ? .playing : .paused
        #endif
        if !made { fetchArtwork(i.artwork, seed: i.placeholder) }
    }

    // The artwork from its address, then the card again; with none, or none to be had, the
    // placeholder at once.
    private func fetchArtwork(_ url: String?, seed: String?) {
        artworkOf = (url, seed)
        artwork = nil
        guard let s = url, let u = URL(string: s) else {
            if placeholder(seed) { showInfo() }
            return
        }
        URLSession.shared.dataTask(with: u) { [weak self] data, _, _ in
            DispatchQueue.main.async {
                guard let self = self, let was = self.artworkOf, was.url == s, was.seed == seed else { return }
                #if canImport(UIKit)
                let image = data.flatMap { UIImage(data: $0) }
                #else
                let image = data.flatMap { NSImage(data: $0) }
                #endif
                if let image = image {
                    self.artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
                } else if !self.placeholder(seed) {
                    return
                }
                self.showInfo()
            }
        }.resume()
    }

    /// How many pixels across the placeholder is drawn: a lock screen's cover.
    private static let placeholderPx: CGFloat = 512

    /// ArkUI's derived square of `seed` made the artwork, on the main queue; false for no seed,
    /// or one SwiftUI could not render.
    private func placeholder(_ seed: String?) -> Bool {
        guard let seed = seed else { return false }
        return MainActor.assumeIsolated {
            guard #available(iOS 16, macOS 13, *) else { return false }
            let side = PhoneMediaPlayer.placeholderPx
            let renderer = ImageRenderer(
                content: Square(seed: seed)
                    .frame(width: side, height: side)
                    .environment(\.squares, squares)
                    .environment(\.colorScheme, .dark))
            renderer.scale = 1
            #if canImport(UIKit)
            guard let image = renderer.uiImage else { return false }
            #else
            guard let image = renderer.nsImage else { return false }
            #endif
            artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
            return true
        }
    }

    // MARK: the system's controls

    private func handleCommands() {
        let c = MPRemoteCommandCenter.shared()
        func on(_ command: MPRemoteCommand, _ f: @escaping (MPRemoteCommandEvent) -> RemoteCommand?) {
            let t = command.addTarget { [weak self] e in
                guard let self = self, let r = f(e) else { return .commandFailed }
                self.tell(.remote(command: r))
                return .success
            }
            commands.append((command, t))
        }
        on(c.playCommand) { _ in .play }
        on(c.pauseCommand) { _ in .pause }
        on(c.togglePlayPauseCommand) { _ in .toggle }
        on(c.stopCommand) { _ in .stop }
        on(c.nextTrackCommand) { _ in .next }
        on(c.previousTrackCommand) { _ in .previous }
        on(c.skipForwardCommand) { e in .skipForward(seconds: UInt32((e as? MPSkipIntervalCommandEvent)?.interval ?? 15)) }
        on(c.skipBackwardCommand) { e in .skipBack(seconds: UInt32((e as? MPSkipIntervalCommandEvent)?.interval ?? 15)) }
        on(c.changePlaybackPositionCommand) { e in
            (e as? MPChangePlaybackPositionCommandEvent).map { .seekTo(atMs: Int64($0.positionTime * 1000)) }
        }
        on(c.changePlaybackRateCommand) { e in (e as? MPChangePlaybackRateCommandEvent).map { .rate(rate: $0.playbackRate) } }
        enableCommands()
    }

    private func enableCommands() {
        let c = MPRemoteCommandCenter.shared()
        let k = controls
        c.playCommand.isEnabled = true
        c.pauseCommand.isEnabled = true
        c.togglePlayPauseCommand.isEnabled = true
        c.stopCommand.isEnabled = true
        // Skips of seconds stand in for the tracks (an episode's), as the system draws one pair.
        c.nextTrackCommand.isEnabled = k.hasNext && k.skipForwardS == 0
        c.previousTrackCommand.isEnabled = k.hasPrevious && k.skipBackS == 0
        c.skipForwardCommand.isEnabled = k.skipForwardS > 0
        c.skipBackwardCommand.isEnabled = k.skipBackS > 0
        if k.skipForwardS > 0 { c.skipForwardCommand.preferredIntervals = [NSNumber(value: k.skipForwardS)] }
        if k.skipBackS > 0 { c.skipBackwardCommand.preferredIntervals = [NSNumber(value: k.skipBackS)] }
        c.changePlaybackPositionCommand.isEnabled = k.canSeek
        c.changePlaybackRateCommand.isEnabled = k.canRate
        if k.canRate { c.changePlaybackRateCommand.supportedPlaybackRates = [0.75, 1, 1.25, 1.5, 2] }
    }

    // MARK: what the system does to it

    #if os(iOS)
    @objc private func interrupted(_ n: Notification) {
        guard let raw = n.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
              let type = AVAudioSession.InterruptionType(rawValue: raw) else { return }
        let opts = (n.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt).map(AVAudioSession.InterruptionOptions.init) ?? []
        DispatchQueue.main.async {
            switch type {
            case .began:
                guard self.wanted else { return }
                self.wanted = false
                self.tell(.interrupted)
            case .ended:
                // Over, and the system says to go on: it goes on, and says so.
                guard opts.contains(.shouldResume), self.player.currentItem != nil else { return }
                try? AVAudioSession.sharedInstance().setActive(true)
                self.wanted = true
                self.player.playImmediately(atRate: self.rate)
                self.tell(.resumed)
            @unknown default: break
            }
        }
    }

    @objc private func routed(_ n: Notification) {
        guard let raw = n.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
              let reason = AVAudioSession.RouteChangeReason(rawValue: raw) else { return }
        DispatchQueue.main.async {
            switch reason {
            case .oldDeviceUnavailable:
                // The headphones out: paused, as a phone always has, and said.
                self.player.pause()
                self.wanted = false
                self.tell(.route(change: .unplugged))
            case .newDeviceAvailable: self.tell(.route(change: .plugged))
            default: self.tell(.route(change: .other))
            }
        }
    }
    #endif
}
