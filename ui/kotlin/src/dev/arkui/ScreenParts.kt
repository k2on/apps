// The parts a screen laid out in Rust is composed of (ui/docs/app.md):
// each has the name and the inputs its Rust builder declares
// (`ark_app::view::Part::call`), so the generated screens (`ark-ui-gen`)
// are only calls to these. Everything a part does is here, by hand, where
// Android Studio previews and checks it. What an app adds, once at its
// root: the frame a tab's page sits in (`LocalTabFrame`, its top bar and
// face), the mark a row of its own kind wears (`LocalRowMark`) and its own
// glyphs by name (`LocalGlyphs`).
package dev.arkui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// * What the app provides

/** The frame a tab's page sits in: its top bar (the face at its end) and its pull to refresh. */
public val LocalTabFrame = compositionLocalOf<@Composable (title: String, onRefresh: (() -> Unit)?, content: @Composable () -> Unit) -> Unit> {
    { _, _, content -> content() }
}

/** The mark a `ListRow(mark = …)` wears, by its kind: an account's. */
public val LocalRowMark = compositionLocalOf<@Composable (String) -> Unit> { { } }

/** The app's glyphs by their shared names, before ArkUI's own. */
public val LocalGlyphs = compositionLocalOf<(String) -> ImageVector?> { { null } }

@Composable
private fun glyph(name: String): ImageVector = LocalGlyphs.current(name) ?: ArkGlyphs.named(name) ?: ArkGlyphs.Chevron

// * The page

/** A tab's page in the app's frame: `top` under the title, then `content`, scrolling. */
@Composable
public fun TabPage(title: String, onRefresh: (() -> Unit)? = null, top: @Composable () -> Unit = {}, content: @Composable () -> Unit) {
    LocalTabFrame.current(title, onRefresh) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = Space.page).padding(bottom = 96.dp)) {
            top()
            content()
        }
    }
}

/**
 * The page's one large figure: its caption, the amount large, a line under it, on a card washed
 * from the neutral tint at its top corner to almost nothing.
 */
