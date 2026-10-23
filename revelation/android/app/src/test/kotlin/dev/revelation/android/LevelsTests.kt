package dev.revelation.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LevelsTests {
    private val bible = Fixtures.bible
    private val t = Fixtures.table
    private val d = Fixtures.DENSITY

    /** The scale at which a text line is [h] pixels tall. */
    private fun scaleFor(h: Double) = h / (t.params.lineHeight * d)

    private fun frame(y: Double, scale: Double): Levels.Frame {
        val c = Fixtures.camera()
        c.set(y, scale)
        return Fixtures.frame(c)
    }

    private val measure: (String) -> Double = { it.length * 20.0 }

    private fun labels(f: Levels.Frame) = Levels.labels(bible, f, 240.0, f.viewHeightPx - 40, 36.0, 8.0, 24.0, measure)

    @Test
    fun theThresholds() {
        Levels.mix(5.0 * 1.2, 300.0).let { assertEquals(1.0, it.glyphs, 0.0); assertEquals(0.0, it.bars, 0.0) }
        Levels.mix(4.0, 300.0).let { assertEquals(0.0, it.glyphs, 0.0); assertEquals(1.0, it.bars, 0.0) }
        Levels.mix(1.5, 20.0).let { assertEquals(1.0, it.bars, 0.0); assertEquals(0.0, it.column, 0.0) }
        Levels.mix(0.9, 10.0).let { assertEquals(0.0, it.bars, 0.0); assertEquals(1.0, it.column, 0.0) }
        // At the thresholds themselves, half and half.
        assertEquals(0.5, Levels.mix(5.0, 300.0).glyphs, 1e-9)
        assertEquals(0.5, Levels.mix(5.0, 300.0).bars, 1e-9)
        // Labels belong to the far levels, not to reading.
        assertEquals(0.0, Levels.mix(27.0 * d, 1000.0).labels, 0.0)
        assertEquals(1.0, Levels.mix(3.0, 30.0).labels, 0.0)
    }

    @Test
    fun nothingPops() {
        // Over the whole range of zoom, the levels sum to one and move by small steps.
        var last = Levels.mix(100.0, 1000.0)
        var h = 100.0
        while (h > 0.01) {
            h *= 0.99
            val m = Levels.mix(h, h * 13)
            assertEquals(1.0, m.glyphs + m.bars + m.column, 1e-9)
            assertTrue(Math.abs(m.glyphs - last.glyphs) < 0.06)
            assertTrue(Math.abs(m.bars - last.bars) < 0.1)
            assertTrue(Math.abs(m.column - last.column) < 0.1)
            assertTrue(Math.abs(m.columnInk - last.columnInk) < 0.06)
            last = m
        }
    }

    @Test
    fun theColumnsInkIsTheBarsCoverageThenWhite() {
        assertEquals(Levels.COVERAGE, Levels.mix(1.0, 16.0).columnInk, 0.0)
        assertEquals(1.0, Levels.mix(0.01, 1.0).columnInk, 0.0)
    }

    @Test
    fun theFarEndIsOneCentredPixel() {
        val c = Fixtures.camera()
        c.set(0.0, 0.0)
        val f = Fixtures.frame(c)
        assertTrue(f.columnPx < 1)
        assertEquals(1.0, f.columnRight - f.columnLeft, 0.0)
        assertEquals(Math.floor(f.columnLeft), f.columnLeft, 0.0)
        assertTrue(Math.abs((f.columnLeft + 0.5) - f.viewWidthPx / 2) <= 0.5)
        val col = Levels.column(bible, f)
        assertEquals(32.0 * d, col.top, 1e-6)
        assertTrue(col.bottom > f.viewHeightPx - 32.0 * d - 2 * t.params.bottomPad * f.k)
        val mix = f.mix()
        assertEquals(1.0, mix.column, 0.0)
        assertEquals(1.0, mix.columnInk, 0.0)
    }

    /** Where chapter [c]'s span starts: its book's title for a book's first chapter. */
    private fun boundary(c: Int): Double {
        val b = bible.chapterBook[c]
        return (if (bible.bookFirstChapter[b] == c) t.bookTop[b] else t.chapterTop[c]).toDouble()
    }

    @Test
    fun separatorsFollowTheirThresholds() {
        // Chapters a few px tall: no chapter rules, but rules between long books.
        // Chapters a few px tall: a rule only where both chapters beside it are at least 6 px tall, or
        // at a book's top where both books are at least 4 px tall, and some of the latter.
        val f = frame(t.height / 2.0, 0.0006)
        val col = Levels.column(bible, f)
        var bookRules = 0
        for (i in 0 until col.ruleCount step 2) {
            val y = col.rules[i].toDouble()
            val c = (1 until bible.chapterCount).first { Math.floor(f.px(boundary(it))) == y }
            fun px(a: Int, b: Int) = (b - a) * f.k
            val chapters = minOf(px(t.chapterTop[c - 1], t.chapterBottom(bible, c - 1)), px(t.chapterTop[c], t.chapterBottom(bible, c)))
            val b = bible.chapterBook[c]
            val atBook = bible.bookFirstChapter[b] == c
            val books = if (atBook) minOf(px(t.bookTop[b - 1], t.bookTop[b]), px(t.bookTop[b], t.bookTop[b + 1])) else 0.0
            if (atBook && books >= Levels.BOOK_RULE) bookRules++
            assertTrue("a rule at $y", chapters >= Levels.CHAPTER_RULE || books >= Levels.BOOK_RULE)
        }
        assertTrue(bookRules > 3)
        // Chapters tens of px tall (Ezekiel's): a rule at every chapter on screen.
        val g = frame(t.chapterTop[bible.bookFirstChapter[bible.bookNames.indexOf("Ezekiel")] + 20].toDouble(), scaleFor(0.4))
        val chapters = (0 until bible.chapterCount).count { val y = g.px(t.chapterTop[it].toDouble()); y > 0 && y < g.viewHeightPx }
        assertTrue(Levels.column(bible, g).ruleCount / 2 >= chapters - 1)
    }

    @Test
    fun labelsNeverOverlapAndTheLongestBooksWin() {
        for (scale in listOf(0.0, 0.0003, 0.0006, scaleFor(0.4), scaleFor(2.0))) {
            val f = frame(t.height / 2.0, scale)
            val ls = labels(f)
            for (side in Levels.Side.entries) {
                val s = ls.filter { it.side == side }.sortedBy { it.top }
                for (i in 1 until s.size) assertTrue("$scale: ${s[i - 1]} / ${s[i]}", s[i].top >= s[i - 1].bottom + 8.0 - 1e-9)
            }
            for (l in ls) {
                assertTrue(l.top >= 240.0 - 1e-9 && l.bottom <= f.viewHeightPx - 40 + 1e-9)
                if (l.side == Levels.Side.LEFT) assertTrue(l.left + l.width <= f.columnLeft) else assertTrue(l.left >= f.columnRight)
            }
        }
        // All the way out, Psalms (the longest book) is labelled, a handful of others are, and no chapter is.
        val far = labels(frame(0.0, 0.0))
        val books = far.filter { it.side == Levels.Side.LEFT }.map { it.text }
        assertTrue(books.toString(), "Psalms" in books)
        assertTrue(books.toString(), books.size in 4..40)
        assertTrue(far.none { it.side == Levels.Side.RIGHT })
        assertFalse("Obadiah" in books)
    }

    @Test
    fun chapterNumbersOnlyWhenChaptersAreTallEnough() {
        val f = frame(t.chapterTop[600].toDouble(), scaleFor(2.0))
        val chapters = labels(f).filter { it.side == Levels.Side.RIGHT }
        assertTrue(chapters.isNotEmpty())
        for (l in chapters) {
            val c = bible.bookFirstChapter[l.target.book] + l.target.chapter - 1
            assertTrue((t.chapterBottom(bible, c) - t.chapterTop[c]) * f.k >= Levels.CHAPTER_LABEL)
        }
    }

    @Test
    fun theHeaderSaysWhatIsUnderTheCentre() {
        val psalm = bible.bookNames.indexOf("Psalm")
        val p23 = bible.bookFirstChapter[psalm] + 22
        val y = t.chapterTop[p23] + 200.0
        assertEquals("Psalm 23", Levels.header(bible, frame(y, 1.0)))
        assertEquals("Psalm 23", Levels.header(bible, frame(y, scaleFor(2.0))))
        // Read from higher than the centre: the camera's centre in Psalm 24, the reading point in 23.
        val p24 = t.chapterTop[p23 + 1] + 300.0
        assertEquals("Psalm 24", Levels.header(bible, frame(p24, 1.0)))
        assertEquals("Psalm 23", Levels.header(bible, frame(p24, 1.0), focus = t.chapterTop[p23 + 1] - 50.0))
        assertEquals("Psalms", Levels.header(bible, frame(y, scaleFor(1.0))))
        // Zoomed out over a stretch of books: the first and last on screen, Psalms between them.
        val merged = Levels.header(bible, frame(y, scaleFor(0.06))).split(" \u2013 ")
        assertEquals(2, merged.size)
        val titles = (0 until bible.bookCount).map(bible::bookTitle)
        assertTrue(titles.indexOf(merged[0]) < psalm && titles.indexOf(merged[1]) > psalm)
        val far = Levels.header(bible, frame(y, 0.0))
        assertEquals("Genesis – Revelation", far)
    }

    @Test
    fun barsAreTheVisibleLines() {
        val f = frame(t.chapterTop[30].toDouble(), scaleFor(3.0))
        val (a, n) = Levels.bars(f, FloatArray(16))
        assertTrue(n > 4 * 100)
        for (i in 0 until n step 4) {
            assertTrue(a[i] >= f.columnLeft - 1e-3 && a[i + 2] <= f.columnRight + 1e-3)
            assertEquals(a[i + 1], a[i + 3], 0f)
        }
        assertEquals(1.5, Levels.barThickness(f), 1e-9)
    }
}
