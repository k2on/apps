// The pieces money's pages are laid out of that are money's, the way the
// iPhone app lays them (an inset grouped list): the one figure a page is
// about in a card washed in its tint ([HeroCard]), a section's total at the
// end of its heading ([HeadingTotal]), an account's kind as its symbol in a
// chart colour over a wash of it ([KindTile]), and the month paged under
// the page's title ([MonthPager]). The heading, the card and its hairlines,
// the tile, the badge and the pager are ArkUI's (`SectionHeader`,
// `GroupCard`, `RowDivider`, `Tile`, `Badge`, `Pager`). So the two phones
// show the same page.
package dev.money.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.arkui.AmountText
import dev.arkui.LocalAmounts
import dev.arkui.Space
import dev.arkui.Pager
import dev.arkui.Tile
import dev.arkui.tintInk
import dev.arkui.Tint
import dev.arkui.chartPalette
import dev.arkui.pressable
import uniffi.ark_format.formatMonth

/** The hero's corners: a little rounder, as the larger thing. */
private val HeroCorner: Dp = 28.dp

/**
 * The one figure a page is about: its caption, quiet; the amount, large and bold, in its tone (ink
 * when neutral); a line under it; and what follows (`footer`), on a card washed from the tone's
 * colour at its top corner to almost nothing. Tapping it is `onClick`, when there is one.
 */
@Composable
fun HeroCard(
    caption: String,
    cents: Long,
    tone: Tint,
    note: String,
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
    footer: @Composable ColumnScope.() -> Unit = {},
) {
    val a = LocalAmounts.current
    val ink = tintInk(tone)
    val shape = RoundedCornerShape(HeroCorner)
    Column(
        modifier
            .fillMaxWidth()
            // Before the clip and the fills, so the whole card gives under the finger.
            .then(if (onClick != null) Modifier.pressable(onClick = onClick) else Modifier)
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .background(Brush.linearGradient(listOf(ink.copy(alpha = 0.22f), ink.copy(alpha = 0.04f))))
            .padding(Space.xl)
            .semantics(mergeDescendants = true) { contentDescription = "$caption, ${a.format(cents)}" + if (note.isNotEmpty()) ", $note" else "" },
        verticalArrangement = Arrangement.spacedBy(Space.xs),
    ) {
        Text(caption, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurfaceVariant)
        AmountText(
            cents,
            style = MaterialTheme.typography.displaySmall.copy(fontWeight = FontWeight.Bold, fontSize = 40.sp, lineHeight = 48.sp),
            color = if (tone == Tint.Neutral) MaterialTheme.colorScheme.onSurface else ink,
        )
        if (note.isNotEmpty()) {
            Text(note, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        footer()
    }
}

/** A section's total at the end of its heading, quiet, or the error's red below nothing. */
@Composable
fun HeadingTotal(cents: Long) {
    AmountText(
        cents,
        style = MaterialTheme.typography.titleMedium,
        color = if (cents < 0) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
}

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

/**
 * The month under a page's title, paged back and forth, the budget's and the insights' alike: the
 * page's first content, so nothing over it but the title's own gap (`TabTitle.BELOW`, `Page`'s).
 * The stepper's round buttons are 40 dp in Material's 48 dp touch target, so the row is drawn up
 * by the Space.xs of touch over them — into the gap, which it does not take room from — and its
 * circles' tops are where the iPhone's pager's are; what follows comes up with it.
 */
@Composable
fun MonthPager(ledger: Ledger, modifier: Modifier = Modifier) {
    val book = LocalShell.current.book
    Row(modifier.fillMaxWidth().offset(y = -Space.xs).padding(bottom = Space.s), horizontalArrangement = Arrangement.Center) {
        Pager(
            title = formatMonth(ledger.month),
            back = { book.stepMonth(-1) },
            forward = { book.stepMonth(1) },
            backLabel = "Previous month",
            forwardLabel = "Next month",
            order = ledger.month.filter { it.isDigit() }.toIntOrNull() ?: 0,
        )
    }
}
