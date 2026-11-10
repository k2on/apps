package dev.revelation.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LayoutTests {
    private val bible = Fixtures.bible
    private val t = Fixtures.table
    private val p = Fixtures.params

    @Test
    fun theColumnIsTheScreenLessMarginsAndCapped() {
        assertEquals(352.7f, LayoutParams.forScreen(392.7f).columnWidth, 1e-3f)
        assertEquals(600f, LayoutParams.forScreen(900f).columnWidth, 1e-3f)
    }

    @Test
    fun everyCharacterIsInExactlyOneLine() {
        for (v in 0 until bible.verseCount) {
            var line = t.verseFirstLine[v]
            assertEquals(bible.verseStart[v], t.start[line])
            while (line + 1 < t.size && t.kind[line + 1] == LineKind.TEXT && t.tag[line + 1] == v) {
                assertEquals("verse $v line $line", t.end[line], t.start[line + 1])
                assertTrue(t.start[line] < t.end[line])
                line++
            }
            assertEquals(bible.verseEnd[v], t.end[line])
        }
        // And the text lines, in order, are every verse's characters once and nothing else.
        var covered = 0L
        for (i in 0 until t.size) if (t.kind[i] == LineKind.TEXT) covered += t.end[i] - t.start[i]
        var verses = 0L
        for (v in 0 until bible.verseCount) verses += bible.verseEnd[v] - bible.verseStart[v]
        assertEquals(verses, covered)
    }

    @Test
    fun noLineIsWiderThanTheColumn() {
        for (i in 0 until t.size) {
            if (t.kind[i] != LineKind.TEXT) continue
            assertTrue("line $i: ${t.x[i]} + ${t.width[i]}", t.x[i] + t.width[i] <= p.columnWidth + 1e-3f)
            // What is drawn is what was measured: the text without the space it was broken at.
            assertEquals(Fixtures.measurer.width(bible.text, t.start[i], trimEnd(t.start[i], t.end[i])), t.width[i], 0.01f)
        }
    }

    private fun trimEnd(s: Int, e: Int): Int {
        var k = e
        while (k > s && bible.text[k - 1] == ' ') k--
        return k
    }

    @Test
    fun linesBreakGreedily() {
        // A line followed by another of the same verse could not have taken the next line's first word.
        val space = Fixtures.measurer.width(" ", 0, 1)
        for (i in 0 until t.size - 1) {
            if (t.kind[i] != LineKind.TEXT || t.kind[i + 1] != LineKind.TEXT || t.tag[i] != t.tag[i + 1]) continue
            var we = t.start[i + 1]
            while (we < t.end[i + 1] && bible.text[we] != ' ') we++
            val next = Fixtures.measurer.width(bible.text, t.start[i + 1], we)
            assertTrue("line $i", t.x[i] + t.width[i] + space + next > p.columnWidth)
        }
    }

    @Test
    fun yIsMonotonicAndTheBlocksAddUp() {
        assertEquals(0, t.y[0])
        for (i in 0 until t.size - 1) assertEquals(t.y[i] + p.height(t.kind[i]), t.y[i + 1])
        assertEquals(t.bottom(t.size - 1) + p.bottomPad, t.height)
        assertEquals(bible.bookCount, (0 until t.size).count { t.kind[it] == LineKind.TITLE })
        assertEquals(bible.chapterCount, (0 until t.size).count { t.kind[it] == LineKind.CHAPTER })
        for (c in 0 until bible.chapterCount) {
            val line = t.lineAt(t.chapterTop[c].toDouble())
            assertEquals(LineKind.CHAPTER, t.kind[line])
            assertEquals(c, t.tag[line])
        }
        for (b in 0 until bible.bookCount) {
            val line = t.lineAt(t.bookTop[b].toDouble())
            assertEquals(LineKind.TITLE, t.kind[line])
            assertEquals(b, t.tag[line])
            assertEquals(t.chapterTop[bible.bookFirstChapter[b]], t.y[line + 1])
        }
    }

    @Test
    fun aVersesFirstLineIsWhereItsRefSays() {
        for (v in 0 until bible.verseCount) {
            val line = t.verseFirstLine[v]
            assertEquals(LineKind.TEXT, t.kind[line])
            assertEquals(v, t.tag[line])
            assertEquals(line, t.lineAt(t.y[line].toDouble()))
            assertEquals(line, t.lineAt(t.y[line] + 26.9))
            assertEquals(v, t.verseAt(t.y[line] + 1.0))
            // The verse number's room is kept on its first line only.
            assertTrue(t.x[line] > 0f)
            if (line + 1 < t.size && t.tag[line + 1] == v && t.kind[line + 1] == LineKind.TEXT) assertEquals(0f, t.x[line + 1])
        }
        val john = bible.bookNames.indexOf("John")
        val v = bible.verseIndex(Ref(john, 3, 16))
        assertEquals(v, t.verseAt(t.y[t.verseFirstLine[v]].toDouble()))
        // A chapter's number belongs to its first verse.
        assertEquals(bible.chapterFirstVerse[100], t.verseAt(t.chapterTop[100].toDouble()))
    }

    @Test
    fun theWholeBibleLaysOutInUnderAThirdOfASecond() {
        LineTable.build(bible, p, Fixtures.measurer)
        var best = Long.MAX_VALUE
        repeat(3) {
            val t0 = System.nanoTime()
            LineTable.build(bible, p, Fixtures.measurer)
            best = minOf(best, System.nanoTime() - t0)
        }
        val ms = best / 1e6
        println("layout: ${t.size} lines, ${t.height} dp tall, in $ms ms")
        assertTrue("layout took $ms ms", ms < 300)
    }

    @Test
    fun eachDistinctTokenIsMeasuredOnce() {
        var calls = 0
        val counting = Measurer { text, s, e -> calls++; Fixtures.measurer.width(text, s, e) }
        LineTable.build(bible, p, counting)
        val distinct = HashSet<String>()
        for (v in 0 until bible.verseCount) bible.verseText(v).split(' ').filter { it.isNotEmpty() }.forEach { distinct += it }
        println("tokens: ${distinct.size} distinct, $calls measurements")
        // The tokens, plus the space, the verse numbers, the titles and the chapter numbers.
        assertTrue(calls <= distinct.size + 1 + 176 + 66 + 1189)
    }
}
