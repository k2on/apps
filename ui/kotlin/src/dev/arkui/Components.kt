// The pieces the shell's native containers are made of, with nothing of any
// app in them: a glyph at a control's size, a tab's title, a page's bar, a
// row that opens something and its chevron. What an app lays out of its own
// (a card, a shelf, an empty list's sentence) is a Rust composition of the
// primitives (ui/docs/primitives.md), not a part here.
package dev.arkui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** A glyph, at the size a row's controls are. */
@Composable
public fun Glyph(
    icon: ImageVector,
    description: String?,
    modifier: Modifier = Modifier,
    size: Dp = 24.dp
) {
    Icon(icon, contentDescription = description, modifier = modifier.size(size))
}

/**
 * A tab's own title, the one large title row every tab of every app has: the iPhone's large title
 * ([LargeTitle], 34 sp bold) at the leading edge, [actions] (a menu, never a +) at the line's end,
 * and [account] — the account's face ([AccountFace]) — after them, at the very end of the title's
 * own line, centred on it, as the iPhone's apps put it: not in a row above the title. One top
 * inset, one height, one font ([TabTitle.TOP], [TabTitle.LINE], [TabTitle.BOTTOM]), laid out from
 * the status bar's foot wherever it is used, so the title's baseline and the face are in the same
 * place on every tab: harken's in its list, money's over its page (`Page`). An app adds no padding
 * of its own around it.
 */
@Composable
public fun TabTitle(
    title: String,
    account: (@Composable () -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Row(
        Modifier.fillMaxWidth()
            .padding(start = Space.page, end = Space.xs, top = TabTitle.TOP, bottom = TabTitle.BOTTOM)
            .height(TabTitle.LINE),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            style = MaterialTheme.typography.headlineLarge.merge(LargeTitle),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).semantics { heading() },
        )
        actions()
        account?.invoke()
    }
}

/** The large title row's measures, written once: [TabTitle] is laid out by nothing else. */
public object TabTitle {
    /** From the status bar's foot to the title's line. */
    public val TOP: Dp = Space.l

    /** The title's line, which the face and the actions are centred on. */
    public val LINE: Dp = 52.dp

    /**
     * Under the line, the row's own foot: with the taller line it puts the title's baseline as far
     * above the row's end as the iPhone's (`AccountSizes.titleBottom` under its 44 pt line).
     */
    public val BOTTOM: Dp = Space.xs

    /** The whole row. */
    public val HEIGHT: Dp = TOP + LINE + BOTTOM

    /**
     * From the row's end to the page's first content, on every tab of every app, the iPhone's too
     * (`AccountSizes.titleBelow`): a section's own top margin, so a card, a field, a pager or a
     * row of chips under the title sits where harken Home's "Recently Added" does: what comes
     * first is padded by it (money's `Page` hands it on as its content's top padding).
     */
    public val BELOW: Dp = Space.xl
}

/** A page's own bar: back, its title, and what else the page offers. */
@Composable
public fun PageBar(
    title: String,
    onBack: () -> Unit,
    actions: @Composable RowScope.() -> Unit = {}
) {
    Row(
        Modifier.fillMaxWidth().height(56.dp).padding(horizontal = Space.xs),
        verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Glyph(ArkGlyphs.Back, "Back") }
            Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f))
            actions()
        }
}

/**
 * A row that opens something: its picture, its name (in the accent when [accent]), a line under
 * it, and what stands at its end — a chevron, unless the caller says otherwise. In a [GroupCard]
 * it is drawn as a grouped list's row: the card's inset, the body's type, the picture in a
 * [GroupRow.LEAD] box.
 */
@Composable
public fun ListRow(
    onClick: () -> Unit,
    title: String,
    under: String? = null,
    trailing: @Composable () -> Unit = { Chevron() },
    accent: Boolean = false,
    picture: @Composable () -> Unit
) {
    if (LocalInGroup.current) {
        GroupListRow(onClick, title, under, accent, trailing, picture)
        return
    }
    Row(
        Modifier.fillMaxWidth()
            .pressable(onClick = onClick, press = Press.Highlight)
            .padding(horizontal = Space.page, vertical = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        picture()
        Spacer(Modifier.width(Space.l))
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                color = if (accent) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis)
            if (under != null)
                Text(under, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 1)
        }
        trailing()
    }
}

/** The chevron at the end of a row that opens something. */
@Composable
public fun Chevron() {
    Icon(
        ArkGlyphs.Chevron,
        contentDescription = null,
        tint = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.size(18.dp))
}
