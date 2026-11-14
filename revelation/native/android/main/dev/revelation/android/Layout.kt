// The reference layout: where every line of the Bible is, at reading size.
//
// The document is laid out once, at reading size, and zoom is a transform
// of it, the way a map is drawn at one scale and the camera moves over it.
// One document unit is one dp at the reading font size. StaticLayout over
// four million characters would be seconds of work; this is cheap because
// the Bible has few distinct words: every distinct token (a run of
// non-spaces, punctuation and all) is measured once, and lines are broken
// greedily by adding up widths. Each verse is its own paragraph, its number
// drawn small at the start of its first line, so every verse is
// addressable by its first line.
//
// Between the verses are two kinds of block: a book's title (tall, its name
// in display type) and a chapter's number (shorter). Every block has a
// fixed height in whole document units, so every y is an integer and exact.
package dev.revelation.android

import kotlin.math.min

/** A text width in document units at the reading font size, for `text[start, end)`. */
fun interface Measurer {
    fun width(text: CharSequence, start: Int, end: Int): Float
}

/** What a line of the table is. */
object LineKind {
    const val TEXT: Byte = 0
    const val TITLE: Byte = 1
    const val CHAPTER: Byte = 2
}

/**
 * What the layout is a function of besides the text. Sizes in document units (dp at reading size);
 * heights whole, so that every line's y is an exact integer however far down the Bible it is.
 */
data class LayoutParams(
    val columnWidth: Float,
    val fontSize: Float = 18f,
    val lineHeight: Int = 27,
    val verseNumberSize: Float = 10f,
    val verseNumberGap: Float = 5f,
    val titleSize: Float = 30f,
    val titleHeight: Int = 162,
    val chapterNumberSize: Float = 28f,
    val chapterHeight: Int = 81,
    val bottomPad: Int = 81,
) {
    companion object {
        const val MARGIN = 20f
        const val MAX_COLUMN = 600f

        /** The column for a screen [screenWidthDp] wide: the width less two margins, at most 600 dp. */
        fun forScreen(screenWidthDp: Float) = LayoutParams(columnWidth = min(screenWidthDp - 2 * MARGIN, MAX_COLUMN).coerceAtLeast(120f))
    }

    fun height(kind: Byte): Int = when (kind) {
        LineKind.TITLE -> titleHeight
        LineKind.CHAPTER -> chapterHeight
        else -> lineHeight
    }
}

/**
 * The layout, as parallel arrays with one entry per line. A text line is `text[start, end)` drawn from
 * `x` (past the verse number on a verse's first line); its range runs to the next line's start, so the
 * space a line was broken at belongs to the line before it, and every character of a verse is in
 * exactly one line. A title line's `tag` is its book and a chapter line's is its chapter; a text
 * line's is its verse. `width` is the measured width of what is drawn: the text without trailing
 * spaces, the title, the chapter number.
 */
