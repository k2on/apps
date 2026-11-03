// Finding a receipt in a camera frame, in plain Kotlin over its luminance —
// no OpenCV, no Android: what the scanner (Scanner.kt) runs on each frame of
// its analysis stream, and what the JVM tests hold to drawn pages.
//
// **The page** is the frame's bright paper: the frame shrunk to about 160
// wide and blurred, split at Otsu's threshold, its largest bright region
// taken, and the four corners of that region's outline found as a quad (the
// extremes of x+y and x−y, then each corner moved along the outline to where
// the quad covers most). A region too small, a quad that is not convex, too
// thin, or covers too little of its region is no page.
//
// **Steady and sharp** is when the scanner takes the picture: the quad has
// moved less than a fraction of the frame for about a second (`Steady`), and
// the frame's Laplacian varies enough that its text has edges (`sharpness`).
//
// **The page made flat** is a homography from the quad to a rectangle of the
// page's own proportions, sampled bilinearly (`Warp`).
package dev.money.android

import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** A point in a frame's pixels. */
data class Pt(val x: Float, val y: Float) {
    fun dist(o: Pt): Float = hypot(x - o.x, y - o.y)

    operator fun times(s: Float): Pt = Pt(x * s, y * s)
}

/** A page's corners, clockwise from the top left. */
data class Quad(val tl: Pt, val tr: Pt, val br: Pt, val bl: Pt) {
    val corners: List<Pt>
        get() = listOf(tl, tr, br, bl)

    /** Its area (the shoelace), positive. */
    val area: Float
        get() {
            val c = corners
            var s = 0f
            for (i in 0 until 4) {
                val a = c[i]
                val b = c[(i + 1) % 4]
                s += a.x * b.y - b.x * a.y
            }
            return abs(s) / 2
        }

    /** Every turn the same way: no corner folded in. */
    val convex: Boolean
        get() {
            val c = corners
            var sign = 0
            for (i in 0 until 4) {
                val a = c[i]
                val b = c[(i + 1) % 4]
                val d = c[(i + 2) % 4]
                val z = (b.x - a.x) * (d.y - b.y) - (b.y - a.y) * (d.x - b.x)
                val s = if (z > 0) 1 else if (z < 0) -1 else 0
                if (s == 0) return false
                if (sign == 0) sign = s else if (s != sign) return false
            }
            return true
        }

    fun scaled(s: Float): Quad = Quad(tl * s, tr * s, br * s, bl * s)

    /** Each corner `t` of the way to `o`'s. */
    fun toward(o: Quad, t: Float): Quad {
        fun l(a: Pt, b: Pt) = Pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
        return Quad(l(tl, o.tl), l(tr, o.tr), l(br, o.br), l(bl, o.bl))
    }

    /** The farthest any corner is from `o`'s. */
    fun moved(o: Quad): Float = corners.zip(o.corners).maxOf { (a, b) -> a.dist(b) }

    /** The page's size flat: its opposite sides' longer lengths. */
    fun flatSize(): Pair<Int, Int> {
        val w = max(tl.dist(tr), bl.dist(br))
        val h = max(tl.dist(bl), tr.dist(br))
        return w.roundToInt().coerceAtLeast(1) to h.roundToInt().coerceAtLeast(1)
    }

    /** Turned with its frame a quarter `turns` times clockwise, the frame `w` by `h` before the turn. */
    fun rotated(turns: Int, w: Int, h: Int): Quad {
        var q = this
        var (fw, fh) = w to h
        repeat(((turns % 4) + 4) % 4) {
            fun r(p: Pt) = Pt(fh - p.y, p.x)
            // Clockwise: the old bottom left becomes the new top left.
            q = Quad(r(q.bl), r(q.tl), r(q.tr), r(q.br))
            val t = fw
            fw = fh
            fh = t
        }
        return q
    }

    companion object {
        /** Four points as a quad, clockwise from the top left (the least x+y). */
        fun ordered(ps: List<Pt>): Quad {
            val cx = ps.sumOf { it.x.toDouble() } / ps.size
            val cy = ps.sumOf { it.y.toDouble() } / ps.size
            // Clockwise in image coordinates (y down): increasing angle.
            val round = ps.sortedBy { kotlin.math.atan2(it.y - cy, it.x - cx) }
            val start = round.indices.minBy { round[it].x + round[it].y }
            val c = List(4) { round[(start + it) % 4] }
            return Quad(c[0], c[1], c[2], c[3])
        }
    }
}

/** A frame's luminance: `w` by `h`, a byte a pixel, rows `stride` apart. */
class Luma(val w: Int, val h: Int, val px: ByteArray, val stride: Int = w) {
    operator fun get(x: Int, y: Int): Int = px[y * stride + x].toInt() and 0xff
}

