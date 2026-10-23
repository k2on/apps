// The activity: the text parsed off the main thread, the surface made once,
// the Compose shell around it, and the camera kept on the phone's own disk.
package dev.revelation.android

import android.content.Context
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

class MainActivity : ComponentActivity() {
    private lateinit var surface: BibleView
    private var bible by mutableStateOf<Bible?>(null)
    private var place by mutableStateOf<BibleView.Place?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        surface = BibleView(this)
        surface.onPlace = { place = it }
        val saved = Saved.load(this)
        val loaded = Text.bible
        if (loaded != null) {
            show(loaded, saved)
        } else {
            // Four megabytes parsed once per process, off the main thread; the surface is the bare
            // ground until it lands, a few tens of milliseconds.
            Thread({
                val b = assets.open(Bible.ASSET).use(Bible::load)
                Text.bible = b
                runOnUiThread { show(b, saved) }
            }, "text").start()
        }
        setContent { RevelationTheme { RevelationApp(surface, bible, place) } }
    }

    private fun show(b: Bible, saved: BibleView.Anchor?) {
        bible = b
        surface.setBible(b, saved)
    }

    override fun onPause() {
        super.onPause()
        surface.anchor()?.let { Saved.store(this, it) }
    }

    /** The parsed text, kept for the process, so an activity made again does not parse it again. */
    private object Text {
        @Volatile var bible: Bible? = null
    }

    /** The camera, in SharedPreferences: the verse under the centre, the offset from its first line, the scale. */
    private object Saved {
        private const val FILE = "camera"

        fun load(context: Context): BibleView.Anchor? {
            val p = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            if (!p.contains("verse")) return null
            return BibleView.Anchor(p.getInt("verse", 0), p.getFloat("offset", 0f).toDouble(), p.getFloat("scale", 1f).toDouble())
        }

        fun store(context: Context, a: BibleView.Anchor) {
            context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit()
                .putInt("verse", a.verse)
                .putFloat("offset", a.offset.toFloat())
                .putFloat("scale", a.scale.toFloat())
                .apply()
        }
    }
}
