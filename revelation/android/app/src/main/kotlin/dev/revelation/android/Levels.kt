// Levels of detail: what is drawn at a given zoom, decided from the
// rendered height of a text line in pixels, h = lineHeight · scale · density,
// and from the rendered heights of chapters and books.
//
// - Glyphs (h ≥ 5): the text itself.
// - Greeked (1.2 ≤ h < 5): each line a bar of its measured width, half the
//   line height thick: text as seen from across a room, with the short last
//   lines of verses and the gaps of the chapter blocks still showing.
// - Column (h < 1.2): the lines have merged into a column drawn at the
//   coverage the bars had, about half, so brightness is continuous; a one
//   pixel separator marks each chapter while chapters are at least 6 px
//   tall and each book while books are at least 4 px; the column's width
//   never drops under one pixel, which is the single line at the far end.
//   As it narrows to that line its ink rises to full, so the line is white.
// - Labels, in screen-space type beside the column like a map's: chapter
//   numbers on the right while a chapter is at least 24 px tall, book names
//   on the left, as many as fit without colliding, longest books first.
//
// Every boundary is a crossfade over a band of h, so nothing pops. All of
// it is pure functions of the layout and the camera, returning what to
// draw, so tests can check it without a canvas.
package dev.revelation.android

import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

object Levels {
    const val GLYPHS = 5.0
    const val BARS = 1.2
    const val CHAPTER_RULE = 6.0
    const val BOOK_RULE = 4.0
    const val CHAPTER_LABEL = 24.0
    const val BOOK_LABEL = 20.0

    /** How much of a line's box the bars cover, and so the merged column's ink. */
    const val COVERAGE = 0.5

    /** Where a bar sits in its line, as a fraction of the line height from its top: on the x-height. */
    const val BAR_CENTRE = 0.56

    fun smoothstep(e0: Double, e1: Double, x: Double): Double {
        val t = ((x - e0) / (e1 - e0)).coerceIn(0.0, 1.0)
        return t * t * (3 - 2 * t)
    }

    /** How much of each level is drawn: each in [0, 1], and adjacent levels sum to one through their band. */
    data class Mix(
        val glyphs: Double,
        val bars: Double,
        val column: Double,
        /** The merged column's ink: the bars' coverage, rising to full as the column narrows to the line. */
        val columnInk: Double,
        val labels: Double,
    )

    fun mix(h: Double, columnPx: Double): Mix {
        val g = smoothstep(GLYPHS * 0.8, GLYPHS * 1.2, h)
        val c = 1 - smoothstep(BARS * 0.8, BARS * 1.25, h)
        val ink = COVERAGE + (1 - COVERAGE) * (1 - smoothstep(1.5, 6.0, columnPx))
        val labels = 1 - smoothstep(8.0, 12.0, h)
        return Mix(g, (1 - g) * (1 - c), c, ink, labels)
    }

    /** The frame's geometry in screen pixels. */
    class Frame(val table: LineTable, val camera: Camera, val density: Double, val viewWidthPx: Double) {
        /** Pixels per document unit. */
        val k: Double = camera.scale * density
        val h: Double = table.params.lineHeight * k
        val columnPx: Double = table.params.columnWidth * k
        val viewHeightPx: Double = camera.viewHeight * density

        /** The column's edges: centred, at least one pixel wide, on whole pixels when it is narrow so the line is crisp. */
        val columnLeft: Double
        val columnRight: Double

        init {
            val centre = viewWidthPx / 2
            if (columnPx < 8) {
                val w = max(1, columnPx.roundToInt())
                columnLeft = floor(centre - w / 2.0 + 0.5)
                columnRight = columnLeft + w
            } else {
                columnLeft = centre - columnPx / 2
                columnRight = centre + columnPx / 2
            }
        }

        fun px(docY: Double): Double = camera.screenY(docY) * density

        fun mix(): Mix = mix(h, columnPx)
    }

