// money's look: Material You (arkui's `MaterialYou`) — the wallpaper's
// colours on Android 12 and later, and before that (and in every
// screenshot) a scheme of money's own, a deep green, made the way Material's
// tonal palettes are: one hue for the primary, a quieter one for the
// secondary, a warm third for the tertiary, which is what "worth a look"
// is drawn in. And how the screens write a day and a month: in the phone's
// locale, as its own apps do.
package dev.money.android

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalConfiguration
import androidx.core.os.ConfigurationCompat
import dev.arkui.Amounts
import dev.arkui.LocalAmounts
import dev.arkui.MaterialYou
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

object Palette {
    val Light: ColorScheme =
        lightColorScheme(
            primary = Color(0xFF1B6B4A),
            onPrimary = Color(0xFFFFFFFF),
            primaryContainer = Color(0xFFA6F2C8),
            onPrimaryContainer = Color(0xFF002113),
            secondary = Color(0xFF4E6356),
            onSecondary = Color(0xFFFFFFFF),
            secondaryContainer = Color(0xFFD0E8D7),
            onSecondaryContainer = Color(0xFF0B1F15),
            tertiary = Color(0xFF8A5100),
            onTertiary = Color(0xFFFFFFFF),
            tertiaryContainer = Color(0xFFFFDCBC),
            onTertiaryContainer = Color(0xFF2C1600),
            error = Color(0xFFBA1A1A),
            onError = Color(0xFFFFFFFF),
            errorContainer = Color(0xFFFFDAD6),
            onErrorContainer = Color(0xFF410002),
            background = Color(0xFFF6FBF4),
            onBackground = Color(0xFF171D19),
            surface = Color(0xFFF6FBF4),
            onSurface = Color(0xFF171D19),
            surfaceVariant = Color(0xFFDCE5DC),
            onSurfaceVariant = Color(0xFF404943),
            outline = Color(0xFF707972),
            outlineVariant = Color(0xFFC0C9C1),
            inversePrimary = Color(0xFF8BD5AD),
            surfaceContainerLowest = Color(0xFFFFFFFF),
            surfaceContainerLow = Color(0xFFF0F5EE),
            surfaceContainer = Color(0xFFEAEFE8),
            surfaceContainerHigh = Color(0xFFE5EAE3),
            surfaceContainerHighest = Color(0xFFDFE4DD),
        )

    val Dark: ColorScheme =
        darkColorScheme(
            primary = Color(0xFF8BD5AD),
            onPrimary = Color(0xFF003824),
            primaryContainer = Color(0xFF005236),
            onPrimaryContainer = Color(0xFFA6F2C8),
            secondary = Color(0xFFB4CCBB),
            onSecondary = Color(0xFF203529),
            secondaryContainer = Color(0xFF364B3F),
            onSecondaryContainer = Color(0xFFD0E8D7),
            tertiary = Color(0xFFFFB86E),
            onTertiary = Color(0xFF4A2800),
            tertiaryContainer = Color(0xFF693C00),
            onTertiaryContainer = Color(0xFFFFDCBC),
            error = Color(0xFFFFB4AB),
            onError = Color(0xFF690005),
            errorContainer = Color(0xFF93000A),
            onErrorContainer = Color(0xFFFFDAD6),
            background = Color(0xFF0F1511),
            onBackground = Color(0xFFDFE4DD),
            surface = Color(0xFF0F1511),
            onSurface = Color(0xFFDFE4DD),
            surfaceVariant = Color(0xFF404943),
            onSurfaceVariant = Color(0xFFC0C9C1),
            outline = Color(0xFF8A938B),
            outlineVariant = Color(0xFF404943),
            inversePrimary = Color(0xFF1B6B4A),
            surfaceContainerLowest = Color(0xFF0A0F0C),
            surfaceContainerLow = Color(0xFF171D19),
            surfaceContainer = Color(0xFF1B211D),
            surfaceContainerHigh = Color(0xFF252B27),
            surfaceContainerHighest = Color(0xFF303632),
        )
}

/** money's theme, and money written in the phone's locale under it. `dynamic` off draws money's own. */
@Composable
fun MoneyTheme(dark: Boolean = isSystemInDarkTheme(), dynamic: Boolean = true, content: @Composable () -> Unit) {
    val locale = ConfigurationCompat.getLocales(LocalConfiguration.current)[0] ?: Locale.getDefault()
    val amounts = remember(locale) { Amounts(locale) }
    MaterialYou(light = Palette.Light, dark = Palette.Dark, darkTheme = dark, dynamic = dynamic) {
        CompositionLocalProvider(LocalAmounts provides amounts, content = content)
    }
}

/** Days and months as the phone's locale writes them. */
object Dates {
    private fun day(d: String): LocalDate? = runCatching { LocalDate.parse(d) }.getOrNull()

    /** `October 2026`, in the locale ("octobre 2026"). */
    fun month(m: String, locale: Locale = Locale.getDefault()): String =
        runCatching { YearMonth.parse(m).format(DateTimeFormatter.ofPattern("LLLL yyyy", locale)) }
            .getOrDefault(m)
            .replaceFirstChar { it.titlecase(locale) }

    /** The month alone: `October`. */
    fun monthAlone(m: String, locale: Locale = Locale.getDefault()): String =
        runCatching { YearMonth.parse(m).format(DateTimeFormatter.ofPattern("LLLL", locale)) }
            .getOrDefault(m)
            .replaceFirstChar { it.titlecase(locale) }

    /** A register's day heading: Today, Yesterday, or `Mon, Oct 26`. */
    fun heading(d: String, today: String, locale: Locale = Locale.getDefault()): String {
        val day = day(d) ?: return d
        val now = day(today)
        return when {
            now != null && day == now -> "Today"
            now != null && day == now.minusDays(1) -> "Yesterday"
            now != null && day.year == now.year -> day.format(DateTimeFormatter.ofPattern("EEE, MMM d", locale))
            else -> day.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale))
        }
    }

    /** A day in a form: `Oct 26, 2026`. */
    fun long(d: String, locale: Locale = Locale.getDefault()): String =
        day(d)?.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale)) ?: d

    /** A day as the date picker holds it: UTC midnight, in ms. */
    fun millis(d: String): Long? = day(d)?.toEpochDay()?.times(86_400_000L)

    fun ofMillis(ms: Long): String = LocalDate.ofEpochDay(Math.floorDiv(ms, 86_400_000L)).toString()

    /** How long ago `ms` was, in words: `just now`, `5 min ago`, `3 h ago`, `2 days ago`. */
    fun ago(ms: Long, now: Long = System.currentTimeMillis()): String {
        if (ms <= 0) return "never"
        val s = (now - ms).coerceAtLeast(0) / 1000
        return when {
            s < 60 -> "just now"
            s < 3600 -> "${s / 60} min ago"
            s < 86_400 -> "${s / 3600} h ago"
            s < 2 * 86_400 -> "yesterday"
            else -> "${s / 86_400} days ago"
        }
    }
}
