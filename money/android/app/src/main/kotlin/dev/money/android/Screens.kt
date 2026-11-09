// The screens whose state, rules and layout are Rust's (docs/ark-app.md):
// what their generated Compose (generated/) reads and sends to. Each holds
// its runtime over the bridge (`AccountsApp`, money_app run behind
// money_ffi) and publishes the record it shows, read again after every
// message and whenever the book moved. Nothing here decides anything.
package dev.money.android

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.view.View
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import dev.arkui.Feedback
import dev.arkui.LocalGlyphs
import dev.arkui.LocalRowMark
import dev.arkui.LocalTabFrame
import dev.arkui.perform
import dev.money.ffi.AccountsApp
import dev.money.ffi.AppShell
import dev.money.ffi.Session
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import uniffi.ark_modules.Clipboard
import uniffi.ark_modules.Haptic
import uniffi.ark_modules.Haptics
import uniffi.money_app.AccountsMsg
import uniffi.money_app.AccountsPage
import uniffi.money_domain.Route

/** The Accounts tab, running in Rust. */
class AccountsHolder(session: Session, refresh: () -> Unit, haptics: Haptics?, clipboard: Clipboard?) {
    /** Opens a route: the app's navigator. */
    var opened: (Route) -> Unit = {}

    private val shell =
        object : AppShell {
            override fun open(route: Route) = opened(route)

            override fun native(name: String) {
                if (name == "refresh") refresh()
            }

            override fun nowMs(): Long = System.currentTimeMillis()
        }
    private val app = AccountsApp(session, shell)
    private val _page = MutableStateFlow(app.page())
    val page: StateFlow<AccountsPage> = _page

    init {
        app.useModules(haptics, clipboard, null, null)
    }

    /** `msg` taken by the app; the page read again. */
    fun send(msg: AccountsMsg) {
        app.send(msg)
        _page.value = app.page()
    }

    /** The page read again: the book moved. */
    fun reload() {
        _page.value = app.page()
    }
}

/** `docs/motion.md`'s haptics, on the view the app is drawn in. */
class PhoneHaptics(private val view: View) : Haptics {
    override fun perform(haptic: Haptic) {
        // A warning is a success's weight here: ArkUI's two are what Android has an answer for.
        view.post { view.perform(if (haptic == Haptic.SELECTION) Feedback.Selection else Feedback.Success) }
    }
}

/** The system clipboard. */
class PhoneClipboard(private val context: Context) : Clipboard {
    override fun copyText(text: String) {
        val m = context.getSystemService(ClipboardManager::class.java)
        m?.setPrimaryClip(ClipData.newPlainText("Amount", text))
    }
}

/** What a screen written from Rust needs of this app: its page frame, its marks and its glyphs. */
@Composable
fun ScreenFrame(content: @Composable () -> Unit) {
    CompositionLocalProvider(
        LocalTabFrame provides { title, _, inner -> Page(title = title) { pad, _ -> Box(Modifier.padding(top = pad.calculateTopPadding())) { inner() } } },
        LocalRowMark provides { kind -> KindTile(kind) },
        LocalGlyphs provides Glyphs::shared,
        content = content,
    )
}
