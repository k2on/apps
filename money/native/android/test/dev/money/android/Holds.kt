// What a screen holds while a test reads it: a register or a receipt's items
// held through the book's session (`MoneyView`), as `watched.watch` holds
// them while a screen is composed, and read as the screen reads them.
package dev.money.android

import dev.arkdb.ffi.Id
import uniffi.money_bridge.Holding
import uniffi.money_bridge.MoneyView
import uniffi.money_bridge.TxnLine

private val holds = HashMap<Pair<Book, MoneyView>, ArrayDeque<Holding>>()

private fun Book.hold(v: MoneyView) {
    holds.getOrPut(this to v) { ArrayDeque() }.addLast(session.hold(v))
}

private fun Book.release(v: MoneyView) {
    holds[this to v]?.removeLastOrNull()?.close()
}

/** `account`'s register (none: every account's) held, as a screen showing it holds it. */
fun Book.holdRegister(account: Id?) = hold(MoneyView.Register(account))

fun Book.releaseRegister(account: Id?) = release(MoneyView.Register(account))

/** A register as a screen holding it reads it. */
fun Book.register(account: Id?): List<TxnLine> = session.register(account)
