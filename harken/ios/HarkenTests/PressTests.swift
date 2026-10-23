// What a press asks of harken's player, as it reads it: each a
// `harken_player::Press`, externally tagged; a queue edit in the queue sheet
// as the session's own transitions; the picker's rows and what each asks;
// and the throttle a dragged slider sends through.
@testable import Harken
import Foundation
import XCTest

final class PressTests: XCTestCase {
    func testEveryPressIsThePlayersJSON() {
        XCTAssertEqual(Press.play.json, "\"Play\"")
        XCTAssertEqual(Press.toggle.json, "\"Toggle\"")
        XCTAssertEqual(Press.skip(-1).json, "{\"Skip\":{\"by\":-1}}")
        XCTAssertEqual(Press.seek(1500).json, "{\"Seek\":{\"ms\":1500}}")
        XCTAssertEqual(Press.jump(3).json, "{\"Jump\":{\"index\":3}}")
        XCTAssertEqual(Press.remove(2).json, "{\"Remove\":{\"index\":2}}")
        XCTAssertEqual(Press.move(from: 4, to: 1).json, "{\"Move\":{\"from\":4,\"to\":1}}")
        XCTAssertEqual(Press.scrub(nil).json, "{\"Scrub\":{\"at_ms\":null}}")
        XCTAssertEqual(Press.transfer("tv").json, "{\"Transfer\":{\"to\":\"tv\"}}")
        XCTAssertEqual(Press.loud(.device("k"), .to(30)).json, "{\"Loud\":{\"level\":{\"Device\":{\"id\":\"k\"}},\"change\":{\"To\":{\"level\":30}}}}")
        XCTAssertEqual(Press.loud(.group, .mute(true)).json, "{\"Loud\":{\"level\":\"Group\",\"change\":{\"Mute\":{\"muted\":true}}}}")
        let t = Fx.track(1)
        let start = Press.start(queue: [t], at: 0, positionMs: 0, from: "#album/Water%20Music").json
        // The queue is the generated `Track`'s own JSON.
        XCTAssertEqual(start, "{\"Start\":{\"queue\":[\(t.json)],\"at\":0,\"position_ms\":0,\"from\":\"#album/Water%20Music\"}}")
        XCTAssertNotNil(J.parse(start) as? [String: Any])
    }

    /// Up-next rows swiped away: one `remove` each, last first, every place
    /// still the one it was. Falsified by removing first-first.
    func testRemovingUpNextRowsIsOneRemoveEachLastFirst() {
        // Playing the queue's 2nd; up next starts at the queue's 3rd.
        XCTAssertEqual(Queue.removing([0, 2], at: 1), [.remove(4), .remove(2)])
    }

    /// Rows dragged in the queue sheet are the session's `move`s, which
    /// leave the queue in the order the list shows. Falsified by `to` not
    /// counting the moved rows above it.
    func testMovingUpNextRowsIsTheSessionsMoves() {
        func apply(_ ps: [Press], to list: [Int], at: Int) -> [Int] {
            var l = list
            for p in ps {
                guard case let .move(f, t) = p else { continue }
                l.insert(l.remove(at: f), at: t)
            }
            return l
        }
        let queue = Array(0..<8)
        // Playing index 1: up next is 2…7. Row 0 (queue 2) dragged to the end.
        var ps = Queue.moving([0], to: 6, count: 6, at: 1)
        XCTAssertEqual(ps, [.move(from: 2, to: 7)])
        XCTAssertEqual(apply(ps, to: queue, at: 1), [0, 1, 3, 4, 5, 6, 7, 2])
        // The last row to the top of up next.
        ps = Queue.moving([5], to: 0, count: 6, at: 1)
        XCTAssertEqual(ps, [.move(from: 7, to: 2)])
        XCTAssertEqual(apply(ps, to: queue, at: 1), [0, 1, 7, 2, 3, 4, 5, 6])
        // Two rows apart, dragged together below a third.
        ps = Queue.moving([0, 2], to: 4, count: 6, at: 1)
        XCTAssertEqual(apply(ps, to: queue, at: 1), [0, 1, 3, 5, 2, 4, 6, 7])
        // Dropped where it was: nothing.
        XCTAssertEqual(Queue.moving([2], to: 3, count: 6, at: 1), [])
    }

    /// The lock screen's seek: a seek within the track; next and previous
    /// skips; anywhere else a jump; outside the queue, nothing.
    func testASeekIsTheSessionsWay() {
        let a = Fx.active((0..<5).map { Fx.track($0) }, at: 2, output: .device(id: "me", moving: false))
        XCTAssertEqual(Queue.seek(a, index: 2, positionMs: 9), .seek(9))
        XCTAssertEqual(Queue.seek(a, index: 3, positionMs: 0), .skip(1))
        XCTAssertEqual(Queue.seek(a, index: 1, positionMs: 0), .skip(-1))
        XCTAssertEqual(Queue.seek(a, index: 4, positionMs: 0), .jump(4))
        XCTAssertNil(Queue.seek(a, index: 9, positionMs: 0))
        XCTAssertNil(Queue.seek(nil, index: 0, positionMs: 0))
    }

