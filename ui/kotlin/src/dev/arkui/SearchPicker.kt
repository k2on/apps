// One of many, found by typing (`ark_app::view::search_picker`, ui/docs/parts.md "SearchPicker"):
// a form's row saying what is chosen, and pressed, the searchable list — a search field at its top
// with the keyboard, the choices found under it by their groups, the chosen one checked. Compact (a
// phone, narrower than `compact-width`, where SwiftUI's size class is compact) it is a bottom sheet
// under its title and a close button, as the iPhone's is a sheet; wider (a tablet, an unfolded
// Fold) it is a panel anchored to the row, as the iPad's and the Mac's popover is.
//
// What is found is `ark_app::view::search`'s rule, in Java's words: folded (lower case, the marks
// NFD splits off taken away), every word of what is typed in the title or the group's name, in the
// app's order under the groups.
//
// The choices are data, not rows: the slot is a builder (`SearchChoices`), whose `SearchChoice`
// adds one, so the generated `for` and `if` fill it as they fill any other part's slot.
//
// Motion: the sheet is Material's `ModalBottomSheet`, rising from its edge (motion.md, "Sheet"); the
// panel is Material's `DropdownMenu`, growing out of the row and back into it ("Menu, popover");
// nothing inside either moves — the keys move the lit row at once. The
// row and each choice light under the finger (`Press.Highlight`); a choice made ticks
// (`SensoryFeedback(Feedback.Selection)`), on the change itself.
package dev.arkui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.CollectionInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.collectionInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.PopupProperties
import java.text.Normalizer

/** One choice as the picker reads it. */
public data class SearchChoiceValue(
    val title: String,
    val glyph: String,
    val tag: String,
    val group: String,
    val detail: String,
    val tint: String,
)

/** The choices found under one group. */
public data class SearchGroup(val name: String, val choices: List<SearchChoiceValue>)

/** A search picker's slot: each [SearchChoice] adds a choice, in order. */
public class SearchChoices internal constructor() {
    internal val all: MutableList<SearchChoiceValue> = mutableListOf()

    /**
     * One choice: its title, its glyph (a Lucide name), the tag
     * choosing it sends, the group it is listed under, and words at its end in a tint.
     */
    @Suppress("FunctionName")
    public fun SearchChoice(title: String, glyph: String, tag: String, group: String = "", detail: String = "", tint: String = "neutral") {
        all += SearchChoiceValue(title, glyph, tag, group, detail, tint)
    }
}

/** What a search picker finds: `ark_app::view::search`'s rule. */
public object SearchFinding {
    private val marks = Regex("\\p{Mn}+")

    /** [s] as it is matched: lower case, its accents taken off. */
    public fun fold(s: String): String = Normalizer.normalize(s.lowercase(), Normalizer.Form.NFD).replace(marks, "")

    /** Whether every word of [query] is in [c]'s title or group. */
    public fun matches(query: String, c: SearchChoiceValue): Boolean {
        val hay = fold("${c.title} ${c.group}")
        return fold(query).split(Regex("\\s+")).filter { it.isNotEmpty() }.all { hay.contains(it) }
    }

    /** What [query] finds of [choices], under their groups in the order given. */
    public fun found(choices: List<SearchChoiceValue>, query: String): List<SearchGroup> {
        val groups = linkedMapOf<String, MutableList<SearchChoiceValue>>()
        for (c in choices) if (matches(query, c)) groups.getOrPut(c.group) { mutableListOf() } += c
        return groups.map { (name, cs) -> SearchGroup(name, cs) }
    }
}

