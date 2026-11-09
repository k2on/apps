// What a finger on something tappable sees the moment it lands, before it
// lifts: Apple's first rule of a fluid interface is that response starts on
// touch-down, not on release. A cover, a card, a tile gives a little under
// the finger — down to 97% of its size, at once — and springs back
// critically damped when the finger lifts or slides off (sliding off
// cancels the tap, as a button's does). A row in a list does not shrink —
// a full-width row moving reads as the list jumping — it is highlighted
// instead, at once, and the highlight fades as it is let go.
//
// It is an Indication, so it rides the `clickable` that already handles the
// tap, its semantics and its cancelling, and it is made only once a finger
// first touches the thing ([clickable]'s lazily created indication): a row
// entering a list pays nothing for it. The give is drawn, not laid out — a
// scale about the middle of what is drawn — so the press measures nothing.
//
// With motion reduced, nothing shrinks: what would shrink is highlighted.
package dev.arkui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.material3.LocalContentColor
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.node.CompositionLocalConsumerModifierNode
import androidx.compose.ui.node.DelegatableNode
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.currentValueOf
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.util.lerp
import kotlinx.coroutines.launch

/**
 * How something tappable answers a finger landing on it: [Give], a cover's or a card's shrink, or
 * [Highlight], a row's.
 */
public class Press private constructor(private val scale: Float, private val highlight: Boolean) :
    IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode =
        PressNode(interactionSource, scale, highlight)

    override fun equals(other: Any?): Boolean =
        other is Press && other.scale == scale && other.highlight == highlight

    override fun hashCode(): Int = scale.hashCode() * 31 + highlight.hashCode()

    public companion object {
        /** How small a pressed cover or card is drawn. */
        public const val SCALE: Float = MotionTokens.PRESS

        /** How strong a pressed row's highlight is, of the content colour. */
        public const val HIGHLIGHT: Float = 0.1f

        /** A cover, a card, a tile: it gives under the finger. */
        public val Give: Press = Press(SCALE, highlight = false)

        /** A row: it is highlighted under the finger. */
        public val Highlight: Press = Press(1f, highlight = true)
    }
}

/**
 * Tappable, answering the finger as it lands ([Press]): a cover's or a card's give unless `press`
 * says a row's highlight. Otherwise `clickable`, with no ripple.
 */
public fun Modifier.pressable(
    onClick: () -> Unit,
    enabled: Boolean = true,
    onClickLabel: String? = null,
    role: Role? = null,
    press: Press = Press.Give,
): Modifier =
    clickable(
        interactionSource = null,
        indication = press,
        enabled = enabled,
        onClickLabel = onClickLabel,
        role = role,
        onClick = onClick,
    )

private class PressNode(
    private val source: InteractionSource,
    private val scale: Float,
    private val highlight: Boolean,
) : Modifier.Node(), DrawModifierNode, CompositionLocalConsumerModifierNode {
    /** How pressed it is drawn: 1 the instant a finger lands, back to 0 on a spring as it lifts. */
    private val amount = Animatable(0f)
    private var down = 0

    override fun onAttach() {
        coroutineScope.launch {
            source.interactions.collect { i ->
                when (i) {
                    is PressInteraction.Press -> {
                        down++
                        launch { amount.snapTo(1f) }
                    }
                    is PressInteraction.Release,
                    is PressInteraction.Cancel -> {
                        down = (down - 1).coerceAtLeast(0)
                        if (down == 0)
                            launch {
                                amount.animateTo(0f, Springs.size())
                            }
                    }
                }
                invalidateDraw()
            }
        }
    }

    override fun ContentDrawScope.draw() {
        val a = amount.value
        if (a <= 0f) {
            drawContent()
            return
        }
        val reduced = currentValueOf(LocalReducedMotion) ?: ReducedMotion.isOn()
        val tint = currentValueOf(LocalContentColor)
        // A row's highlight is behind what it says; a cover, with motion reduced, is dimmed over
        // its picture, which would hide anything behind it.
        if (highlight) {
            drawRect(tint, alpha = Press.HIGHLIGHT * a)
            drawContent()
            return
        }
        if (reduced) {
            drawContent()
            drawRect(tint, alpha = Press.HIGHLIGHT * a)
            return
        }
        val s = lerp(1f, scale, a)
        scale(s, s) { this@draw.drawContent() }
    }
}
