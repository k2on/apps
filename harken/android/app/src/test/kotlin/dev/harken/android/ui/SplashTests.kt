// The splash stays while nothing is ready and the ceiling has not passed,
// and goes on whichever comes first.
package dev.arkui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SplashTests {
    @Test
    fun theSplashGoesWhenReadyOrAtTheCeiling() {
        assertTrue(keepsSplash(ready = false, elapsedMs = 0, ceilingMs = 1500))
        assertTrue(keepsSplash(ready = false, elapsedMs = 1499, ceilingMs = 1500))
        assertFalse(keepsSplash(ready = false, elapsedMs = 1500, ceilingMs = 1500))
        assertFalse(keepsSplash(ready = true, elapsedMs = 10, ceilingMs = 1500))
    }
}
