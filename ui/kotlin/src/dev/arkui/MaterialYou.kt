// An app drawn the way the phone's own apps are: Material 3 in the
// wallpaper's colours (dynamic colour, Android 12 and later) or the app's
// own scheme before that, with the expressive shapes — rounder, larger
// corners as things get larger — and a type scale whose headlines carry a
// little more weight. Nothing here names a colour of its own; the app hands
// its fallback schemes in, and everything else asks `MaterialTheme`.
//
// And the small pieces such an app reads with: a value in a tinted pill
// ([Pill]), a bar that fills toward a goal ([ProgressBar]), a figure under its
// label ([Stat]). A section's heading and the pager are in `Parts.kt`.
package dev.arkui

import android.os.Build
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.progressSemantics
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.Typography
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** The expressive shapes: corners that grow with the thing they round. */
public val ExpressiveShapes: Shapes =
    Shapes(
        extraSmall = RoundedCornerShape(8.dp),
        small = RoundedCornerShape(12.dp),
        medium = RoundedCornerShape(16.dp),
        large = RoundedCornerShape(24.dp),
        extraLarge = RoundedCornerShape(32.dp),
    )

/**
 * The type scale, emphasised as the expressive one is: displays and headlines a weight heavier,
 * titles medium. The family is the phone's own.
 */
public fun expressiveTypography(base: Typography = Typography()): Typography =
    base.copy(
        displayLarge = base.displayLarge.copy(fontWeight = FontWeight.Medium),
        displayMedium = base.displayMedium.copy(fontWeight = FontWeight.Medium),
        displaySmall = base.displaySmall.copy(fontWeight = FontWeight.Medium),
        headlineLarge = base.headlineLarge.copy(fontWeight = FontWeight.Medium),
        headlineMedium = base.headlineMedium.copy(fontWeight = FontWeight.Medium),
        headlineSmall = base.headlineSmall.copy(fontWeight = FontWeight.Medium),
        titleLarge = base.titleLarge.copy(fontWeight = FontWeight.Medium),
    )

/**
 * Material You: the wallpaper's scheme where the phone has one (Android 12 and later) and `dynamic`
 * is on, the app's `light` or `dark` otherwise — and always in a preview or a screenshot, which have
 * no wallpaper.
 */
@Composable
public fun MaterialYou(
    light: ColorScheme,
    dark: ColorScheme,
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamic: Boolean = true,
    typography: Typography = expressiveTypography(),
    shapes: Shapes = ExpressiveShapes,
    content: @Composable () -> Unit,
) {
    val context = LocalContext.current
    val wallpaper = dynamic && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && !LocalInspectionMode.current
    val scheme =
        when {
            wallpaper && darkTheme -> dynamicDarkColorScheme(context)
            wallpaper -> dynamicLightColorScheme(context)
            darkTheme -> dark
            else -> light
        }
    MaterialTheme(colorScheme = scheme, typography = typography, shapes = shapes, content = content)
}

/**
 * What a value says about itself: good, nothing to say, worth a look, wrong, or only the app's
 * accent. The same five on every platform (SwiftUI's `Tint`, iced's `arkui::parts::Tint`).
 */
public enum class Tint {
    Positive,
    Neutral,
    Caution,
    Negative,
    Accent,
}

/** A tone's container and the colour on it, from the scheme. */
@Composable
public fun tintColors(tone: Tint): Pair<Color, Color> {
    val c = MaterialTheme.colorScheme
    return when (tone) {
        Tint.Positive -> c.primaryContainer to c.onPrimaryContainer
        Tint.Neutral -> c.surfaceContainerHighest to c.onSurfaceVariant
        Tint.Caution -> c.tertiaryContainer to c.onTertiaryContainer
        Tint.Negative -> c.errorContainer to c.onErrorContainer
        Tint.Accent -> c.primaryContainer to c.onPrimaryContainer
    }
}

/** A tint's colour as a figure is written in it: the theme's signals, the accent, or the quiet grey. */
@Composable
public fun tintInk(tint: Tint): Color {
    val c = MaterialTheme.colorScheme
    return when (tint) {
        Tint.Positive, Tint.Accent -> c.primary
        Tint.Neutral -> c.onSurfaceVariant
        Tint.Caution -> c.tertiary
        Tint.Negative -> c.error
    }
}

/** A value in a fully rounded, tinted capsule — an available balance, a count. */
@Composable
public fun Pill(tone: Tint, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val (container, on) = tintColors(tone)
    Surface(color = container, contentColor = on, shape = CircleShape, modifier = modifier) {
        Row(Modifier.padding(horizontal = 10.dp, vertical = 4.dp)) { content() }
    }
}

/**
 * A bar filling toward a goal: `fraction` of the way (clamped to the bar), in `color` over
 * `track`. It springs to a new value rather than jumping. TalkBack reads it as progress.
 */
@Composable
public fun ProgressBar(
    fraction: Float,
    modifier: Modifier = Modifier,
    color: Color = MaterialTheme.colorScheme.primary,
    track: Color = MaterialTheme.colorScheme.surfaceContainerHighest,
    height: Dp = 6.dp,
) {
    val f = fraction.coerceIn(0f, 1f)
    val shown by
        animateFloatAsState(
            f,
            Springs.size(),
            label = "progress")
    Canvas(modifier.fillMaxWidth().height(height).progressSemantics(f)) {
        val r = CornerRadius(size.height / 2, size.height / 2)
        // The track, and the fill with a gap before it, as Material's own indicators leave one.
        drawRoundRect(track, cornerRadius = r)
        if (shown > 0f) {
            val w = (size.width * shown).coerceAtLeast(size.height)
            drawRoundRect(color, Offset.Zero, Size(w, size.height), r)
        }
    }
}

/** A figure under its label, as a summary row sets them side by side. */
@Composable
public fun Stat(
    label: String,
    modifier: Modifier = Modifier,
    value: @Composable () -> Unit,
) {
    Column(modifier) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis)
        value()
    }
}
