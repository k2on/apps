// The tokens every screen is laid out by: one set of spacings, the corner a
// cover and a card have, and the little an app has to say that Material's
// scheme has no slot for — the gradients its derived squares are drawn in
// (`ArkTheme`). Colours are the theme's (ui/theme.toml, generated into
// Tokens.kt): an app is drawn in `arkuiScheme(accent, dark)`, everything here
// asks `MaterialTheme.colorScheme`, and this library writes no colour of
// its own.
package dev.arkui

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
        /** The neutral squares against white: ui/theme.toml's, as `Accent.Blue` draws them. */
        public val NeutralLight: List<Pair<Color, Color>> = Accent.Blue.art(dark = false)

        /** …and against a dark page. */
        public val NeutralDark: List<Pair<Color, Color>> = Accent.Blue.art(dark = true)
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
