// The derived square is the same square on every client: which of an app's
// gradients a name picks is FNV-1a over UTF-16 code units, pinned here with
// `rust/arkui`'s own pins (`art::hash`), so a phone and the desktop draw one
// album as one square.
package dev.arkui

import org.junit.Assert.assertEquals
import org.junit.Test

class ArtTests {
    @Test
    fun theDerivedSquareMatchesTheDesktop() {
        // `arkui::art::hash`'s own pins: the desktop and the phone draw one album as one square.
        assertEquals(0x811c9dc5.toInt(), Art.hash(""))
        assertEquals(0xe40c292c.toInt(), Art.hash("a"))
        assertEquals(0x74417273, Art.hash("Water Music"))
        assertEquals(0x939e323b.toInt(), Art.hash("Für Elise"))
        assertEquals(0x442e75ca, Art.hash("🎵"))
    }

    @Test
    fun aNamePicksOneOfTheSetByItsHash() {
        val set = listOf("a", "b", "c", "d", "e", "f")
        // The hash read as unsigned: "Water Music" is 0x74417273, 1950446195 % 6.
        assertEquals(set[(0x74417273L % 6).toInt()], Art.pick(set, "Water Music"))
        // And one whose hash has its top bit set, which a signed remainder would get wrong.
        assertEquals(set[(0xe40c292cL % 6).toInt()], Art.pick(set, "a"))
    }
}
