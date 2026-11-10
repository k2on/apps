// money's own drawings, the navigation bar's above all, drawn as arkui's
// glyphs are (`ArkGlyphs.glyph`): 24 units, stroked two wide — outlined at
// rest, and filled for the tab that is showing, as Material's navigation bar
// draws its icons. Everything else is Material's own icons.
package dev.money.android

import androidx.compose.ui.graphics.vector.ImageVector
import dev.arkui.ArkGlyphs.circle
import dev.arkui.ArkGlyphs.glyph
import dev.arkui.ArkGlyphs.rect

object Glyphs {
    /** This app's glyphs by their shared names, for the screens written from Rust (`LocalGlyphs`). */
    fun shared(name: String): ImageVector? =
        when (name) {
            "layers" -> Activity
            "link" -> Link
            else -> null
        }

    private const val WALLET_BODY = "M19 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2H5"
    private const val WALLET_CLASP = "M21 12h-4a2 2 0 0 0 0 4h4z"

    /** The budget: a wallet. */
    val Budget: ImageVector = glyph("wallet", WALLET_BODY, WALLET_CLASP)
    val BudgetFilled: ImageVector = glyph("wallet-filled", WALLET_BODY, WALLET_CLASP, filled = true)

    private const val ROOF = "M3 9 12 3l9 6z"
    private const val GROUND = "M3 21h18"

    /** Accounts: a bank's columns. */
    val Accounts: ImageVector = glyph("landmark", ROOF, GROUND, "M5 17h14", "M6 10v7", "M10 10v7", "M14 10v7", "M18 10v7")
    val AccountsFilled: ImageVector =
        glyph("landmark-filled", ROOF, GROUND, "M5 17h14", rect(5f, 10f, 3f, 7f), rect(10.5f, 10f, 3f, 7f), rect(16f, 10f, 3f, 7f), filled = true)

    private const val TRAY = "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"

    /** Transactions: an inbox's tray. */
    val Activity: ImageVector = glyph("inbox", "M22 12h-6l-2 3h-4l-2-3H2", TRAY)
    val ActivityFilled: ImageVector = glyph("inbox-filled", TRAY, filled = true)

    private const val PIE_SLICE = "M21 12A9 9 0 0 0 12 3v9z"
    private const val PIE_REST = "M21 12a9 9 0 1 1-9-9"

    /** Insights: a pie. */
    val Insights: ImageVector = glyph("chart-pie", PIE_SLICE, PIE_REST)
    val InsightsFilled: ImageVector = glyph("chart-pie-filled", PIE_SLICE, PIE_REST + "v9z", filled = true)

    /** A transfer: two arrows, each way. */
    val Transfer: ImageVector = glyph("arrow-left-right", "M8 3 4 7l4 4", "M4 7h16", "m16 21 4-4-4-4", "M20 17H4")

    /** Moving money between categories. */
    val Move: ImageVector = glyph("move", "M5 12h14", "m13 6 6 6-6 6")

    /** Cleared: the bank has seen it. */
    val Cleared: ImageVector = glyph("circle-check", circle(12f, 12f, 10f), "m8 12 3 3 5-6")
    val ClearedFilled: ImageVector = glyph("circle-check-filled", circle(12f, 12f, 10f), filled = true)

    /** Reconciled: agreed with the bank and locked. */
    val Locked: ImageVector = glyph("lock", rect(4f, 11f, 16f, 10f), "M8 11V7a4 4 0 0 1 8 0v4")

    /** An account that is a card. */
    val Card: ImageVector = glyph("credit-card", rect(2f, 5f, 20f, 14f), "M2 10h20")

    /** Cash. */
    val Cash: ImageVector = glyph("banknote", rect(2f, 6f, 20f, 12f), circle(12f, 12f, 2f), "M6 12h.01M18 12h.01")

    /** Savings: a jar. */
    val Savings: ImageVector = glyph("piggy", "M7 7h10l3 4v8H4v-8z", "M9 4h6v3H9z")

    /** Investments and loans: a line going up. */
    val Growth: ImageVector = glyph("trending-up", "m22 7-8.5 8.5-5-5L2 17", "M16 7h6v6")

    /** A link to the bank. */
    val Link: ImageVector = glyph("link", "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71", "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71")

    /** All caught up. */
    val Done: ImageVector = glyph("check-check", "M18 6 7 17l-5-5", "m22 10-7.5 7.5L13 16")

    /** A sparkle for auto-assign. */
    val Auto: ImageVector = glyph("sparkles", "M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z", "M19 17v4", "M17 19h4")

    /** Assigned by rule: a share of what came in. */
    val Rule: ImageVector = glyph("percent", "M19 5 5 19", circle(6.5f, 6.5f, 2.5f), circle(17.5f, 17.5f, 2.5f))

    private const val SLIP = "M4 2v20l2-1 2 1 2-1 2 1 2-1 2 1 2-1 2 1V2l-2 1-2-1-2 1-2-1-2 1-2-1-2 1z"

    /** A receipt: a slip with a torn edge, and its lines. */
    val Receipt: ImageVector = glyph("receipt", SLIP, "M8 8h8", "M8 12h8", "M8 16h5")

    /** Scanning: a page in a viewfinder's corners. */
    val Scan: ImageVector =
        glyph("scan", "M3 7V5a2 2 0 0 1 2-2h2", "M17 3h2a2 2 0 0 1 2 2v2", "M21 17v2a2 2 0 0 1-2 2h-2", "M7 21H5a2 2 0 0 1-2-2v-2", "M7 12h10")

    /** A photo from the library. */
    val Photo: ImageVector = glyph("image", rect(3f, 3f, 18f, 18f), circle(9f, 9f, 2f), "m21 15-3.09-3.09a2 2 0 0 0-2.82 0L6 21")

    /** Typed by hand: a pencil. */
    val Write: ImageVector =
        glyph(
            "pencil",
            "M21.17 6.81a1 1 0 0 0-3.98-3.98L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5z",
            "m15 5 4 4")

    /** The glyph an account of `kind` is drawn with. */
    fun account(kind: String): ImageVector =
        when (kind) {
            "credit" -> Card
            "cash" -> Cash
            "savings" -> Savings
            "loan", "investment" -> Growth
            else -> Accounts
        }
}