    /**
     * The greeked bars of the visible lines as `drawLines` wants them, (x0, y0, x1, y1) in screen
     * pixels, into [out] (grown if needed); returns the array and the count of floats in it. Titles
     * and chapter numbers are bars too, centred and to the left as they are drawn.
     */
    fun bars(f: Frame, out: FloatArray): Pair<FloatArray, Int> {
        val t = f.table
        val p = t.params
        val first = t.lineAt(f.camera.docTop - p.titleHeight)
        val last = t.lineAt(f.camera.docBottom)
        var a = out
        var n = 0
        for (i in first..last) {
            val w = t.width[i]
            if (w <= 0f) continue
            val kind = t.kind[i]
            val x0: Double
            val centre: Double
            when (kind) {
                LineKind.TITLE -> {
                    x0 = (p.columnWidth - w) / 2.0
                    centre = t.y[i] + p.titleHeight * 0.6
                }
                LineKind.CHAPTER -> {
                    x0 = 0.0
                    centre = t.y[i] + p.chapterHeight * 0.62
                }
                else -> {
                    x0 = t.x[i].toDouble()
                    centre = t.y[i] + p.lineHeight * BAR_CENTRE
                }
            }
            if (n + 4 > a.size) a = a.copyOf(a.size * 2 + 64)
            val y = f.px(centre).toFloat()
            a[n++] = (f.columnLeft + x0 * f.k).toFloat()
            a[n++] = y
            a[n++] = (f.columnLeft + (x0 + w) * f.k).toFloat()
            a[n++] = y
        }
        return Pair(a, n)
    }

    /** The bars' stroke: half a line's height, in pixels. */
    fun barThickness(f: Frame): Double = f.h / 2

    /**
     * The merged column: one span from the text's top to its end (clipped to the screen), and the
     * separators over it, each a whole-pixel row with its alpha: a chapter's while both chapters
     * beside it are at least [CHAPTER_RULE] px tall, a book's while both books are at least
     * [BOOK_RULE] px, faded in over half as much again.
     */
    class Column(val top: Double, val bottom: Double, val left: Double, val right: Double, val rules: FloatArray, val ruleCount: Int)

    fun column(bible: Bible, f: Frame): Column {
        val t = f.table
        val end = t.bookTop[bible.bookCount].toDouble()
        val top = max(0.0, f.px(0.0))
        val bottom = min(f.viewHeightPx, f.px(end))
        val rules = FloatArray(2 * 512)
        var n = 0
        val firstChapter = t.chapterAt(max(0.0, f.camera.docTop))
        val lastChapter = t.chapterAt(min(end, f.camera.docBottom))
        var prevHeight = if (firstChapter > 0) (t.chapterBottom(bible, firstChapter - 1) - t.chapterTop[firstChapter - 1]) * f.k else 0.0
        for (c in firstChapter..lastChapter) {
            val height = (t.chapterBottom(bible, c) - t.chapterTop[c]) * f.k
            val b = bible.chapterBook[c]
            val bookStart = bible.bookFirstChapter[b] == c
            if (c > 0) {
                var alpha = smoothstep(CHAPTER_RULE, CHAPTER_RULE * 1.5, min(prevHeight, height))
                val boundary: Double
                if (bookStart) {
                    val above = (t.bookTop[b] - t.bookTop[b - 1]) * f.k
                    val below = (t.bookTop[b + 1] - t.bookTop[b]) * f.k
                    alpha = max(alpha, smoothstep(BOOK_RULE, BOOK_RULE * 1.5, min(above, below)))
                    boundary = t.bookTop[b].toDouble()
                } else {
                    boundary = t.chapterTop[c].toDouble()
                }
                val y = floor(f.px(boundary))
                if (alpha > 0.01 && y >= top && y < bottom && n + 2 <= rules.size) {
                    rules[n++] = y.toFloat()
                    rules[n++] = alpha.toFloat()
                }
            }
            prevHeight = height
        }
        return Column(top, bottom, f.columnLeft, f.columnRight, rules, n)
    }

    /** Where the Old Testament ends and the New begins, as a screen y in pixels: Matthew's top. */
    fun testamentsMeet(bible: Bible, f: Frame): Double {
        val matthew = bible.bookNames.indexOf("Matthew").takeIf { it >= 0 } ?: 39
        return f.px(f.table.bookTop[matthew].toDouble())
    }

    enum class Side { LEFT, RIGHT }

    /** A label beside the column: its text, its box in screen pixels, its alpha, and where a tap on it goes. */
    data class Label(
        val text: String,
        val side: Side,
        val left: Double,
        val centreY: Double,
        val width: Double,
        val height: Double,
        val alpha: Double,
        val target: Ref,
    ) {
        val top: Double get() = centreY - height / 2
        val bottom: Double get() = centreY + height / 2
        fun contains(x: Double, y: Double, slop: Double = 0.0) =
            x >= left - slop && x <= left + width + slop && y >= top - slop && y <= bottom + slop
    }

