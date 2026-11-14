// Tracking and leading by size, as Apple sets type (*The Details of UI
// Typography*). A single letter-spacing is wrong somewhere: letters drawn
// large read as too far apart, and drawn small as too close, so large
// display text is tracked a little tighter, body text is left alone, and
// captions are opened a little. Leading goes the other way from size: big
// text set at body leading looks double-spaced, so a headline's lines sit
// close, and body keeps the room it reads comfortably in.
//
// The numbers follow the shape of Apple's tracking table for its system
// font — about +2% of the size at 11, nothing from 15 to 17, −2% by the
// large title's 34 — and its leading, about 1.2 of the size from 28 up;
// below 20 sp an app's own line heights stand. They are said in `em`, so
// they scale with the size the user has asked text to be. [tracked] applies
// them to a whole type scale, which is how both apps' themes take them.
package dev.arkui

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.isSpecified
import androidx.compose.ui.unit.sp

public object Typeset {
    // (size in sp, tracking in em): between two, the line between them; past the ends, the end.
    private val TRACKING =
        listOf(
            11f to 0.02f,
            13f to 0.01f,
            15f to 0f,
            17f to 0f,
            20f to -0.005f,
            28f to -0.015f,
            34f to -0.02f,
            57f to -0.025f,
        )

    // (size in sp, line height as a multiple of it), from where leading is set at all.
    private val LEADING =
        listOf(
            20f to 1.25f,
            28f to 1.2f,
            34f to 1.2f,
            57f to 1.12f,
        )

    /** Leading is set from this size up; smaller text keeps the line height it has. */
    public const val LEADING_FROM: Float = 20f

    /** The tracking for text `size` sp tall: tighter as it grows, a little open when small. */
    public fun tracking(size: Float): TextUnit = along(TRACKING, size).em

    /** The line height for text `size` sp tall, from [LEADING_FROM] up: tight on big text. */
    public fun leading(size: Float): TextUnit = (size * along(LEADING, size)).sp

    private fun along(points: List<Pair<Float, Float>>, x: Float): Float {
        if (x <= points.first().first) return points.first().second
        if (x >= points.last().first) return points.last().second
        val i = points.indexOfFirst { it.first >= x }
        val (x0, y0) = points[i - 1]
        val (x1, y1) = points[i]
        return y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    }
}

/**
 * This style tracked for its size ([Typeset.tracking]), and, from [Typeset.LEADING_FROM] up, led for it
 * ([Typeset.leading]). A style without a size is left as it is.
 */
public fun TextStyle.tracked(): TextStyle {
    if (!fontSize.isSpecified || !fontSize.isSp) return this
    val size = fontSize.value
    return copy(
        letterSpacing = Typeset.tracking(size),
        lineHeight = if (size >= Typeset.LEADING_FROM) Typeset.leading(size) else lineHeight,
    )
}

/** Every style of this scale [tracked] for its size. */
public fun Typography.tracked(): Typography =
    Typography(
        displayLarge = displayLarge.tracked(),
        displayMedium = displayMedium.tracked(),
        displaySmall = displaySmall.tracked(),
        headlineLarge = headlineLarge.tracked(),
        headlineMedium = headlineMedium.tracked(),
        headlineSmall = headlineSmall.tracked(),
        titleLarge = titleLarge.tracked(),
        titleMedium = titleMedium.tracked(),
        titleSmall = titleSmall.tracked(),
        bodyLarge = bodyLarge.tracked(),
        bodyMedium = bodyMedium.tracked(),
        bodySmall = bodySmall.tracked(),
        labelLarge = labelLarge.tracked(),
        labelMedium = labelMedium.tracked(),
        labelSmall = labelSmall.tracked(),
    )