object Vision {
    /** About how wide a frame is looked at for its page. */
    const val LOOK = 160

    /** The least of the frame a page takes. */
    const val LEAST = 0.08f

    /**
     * The page in `frame`, in its pixels, or none: the largest bright region's corners, when they
     * make a page.
     */
    fun page(frame: Luma): Quad? {
        val step = max(1, frame.w / LOOK)
        val w = frame.w / step
        val h = frame.h / step
        if (w < 8 || h < 8) return null
        val small = IntArray(w * h)
        for (y in 0 until h) for (x in 0 until w) small[y * w + x] = frame[x * step, y * step]
        val blurred = blur(blur(small, w, h), w, h)
        val cut = otsu(blurred)
        val region = largest(blurred, w, h, cut) ?: return null
        if (region.size < LEAST * w * h) return null
        if (contrast(blurred, region) < CONTRAST) return null
        val outline = outline(region, w)
        val q = corners(outline) ?: return null
        if (!q.convex) return null
        if (q.area < 0.85f * region.size) return null
        val (fw, fh) = q.flatSize()
        if (min(fw, fh) < 0.12f * max(fw, fh)) return null
        // The small frame's pixel centres, back in the frame's.
        val s = step.toFloat()
        val half = s / 2
        return Quad(
            Pt(q.tl.x * s + half, q.tl.y * s + half),
            Pt(q.tr.x * s + half, q.tr.y * s + half),
            Pt(q.br.x * s + half, q.br.y * s + half),
            Pt(q.bl.x * s + half, q.bl.y * s + half))
    }

    /** How much brighter paper is than what it lies on, at the least, in levels of 255. */
    const val CONTRAST = 45

    /** The region's mean brightness less the rest's. */
    fun contrast(px: IntArray, region: IntArray): Int {
        if (region.size == px.size) return 0
        val inside = BooleanArray(px.size)
        var sumIn = 0L
        for (i in region) {
            inside[i] = true
            sumIn += px[i]
        }
        var sumOut = 0L
        for (i in px.indices) if (!inside[i]) sumOut += px[i]
        return (sumIn / region.size - sumOut / (px.size - region.size)).toInt()
    }

    /** A 3×3 box blur, edges clamped. */
    fun blur(src: IntArray, w: Int, h: Int): IntArray {
        val out = IntArray(w * h)
        for (y in 0 until h) for (x in 0 until w) {
            var sum = 0
            for (dy in -1..1) for (dx in -1..1) {
                val xx = (x + dx).coerceIn(0, w - 1)
                val yy = (y + dy).coerceIn(0, h - 1)
                sum += src[yy * w + xx]
            }
            out[y * w + x] = sum / 9
        }
        return out
    }

    /** Otsu's threshold: the level that best splits the pixels in two. */
    fun otsu(px: IntArray): Int {
        val hist = IntArray(256)
        for (v in px) hist[v.coerceIn(0, 255)]++
        val total = px.size.toDouble()
        var sum = 0.0
        for (i in 0 until 256) sum += i * hist[i].toDouble()
        var sumB = 0.0
        var wB = 0.0
        var best = 0.0
        var cut = 128
        for (t in 0 until 256) {
            wB += hist[t]
            if (wB == 0.0) continue
            val wF = total - wB
            if (wF == 0.0) break
            sumB += t * hist[t].toDouble()
            val mB = sumB / wB
            val mF = (sum - sumB) / wF
            val between = wB * wF * (mB - mF) * (mB - mF)
            if (between > best) {
                best = between
                cut = t
            }
        }
        return cut
    }

    /** The largest 4-connected region brighter than `cut`, as its pixels' indexes. */
    fun largest(px: IntArray, w: Int, h: Int, cut: Int): IntArray? {
        val seen = BooleanArray(w * h)
        val queue = IntArray(w * h)
        var best: IntArray? = null
        for (start in px.indices) {
            if (seen[start] || px[start] <= cut) continue
            var head = 0
            var tail = 0
            queue[tail++] = start
            seen[start] = true
            while (head < tail) {
                val i = queue[head++]
                val x = i % w
                val y = i / w
                if (x > 0) tail = visit(i - 1, px, cut, seen, queue, tail)
                if (x < w - 1) tail = visit(i + 1, px, cut, seen, queue, tail)
                if (y > 0) tail = visit(i - w, px, cut, seen, queue, tail)
                if (y < h - 1) tail = visit(i + w, px, cut, seen, queue, tail)
            }
            if (tail > (best?.size ?: 0)) best = queue.copyOf(tail)
        }
        return best
    }

    private fun visit(i: Int, px: IntArray, cut: Int, seen: BooleanArray, queue: IntArray, tail: Int): Int {
        if (seen[i] || px[i] <= cut) return tail
        seen[i] = true
        queue[tail] = i
        return tail + 1
    }

