// The tokens every screen is laid out by: one set of spacings, the corner a
// cover and a card have, and the little an app has to say that Material's
// scheme has no slot for — the gradients its derived squares are drawn in
// (`ArkTheme`). Colours are the app's: everything here asks
// `MaterialTheme.colorScheme`, so an app's light and dark schemes are what
// it is drawn in, and this library writes no colour of its own but the
// neutral squares an app that names none gets — `rust/arkui`'s
// `Palette::NEUTRAL`, the same three pairs.
package dev.arkdb.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** The spacings, so that every screen has the same rhythm. */
public object Space {
    public val xs: Dp = 4.dp
    public val s: Dp = 8.dp
    public val m: Dp = 12.dp
    public val l: Dp = 16.dp
    public val xl: Dp = 24.dp
    public val xxl: Dp = 32.dp

    /** Where a page's content starts from its edges. */
    public val page: Dp = 16.dp

    /** A cover on a shelf, and a card in a grid at its narrowest. */
    public val shelf: Dp = 148.dp

    /** A cover beside a row. */
    public val thumb: Dp = 48.dp

    /** A page's header cover. */
    public val header: Dp = 200.dp

    /** Corners: a cover, a card, a sheet's row. */
    public val corner: Dp = 8.dp
}

/**
 * What an app says that a Material scheme has no slot for: the gradients its derived squares are
 * drawn in ([Square]), as each one's two stops, for the side it is drawn on. Which one a name picks
 * is [Art.pick]. Equal by what it holds, so an app's theme made again on a recomposition is not a
 * change to every screen under it.
 */
@Immutable
public data class ArkTheme(public val art: List<Pair<Color, Color>> = NeutralLight) {
    public companion object {
        /** `rust/arkui`'s neutral squares against white. */
        public val NeutralLight: List<Pair<Color, Color>> =
            listOf(
                Color(0xFFE4E4E6) to Color(0xFFBCBCC0),
                Color(0xFFDDE2EA) to Color(0xFFAEB8C6),
                Color(0xFFE6E1DC) to Color(0xFFC0B6AC),
            )

        /** …and against a dark page. */
        public val NeutralDark: List<Pair<Color, Color>> =
            listOf(
                Color(0xFF5A5A5E) to Color(0xFF18181A),
                Color(0xFF4E5866) to Color(0xFF14171B),
                Color(0xFF605850) to Color(0xFF1A1714),
            )
    }
}

/** The app's [ArkTheme]; the neutral one where it names none. */
public val LocalArkTheme: ProvidableCompositionLocal<ArkTheme> = staticCompositionLocalOf {
    ArkTheme()
}

/** A text style one step quieter than `base`: the same size, the secondary colour. */
@Composable
public fun quiet(base: TextStyle): TextStyle =
    base.copy(color = MaterialTheme.colorScheme.onSurfaceVariant)
