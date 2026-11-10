// The surface at each level of zoom, and the chrome's places, in the app's
// one dark look, at a phone's size: what a change to the drawing is reviewed by.
//
// Paparazzi renders each on the JVM through layoutlib, the renderer Android
// Studio's previews use, so nothing here needs a phone or an emulator. The
// surface is the app's own BibleView over the real text, laid out with the
// real Paint (layoutlib's), inside the same Compose shell the app shows, so
// the header says what the app would say. The six camera states are named
// so they sort from reading size out to the single line:
//
//     01_reading        Genesis 1 at reading size
//     02_small_glyphs   John 1 with a text line 6 px tall: glyphs at their smallest
//     03_greeked        Psalm 23 with lines 2.5 px tall: each line a bar
//     04_chapters       Isaiah 40 with lines half a pixel: the column, a rule at each chapter
//     05_books          Matthew at the centre, chapters a few pixels: a rule at each long book
//     06_line           the whole Bible, the single line
//
// and the chrome after them (07 on). Each is named `-dark`.
package dev.revelation.android

import android.view.View.MeasureSpec
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.fillMaxSize
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import org.junit.Rule
import org.junit.Test

class Screens {
    @get:Rule val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true)

    private val bible = Fixtures.bible
    private fun book(name: String) = bible.bookNames.indexOf(name)
    private fun verse(name: String, chapter: Int, verse: Int = 1) = bible.verseIndex(Ref(book(name), chapter, verse))

    /** The scale at which a text line is [h] pixels tall on this phone (27 dp at 2.75 px per dp). */
    private fun lines(h: Double) = h / (27 * 2.75)

    /** The surface laid out at the phone's size with the camera at [anchor], before Compose takes it. */
    private fun surface(anchor: BibleView.Anchor): BibleView {
        val v = BibleView(paparazzi.context)
        v.synchronousLayout = true
        v.setBible(bible, anchor)
        val w = paparazzi.context.resources.displayMetrics.widthPixels
        val h = paparazzi.context.resources.displayMetrics.heightPixels
        v.measure(MeasureSpec.makeMeasureSpec(w, MeasureSpec.EXACTLY), MeasureSpec.makeMeasureSpec(h, MeasureSpec.EXACTLY))
        v.layout(0, 0, w, h)
        return v
    }

    private fun camera(name: String, anchor: BibleView.Anchor) {
        val v = surface(anchor)
        val place = v.place()
        paparazzi.snapshot("$name-dark") {
            RevelationTheme { RevelationApp(v, bible, place) }
        }
    }

    private fun chrome(name: String, content: @Composable () -> Unit) {
        paparazzi.snapshot("$name-dark") {
            RevelationTheme {
                Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) { content() }
            }
        }
    }

    @Test
    fun a_surface() {
        camera("01_reading", BibleView.Anchor(0, 0.0, 1.0))
        camera("02_small_glyphs", BibleView.Anchor(verse("John", 1), 4000.0, lines(6.0)))
        camera("03_greeked", BibleView.Anchor(verse("Psalm", 23), 0.0, lines(2.5)))
        camera("04_chapters", BibleView.Anchor(verse("Isaiah", 40), 0.0, lines(0.5)))
        camera("05_books", BibleView.Anchor(verse("Matthew", 1), 0.0, 0.0007))
        camera("06_line", BibleView.Anchor(0, 0.0, 0.0))
    }

    @Test
    fun b_chrome() {
        val psalm23 = verse("Psalm", 23)
        chrome("07_picker") { Picker(bible, psalm23, {}, {}) }
        chrome("08_picker_chapters") { Picker(bible, psalm23, {}, {}, book = book("Psalm")) }
        chrome("09_search") { SearchPanel(bible, {}, {}, initial = "shepherd", focus = false) }
        chrome("10_search_reference") { SearchPanel(bible, {}, {}, initial = "john 3:16", focus = false) }
        val v = surface(BibleView.Anchor(verse("Psalm", 23), 0.0, 1.0))
        val place = v.place()
        paparazzi.snapshot("11_about-dark") {
            RevelationTheme { RevelationApp(v, bible, place, start = Overlay.ABOUT) }
        }
    }
}
