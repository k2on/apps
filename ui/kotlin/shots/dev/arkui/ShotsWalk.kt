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

    private fun run(plan: File, ui: UiModeManager) {
        val out = File(plan.parentFile, "out")
        out.deleteRecursively()
        out.mkdirs()
        val log = StringBuilder()
        val say = { line: String ->
            android.util.Log.i("ArkShots", line)
            log.append(line).append('\n')
        }
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
        File(out, "walk.log").writeText(log.toString())
        File(out, if (failed == null) "done" else "failed").writeText(failed ?: "")
        Process.killProcess(Process.myPid())
    }

    private fun walk(plan: File, out: File, ui: UiModeManager, say: (String) -> Unit) {
        val p = JSONObject(plan.readText())
        val shots = p.getJSONArray("shots")
        val one = p.optString("theme").takeIf { it.isNotEmpty() }
        var dark = false
        theme(ui, dark)
        settled("the first screen")
        for (i in 0 until shots.length()) {
            val shot = shots.getJSONObject(i)
            val id = shot.getString("id")
            val steps = shot.optJSONArray("steps")
            val n = steps?.length() ?: 0
            var t = System.currentTimeMillis()
            for (k in 0 until n) take(steps!!.getJSONObject(k).toString(), id)
            val stepped = System.currentTimeMillis() - t
            val tree = this.steps?.tree() ?: ""
            val times = mutableListOf<String>()
            var first: Bitmap? = null
            for (pass in if (one != null) 0..0 else 0..1) {
                if (pass == 1) {
                    // The other theme, as the system's switch would set it: no relaunch.
                    t = System.currentTimeMillis()
                    dark = !dark
                    theme(ui, dark)
                    settled("$id in ${if (dark) "dark" else "light"}")
                    times += "switched ${System.currentTimeMillis() - t} ms"
                }
                t = System.currentTimeMillis()
                val picture = picture() ?: throw Stop("$id: no window to draw")
                if (first != null && first.sameAs(picture)) throw Stop("$id: dark and light drew the same picture")
                first = picture
                val theme = if (dark) "dark" else "light"
                for (t in if (one != null) listOf("light", "dark") else listOf(theme)) {
                    val dir = File(out, t).apply { mkdirs() }
                    File(dir, "$id.png").outputStream().use { picture.compress(Bitmap.CompressFormat.PNG, 100, it) }
                    File(dir, "$id.tree.txt").writeText(tree)
                }
                times += "$theme picture ${System.currentTimeMillis() - t} ms"
            }
            say("$id: $n steps $stepped ms, ${times.joinToString(", ")}")
        }
    }

    /** One step, once what it names is on the screen, then the screen waited for. */
    private fun take(json: String, id: String) {
        val by = this.steps ?: throw Stop("$id: $json: this app takes no steps")
        val until = System.currentTimeMillis() + FIND
        while (true) {
            val g = Ready.generation()
            when (val took = by.step(json)) {
                is Took.Done -> {
                    settled("$id after $json")
                    return
                }
                is Took.NotYet -> {
                    if (System.currentTimeMillis() > until) throw Stop("$id: nothing on the screen for $json in ${FIND / 1000} s; there was:\n${took.labels}")
                    // Looked for again once the screen has changed (or a moment on).
                    Ready.awaitReady(g, 500)
                }
                is Took.Failed -> throw Stop("$id: $json: ${took.why}")
            }
        }
    }

    /** Until the app says its screen is ready at a newer generation. */
    private fun settled(what: String) {
        val g = Ready.generation()
        Ready.changed()
        if (!Ready.awaitReady(g, SETTLE)) throw Stop("$what: not ready in ${SETTLE / 1000} s (${Ready.why})")
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

    /** Every window of the app's, drawn as the screen shows them, at the screen's size. */
    private fun picture(): Bitmap? {
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
