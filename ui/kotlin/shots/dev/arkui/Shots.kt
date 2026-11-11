// Screenshots that never depend on time (ui/docs/shots.md). A capture
// launches the app with `-shots 1` among its launch arguments (the intent's
// `ark.args`, as every shell reads them); then, and only then:
//
// - **Motion is at its end at once.** The activity's Compose runs on a
//   duration scale of 0 ([ReducedMotion.scale], [keepMotion]): every spring
//   and transition of arkui's, Material's too, ends the frame it starts. Not
//   reduced motion, whose fade is a move too.
// - **The app says when it is ready** ([Ready]): when nothing it said it
//   waits for is still to come ([Ready.waiting]: the peer settling, a picture
//   loading), and a frame has been drawn since. Then a view on the window,
//   1 px and drawing nothing, says `ark.ready ready <generation> <route>` as
//   its content description, which uiautomator's dump shows; anything that
//   may change what is drawn (a touch, a link, a route, the peer's news)
//   takes it away and grows the generation. The driver
//   (.github/screens/app-android.sh) does something, then waits for a newer
//   generation, then shoots: no sleep, no "the same picture twice".
//
// Off (every other launch) none of it exists.
//
// Its own source directory (ui/kotlin/shots), Android's framework alone, so
// an app that draws nothing of ArkUI's (revelation) has it too: every app's
// project compiles it ([watchForShots] is what such an app calls; an ArkUI
// app's [keepMotion] calls it).
package dev.arkui

import androidx.activity.ComponentActivity

import android.content.Intent
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.Choreographer
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.widget.FrameLayout

public object Shots {
    /** Whether this launch is a screenshot's: read from its intent by [keepMotion]. */
    @Volatile
    public var on: Boolean = false
        private set

    /** `-shots 1` (or `YES`, `true`) among `intent`'s `ark.args`. */
    public fun read(intent: Intent?) {
        val args = intent?.getStringArrayExtra("ark.args") ?: return
        val i = args.indexOf("-shots")
        on = i >= 0 && args.getOrNull(i + 1)?.lowercase() in setOf("1", "yes", "true")
    }
}

/** Whether the screen is what it will stay, said to the driver. Any thread may tell it. */
public object Ready {
    private val main by lazy { Handler(Looper.getMainLooper()) }
    private val lock = Any()
    private val waiting = mutableMapOf<String, Int>()
    private var generation = 0
    private var route = ""
    private var quiet = 0
    private var ticking = false
    private var view: View? = null
    /** Whether the window has drawn since the last frame ([attach]'s draw listener). */
    @Volatile private var drawn = false
    /** Frames running in which the window drew with nothing waited for. */
    private var drawing = 0
    /** How many such frames are a drawing with no end (about a second). */
    private const val ENDLESS = 60

    /** `what` is on its way (true), or has come (false): not ready while any is. */
    public fun waiting(what: String, on: Boolean) {
        if (!Shots.on) return
        synchronized(lock) {
            if (on) waiting[what] = (waiting[what] ?: 0) + 1
            else waiting[what]?.let { n -> if (n > 1) waiting[what] = n - 1 else waiting.remove(what) }
        }
        changed()
    }

    /** Whether the app's own work is done, each asked on every frame while it waits, by name. */
    private val probes = mutableMapOf<String, () -> Boolean>()

    /**
     * `idle` asked on each frame, until it says so, whenever the screen may have changed: the
     * app's runtime idle (`Runtime::is_idle`), say. A newer probe of the same name replaces it.
     */
    public fun probe(name: String, idle: () -> Boolean) {
        if (!Shots.on) return
        synchronized(lock) { probes[name] = idle }
        changed()
    }

    /** `what` is on its way exactly while `on`: a state said each time, not counted. */
    public fun still(what: String, on: Boolean) {
        if (!Shots.on) return
        val moved = synchronized(lock) { if (on) waiting.put(what, 1) == null else waiting.remove(what) != null }
        if (moved) changed()
    }

