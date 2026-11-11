// The screens whose state, rules and layout are Rust's (ui/docs/app.md):
// what their generated Compose (generated/) reads and sends to. Each holds
// its runtime over the bridge (`AccountsApp`, money_app run behind
// money_ffi) and publishes the record it shows, read again after every
// message and whenever the book moved. Nothing here decides anything.
package dev.money.android

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import dev.arkui.LocalGlyphs
import dev.arkui.LocalRowMark
import dev.arkdb.ffi.Id
import dev.arkui.LocalTabFrame
import uniffi.money_bridge.AccountsApp
import uniffi.money_bridge.BudgetApp
import uniffi.money_bridge.BudgetMsg
import uniffi.money_bridge.BudgetPage
import uniffi.money_bridge.AppShell
import uniffi.money_bridge.MoneyRequest
import uniffi.money_bridge.Session
import uniffi.money_bridge.ShellApp
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import uniffi.ark_clipboard.Clipboard
import uniffi.ark_haptics.Haptics
import uniffi.money_bridge.AccountsMsg
import uniffi.money_bridge.AccountsPage
import uniffi.money_bridge.ShellMsg
import uniffi.money_bridge.ShellPage
import uniffi.money_bridge.Route

/** The Accounts tab, running in Rust. */
class AccountsHolder(session: Session, refresh: () -> Unit, today: () -> String, haptics: Haptics?, clipboard: Clipboard?) {
    /** Opens a route: the app's navigator. */
    var opened: (Route) -> Unit = {}

    /** Raises the reconcile dialog for an account: the register's own, until reconciling moves to Rust. */
    var reconciling: (Id) -> Unit = {}

    private val shell =
        object : AppShell {
            override fun openRoute(route: Route) = opened(route)

            override fun request(request: MoneyRequest) {
                when (request) {
                    is MoneyRequest.Refresh -> refresh()
                    is MoneyRequest.Reconcile -> reconciling(Id(request.account))
                    else -> {}
                }
            }

            override fun nowMs(): Long = System.currentTimeMillis()

            /** The session's day: the demo's in the demo, what a new account is dated with. */
            override fun today(): String = today()
        }
    private val app = AccountsApp(session, shell)
    private val _page = MutableStateFlow(app.page())
    val page: StateFlow<AccountsPage> = _page

    /** The version last seen by a screenshot's ready signal ([quiet]). */
    private var seen = ULong.MAX_VALUE

    /**
     * For a screenshot's ready signal (ArkUI's `Ready`, ui/docs/shots.md): idle at the version last
     * seen (`Runtime::is_idle`, `Runtime::version`); a newer version is a newer screen, waited for
     * again.
     */
    private fun quiet(): Boolean {
        val v = app.version()
        if (v != seen) {
            seen = v
            dev.arkui.Ready.changed()
            return false
        }
        return app.isIdle()
    }

    init {
        dev.arkui.Ready.probe("accounts") { quiet() }
        // money's modules (money/app.toml): no share sheet on Android yet.
        app.useModules(haptics, clipboard, null)
    }

    /** `msg` taken by the app; the page read again. */
    fun send(msg: AccountsMsg) {
        app.send(msg)
        _page.value = app.page()
    }

    /** Nothing left to come (`Runtime::is_idle`), and the number that grows with every message. */
    fun isIdle(): Boolean = app.isIdle()

    fun version(): ULong = app.version()

    /** The page read again: the book moved. */
    fun reload() {
        _page.value = app.page()
    }
}

/** The Budget tab, running in Rust (money/app/src/budget): its month the book's, told as it pages. */
class BudgetHolder(session: Session, refresh: () -> Unit, today: () -> String, haptics: Haptics?) {
    /** Opens a route: a category's month. */
    var opened: (Route) -> Unit = {}

    private val shell =
        object : AppShell {
            override fun openRoute(route: Route) = opened(route)

            override fun request(request: MoneyRequest) {
                if (request is MoneyRequest.Refresh) refresh()
            }

            override fun nowMs(): Long = System.currentTimeMillis()

