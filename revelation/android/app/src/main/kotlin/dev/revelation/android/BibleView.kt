// The surface: the whole Bible as one column, drawn by one View on the
// hardware canvas under the camera.
//
// The view is thin on purpose. What is drawn is decided by Levels, where
// the camera may go by Camera, how it flies by FlyTo, what a touch means by
// Gestures; the view turns touches into calls on those and their answers
// into draw calls. The platform's own classes give the feel: GestureDetector
// and ScaleGestureDetector the touch slop and the velocity, OverScroller the
// fling's spline and friction (the ones RecyclerView and ScrollView use, run
// in screen pixels and converted to document units each frame at the
// current scale), EdgeEffect the stretch at either end (the glow before
// Android 12). A frame is asked for only while something moves; text held
// still costs nothing.
package dev.revelation.android

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.os.Build
import android.os.SystemClock
import android.util.AttributeSet
import android.util.TypedValue
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.ViewConfiguration
import android.widget.EdgeEffect
import android.widget.OverScroller
import java.util.concurrent.Executors
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

class BibleView @JvmOverloads constructor(context: Context, attrs: AttributeSet? = null) : View(context, attrs) {

    /** Where the camera is, in the terms the chrome shows: the header's words and the verse being read (see [readingY]). */
    data class Place(val header: String, val verse: Int)

    /** The camera kept across launches and layouts: the verse under the centre, how far past its first line, and the scale. */
    data class Anchor(val verse: Int, val offset: Double, val scale: Double)

    var onPlace: ((Place) -> Unit)? = null

    var palette: Palette = Palette.Dark
        set(value) {
            field = value
            applyPalette()
            invalidate()
        }

    /** The band at the top the chrome covers (the status bar and the header), in pixels: labels keep out of it and a fly-to lands below it. */
    var topInset: Float = 0f
        set(value) {
            field = value
            camera?.let(::fitUnderHeader)
            invalidate()
        }

    /** The band at the bottom the navigation bar covers, in pixels. */
    var bottomInset: Float = 0f
        set(value) {
            field = value
            invalidate()
        }

    /** Lay out on the calling thread, as the screenshots need; on a phone the layout runs on a thread of its own. */
    var synchronousLayout = false

    private val density = resources.displayMetrics.density.toDouble()
    private var bible: Bible? = null
    private var table: LineTable? = null
    private var camera: Camera? = null
    private var pending: Anchor? = null
    private var layingOutFor = -1f
    private var lastPlace: Place? = null

