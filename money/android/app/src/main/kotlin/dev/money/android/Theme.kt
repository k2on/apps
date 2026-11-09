// money's look: Material You (arkui's `MaterialYou`) in arkui's theme
// (ui/theme.toml) — the neutrals every Ark app shares and money's accent,
// the green, with the shared caution in the tertiary slot, which is what
// "worth a look" is drawn in. The wallpaper's colours are there for asking
// (`dynamic`), and off by default: money is green on every phone, as it is
// on the iPhone and the desktop. And money written as every client writes
// it: by the library (ark_format), in what the phone's locale says.
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
import dev.arkui.LocaleNumbers
import dev.arkui.ArkTheme
import dev.arkui.LocalAmounts
import dev.arkui.LocalArkTheme
import dev.arkui.MaterialYou
import dev.arkui.Tint
import dev.arkui.arkuiScheme
import dev.arkui.expressiveTypography
import dev.arkui.tracked
import java.time.LocalDate
import java.util.Locale
import uniffi.ark_format.NumberStyle
import uniffi.ark_format.formatAmount
import uniffi.ark_format.formatAmountEditable
import uniffi.ark_format.numberStyleUs
import uniffi.ark_format.parseAmount
import uniffi.money_domain.Tone

/** money's type scale: arkui's expressive one, tracked for each size. */
private val MoneyType = expressiveTypography().tracked()

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
    val amounts = remember(locale) { LibraryAmounts(LocaleNumbers.of(locale)) }
    // Material's expressive scale, tracked and led for each size as the iPhone's is (arkui's
    // `tracked`): the big figures and titles tight, body text as drawn, captions a little open.
    MaterialYou(light = Palette.Light, dark = Palette.Dark, darkTheme = dark, dynamic = dynamic, typography = MoneyType) {
        CompositionLocalProvider(LocalAmounts provides amounts, LocalArkTheme provides ArkTheme(Accent.Green.art(dark)), content = content)
    }
}

/**
 * Money as the library writes it (ark_format's, every client's): in [numbers], what java.text says
 * of the phone's locale, read once ([LocaleNumbers.of]).
 */
class LibraryAmounts(numbers: LocaleNumbers = LocaleNumbers.of()) : Amounts {
    val style =
        NumberStyle(
            numbers.currency, numbers.symbol, numbers.symbolFirst, numbers.symbolSpaced,
            numbers.decimal, numbers.grouping, numbers.minus, numbers.minusFirst)

    override fun format(minor: Long): String = formatAmount(minor, false, style)

    override fun signed(minor: Long): String = formatAmount(minor, true, style)

    override val symbol: String = numbers.symbol

    override fun editable(minor: Long): String = formatAmountEditable(minor, style)

    override fun parse(typed: String): Long? = parseAmount(typed, style)
}

/** The number style the core writes this phone's amounts in (`BudgetLine.story`, `captions`). */
val Amounts.style: NumberStyle
    get() = (this as? LibraryAmounts)?.style ?: numberStyleUs()

/** What arkui draws one of the core's tones in: money's one map, as the iPhone's and the desktop's. */
fun Tone.tint(): Tint =
    when (this) {
        Tone.NEUTRAL -> Tint.Neutral
        Tone.POSITIVE -> Tint.Positive
        Tone.SHORT, Tone.ON_CARD -> Tint.Caution
        Tone.OVERSPENT -> Tint.Negative
    }

/** A day as the date picker holds it: UTC midnight, in ms. The words for a day are ark_format's. */
object Dates {
    private fun day(d: String): LocalDate? = runCatching { LocalDate.parse(d) }.getOrNull()

    /** A day as the date picker holds it: UTC midnight, in ms. */
    fun millis(d: String): Long? = day(d)?.toEpochDay()?.times(86_400_000L)

    fun ofMillis(ms: Long): String = LocalDate.ofEpochDay(Math.floorDiv(ms, 86_400_000L)).toString()
}
