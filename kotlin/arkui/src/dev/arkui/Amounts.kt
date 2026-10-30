// Money as people read it where they are: an amount kept in minor units
// (cents) written in the phone's locale — its symbol, its grouping, its
// decimal mark, where the minus goes — and read back from what somebody
// typed. The currency is the locale's when it counts in hundredths, and
// the dollar otherwise, because the amounts are hundredths whatever the
// symbol: a budget in yen would need amounts that are not.
//
// Figures are drawn in tabular numerals ([AmountText]), so a column of
// them lines up digit under digit, as a bank statement's do.
package dev.arkui

import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import java.math.BigDecimal
import java.text.DecimalFormatSymbols
import java.text.NumberFormat
import java.util.Currency
import java.util.Locale

/** Amounts in minor units, written and read in `locale`, in `currency`. */
public class Amounts(
    public val locale: Locale = Locale.getDefault(),
    currency: Currency? = null,
) {
    public val currency: Currency = currency ?: localCurrency(locale)

    private val money: NumberFormat =
        NumberFormat.getCurrencyInstance(locale).apply {
            this.currency = this@Amounts.currency
            minimumFractionDigits = 2
            maximumFractionDigits = 2
        }

    private val plain: NumberFormat =
        NumberFormat.getNumberInstance(locale).apply {
            minimumFractionDigits = 2
            maximumFractionDigits = 2
            isGroupingUsed = false
        }

    private val decimal: Char = DecimalFormatSymbols.getInstance(locale).decimalSeparator

    /** `$1,234.56`, `-$12.00` — or `1.234,56 $` where that is how it is written. */
    public fun format(minor: Long): String =
        synchronized(money) { money.format(BigDecimal.valueOf(minor, 2)) }

    /** [format] with a plus on what came in: `+$25.00`. */
    public fun signed(minor: Long): String = if (minor > 0) "+" + format(minor) else format(minor)

    /** The symbol alone, for a field's prefix: `$`, `€`. */
    public val symbol: String
        get() = currency.getSymbol(locale)

    /** An amount as a field shows it for editing: `1234.50` in the locale's decimal mark; nothing for nothing. */
    public fun editable(minor: Long): String =
        if (minor == 0L) "" else synchronized(plain) { plain.format(BigDecimal.valueOf(minor, 2)) }

    /**
     * What somebody typed, in minor units: `12`, `12.5`, `$1,200.00`, `-3.10`, or `12,50` where the
     * comma is the decimal mark. Nothing typed is nothing; anything else that is not an amount is
     * null.
     */
    public fun parse(typed: String): Long? {
        val other = if (decimal == ',') '.' else ','
        val t =
            typed.filterNot { it.isWhitespace() || it == other || it == ' ' }
                .replace(symbol, "")
                .replace(currency.currencyCode, "")
                .filterNot { it == '$' || it == '€' || it == '£' }
                .replace(decimal, '.')
        if (t.isEmpty()) return 0
        val negative = t.startsWith('-')
        val body = t.removePrefix("-").removePrefix("+")
        val whole = body.substringBefore('.')
        val frac = body.substringAfter('.', "")
        if (whole.isEmpty() && frac.isEmpty()) return null
        if (body.count { it == '.' } > 1 || frac.length > 2) return null
        if (!(whole + frac).all { it in '0'..'9' }) return null
        val w = if (whole.isEmpty()) 0L else whole.toLongOrNull() ?: return null
        val f = frac.padEnd(2, '0').toLong()
        val minor = Math.addExact(Math.multiplyExact(w, 100L), f)
        return if (negative) -minor else minor
    }

    public companion object {
        /** The locale's currency when it counts in hundredths; the dollar otherwise. */
        public fun localCurrency(locale: Locale): Currency =
            runCatching { Currency.getInstance(locale) }
                .getOrNull()
                ?.takeIf { it.defaultFractionDigits == 2 }
                ?: Currency.getInstance("USD")
    }
}

/** How the screens under it write money; the phone's locale where nobody says. */
public val LocalAmounts: ProvidableCompositionLocal<Amounts> = staticCompositionLocalOf { Amounts() }

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
