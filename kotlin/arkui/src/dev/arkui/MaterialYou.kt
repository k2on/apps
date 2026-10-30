// An app drawn the way the phone's own apps are: Material 3 in the
// wallpaper's colours (dynamic colour, Android 12 and later) or the app's
// own scheme before that, with the expressive shapes — rounder, larger
// corners as things get larger — and a type scale whose headlines carry a
// little more weight. Nothing here names a colour of its own; the app hands
// its fallback schemes in, and everything else asks `MaterialTheme`.
//
// And the small pieces such an app reads with: a value in a tinted pill
// ([Pill]), a bar that fills toward a goal ([Meter]), a figure under its
// label ([Stat]), a section's heading ([SectionHeader]), and a title that
// steps back and forth through something ordered, a month say ([Stepper]).
package dev.arkui

import android.os.Build
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.progressSemantics
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.Typography
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
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

/** What a [Pill] says about its value: good, nothing to say, worth a look, or wrong. */
public enum class Tone {
    Positive,
    Neutral,
    Caution,
    Negative,
}

/** A tone's container and the colour on it, from the scheme. */
@Composable
public fun toneColors(tone: Tone): Pair<Color, Color> {
    val c = MaterialTheme.colorScheme
    return when (tone) {
        Tone.Positive -> c.primaryContainer to c.onPrimaryContainer
        Tone.Neutral -> c.surfaceContainerHighest to c.onSurfaceVariant
        Tone.Caution -> c.tertiaryContainer to c.onTertiaryContainer
        Tone.Negative -> c.errorContainer to c.onErrorContainer
    }
}

/** A value in a fully rounded, tinted capsule — an available balance, a count. */
@Composable
public fun Pill(tone: Tone, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val (container, on) = toneColors(tone)
    Surface(color = container, contentColor = on, shape = CircleShape, modifier = modifier) {
        Row(Modifier.padding(horizontal = 10.dp, vertical = 4.dp)) { content() }
    }
}

/**
 * A bar filling toward a goal: `fraction` of the way (clamped to the bar), in `color` over
 * `track`. It springs to a new value rather than jumping. TalkBack reads it as progress.
 */
@Composable
public fun Meter(
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
            spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMediumLow),
            label = "meter")
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

/** A section's heading, read by TalkBack as one, with an optional action at its end. */
@Composable
public fun SectionHeader(
    title: String,
    modifier: Modifier = Modifier,
    action: String? = null,
    onAction: () -> Unit = {},
) {
    Row(
        modifier.fillMaxWidth().padding(start = Space.page, end = Space.s, top = Space.l, bottom = Space.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween) {
            Text(
                title,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.semantics { heading() })
            if (action != null) TextButton(onClick = onAction) { Text(action) }
        }
}

/**
 * A title stepped through something ordered — a month, a week — with a button either side. The
 * title slides the way it went. `previousLabel` and `nextLabel` are what TalkBack says of the
 * buttons.
 */
@Composable
public fun Stepper(
    label: String,
    onPrevious: () -> Unit,
    onNext: () -> Unit,
    previousLabel: String,
    nextLabel: String,
    modifier: Modifier = Modifier,
    order: Int = 0,
) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        FilledTonalIconButton(onClick = onPrevious) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = previousLabel)
        }
        AnimatedContent(
            targetState = label to order,
            transitionSpec = {
                val forward = targetState.second >= initialState.second
                val d = if (forward) 1 else -1
                (slideInHorizontally { w -> d * w / 3 } + fadeIn()) togetherWith
                    (slideOutHorizontally { w -> -d * w / 3 } + fadeOut())
            },
            label = "stepper",
            modifier = Modifier.weight(1f, fill = false).widthIn(min = 160.dp),
        ) { (text, _) ->
            Text(
                text,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                modifier = Modifier.padding(horizontal = Space.s).semantics { heading() },
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
        }
        FilledTonalIconButton(onClick = onNext) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = nextLabel)
        }
    }
}
