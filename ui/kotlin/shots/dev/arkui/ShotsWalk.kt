// A screenshot walk, taken inside the app (ui/docs/shots.md, "In the App"): a launch with
// `-shots 1` that finds a walk's plan in the app's own directory, on an emulator, walks it and
// photographs itself, then ends its process. The script that wrote the plan
// (.github/screens/app-android.sh) pulls the pictures.
//
// The plan, `<external files>/shots/plan.json` (`/sdcard/Android/data/<package>/files/shots`, the
// app's own, which only it and adb write):
//
//   {"shots": [{"id": "welcome", "steps": []}, {"id": "budget", "steps": [{"tap": …}, …]}, …]}
//
// `"theme": "dark"` in it: an app that draws one theme whatever the system's (revelation's page,
// always dark), each screen taken once and written as both.
//
// Each shot's steps are taken by the app's [ShotsWalk.Steps] (an Ark app's: Rust's,
// `ArkHost.shotsStep`, which reaches what a step names in the views on the screen and sends the
// message a tap on it sends); after each the walk waits for the app's own ready signal ([Ready]).
// Then the screen is taken in the theme it is in, the theme switched (the app's own night mode,
// as the system's switch would set it: no relaunch), and taken again; the next shot starts in
// the theme this one ended in. Each picture lands in `shots/out/<theme>/<id>.png`, what a step
// can name beside it (`<id>.tree.txt`), each shot's times in `shots/out/walk.log`, and
// `shots/out/done` (or `failed`, why) once it is over.
//
// The picture is the app's own windows, drawn by the app: the activity's and any dialog's or
// popup's over it (PixelCopy of each window, the dim behind a dialog drawn as the system would).
// The status bar is the system's, not the app's, and is not in it, as the iPhone's walk leaves
// it out too.
package dev.arkui

import android.app.Activity
import android.app.UiModeManager
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.view.PixelCopy
import android.view.View
import android.view.WindowManager
import android.view.inspector.WindowInspector
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONObject

public object ShotsWalk {
    /** What a step came to: done; not yet there (what was, `labels`); or why it cannot be. */
    public sealed interface Took {
        public object Done : Took
        public class NotYet(public val labels: String) : Took
        public class Failed(public val why: String) : Took
    }

    /** How an app takes a walk's steps: an Ark app's are Rust's (dev.arkui.app.ArkApp). */
    public interface Steps {
        /** One step, as shots.json writes it (JSON), against the screen now. */
        public fun step(json: String): Took

        /** What is on the screen, as a step names it. */
        public fun tree(): String

        /** The app put back where a walk starts once signed in, for the next walk. */
        public fun reset(): Took = Took.Done
    }

    /** The app's steps, said before its activity's window is watched; none: a walk of no steps. */
    @Volatile public var steps: Steps? = null

    /** How long a step waits for what it names, and for the screen after it: guards. */
    private const val FIND = 20_000L
    private const val SETTLE = 60_000L

    @Volatile private var started = false
    private val main by lazy { Handler(Looper.getMainLooper()) }

    /** Whether this is an emulator: a phone in a person's hand never walks. */
    private val emulator: Boolean
        get() = Build.HARDWARE in setOf("ranchu", "goldfish") || Build.PRODUCT.startsWith("sdk_")

    /** The plan this launch walks: a screenshot's launch, on an emulator, with a plan in its own directory. */
    public fun planned(context: Context): File? {
        if (!Shots.on || !emulator) return null
        val f = File(context.getExternalFilesDir("shots") ?: return null, "plan.json")
        return f.takeIf { it.isFile }
    }

    /** The walk started, once a process, when this launch is one. */
    internal fun start(activity: Activity) {
        if (started) return
        val plan = planned(activity) ?: return
        started = true
        val ui = activity.getSystemService(UiModeManager::class.java)
        Thread({ run(plan, ui) }, "ShotsWalk").start()
    }

    private class Stop(val why: String) : Exception(why)

    /** The walk's log, as it goes: Android's (`ArkShots`) and walk.log. */
    private val written = StringBuilder()

    private fun log(line: String) {
        android.util.Log.i("ArkShots", line)
        synchronized(written) { written.append(line).append('\n') }
    }

    private fun run(plan: File, ui: UiModeManager) {
        val out = File(plan.parentFile, "out")
        out.deleteRecursively()
        out.mkdirs()
        val say: (String) -> Unit = { line -> log(line) }
        val failed =
            try {
                walk(plan, out, ui, say)
                null
            } catch (e: Stop) {
                e.why
            } catch (e: Exception) {
                e.toString()
            }
        say(failed?.let { "failed: $it" } ?: "done")
        File(out, "walk.log").writeText(synchronized(written) { written.toString() })
        File(out, if (failed == null) "done" else "failed").writeText(failed ?: "")
        Process.killProcess(Process.myPid())
    }

