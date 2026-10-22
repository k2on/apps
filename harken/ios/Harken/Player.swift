// The iOS end of harken's one reconciler (`harken/player`, S3b): the same
// rules that drive the Android phone's ExoPlayer and a Home Assistant
// speaker, run in harken's library behind `Player`, which drives whatever
// implements `PhoneSound`. The app has no player of its own yet, so this is
// one that makes no sound; a device that says it cannot play is never handed
// the sound, so nothing here is ever asked to. An AVPlayer that implements
// `PhoneSound` is what the app plays with when it has one.

import Foundation

/// A player that makes no sound and holds nothing.
final class Silent: PhoneSound, @unchecked Sendable {
    func state() -> SoundState {
        SoundState(queue: [], index: -1, playing: false, buffering: false, positionMs: 0, ended: false, from: "")
    }

    func takeStalled() -> Bool { false }

    func tracks() -> String { "[]" }

    func load(queue: String, at: Int64, positionMs: Int64, playing: Bool, from: String) {}

    func jump(at: Int64, positionMs: Int64) {}

    func resume() {}

    func pause() {}

    func seek(ms: Int64) {}

    func wakeIn(ms: Int64) {}

    func volume() -> Loudness? { nil }

    func setVolume(level: Int64, muted: Bool) {}
}

/// The player's screens as harken's player makes them (`harken/player/src/screens.rs`): one row
/// for what plays, one for the device picker, the diagnostics' lines, and every control an `Ask`
/// resolved there against the session. A SwiftUI player screen draws these and decides nothing;
/// the app has none yet, so this is where one would read them.
final class Screens {
    private let player: Player
    private let here: String
    private let server: String

    init(peer: Peer, me: String, here: String, server: String, dir: String?) {
        player = Player(peer: peer, me: me, dir: dir)
        player.attach(sound: Silent())
        self.here = here
        self.server = server
    }

    /// Reconcile, then the rows: what a change of the peer is answered with.
    func step() -> (now: NowPlaying, devices: Devices) {
        _ = player.step()
        return (player.nowPlaying(here: here, server: server), player.picker())
    }

    func lines() -> [Line] { player.numbers() }

    /// A control pressed: nil when it was taken, else why not ("not part of the session": the
    /// app's own player's to do).
    func ask(_ a: Ask) -> String? { player.ask(ask: a) }
}
