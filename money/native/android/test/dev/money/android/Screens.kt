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
import dev.arkdb.ffi.HostShell
import dev.arkdb.ffi.Launch
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.money_ffi.MoneyHost
import uniffi.money_ffi.WelcomeMsg
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import dev.arkui.WideButton
import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/** A shell that is told and does nothing: the pages are read as they stand. */
private object Quiet : HostShell {
    override fun changed() {}

    override fun browse(url: String) {}
}

/** money's host on a directory of its own, as a launch says, started. */
private fun host(demo: Boolean, kept: String = ""): MoneyHost {
    val root = Files.createTempDirectory("money-screens").toFile()
    if (kept.isNotEmpty()) File(root, "money").apply { mkdirs() }.resolve("first-run").writeText(kept)
    val m = MoneyHost(Launch(root.path, "0.1", demo, if (demo) "2026-10-26" else null, false, false, false, emptyList()), null)
    m.host().start(Quiet)
    return m
}

/** Until `f` holds, a few seconds at most: the host opens off the test's thread. */
private fun until(f: () -> Boolean) {
    val end = System.currentTimeMillis() + 20_000
    while (!f()) {
        check(System.currentTimeMillis() < end) { "never came" }
        Thread.sleep(10)
    }
}

class Screens {
    @get:Rule val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true)

    /** The demo's household on its day, every screen's runtime made. */
    private val money = host(demo = true).also { m -> until { m.host().showsApp() } }

    private fun shot(name: String, content: @Composable () -> Unit) {
        val shell = Shell(money, SnackbarHostState(), MutableStateFlow(money.shell().page().household), {}, {})
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

    /** The first run walked to a page by `steps`, as its generated page draws it. */
    private fun first(kept: String = "", steps: (FirstRunHolder) -> Unit = {}): FirstRunHolder {
        val m = host(demo = false, kept = kept)
        val holder = FirstRunHolder(m.welcome())
        steps(holder)
        return holder
    }

    @Test fun welcome() = shot("welcome") { WelcomeScreen(first()) }

    @Test fun welcomeServer() = shot("welcome-server") { WelcomeScreen(first { it.send(WelcomeMsg.Skip) }) }

    @Test fun welcomeRefused() {
        val page = first {
            it.send(WelcomeMsg.Skip)
            it.send(WelcomeMsg.Typed("http://127.0.0.1:9"))
            it.send(WelcomeMsg.Check)
            until {
                it.reload()
                it.page.value.server?.problem != null
            }
        }
        shot("welcome-refused") { WelcomeScreen(page) }
    }

    @Test fun signIn() = shot("sign-in") { WelcomeScreen(first(kept = "server https://money.example.com\nseen 1\n")) }

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

    @Test fun budget() = shot("budget") { ScreenFrame { BudgetScreen(BudgetHolder(money.budget())) } }

    @Test fun accounts() = shot("accounts") { ScreenFrame { AccountsScreen(AccountsHolder(money.accounts())) } }

    @Test fun review() = shot("transactions") { ScreenFrame { TransactionsScreen(TransactionsHolder(money.transactions())) } }

    @Test fun insights() = shot("insights") { ScreenFrame { InsightsScreen(InsightsHolder(money.insights())) } }

    @Test fun register() {
        val card = money.accounts().page().shelves.flatMap { it.accounts }.first { it.kindKey == "credit" }
        val holder = RegisterHolder(money.register(uniffi.money_ffi.RegisterMsg.Show(account = card.id))!!)
        shot("register") { ScreenFrame { RegisterScreen(holder) } }
    }

    @Test fun settings() = shot("settings") { ScreenFrame { MoneySettings(MoneyShellHolder(money.shell())) } }
}
