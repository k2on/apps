// harken's look on the phone: the desktop's palette (`harken/iced/src/palette.rs`)
// — AppKit's neutral greys and the gold — as a Material 3 colour scheme in
// light and dark, and one type scale. The spacings are `ark-ui`'s (`Space`),
// which is drawn in these colours.
//
// **The only file in this app that writes a colour down.** Every screen asks
// `MaterialTheme.colorScheme` or [Harken], which is what makes the dark
// theme work: a colour chosen against one background is wrong on the other.
//
// The gold is not one colour, for the desktop's reason: a dark page can take
// a bright leaf gold, and a white one cannot, because text drawn *on* the
// gold has to be legible. The greys are AppKit's named system colours,
// flattened over the plane they are drawn on — `controlBackgroundColor` for
// the page, `underPageBackgroundColor` and `windowBackgroundColor` for the
// two raised planes, the three ranks of `labelColor` for text — so each can
// be checked against Apple rather than against taste.
package dev.harken.android

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import dev.harken.android.ui.ArkTheme
import dev.harken.android.ui.LocalArkTheme
import dev.harken.android.ui.LocalMoreGlyph

/** The palette, both sides. */
object Palette {
    // The gold: `controlAccentColor`'s slot.
    val GoldDark = Color(0xFFE9BB45)
    val GoldLight = Color(0xFF9A741A)

    val DarkScheme: ColorScheme =
        darkColorScheme(
            primary = GoldDark,
            onPrimary = Color(0xFF1E1E1E),
            primaryContainer = Color(0xFF514528),
            onPrimaryContainer = Color(0xFFF3D99A),
            secondary = GoldDark,
            onSecondary = Color(0xFF1E1E1E),
            secondaryContainer = Color(0xFF514528),
            onSecondaryContainer = Color(0xFFF3D99A),
            tertiary = GoldDark,
            onTertiary = Color(0xFF1E1E1E),
            background = Color(0xFF1E1E1E),
            onBackground = Color(0xFFDDDDDD),
            surface = Color(0xFF1E1E1E),
            onSurface = Color(0xFFDDDDDD),
            surfaceVariant = Color(0xFF2C2C2C),
            onSurfaceVariant = Color(0xFF9A9A9A),
            surfaceTint = Color(0xFF1E1E1E),
            surfaceContainerLowest = Color(0xFF181818),
            surfaceContainerLow = Color(0xFF232323),
            surfaceContainer = Color(0xFF282828),
            surfaceContainerHigh = Color(0xFF323232),
            surfaceContainerHighest = Color(0xFF3A3A3A),
            surfaceBright = Color(0xFF3A3A3A),
            surfaceDim = Color(0xFF1E1E1E),
            inverseSurface = Color(0xFFDDDDDD),
            inverseOnSurface = Color(0xFF262626),
            inversePrimary = GoldLight,
            outline = Color(0xFF565656),
            outlineVariant = Color(0xFF343434),
            error = Color(0xFFFF453A),
            onError = Color(0xFF1E1E1E),
            errorContainer = Color(0xFF5C1F1B),
            onErrorContainer = Color(0xFFFFB4AB),
            scrim = Color(0xFF000000),
        )

    val LightScheme: ColorScheme =
        lightColorScheme(
            primary = GoldLight,
            onPrimary = Color(0xFFFFFFFF),
            primaryContainer = Color(0xFFF0E5C8),
            onPrimaryContainer = Color(0xFF5C4510),
            secondary = GoldLight,
            onSecondary = Color(0xFFFFFFFF),
            secondaryContainer = Color(0xFFF0E5C8),
            onSecondaryContainer = Color(0xFF5C4510),
            tertiary = GoldLight,
            onTertiary = Color(0xFFFFFFFF),
            background = Color(0xFFFFFFFF),
            onBackground = Color(0xFF262626),
            surface = Color(0xFFFFFFFF),
            onSurface = Color(0xFF262626),
            surfaceVariant = Color(0xFFF0F0F0),
            // `secondaryLabelColor` is black at half: #808080, a shade too light for small
            // text on white, so a shade darker here, at 4.7:1.
            onSurfaceVariant = Color(0xFF737373),
            surfaceTint = Color(0xFFFFFFFF),
            surfaceContainerLowest = Color(0xFFFFFFFF),
            surfaceContainerLow = Color(0xFFF9F9F9),
            surfaceContainer = Color(0xFFF5F5F5),
            surfaceContainerHigh = Color(0xFFECECEC),
            surfaceContainerHighest = Color(0xFFE3E3E3),
            surfaceBright = Color(0xFFFFFFFF),
            surfaceDim = Color(0xFFE3E3E3),
            inverseSurface = Color(0xFF262626),
            inverseOnSurface = Color(0xFFF5F5F5),
            inversePrimary = GoldDark,
            outline = Color(0xFFBDBDBD),
            outlineVariant = Color(0xFFE6E6E6),
            error = Color(0xFFD70015),
            onError = Color(0xFFFFFFFF),
            errorContainer = Color(0xFFFFDAD6),
            onErrorContainer = Color(0xFF410002),
            scrim = Color(0xFF000000),
        )

    /**
     * The derived square's gradients — the desktop's `DARK_ART` and `LIGHT_ART` — as their two
     * stops, handed to `ark-ui` as its `ArkTheme`. Which one a name picks is `Art.hash`, FNV-1a
     * over UTF-16, so the phone and the desktop draw one album as one square.
     */
    val DarkArt: List<Pair<Color, Color>> =
        listOf(
            Color(0xFF7A5C15) to Color(0xFF201907),
            Color(0xFF8A6B22) to Color(0xFF1B1509),
            Color(0xFF6B5A2A) to Color(0xFF17140B),
            Color(0xFF8F7327) to Color(0xFF221B09),
            Color(0xFF5E4A18) to Color(0xFF141007),
            Color(0xFFA08137) to Color(0xFF261E08),
        )

    val LightArt: List<Pair<Color, Color>> =
        listOf(
            Color(0xFFE8D19A) to Color(0xFFC9A85F),
            Color(0xFFEFDCAE) to Color(0xFFD2B370),
            Color(0xFFE2D2AC) to Color(0xFFBFA671),
            Color(0xFFF0DFA8) to Color(0xFFCBAA5C),
            Color(0xFFE6D0A0) to Color(0xFFC3A05A),
            Color(0xFFF3E6BE) to Color(0xFFD6B877),
        )

    /** What a glyph is drawn in before `Icon` tints it: a placeholder, never seen. */
    val Ink = Color(0xFF000000)

    /** The scrim under the full player's cover, and over a card's caption. */
    val Shade = Color(0x99000000)
}

/** One type scale for every screen: the platform's sans, in a few sizes and two weights. */
val HarkenType: Typography =
    Typography().run {
        copy(
            displaySmall = displaySmall.copy(fontWeight = FontWeight.Bold),
            headlineLarge = headlineLarge.copy(fontWeight = FontWeight.Bold),
            headlineMedium = headlineMedium.copy(fontWeight = FontWeight.Bold, fontSize = 26.sp, lineHeight = 32.sp),
            headlineSmall = headlineSmall.copy(fontWeight = FontWeight.Bold, fontSize = 22.sp, lineHeight = 28.sp),
            titleLarge = titleLarge.copy(fontWeight = FontWeight.SemiBold, fontSize = 20.sp),
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