    /**
     * The labels for frame [f]: chapter numbers right of the column, book names left of it. Each is
     * placed within [top, bottom] of the screen (pixels; the header's band is kept clear), centred on
     * the visible part of its book, or at the top of the visible part of its chapter (so a chapter's
     * number stays in view while the chapter does). Candidates are placed longest first and dropped
     * if they would overlap one already placed, [gap] pixels apart.
     */
    fun labels(
        bible: Bible,
        f: Frame,
        top: Double,
        bottom: Double,
        labelHeight: Double,
        gap: Double,
        margin: Double,
        measure: (String) -> Double,
    ): List<Label> {
        val t = f.table
        val out = ArrayList<Label>()
        val end = t.bookTop[bible.bookCount].toDouble()
        val docTop = max(0.0, f.camera.docY(top / f.density))
        val docBottom = min(end, f.camera.docY(bottom / f.density))
        if (docBottom <= docTop) return out

        // Books, left of the column.
        val placedLeft = ArrayList<Label>()
        val books = ArrayList<IntArray>()
        for (b in t.bookAt(docTop)..t.bookAt(docBottom)) books += intArrayOf(b, t.bookTop[b + 1] - t.bookTop[b])
        books.sortByDescending { it[1] }
        for ((b, _) in books) {
            val segTop = f.px(t.bookTop[b].toDouble())
            val segBottom = f.px(t.bookTop[b + 1].toDouble())
            val visTop = max(segTop, top)
            val visBottom = min(segBottom, bottom)
            if (visBottom <= visTop) continue
            val centre = ((visTop + visBottom) / 2).coerceIn(top + labelHeight / 2, bottom - labelHeight / 2)
            val text = bible.bookTitle(b)
            val w = measure(text)
            val label = Label(text, Side.LEFT, f.columnLeft - margin - w, centre, w, labelHeight, 1.0, Ref(b, 1, 1))
            if (label.left < 0) continue
            if (placedLeft.none { overlaps(it, label, gap) }) placedLeft += label
        }
        out += placedLeft

        // Chapters, right of the column.
        val placedRight = ArrayList<Label>()
        val c0 = t.chapterAt(docTop)
        val c1 = t.chapterAt(docBottom)
        if (c1 - c0 <= 400) {
            val chapters = (c0..c1).filter { (t.chapterBottom(bible, it) - t.chapterTop[it]) * f.k >= CHAPTER_LABEL }
                .sortedByDescending { t.chapterBottom(bible, it) - t.chapterTop[it] }
            for (c in chapters) {
                val segTop = f.px(t.chapterTop[c].toDouble())
                val segBottom = f.px(t.chapterBottom(bible, c).toDouble())
                val height = segBottom - segTop
                val visTop = max(segTop, top)
                val visBottom = min(segBottom, bottom)
                if (visBottom - visTop < labelHeight) continue
                val centre = visTop + labelHeight / 2
                val text = bible.chapterNumber[c].toString()
                val w = measure(text)
                val alpha = smoothstep(CHAPTER_LABEL, CHAPTER_LABEL * 1.4, height)
                val label = Label(text, Side.RIGHT, f.columnRight + margin, centre, w, labelHeight, alpha, Ref(bible.chapterBook[c], bible.chapterNumber[c], 1))
                if (placedRight.none { overlaps(it, label, gap) }) placedRight += label
            }
        }
        out += placedRight
        return out
    }

    private fun overlaps(a: Label, b: Label, gap: Double) = a.top < b.bottom + gap && b.top < a.bottom + gap

    /**
     * What the header says for the camera: the chapter at [focus] (a document y; the screen's centre
     * unless the view reads from higher up) while chapters are
     * tall enough to be labelled ("Psalm 23"); the book under it while at most two books share the
     * screen ("Psalms"); and once books have merged into a stretch of the column, the first and last
     * book on screen ("Psalms – Isaiah", and all the way out "Genesis – Revelation").
     */
    fun header(bible: Bible, f: Frame, focus: Double = f.camera.y): String {
        val t = f.table
        val end = t.bookTop[bible.bookCount].toDouble()
        val centre = focus.coerceIn(0.0, end - 1)
        val chapter = t.chapterAt(centre)
        val chapterPx = (t.chapterBottom(bible, chapter) - t.chapterTop[chapter]) * f.k
        if (chapterPx >= CHAPTER_LABEL) return bible.chapterLabel(chapter)
        val first = t.bookAt(max(0.0, f.camera.docTop))
        val last = t.bookAt(min(end - 1, f.camera.docBottom))
        if (last - first <= 1) return bible.bookTitle(t.bookAt(centre))
        return "${bible.bookTitle(first)} \u2013 ${bible.bookTitle(last)}"
    }
}