    /// The picker: this phone the default, the output ticked, the group first
    /// as a whole, a follower's button its leaving. Falsified by dropping the group row.
    func testThePickerDefaultsToThisPhoneAndGroupsSpeakers() {
        let me = Fx.device("me", "Phone")
        let k = Fx.device("k", "Kitchen", kind: "speaker", volume: 40, family: "sonos")
        let l = Fx.device("l", "Lounge", kind: "speaker", volume: 20, family: "sonos", follows: "k")
        let b = Fx.device("b", "Bath", kind: "speaker", volume: 10, family: "sonos")
        let idle = Fx.session([me, k], nil)
        let rows = DevicePicker.rows(idle, me: "me")
        XCTAssertEqual(rows.map { $0.name }, ["Kitchen", "Phone"])
        XCTAssertEqual(DevicePicker.cursor(rows), 1)
        XCTAssertNil(DevicePicker.pick(rows[0], idle))
        let s = Fx.session([me, k, l, b], Fx.active([Fx.track(1)], output: .device(id: "k", moving: false)))
        let g = DevicePicker.rows(s, me: "me")
        XCTAssertEqual(g.map { $0.name }, ["Kitchen + 1", "Bath", "Kitchen", "Lounge", "Phone"])
        XCTAssertTrue(g[0].group)
        XCTAssertEqual(g[0].volume, 30)
        XCTAssertEqual(g[1].join, true)
        XCTAssertEqual(g[3].join, false)
        XCTAssertEqual(DevicePicker.cursor(g), 2)
        XCTAssertEqual(DevicePicker.pick(g[4], s), .transfer("me"))
        XCTAssertNil(DevicePicker.pick(g[2], s))
        XCTAssertEqual(DevicePicker.join(g[1]), .join("b"))
        XCTAssertEqual(DevicePicker.join(g[3]), .unjoin("l"))
        XCTAssertEqual(DevicePicker.mute(g[0]), .loud(.group, .mute(true)))
        XCTAssertEqual(s.volume("me"), Volume(of: .group, level: 30, muted: false, remote: true))
        // The output gone: drawn, ticked, not takeable.
        let away = Fx.session([me], Fx.active([Fx.track(1)], output: .away(id: "lap", name: "Laptop")))
        let a = DevicePicker.rows(away, me: "me")
        XCTAssertEqual(a.map { $0.name }, ["Laptop", "Phone"])
        XCTAssertTrue(a[0].away && a[0].ticked && !a[0].takeable)
        XCTAssertEqual(a[0].under, "not answering")
    }

    /// A dragged slider: at most once a window, the last value always.
    func testADraggedSliderSendsOnceAWindowAndTheLastAlways() {
        var now: Int64 = 1_000
        var later = [(Int64, () -> Void)]()
        var sent = [(Level, Int)]()
        let d = Drag(clock: { now }, later: { ms, f in later.append((ms, f)) }, send: { sent.append(($0, $1)) })
        d.to(.group, 10)
        d.to(.group, 20)
        d.to(.group, 30)
        XCTAssertEqual(sent.map { $0.1 }, [10])
        XCTAssertEqual(later.count, 1)
        now += 50
        later.removeFirst().1()
        XCTAssertEqual(sent.map { $0.1 }, [10, 30])
        d.to(.group, 40)
        d.done()
        XCTAssertEqual(sent.map { $0.1 }, [10, 30, 40])
        // Another slider while one is held sends the held one first.
        now += 10
        d.to(.device("k"), 5)
        d.to(.device("k"), 6)
        d.to(.group, 7)
        XCTAssertEqual(sent.map { $0.1 }, [10, 30, 40, 6])
    }

    /// The reconciler's view read: joined, the session and its active.
    func testThePlayersViewIsRead() {
        let t = Fx.track(1)
        let json = "{\"me\":\"me\",\"joined\":true,\"linked\":true,\"epoch\":3,\"listening\":{\"devices\":[\(Fx.device("me", "Phone").json)],\"scrub\":{\"k\":5},\"session\":null},"
            + "\"active\":\(Fx.active([t], output: .nowhere).json),\"stats\":{\"calls\":2,\"refused\":1,\"plays\":0,\"came_back\":0,\"last_refusal\":null},\"refused\":null}"
        let h = Heard.of(json)
        XCTAssertTrue(h.joined)
        XCTAssertEqual(h.epoch, 3)
        XCTAssertEqual(h.listening?.devices["me"]?.name, "Phone")
        XCTAssertEqual(h.scrubbing, 5)
        XCTAssertEqual(h.active?.now, t)
        XCTAssertEqual(h.stats.calls, 2)
        XCTAssertNil(h.refused)
        XCTAssertEqual(Heard.of("{}"), Heard())
    }
}
