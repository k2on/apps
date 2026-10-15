// The few drawings this library's own pieces draw — a row's menu, the
// chevron at a row's end, a page's way back — as `rust/arkui`'s Lucide
// glyphs (ISC; the licence is `rust/arkui/src/LICENSE.lucide`): the same
// paths, stroked two units wide with butt caps and miter joins, so one
// menu button is one shape on every client. Every other glyph is the
// app's. Geometry only: each is drawn in a placeholder ink that `Icon`
// replaces with the content colour.
package dev.arkdb.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/** The drawings this library's components need, and the way an app draws its own the same. */
public object ArkGlyphs {
    /** What a glyph is drawn in before `Icon` tints it: a placeholder, never seen. */
    public val Ink: Color = Color(0xFF000000)

    /**
     * A 24-unit glyph from SVG path data, stroked two units wide with butt caps and miter joins —
     * or filled too, which reads better solid at a thumb's size.
     */
    public fun glyph(name: String, vararg paths: String, filled: Boolean = false): ImageVector {
        val b = ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f)
        for (d in paths) {
            b.addPath(
                pathData = PathParser().parsePathString(d).toNodes(),
                fill = if (filled) SolidColor(Ink) else null,
                stroke = SolidColor(Ink),
                strokeLineWidth = 2f,
                strokeLineCap = StrokeCap.Butt,
                strokeLineJoin = StrokeJoin.Miter,
            )
        }
        return b.build()
    }

    /** A circle as path data, for a glyph that is drawn of paths alone. */
    public fun circle(cx: Float, cy: Float, r: Float): String =
        "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0z"

    /** A rectangle as path data. */
    public fun rect(x: Float, y: Float, w: Float, h: Float): String = "M$x ${y}h${w}v${h}h${-w}z"

    /** A row's menu: three dots, upright. */
    public val More: ImageVector =
        glyph("more", circle(12f, 12f, 1f), circle(12f, 5f, 1f), circle(12f, 19f, 1f))

    /** Back. */
    public val Back: ImageVector = glyph("chevron-left", "m15 18-6-6 6-6")

    /** A row that opens something. */
    public val Chevron: ImageVector = glyph("chevron-right", "m9 18 6-6-6-6")
}
