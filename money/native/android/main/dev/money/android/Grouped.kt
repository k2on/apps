// An account's kind as money's pages mark it: its glyph in a chart colour over a wash of it
// ([KindTile]), the iPhone's `AccountMark`. The tile is ArkUI's (`Tile`), so the two phones show
// the same mark.
package dev.money.android

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import dev.arkui.Tile
import dev.arkui.chartPalette

/** A kind's colour, by its place in the theme's chart palette: the iPhone's `AccountMark.color`. */
@Composable
fun kindColor(kind: String): Color {
    val c = chartPalette()
    return when (kind) {
        "checking" -> c[1]
        "savings" -> c[0]
        "cash" -> c[5]
        "credit" -> c[3]
        "loan" -> c[2]
        "investment" -> c[4]
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
}

/** An account's kind as its mark: the kind's glyph in the kind's colour. */
@Composable
fun KindTile(kind: String, side: Dp = 32.dp) = Tile(Glyphs.account(kind), kindColor(kind), side)
