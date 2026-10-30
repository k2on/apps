// The drawings: the desktop's Lucide glyphs (`rust/arkui/src/glyphs.rs`,
// ISC; the licence is `rust/arkui/src/LICENSE.lucide`), as the same paths
// with the same butt caps and miter joins, so a play button is one shape
// on every client. A circle or a rect there is a path here, written out.
// `repeat`, `repeat-1`, `bell`, and `captions`, `locate-fixed` and
// `external-link` for a transcript and a person's page, are Lucide's too,
// drawn the same way; the desktop has no use for them. The devices of the listening
// session's picker — `smartphone`, `laptop`, `speaker`, and `cast` for the
// button that opens it — are the desktop's own; `globe`, for a browser, is
// Lucide's, drawn the same way.
//
// Geometry only: every glyph is drawn in a placeholder ink that `Icon`
// replaces with the content colour, so its colour is always the theme's.
// The three `ark-ui` draws itself — the menu, back, the chevron — are its
// (`ArkGlyphs`), the same paths, named here so a screen asks one table.
package dev.harken.android

import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp
import dev.arkui.ArkGlyphs

object Glyphs {
    // A 24-unit glyph from SVG path data, stroked two units wide — or filled,
    // for the transport, which reads better solid at a thumb's size.
    private fun glyph(name: String, vararg paths: String, filled: Boolean = false): ImageVector {
        val b = ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f)
        for (d in paths) {
            b.addPath(
                pathData = PathParser().parsePathString(d).toNodes(),
                fill = if (filled) SolidColor(Palette.Ink) else null,
                stroke = SolidColor(Palette.Ink),
                strokeLineWidth = 2f,
                strokeLineCap = StrokeCap.Butt,
                strokeLineJoin = StrokeJoin.Miter,
            )
        }
        return b.build()
    }

    private fun circle(cx: Float, cy: Float, r: Float): String =
        "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0z"

    private fun rect(x: Float, y: Float, w: Float, h: Float): String = "M$x ${y}h${w}v${h}h${-w}z"

    val Home =
        glyph(
            "home",
            "M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8",
            "M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
        )
    val Search = glyph("search", "m21 21-4.34-4.34", circle(11f, 11f, 8f))
    val Library =
        glyph(
            "library",
            rect(3f, 3f, 8f, 18f),
            "M7 3v18",
            "M20.4 18.9c.2.5-.1 1.1-.6 1.3l-1.9.7c-.5.2-1.1-.1-1.3-.6L11.1 5.1c-.2-.5.1-1.1.6-1.3l1.9-.7c.5-.2 1.1.1 1.3.6Z",
        )
    val Play =
        glyph("play", "M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z", filled = true)
    val Pause = glyph("pause", rect(14f, 3f, 5f, 18f), rect(5f, 3f, 5f, 18f), filled = true)
    val Next =
        glyph(
            "next",
            "M21 4v16",
            "M6.029 4.285A2 2 0 0 0 3 6v12a2 2 0 0 0 3.029 1.715l9.997-5.998a2 2 0 0 0 .003-3.432z",
            filled = true,
        )
    val Previous =
        glyph(
            "previous",
            "M17.971 4.285A2 2 0 0 1 21 6v12a2 2 0 0 1-3.029 1.715l-9.997-5.998a2 2 0 0 1-.003-3.432z",
            "M3 20V4",
            filled = true,
        )
    val Shuffle =
        glyph(
            "shuffle",
            "m18 14 4 4-4 4",
            "m18 2 4 4-4 4",
            "M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-8.6a4 4 0 0 1 3.3-1.7H22",
            "M2 6h1.972a4 4 0 0 1 3.6 2.2",
            "M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45",
        )
    val Repeat =
        glyph("repeat", "m17 2 4 4-4 4", "M3 11v-1a4 4 0 0 1 4-4h14", "m7 22-4-4 4-4", "M21 13v1a4 4 0 0 1-4 4H3")
    val RepeatOne =
        glyph(
            "repeat-1",
            "m17 2 4 4-4 4",
            "M3 11v-1a4 4 0 0 1 4-4h14",
            "m7 22-4-4 4-4",
            "M21 13v1a4 4 0 0 1-4 4H3",
            "M11 10h1v4",
        )
    val Queue = glyph("list-music", "M16 5H3", "M11 12H3", "M11 19H3", "M21 16V5", circle(18f, 16f, 3f))
    val Playlist = Queue
    val AddTo = glyph("list-plus", "M16 5H3", "M11 12H3", "M16 19H3", "M18 9v6", "M21 12h-6")
    val Album = glyph("disc-3", circle(12f, 12f, 10f), "M6 12c0-1.7.7-3.2 1.8-4.2", circle(12f, 12f, 2f), "M18 12c0 1.7-.7 3.2-1.8 4.2")
    val Artist = glyph("user", "M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2", circle(12f, 7f, 4f))
    /** Back a few seconds, and on: Lucide's `rotate-ccw` and `rotate-cw`, an episode's skip. */
    val SkipBack = glyph("rotate-ccw", "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8", "M3 3v5h5")
    val SkipForward = glyph("rotate-cw", "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8", "M21 3v5h-5")

    /** A podcast, a sermon, a show of either: Lucide's `mic`, its capsule's ends kept round. */
    val Show = glyph("mic", "M12 19v3", "M19 10v2a7 7 0 0 1-14 0v-2", "M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0z")
    /** Sermons, and a series of them: a pulpit, its reading desk tilted on a stand. Not Lucide's, which has none: drawn in its grid. */
    val Pulpit = glyph("pulpit", "M3 8l18-4", "M5 7.6V10h14V5.6", "M8 10l1 9h6l1-9", "M6 21h12")
    /** What an episode says, a line at a time: Lucide's `captions`. */
    val Transcript = glyph("captions", "M5 5h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z", "M7 15h4M15 15h2M7 11h2M13 11h4")
    /** Back to the line being said: Lucide's `locate-fixed`. */
    val Follow = glyph("locate-fixed", "M2 12h3", "M19 12h3", "M12 2v3", "M12 19v3", circle(12f, 12f, 7f), circle(12f, 12f, 3f))
    /** Somebody's own page on the web: Lucide's `external-link`. */
    val Link = glyph("external-link", "M15 3h6v6", "M10 14 21 3", "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6")
    val Note = glyph("music", "M9 18V5l12-2v13", circle(6f, 18f, 3f), circle(18f, 16f, 3f))
    val More = ArkGlyphs.More
    val Back = ArkGlyphs.Back
    val Chevron = ArkGlyphs.Chevron
    val Down = glyph("chevron-down", "m6 9 6 6 6-6")
    val Close = glyph("x", "M18 6 6 18", "m6 6 12 12")
    val Plus = glyph("plus", "M5 12h14", "M12 5v14")
    val Minus = glyph("minus", "M5 12h14")
    val Tick = glyph("check", "M20 6 9 17l-5-5")
    val Ticked = glyph("circle-check", circle(12f, 12f, 10f), "m16 9-5.5 5.5L8 12")
    val Untick = glyph("circle", circle(12f, 12f, 10f))
    val SignOut = glyph("log-out", "m16 17 5-5-5-5", "M21 12H9", "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4")
    val Offline =
        glyph(
            "wifi-off",
            "M12 20h.01",
            "M8.5 16.429a5 5 0 0 1 7 0",
            "M5 12.859a10 10 0 0 1 5.17-2.69",
            "M19 12.859a10 10 0 0 0-2.007-1.523",
            "M2 8.82a15 15 0 0 1 4.177-2.643",
            "M22 8.82a15 15 0 0 0-11.288-3.764",
            "m2 2 20 20",
        )
    val Playing = glyph("audio-lines", "M2 10v3", "M6 6v11", "M10 3v18", "M14 8v7", "M18 5v13", "M22 10v3")
    val Remove = glyph("list-minus", "M16 5H3", "M11 12H3", "M16 19H3", "M21 12h-6")
    val PlayNext = glyph("list-start", "M16 12H3", "M16 18H3", "M10 6H3", "M21 18V8a2 2 0 0 0-2-2h-5", "m16 8-2-2 2-2")
    val Phone = glyph("smartphone", rect(5f, 2f, 14f, 20f), "M12 18h.01")
    val Computer =
        glyph(
            "laptop",
            "M18 5a2 2 0 0 1 2 2v8.526a2 2 0 0 0 .212.897l1.068 2.127a1 1 0 0 1-.9 1.45H3.62a1 1 0 0 1-.9-1.45l1.068-2.127A2 2 0 0 0 4 15.526V7a2 2 0 0 1 2-2z",
            "M20.054 15.987H3.946",
        )
    val Speaker = glyph("speaker", rect(4f, 2f, 16f, 20f), "M12 6h.01", circle(12f, 14f, 4f), "M12 14h.01")
    val Browser = glyph("globe", circle(12f, 12f, 10f), "M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20", "M2 12h20")
    val Devices =
        glyph(
            "cast",
            "M2 8V6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-6",
            "M2 12a9 9 0 0 1 8 8",
            "M2 16a5 5 0 0 1 4 4",
            "M2 20h.01",
        )

    /**
     * A device of the listening session, by what it declared itself: a phone, a speaker, a browser.
     * The web build declares a computer named "Browser" (`harken/iced`), so that name is a browser too.
     */
    fun device(kind: String, name: String = ""): androidx.compose.ui.graphics.vector.ImageVector =
        when (kind) {
            "phone" -> Phone
            "speaker" -> Speaker
            "browser" -> Browser
            else -> if (name == "Browser") Browser else Computer
        }

    val Bell =
        glyph(
            "bell",
            "M10.268 21a2 2 0 0 0 3.464 0",
            "M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326",
        )
}