    // The paints. Glyphs are drawn under the camera's scale, so their sizes are document units; with
    // linear text, a width measured at reading size is the width drawn at any scale.
    private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG or Paint.LINEAR_TEXT_FLAG)
    private val numberPaint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG or Paint.LINEAR_TEXT_FLAG)
    private val titlePaint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG or Paint.LINEAR_TEXT_FLAG).apply { textAlign = Paint.Align.CENTER }
    private val chapterPaint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG or Paint.LINEAR_TEXT_FLAG)
    private val barPaint = Paint().apply { style = Paint.Style.STROKE; strokeCap = Paint.Cap.BUTT }
    private val fillPaint = Paint()
    private val rulePaint = Paint()
    private val highlightPaint = Paint()

    // Labels are in screen space and do not scale, like a map's.
    private val bookLabelPaint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG)
    private val chapterLabelPaint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG)
    private val labelHeight = sp(13f) * 1.3f
    private val labelWidths = HashMap<String, Double>()
    private var labels: List<Levels.Label> = emptyList()

    /**
     * A label's own opacity over time, so book and chapter names fade in when they are placed and
     * fade out when they are dropped, rather than popping. Keyed by side and target; [docY] keeps a
     * label that is fading out on the text it named while the camera moves.
     */
    private class Fade(var label: Levels.Label, var docY: Double, var opacity: Double, var shown: Boolean)
    private val fades = HashMap<String, Fade>()
    private var fadedAt = 0L

    private var bars = FloatArray(4 * 1024)
    private val verseNumbers = Array(200) { (it + 1).toString() }
    private var baseline = 0f

    // Motion.
    private val gestures = Gestures()
    private val scroller = OverScroller(context)
    private var scrollerLast = 0
    private var scrollerMin = 0
    private var scrollerMax = 0
    private var motion: Motion? = null
    private var highlight = -1
    private var highlightFrom = 0L
    private val topEdge = EdgeEffect(context)
    private val bottomEdge = EdgeEffect(context)
    private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop
    private var rawScale = 1.0
    private var pinchFocus = 0f

    // A two-finger tap: the second finger down and up again quickly, with neither moving.
    private var twoDownAt = 0L
    private var twoCandidate = false
    private val twoStart = FloatArray(4)

    init {
        typefaceFor(Typeface.SERIF)
        applyPalette()
        isClickable = true
        isFocusable = true
    }

    /** The face the text is set in: the system serif. The screenshots may set another if layoutlib lacks it. */
    fun typefaceFor(face: Typeface) {
        for (p in listOf(textPaint, numberPaint, titlePaint, chapterPaint, bookLabelPaint, chapterLabelPaint)) p.typeface = face
        textPaint.textSize = 18f
        val fm = textPaint.fontMetrics
        // The line's box is the line height; the glyphs' box (ascent to descent) is centred in it.
        baseline = (LINE_HEIGHT - (fm.descent - fm.ascent)) / 2 - fm.ascent
        labelWidths.clear()
        if (table != null) pending = anchor()
        table = null
        requestLayoutOfText()
        invalidate()
    }

    private fun applyPalette() {
        val ink = palette.ink
        textPaint.color = ink
        numberPaint.color = ink
        titlePaint.color = ink
        chapterPaint.color = ink
        barPaint.color = ink
        fillPaint.color = ink
        rulePaint.color = palette.ground
        highlightPaint.color = ink
        bookLabelPaint.color = ink
        chapterLabelPaint.color = ink
    }

    // The text and the layout ------------------------------------------------

    /** The text to show, and where to put the camera once it is laid out (Genesis 1:1 at reading size if null). */
    fun setBible(bible: Bible, anchor: Anchor? = null) {
        this.bible = bible
        this.table = null
        pending = anchor ?: pending ?: Anchor(0, 0.0, 1.0)
        requestLayoutOfText()
    }

    /** Where the camera is, to keep: null until the text is laid out. */
    fun anchor(): Anchor? {
        val t = table ?: return pending
        val c = camera ?: return pending
        val v = t.verseAt(c.y)
        return Anchor(v, c.y - t.y[t.verseFirstLine[v]], c.scale)
    }

    /** Puts the camera at [anchor] at once (no animation). */
    fun show(anchor: Anchor) {
        val t = table
        val c = camera
        if (t == null || c == null) {
            pending = anchor
            return
        }
        stopMotion()
        c.set(t.y[t.verseFirstLine[anchor.verse.coerceIn(0, t.verseFirstLine.size - 1)]] + anchor.offset, anchor.scale)
        invalidate()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        camera?.let {
            it.viewHeight = h / density
            it.set(it.y, it.scale)
        }
        if (w != oldw) {
            if (table != null) pending = anchor()
            requestLayoutOfText()
        }
    }

    private fun requestLayoutOfText() {
        val b = bible ?: return
        if (width == 0 || height == 0) return
        val params = LayoutParams.forScreen((width / density).toFloat())
        if (table?.params == params || layingOutFor == params.columnWidth) return
        val face = textPaint.typeface
        Kept.find(b, face, params)?.let {
            adopt(it)
            return
        }
        // A Paint of its own for the layout, so the thread it runs on shares nothing with drawing.
        val measure = Paint(textPaint)
        val measurer = Measurer { text, s, e -> measure.measureText(text, s, e) }
        if (synchronousLayout) {
            adopt(Kept.keep(b, face, LineTable.build(b, params, measurer)))
            return
        }
        layingOutFor = params.columnWidth
        LAYOUT.execute {
            val t = Kept.keep(b, face, LineTable.build(b, params, measurer))
            post {
                if (layingOutFor == params.columnWidth && bible === b) {
                    layingOutFor = -1f
                    adopt(t)
                }
            }
        }
    }

    private fun adopt(t: LineTable) {
        val keep = if (table != null) anchor() else pending
        table = t
        camera = Camera(height / density, t.height.toDouble()).also(::fitUnderHeader)
        pending = null
        show(keep ?: Anchor(0, 0.0, 1.0))
    }

    /** All the way out, the line starts below the header and ends as far above the bottom. */
    private fun fitUnderHeader(c: Camera) {
        // A camera at the far end stays there as the far end moves.
        val atFarEnd = c.scale <= c.scaleMin * 1.0001
        c.pad = max(32.0, topInset / density + 12)
        if (atFarEnd || c.scale < c.scaleMin) c.set(c.y, c.scaleMin)
    }

    // Drawing ----------------------------------------------------------------

    override fun onDraw(canvas: Canvas) {
        canvas.drawColor(palette.ground)
        val b = bible ?: return
        val t = table ?: return
        val c = camera ?: return
        val now = SystemClock.uptimeMillis()
        var again = step(now)

        val f = Levels.Frame(t, c, density, width.toDouble())
        val mix = f.mix()
        if (mix.glyphs > 0.002) drawGlyphs(canvas, b, f, mix.glyphs)
        if (mix.bars > 0.002) drawBars(canvas, f, mix.bars)
        if (mix.column > 0.002) drawColumn(canvas, b, f, mix.column, mix.columnInk)
        again = drawHighlight(canvas, b, f, now) || again
        again = drawLabels(canvas, b, f, mix.labels, now) || again
        again = drawEdges(canvas) || again
        if (again) postInvalidateOnAnimation()

        val place = place(b, f)
        if (place != lastPlace) {
            lastPlace = place
            onPlace?.invoke(place)
        }
    }

    /** Where the camera is now, as the header says it; null until the text is laid out. */
    fun place(): Place? {
        val b = bible ?: return null
        val t = table ?: return null
        val c = camera ?: return null
        return place(b, Levels.Frame(t, c, density, width.toDouble()))
    }

    private fun place(b: Bible, f: Levels.Frame): Place {
        val focus = readingY(f.camera)
        return Place(Levels.header(b, f, focus), f.table.verseAt(focus))
    }

    /**
     * Where the reader is reading: a quarter of the way down the screen below the header, rather than
     * its centre, because a fly-to lands its chapter just under the header, and a short chapter can
     * end before the centre.
     */
    private fun readingY(c: Camera): Double {
        val top = topInset / density
        return c.docY(top + (c.viewHeight - top) * 0.25)
    }

    private fun drawGlyphs(canvas: Canvas, b: Bible, f: Levels.Frame, alpha: Double) {
        val t = f.table
        val p = t.params
        val c = f.camera
        val a = (alpha * 255).roundToInt()
        textPaint.alpha = (a * TEXT_INK).roundToInt()
        numberPaint.alpha = (a * NUMBER_INK).roundToInt()
        titlePaint.alpha = a
        chapterPaint.alpha = (a * CHAPTER_INK).roundToInt()
        textPaint.textSize = p.fontSize
        numberPaint.textSize = p.verseNumberSize
        titlePaint.textSize = p.titleSize
        chapterPaint.textSize = p.chapterNumberSize
        val save = canvas.save()
        canvas.translate(f.columnLeft.toFloat(), (f.viewHeightPx / 2).toFloat())
        canvas.scale(f.k.toFloat(), f.k.toFloat())
        val first = t.lineAt(c.docTop - p.titleHeight)
        val last = t.lineAt(c.docBottom)
        val text = b.text
        for (i in first..last) {
            // Relative to the camera in doubles, then small floats: a y three million units down the
            // document would lose a pixel to a float's precision.
            val top = (t.y[i] - c.y).toFloat()
            when (t.kind[i]) {
                LineKind.TEXT -> {
                    val v = t.tag[i]
                    if (t.end[i] > t.start[i]) canvas.drawText(text, t.start[i], t.end[i], t.x[i], top + baseline, textPaint)
                    if (t.verseFirstLine[v] == i) {
                        val n = b.verseNumber[v]
                        // Raised a little, as a superscript is, and dim.
                        canvas.drawText(verseNumbers[(n - 1).coerceIn(0, verseNumbers.size - 1)], 0f, top + baseline - 5f, numberPaint)
                    }
                }
                LineKind.TITLE -> canvas.drawText(b.bookTitle(t.tag[i]), p.columnWidth / 2, top + p.titleHeight * 0.62f, titlePaint)
                LineKind.CHAPTER -> canvas.drawText(b.chapterNumber[t.tag[i]].toString(), 0f, top + p.chapterHeight * 0.72f, chapterPaint)
            }
        }
        canvas.restoreToCount(save)
    }

    private fun drawBars(canvas: Canvas, f: Levels.Frame, alpha: Double) {
        val (a, n) = Levels.bars(f, bars)
        bars = a
        barPaint.strokeWidth = Levels.barThickness(f).toFloat()
        barPaint.alpha = (alpha * 255 * TEXT_INK).roundToInt()
        canvas.drawLines(a, 0, n, barPaint)
    }

    private fun drawColumn(canvas: Canvas, b: Bible, f: Levels.Frame, alpha: Double, ink: Double) {
        val col = Levels.column(b, f)
        if (col.bottom <= col.top) return
        fillPaint.alpha = (alpha * ink * 255).roundToInt()
        canvas.drawRect(col.left.toFloat(), col.top.toFloat(), col.right.toFloat(), col.bottom.toFloat(), fillPaint)
        for (i in 0 until col.ruleCount step 2) {
            val y = col.rules[i]
            rulePaint.alpha = (col.rules[i + 1] * alpha * 255).roundToInt()
            canvas.drawRect(col.left.toFloat(), y, col.right.toFloat(), y + 1f, rulePaint)
        }
        // Where the Testaments meet: a short tick across the line, as a map marks a border.
        val tick = floor(Levels.testamentsMeet(b, f)).toFloat()
        if (tick > col.top && tick < col.bottom) {
            val reach = (6 * density).toFloat()
            fillPaint.alpha = (alpha * 255 * 0.7).roundToInt()
            canvas.drawRect(col.left.toFloat() - reach, tick, col.right.toFloat() + reach, tick + max(1f, density.toFloat() / 2), fillPaint)
        }
    }

    /** The verse a fly-to landed on, held for a moment under a soft band. True while it fades. */
    private fun drawHighlight(canvas: Canvas, b: Bible, f: Levels.Frame, now: Long): Boolean {
        if (highlight < 0 || motion != null) return false
        val age = now - highlightFrom
        if (age > HIGHLIGHT_MS) {
            highlight = -1
            return false
        }
        val t = f.table
        val first = t.verseFirstLine[highlight]
        var last = first
        while (last + 1 < t.size && t.kind[last + 1] == LineKind.TEXT && t.tag[last + 1] == highlight) last++
        val fade = 1 - FlyTo.ease(age.toDouble() / HIGHLIGHT_MS)
        highlightPaint.alpha = (fade * 255 * 0.09).roundToInt()
        val pad = 4 * f.k
        canvas.drawRect(
            (f.columnLeft - pad * 2).toFloat(), (f.px(t.y[first].toDouble()) - pad).toFloat(),
            (f.columnRight + pad * 2).toFloat(), (f.px(t.bottom(last).toDouble()) + pad).toFloat(), highlightPaint,
        )
        return true
    }

    /** Draws the labels, each at its faded opacity; true while any is still fading. */
    private fun drawLabels(canvas: Canvas, b: Bible, f: Levels.Frame, alpha: Double, now: Long): Boolean {
        bookLabelPaint.textSize = sp(13f)
        chapterLabelPaint.textSize = sp(11f)
        val measure: (String) -> Double = { s ->
            // Chapter labels are numbers alone; a book's name may start with one ("1 Corinthians").
            labelWidths.getOrPut(s) { (if (s.all(Char::isDigit)) chapterLabelPaint else bookLabelPaint).measureText(s).toDouble() }
        }
        labels = if (alpha > 0.002) {
            Levels.labels(
                b, f,
                top = topInset + 8 * density, bottom = height - bottomInset - 8 * density,
                labelHeight = labelHeight.toDouble(), gap = labelHeight.toDouble(), margin = 14 * density,
                measure = measure,
            )
        } else {
            emptyList()
        }

        // The first frame shows what it places at once (nothing has moved yet); after that a
        // frame moves each label's opacity toward its target by the time since the last, capped
        // so that a frame after a still spell does not jump.
        val first = fadedAt == 0L
        val step = if (first) 1.0 else (now - fadedAt).coerceIn(0L, 32L) / LABEL_FADE_MS
        fadedAt = now
        for (fade in fades.values) fade.shown = false
        for (l in labels) {
            val key = "${l.side}:${l.target.book}:${l.target.chapter}"
            val docY = f.camera.docY(l.centreY / density)
            val fade = fades.getOrPut(key) { Fade(l, docY, if (first) 1.0 else 0.0, true) }
            fade.label = l
            fade.docY = docY
            fade.shown = true
        }

        var again = false
        val gone = ArrayList<String>()
        for ((key, fade) in fades) {
            fade.opacity = if (fade.shown) min(1.0, fade.opacity + step) else max(0.0, fade.opacity - step)
            if (!fade.shown && fade.opacity <= 0.0) {
                gone += key
                continue
            }
            if (fade.opacity < 1.0) again = true
            val l = fade.label
            // A label being dropped is drawn where its text now is; a placed one where it was placed.
            val centreY = if (fade.shown) l.centreY else f.px(fade.docY)
            val paint = if (l.side == Levels.Side.LEFT) bookLabelPaint else chapterLabelPaint
            val ink = if (l.side == Levels.Side.LEFT) 0.85 else 0.6
            val eased = smooth(fade.opacity)
            paint.alpha = (alpha * l.alpha * ink * eased * 255).roundToInt()
            val fm = paint.fontMetrics
            canvas.drawText(l.text, l.left.toFloat(), (centreY - (fm.ascent + fm.descent) / 2).toFloat(), paint)
        }
        for (key in gone) fades.remove(key)
        return again
    }

    /** Ease in and out over [0, 1]. */
    private fun smooth(x: Double) = x * x * (3 - 2 * x)

    private fun drawEdges(canvas: Canvas): Boolean {
        var again = false
        if (!topEdge.isFinished) {
            val save = canvas.save()
            topEdge.setSize(width, height)
            again = topEdge.draw(canvas)
            canvas.restoreToCount(save)
        }
        if (!bottomEdge.isFinished) {
            val save = canvas.save()
            canvas.translate(-width.toFloat(), height.toFloat())
            canvas.rotate(180f, width.toFloat(), 0f)
            bottomEdge.setSize(width, height)
            again = bottomEdge.draw(canvas) || again
            canvas.restoreToCount(save)
        }
        return again
    }

    // Motion -----------------------------------------------------------------

    /** An animated move of the camera: a fly-to, or a zoom about a point (a snap, a double tap). */
    private sealed class Motion(val start: Long, val duration: Long) {
        class Fly(val path: FlyTo, start: Long, duration: Long, val verse: Int) : Motion(start, duration)
        class Zoom(val focus: Double, val anchorDoc: Double, val s0: Double, val s1: Double, start: Long, duration: Long) : Motion(start, duration)
    }

    /** Advances the fling or the animation to [now]. True if another frame is needed. */
    private fun step(now: Long): Boolean {
        val c = camera ?: return false
        if (gestures.phase == Gestures.Phase.FLINGING) {
            if (scroller.computeScrollOffset()) {
                val y = scroller.currY
                val k = c.scale * density
                c.y = c.clampY(c.y + (y - scrollerLast) / k)
                scrollerLast = y
                if ((y <= scrollerMin || y >= scrollerMax) && !scroller.isFinished) {
                    val v = scroller.currVelocity.toInt()
                    (if (y <= scrollerMin) topEdge else bottomEdge).onAbsorb(v)
                    scroller.abortAnimation()
                    gestures.settled()
                    return true
                }
                return true
            }
            gestures.settled()
            return false
        }
        val m = motion ?: return false
        val e = ((now - m.start).toDouble() / m.duration).coerceIn(0.0, 1.0)
        val eased = FlyTo.ease(e)
        when (m) {
            is Motion.Fly -> {
                val v = m.path.at(eased)
                c.set(v.y, c.viewHeight / v.w)
                if (e >= 1) {
                    highlight = m.verse
                    highlightFrom = now
                }
            }
            is Motion.Zoom -> {
                val s = exp(ln(m.s0) + (ln(m.s1) - ln(m.s0)) * eased)
                c.place(m.anchorDoc - (m.focus - c.viewHeight / 2) / s, s)
            }
        }
        if (e >= 1) {
            motion = null
            gestures.settled()
            return highlight >= 0
        }
        return true
    }

    private fun stopMotion() {
        scroller.forceFinished(true)
        motion = null
    }

    /**
     * Flies to [ref]: out, across and in, landing at reading size with the place just under the
     * header. A chapter (verse 1) lands on its number, a book's first chapter on its title; any other
     * verse lands on its first line and is held for a moment.
     */
    fun flyTo(ref: Ref) {
        val b = bible ?: return
        val t = table ?: return
        val c = camera ?: return
        val v = b.verseIndex(ref)
        val chapter = b.verseChapter[v]
        val book = b.verseBook[v]
        val target = when {
            b.verseNumber[v] > 1 -> t.y[t.verseFirstLine[v]]
            b.bookFirstChapter[book] == chapter -> t.bookTop[book]
            else -> t.chapterTop[chapter]
        }.toDouble()
        val land = topInset / density + 12
        val y1 = c.clampY(target + c.viewHeight / 2 - land, 1.0)
        if (!gestures.animate()) return
        scroller.forceFinished(true)
        val path = FlyTo(c.y, c.visibleHeight, y1, c.viewHeight)
        motion = Motion.Fly(path, SystemClock.uptimeMillis(), path.durationMs, if (b.verseNumber[v] > 1) v else -1)
        highlight = -1
        postInvalidateOnAnimation()
    }

    /** Zooms to [target] keeping the document under [focusPx] still, over [ms], unless a finger is driving. */
    private fun zoomTo(focusPx: Float, target: Double, ms: Long) {
        if (gestures.animate()) startZoom(focusPx, target, ms)
    }

    private fun startZoom(focusPx: Float, target: Double, ms: Long) {
        val c = camera ?: return
        val focus = focusPx / density
        motion = Motion.Zoom(focus, c.docY(focus), c.scale, target, SystemClock.uptimeMillis(), ms)
        postInvalidateOnAnimation()
    }

    // Touch ------------------------------------------------------------------

    private val detector = GestureDetector(
        context,
        object : GestureDetector.SimpleOnGestureListener() {
            override fun onDown(e: MotionEvent): Boolean = true

            override fun onScroll(e1: MotionEvent?, e2: MotionEvent, distanceX: Float, distanceY: Float): Boolean {
                if (e2.pointerCount > 1 || !gestures.scroll()) return false
                drag(-distanceY, e2.x)
                return true
            }

            override fun onFling(e1: MotionEvent?, e2: MotionEvent, velocityX: Float, velocityY: Float): Boolean {
                val c = camera ?: return false
                if (e2.pointerCount > 1 || !gestures.fling()) return false
                // In screen pixels, as every scrolling view's fling is; the bounds are where the
                // document's ends are at the current scale, relative to here.
                val k = c.scale * density
                scrollerLast = 0
                scrollerMin = ((c.minY() - c.y) * k).coerceIn(-1e9, 0.0).toInt()
                scrollerMax = ((c.maxY() - c.y) * k).coerceIn(0.0, 1e9).toInt()
                scroller.fling(0, 0, 0, (-velocityY).roundToInt(), 0, 0, scrollerMin, scrollerMax)
                postInvalidateOnAnimation()
                return true
            }

            override fun onDoubleTap(e: MotionEvent): Boolean {
                val c = camera ?: return false
                // A step toward reading size at the tapped place; from near it, all the way.
                if (c.scale >= c.scaleMax * SNAP) return true
                var s = min(c.scaleMax, c.scale * STEP)
                if (s > c.scaleMax * 0.6) s = c.scaleMax
                zoomTo(e.y, s, 320)
                return true
            }

            override fun onSingleTapConfirmed(e: MotionEvent): Boolean {
                val slop = 8 * density
                val hit = labels.firstOrNull { it.contains(e.x.toDouble(), e.y.toDouble(), slop) } ?: return false
                flyTo(hit.target)
                return true
            }
        },
    )

    private val scaler = ScaleGestureDetector(
        context,
        object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
            override fun onScaleBegin(d: ScaleGestureDetector): Boolean {
                val c = camera ?: return false
                stopMotion()
                gestures.scaleBegin()
                rawScale = c.scale
                pinchFocus = d.focusY
                return true
            }

            override fun onScale(d: ScaleGestureDetector): Boolean {
                val c = camera ?: return false
                rawScale *= d.scaleFactor
                // Past reading size or the far end the pinch gives a little and no more.
                c.pinch(pinchFocus / density, d.focusY / density, Camera.rubber(rawScale, c.scaleMin, c.scaleMax))
                pinchFocus = d.focusY
                invalidate()
                return true
            }

            override fun onScaleEnd(d: ScaleGestureDetector) {
                val c = camera ?: return
                // Text at rest is at reading size or plainly not: near it, snap to it, so it is crisp.
                val s = c.scale
                val target = when {
                    s >= c.scaleMax * SNAP -> c.scaleMax
                    s <= c.scaleMin * 1.15 -> c.scaleMin
                    else -> null
                }
                val snap = target != null && target != s
                gestures.scaleEnd(snap)
                if (snap) startZoom(pinchFocus, target!!, 220)
            }
        },
    ).apply { isQuickScaleEnabled = false }

    private fun drag(dyPx: Float, x: Float) {
        val c = camera ?: return
        var dy = dyPx
        // Pulling back from a stretched edge takes the stretch in first.
        if (Build.VERSION.SDK_INT >= 31) {
            if (topEdge.distance > 0 && dy < 0) dy -= topEdge.onPullDistance(dy / height, x / width) * height
            if (bottomEdge.distance > 0 && dy > 0) dy += bottomEdge.onPullDistance(-dy / height, 1 - x / width) * height
        }
        val over = c.dragBy(dy / density) * density
        if (over > 0) pull(topEdge, (over / height).toFloat(), x / width)
        if (over < 0) pull(bottomEdge, (-over / height).toFloat(), 1 - x / width)
        invalidate()
    }

    private fun pull(edge: EdgeEffect, amount: Float, at: Float) {
        if (Build.VERSION.SDK_INT >= 31) edge.onPullDistance(amount, at) else edge.onPull(amount, at)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (camera == null) return false
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            // A touch during any motion ends it and takes the camera from where it is.
            if (gestures.down()) stopMotion()
            highlight = -1
        }
        scaler.onTouchEvent(event)
        detector.onTouchEvent(event)
        // After the pinch has ended on the same event, so a two-finger tap's zoom replaces any snap.
        twoFingerTap(event)
        when (event.actionMasked) {
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                gestures.up()
                topEdge.onRelease()
                bottomEdge.onRelease()
                invalidate()
            }
        }
        return true
    }

    private fun twoFingerTap(e: MotionEvent) {
        when (e.actionMasked) {
            MotionEvent.ACTION_POINTER_DOWN -> {
                twoCandidate = e.pointerCount == 2
                twoDownAt = e.eventTime
                if (twoCandidate) for (i in 0..1) {
                    twoStart[2 * i] = e.getX(i)
                    twoStart[2 * i + 1] = e.getY(i)
                }
            }
            MotionEvent.ACTION_MOVE -> if (twoCandidate && e.pointerCount >= 2) {
                for (i in 0..1) if (hypot(e.getX(i) - twoStart[2 * i], e.getY(i) - twoStart[2 * i + 1]) > touchSlop) twoCandidate = false
            }
            MotionEvent.ACTION_POINTER_UP -> if (twoCandidate) {
                twoCandidate = false
                if (e.eventTime - twoDownAt < ViewConfiguration.getDoubleTapTimeout()) {
                    val c = camera ?: return
                    // A step out, about the point between the fingers; near the far end, all the way.
                    var s = max(c.scaleMin, c.scale / STEP)
                    if (s < c.scaleMin * 1.5) s = c.scaleMin
                    zoomTo((twoStart[1] + twoStart[3]) / 2, s, 320)
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> twoCandidate = false
        }
    }

    private fun sp(v: Float): Float = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, v, resources.displayMetrics)

    /**
     * The last layout made, for the process: a view made again for the same text, face and width (an
     * activity recreated for a theme change, the screenshots' many views) takes it rather than
     * measuring the text again.
     */
    private object Kept {
        class Entry(val bible: Bible, val face: Typeface?, val table: LineTable)

        @Volatile var entry: Entry? = null

        fun find(b: Bible, f: Typeface?, params: LayoutParams): LineTable? =
            entry?.takeIf { it.bible === b && it.face == f && it.table.params == params }?.table

        fun keep(b: Bible, f: Typeface?, t: LineTable): LineTable {
            entry = Entry(b, f, t)
            return t
        }
    }

    companion object {
        private const val LINE_HEIGHT = 27f
        private const val TEXT_INK = 0.9
        private const val NUMBER_INK = 0.45
        private const val CHAPTER_INK = 0.75
        private const val HIGHLIGHT_MS = 1600L
        /** How long a book or chapter name takes to fade in or out. */
        private const val LABEL_FADE_MS = 200.0

        /** How far a double tap or a two-finger tap zooms. */
        private const val STEP = 2.5

        /** Within this fraction of reading size a pinch lands on it. */
        private const val SNAP = 0.94

        /** One thread for layouts, so a launch or a rotation never lays out on the main thread. */
        private val LAYOUT = Executors.newSingleThreadExecutor { r -> Thread(r, "layout").apply { isDaemon = true } }
    }
}
