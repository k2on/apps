// What `arkc gen swift` prints of the demo's room (`docs/plan-live.md`),
// held to the values it stands for. SwiftPM compiles a target's own files
// only, so `DemoRoom.swift` beside this is a copy of
// `spec/vectors/gen/Room.swift`, and the first claim is that it is still the
// same text. Then every state of `eval/live-room.json` — the initial value,
// and the state after each step that applies — read as a `Room` and written
// back is the same value; at least one of them is counting, so the sum's
// fields are read and not only its tag; and a value whose `t` names no
// variant is refused, naming it. Falsified by reading and writing
// `can_play` as `canPlay` in the copy: "the copy differs from
// gen/Room.swift", and "state 1: not read as a room: no field canPlay".

import ArkDB
import Foundation

enum Printed {
    static func run(_ root: String) -> [String] {
        var problems: [String] = []
        let copy = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("DemoRoom.swift").path
        do {
            if try readFile(copy) != readFile(root + "/gen/Room.swift") {
                problems.append("DemoRoom.swift differs from gen/Room.swift: copy it again")
            }
            let v = try readJson(root + "/eval/live-room.json")
            let states = [try v.field("init")] + (try v.field("steps").list()).compactMap { $0.opt("state") }
            if LiveStates.roomName != (try v.field("live").text()) {
                problems.append("the state is named \(LiveStates.roomName), the vector says \(try v.field("live").text())")
            }
            var counting = 0
            for (k, s) in states.enumerated() {
                let room: Room
                do {
                    room = try LiveStates.room(s)
                } catch {
                    problems.append("state \(k): not read as a room: \(error)")
                    continue
                }
                if case .counting = room.mode { counting += 1 }
                if room.value != s {
                    problems.append("state \(k): written back as \(room.value.show()), was \(s.show())")
                }
            }
            if counting == 0 { problems.append("no state is counting, so no variant's fields were read") }
            do {
                _ = try Mode(.record([("t", .text("paused"))]))
                problems.append("a variant the sum does not have was read")
            } catch {
                if "\(error)" != "Mode: no variant paused" { problems.append("a variant the sum does not have: \(error)") }
            }
            print("printed: \(states.count) states of the room read and written back, \(counting) counting")
        } catch {
            problems.append("\(error)")
        }
        return problems
    }
}
