// The parts a screen laid out in Rust is composed of (ui/docs/app.md):
// each has the name and the inputs its Rust builder declares
// (`ark_app::view::Part::call`), so the generated screens (`ark-ui-gen`)
// are only calls to these. Everything a part does is here, by hand, where
// Android Studio previews and checks it. What the app library's shell adds,
// once at its root: the frame a tab's page sits in (`LocalTabFrame`: under
// the shell's top bar, its pull to refresh). Nothing is an app's own: a
// row's mark is its glyph and its chart slot ([MarkTile]).
package dev.arkui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
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
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
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

private fun glyph(name: String): ImageVector = ArkGlyphs.named(name) ?: ArkGlyphs.Chevron

// * The page

/**
 * A tab's page in the app's frame: `top` under the title, then `content`, scrolling, as the iPhone's
 * inset grouped list lays one out — the first card `title-below` under the large title, the page's
 * margin at its sides, and one `section-gap` between each part and the next (a section is one part:
 * its heading, its card and its note), so no part keeps a margin of its own.
 */
@Composable
public fun TabPage(title: String, onRefresh: (() -> Unit)? = null, top: @Composable () -> Unit = {}, content: @Composable () -> Unit) {
    LocalTabFrame.current(title, onRefresh) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState())
                .padding(horizontal = Space.page)
                .padding(top = LayoutTokens.TITLE_BELOW.dp, bottom = 96.dp),
            verticalArrangement = Arrangement.spacedBy(Space.section),
        ) {
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

/**
 * A section's card under its heading: a total at its end (the error's red below nothing) or words in
 * the caution's tint in its place, and, short of compact, two more figures before it under their
 * columns' titles; a note under it. A collapsible one's heading toggles it, its rows shown while
 * [open]; held, its [menu].
 */
@Composable
public fun CardSection(
    title: String? = null,
    total: Long? = null,
    showsTotal: Boolean = true,
    footer: String? = null,
    totalNote: String? = null,
    firstTitle: String? = null,
    secondTitle: String? = null,
    totalTitle: String? = null,
    first: Long? = null,
    second: Long? = null,
    open: Boolean = true,
    onToggle: (() -> Unit)? = null,
    menu: List<MenuAction> = emptyList(),
    content: @Composable () -> Unit,
) = Column {
    if (title != null) {
        var held by remember { mutableStateOf(false) }
        val offered = menu.filter { it.shown }
        val press = when {
            onToggle == null && offered.isEmpty() -> Modifier
            offered.isEmpty() -> Modifier.pressable(onClick = { onToggle?.invoke() }, press = Press.Highlight)
            else -> Modifier.pressable(onClick = { onToggle?.invoke() }, onLongClick = { held = true }, press = Press.Highlight)
        }
        val columns = currentSizeClass() != SizeClass.Compact && first != null && second != null
        Box(press) {
            SectionHeader(if (onToggle != null) (if (open) "▾ " else "▸ ") + title else title, top = 0.dp) {
                if (columns) {
                    FigureColumn(title = firstTitle) { AmountText(first!!, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    FigureColumn(title = secondTitle) { AmountText(second!!, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    FigureColumn(title = totalTitle) { SectionFigure(total, showsTotal, totalNote) }
                } else {
                    SectionFigure(total, showsTotal, totalNote)
                }
            }
            MenuOf(offered, held) { held = false }
        }
    }
    if (open) GroupCard { content() }
    if (footer != null) {
        Text(footer, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = Space.l, vertical = Space.s))
    }
}

@Composable
private fun SectionFigure(total: Long?, showsTotal: Boolean, totalNote: String?) {
    when {
        totalNote != null -> Text(totalNote, style = MaterialTheme.typography.titleSmall, color = tintInk(Tint.Caution))
        total != null && showsTotal -> AmountText(
            total,
            style = MaterialTheme.typography.titleMedium,
            color = if (total < 0) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** A figure under its column's title, as wide as the theme's amount column. */
@Composable
private fun FigureColumn(title: String?, figure: @Composable () -> Unit) {
    androidx.compose.foundation.layout.Column(Modifier.width(LayoutTokens.AMOUNT_COLUMN_WIDTH.dp), horizontalAlignment = Alignment.End) {
        if (title != null) Text(title, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.outline)
        figure()
    }
}

/** A menu's offered actions, while [open]. */
@Composable
private fun MenuOf(offered: List<MenuAction>, open: Boolean, onDismiss: () -> Unit) {
    DropdownMenu(expanded = open, onDismissRequest = onDismiss) {
        for (m in offered) {
            DropdownMenuItem(
                text = { Text(m.title, color = if (m.destructive) MaterialTheme.colorScheme.error else Color.Unspecified) },
                leadingIcon = {
                    Icon(glyph(m.glyph), contentDescription = null, tint = if (m.destructive) MaterialTheme.colorScheme.error else LocalContentColor.current)
                },
                onClick = {
                    onDismiss()
                    m.onTap()
                })
        }
    }
}

/**
 * A row of a thing toward a goal: its mark before its title (faded while hidden), a slim bar
 * of how far in its tint, words under it in a tint with quiet ones at their end; short of compact,
 * two figures in columns; its figure in a status badge at its end. Tapped, [onTap]; held, its menu
 * — lit under the finger either way (`Press.Highlight`).
 */
@Composable
public fun MeterRow(
    title: String,
    status: String,
    statusTint: String,
    mark: String? = null,
    markSlot: Long = -1,
    faded: Boolean = false,
    fraction: Double? = null,
    meterTint: String = "positive",
    note: String? = null,
    noteTint: String = "neutral",
    noteEnd: String? = null,
    first: Long? = null,
    second: Long? = null,
    said: String? = null,
    menu: List<MenuAction> = emptyList(),
    onTap: (() -> Unit)? = null,
) {
    var open by remember { mutableStateOf(false) }
    val offered = menu.filter { it.shown }
    val press =
        if (offered.isEmpty()) Modifier.pressable(onClick = { onTap?.invoke() }, press = Press.Highlight)
        else Modifier.pressable(onClick = { onTap?.invoke() }, onLongClick = { open = true }, press = Press.Highlight)
    val columns = currentSizeClass() != SizeClass.Compact && first != null && second != null
    Row(
        Modifier.fillMaxWidth()
            .then(press)
            .padding(horizontal = Space.l, vertical = Space.m)
            .semantics(mergeDescendants = true) { contentDescription = said ?: title },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        androidx.compose.foundation.layout.Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.xs)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (mark != null) {
                    MarkTile(mark, markSlot)
                    Spacer(Modifier.width(Space.s))
                }
                Text(
                    title,
                    style = MaterialTheme.typography.bodyLarge,
                    color = if (faded) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis)
            }
            if (fraction != null) GoalBar(fraction.toFloat(), tintNamed(meterTint))
            if (note != null || noteEnd != null) {
                Row(Modifier.fillMaxWidth()) {
                    if (note != null) {
                        val t = tintNamed(noteTint)
                        Text(note, style = MaterialTheme.typography.bodySmall, color = if (t == Tint.Neutral) MaterialTheme.colorScheme.onSurfaceVariant else tintInk(t))
                    }
                    Spacer(Modifier.weight(1f))
                    if (noteEnd != null) Text(noteEnd, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        Spacer(Modifier.width(Space.s))
        if (columns) {
            Box(Modifier.width(LayoutTokens.AMOUNT_COLUMN_WIDTH.dp), contentAlignment = Alignment.CenterEnd) {
                AmountText(first!!, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface)
            }
            Box(Modifier.width(LayoutTokens.AMOUNT_COLUMN_WIDTH.dp), contentAlignment = Alignment.CenterEnd) {
                AmountText(second!!, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Box(Modifier.width(LayoutTokens.AMOUNT_COLUMN_WIDTH.dp), contentAlignment = Alignment.CenterEnd) { StatusBadge(status, tintNamed(statusTint)) }
        } else {
            StatusBadge(status, tintNamed(statusTint))
        }
        MenuOf(offered, open) { open = false }
    }
}

/** A view's headline (`ark_app::view::headline`): its tint by name, its one action a prominent capsule at its end. */
@Composable
public fun Headline(label: String, figure: String, tint: String, caption: String, actionTitle: String? = null, actionGlyph: String? = null, onAction: (() -> Unit)? = null) {
    GroupCard {
        Box(Modifier.padding(horizontal = Space.l, vertical = Space.m)) {
            Headline(label, figure, tintNamed(tint), caption) {
                if (onAction != null) {
                    Button(onClick = onAction) {
                        if (actionGlyph != null) {
                            Icon(glyph(actionGlyph), contentDescription = null, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(Space.s))
                        }
                        Text(actionTitle ?: "")
                    }
                }
            }
        }
    }
}

// * Rows

/** One thing a row's menu offers; offered only while [shown]. */
public class MenuAction(
    public val title: String,
    public val glyph: String,
    public val destructive: Boolean = false,
    public val shown: Boolean = true,
    public val onTap: () -> Unit,
)

/**
 * A row of a card: its picture (a tile, a thing's mark, or a bare glyph in the accent), its title
 * and the line under it, a count in a badge, an amount, and a chevron when it opens a page
 * (`pushes`); tapped, `onTap`; held, its menu — lit under the finger either way
 * (`Press.Highlight`).
 */
@Composable
public fun ListRow(
    title: String,
    subtitle: String? = null,
    tile: String? = null,
    mark: String? = null,
    markSlot: Long = -1,
    glyph: String? = null,
    badge: Long? = null,
    amount: Long? = null,
    value: String? = null,
    quiet: Boolean = false,
    accent: Boolean = false,
    pushes: Boolean = false,
    said: String? = null,
    menu: List<MenuAction> = emptyList(),
    onTap: (() -> Unit)? = null,
) {
    var open by remember { mutableStateOf(false) }
    val offered = menu.filter { it.shown }
    val press =
        if (offered.isEmpty()) Modifier.pressable(onClick = { onTap?.invoke() }, press = Press.Highlight)
        else Modifier.pressable(onClick = { onTap?.invoke() }, onLongClick = { open = true }, press = Press.Highlight)
    Row(
        Modifier.fillMaxWidth()
            .then(press)
            .padding(horizontal = Space.l, vertical = Space.m)
            .semantics(mergeDescendants = true) { if (said != null) contentDescription = said },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        when {
            tile != null -> Tile(glyph(tile), MaterialTheme.colorScheme.onSurfaceVariant)
            mark != null -> MarkTile(mark, markSlot)
            glyph != null -> Icon(glyph(glyph), contentDescription = null, tint = MaterialTheme.colorScheme.primary)
        }
        // The words at the card's inset where nothing is before them, as every card's are.
        if (tile != null || mark != null || glyph != null) Spacer(Modifier.width(Space.m))
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
        if (value != null) {
            Spacer(Modifier.width(Space.s))
            Text(value, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
        }
        if (pushes) {
            Spacer(Modifier.width(Space.xs))
            Chevron()
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (m in offered) {
                DropdownMenuItem(
                    text = { Text(m.title, color = if (m.destructive) MaterialTheme.colorScheme.error else Color.Unspecified) },
                    leadingIcon = {
                        Icon(glyph(m.glyph), contentDescription = null, tint = if (m.destructive) MaterialTheme.colorScheme.error else LocalContentColor.current)
                    },
                    onClick = {
                        open = false
                        m.onTap()
                    })
            }
        }
    }
}

// * What rises over a page

/**
 * A sheet's bar, as the iPhone's: ✕ at its start (`onClose`), the title centred, and, with a
 * `confirm`, ✓ at its end, filled in the primary, enabled while `canConfirm`.
 */
@Composable
public fun SheetBar(title: String, onClose: () -> Unit, confirm: String? = null, canConfirm: Boolean = true, onConfirm: (() -> Unit)? = null) {
    Box(Modifier.fillMaxWidth().height(LayoutTokens.SHEET_BAR_HEIGHT.dp).padding(horizontal = Space.s)) {
        FilledTonalIconButton(onClick = onClose, modifier = Modifier.align(Alignment.CenterStart)) {
            Icon(ArkGlyphs.named("x") ?: ArkGlyphs.Chevron, contentDescription = "Cancel")
        }
        Text(
            title,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.align(Alignment.Center).padding(horizontal = 56.dp).semantics { heading() },
        )
        if (confirm != null && onConfirm != null) {
            FilledIconButton(onClick = onConfirm, enabled = canConfirm, modifier = Modifier.align(Alignment.CenterEnd)) {
                Icon(ArkGlyphs.named("check") ?: ArkGlyphs.Chevron, contentDescription = confirm)
            }
        }
    }
}

/**
 * A sheet with a form, up for as long as it is composed; put away by the person, `onDismiss`. Its
 * bar is the iPhone's ([SheetBar]): ✕ puts it away, ✓ confirms, enabled while `canConfirm`.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun FormSheet(
    title: String,
    onDismiss: () -> Unit,
    confirm: String? = null,
    canConfirm: Boolean = true,
    onConfirm: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        SheetBar(title, onClose = onDismiss, confirm = confirm, canConfirm = canConfirm, onConfirm = onConfirm)
        Column(
            Modifier.padding(horizontal = Space.page).navigationBarsPadding().imePadding().padding(top = Space.s, bottom = Space.xl),
            verticalArrangement = Arrangement.spacedBy(Space.section),
        ) {
            content()
        }
    }
}

/** A group of a form's fields: its heading over them, its note under them. */
@Composable
public fun FormSection(header: String? = null, footer: String? = null, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.xs)) {
        if (header != null) {
            Text(header, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.semantics { heading() })
        }
        content()
        if (footer != null) {
            Text(footer, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** What the choices of an `OptionPicker` read: the chosen tag, and what choosing one does. */
private val LocalPicking = compositionLocalOf<Pair<String, (String) -> Unit>> { "" to { } }

/** One of several, chosen as Material does: filter chips in rows that wrap, the chosen one ticked. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
public fun OptionPicker(label: String, selection: String, onChange: (String) -> Unit, content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalPicking provides (selection to onChange)) {
        FlowRow(
            Modifier.semantics { contentDescription = label },
            horizontalArrangement = Arrangement.spacedBy(Space.s),
        ) { content() }
    }
}

/** One choice of an `OptionPicker`: a filter chip, its glyph before its title while chosen. */
@Composable
public fun PickerChoice(title: String, glyph: String, tag: String) {
    val (selection, onChange) = LocalPicking.current
    Chip(title, glyph = glyph, selected = selection == tag, onTap = { onChange(tag) })
}

/** An amount as Android takes it: typed in a text field; the core says what it means. */
@Composable
public fun AmountEntry(
    label: String,
    prompt: String,
    text: String,
    cents: Long,
    problem: String?,
    onType: (String) -> Unit,
    onCents: (Long) -> Unit,
) {
    AmountField(label, text = text, problem = problem, onChange = onType)
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
public fun SwitchRow(title: String, note: String? = null, isOn: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = note?.let { n -> { Text(n) } },
        trailingContent = { Switch(checked = isOn, onCheckedChange = onChange) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
    )
}

/** A form's button, the width of the form. */
@Composable
public fun FormButton(title: String, enabled: Boolean, onTap: () -> Unit) {
    Button(onClick = onTap, enabled = enabled, modifier = Modifier.fillMaxWidth()) { Text(title) }
}


/** The tint a view names (`ark_app::view`'s words); anything else neutral. */
public fun tintNamed(name: String): Tint = when (name) {
    "positive" -> Tint.Positive
    "caution" -> Tint.Caution
    "negative" -> Tint.Negative
    "accent" -> Tint.Accent
    else -> Tint.Neutral
}

/**
 * An inspector's or a detail's page: its cards one under another, scrolling, at the page's margin
 * and a section's gap apart, as a tab's are ([TabPage]).
 */
@Composable
public fun DetailPage(content: @Composable () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = Space.page, vertical = Space.m),
        verticalArrangement = Arrangement.spacedBy(Space.section),
    ) { content() }
}