    /** The app is at `route` now. */
    public fun at(route: String) {
        if (!Shots.on) return
        val moved = synchronized(lock) { (this.route != route).also { this.route = route } }
        if (moved) changed()
    }

    /** What is drawn may have changed: not ready, a newer generation, each frame checked. */
    public fun changed() {
        if (!Shots.on) return
        if (Looper.myLooper() != Looper.getMainLooper()) {
            main.post { changed() }
            return
        }
        synchronized(lock) {
            generation += 1
            quiet = 0
            drawing = 0
        }
        say(null)
        if (!ticking) {
            ticking = true
            Choreographer.getInstance().postFrameCallback(::frame)
        }
    }

    private fun frame(@Suppress("UNUSED_PARAMETER") nanos: Long) {
        // Whether the window drew since the last frame: Compose draws only while something on it
        // changes (a sheet still finding its place, a picture just come), so two frames running
        // with nothing drawn are a screen at its end. One that draws on and on with nothing
        // waited for (a spinner, a blinking caret) has no end: after a second of it, not waited
        // for either.
        val drew = drawn
        drawn = false
        val asked = synchronized(lock) { probes.values.toList() }
        val busy = asked.any { !it() }
        val label =
            synchronized(lock) {
                if (waiting.isNotEmpty() || busy) {
                    quiet = 0
                    drawing = 0
                    null
                } else if (drew && drawing < ENDLESS) {
                    quiet = 0
                    drawing += 1
                    null
                } else {
                    quiet += 1
                    if (quiet > 1) "ark.ready ready $generation $route" else null
                }
            }
        if (label == null) {
            Choreographer.getInstance().postFrameCallback(::frame)
            return
        }
        ticking = false
        say(label)
    }

    private fun say(label: String?) {
        // Said to the log too, which the driver reads as it comes (`adb logcat -s ArkReady`).
        android.util.Log.i("ArkReady", label?.removePrefix("ark.ready ") ?: "busy $generation $route")
        val v = view ?: return
        v.contentDescription = label
        v.importantForAccessibility = if (label == null) View.IMPORTANT_FOR_ACCESSIBILITY_NO else View.IMPORTANT_FOR_ACCESSIBILITY_YES
    }

    /**
     * The signal's view on `window` (on its decor, beside the content, which `setContent`
     * replaces), and every touch on it said: [watchForShots]'s.
     */
    internal fun attach(window: Window) {
        if (!Shots.on) return
        val decor = window.decorView as? ViewGroup ?: return
        val v = View(decor.context)
        decor.addView(v, FrameLayout.LayoutParams(1, 1))
        view = v
        decor.viewTreeObserver.addOnDrawListener { drawn = true }
        // The system's navigation hidden (the gesture handle; on a tablet or an open Fold, the
        // launcher's taskbar): neither is the app's, and both differed between two runs' shots,
        // the handle's shade sampled from what is under it and the taskbar holding whatever apps
        // the launcher predicted, in its own order. The status bar stays (demo mode holds it).
        if (Build.VERSION.SDK_INT >= 30) decor.post { hideNavigation(window) }
        val inner = window.callback
        window.callback =
            object : Window.Callback by inner {
                override fun dispatchTouchEvent(event: MotionEvent?): Boolean {
                    // Down, up or cancelled (not each move): each may change what is drawn.
                    if (event != null && event.actionMasked != MotionEvent.ACTION_MOVE) changed()
                    return inner.dispatchTouchEvent(event)
                }
            }
        changed()
    }
}

/** [window]'s navigation bars hidden until swiped back (Android 11's insets controller). */
@android.annotation.TargetApi(30)
private fun hideNavigation(window: Window) {
    window.insetsController?.let {
        it.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        it.hide(WindowInsets.Type.navigationBars())
    }
}

/**
 * This activity, in a screenshot's launch (`-shots 1`): [Shots.on] read from its intent and the
 * ready signal on its window. Called before `setContent`; nothing at all on any other launch.
 */
public fun ComponentActivity.watchForShots() {
    Shots.read(intent)
    Ready.attach(window)
}
