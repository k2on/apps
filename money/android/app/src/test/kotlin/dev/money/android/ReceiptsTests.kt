// A transaction's receipt, over the real kernel: the demo's household, its
// book inline. A receipt typed for a transaction pays for it and marks its
// row; edited, it is put right, not doubled; taken off, it waits to be
// attached again. A photo taken is kept on the phone at once and waits to be
// read (the demo has no server to read it), its bytes there to show.
package dev.money.android

import uniffi.money_domain.WriteReceipt
import uniffi.money_domain.StoreItem
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class ReceiptsTests {
    private lateinit var book: Book
    private val l: Ledger
        get() = book.state.value

    @Before fun open() {
        book = demoBook()
    }

    @After fun close() = book.close()

    private fun spending() = l.accounts.first { it.onBudget && !it.closed }.id.also { book.holdRegister(it) }

    @Test fun aReceiptTypedForATransactionPaysForIt() {
        val acc = spending()
        val t = book.register(acc).first { it.amount < 0 && !it.transfer && it.receipt.isEmpty() }
        val item = { name: String, cents: Long -> StoreItem(name, "", 1000, "ea", -cents) }
        book.writeReceipt(WriteReceipt("", t.id, "Corner Shop", t.date, null, listOf(item("EGGS", -t.amount - 100), item("TAX", 100))))
        val r = l.receiptsOf(t.id, book.register(acc).first { it.id == t.id }.receipt).single()
        assertEquals(t.amount, r.total)
        assertEquals(t.id, r.txnId)
        assertEquals("the row is marked", r.id, book.register(acc).first { it.id == t.id }.receipt)
        book.holdItems(r.id)
        assertEquals(listOf("EGGS", "TAX"), book.itemsOf(r.id).map { it.name })

        // Put right: the same receipt, one item.
        book.writeReceipt(WriteReceipt(r.id, t.id, "Corner Shop", t.date, null, listOf(item("EGGS", -t.amount))))
        assertEquals(1, l.receiptsOf(t.id, r.id).size)
        assertEquals(listOf("EGGS"), book.itemsOf(r.id).map { it.name })

        // Taken off, it waits for another to say what paid for it.
        book.pay(r.id, null)
        assertTrue(l.receiptsOf(t.id, "").isEmpty())
        assertTrue(l.unmatched(t.amount, t.date).any { it.id == r.id })
        book.pay(r.id, t.id)
        assertEquals(r.id, book.register(acc).first { it.id == t.id }.receipt)
    }

    @Test fun aPhotoTakenIsKeptAndWaitsToBeRead() {
        val acc = spending()
        val t = book.register(acc).first { it.amount < 0 && !it.transfer }
        val jpeg = byteArrayOf(0xff.toByte(), 0xd8.toByte(), 0xff.toByte(), 0xe0.toByte()) + ByteArray(5000) { (it % 251).toByte() }
        book.addPhoto(jpeg, t.id)
        val p = l.photosOf(t.id).single()
        assertEquals("reading", p.state)
        assertEquals(jpeg.size.toLong(), p.file.size)
        assertEquals("the row says it has a photo", 1L, book.register(acc).first { it.id == t.id }.photos)
        assertArrayEquals("the bytes are on the phone", jpeg, book.photo(p.file))
    }
}
