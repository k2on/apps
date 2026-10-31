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
import dev.arkui.SignInPhase
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

    @Test fun welcome() = shot("welcome") { StartScreens(Step.Welcome(), {}, none, none, none, none, none) }

    @Test fun welcomeRefused() =
        shot("welcome-refused") {
            StartScreens(Step.Welcome(typed = "example.com", error = "example.com answered, but it isn't an Ark server: it has no health page."), {}, none, none, none, none, none)
        }

    @Test fun signIn() =
        shot("sign-in") { StartScreens(Step.SignIn("https://money.koon.us", SignInPhase.Browser), {}, none, none, none, none, none) }

    @Test fun budget() = shot("budget") { BudgetScreen(book.state.value) }

    @Test fun accounts() = shot("accounts") { AccountsScreen(book.state.value) }

    @Test fun review() = shot("transactions") { ActivityScreen(book.state.value) }

    @Test fun insights() = shot("insights") { InsightsScreen(book.state.value) }

    @Test fun register() {
        val card = book.state.value.accounts.first { it.kind == "credit" }
        book.holdRegister(card.id)
        shot("register") { RegisterScreen(book.state.value, card.id, onBack = none) }
    }

    @Test fun settings() = shot("settings") { SettingsScreen(book.state.value, onBack = none) }
}
