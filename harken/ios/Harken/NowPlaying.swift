// What plays, on the lock screen, in Control Center, on a headset's buttons
// and on CarPlay's now-playing screen: `MPNowPlayingInfoCenter` told what the
// screens draw — one [PlayerState], the session's while joined — and
// `MPRemoteCommandCenter`'s commands each one of the model's presses, so
// the lock screen is a remote for the account's session as Android's media
// session is: a pause there pauses whichever device is playing. While the
// sound is on another device the title says where ("Playing on Laptop").
//
// iOS shows a now-playing card for the app whose audio session last played,
// so while the sound has only ever been elsewhere the card may not appear
// at all (PARITY.md); and iOS has no way for an app to hand the hardware
// volume buttons to another device (Android's remote `DeviceInfo`), so they
// stay the phone's own.
import MediaPlayer
import UIKit

@MainActor
final class NowPlaying {
    private weak var model: Model?
    private var shownArt = ""
    private var artwork: MPMediaItemArtwork? = nil
    private var targets = [(MPRemoteCommand, Any)]()

    init(model: Model) {
        self.model = model
        let c = MPRemoteCommandCenter.shared()
        func on(_ command: MPRemoteCommand, _ f: @escaping (Model, MPRemoteCommandEvent) -> Void) {
            command.isEnabled = true
            // Called on the main queue, as MediaPlayer documents.
            let t = command.addTarget { [weak self] e in
                MainActor.assumeIsolated {
                    guard let m = self?.model else { return MPRemoteCommandHandlerStatus.noActionableNowPlayingItem }
                    f(m, e)
                    return MPRemoteCommandHandlerStatus.success
                }
            }
            targets.append((command, t))
        }
        on(c.playCommand) { m, _ in m.resume() }
        on(c.pauseCommand) { m, _ in m.pause() }
        on(c.togglePlayPauseCommand) { m, _ in m.toggle() }
        on(c.nextTrackCommand) { m, _ in m.next() }
        on(c.previousTrackCommand) { m, _ in m.previous() }
        // A headset's stop is a pause: stopping the session would forget a queue nobody asked to lose.
        on(c.stopCommand) { m, _ in m.pause() }
        on(c.changePlaybackPositionCommand) { m, e in
            if let p = e as? MPChangePlaybackPositionCommandEvent { m.lockScreenSeek(Int64(p.positionTime * 1000)) }
        }
        model.onPlayer = { [weak self] s in self?.show(s) }
        show(model.player)
    }

    /// The card drawn again: with nothing queued anywhere, there is none.
    func show(_ s: PlayerState) {
        let center = MPNowPlayingInfoCenter.default()
        guard let now = s.now else {
            center.nowPlayingInfo = nil
            return
        }
        let c = MPRemoteCommandCenter.shared()
        c.nextTrackCommand.isEnabled = s.hasNext
        c.changePlaybackPositionCommand.isEnabled = s.durationMs > 0
        let at = s.position(nowMs())
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: now.title,
            MPMediaItemPropertyArtist: s.lockScreenArtist,
            MPMediaItemPropertyAlbumTitle: now.album,
            MPMediaItemPropertyPlaybackDuration: Double(s.durationMs) / 1000,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: Double(at) / 1000,
            MPNowPlayingInfoPropertyPlaybackRate: s.counting ? 1.0 : 0.0,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
            MPNowPlayingInfoPropertyPlaybackQueueIndex: now.index,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
        ]
        if let a = artwork, shownArt == now.art { info[MPMediaItemPropertyArtwork] = a }
        center.nowPlayingInfo = info
        if shownArt != now.art { fetch(now.art) }
    }

    // The cover, from the same cache the screens draw from, then the card again.
    private func fetch(_ url: String) {
        shownArt = url
        artwork = nil
        guard !url.isEmpty else { return }
        Task {
            guard let image = await Covers.shared.load(url, side: 512), self.shownArt == url else { return }
            self.artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
            if let s = self.model?.player { self.show(s) }
        }
    }
}
