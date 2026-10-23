package dev.revelation.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BibleTests {
    private val bible = Fixtures.bible

    @Test
    fun counts() {
        assertEquals(66, bible.bookCount)
        assertEquals(1189, bible.chapterCount)
        assertEquals(31102, bible.verseCount)
        assertEquals(39, bible.bookNames.indexOf("Matthew"))
        assertEquals(150, bible.chapters(bible.bookNames.indexOf("Psalm")))
    }

    @Test
    fun theFirstAndLastVerses() {
        assertTrue(bible.verseText(0).startsWith("In the beginning"))
        assertEquals("Genesis 1:1", bible.label(0))
        assertEquals("Revelation 22:21", bible.label(bible.verseCount - 1))
        assertEquals("The grace of the Lord Jesus be with all the saints. Amen.", bible.verseText(bible.verseCount - 1))
    }

    @Test
    fun knownVerses() {
        val john = bible.bookNames.indexOf("John")
        assertTrue(bible.verseText(bible.verseIndex(Ref(john, 3, 16))).startsWith("For God so loved the world"))
        val psalm = bible.bookNames.indexOf("Psalm")
        assertTrue(bible.verseText(bible.verseIndex(Ref(psalm, 23, 1))).contains("The LORD is my shepherd"))
        assertEquals("Psalm 119:176", bible.label(bible.chapterFirstVerse[bible.bookFirstChapter[psalm] + 119] - 1))
        assertEquals("Psalms", bible.bookTitle(psalm))
        // Unicode punctuation survives the parse.
        assertTrue(bible.verseText(bible.verseCount - 2).contains('“'))
    }

    @Test
    fun theAttributionIsKept() {
        assertEquals(2, bible.attribution.size)
        assertTrue(bible.attribution[0].startsWith("The Holy Bible, Berean Standard Bible"))
        assertTrue(bible.attribution[1].contains("public domain"))
    }

    @Test
    fun refsRoundTrip() {
        for (v in 0 until bible.verseCount) assertEquals(v, bible.verseIndex(bible.ref(v)))
        for (c in 0 until bible.chapterCount) {
            val first = bible.chapterFirstVerse[c]
            assertEquals(c, bible.verseChapter[first])
            assertEquals(1, bible.verseNumber[first])
        }
    }

    @Test
    fun versesAreRangesOfTheText() {
        for (v in 0 until bible.verseCount) {
            assertTrue(bible.verseStart[v] <= bible.verseEnd[v])
            assertEquals('\n', bible.text[bible.verseEnd[v]])
            assertEquals(v, bible.verseAt(bible.verseStart[v]))
        }
        // The verses the source omits keep their numbers and have no text.
        val matthew = bible.bookNames.indexOf("Matthew")
        assertEquals("", bible.verseText(bible.verseIndex(Ref(matthew, 17, 21))))
    }

    @Test
    fun searchIgnoresCaseAndFindsEachVerseOnce() {
        val hits = bible.search("IN THE BEGINNING")
        assertTrue(0 in hits)
        val john = bible.bookNames.indexOf("John")
        assertTrue(bible.verseIndex(Ref(john, 1, 1)) in hits)
        assertEquals(hits.toList(), hits.distinct().sorted())
        assertEquals(0, bible.search("zzzzqqq").size)
        assertEquals(0, bible.search("  ").size)
        // No match spans two verses: Genesis 1:1 ends "the earth." and 1:2 begins "Now the earth".
        assertEquals(0, bible.search("earth. Now").size)
    }

    @Test
    fun searchIsAScanInMilliseconds() {
        bible.search("shepherd")
        val t0 = System.nanoTime()
        val hits = bible.search("shepherd")
        val ms = (System.nanoTime() - t0) / 1e6
        assertTrue(hits.isNotEmpty())
        assertTrue("search took $ms ms", ms < 200)
    }

    @Test
    fun typedReferences() {
        val john = bible.bookNames.indexOf("John")
        assertEquals(Ref(john, 3, 16), bible.parseRef("john 3:16"))
        assertEquals(Ref(bible.bookNames.indexOf("Psalm"), 23, 1), bible.parseRef("Ps 23"))
        assertEquals(Ref(bible.bookNames.indexOf("Psalm"), 23, 1), bible.parseRef("psalms 23"))
        assertEquals(Ref(bible.bookNames.indexOf("1 Corinthians"), 13, 4), bible.parseRef("1 cor 13:4"))
        assertEquals(Ref(bible.bookNames.indexOf("Genesis"), 1, 1), bible.parseRef("gen 1"))
        assertNull(bible.parseRef("love"))
        assertNull(bible.parseRef("jude 2"))
    }
}
