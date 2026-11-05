// harken's look on the phone: arkui's theme (arkui/theme.toml) — the
// neutrals every Ark app shares and harken's accent, the gold — as the
// Material scheme in light and dark, and one type scale. The spacings are
// `ark-ui`'s (`Space`), which is drawn in these colours.
//
// **No colour is written down here.** Every screen asks
// `MaterialTheme.colorScheme` or [Harken], which is what makes the dark
// theme work: a colour chosen against one background is wrong on the other.
// A colour is changed in arkui/theme.toml, for every app and platform at once.
//
// The gold is not one colour: a dark page can take a bright leaf gold, and a
// white one cannot, because text in the gold has to be legible on it.
package dev.harken.android

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import dev.arkui.Accent
import dev.arkui.ArkTheme
import dev.arkui.LocalArkTheme
import dev.arkui.LocalMoreGlyph
import dev.arkui.arkuiScheme

/** The palette, both sides: arkui's, in the gold. */
object Palette {
    val DarkScheme: ColorScheme = arkuiScheme(Accent.Gold, dark = true)
    val LightScheme: ColorScheme = arkuiScheme(Accent.Gold, dark = false)

    /**
     * The derived square's gradients, as their two stops, handed to `ark-ui` as its `ArkTheme`.
     * Which one a name picks is `Art.hash`, FNV-1a over UTF-16, so the phone and the desktop draw
     * one album as one square.
     */
    val DarkArt: List<Pair<Color, Color>> = Accent.Gold.art(dark = true)
    val LightArt: List<Pair<Color, Color>> = Accent.Gold.art(dark = false)

/** What a glyph is drawn in before `Icon` tints it: a placeholder, never seen. */
    val Ink = Color(0xFF000000)

    /** The scrim under the full player's cover, and over a card's caption. */
    val Shade = Color(0x99000000)

    /** Where a banner's shade has not begun. */
    val Clear = Color(0x00000000)

    /** What is written on a banner's shade, whichever the theme: a picture is not light or dark. */
    val OnShade = Color(0xFFFFFFFF)
}

/**
 * One type scale for every screen: the platform's sans, in a few sizes and two weights, sized as
 * the iPhone's are — a tab's title (headlineLarge) its large title, a section's heading (titleLarge)
 * its title 2, both bold.
 */
val HarkenType: Typography =
    Typography().run {
        copy(
            displaySmall = displaySmall.copy(fontWeight = FontWeight.Bold),
            headlineLarge = headlineLarge.copy(fontWeight = FontWeight.Bold, fontSize = 34.sp, lineHeight = 41.sp),
            headlineMedium = headlineMedium.copy(fontWeight = FontWeight.Bold, fontSize = 26.sp, lineHeight = 32.sp),
            headlineSmall = headlineSmall.copy(fontWeight = FontWeight.Bold, fontSize = 22.sp, lineHeight = 28.sp),
            titleLarge = titleLarge.copy(fontWeight = FontWeight.Bold, fontSize = 22.sp, lineHeight = 28.sp),
            titleMedium = titleMedium.copy(fontWeight = FontWeight.SemiBold),
            titleSmall = titleSmall.copy(fontWeight = FontWeight.SemiBold),
            labelLarge = labelLarge.copy(fontWeight = FontWeight.SemiBold),
        )
    }

/**
 * The theme: the scheme for the side asked (the system's by default), the type, and what `ark-ui`
 * draws that a scheme has no slot for — the squares' gradients, and the one painter every row's
 * menu button shares.
 */
@Composable
fun HarkenTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val extras = ArkTheme(if (dark) Palette.DarkArt else Palette.LightArt)
    CompositionLocalProvider(LocalArkTheme provides extras, LocalMoreGlyph provides rememberVectorPainter(Glyphs.More)) {
        MaterialTheme(colorScheme = if (dark) Palette.DarkScheme else Palette.LightScheme, typography = HarkenType, content = content)
    }
}
