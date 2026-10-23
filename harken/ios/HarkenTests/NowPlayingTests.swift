// What plays, derived from one value — the reconciler's view — as the mini
// player, the full player, Home and the lock screen draw it: the session's
// while joined, this phone's own player's alone, never a mix.
@testable import Harken
import Foundation
import XCTest

final class NowPlayingTests: XCTestCase {
    private func art(_ album: String) -> String { album == "Water Music" ? "art/water.jpg" : "" }

    private func state(_ h: Heard, now: Int64 = 12_000) -> PlayerState {
        let local = Volume(of: .here, level: 70, muted: false, remote: false)
        return PlayerState.of(h, server: "https://h", name: "Ana's iPhone", art: art, local: local, shuffle: true, repeatMode: .all, buffering: true, now: now)
    }

    /// Joined, on another device: its track, its clock counted on, where it
    /// is, its volume — and none of what only this phone's player has.
    /// Falsified by drawing the local shuffle while joined.
    func testJoinedTheSessionIsDrawnAndWhereItPlays() {
        let q = (0..<3).map { Fx.track($0) }
        let s = Fx.session([Fx.device("me", "Phone"), Fx.device("lap", "Laptop", kind: "computer", volume: 60)],
                           Fx.active(q, at: 1, output: .device(id: "lap", moving: false)))
        let p = state(Fx.joined(s))
        XCTAssertEqual(p.now?.title, "Song 1")
        XCTAssertEqual(p.now?.art, "https://h/media/art/water.jpg")
        XCTAssertEqual(p.upNext.map { $0.title }, ["Song 2"])
        XCTAssertEqual(p.upNext.first?.index, 2)
        XCTAssertTrue(p.playing)
        XCTAssertFalse(p.shuffle)
        XCTAssertFalse(p.buffering)
        XCTAssertEqual(p.repeatMode, .off)
        XCTAssertEqual(p.on, Where(name: "Laptop", kind: "computer", here: false))
        XCTAssertEqual(p.volume, Volume(of: .device("lap"), level: 60, muted: false, remote: true))
        XCTAssertFalse(p.alone)
        // Playing since 10 s at 1 s in: 3 s in at 12 s, 4 s at 13 s.
        XCTAssertEqual(p.position(12_000), 3_000)
        XCTAssertEqual(p.position(13_000), 4_000)
        XCTAssertEqual(p.under, "On Laptop")
        XCTAssertEqual(p.lockScreenArtist, "Handel · Playing on Laptop")
    }

    /// The sound on its way: the position stands where it was handed over.
    func testAHandOffStandsStill() {
        let s = Fx.session([Fx.device("me", "Phone")], Fx.active([Fx.track(0)], output: .device(id: "tv", moving: true)))
        let p = state(Fx.joined(s))
        XCTAssertFalse(p.counting)
        XCTAssertEqual(p.position(99_000), 1_000)
        XCTAssertEqual(p.under, "Connecting to tv…")
    }

    /// Alone: this phone's own player's queue, and its shuffle, repeat and volume.
    func testAloneThePlayersOwnIsDrawn() {
        var h = Heard()
        h.me = "me"
        h.active = Fx.active([Fx.track(0, album: "")], output: .device(id: "me", moving: false), clock: .paused(atMs: 5_000))
        let p = state(h)
        XCTAssertNil(p.on)
        XCTAssertTrue(p.alone)
        XCTAssertTrue(p.shuffle)
        XCTAssertTrue(p.buffering)
        XCTAssertEqual(p.repeatMode, .all)
        XCTAssertTrue(p.hasNext)
        XCTAssertEqual(p.volume?.of, .here)
        XCTAssertEqual(p.now?.art, "")
        XCTAssertEqual(p.now?.seed, "Song 0")
        XCTAssertEqual(p.position(99_000), 5_000)
        XCTAssertEqual(p.under, "Handel")
        XCTAssertEqual(state(Heard()), PlayerState(repeatMode: .all, volume: Volume(of: .here, level: 70, muted: false, remote: false)))
    }

    /// Home's last played: the session's track while there is one, else the log's play.
    func testHomesLastPlayedIsTheSessionsElseTheLogs() {
        let s = Fx.session([Fx.device("me", "Phone")], Fx.active([Fx.track(0)], output: .device(id: "me", moving: false), from: "#album/Water%20Music"))
        let live = LastPlayed.of(Fx.joined(s), played: nil, playlists: [], art: art, server: "https://h", here: "Ana's iPhone")
        XCTAssertEqual(live?.live, true)
        XCTAssertEqual(live?.from, .album("Water Music"))
        XCTAssertEqual(live?.on, "Playing on Ana's iPhone")
        let play = Play(id: Fx.id(9), userId: "ana", track: Fx.id(4).hex, title: "Air", creator: "Bach", album: "", durationMs: 1, file: "f", playedMs: 1, from: "#library")
        let logged = LastPlayed.of(Heard(), played: play, playlists: [], art: art, server: "https://h", here: "x")
        XCTAssertEqual(logged?.live, false)
        XCTAssertEqual(logged?.track.id, Fx.id(4).hex)
        XCTAssertEqual(logged?.from, .songs)
        XCTAssertEqual(logged?.saying([]), "From your library")
        XCTAssertNil(LastPlayed.of(Heard(), played: nil, playlists: [], art: art, server: "h", here: "x"))
    }

    /// Shuffled, the track pressed is first and the rest follow it in another order.
    func testAShuffledStartPlaysThePressedTrackFirst() {
        let list = (0..<5).map { Fx.entry($0) }
        let q = Started.queue(list, at: 3, shuffled: true, album: { _ in "A" }, shuffle: { $0.reversed() })
        XCTAssertEqual(q?.at, 0)
        XCTAssertEqual(q?.queue.map { $0.title }, ["Song 3", "Song 4", "Song 2", "Song 1", "Song 0"])
        let plain = Started.queue(list, at: 9, shuffled: false, album: { _ in "A" })
        XCTAssertEqual(plain?.at, 4)
        XCTAssertEqual(plain?.queue.first?.album, "A")
        XCTAssertNil(Started.queue([], at: 0, shuffled: false, album: { _ in "" }))
    }
}
