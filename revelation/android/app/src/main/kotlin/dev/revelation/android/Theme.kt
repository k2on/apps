// The two looks: white on near-black, the owner's image and the default on
// a phone in dark mode, and its inverse for a phone in light mode. The
// surface (BibleView) and the chrome (Compose) draw from the same palette.
package dev.revelation.android

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.sp

/** The ground the text is on and the ink it is in, as ARGB, for the view's Paints and Compose's colours alike. */
data class Palette(val ground: Int, val ink: Int, val dark: Boolean) {
    companion object {
        /** Near-black, and white: the far end's line is pure white on it. */
        val Dark = Palette(ground = 0xFF0B0B0C.toInt(), ink = 0xFFFFFFFF.toInt(), dark = true)

        /** Its inverse, a little warm, and the line is near-black. */
        val Light = Palette(ground = 0xFFF7F6F2.toInt(), ink = 0xFF111111.toInt(), dark = false)
    }
}

val LocalPalette = staticCompositionLocalOf { Palette.Dark }

@Composable
fun RevelationTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val palette = if (dark) Palette.Dark else Palette.Light
    val ground = Color(palette.ground)
    val ink = Color(palette.ink)
    val dim = ink.copy(alpha = 0.6f)
    val raised = if (dark) Color(0xFF17171A) else Color(0xFFEDEBE5)
    val scheme =
        if (dark) {
            darkColorScheme(
                background = ground, onBackground = ink, surface = ground, onSurface = ink,
                surfaceVariant = raised, onSurfaceVariant = dim, surfaceContainerHigh = raised,
                primary = ink, onPrimary = ground, outline = ink.copy(alpha = 0.2f),
            )
        } else {
            lightColorScheme(
                background = ground, onBackground = ink, surface = ground, onSurface = ink,
                surfaceVariant = raised, onSurfaceVariant = dim, surfaceContainerHigh = raised,
                primary = ink, onPrimary = ground, outline = ink.copy(alpha = 0.2f),
            )
        }
    val serif = TextStyle(fontFamily = FontFamily.Serif)
    val type = Typography(
        titleMedium = serif.copy(fontSize = 18.sp),
        titleLarge = serif.copy(fontSize = 22.sp),
        bodyLarge = serif.copy(fontSize = 17.sp, lineHeight = 24.sp),
        bodyMedium = serif.copy(fontSize = 15.sp, lineHeight = 21.sp),
        labelLarge = serif.copy(fontSize = 15.sp),
        labelMedium = TextStyle(fontSize = 12.sp),
    )
    CompositionLocalProvider(LocalPalette provides palette) {
        MaterialTheme(colorScheme = scheme, typography = type, content = content)
    }
}