class LineTable(
    val params: LayoutParams,
    val start: IntArray,
    val end: IntArray,
    val x: FloatArray,
    val width: FloatArray,
    val y: IntArray,
    val kind: ByteArray,
    val tag: IntArray,
    /** Per verse: its first line. */
    val verseFirstLine: IntArray,
    /** Per chapter, with one more entry: the top of its number's block; the last is the end of the text. */
    val chapterTop: IntArray,
    /** Per book, with one more entry: the top of its title's block; the last is the end of the text. */
    val bookTop: IntArray,
    /** The whole document, the bottom's air included. */
    val height: Int,
) {
    val size: Int get() = y.size

    /** Where a line ends: the next one's top, or for the last, the end of the text. */
    fun bottom(line: Int): Int = y[line] + params.height(kind[line])

    /** The line at document [docY]: the last whose top is at or above it (the first, above the top). */
    fun lineAt(docY: Double): Int {
        var lo = 0
        var hi = size - 1
        while (lo < hi) {
            val mid = (lo + hi + 1) ushr 1
            if (y[mid] <= docY) lo = mid else hi = mid - 1
        }
        return lo
    }

    /** The bottom of [chapter]'s span: its next chapter's top, or its book's end if it is the last. */
    fun chapterBottom(bible: Bible, chapter: Int): Int {
        val b = bible.chapterBook[chapter]
        return if (chapter + 1 < bible.chapterCount && bible.chapterBook[chapter + 1] == b) chapterTop[chapter + 1] else bookTop[b + 1]
    }

    /** The verse at document [docY]: a title or a chapter number belongs to the verse after it. */
    fun verseAt(docY: Double): Int {
        var line = lineAt(docY)
        while (line < size - 1 && kind[line] != LineKind.TEXT) line++
        return tag[line]
    }

    /** The chapter at document [docY]: the last whose top is at or above it. */
    fun chapterAt(docY: Double): Int = lastAtOrAbove(chapterTop, chapterTop.size - 1, docY)

    /** The book at document [docY]. */
    fun bookAt(docY: Double): Int = lastAtOrAbove(bookTop, bookTop.size - 1, docY)

    private fun lastAtOrAbove(a: IntArray, n: Int, v: Double): Int {
        var lo = 0
        var hi = n - 1
        while (lo < hi) {
            val mid = (lo + hi + 1) ushr 1
            if (a[mid] <= v) lo = mid else hi = mid - 1
        }
        return lo
    }

    companion object {
        /** The whole Bible at [params], each distinct token measured once by [measurer]. */
        fun build(bible: Bible, params: LayoutParams, measurer: Measurer): LineTable {
            val text = bible.text
            val tokens = TokenWidths(text, measurer)
            val space = measurer.width(" ", 0, 1)
            val column = params.columnWidth
            val estimate = bible.verseCount * 4 + bible.chapterCount + bible.bookCount
            val start = Ints(estimate)
            val end = Ints(estimate)
            val x = Floats(estimate)
            val width = Floats(estimate)
            val ys = Ints(estimate)
            val kind = Bytes(estimate)
            val tag = Ints(estimate)
            val verseFirstLine = IntArray(bible.verseCount)
            val chapterTop = IntArray(bible.chapterCount + 1)
            val bookTop = IntArray(bible.bookCount + 1)
            var y = 0

            fun line(s: Int, e: Int, lx: Float, w: Float, k: Byte, t: Int) {
                start.add(s)
                end.add(e)
                x.add(lx)
                width.add(w)
                ys.add(y)
                kind.add(k)
                tag.add(t)
                y += params.height(k)
            }

            // The verse numbers' widths, small, with the gap after them: there are at most 176 of them.
            val numberScale = params.verseNumberSize / params.fontSize
            val numberWidth = HashMap<Int, Float>()
            fun numberWidth(n: Int): Float = numberWidth.getOrPut(n) {
                val s = n.toString()
                measurer.width(s, 0, s.length) * numberScale + params.verseNumberGap
            }

            for (c in 0 until bible.chapterCount) {
                val b = bible.chapterBook[c]
                if (c == bible.bookFirstChapter[b]) {
                    bookTop[b] = y
                    val title = bible.bookTitle(b)
                    line(0, 0, 0f, measurer.width(title, 0, title.length) * params.titleSize / params.fontSize, LineKind.TITLE, b)
                }
                chapterTop[c] = y
                val number = bible.chapterNumber[c].toString()
                line(0, 0, 0f, measurer.width(number, 0, number.length) * params.chapterNumberSize / params.fontSize, LineKind.CHAPTER, c)

                for (v in bible.chapterFirstVerse[c] until bible.chapterFirstVerse[c + 1]) {
                    verseFirstLine[v] = ys.size
                    val vs = bible.verseStart[v]
                    val ve = bible.verseEnd[v]
                    var lineX = numberWidth(bible.verseNumber[v])
                    var avail = column - lineX
                    var lineStart = vs
                    var lineWidth = 0f
                    var hasToken = false
                    var lastEnd = vs
                    var i = vs
                    while (i < ve) {
                        // The next token: [ts, te), after the spaces from lastEnd.
                        var ts = i
                        while (ts < ve && text[ts] == ' ') ts++
                        if (ts >= ve) break
                        var te = ts
                        while (te < ve && text[te] != ' ') te++
                        val w = tokens.width(ts, te)
                        val gap = if (hasToken) (ts - lastEnd) * space else 0f
                        if (hasToken && lineWidth + gap + w > avail) {
                            line(lineStart, ts, lineX, lineWidth, LineKind.TEXT, v)
                            lineStart = ts
                            lineX = 0f
                            avail = column
                            lineWidth = 0f
                            hasToken = false
                        }
                        if (!hasToken && w > avail) {
                            // A token wider than the column (none is, in this text at these sizes): broken
                            // between characters, at the widest prefix that fits.
                            var s = ts
                            while (s < te) {
                                var k = s + 1
                                while (k < te && measurer.width(text, s, k + 1) <= avail) k++
                                val pw = measurer.width(text, s, k)
                                if (k < te) {
                                    line(lineStart, k, lineX, pw, LineKind.TEXT, v)
                                    lineStart = k
                                    lineX = 0f
                                    avail = column
                                    s = k
                                } else {
                                    lineWidth = pw
                                    s = te
                                }
                            }
                        } else {
                            lineWidth += (if (hasToken) gap else 0f) + w
                        }
                        hasToken = true
                        lastEnd = te
                        i = te
                    }
                    // The verse's last line, or for a verse with no text (the source keeps the numbers
                    // of verses it omits), one empty line for its number alone.
                    line(lineStart, ve, lineX, lineWidth, LineKind.TEXT, v)
                }
            }
            chapterTop[bible.chapterCount] = y
            bookTop[bible.bookCount] = y
            val height = y + params.bottomPad
            return LineTable(
                params, start.toArray(), end.toArray(), x.toArray(), width.toArray(), ys.toArray(), kind.toArray(), tag.toArray(),
                verseFirstLine, chapterTop, bookTop, height,
            )
        }
    }
}

