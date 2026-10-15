// Nothing makes a scroll state but `ark-ui`'s helpers (docs/plan-android.md,
// "Where a destination's state lives"). A scan of the sources, not a lint
// rule: every Kotlin file under harken's app and under ark-ui, read as text.
//
// - No `rememberLazyListState(`, `rememberLazyGridState(`,
//   `rememberLazyStaggeredGridState(` or `rememberScrollState(`: those keep a
//   place by where in the tree they are called from, which is the bug this
//   closes. A destination's container takes `rememberDestination…State`; a
//   sheet's, `rememberTransient…State`.
// - No state made by hand (`LazyListState(`, `ScrollState(` …), outside
//   ark-ui's Destination.kt, which is where the helpers make theirs.
// - No lazy list or grid without a `state =`: its default is
//   `rememberLazyListState()`, the same bug with nothing to scan for.
//
// Comments are not code: a line's `//` tail and `/* … */` blocks are taken
// out first.
package dev.harken.android

import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Test

class SourceTests {
    private fun repo(): File {
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            if (File(dir, "kotlin/ark-ui").isDirectory && File(dir, "harken/android/app").isDirectory) return dir
            dir = dir.parentFile
        }
        error("no repository above ${File("").absolutePath}")
    }

    /** Every file the rule holds. */
    private fun sources(): List<File> {
        val r = repo()
        return listOf("harken/android/app/src/main", "kotlin/ark-ui/src/main").flatMap { d ->
            File(r, d).walkTopDown().filter { it.isFile && it.extension == "kt" }.toList()
        }
    }

    private fun code(f: File): String =
        f.readText().replace(Regex("/\\*.*?\\*/", RegexOption.DOT_MATCHES_ALL), "").lines().joinToString("\n") { line ->
            // A `//` outside a string: good enough for this tree, where no string holds one.
            val i = line.indexOf("//")
            if (i >= 0 && !line.substring(0, i).contains('"')) line.substring(0, i) else line
        }

    /** What `f` breaks, as messages. */
    fun broken(name: String, text: String): List<String> {
        val out = mutableListOf<String>()
        for (call in listOf("rememberLazyListState(", "rememberLazyGridState(", "rememberLazyStaggeredGridState(", "rememberScrollState(")) {
            if (Regex("(?<![A-Za-z0-9_])" + Regex.escape(call)).containsMatchIn(text)) out += "$name calls $call"
        }
        if (!name.endsWith("ark-ui/src/main/kotlin/dev/arkdb/ui/Destination.kt")) {
            for (made in listOf("LazyListState(", "LazyGridState(", "LazyStaggeredGridState(", "ScrollState(")) {
                if (Regex("(?<![A-Za-z0-9_.])" + Regex.escape(made)).containsMatchIn(text)) out += "$name makes a $made…)"
            }
        }
        for (lazy in listOf("LazyColumn", "LazyRow", "LazyVerticalGrid", "LazyHorizontalGrid", "LazyVerticalStaggeredGrid", "LazyHorizontalStaggeredGrid")) {
            for (m in Regex("(?<![A-Za-z0-9_])" + lazy + "\\s*\\(").findAll(text)) {
                val args = balanced(text, m.range.last)
                if (!Regex("(?<![A-Za-z0-9_])state\\s*=").containsMatchIn(args)) out += "$name: a $lazy with no `state =` (${args.take(60).replace('\n', ' ')}…)"
            }
        }
        return out
    }

    /** What is between the `(` at `open` and the `)` that closes it. */
    private fun balanced(text: String, open: Int): String {
        var depth = 0
        for (i in open until text.length) {
            when (text[i]) {
                '(' -> depth++
                ')' -> if (--depth == 0) return text.substring(open + 1, i)
            }
        }
        return text.substring(open + 1)
    }

    @Test
    fun nothingMakesAScrollStateButTheHelpers() {
        val files = sources()
        assertTrue("the scan found the sources: ${files.size}", files.size > 20)
        val found = files.flatMap { broken(it.invariantSeparatorsPath, code(it)) }
        assertTrue(found.joinToString("\n"), found.isEmpty())
    }

    @Test
    fun theScanSeesWhatItIsFor() {
        // Each of the three, said the way a screen would.
        assertTrue(broken("a.kt", "val list = rememberLazyListState()").isNotEmpty())
        assertTrue(broken("a.kt", "Column(Modifier.verticalScroll(rememberScrollState()))").isNotEmpty())
        assertTrue(broken("a.kt", "val s = remember { LazyListState() }").isNotEmpty())
        assertTrue(broken("a.kt", "LazyColumn(Modifier.fillMaxSize()) { item {} }").isNotEmpty())
        assertTrue(broken("a.kt", "LazyColumn(\n  Modifier.fillMaxSize(),\n  contentPadding = p,\n) { item {} }").isNotEmpty())
        // And not what it is not.
        assertTrue(broken("a.kt", "LazyColumn(Modifier.fillMaxSize(), state = rememberDestinationListState(\"x\")) {}").isEmpty())
        assertTrue(broken("a.kt", "val s = rememberDestinationScrollState(\"x\")").isEmpty())
    }
}
