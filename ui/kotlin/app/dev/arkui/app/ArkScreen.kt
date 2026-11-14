// A screen's runtime as its generated Compose reads it, for every Ark app: the record it shows
// (`page`, read again after every message and whenever its host says something moved) and where
// a press goes (`send`). Nothing here decides anything: a message goes to Rust, and Rust says
// what the page is now. The app's generated holders are this over each of its screens' runtimes
// (`class AccountsHolder(r: AccountsApp) : ArkScreen<AccountsPage, AccountsMsg>(…)`, written by
// ui/prebuild.py from app.toml's `[screens]`); the first run's is too.
//
// What waits for the page to be drawn (`Command::AfterDraw`: an inspector put away before a launch
// moves a section) is taken once the next frame is: a frame's callback, then the turn after it,
// when what it drew is on the screen (`drawn`).
package dev.arkui.app

import android.os.Handler
import android.os.Looper
import android.view.Choreographer
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** A screen's runtime, as the bridge exports every one of them. */
public open class ArkScreen<P, M>(
    private val read: () -> P,
    private val take: (M) -> Unit,
    private val count: () -> ULong,
    private val quiet: () -> Boolean,
    private val draw: () -> Boolean = { false },
    private val awaits: () -> Boolean = { false },
) {
    private val _page = MutableStateFlow(read())

    /** What the screen shows now. */
    public val page: StateFlow<P> = _page

    /** `msg` taken by the runtime; the page read again. */
    public fun send(msg: M) {
        take(msg)
        _page.value = read()
        afterDraw()
    }

    /** The page read again: something it holds moved. */
    public fun reload() {
        _page.value = read()
        afterDraw()
    }

    /** Nothing left to come (`Runtime::is_idle`), and the number that grows with every message. */
    public fun isIdle(): Boolean = quiet()

    public fun version(): ULong = count()

    private fun afterDraw() {
        if (!awaits()) return
        Choreographer.getInstance().postFrameCallback {
            Handler(Looper.getMainLooper()).post {
                if (draw()) _page.value = read()
                afterDraw()
            }
        }
    }
}