@Composable
public fun HeroFigure(caption: String, cents: Long, note: String) {
    val a = LocalAmounts.current
    val ink = tintInk(Tint.Neutral)
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(28.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .background(Brush.linearGradient(listOf(ink.copy(alpha = 0.22f), ink.copy(alpha = 0.04f))))
            .padding(Space.xl)
            .semantics(mergeDescendants = true) { contentDescription = "$caption, ${a.format(cents)}, $note" },
        verticalArrangement = Arrangement.spacedBy(Space.xs),
    ) {
        Text(caption, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurfaceVariant)
        AmountText(
            cents,
            style = MaterialTheme.typography.displaySmall.copy(fontWeight = FontWeight.Bold, fontSize = 40.sp, lineHeight = 48.sp),
            color = MaterialTheme.colorScheme.onSurface,
        )
        Text(note, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** A section's card under its heading (a total at its end, the error's red below nothing), a note under it. */
@Composable
public fun CardSection(title: String? = null, total: Long? = null, showsTotal: Boolean = true, footer: String? = null, content: @Composable () -> Unit) {
    if (title != null) {
        SectionHeader(title) {
            if (total != null && showsTotal) {
                AmountText(
                    total,
                    style = MaterialTheme.typography.titleMedium,
                    color = if (total < 0) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    } else {
        Spacer(Modifier.height(Space.xl))
    }
    GroupCard { content() }
    if (footer != null) {
        Text(footer, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = Space.l, vertical = Space.s))
    }
}

// * Rows

/** One thing a row's menu offers. */
public class MenuAction(public val title: String, public val glyph: String, public val destructive: Boolean = false, public val onTap: () -> Unit)

/**
 * A row of a card: its picture (a tile, the app's mark, or a bare glyph in the accent), its title
 * and the line under it, a count in a badge, an amount; tapped, `onTap`, lit as it is
 * (`Press.Highlight`); held, its menu.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
public fun ListRow(
    title: String,
    subtitle: String? = null,
    tile: String? = null,
    mark: String? = null,
    glyph: String? = null,
    badge: Long? = null,
    amount: Long? = null,
    quiet: Boolean = false,
    accent: Boolean = false,
    said: String? = null,
    menu: List<MenuAction> = emptyList(),
    onTap: (() -> Unit)? = null,
) {
    var open by remember { mutableStateOf(false) }
    val press = if (menu.isEmpty()) Modifier.pressable(onClick = { onTap?.invoke() }, press = Press.Highlight)
    else Modifier.combinedClickable(interactionSource = null, indication = null, onClick = { onTap?.invoke() }, onLongClick = { open = true })
    Row(
        Modifier.fillMaxWidth()
            .then(press)
            .padding(horizontal = Space.l, vertical = Space.m)
            .semantics(mergeDescendants = true) { if (said != null) contentDescription = said },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        when {
            tile != null -> Tile(glyph(tile), MaterialTheme.colorScheme.onSurfaceVariant)
            mark != null -> LocalRowMark.current(mark)
            glyph != null -> Icon(glyph(glyph), contentDescription = null, tint = MaterialTheme.colorScheme.primary)
        }
        Spacer(Modifier.width(Space.m))
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MaterialTheme.typography.bodyLarge,
                color = if (accent) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis)
            if (subtitle != null) {
                Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            }
        }
        if (badge != null) {
            Spacer(Modifier.width(Space.s))
            Badge(badge.toInt(), "$badge to review")
        }
        if (amount != null) {
            Spacer(Modifier.width(Space.s))
            AmountText(
                amount,
                style = MaterialTheme.typography.bodyLarge,
                color = when {
                    quiet -> MaterialTheme.colorScheme.onSurfaceVariant
                    amount < 0 -> MaterialTheme.colorScheme.error
                    else -> MaterialTheme.colorScheme.onSurface
                })
        }
        if (onTap != null && !accent) {
            Spacer(Modifier.width(Space.xs))
            Chevron()
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (m in menu) {
                DropdownMenuItem(
                    text = { Text(m.title, color = if (m.destructive) MaterialTheme.colorScheme.error else Color.Unspecified) },
                    leadingIcon = { Icon(glyph(m.glyph), contentDescription = null) },
                    onClick = {
                        open = false
                        m.onTap()
                    })
            }
        }
    }
}

// * What rises over a page

/** A sheet, up for as long as it is composed; put away by the person, `onDismiss`. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun Sheet(title: String, onDismiss: () -> Unit, content: @Composable () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.padding(horizontal = Space.xl).navigationBarsPadding().imePadding(), verticalArrangement = Arrangement.spacedBy(Space.s)) {
            Text(title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
            content()
            Spacer(Modifier.height(Space.xl))
        }
    }
}

/** An alert with one text field, up for as long as it is composed. */
@Composable
public fun Prompt(title: String, text: String, onChange: (String) -> Unit, confirm: String, onConfirm: () -> Unit, onCancel: () -> Unit) {
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(title) },
        text = { OutlinedTextField(value = text, onValueChange = onChange, singleLine = true) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(confirm) } },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel") } },
    )
}

// * A form's fields

/** A name typed, the domain's sentence under it. */
@Composable
public fun NameField(label: String, text: String, problem: String?, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = text,
        onValueChange = onChange,
        label = { Text(label) },
        isError = problem != null,
        supportingText = problem?.let { why -> { Text(why) } },
        singleLine = true,
        modifier = Modifier.fillMaxWidth())
}

/** An amount typed as text; the core says what it means. */
@Composable
public fun AmountField(label: String, text: String, problem: String?, onChange: (String) -> Unit) {
    AmountField(text, onType = onChange, label = label, error = problem, modifier = Modifier.fillMaxWidth())
}

/** Chips in a row that scrolls sideways. */
@Composable
public fun ChipRow(content: @Composable () -> Unit) {
    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(Space.s)) { content() }
}

/** A chip, its glyph before its label while it is chosen. */
@Composable
public fun Chip(label: String, glyph: String, selected: Boolean, onTap: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onTap,
        label = { Text(label) },
        leadingIcon = if (selected) { { Icon(glyph(glyph), contentDescription = null, modifier = Modifier.size(18.dp)) } } else null,
    )
}

/** A switch with its title and a note under it. */
@Composable
public fun SwitchRow(title: String, note: String, isOn: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(note) },
        trailingContent = { Switch(checked = isOn, onCheckedChange = onChange) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
    )
}

/** A form's button, the width of the form. */
@Composable
public fun FormButton(title: String, enabled: Boolean, onTap: () -> Unit) {
    Button(onClick = onTap, enabled = enabled, modifier = Modifier.fillMaxWidth()) { Text(title) }
}