    private fun walk(plan: File, out: File, ui: UiModeManager, say: (String) -> Unit) {
        val p = JSONObject(plan.readText())
        val shots = (0 until p.getJSONArray("shots").length()).map { p.getJSONArray("shots").getJSONObject(it) }
        val one = p.optString("theme").takeIf { it.isNotEmpty() }
        // The ways a screen is taken, in the order the walks go: light, then dark (a one-theme
        // app's: one).
        val ways = if (one != null) listOf(false) else listOf(false, true)
        var dark = false
        theme(ui, dark)
        settled("the first screen")
        val go = { to: Boolean ->
            val t = System.currentTimeMillis()
            dark = to
            theme(ui, dark)
            settled(if (dark) "dark" else "light")
            System.currentTimeMillis() - t
        }
        val drawn = mutableMapOf<String, MutableMap<Boolean, Bitmap>>()
        val shoot = { id: String, tree: String ->
            val t = System.currentTimeMillis()
            val picture = picture() ?: throw Stop("$id: no window to draw")
            // The theme really switched: the app's own window drawn in it at its top (its bar, or
            // its page's own background), not left in the last. A one-theme app's is its own.
            val top = topLightness
            if (one == null && top != null && (top < 0.5) != dark) {
                val drawnAs = if (top < 0.5) "dark" else "light"
                val way = if (dark) "dark" else "light"
                throw Stop("$id $way: its window's top is drawn $drawnAs (${"%.2f".format(top)}): the theme did not switch")
            }
            drawn.getOrPut(id) { mutableMapOf() }[dark] = picture
            for (th in if (one != null) listOf("light", "dark") else listOf(if (dark) "dark" else "light")) {
                val dir = File(out, th).apply { mkdirs() }
                File(dir, "$id.png").outputStream().use { picture.compress(Bitmap.CompressFormat.PNG, 100, it) }
                File(dir, "$id.tree.txt").writeText(tree)
            }
            "${System.currentTimeMillis() - t} ms${top?.let { ", top %.2f".format(it) } ?: ""}"
        }
        // The shots before the sign-in's (its shot says `signIn`): the first run's pages, each
        // taken every way at the start, then back to the first.
        val first = shots.indexOfFirst { it.optBoolean("signIn") }.coerceAtLeast(0)
        for (shot in shots.subList(0, first)) {
            val id = shot.getString("id")
            val steps = shot.optJSONArray("steps")
            for (k in 0 until (steps?.length() ?: 0)) take(steps!!.getJSONObject(k).toString(), id)
            val tree = this.steps?.tree() ?: ""
            val times = mutableListOf<String>()
            ways.forEachIndexed { k, w ->
                if (k > 0) times += "switched ${go(w)} ms"
                times += "${if (dark) "dark" else "light"} picture ${shoot(id, tree)}"
            }
            say("$id: ${times.joinToString(", ")}")
        }
        if (dark != ways[0]) say("back to light ${go(ways[0])} ms")
        // Then the rest, walked whole once a way: the sign-in's steps on the first walk alone;
        // before each walk after, the app put back where the walk starts (`ArkHost.shotsReset`),
        // then the next way switched.
        ways.forEachIndexed { k, w ->
            val began = System.currentTimeMillis()
            if (k > 0) {
                val r = System.currentTimeMillis()
                when (val took = this.steps?.reset() ?: Took.Done) {
                    is Took.Done -> {}
                    is Took.NotYet -> throw Stop("reset: ${took.labels}")
                    is Took.Failed -> throw Stop("reset: ${took.why}")
                }
                settled("reset")
                val reset = System.currentTimeMillis() - r
                say("reset $reset ms, switched ${go(w)} ms")
            }
            for (shot in shots.subList(first, shots.size)) {
                val id = shot.getString("id")
                val all = shot.optJSONArray("steps")
                val n = if (k > 0 && shot.optBoolean("signIn")) 0 else all?.length() ?: 0
                val t = System.currentTimeMillis()
                val each = (0 until n).map { take(all!!.getJSONObject(it).toString(), id) }
                val stepped = System.currentTimeMillis() - t
                val tree = this.steps?.tree() ?: ""
                val took = shoot(id, tree)
                val way = if (dark) "dark" else "light"
                say("$id $way: $n steps $stepped ms [${each.joinToString("; ")}], picture $took")
                // Within a walk, two shots of one picture: a step that went nowhere.
                val mine = drawn[id]?.get(dark)
                val same = shots.subList(first, shots.size).map { it.getString("id") }.firstOrNull { it != id && drawn[it]?.get(dark)?.sameAs(mine) == true }
                if (same != null) throw Stop("$id and $same drew the same picture ($way)")
            }
            say("the walk ${if (dark) "dark" else "light"}: ${System.currentTimeMillis() - began} ms")
        }
        // Each screen's two themes apart: one the same as the other did not switch.
        for ((id, by) in drawn) {
            val l = by[false]
            val d = by[true]
            if (l != null && d != null && l.sameAs(d)) throw Stop("$id: light and dark drew the same picture")
        }
    }

