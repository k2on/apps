// money's look: Material You (arkui's `MaterialYou`) in arkui's theme
// (arkui/theme.toml) — the neutrals every Ark app shares and money's accent,
// the green, with the shared caution in the tertiary slot, which is what
// "worth a look" is drawn in. The wallpaper's colours are there for asking
// (`dynamic`), and off by default: money is green on every phone, as it is
// on the iPhone and the desktop. And how the screens write a day and a
// month: in the phone's locale, as its own apps do.
package dev.money.android

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalConfiguration
import androidx.core.os.ConfigurationCompat
import dev.arkui.Accent
import dev.arkui.Amounts
import dev.arkui.ArkTheme
import dev.arkui.LocalAmounts
import dev.arkui.LocalArkTheme
import dev.arkui.MaterialYou
import dev.arkui.arkuiScheme
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/** The palette, both sides: arkui's, in the green. */
object Palette {
    val Light: ColorScheme = arkuiScheme(Accent.Green, dark = false)
    val Dark: ColorScheme = arkuiScheme(Accent.Green, dark = true)
}

/**
 * money's theme, its derived squares in the green, and money written in the phone's locale under
 * it. `dynamic` draws the wallpaper's colours instead, on Android 12 and later.
 */
@Composable
fun MoneyTheme(dark: Boolean = isSystemInDarkTheme(), dynamic: Boolean = false, content: @Composable () -> Unit) {
    val locale = ConfigurationCompat.getLocales(LocalConfiguration.current)[0] ?: Locale.getDefault()
    val amounts = remember(locale) { Amounts(locale) }
    MaterialYou(light = Palette.Light, dark = Palette.Dark, darkTheme = dark, dynamic = dynamic) {
        CompositionLocalProvider(LocalAmounts provides amounts, LocalArkTheme provides ArkTheme(Accent.Green.art(dark)), content = content)
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
