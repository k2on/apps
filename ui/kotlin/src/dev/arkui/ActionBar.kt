// What a watch's buttons do, beside what it holds (`Part::ActionBar`, ui/docs/pebble.md):
// Pebble's `ActionBarLayer`, its up, select and down buttons' glyphs at the trailing edge. A phone
// has no such buttons, so here it is a column of glyph buttons there, top, middle and bottom,
// each pressed as a give (`Pressable`) and said by its label. Its twins are SwiftUI's
// ActionBar.swift, iced's and arkui-web's `Part::ActionBar` and ark-pebble's action bar.
package dev.arkui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier

/** Which of a watch's buttons an [ActionItem] is. */
public enum class ActionButton { Up, Select, Down }

/** One button of an action bar: its glyph, what it is said as, and what pressing it does. */
public class ActionItem(
    public val glyph: String,
    public val label: String,
    public val button: ActionButton,
    public val onTap: () -> Unit,
)

/** [content], and a column of its buttons at the trailing edge, top, middle and bottom. */
@Composable
public fun ActionBar(items: List<ActionItem>, content: @Composable () -> Unit) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) { content() }
        Column(
            Modifier.fillMaxHeight(),
            verticalArrangement = Arrangement.SpaceEvenly,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            for (button in ActionButton.values()) {
                val item = items.firstOrNull { it.button == button } ?: continue
                Pressable(item.label, PressKind.Give, onTap = item.onTap) {
                    ArkGlyph(item.glyph, ArkType.Title, ArkInk.ink, Modifier.padding(Space.s))
                }
            }
        }
    }
}
