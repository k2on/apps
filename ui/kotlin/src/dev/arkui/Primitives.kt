// What the generator's primitives (ui/docs/primitives.md) are written with:
// a text style by name, the theme's inks, surfaces and corners, a glyph
// sized as its words, and the leaves a stack holds — a glyph's tile, a
// status badge, a meter — called with what a recorded view hands them. The
// columns, rows, spacers and text themselves are Compose's own.
package dev.arkui

import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** The face words are set in: Material's type roles, so the font scale grows them (`ark_app::view::TextStyle`; SwiftUI's `ArkType`). */
public enum class ArkType {
    LargeTitle,
    Title,
    Headline,
    Body,
    Subheadline,
    Footnote,
    Caption,
    Figure;

    public val style: TextStyle
        @Composable @ReadOnlyComposable
        get() =
            with(MaterialTheme.typography) {
                when (this@ArkType) {
                    LargeTitle -> headlineLarge.copy(fontWeight = FontWeight.Bold)
                    Title -> titleLarge.copy(fontWeight = FontWeight.Bold)
                    Headline -> titleMedium
                    Body -> bodyLarge
                    Subheadline -> bodyMedium
                    Footnote -> bodySmall
                    Caption -> labelSmall
                    // A card's figure: large and bold, its digits in columns.
                    Figure -> displaySmall.copy(fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum")
                }
            }
}

/** The colours words and glyphs are drawn in (`Ink`): the scheme's roles. */
public object ArkInk {
    public val ink: Color
        @Composable @ReadOnlyComposable get() = MaterialTheme.colorScheme.onSurface
    public val quiet: Color
        @Composable @ReadOnlyComposable get() = MaterialTheme.colorScheme.onSurfaceVariant
    public val accent: Color
        @Composable @ReadOnlyComposable get() = MaterialTheme.colorScheme.primary

    /** A tint a view names; neutral is the quiet colour. */
    @Composable
    public fun tint(name: String): Color = tintInk(tintNamed(name))

    /** A figure in a tint a view names; neutral is the words' colour. */
    @Composable
    public fun figure(name: String): Color = tintNamed(name).let { if (it == Tint.Neutral) ink else tintInk(it) }
}

/** What is behind a primitive (`Surface`): the scheme's containers. */
public object ArkSurface {
    public val card: Color
        @Composable @ReadOnlyComposable get() = MaterialTheme.colorScheme.surfaceContainerLow
    public val raised: Color
        @Composable @ReadOnlyComposable get() = MaterialTheme.colorScheme.surfaceContainerHigh
}

/** A primitive's corners (`Corner`): `card-corner` and `corner`. */
public object ArkCorner {
    public val card: Dp = LayoutTokens.CARD_CORNER.dp
    public val small: Dp = LayoutTokens.CORNER.dp
}

/** A glyph by its Lucide name, as large as `type`'s words, in `color`. Decorative: the words beside it say it. */
@Composable
public fun ArkGlyph(name: String, type: ArkType, color: Color, modifier: Modifier = Modifier) {
    val side = with(LocalDensity.current) { type.style.fontSize.toDp() }
    Icon(
        LocalGlyphs.current(name) ?: ArkGlyphs.named(name) ?: ArkGlyphs.Chevron,
        contentDescription = null,
        tint = color,
        modifier = modifier.size(side),
    )
}

/** A glyph by its Lucide name on a tile of the tint a view names. */
@Composable
public fun Tile(glyph: String, tint: String) {
    Tile(LocalGlyphs.current(glyph) ?: ArkGlyphs.named(glyph) ?: ArkGlyphs.Chevron, tintInk(tintNamed(tint)))
}

/** A figure in a badge of the tint a view names. */
@Composable
public fun StatusBadge(figure: String, tint: String) {
    StatusBadge(figure, tintNamed(tint))
}

/** How far, 0 to 1, as a bar in the tint a view names (neutral is the positive's), the width it is given. */
@Composable
public fun MeterBar(fraction: Double, tint: String) {
    val t = tintNamed(tint).let { if (it == Tint.Neutral) Tint.Positive else it }
    ProgressBar(fraction.toFloat(), color = tintInk(t))
}
