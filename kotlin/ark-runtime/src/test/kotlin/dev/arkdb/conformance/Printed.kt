// What `arkc gen kotlin` prints of the demo's room (`spec/vectors/gen/Room.kt`,
// `docs/plan-live.md`), compiled into this source set from where the vectors
// keep it and held to the values it stands for: every state of
// `eval/live-room.json` — the initial value, and the state after each step
// that applies — read as a `Room` and written back is the same value; at
// least one of them is counting, so the sum's fields are read and not only
// its tag; and a value whose `t` names no variant is refused, naming it.
// Falsified by printing a field under the name the type gives it rather than
// the module's (`can_play` read and written as `canPlay`): "state 1: not
// read as a room: no field "canPlay" in {"can_play":true,…}".
package dev.arkdb.conformance

import dev.arkdb.Value
import dev.arkdb.demo.LiveStates
import dev.arkdb.demo.Mode
import java.io.File
import kotlin.system.exitProcess

object Printed {
    @JvmStatic
    fun main(args: Array<String>) {
        val root = File(args.getOrElse(0) { "../spec/vectors" })
        val v = Json.parse(File(root, "eval/live-room.json").readText(Charsets.UTF_8))
        val states =
            listOf(v.field("init")) +
                v.field("steps").asList().mapNotNull { it.asStruct().fields["state"] }
        val problems = ArrayList<String>()
        if (LiveStates.ROOM != v.field("live").asText())
            problems.add(
                "the state is named ${LiveStates.ROOM}, the vector says ${v.field("live")}")
        var counting = 0
        for ((k, s) in states.withIndex()) {
            val room =
                try {
                    LiveStates.room(s)
                } catch (e: RuntimeException) {
                    problems.add("state $k: not read as a room: ${e.message}")
                    continue
                }
            if (room.mode is Mode.Counting) counting++
            val back = room.toValue()
            if (back != s) problems.add("state $k: written back as ${back.show()}, was ${s.show()}")
        }
        if (counting == 0) problems.add("no state is counting, so no variant's fields were read")
        val stray = Value.record("t" to Value.VText("paused"))
        val refused =
            try {
                Mode.of(stray)
                null
            } catch (e: IllegalArgumentException) {
                e.message
            }
        if (refused != "Mode: no variant paused")
            problems.add("a variant the sum does not have: ${refused ?: "read"}")
        println(
            "printed: ${states.size} states of the room read and written back, $counting counting")
        if (problems.isNotEmpty()) {
            for (p in problems) println("FAIL: $p")
            exitProcess(1)
        }
    }
}
