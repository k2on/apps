// Every page, light and dark, at a phone's size, drawn from the demo's
// household on a fixed day: what `packages.money-android-screens` is made
// of. Paparazzi renders each on the JVM through layoutlib, as Android
// Studio's previews are, from the same composables the app draws — a page
// as its tab shows it, in money's own scheme (a screenshot has no wallpaper
// to take colours from). Sheets are windows of their own, which layoutlib
// does not draw, so none is raised here.
//
// Only `recordPaparazziDebug` runs this (app/build.gradle.kts): the APK's
// build runs the other tests.
package dev.money.android

import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import dev.arkui.Space
import uniffi.money_bridge.StoreItem
import uniffi.money_bridge.WriteReceipt
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import dev.arkdb.auth.Phase
import dev.arkdb.auth.Said
import dev.arkdb.auth.Step
import org.junit.After
import org.junit.Rule
import org.junit.Test

class Screens {
    @get:Rule val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true)

    private val book = demoBook()

    @After fun close() = book.close()

    private fun shot(name: String, content: @Composable () -> Unit) {
        val shell = Shell(book, SnackbarHostState(), {}, {}, {}, {}, {}, {})
        for (dark in listOf(false, true)) {
            paparazzi.snapshot(if (dark) "$name-dark" else "$name-light") {
                MoneyTheme(dark = dark, dynamic = false) {
                    androidx.compose.material3.Surface {
                        CompositionLocalProvider(LocalShell provides shell) { content() }
                    }
                }
            }
        }
    }

    private val none: () -> Unit = {}
    private val said = Said(null, null, null)

    @Test fun welcome() = shot("welcome") { StartScreens(Step.Server("", false, null), said, null, {}, none) }

    @Test fun welcomeRefused() =
        shot("welcome-refused") {
            StartScreens(Step.Server("example.com", false, "example.com answered, but it isn't an Ark server: it has no health page."), said, null, {}, none)
        }

    @Test fun signIn() =
        shot("sign-in") { StartScreens(Step.SignIn("https://money.koon.us", Phase.BROWSER, null), said, null, {}, none) }

    @Test fun budget() {
        val holder = BudgetHolder(book.session, {}, { book.state.value.today }, null)
        holder.send(uniffi.money_bridge.BudgetMsg.Month(month = book.state.value.month))
        shot("budget") { ScreenFrame { BudgetScreen(holder) } }
    }

    @Test fun accounts() = shot("accounts") { ScreenFrame { AccountsScreen(AccountsHolder(book.session, {}, { book.state.value.today }, null, null)) } }

    @Test fun review() = shot("transactions") { ActivityScreen(book.state.value) }

    @Test fun insights() = shot("insights") { InsightsScreen(book.state.value) }

    @Test fun register() {
        val card = book.state.value.accounts.first { it.kind == "credit" }
        book.holdRegister(card.id)
        shot("register") { RegisterScreen(book.state.value, card.id, onBack = none) }
    }

    @Test fun settings() = shot("settings") { SettingsScreen(book.state.value, onBack = none) }

    /** A transaction's receipt as its sheet draws it: the paper, white in light and dark. */
    @Test fun receipt() {
        val acc = book.state.value.accounts.first { it.onBudget && !it.closed }.id
        book.holdRegister(acc)
        val t = book.register(acc).first { it.amount < -2_000 && !it.transfer && it.receipt.isEmpty() }
        val item = { name: String, cents: Long -> StoreItem(name, "", 1000, "ea", -cents) }
        val tax = 83L
        val items =
            listOf(item("KS ORGANIC EGGS 24CT", 899), item("BANANAS 3LB", 149), item("SPARKLING WATER", -t.amount - 899 - 149 - tax), item("TAX", tax))
        book.writeReceipt(WriteReceipt("", t.id, "Corner Shop", t.date, null, items))
        val line = book.register(acc).first { it.id == t.id }
        book.holdItems(line.receipt)
        shot("receipt") { Column(Modifier.padding(Space.xl)) { ReceiptSection(book.state.value, t.id, line.receipt, t.amount, t.date) } }
    }
}