    /** A region's convex outline: each row's ends, hulled (Andrew's monotone chain). */
    fun outline(region: IntArray, w: Int): List<Pt> {
        val ends = HashMap<Int, IntArray>()
        for (i in region) {
            val x = i % w
            val y = i / w
            val e = ends.getOrPut(y) { intArrayOf(x, x) }
            if (x < e[0]) e[0] = x
            if (x > e[1]) e[1] = x
        }
        val pts = ArrayList<Pt>()
        for ((y, e) in ends) {
            // A pixel is a square: its outer corners, so a page's sides are where its pixels end.
            pts.add(Pt(e[0].toFloat() - 0.5f, y - 0.5f))
            pts.add(Pt(e[0].toFloat() - 0.5f, y + 0.5f))
            pts.add(Pt(e[1].toFloat() + 0.5f, y - 0.5f))
            pts.add(Pt(e[1].toFloat() + 0.5f, y + 0.5f))
        }
        return hull(pts)
    }

    /** The convex hull, clockwise in image coordinates. */
    fun hull(points: List<Pt>): List<Pt> {
        val ps = points.distinct().sortedWith(compareBy({ it.x }, { it.y }))
        if (ps.size < 3) return ps
        fun cross(o: Pt, a: Pt, b: Pt) = (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
        val lower = ArrayList<Pt>()
        for (p in ps) {
            while (lower.size >= 2 && cross(lower[lower.size - 2], lower[lower.size - 1], p) <= 0) lower.removeAt(lower.size - 1)
            lower.add(p)
        }
        val upper = ArrayList<Pt>()
        for (p in ps.asReversed()) {
            while (upper.size >= 2 && cross(upper[upper.size - 2], upper[upper.size - 1], p) <= 0) upper.removeAt(upper.size - 1)
            upper.add(p)
        }
        lower.removeAt(lower.size - 1)
        upper.removeAt(upper.size - 1)
        return lower + upper
    }

    /**
     * The four outline points that make the largest quad: first the extremes of x+y and x−y, then
     * each moved to whichever outline point between its neighbours covers most, until none moves.
     */
    fun corners(outline: List<Pt>): Quad? {
        if (outline.size < 4) return null
        val n = outline.size
        val idx =
            intArrayOf(
                outline.indices.minBy { outline[it].x + outline[it].y },
                outline.indices.maxBy { outline[it].x - outline[it].y },
                outline.indices.maxBy { outline[it].x + outline[it].y },
                outline.indices.minBy { outline[it].x - outline[it].y },
            )
        if (idx.toSet().size < 4) return null
        fun area(ix: IntArray): Float = Quad(outline[ix[0]], outline[ix[1]], outline[ix[2]], outline[ix[3]]).area
        var moved = true
        var rounds = 0
        while (moved && rounds++ < 16) {
            moved = false
            for (k in 0 until 4) {
                val before = idx[(k + 3) % 4]
                val after = idx[(k + 1) % 4]
                var bestI = idx[k]
                var bestA = area(idx)
                var i = (before + 1) % n
                while (i != after) {
                    val trial = idx.copyOf()
                    trial[k] = i
                    val a = area(trial)
                    if (a > bestA + 1e-3f) {
                        bestA = a
                        bestI = i
                    }
                    i = (i + 1) % n
                }
                if (bestI != idx[k]) {
                    idx[k] = bestI
                    moved = true
                }
            }
        }
        return Quad.ordered(idx.map { outline[it] })
    }

    /**
     * How sharp a frame is: the variance of its Laplacian over its middle, every `step`th pixel. Text
     * in focus is in the hundreds; a blurred frame in the tens.
     */
    fun sharpness(frame: Luma, step: Int = 2): Double {
        var n = 0
        var sum = 0.0
        var sq = 0.0
        val x0 = frame.w / 8
        val y0 = frame.h / 8
        var y = max(1, y0)
        while (y < min(frame.h - 1, frame.h - y0)) {
            var x = max(1, x0)
            while (x < min(frame.w - 1, frame.w - x0)) {
                val l = frame[x - 1, y] + frame[x + 1, y] + frame[x, y - 1] + frame[x, y + 1] - 4 * frame[x, y]
                sum += l
                sq += l.toDouble() * l
                n++
                x += step
            }
            y += step
        }
        if (n == 0) return 0.0
        val mean = sum / n
        return sq / n - mean * mean
    }
}

/**
 * Whether a page has held still: each quad told, with the time, and steady once every quad of the
 * last `holdMs` is within `tolerance` of the frame's diagonal of the newest. A frame with no page
 * starts it again.
 */
class Steady(private val holdMs: Long = 1000, private val tolerance: Float = 0.02f) {
    private val seen = ArrayDeque<Pair<Long, Quad>>()

