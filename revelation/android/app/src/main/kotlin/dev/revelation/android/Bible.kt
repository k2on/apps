// The text: the Berean Standard Bible as one string and an index over it.
//
// The asset is the file the translation is published as (bsb.txt): UTF-8
// with a byte order mark, CRLF line endings, two lines of attribution, a
// column header, then one verse per line as `Genesis 1:1<TAB>text`, in
// canonical order. It is parsed here at launch rather than by a build tool,
// so the app is Kotlin and nothing else; the parse is one pass over four
// megabytes.
//
// Loaded, the text is one String holding every verse's text, each followed
// by a newline so that no search can match across two verses, and the
// index is parallel IntArrays: a verse is an index (0 until 31,102), and a
// verse index is a character range of the string. Chapters (1,189) and
// books (66) are indices too, each knowing the first verse it holds.
package dev.revelation.android

import java.io.InputStream

/** A place in the text by its reader's name: [book] from 0, [chapter] and [verse] from 1. */
data class Ref(val book: Int, val chapter: Int, val verse: Int = 1)

class Bible private constructor(
    /** Every verse's text, each followed by '\n'. */
    val text: String,
    /** Per verse: where its text starts and ends in [text] (end exclusive, before the newline). */
    val verseStart: IntArray,
    val verseEnd: IntArray,
    /** Per verse: its book (from 0), its chapter within the book and its number (both from 1). */
    val verseBook: IntArray,
    val verseChapterNumber: IntArray,
    val verseNumber: IntArray,
    /** Per verse: its chapter as an index over the whole Bible (0 until 1,189). */
    val verseChapter: IntArray,
    /** Per chapter, with one more entry at the end: the first verse it holds. */
    val chapterFirstVerse: IntArray,
    /** Per chapter: its book and its number within the book. */
    val chapterBook: IntArray,
    val chapterNumber: IntArray,
    /** Per book, with one more entry at the end: its first verse and its first chapter. */
    val bookFirstVerse: IntArray,
    val bookFirstChapter: IntArray,
    /** Per book: its name as the source writes it in a reference ("Psalm"). */
    val bookNames: List<String>,
    /** The source's own attribution, which the translation asks to be shown. */
    val attribution: List<String>,
) {
    val verseCount: Int get() = verseStart.size
    val chapterCount: Int get() = chapterBook.size
    val bookCount: Int get() = bookNames.size

    /** The number of chapters [book] has. */
    fun chapters(book: Int): Int = bookFirstChapter[book + 1] - bookFirstChapter[book]

    /** A book's name as a title: the source says "Psalm 23" in a reference, and the book is Psalms. */
    fun bookTitle(book: Int): String = bookNames[book].let { if (it == "Psalm") "Psalms" else it }

    /** The verse a reference names, or the nearest one in its chapter if the verse number is past the end. */
    fun verseIndex(ref: Ref): Int {
        val book = ref.book.coerceIn(0, bookCount - 1)
        val chapter = bookFirstChapter[book] + (ref.chapter - 1).coerceIn(0, chapters(book) - 1)
        val first = chapterFirstVerse[chapter]
        val last = chapterFirstVerse[chapter + 1] - 1
        // Verse numbers run 1, 2, 3 … in every chapter of this text, so this is
        // first + verse - 1; the scan keeps it right if a source ever skips one.
        for (v in first..last) if (verseNumber[v] >= ref.verse) return v
        return last
    }

    fun ref(verse: Int): Ref = Ref(verseBook[verse], verseChapterNumber[verse], verseNumber[verse])

    /** "Genesis 1:1". */
    fun label(verse: Int): String = "${bookNames[verseBook[verse]]} ${verseChapterNumber[verse]}:${verseNumber[verse]}"

    /** "Psalm 23". */
    fun chapterLabel(chapter: Int): String = "${bookNames[chapterBook[chapter]]} ${chapterNumber[chapter]}"

    fun verseText(verse: Int): String = text.substring(verseStart[verse], verseEnd[verse])

    /** The verse whose text holds character [offset] of [text]. */
    fun verseAt(offset: Int): Int {
        var lo = 0
        var hi = verseCount - 1
        while (lo < hi) {
            val mid = (lo + hi + 1) ushr 1
            if (verseStart[mid] <= offset) lo = mid else hi = mid - 1
        }
        return lo
    }

    /**
     * Every verse whose text holds [query], ignoring case, in canonical order, at most [limit] of them.
     * A scan of the string, with no index: one pass over four million characters.
     */
    fun search(query: String, limit: Int = 500): IntArray {
        val q = query.trim()
        if (q.isEmpty()) return IntArray(0)
        val lower = CharArray(q.length) { Character.toLowerCase(q[it]) }
        val upper = CharArray(q.length) { Character.toUpperCase(q[it]) }
        val out = IntArray(limit)
        var n = 0
        val t = text
        val last = t.length - q.length
        var i = 0
        while (i <= last && n < limit) {
            val c = t[i]
            if (c == lower[0] || c == upper[0]) {
                var k = 1
                while (k < q.length) {
                    val d = t[i + k]
                    if (d != lower[k] && d != upper[k] && Character.toLowerCase(d) != lower[k]) break
                    k++
                }
                if (k == q.length) {
                    val v = verseAt(i)
                    out[n++] = v
                    // One hit per verse: go on from the next verse's start.
                    i = if (v + 1 < verseCount) verseStart[v + 1] else t.length
                    continue
                }
            }
            i++
        }
        return out.copyOf(n)
    }

    /**
     * A typed reference — "john 3:16", "Ps 23", "1 cor 13" — as a [Ref], or null if [query] is not
     * one. The book is the first whose name starts with what was typed, ignoring case and spaces.
     */
    fun parseRef(query: String): Ref? {
        val m = REF.matchEntire(query.trim()) ?: return null
        val name = m.groupValues[1].replace(" ", "").lowercase()
        if (name.isEmpty()) return null
        val book = bookNames.indexOfFirst { it.replace(" ", "").lowercase().startsWith(name) }
            .takeIf { it >= 0 }
            ?: bookNames.indices.firstOrNull { bookTitle(it).replace(" ", "").lowercase().startsWith(name) }
            ?: return null
        val chapter = m.groupValues[2].toInt()
        if (chapter < 1 || chapter > chapters(book)) return null
        val verse = m.groupValues[3].ifEmpty { "1" }.toInt().coerceAtLeast(1)
        return Ref(book, chapter, verse)
    }

    companion object {
        const val ASSET = "bsb.txt"

        private val REF = Regex("""^([1-3]?\s*[A-Za-z][A-Za-z ]*?)\s*(\d+)(?:\s*[:.]\s*(\d+))?$""")

        fun load(input: InputStream): Bible = parse(input.readBytes().toString(Charsets.UTF_8))

        /** The source file's text, as [ASSET] holds it. */
        fun parse(source: String): Bible {
            val s = if (source.startsWith('﻿')) source.substring(1) else source
            val attribution = ArrayList<String>(2)
            val text = StringBuilder(s.length)
            val vs = Ints()
            val ve = Ints()
            val vb = Ints()
            val vc = Ints()
            val vn = Ints()
            val vci = Ints()
            val cf = Ints()
            val cb = Ints()
            val cn = Ints()
            val bf = Ints()
            val bc = Ints()
            val names = ArrayList<String>(66)
            var headerDone = false
            var pos = 0
            while (pos < s.length) {
                var end = s.indexOf('\n', pos)
                if (end < 0) end = s.length
                var lineEnd = end
                if (lineEnd > pos && s[lineEnd - 1] == '\r') lineEnd--
                val start = pos
                pos = end + 1
                if (lineEnd == start) continue
                val tab = s.indexOf('\t', start).let { if (it < 0 || it > lineEnd) -1 else it }
                if (!headerDone) {
                    // The attribution lines, then the column header ("Verse<TAB>Berean Standard Bible").
                    val first = s.substring(start, if (tab < 0) lineEnd else tab).trim()
                    if (first == "Verse") headerDone = true
                    else attribution += s.substring(start, lineEnd).trim()
                    continue
                }
                require(tab > 0) { "a verse line without a tab: ${s.substring(start, lineEnd)}" }
                // "1 Samuel 3:10": the book is everything before the last space.
                val space = s.lastIndexOf(' ', tab)
                val colon = s.indexOf(':', space)
                require(space > start && colon in (space + 1) until tab) { "a reference that is not Book C:V: ${s.substring(start, tab)}" }
                val book = s.substring(start, space)
                val chapter = parseInt(s, space + 1, colon)
                val verse = parseInt(s, colon + 1, tab)
                if (names.isEmpty() || names.last() != book) {
                    names += book
                    bf.add(vs.size)
                    bc.add(cb.size)
                }
                val b = names.size - 1
                if (cb.size == 0 || cb.last() != b || cn.last() != chapter) {
                    cf.add(vs.size)
                    cb.add(b)
                    cn.add(chapter)
                }
                var textStart = tab + 1
                var textEnd = lineEnd
                while (textStart < textEnd && s[textStart] == ' ') textStart++
                while (textEnd > textStart && s[textEnd - 1] == ' ') textEnd--
                vs.add(text.length)
                text.append(s, textStart, textEnd)
                ve.add(text.length)
                text.append('\n')
                vb.add(b)
                vc.add(chapter)
                vn.add(verse)
                vci.add(cb.size - 1)
            }
            cf.add(vs.size)
            bf.add(vs.size)
            bc.add(cb.size)
            return Bible(
                text.toString(),
                vs.toArray(), ve.toArray(), vb.toArray(), vc.toArray(), vn.toArray(), vci.toArray(),
                cf.toArray(), cb.toArray(), cn.toArray(), bf.toArray(), bc.toArray(),
                names, attribution,
            )
        }

        private fun parseInt(s: String, from: Int, to: Int): Int {
            var n = 0
            for (i in from until to) {
                val d = s[i] - '0'
                require(d in 0..9) { "not a number: ${s.substring(from, to)}" }
                n = n * 10 + d
            }
            return n
        }
    }
}

/** A growable IntArray, so a pass over the text allocates no boxes. */
internal class Ints(capacity: Int = 1024) {
    private var a = IntArray(capacity)
    var size = 0
        private set

    fun add(v: Int) {
        if (size == a.size) a = a.copyOf(a.size * 2)
        a[size++] = v
    }

    fun last(): Int = a[size - 1]

    operator fun get(i: Int): Int = a[i]

    operator fun set(i: Int, v: Int) {
        a[i] = v
    }

    fun toArray(): IntArray = a.copyOf(size)
}
