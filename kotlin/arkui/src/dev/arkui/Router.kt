// An app's routes on Compose's back stack. A route is the domain's `Route`
// (`harken_domain::routes`, `money_domain::routes`), and its one spelling is
// its path (`album/Water%20Music`); a destination's pattern takes it as an
// argument. Compose decodes an argument once on the way back, so a path whose
// names are already percent-encoded would come back changed: it goes as the
// hex of its UTF-8 instead, which has nothing in it to decode.
package dev.arkui

import android.content.Intent
import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalContext
import androidx.core.util.Consumer
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController

/**
 * Navigating, by the same names on every platform: `push`, `replace`, `back`, `reset` and `handle`
 * — this one over Compose's back stack, the iPhone's over a `NavigationStack`'s path
 * (swift/ArkUI), the desktop's over its address bar's history (rust/arkui's `navigator`).
 * [destination] is where the back stack goes for a route: a destination's pattern with the route's
 * path as its argument ([RouteArg]). The system's back gesture and predictive back pop it as they
 * always did.
 */
class Navigator<R>(private val nav: NavHostController, private val destination: (R) -> String) {
    /** Navigate to `route`, a new entry on the back stack. */
    fun push(route: R) = nav.navigate(destination(route))

    /** Navigate to `route` in place of the entry on top. */
    fun replace(route: R) {
        val top = nav.currentBackStackEntry?.destination?.id
        nav.navigate(destination(route)) { if (top != null) popUpTo(top) { inclusive = true } }
    }

    /** Back one entry; false at the root. */
    fun back(): Boolean = nav.popBackStack()

    /** Start again at the root, or at `route` above it. */
    fun reset(route: R? = null) {
        nav.popBackStack(nav.graph.findStartDestination().id, inclusive = false)
        if (route != null) push(route)
    }

    /**
     * A link the app was opened with (`harken://album/Water%20Music`): the route [parse] reads from
     * its path is offered to [take] first — a sign-in link is the onboarding's, not a page — and
     * navigated to if [take] does not take it. False for a link of another scheme, or one whose
     * path names nothing here (the sign-in's own `auth?code=…`).
     */
    fun handle(link: String, scheme: String, parse: (String) -> R?, take: (R) -> Boolean = { false }): Boolean {
        val route = routeOf(link, scheme, parse) ?: return false
        if (!take(route)) push(route)
        return true
    }

    companion object {
        /** The route a link of `scheme` names: its path is everything after `scheme://`, query and all. */
        fun <R> routeOf(link: String, scheme: String, parse: (String) -> R?): R? {
            val prefix = "$scheme://"
            return if (link.startsWith(prefix)) parse(link.removePrefix(prefix)) else null
        }
    }
}

/**
 * Every link of `scheme` the app is opened with — the intent it was started by, and each one after
 * (`onNewIntent`) — handed to [onLink] once each, for a [Navigator] to [Navigator.handle]. The
 * sign-in's own `scheme://auth?…` is the onboarding's and is left alone.
 */
@Composable
fun Links(scheme: String, onLink: (String) -> Unit) {
    val activity = LocalContext.current as? ComponentActivity ?: return
    val latest = rememberUpdatedState(onLink)
    DisposableEffect(activity, scheme) {
        fun heard(i: Intent?) {
            val data = i?.data ?: return
            if (data.scheme != scheme || data.host == "auth") return
            // Once: the launch intent is still the activity's when it is recreated.
            i.data = null
            latest.value(data.toString())
        }
        heard(activity.intent)
        val listener = Consumer<Intent> { heard(it) }
        activity.addOnNewIntentListener(listener)
        onDispose { activity.removeOnNewIntentListener(listener) }
    }
}

/** A route's path as a destination's argument, and back. */
object RouteArg {
    /** The argument `path` is navigated with. */
    fun of(path: String): String = path.toByteArray(Charsets.UTF_8).joinToString("") { "%02x".format(it.toInt() and 0xff) }

    /** The path an argument carries; null for one that is not an argument [of] wrote. */
    fun path(arg: String?): String? {
        if (arg == null || arg.length % 2 != 0) return null
        return runCatching { String(ByteArray(arg.length / 2) { arg.substring(it * 2, it * 2 + 2).toInt(16).toByte() }, Charsets.UTF_8) }.getOrNull()
    }
}