/**
 * One of many, found by typing: the row says [label] and the chosen one's title (or [unset] after a
 * question mark, in the caution tint, while none is); pressed, the dropdown, its search field ([prompt])
 * with the keyboard. Choosing sends [onChange] with its tag. TalkBack hears a button with its label
 * and value; the list as a list, the chosen one selected; the search field by its prompt.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun SearchPicker(
    label: String,
    selection: String,
    onChange: (String) -> Unit,
    unset: String = "",
    prompt: String = "Search",
    content: SearchChoices.() -> Unit,
) {
    val choices = SearchChoices().apply(content).all
    val chosen = choices.firstOrNull { it.tag == selection }
    var open by remember { mutableStateOf(false) }
    SensoryFeedback(Feedback.Selection, selection)
    Box {
        ListItem(
            headlineContent = { Text(label) },
            trailingContent = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val tone = if (chosen == null) tintInk(Tint.Caution) else ArkInk.quiet
                    ArkGlyph(chosen?.glyph ?: "circle-help", ArkType.Body, tone)
                    Spacer(Modifier.width(Space.s))
                    Text(
                        chosen?.title ?: unset,
                        color = tone,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    Spacer(Modifier.width(Space.xs))
                    ArkGlyph("chevrons-up-down", ArkType.Footnote, ArkInk.quiet)
                }
            },
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            modifier =
                Modifier
                    .pressable(onClick = { open = true }, role = Role.Button, press = Press.Highlight)
                    .semantics(mergeDescendants = true) {
                        contentDescription = label
                        stateDescription = chosen?.title ?: unset
                    },
        )
        val choose = { tag: String ->
            open = false
            if (tag != selection) onChange(tag)
        }
        if (currentSizeClass() == SizeClass.Compact) {
            if (open) {
                ModalBottomSheet(onDismissRequest = { open = false }, sheetState = rememberModalBottomSheetState()) {
                    SheetBar(label, onClose = { open = false })
                    SearchDropdown(label, prompt, choices, selection, sheet = true, choose = choose, close = { open = false })
                }
            }
        } else {
            DropdownMenu(
                expanded = open,
                onDismissRequest = { open = false },
                // Focusable, so its search field takes the keyboard.
                properties = PopupProperties(focusable = true),
            ) {
                SearchDropdown(label, prompt, choices, selection, sheet = false, choose = choose, close = { open = false })
            }
        }
    }
}

@Composable
private fun SearchDropdown(
    label: String,
    prompt: String,
    choices: List<SearchChoiceValue>,
    selection: String,
    sheet: Boolean,
    choose: (String) -> Unit,
    close: () -> Unit,
) {
    var query by remember { mutableStateOf("") }
    var lit by remember { mutableStateOf(if (choices.any { it.tag == selection }) selection else choices.firstOrNull()?.tag) }
    val groups = remember(choices, query) { SearchFinding.found(choices, query) }
    val listed = groups.flatMap { it.choices }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    fun step(by: Int) {
        if (listed.isEmpty()) return
        val here = listed.indexOfFirst { it.tag == lit }
        lit = listed[if (here < 0) 0 else (here + by).coerceIn(0, listed.lastIndex)].tag
    }
    // The sheet as wide as the window and its list as tall as the sheet lets it; the panel the
    // dropdown's measures.
    val frame = if (sheet) Modifier.fillMaxWidth().navigationBarsPadding().imePadding() else Modifier.width(LayoutTokens.DROPDOWN_WIDTH.dp)
    Column(frame) {
        SearchField(
            query = query,
            onQuery = {
                query = it
                lit = SearchFinding.found(choices, it).firstOrNull()?.choices?.firstOrNull()?.tag
            },
            placeholder = prompt,
            onSearch = { lit?.takeIf { t -> listed.any { it.tag == t } }?.let(choose) },
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = Space.s, vertical = Space.xs)
                    .focusRequester(focus)
                    .semantics { contentDescription = prompt }
                    .onPreviewKeyEvent { e ->
                        if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                        when (e.key) {
                            Key.DirectionDown -> step(1)
                            Key.DirectionUp -> step(-1)
                            Key.Escape -> close()
                            Key.Enter, Key.NumPadEnter -> lit?.takeIf { t -> listed.any { it.tag == t } }?.let(choose)
                            else -> return@onPreviewKeyEvent false
                        }
                        true
                    },
        )
        HorizontalDivider()
        if (groups.isEmpty()) {
            Text(
                "No Matches",
                style = ArkType.Body.style,
                color = ArkInk.quiet,
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = LayoutTokens.ROW_HEIGHT.dp)
                        .padding(horizontal = Space.l, vertical = Space.m)
                        .semantics { liveRegion = LiveRegionMode.Polite },
            )
        } else {
            Column(
                (if (sheet) Modifier.weight(1f, fill = false) else Modifier.heightIn(max = LayoutTokens.DROPDOWN_HEIGHT.dp))
                    .verticalScroll(rememberScrollState())
                    .padding(vertical = Space.s)
                    .semantics {
                        contentDescription = label
                        collectionInfo = CollectionInfo(rowCount = listed.size, columnCount = 1)
                    },
            ) {
                for (g in groups) {
                    if (g.name.isNotEmpty()) {
                        Text(
                            g.name,
                            style = ArkType.Footnote.style,
                            color = ArkInk.quiet,
                            modifier = Modifier.padding(start = Space.l, end = Space.l, top = Space.s, bottom = Space.xs).semantics { heading() },
                        )
                    }
                    for (c in g.choices) SearchLine(c, chosen = c.tag == selection, lit = c.tag == lit, choose = choose)
                }
            }
        }
    }
}

@Composable
private fun SearchLine(c: SearchChoiceValue, chosen: Boolean, lit: Boolean, choose: (String) -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier =
            Modifier
                .fillMaxWidth()
                .heightIn(min = LayoutTokens.ROW_HEIGHT.dp)
                .background(if (lit) MaterialTheme.colorScheme.surfaceContainerHigh else Color.Transparent)
                .pressable(onClick = { choose(c.tag) }, role = Role.Button, press = Press.Highlight)
                .semantics(mergeDescendants = true) { selected = chosen }
                .padding(horizontal = Space.l),
    ) {
        ArkGlyph(c.glyph, ArkType.Body, ArkInk.quiet)
        Spacer(Modifier.width(Space.m))
        Text(c.title, style = ArkType.Body.style, color = ArkInk.ink, modifier = Modifier.weight(1f))
        if (c.detail.isNotEmpty()) {
            Spacer(Modifier.width(Space.s))
            Text(c.detail, style = ArkType.Subheadline.style.copy(fontFeatureSettings = "tnum"), color = ArkInk.tint(c.tint))
        }
        Spacer(Modifier.width(Space.s))
        ArkGlyph("check", ArkType.Footnote, ArkInk.accent, Modifier.alpha(if (chosen) 1f else 0f))
    }
}
