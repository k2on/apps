// What the phone's volume is made of that is not the session: its media
// stream read as the session's 0 to 100 (`StreamLevel`), and the throttle a
// dragged slider sends its ordered calls through (`Drag`). Plain values and
// a clock a test moves.
//
// Each was falsified once by breaking what it holds; the comment beside each
// says how, and what failed.
package dev.harken.android

import org.junit.Assert.assertEquals
import org.junit.Test

class VolumeTests {
    /**
     * A stream of fifteen steps as the session's hundred: every level lands on its nearest step, and
     * every step read back and set again is the same step — so a level the phone set itself reads
     * back as a step it would set again, never as a new one.
     *
     * Falsified by a level truncated to its step rather than rounded: 30 landed on the fourth step,
     * not the fifth.
     */
    @Test
    fun aStreamsStepsAreTheSessionsHundred() {
        val stream = FakeStream(index = 0)
        val level = StreamLevel(stream)
        assertEquals(5, level.index(30))
        assertEquals(33, level.level(5))
        assertEquals("the ends are the ends", listOf(0, 15), listOf(level.index(0), level.index(100)))
        for (i in 0..15) assertEquals("step $i read and set again", i, level.index(level.level(i)))
        level.apply(30, true)
        assertEquals("set, then muted", 5, stream.index)
        assertEquals(33 to true, level.read())
        val writes = stream.writes
        level.apply(31, true)
        assertEquals("the same step and the same mute write nothing", writes, stream.writes)
    }

    /**
     * A slider dragged sends at most once a window, the last value always: the first at once, what
     * follows within the window held — each replacing the one before — and sent when the window
     * ends, or at once when it is let go; another slider dragged meanwhile sends what was held first.
     *
     * Falsified by a throttle that sent the first value it held rather than the last: 20 went at the
     * window's end where 30 had been asked for since.
     */
    @Test
    fun aDraggedSliderSendsOnceAWindowAndTheLastAlways() {
        var now = 0L
        val timers = ArrayList<Pair<Long, () -> Unit>>()
        val sent = ArrayList<Pair<String, Int>>()
        val drag = Drag<String>({ now }, { ms, f -> timers += (now + ms) to f }) { of, level -> sent += of to level }
        fun run(until: Long) {
            now = until
            val due = timers.filter { it.first <= now }
            timers.removeAll(due)
            due.forEach { it.second() }
        }
        val kitchen = "kitchen"

        drag.to(kitchen, 10)
        assertEquals("the first at once", listOf(kitchen to 10), sent)
        now = 10
        drag.to(kitchen, 20)
        now = 20
        drag.to(kitchen, 30)
        assertEquals("the rest held", 1, sent.size)
        assertEquals("and one timer for them", 1, timers.size)
        run(50)
        assertEquals("the last, at the window's end", kitchen to 30, sent.last())
        assertEquals(2, sent.size)

        now = 60
        drag.to(kitchen, 40)
        drag.done()
        assertEquals("let go: what was held goes now", kitchen to 40, sent.last())
        run(200)
        assertEquals("and not again", 3, sent.size)

        now = 300
        drag.to(kitchen, 50)
        drag.to(kitchen, 55)
        drag.to("", 70)
        assertEquals("another slider: the held one first", listOf(kitchen to 50, kitchen to 55), sent.drop(3).take(2))
        drag.done()
        assertEquals("" to 70, sent.last())
    }
}
