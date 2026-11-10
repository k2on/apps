// The few drawings this library's own pieces draw — a row's menu, the
// chevron at a row's end, a page's way back, a volume's mute, the account
// sheet's rows — and the plain ones any app reaches for (search, clear,
// add, a tick, offline), as `ui/rust/arkui`'s Lucide
// glyphs (ISC; the licence is `ui/rust/arkui/src/LICENSE.lucide`): the same
// paths, stroked two units wide with butt caps and miter joins, so one
// menu button is one shape on every client. [ArkGlyphs.named] finds one
// by its Lucide name, as `arkui::glyphs::named` does on the desktop and
// `ArkGlyphs.symbol` maps the same names to SF Symbols on the iPhone. A
// glyph only one app means anything by is the app's. Geometry only: each is drawn in a placeholder ink that `Icon`
// replaces with the content colour.
package dev.arkui

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

    // A loudspeaker, as Lucide draws it, with its compact arcs written out: the same shape.
    private const val CONE: String =
        "M11 4.702a0.705 0.705 0 0 0 -1.203 -0.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0 -1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 0.997 0.413l3.383 3.384A0.705 0.705 0 0 0 11 19.298z"

    /** A volume that sounds: the loudspeaker and two waves (`volume-2`). */
    public val Volume: ImageVector =
        glyph("volume-2", CONE, "M16 9a5 5 0 0 1 0 6", "M19.364 18.364a9 9 0 0 0 0 -12.728")

    /** A volume muted: the loudspeaker struck through (`volume-x`). */
    public val Muted: ImageVector = glyph("volume-x", CONE, "M22 9l-6 6", "M16 9l6 6")

    /** Settings: a cog (`settings`). */
    public val Settings: ImageVector =
        glyph(
            "settings",
            "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z",
            circle(12f, 12f, 3f),
        )

    /** Signing out: out of a door (`log-out`). */
    public val SignOut: ImageVector = glyph("log-out", "m16 17 5-5-5-5", "M21 12H9", "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4")

    /** The server answers: a cloud (`cloud`). */
    public val Linked: ImageVector = glyph("cloud", "M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z")

    /** The server does not answer: the cloud struck through (`cloud-off`). */
    public val Unlinked: ImageVector =
        glyph(
            "cloud-off",
            "m2 2 20 20",
            "M5.782 5.782A7 7 0 0 0 9 19h8.5a4.5 4.5 0 0 0 1.307-.193",
            "M21.532 16.5A4.5 4.5 0 0 0 17.5 10h-1.79A7.008 7.008 0 0 0 10 5.07",
        )

    /** Something wants a look: a circle with a mark (`circle-alert`). */
    public val Attention: ImageVector = glyph("circle-alert", circle(12f, 12f, 10f), "M12 8v4", "M12 16h.01")

    /** Searching: a magnifier (`search`). */
    public val Search: ImageVector = glyph("search", "m21 21-4.34-4.34", circle(11f, 11f, 8f))

    /** Clearing or closing (`x`). */
    public val Close: ImageVector = glyph("x", "M18 6 6 18", "m6 6 12 12")

    /** Adding (`plus`). */
    public val Plus: ImageVector = glyph("plus", "M5 12h14", "M12 5v14")

    /** Taking away (`minus`). */
    public val Minus: ImageVector = glyph("minus", "M5 12h14")

    /** Done, or chosen (`check`). */
    public val Tick: ImageVector = glyph("check", "M20 6 9 17l-5-5")

    /** A box ticked (`circle-check`). */
    public val Ticked: ImageVector = glyph("circle-check", circle(12f, 12f, 10f), "m16 9-5.5 5.5L8 12")

    /** A box not ticked (`circle`). */
    public val Untick: ImageVector = glyph("circle", circle(12f, 12f, 10f))

    /** Opening downward (`chevron-down`). */
    public val Down: ImageVector = glyph("chevron-down", "m6 9 6 6 6-6")

    /** No network (`wifi-off`). */
    public val Offline: ImageVector =
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

    /** Renaming (`pencil`). */
    public val Rename: ImageVector =
        glyph(
            "pencil",
            "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z",
            "m15 5 4 4",
        )

    /** Copying (`copy`). */
    public val Copy: ImageVector =
        glyph(
            "copy",
            "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z",
            "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
        )

    /** Closing an account, putting something away (`archive`). */
    public val Archive: ImageVector =
        glyph(
            "archive",
            "M3 3h18a1 1 0 0 1 1 1v3a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z",
            "M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8",
            "M10 12h4",
        )

    /** Reopening, undoing (`undo-2`). */
    public val Reopen: ImageVector =
        glyph("undo-2", "M9 14 4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11")

    /** Agreeing with the bank (`badge-check`). */
    public val Reconcile: ImageVector =
        glyph(
            "badge-check",
            "M3.85 8.62a4 4 0 0 1 4.78-4.77 4 4 0 0 1 6.74 0 4 4 0 0 1 4.78 4.78 4 4 0 0 1 0 6.74 4 4 0 0 1-4.77 4.78 4 4 0 0 1-6.75 0 4 4 0 0 1-4.78-4.77 4 4 0 0 1 0-6.76Z",
            "m9 12 2 2 4-4",
        )

    /** Adding a thing of a list's own (`circle-plus`). */
    public val Add: ImageVector = glyph("circle-plus", circle(12f, 12f, 10f), "M8 12h8", "M12 8v8")

    /** `panel-right`, Lucide's. */
    public val PanelRight: ImageVector = glyph("panel-right", "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M15 3v18")

    /** `wand-sparkles`, Lucide's. */
    public val Wand: ImageVector = glyph("wand-sparkles", "m21.64 3.64-1.28-1.28a1.21 1.21 0 0 0-1.72 0L2.36 18.64a1.21 1.21 0 0 0 0 1.72l1.28 1.28a1.2 1.2 0 0 0 1.72 0L21.64 5.36a1.2 1.2 0 0 0 0-1.72", "m14 7 3 3", "M5 6v4", "M19 14v4", "M10 2v2", "M7 8H3", "M21 16h-4", "M11 3H9")

    /** `refresh-cw`, Lucide's. */
    public val Sync: ImageVector = glyph("refresh-cw", "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5")

    /** `list-filter`, Lucide's. */
    public val Filter: ImageVector = glyph("list-filter", "M3 6h18", "M7 12h10", "M10 18h4")

    /** `mail`, Lucide's. */
    public val Mail: ImageVector = glyph("mail", "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z", "m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7")

    /** `chart-column`, Lucide's. */
    public val Chart: ImageVector = glyph("chart-column", "M3 3v16a2 2 0 0 0 2 2h16", "M18 17V9", "M13 17V5", "M8 17v-3")

    /** `list`, Lucide's. */
    public val Rows: ImageVector = glyph("list", "M3 12h.01", "M3 18h.01", "M3 6h.01", "M8 12h13", "M8 18h13", "M8 6h13")

    /** `inbox`, Lucide's. */
    public val Inbox: ImageVector = glyph("inbox", "M22 12h-6l-2 3h-4l-2-3H2", "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z")

    /** `triangle-alert`, Lucide's. */
    public val Alert: ImageVector = glyph("triangle-alert", "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3", "M12 9v4", "M12 17h.01")

    /** `clock`, Lucide's. */
    public val Clock: ImageVector = glyph("clock", "M22 12a10 10 0 1 1-20 0a10 10 0 1 1 20 0z", "M12 6v6l4 2")

    /** `dollar-sign`, Lucide's. */
    public val Dollar: ImageVector = glyph("dollar-sign", "M12 2v20", "M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6")

    /** `banknote`, Lucide's. */
    public val Banknote: ImageVector = glyph("banknote", "M4 6h16a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2z", "M14 12a2 2 0 1 1-4 0a2 2 0 1 1 4 0z", "M6 12h.01", "M18 12h.01")

    /** `target`, Lucide's. */
    public val Target: ImageVector = glyph("target", "M22 12a10 10 0 1 1-20 0a10 10 0 1 1 20 0z", "M18 12a6 6 0 1 1-12 0a6 6 0 1 1 12 0z", "M14 12a2 2 0 1 1-4 0a2 2 0 1 1 4 0z")

    /** `arrow-left-right`, Lucide's. */
    public val Arrows: ImageVector = glyph("arrow-left-right", "M8 3 4 7l4 4", "M4 7h16", "m16 21 4-4-4-4", "M20 17H4")

    /** `users`, Lucide's. */
    public val Users: ImageVector = glyph("users", "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2", "M13 7a4 4 0 1 1-8 0a4 4 0 1 1 8 0z", "M22 21v-2a4 4 0 0 0-3-3.87", "M16 3.13a4 4 0 0 1 0 7.75")

    /** `house`, Lucide's: a home, a mortgage. */
    public val House: ImageVector = glyph("house", "M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8", "M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z")

    /** `landmark`, Lucide's: a bank, a checking account. */
    public val Landmark: ImageVector = glyph("landmark", "M3 22h18", "M6 18v-7", "M10 18v-7", "M14 18v-7", "M18 18v-7", "M12 2 20 7H4z")

    /** `credit-card`, Lucide's. */
    public val Card: ImageVector = glyph("credit-card", "M4 5h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z", "M2 10h20")

    /** `trending-up`, Lucide's: an investment. */
    public val Trending: ImageVector = glyph("trending-up", "M16 7h6v6", "m22 7-8.5 8.5-5-5L2 17")

    /** Every glyph here by its Lucide name, the name the other toolkits know it by. */
    private val byName: Map<String, ImageVector> by lazy {
        listOf(
            More, Back, Chevron, Volume, Muted, Settings, SignOut, Linked, Unlinked, Attention, Search, Close, Plus, Minus, Tick, Ticked,
            Untick, Down, Offline, Rename, Copy, Archive, Reopen, Reconcile, Add,
            PanelRight, Wand, Sync, Filter, Mail, Chart, Rows, Inbox, Alert, Clock, Dollar, Banknote, Target, Arrows, Users, House,
            Landmark, Card, Trending,
        )
            .associateBy { it.name } + ("ellipsis-vertical" to More)
    }

    /**
     * The glyph Lucide calls [name] (`search`, `x`, `chevron-right`, `ellipsis-vertical`), if this
     * library has it: the names ui/docs/parts.md lists, the same on every toolkit.
     */
    public fun named(name: String): ImageVector? = byName[name]
}
