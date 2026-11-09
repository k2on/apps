// The small generic pieces a page is laid out of, the same names and the
// same inputs in every toolkit (SwiftUI's `Parts.swift`, iced's
// `arkui::parts`; ui/docs/parts.md lists them): a page's header about one
// thing, a section's heading, rows in a rounded card with hairlines between
// them, a notice across the page, a choice among a few, a count, a glyph on
// a washed square, the field a search is typed in and the one a name is,
// a pager stepped back and forth, and a scrubber. Nothing here names a
// thing any app is about; every colour asks the theme.
package dev.arkui

import androidx.compose.animation.AnimatedContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** A card's corners, as the iPhone's grouped lists round theirs. */
public val CardCorner: Dp = 20.dp

/**
 * The header of a page that is about one thing: its [cover], what kind of thing it is, its name,
 * who it is by (a link in the accent when [onBy] goes somewhere), the facts true of the whole of it,
 * and the page's own [actions] under them all. The cover is where a page that grew out of a cover
 * lands ([LocalPageFrame]).
 */
@Composable
public fun PageHeader(
    kind: String,
    title: String,
    facts: String,
    by: String? = null,
    onBy: (() -> Unit)? = null,
    cover: @Composable () -> Unit,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Column(Modifier.fillMaxWidth().padding(horizontal = Space.page), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(Space.s))
        // Where the art a page grew from lands.
        Box(LocalPageFrame.current?.art?.modifier ?: Modifier) { cover() }
        Spacer(Modifier.height(Space.l))
        Text(kind, style = quiet(MaterialTheme.typography.labelMedium))
        Spacer(Modifier.height(Space.xs))
        Text(title, style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
        if (by != null) {
            Text(
                by,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary,
                textAlign = TextAlign.Center,
                modifier = if (onBy != null) Modifier.pressable(onClick = onBy).padding(Space.xs) else Modifier.padding(Space.xs),
            )
        }
        Text(facts, style = quiet(MaterialTheme.typography.bodySmall), textAlign = TextAlign.Center)
        Spacer(Modifier.height(Space.l))
        Row(horizontalArrangement = Arrangement.spacedBy(Space.m), content = actions)
        Spacer(Modifier.height(Space.m))
    }
}

/**
 * A section's heading over its card, as a grouped list sets one: its name, quiet, read by TalkBack
 * as a heading, and what it adds up to — or an action — at its end ([trailing]).
 */
@Composable
public fun SectionHeader(title: String, modifier: Modifier = Modifier, trailing: @Composable RowScope.() -> Unit = {}) {
    Row(
        modifier.fillMaxWidth().padding(start = Space.l, end = Space.l, top = Space.xl, bottom = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).semantics { heading() },
        )
        trailing()
    }
}

/** Whether what is composed is a row of a [GroupCard]: a [ListRow] there is drawn as a grouped list's row. */
public val LocalInGroup: ProvidableCompositionLocal<Boolean> = staticCompositionLocalOf { false }

/** Rows in a rounded card, as a grouped list draws a section; a [RowDivider] between each. */
@Composable
public fun GroupCard(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainerLow, shape = RoundedCornerShape(CardCorner), modifier = modifier.fillMaxWidth()) {
        CompositionLocalProvider(LocalInGroup provides true) { Column(content = content) }
    }
}

/** The hairline between two rows of a [GroupCard], from where the text starts. */
@Composable
public fun RowDivider(inset: Dp = Space.l) {
    HorizontalDivider(Modifier.padding(start = inset), thickness = 0.5.dp, color = MaterialTheme.colorScheme.outlineVariant)
}

/** A [ListRow] in a [GroupCard]: its leading picture's box, and the row's height inside its margins. */
public object GroupRow {
    public val LEAD: Dp = 32.dp
}

/** [ListRow] as a [GroupCard]'s row: the card's inset, the body's type, the picture in a [GroupRow.LEAD] box. */
@Composable
internal fun GroupListRow(onClick: () -> Unit, title: String, under: String?, accent: Boolean, trailing: @Composable () -> Unit, picture: @Composable () -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .pressable(onClick = onClick, press = Press.Highlight)
            .padding(horizontal = Space.l, vertical = Space.m)
            .then(if (under == null) Modifier.height(GroupRow.LEAD) else Modifier.heightIn(min = GroupRow.LEAD)),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(GroupRow.LEAD), contentAlignment = Alignment.Center) { picture() }
        Spacer(Modifier.width(Space.m))
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MaterialTheme.typography.bodyLarge,
                color = if (accent) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (under != null) Text(under, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 1)
        }
        trailing()
    }
}

/**
 * A notice across the page — the server cannot be reached, say: a quiet [icon] beside a sentence,
 * on a raised container. It says; it does not ask.
 */
@Composable
public fun Banner(icon: ImageVector, text: String, modifier: Modifier = Modifier) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = RoundedCornerShape(Space.corner),
        modifier = modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s),
    ) {
        Row(Modifier.padding(Space.m), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(Space.m))
            Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** One of a few to choose from, in a capsule: the chosen one filled in the accent, the rest raised. */
@Composable
public fun Chip(label: String, selected: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(label) },
        shape = CircleShape,
        colors =
            FilterChipDefaults.filterChipColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                labelColor = MaterialTheme.colorScheme.onSurface,
                selectedContainerColor = MaterialTheme.colorScheme.primary,
                selectedLabelColor = MaterialTheme.colorScheme.onPrimary,
            ),
        border = FilterChipDefaults.filterChipBorder(enabled = true, selected = selected, borderColor = Color.Transparent, selectedBorderColor = Color.Transparent),
        modifier = modifier,
    )
}

