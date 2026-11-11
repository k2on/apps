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
import androidx.compose.runtime.CompositionLocalProvider
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import dev.arkdb.auth.Event
import dev.arkdb.auth.Found
import dev.arkdb.auth.Remembered
import dev.arkdb.auth.Step
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.money_bridge.FirstRunApp
import uniffi.money_bridge.WelcomeMsg
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import dev.arkui.WideButton
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class Screens {
    @get:Rule val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true)

    private val book = demoBook()

    @After fun close() = book.close()

    private fun shot(name: String, content: @Composable () -> Unit) {
        val shell = Shell(book, SnackbarHostState(), {}, {}, {}, {}, {})
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

    /** The first run walked to a page by `steps`, as its generated page draws it. */
    private fun first(steps: (FirstRunApp) -> Unit): FirstRunHolder {
        val run = FirstRunApp(MONEY, false, true)
        run.event(Event.Launched(Remembered(null, null, false), null, false))
        steps(run)
        return FirstRunHolder(MutableStateFlow(run.page())) {}
    }

    @Test fun welcome() = shot("welcome") { StartScreens(Step.Server("", false, null), first {}, null, {}, none) }

    @Test fun welcomeServer() =
        shot("welcome-server") { StartScreens(Step.Server("", false, null), first { it.send(WelcomeMsg.Skip) }, null, {}, none) }

    @Test fun welcomeRefused() =
        shot("welcome-refused") {
            val page = first {
                it.send(WelcomeMsg.Skip)
                it.send(WelcomeMsg.Typed("example.com"))
                it.send(WelcomeMsg.Check)
                it.event(Event.Checked(Found.Failed("example.com answered, but it isn't an Ark server: it has no health page.")))
            }
            StartScreens(Step.Server("example.com", false, null), page, null, {}, none)
        }

    @Test fun signIn() =
        shot("sign-in") {
            val page = first { it.event(Event.Launched(Remembered("https://money.example.com", null, false), null, false)) }
            StartScreens(Step.Server("", false, null), page, null, {}, none)
        }

    /**
     * The first run's wide button says its title to a screen reader and a driver, on a phone and on
     * a tablet's width: dry run 43105302139 found a tablet's and an open Fold's Continue with no text
     * in the accessibility tree. Falsified by taking `contentDescription` off `WideButton`'s button
     * while its label clears its own semantics.
     */
    @Test fun wideButtonIsNamed() {
        for (config in listOf(DeviceConfig.PIXEL_5, DeviceConfig.NEXUS_10)) {
            paparazzi.unsafeUpdateConfig(deviceConfig = config)
            val view = ComposeView(paparazzi.context).apply { setContent { MoneyTheme(dark = false, dynamic = false) { WideButton("Continue", true, true, false) {} } } }
            paparazzi.snapshot(view, "first-run-button-${config.screenWidth}")
            val root = (view.getChildAt(0) as ViewRootForTest).semanticsOwner.rootSemanticsNode
            fun all(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap(::all)
            val button = all(root).firstOrNull { it.config.getOrNull(SemanticsProperties.Role) == Role.Button }
            val said = button?.config?.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
            assertTrue("the button at ${config.screenWidth}px says ${said}", "Continue" in said)
        }
    }

    @Test fun budget() {
        val holder = BudgetHolder(book.session, {}, { book.state.value.today }, null)
        holder.send(uniffi.money_bridge.BudgetMsg.Month(month = book.state.value.month))
        shot("budget") { ScreenFrame { BudgetScreen(holder) } }
    }

    @Test fun accounts() = shot("accounts") { ScreenFrame { AccountsScreen(AccountsHolder(book.session, {}, { book.state.value.today }, null)) } }

    @Test fun review() =
        shot("transactions") {
            val holder = TransactionsHolder(book.session, {}, { book.state.value.today }, null)
            holder.send(uniffi.money_bridge.TransactionsMsg.Month(month = book.state.value.month))
            ScreenFrame { TransactionsScreen(holder) }
        }

    @Test fun insights() {
        val holder = InsightsHolder(book.session, {}, { book.state.value.today })
        holder.send(uniffi.money_bridge.InsightsMsg.Month(month = book.state.value.month))
        shot("insights") { ScreenFrame { InsightsScreen(holder) } }
    }

    @Test fun register() {
        val card = book.state.value.accounts.first { it.kind == "credit" }
        val holder = RegisterHolder(book.session, {}, { book.state.value.today }, null, card.id.hex)
        shot("register") { ScreenFrame { RegisterScreen(holder) } }
    }

    @Test fun settings() = shot("settings") { SettingsScreen(book.state.value, onBack = none) }
}
