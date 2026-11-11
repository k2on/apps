// A launch link (`money://?screen=…`, ui/docs/shots.md) read back as the
// launch arguments it says, as the screenshots' driver writes it
// (.github/screens/app-android.sh); any other link is none.
package dev.money.android

import dev.arkui.Navigator
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LinksTests {
    @Test
    fun aLaunchLinkIsItsArguments() {
        assertEquals(
            listOf("-screen", "category:vacation", "-inspector", "1"),
            Navigator.launchArgsOf("money://?screen=category%3Avacation&inspector=1", "money"),
        )
        assertEquals(listOf("-screen", "settings"), Navigator.launchArgsOf("money://?screen=settings", "money"))
    }

    @Test
    fun anyOtherLinkIsNone() {
        assertNull(Navigator.launchArgsOf("money://account/0123/checking/", "money"))
        assertNull(Navigator.launchArgsOf("money://join?server=x", "money"))
        assertNull(Navigator.launchArgsOf("harken://?screen=budget", "money"))
        assertNull(Navigator.launchArgsOf("money://?", "money"))
    }
}