    /**
     * One step, then the screen waited for: its times, Rust's and the ready signal's. What it names
     * not there at a ready screen fails at once (it will not come); while the screen is on its way
     * (a sign-in, a sync) it is looked for again each time the screen is ready anew.
     */
    private fun take(json: String, id: String): String {
        val by = this.steps ?: throw Stop("$id: $json: this app takes no steps")
        val began = System.currentTimeMillis()
        val until = began + FIND
        while (true) {
            val g = Ready.generation()
            val r = System.currentTimeMillis()
            val took = by.step(json)
            val rust = System.currentTimeMillis() - r
            when (took) {
                is Took.Done -> {
                    val w = System.currentTimeMillis()
                    settled("$id after $json")
                    return "$json $rust ms + ready ${System.currentTimeMillis() - w} ms"
                }
                is Took.NotYet -> {
                    if (Ready.isReady(g) || System.currentTimeMillis() > until) {
                        throw Stop("$id: nothing on the ready screen is $json (${System.currentTimeMillis() - began} ms); there was:\n${took.labels}")
                    }
                    Ready.awaitReady(g, FIND)
                }
                is Took.Failed -> throw Stop("$id: $json: ${took.why}")
            }
        }
    }

    /** Until the app says its screen is ready at a newer generation. */
    private fun settled(what: String) {
        val g = Ready.generation()
        val t = System.currentTimeMillis()
        Ready.trace()
        Ready.changed()
        if (!Ready.awaitReady(g, SETTLE)) throw Stop("$what: not ready in ${SETTLE / 1000} s (${Ready.why})")
        val held = Ready.held().filter { it.second != "quiet" }
        if (held.isNotEmpty()) log("  $what: ready in ${System.currentTimeMillis() - t} ms, held by: ${held.joinToString(" | ") { "${it.first} ms ${it.second}" }}")
    }

    /** The app drawn dark or light: its own night mode, which makes its activity again in it. */
    private fun theme(ui: UiModeManager, dark: Boolean) {
        if (Build.VERSION.SDK_INT >= 31) {
            ui.setApplicationNightMode(if (dark) UiModeManager.MODE_NIGHT_YES else UiModeManager.MODE_NIGHT_NO)
        } else {
            throw Stop("a walk switches the theme with Android 12's own night mode")
        }
    }

    /** `f` on the main thread, its answer here. */
    private fun <T> onMain(f: () -> T): T {
        var r: Result<T>? = null
        val done = CountDownLatch(1)
        main.post {
            r = runCatching(f)
            done.countDown()
        }
        if (!done.await(10, TimeUnit.SECONDS)) throw Stop("the main thread did not answer")
        return r!!.getOrThrow()
    }

    /** How light the app's own window is at its top in the last [picture] (0 black, 1 white). */
    private var topLightness: Double? = null

    /** The median lightness of `b`'s top `rows` rows. */
    private fun lightness(b: Bitmap, rows: Int): Double? {
        val h = rows.coerceAtMost(b.height)
        if (h <= 0 || b.width <= 0) return null
        val px = IntArray(b.width * h)
        b.getPixels(px, 0, b.width, 0, 0, b.width, h)
        val grey = px.map { (Color.red(it) * 299 + Color.green(it) * 587 + Color.blue(it) * 114) / 1000 }.sorted()
        return grey[grey.size / 2] / 255.0
    }

    /** Every window of the app's, drawn as the screen shows them, at the screen's size. */
    private fun picture(): Bitmap? {
        topLightness = null
        if (Build.VERSION.SDK_INT < 29) throw Stop("a walk finds the app's windows with Android 10's WindowInspector")
        val roots =
            onMain {
                WindowInspector.getGlobalWindowViews().filter { it.isAttachedToWindow && it.isShown && it.width > 0 && it.height > 0 }
            }
        val main = onMain { Ready.window?.decorView } ?: return null
        val (w, h) = onMain { main.width to main.height }
        val out = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(out)
        canvas.drawColor(Color.BLACK)
        for (v in roots) {
            val at = IntArray(2)
            val lp = onMain {
                v.getLocationOnScreen(at)
                v.layoutParams as? WindowManager.LayoutParams
            }
            // What the system dims behind a dialog, as it dims it.
            if (lp != null && lp.flags and WindowManager.LayoutParams.FLAG_DIM_BEHIND != 0) {
                canvas.drawColor(Color.argb((lp.dimAmount * 255).toInt(), 0, 0, 0))
            }
            val drawn = window(v) ?: continue
            if (v === main) topLightness = lightness(drawn, (24 * v.resources.displayMetrics.density).toInt())
            canvas.drawBitmap(drawn, at[0].toFloat(), at[1].toFloat(), null)
        }
        return out
    }

    /** One window's picture: its surface as it was composed (Android 14's PixelCopy), else drawn. */
    private fun window(v: View): Bitmap? {
        if (Build.VERSION.SDK_INT >= 34) {
            var got: Bitmap? = null
            val done = CountDownLatch(1)
            onMain {
                PixelCopy.request(PixelCopy.Request.Builder.ofWindow(v).build(), { it.run() }) { r ->
                    if (r.status == PixelCopy.SUCCESS) got = r.bitmap
                    done.countDown()
                }
            }
            if (done.await(10, TimeUnit.SECONDS) && got != null) return got
        }
        return onMain {
            val b = Bitmap.createBitmap(v.width, v.height, Bitmap.Config.ARGB_8888)
            v.draw(Canvas(b))
            b
        }
    }
}