/**
 * Token widths by character range, measured once each: an open-addressed table keyed by the range's
 * characters, so the 780,000 tokens of the text cost a hash and a compare each, and no allocation.
 */
internal class TokenWidths(private val text: CharSequence, private val measurer: Measurer) {
    private var cap = 1 shl 16
    private var starts = IntArray(cap)
    private var lens = IntArray(cap)
    private var widths = FloatArray(cap)
    private var count = 0

    val distinct: Int get() = count

    fun width(s: Int, e: Int): Float {
        val len = e - s
        var h = len
        for (i in s until e) h = 31 * h + text[i].code
        h = h xor (h ushr 16)
        var idx = h and (cap - 1)
        while (true) {
            val l = lens[idx]
            if (l == 0) break
            if (l == len && same(starts[idx], s, len)) return widths[idx]
            idx = (idx + 1) and (cap - 1)
        }
        val w = measurer.width(text, s, e)
        starts[idx] = s
        lens[idx] = len
        widths[idx] = w
        if (++count * 2 > cap) grow()
        return w
    }

    private fun same(a: Int, b: Int, len: Int): Boolean {
        for (k in 0 until len) if (text[a + k] != text[b + k]) return false
        return true
    }

    private fun grow() {
        val os = starts
        val ol = lens
        val ow = widths
        cap *= 2
        starts = IntArray(cap)
        lens = IntArray(cap)
        widths = FloatArray(cap)
        for (j in os.indices) {
            val len = ol[j]
            if (len == 0) continue
            val s = os[j]
            var h = len
            for (i in s until s + len) h = 31 * h + text[i].code
            h = h xor (h ushr 16)
            var idx = h and (cap - 1)
            while (lens[idx] != 0) idx = (idx + 1) and (cap - 1)
            starts[idx] = s
            lens[idx] = len
            widths[idx] = ow[j]
        }
    }
}

internal class Floats(capacity: Int = 1024) {
    private var a = FloatArray(capacity)
    var size = 0
        private set

    fun add(v: Float) {
        if (size == a.size) a = a.copyOf(a.size * 2)
        a[size++] = v
    }

    fun toArray(): FloatArray = a.copyOf(size)
}

internal class Bytes(capacity: Int = 1024) {
    private var a = ByteArray(capacity)
    var size = 0
        private set

    fun add(v: Byte) {
        if (size == a.size) a = a.copyOf(a.size * 2)
        a[size++] = v
    }

    fun toArray(): ByteArray = a.copyOf(size)
}