    /** The quad of a frame at `atMs`, `diagonal` the frame's; whether the page is steady now. */
    fun tell(q: Quad?, atMs: Long, diagonal: Float): Boolean {
        if (q == null) {
            seen.clear()
            return false
        }
        seen.addLast(atMs to q)
        while (seen.size > 1 && atMs - seen[1].first >= holdMs) seen.removeFirst()
        val limit = tolerance * diagonal
        if (seen.any { it.second.moved(q) > limit }) {
            // Moved: only what is near the newest counts from here.
            while (seen.isNotEmpty() && seen.first().second.moved(q) > limit) seen.removeFirst()
            return false
        }
        return atMs - seen.first().first >= holdMs
    }

    fun reset() = seen.clear()
}

/** A quad made a flat rectangle: the homography and the sampling. */
object Warp {
    /**
     * The homography taking the rectangle `w` by `h` (its corners clockwise from 0,0) onto `q`: the
     * eight coefficients of h11..h32 (h33 is 1), from the four corners' equations.
     */
    fun homography(q: Quad, w: Int, h: Int): DoubleArray {
        val src = listOf(Pt(0f, 0f), Pt(w.toFloat(), 0f), Pt(w.toFloat(), h.toFloat()), Pt(0f, h.toFloat()))
        val dst = q.corners
        val a = Array(8) { DoubleArray(9) }
        for (i in 0 until 4) {
            val (x, y) = src[i].x.toDouble() to src[i].y.toDouble()
            val (u, v) = dst[i].x.toDouble() to dst[i].y.toDouble()
            a[2 * i] = doubleArrayOf(x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u)
            a[2 * i + 1] = doubleArrayOf(0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v)
        }
        return solve(a)
    }

    /** Gaussian elimination with partial pivoting over an augmented 8×9. */
    private fun solve(a: Array<DoubleArray>): DoubleArray {
        val n = 8
        for (c in 0 until n) {
            val p = (c until n).maxBy { abs(a[it][c]) }
            val t = a[c]
            a[c] = a[p]
            a[p] = t
            val d = a[c][c]
            require(abs(d) > 1e-12) { "the corners are in a line" }
            for (j in c..n) a[c][j] /= d
            for (r in 0 until n) {
                if (r == c) continue
                val f = a[r][c]
                if (f == 0.0) continue
                for (j in c..n) a[r][j] -= f * a[c][j]
            }
        }
        return DoubleArray(n) { a[it][n] }
    }

    /** Where the homography `m` takes (x, y). */
    fun map(m: DoubleArray, x: Double, y: Double): Pair<Double, Double> {
        val d = m[6] * x + m[7] * y + 1
        return (m[0] * x + m[1] * y + m[2]) / d to (m[3] * x + m[4] * y + m[5]) / d
    }

    /**
     * The page in `src` (ARGB pixels, `sw` by `sh`) within `q`, flat: `w` by `h` pixels, each sampled
     * bilinearly where the homography puts it.
     */
    fun flatten(src: IntArray, sw: Int, sh: Int, q: Quad, w: Int, h: Int): IntArray {
        val m = homography(q, w, h)
        val out = IntArray(w * h)
        for (y in 0 until h) {
            for (x in 0 until w) {
                val (u, v) = map(m, x + 0.5, y + 0.5)
                out[y * w + x] = sample(src, sw, sh, u - 0.5, v - 0.5)
            }
        }
        return out
    }

    private fun sample(src: IntArray, w: Int, h: Int, u: Double, v: Double): Int {
        val x0 = kotlin.math.floor(u).toInt().coerceIn(0, w - 1)
        val y0 = kotlin.math.floor(v).toInt().coerceIn(0, h - 1)
        val x1 = (x0 + 1).coerceAtMost(w - 1)
        val y1 = (y0 + 1).coerceAtMost(h - 1)
        val fx = (u - x0).coerceIn(0.0, 1.0)
        val fy = (v - y0).coerceIn(0.0, 1.0)
        val a = src[y0 * w + x0]
        val b = src[y0 * w + x1]
        val c = src[y1 * w + x0]
        val d = src[y1 * w + x1]
        var out = 0xff shl 24
        for (shift in intArrayOf(16, 8, 0)) {
            val top = ((a shr shift) and 0xff) * (1 - fx) + ((b shr shift) and 0xff) * fx
            val bottom = ((c shr shift) and 0xff) * (1 - fx) + ((d shr shift) and 0xff) * fx
            out = out or (((top * (1 - fy) + bottom * fy).roundToInt().coerceIn(0, 255)) shl shift)
        }
        return out
    }
}
