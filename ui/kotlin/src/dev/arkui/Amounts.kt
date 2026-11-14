// Money as people read it where they are: an amount kept in minor units
// (cents), written by the Rust every client formats with (ark_format, in the
// app's library) in the phone's locale — its symbol, its grouping, its
// decimal mark, where the minus goes. What the locale says is read once,
// here, from java.text ([LocaleNumbers]); the app hands it to its library's
// formatter and provides the result as [LocalAmounts]. The currency is the
// locale's when it counts in hundredths, and the dollar otherwise, because
// the amounts are hundredths whatever the symbol: a budget in yen would
// need amounts that are not.
//
// Figures are drawn in tabular numerals ([AmountText]), so a column of
// them lines up digit under digit, as a bank statement's do.
package dev.arkui

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.isSpecified
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.isSpecified
import androidx.compose.ui.unit.sp
import java.text.DecimalFormat
import java.text.NumberFormat
import java.util.Currency
import java.util.Locale

/**
 * How a place writes money, as java.text says it for `locale`: the fields of ark_format's
 * `NumberStyle`, which the app's library formats with. Read once, when the locale is known.
 */
public data class LocaleNumbers(
    /** The currency's ISO code: `USD`. */
    val currency: String,
    /** What is written beside the figure: `$`, `€`. */
    val symbol: String,
    /** The symbol before the figure (`$1.00`) or after it (`1,00 €`). */
    val symbolFirst: Boolean,
    /** A space between the symbol and the figure. */
    val symbolSpaced: Boolean,
    /** The decimal mark. */
    val decimal: String,
    /** What groups thousands; nothing for none. */
    val grouping: String,
    /** The minus sign. */
    val minus: String,
    /** With the symbol first, the minus before it (`-$12.00`) rather than after it (`$-12.00`). */
    val minusFirst: Boolean,
) {
    public companion object {
        /** What java.text says of money in `locale`, in `currency` (the locale's own by [localCurrency]). */
        public fun of(locale: Locale = Locale.getDefault(), currency: Currency? = null): LocaleNumbers {
            val cur = currency ?: localCurrency(locale)
            val f = NumberFormat.getCurrencyInstance(locale).apply { this.currency = cur }
            val marks = (f as? DecimalFormat)?.decimalFormatSymbols
            val symbol = cur.getSymbol(locale)
            val one = f.format(1)
            val at = one.indexOf(symbol)
            val digit = one.indexOfFirst { it.isDigit() }
            val first = at in 0 until digit
            val gap = if (at < 0) null else if (first) one.getOrNull(at + symbol.length) else one.getOrNull(at - 1)
            val minus = (marks?.minusSign ?: '-').toString()
            val less = f.format(-1)
            val minusFirst = less.indexOf(minus).let { m -> m < 0 || less.indexOf(symbol) < 0 || m < less.indexOf(symbol) }
            return LocaleNumbers(
                currency = cur.currencyCode,
                symbol = symbol,
                symbolFirst = first,
                symbolSpaced = gap?.isWhitespace() == true,
                decimal = (marks?.monetaryDecimalSeparator ?: '.').toString(),
                grouping = if (f.isGroupingUsed) (marks?.groupingSeparator ?: ',').toString() else "",
                minus = minus,
                minusFirst = minusFirst,
            )
        }

        /** The locale's currency when it counts in hundredths; the dollar otherwise. */
        public fun localCurrency(locale: Locale): Currency =
            runCatching { Currency.getInstance(locale) }
                .getOrNull()
                ?.takeIf { it.defaultFractionDigits == 2 }
                ?: Currency.getInstance("USD")
    }
}

/**
 * Amounts in minor units, written and read as the app's library writes them (ark_format) in a
 * [LocaleNumbers]: what [AmountText], [MarkedAmount] and [AmountField] draw with.
 */
public interface Amounts {
    /** `$1,234.56`, `-$12.00` — or `1.234,56 €` where that is how it is written. */
    public fun format(minor: Long): String

    /** [format] with a plus on what came in: `+$25.00`. */
    public fun signed(minor: Long): String

    /** The symbol alone, for a field's prefix: `$`, `€`. */
    public val symbol: String

    /** An amount as a field shows it for editing: `1234.50` in the locale's decimal mark; nothing for nothing. */
    public fun editable(minor: Long): String

    /** What somebody typed, in minor units: nothing typed is 0, what is not an amount null. */
    public fun parse(typed: String): Long?
}

/** How the screens under it write money: the app provides its library's. */
public val LocalAmounts: ProvidableCompositionLocal<Amounts> =
    staticCompositionLocalOf { error("No Amounts here: the app provides its library's (ark_format) in its theme") }

/**
 * An amount in tabular figures, so a column of them lines up. `signed` writes a plus on money that
 * came in. TalkBack reads it as written, which is how the locale says it.
 */
@Composable
public fun AmountText(
    minor: Long,
    modifier: Modifier = Modifier,
    style: TextStyle = LocalTextStyle.current,
    color: Color = Color.Unspecified,
    signed: Boolean = false,
    textAlign: TextAlign? = null,
) {
    val a = LocalAmounts.current
    Text(
        if (signed) a.signed(minor) else a.format(minor),
        modifier = modifier,
        style = style.copy(fontFeatureSettings = "tnum"),
        color = color,
        textAlign = textAlign,
        maxLines = 1,
    )
}

/** The colour an amount is drawn in: what came in in the primary, what went out as text is. */
@Composable
public fun amountColor(minor: Long): Color =
    if (minor > 0) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface

/**
 * An amount that is not this screen's to change, with `mark` after it at the text's own height: one
 * worked out elsewhere (by a rule, a schedule, the bank) and shown as it is. `why` is the mark's
 * meaning, said by TalkBack after the amount.
 */
@Composable
public fun MarkedAmount(
    minor: Long,
    mark: ImageVector,
    why: String,
    modifier: Modifier = Modifier,
    style: TextStyle = LocalTextStyle.current,
    color: Color = Color.Unspecified,
) {
    val a = LocalAmounts.current
    val size = with(LocalDensity.current) { (if (style.fontSize.isSpecified) style.fontSize else 14.sp).toDp() }
    Row(
        modifier.semantics(mergeDescendants = true) { contentDescription = "${a.format(minor)}, $why" },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AmountText(minor, style = style, color = color)
        Spacer(Modifier.width(Space.xs))
        Icon(
            mark,
            contentDescription = null,
            tint = if (color.isSpecified) color else LocalContentColor.current,
            modifier = Modifier.size(size))
    }
}
