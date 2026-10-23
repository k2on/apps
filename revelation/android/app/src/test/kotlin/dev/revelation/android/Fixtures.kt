// What the JVM tests share: the real text, parsed once, and a measurer that
// stands in for Paint without a canvas.
package dev.revelation.android

import java.io.File

object Fixtures {
    /** The asset the app ships, read from the source tree (the tests run in the module's directory). */
    val bible: Bible by lazy { File("src/main/assets/${Bible.ASSET}").inputStream().use(Bible::load) }

    /**
     * A stand-in for `Paint.measureText` at 18 dp: a width per character, narrower for the narrow
     * letters and wider for the capitals, so that lines break at different places for different words.
     */
    val measurer = Measurer { text, start, end ->
        var w = 0f
        for (i in start until end) {
            w += when (text[i]) {
                'i', 'l', 'j', 't', 'f', '.', ',', ';', ':', '\'', '’', '!', ' ' -> 4.5f
                'm', 'w', 'M', 'W' -> 13.5f
                in 'A'..'Z' -> 11.5f
                else -> 8.5f
            }
        }
        w
    }

    /** A Pixel 5 in portrait: 393 by 851 dp at 2.75 pixels per dp. */
    const val WIDTH_DP = 392.7
    const val HEIGHT_DP = 850.9
    const val DENSITY = 2.75

    val params: LayoutParams = LayoutParams.forScreen(WIDTH_DP.toFloat())

    val table: LineTable by lazy { LineTable.build(bible, params, measurer) }

    fun camera(): Camera = Camera(HEIGHT_DP, table.height.toDouble())

    fun frame(camera: Camera) = Levels.Frame(table, camera, DENSITY, WIDTH_DP * DENSITY)
}
