// arkui's Amounts, as money uses it: cents written as the locale writes
// money and read back from what somebody typed, in the dollar where the
// locale's own currency does not count in hundredths.
package dev.money.android

import dev.arkui.Amounts
import java.util.Currency
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AmountsTests {
    private val us = Amounts(Locale.US)
    private val de = Amounts(Locale.GERMANY)

    @Test fun centsAreWrittenAsTheLocaleWritesMoney() {
        assertEquals("$1,234.56", us.format(123_456))
        assertEquals("-$12.00", us.format(-1_200))
        assertEquals("+$25.00", us.signed(2_500))
        assertEquals("$0.00", us.signed(0))
        // The euro, with the German grouping and decimal mark, after the figure.
        assertEquals("EUR", de.currency.currencyCode)
        assertEquals("1.234,56 €", de.format(123_456))
    }

    @Test fun whatIsTypedIsReadInTheLocale() {
        assertEquals(1_250L, us.parse("12.5"))
        assertEquals(120_000L, us.parse("$1,200.00"))
        assertEquals(-310L, us.parse("-3.10"))
        assertEquals(0L, us.parse(""))
        assertNull(us.parse("12.345"))
        assertNull(us.parse("twelve"))
        assertNull(us.parse("1.2.3"))
        assertEquals(1_250L, de.parse("12,50"))
        assertEquals(123_456L, de.parse("1.234,56"))
    }

    @Test fun anAmountIsEditedAsItIsTyped() {
        assertEquals("1234.50", us.editable(123_450))
        assertEquals("", us.editable(0))
        assertEquals("12,50", de.editable(1_250))
        assertEquals(1_250L, de.parse(de.editable(1_250)))
    }

    @Test fun aLocaleWhoseMoneyIsNotInHundredthsCountsInDollars() {
        assertEquals(Currency.getInstance("USD"), Amounts.localCurrency(Locale.JAPAN))
        assertEquals(Currency.getInstance("USD"), Amounts.localCurrency(Locale.ENGLISH))
    }
}
