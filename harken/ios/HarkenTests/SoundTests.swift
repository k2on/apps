// This phone's player as harken's one reconciler drives it, end to end in
// the process: real peers of the kernel in a `Hall` (a harken server in the
// process), the FFI's `Player`, and [SoundCore] — the `PhoneSound` the app
// plays through — over an engine that does what it is told and moves only
// when a test says so (what `AVPlayer` is to the phone). Run on Linux by
// `tools/typecheck.sh` when it is handed the library; on a Mac by Xcode.
@testable import Harken
import Foundation
import XCTest

/// An engine that does what it is told.
final class FakeEngine: Engine {
    var onEnd: (() -> Void)?
    var onStall: (() -> Void)?
    var onResume: (() -> Void)?
    var onChange: (() -> Void)?
    var url: URL? = nil
    var playingNow = false
    var positionMs: Int64 = 0
    var sets = 0
    var sounding: Bool { playingNow && url != nil }
    var buffering = false
    var gain: Float = 1
    var muted = false

    func set(_ url: URL?, at ms: Int64, playing: Bool) {
        self.url = url
        positionMs = ms
        playingNow = playing
        sets += 1
    }
    func play() { playingNow = true }
    func pause() { playingNow = false }
    func seek(_ ms: Int64) { positionMs = ms }
}

final class SoundTests: XCTestCase {
    private struct Phone {
        let hall: Hall
        let peer: Peer
        let player: Player
        let sound: SoundCore
        let engine: FakeEngine

        /// The reconciler stepped, the hall carried, and the main queue run, until quiet.
        func settle() {
            for _ in 0..<4 {
                _ = player.step()
                hall.carry()
                RunLoop.main.run(until: Date().addingTimeInterval(0.02))
            }
        }

        func heard() -> Heard { Heard.of(player.view()) }
    }

    private func dir() -> String {
        let d = FileManager.default.temporaryDirectory.appendingPathComponent("harken-tests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d.path
    }

    private func device(_ hall: Hall, _ id: String, canPlay: Bool) -> Phone {
        let d = dir()
        let said = Device(canPlay: canPlay, id: "", kind: canPlay ? "phone" : "computer", name: id, user: "").json
        let peer = try! hall.device(dir: d, user: "alice", session: id, device: said)
        let engine = FakeEngine()
        let sound = SoundCore(engine: engine)
        sound.urlOf = { URL(string: Files.url("https://h", $0.file)) }
        let player = Player(peer: peer, me: id, dir: d)
        player.attach(sound: sound)
        hall.carry()
        return Phone(hall: hall, peer: peer, player: player, sound: sound, engine: engine)
    }

    private let queue = [Fx.track(1), Fx.track(2)]

    /// A track pressed on the phone plays on the phone; a pause pressed on a
    /// remote pauses it, through nothing but the session; the queue's first
    /// track ending moves the session on. Falsified by `SoundCore.load`
    /// leaving the engine's track unset: no URL reached the engine.
    func testThePhonesPlayerObeysTheSession() {
        let hall = Hall()
        let phone = device(hall, "phone", canPlay: true)
        let desk = device(hall, "desktop", canPlay: false)
        phone.settle()
        desk.settle()
        XCTAssertTrue(phone.heard().joined, "the phone stands in the session")
        let why = phone.player.press(press: Press.start(queue: queue, at: 0, positionMs: 0, from: "#album/Water%20Music").json)
        XCTAssertNil(why)
        phone.settle()
        XCTAssertEqual(phone.engine.url?.absoluteString, "https://h/media/music/1.flac")
        XCTAssertTrue(phone.engine.playingNow)
        XCTAssertEqual(phone.sound.state().queue, queue.map { $0.id.hex })
        XCTAssertFalse(desk.engine.playingNow, "the desktop makes no sound")
        XCTAssertNil(desk.engine.url)

        XCTAssertNil(desk.player.press(press: Press.pause.json))
        hall.carry()
        phone.settle()
        XCTAssertFalse(phone.engine.playingNow, "a pause pressed elsewhere is a pause here")
        XCTAssertNil(desk.player.press(press: Press.play.json))
        hall.carry()
        phone.settle()
        XCTAssertTrue(phone.engine.playingNow)

        // The first track ends by itself: the next plays, and the session says so.
        phone.engine.onEnd?()
        phone.settle()
        XCTAssertEqual(phone.engine.url?.absoluteString, "https://h/media/music/2.flac")
        desk.settle()
        XCTAssertEqual(desk.heard().listening?.active?.at, 1, "the session followed the phone")
        let state = PlayerState.of(desk.heard(), server: "https://h", name: "Desk", art: { _, _ in "" }, local: nil, shuffle: false, repeatMode: .off, buffering: false, now: nowMs())
        XCTAssertEqual(state.now?.title, "Song 2")
        XCTAssertEqual(state.on?.name, "phone")
    }

    /// Unlinked, a press is refused as not part of the session, and the
    /// screens play this phone's own player directly — which the
    /// reconciler's view then shows as what plays. Falsified by `start`
    /// loading nothing.
    func testAPhoneUnlinkedIsItsOwnSession() {
        let hall = Hall()
        let phone = device(hall, "phone", canPlay: true)
        phone.settle()
        phone.peer.setOnline(online: false)
        hall.carry()
        phone.settle()
        XCTAssertFalse(phone.heard().joined)
        XCTAssertEqual(phone.player.press(press: Press.play.json), notPart)
        phone.sound.start(queue, at: 1, from: "", shuffled: false)
        phone.settle()
        XCTAssertEqual(phone.engine.url?.absoluteString, "https://h/media/music/2.flac")
        let h = phone.heard()
        XCTAssertEqual(h.active?.now?.title, "Song 2", "the view is the player's own")
        phone.sound.previous()
        phone.settle()
        XCTAssertEqual(phone.engine.url?.absoluteString, "https://h/media/music/1.flac")
        phone.sound.insert([Fx.track(3)], next: true, from: "")
        XCTAssertEqual(phone.sound.queue.tracks.map { $0.title }, ["Song 1", "Song 3", "Song 2"])
        phone.sound.move(from: 2, to: 1)
        XCTAssertEqual(phone.sound.queue.tracks.map { $0.title }, ["Song 1", "Song 2", "Song 3"])
        phone.sound.remove(2)
        XCTAssertEqual(phone.sound.queue.tracks.count, 2)
        phone.sound.cycleRepeat()
        XCTAssertEqual(phone.sound.repeatMode, .all)
    }
}