/** A count in the accent's capsule, as a row's "3 to review" is; TalkBack says [said]. */
@Composable
public fun Badge(count: Int, said: String) {
    Box(
        Modifier.clip(RoundedCornerShape(50)).background(MaterialTheme.colorScheme.primary).padding(horizontal = 7.dp, vertical = 2.dp)
            .semantics { contentDescription = said }) {
            Text("$count", style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.onPrimary)
        }
}

/** A glyph in [color] on a rounded square washed in it, [side] across. */
@Composable
public fun Tile(icon: ImageVector, color: Color, side: Dp = GroupRow.LEAD) {
    Box(
        Modifier.size(side).clip(RoundedCornerShape(side * 0.26f)).background(color.copy(alpha = 0.16f)),
        contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = null, tint = color, modifier = Modifier.size(side * 0.56f))
        }
}

/**
 * The field a search is typed in, as a row of the page: a filled capsule as the iPhone's
 * (`SearchField`) and the system's own field in iOS 26 are, the glass at its start, [placeholder]
 * while it is empty, a way to clear it once it is not, and the keyboard's Search key saying
 * [onSearch].
 */
@Composable
public fun SearchField(query: String, onQuery: (String) -> Unit, placeholder: String, modifier: Modifier = Modifier, onSearch: () -> Unit = {}) {
    TextField(
        value = query,
        onValueChange = onQuery,
        placeholder = { Text(placeholder, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
        trailingIcon = {
            if (query.isNotEmpty()) {
                IconButton(onClick = { onQuery("") }) { Icon(Icons.Default.Clear, contentDescription = "Clear the search") }
            }
        },
        singleLine = true,
        shape = CircleShape,
        colors =
            TextFieldDefaults.colors(
                focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
            ),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = { onSearch() }),
        modifier = modifier,
    )
}

/**
 * The field a new thing's name is typed in: [label] over it, and under it the reason the name will
 * not do ([problem], once something is typed) or else [hint].
 */
@Composable
public fun NameField(name: String, onName: (String) -> Unit, problem: String? = null, hint: String = "", label: String = "Name") {
    Column {
        val wrong = problem != null && name.isNotEmpty()
        OutlinedTextField(
            value = name,
            onValueChange = onName,
            label = { Text(label) },
            singleLine = true,
            isError = wrong,
            supportingText = { Text(if (wrong) problem.orEmpty().replaceFirstChar { it.uppercase() } else hint) },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(Space.xs))
    }
}

/**
 * A title stepped through something ordered — a month, a week — with a button either side, and,
 * when it is elsewhere, a way back to where it started ([home], said as [homeLabel]). The title
 * slides the way it went ([order] says which way: higher is forward), with a tick each step.
 * [backLabel] and [forwardLabel] are what TalkBack says of the buttons.
 */
@Composable
public fun Pager(
    title: String,
    back: () -> Unit,
    forward: () -> Unit,
    modifier: Modifier = Modifier,
    order: Int = 0,
    home: (() -> Unit)? = null,
    homeLabel: String = "Today",
    backLabel: String = "Previous",
    forwardLabel: String = "Next",
) {
    // A tick each step, as the iPhone's pager gives one (`.sensoryFeedback(.selection)`).
    SensoryFeedback(Feedback.Selection, order)
    val reduced = reducedMotion()
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        FilledTonalIconButton(onClick = back) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = backLabel)
        }
        AnimatedContent(
            targetState = title to order,
            transitionSpec = {
                // The new title comes in from the side it was stepped toward, on arkui's
                // critically damped springs; with motion reduced, it only cross-fades.
                Transitions.stepped(forward = targetState.second >= initialState.second, reduced = reduced)
            },
            label = "pager",
            modifier = Modifier.weight(1f, fill = false).widthIn(min = 160.dp),
        ) { (text, _) ->
            Text(
                text,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                modifier = Modifier.padding(horizontal = Space.s).semantics { heading() },
                textAlign = TextAlign.Center,
            )
        }
        FilledTonalIconButton(onClick = forward) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = forwardLabel)
        }
        if (home != null) {
            Spacer(Modifier.width(Space.s))
            FilledTonalButton(onClick = home) { Text(homeLabel, style = MaterialTheme.typography.labelLarge) }
        }
    }
}

/**
 * A position through something that runs, which a finger drags: the [Bar] with no handle, and under
 * it what has gone ([left]) and what is left ([right]), written by the caller from the bar's value.
 * Under the finger it is where the finger is, saying [onValue] as it goes; let go, [onDone] says
 * where; otherwise it is at [value].
 */
@Composable
public fun Scrubber(
    value: Float,
    onValue: (Float) -> Unit,
    onDone: (Float) -> Unit,
    left: (Float) -> String,
    right: (Float) -> String,
    enabled: Boolean = true,
    label: String = "Position",
) {
    var dragging by remember { mutableStateOf<Float?>(null) }
    val at = dragging ?: value.coerceIn(0f, 1f)
    Bar(
        value = at,
        onValue = {
            dragging = it
            onValue(it)
        },
        onDone = {
            dragging?.let(onDone)
            dragging = null
        },
        enabled = enabled,
        label = label,
    )
    Row(Modifier.fillMaxWidth()) {
        Text(left(at), style = quiet(MaterialTheme.typography.labelMedium), modifier = Modifier.weight(1f))
        Text(right(at), style = quiet(MaterialTheme.typography.labelMedium))
    }
}