            override fun today(): String = today()
        }
    private val app = BudgetApp(session, shell)
    private val _page = MutableStateFlow(app.page())
    val page: StateFlow<BudgetPage> = _page

    /** The version last seen by a screenshot's ready signal ([quiet]). */
    private var seen = ULong.MAX_VALUE

    /**
     * For a screenshot's ready signal (ArkUI's `Ready`, ui/docs/shots.md): idle at the version last
     * seen (`Runtime::is_idle`, `Runtime::version`); a newer version is a newer screen, waited for
     * again.
     */
    private fun quiet(): Boolean {
        val v = app.version()
        if (v != seen) {
            seen = v
            dev.arkui.Ready.changed()
            return false
        }
        return app.isIdle()
    }

    init {
        dev.arkui.Ready.probe("budget") { quiet() }
        app.useModules(haptics, null, null)
    }

    /** `msg` taken by the app; the page read again. */
    fun send(msg: BudgetMsg) {
        app.send(msg)
        _page.value = app.page()
    }

    /** Nothing left to come (`Runtime::is_idle`), and the number that grows with every message. */
    fun isIdle(): Boolean = app.isIdle()

    fun version(): ULong = app.version()

    /** The page read again: the book moved. */
    fun reload() {
        _page.value = app.page()
    }
}

/**
 * money's shell, running in Rust (money/app/src/shell): its sections, toolbar, inspector, search
 * and commands, which the generated `MoneyShell`, `MoneySettings` and `moneyCommands` read and send
 * to. A command only a hosted screen can carry out comes back typed (`request`).
 */
class MoneyShellHolder(session: Session, refresh: () -> Unit, today: () -> String) {
    /** What the hosted screens carry out: a new transaction, the month moved, a Budget message… */
    var request: (MoneyRequest) -> Unit = {}

    private val shell =
        object : AppShell {
            override fun openRoute(route: Route) {}

            override fun request(request: MoneyRequest) {
                if (request is MoneyRequest.Refresh) refresh() else this@MoneyShellHolder.request.invoke(request)
            }

            override fun nowMs(): Long = System.currentTimeMillis()

            override fun today(): String = today()
        }
    private val app = ShellApp(session, shell)
    private val _page = MutableStateFlow(app.page())
    val page: StateFlow<ShellPage> = _page

    /** The version last seen by a screenshot's ready signal ([quiet]). */
    private var seen = ULong.MAX_VALUE

    /**
     * For a screenshot's ready signal (ArkUI's `Ready`, ui/docs/shots.md): idle at the version last
     * seen (`Runtime::is_idle`, `Runtime::version`); a newer version is a newer screen, waited for
     * again.
     */
    private fun quiet(): Boolean {
        val v = app.version()
        if (v != seen) {
            seen = v
            dev.arkui.Ready.changed()
            return false
        }
        return app.isIdle()
    }

    init {
        dev.arkui.Ready.probe("shell") { quiet() }
    }

    /** `msg` taken by the shell; the page read again. */
    fun send(msg: ShellMsg) {
        app.send(msg)
        _page.value = app.page()
    }

    /** Nothing left to come (`Runtime::is_idle`), and the number that grows with every message. */
    fun isIdle(): Boolean = app.isIdle()

    fun version(): ULong = app.version()

    /** The page read again: the book moved. */
    fun reload() {
        _page.value = app.page()
    }
}

/**
 * What a screen written from Rust needs of this app: its page frame (the top bar; adding a
 * transaction is the shell toolbar's), its marks and its glyphs.
 */
@Composable
fun ScreenFrame(content: @Composable () -> Unit) {
    CompositionLocalProvider(
        LocalTabFrame provides { title, _, inner ->
            // No floating button over the rows: a new transaction is the
            // shell's toolbar's (money/app/src/shell/view.rs), as on every client.
            Page(title = title) { pad, _ -> Box(Modifier.padding(top = pad.calculateTopPadding())) { inner() } }
        },
        LocalRowMark provides { kind -> KindTile(kind) },
        LocalGlyphs provides Glyphs::shared,
        content = content,
    )
}
