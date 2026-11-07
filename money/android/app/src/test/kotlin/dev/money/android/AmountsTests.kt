// What java.text says of a locale's money (arkui's LocaleNumbers), read
// once, and the library writing with it (LibraryAmounts): the reading is
// this phone's, the writing every client's (ark_format's own tests hold
// it). A wrong reading shows here as a German amount written the American
// way.
package dev.money.android

import dev.arkui.LocaleNumbers
import java.util.Currency
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AmountsTests {
    private val us = LibraryAmounts(LocaleNumbers.of(Locale.US))
    private val de = LibraryAmounts(LocaleNumbers.of(Locale.GERMANY))

    @Test fun theLocaleIsReadAsJavaTextWritesMoney() {
        val n = LocaleNumbers.of(Locale.US)
        assertEquals("USD", n.currency)
        assertEquals("$", n.symbol)
        assertTrue(n.symbolFirst)
        assertFalse(n.symbolSpaced)
        assertEquals(".", n.decimal)
        assertEquals(",", n.grouping)
        assertTrue(n.minusFirst)
        val g = LocaleNumbers.of(Locale.GERMANY)
        assertEquals("EUR", g.currency)
        assertFalse(g.symbolFirst)
        assertTrue(g.symbolSpaced)
        assertEquals(",", g.decimal)
        assertEquals(".", g.grouping)
    }

    @Test fun centsAreWrittenAsTheLocaleWritesMoney() {
        assertEquals("$1,234.56", us.format(123_456))
        assertEquals("-$12.00", us.format(-1_200))
        assertEquals("+$25.00", us.signed(2_500))
        // The euro, with the German grouping and decimal mark, after the figure.
        assertEquals("1.234,56 €", de.format(123_456))
    }

    @Test fun whatIsTypedIsReadInTheLocale() {
        assertEquals(120_000L, us.parse("$1,200.00"))
        assertEquals(1_250L, de.parse("12,50"))
        assertEquals(1_250L, de.parse(de.editable(1_250)))
        assertEquals("12,50", de.editable(1_250))
    }

    @Test fun aLocaleWhoseMoneyIsNotInHundredthsCountsInDollars() {
        assertEquals(Currency.getInstance("USD"), LocaleNumbers.localCurrency(Locale.JAPAN))
        assertEquals(Currency.getInstance("USD"), LocaleNumbers.localCurrency(Locale.ENGLISH))
    }
}
