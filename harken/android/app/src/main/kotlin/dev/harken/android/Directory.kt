// Finding a show to follow: Apple's podcast directory, searched by what is
// typed, or a feed's own address pasted in — the desktop's
// (`harken/iced/src/directory.rs`).
//
// The directory is asked from the phone, not through the server: what is
// searched for is nobody else's business. What comes back is a list of
// shows with their feeds — following one is the same `library.subscribe` a
// pasted address is. A search waits a moment after the last key ([PAUSE]),
// so a word typed is one request rather than one a letter; the sheet that
// asks drops an answer for words it no longer says.
//
// No dependency: `HttpURLConnection`, as the covers are fetched, and the
// platform's `org.json`, as the generated bindings read.
package dev.harken.android

import java.net.HttpURLConnection
import java.net.URI
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONException
import org.json.JSONObject

object Directory {
    /** How long the typing has to stop before a search goes, in milliseconds. */
    const val PAUSE = 300L

    /** The most shows one search lists. */
    private const val LIMIT = 25

    /** A show the directory found: what it is called, who makes it, its feed and its picture's address. */
    data class Found(val title: String, val author: String, val feed: String, val art: String)

    /** Whether what was typed is a feed's address rather than words to search for. */
    fun isFeed(typed: String): Boolean {
        val t = typed.trim()
        return t.startsWith("http://") || t.startsWith("https://")
    }

    /** The directory's search for `words`, podcasts only. */
    fun searchUrl(words: String): String =
        "https://itunes.apple.com/search?media=podcast&entity=podcast&limit=$LIMIT&term=" + Places.encode(words.trim())

    /**
     * The shows the sheet lists for what was typed: a feed's address is the one show it names;
     * words, what the directory found for them.
     */
    fun rows(typed: String, found: List<Found>): List<Found> =
        if (isFeed(typed)) listOf(Found(typed.trim(), "", typed.trim(), "")) else found

    /** Whether the library already follows the feed `show` is. */
    fun following(show: Found, feeds: List<Subscription>): Boolean = feeds.any { it.url == show.feed }

    /** What the directory answered, as shows: each that has a feed, which is every one that can be followed. */
    fun parse(json: String): Result<List<Found>> =
        try {
            val results = JSONObject(json).getJSONArray("results")
            val found = ArrayList<Found>()
            for (i in 0 until results.length()) {
                val o = results.optJSONObject(i) ?: continue
                val feed = text(o, "feedUrl")
                if (feed.isEmpty()) continue
                found.add(Found(text(o, "collectionName"), text(o, "artistName"), feed, text(o, "artworkUrl600").ifEmpty { text(o, "artworkUrl100") }))
            }
            // A show listed twice is one row: the sheet's rows are keyed by their feeds.
            Result.success(found.distinctBy { it.feed })
        } catch (e: JSONException) {
            Result.failure(Exception("the directory said something else: ${e.message}"))
        }

    // A field's text; missing or null, none.
    private fun text(o: JSONObject, key: String): String = if (o.isNull(key)) "" else o.optString(key)

    /** Search the directory for `words`, off the main thread: the shows, or why there are none. */
    suspend fun search(words: String): Result<List<Found>> =
        withContext(Dispatchers.IO) {
            val body =
                try {
                    get(searchUrl(words))
                } catch (e: Exception) {
                    return@withContext Result.failure(Exception("could not search: ${e.message ?: e.javaClass.simpleName}"))
                }
            parse(body)
        }

    // The body of a GET; anything but a 2xx is a failure.
    private fun get(url: String): String {
        val c = URI(url).toURL().openConnection() as HttpURLConnection
        c.connectTimeout = 10_000
        c.readTimeout = 20_000
        try {
            if (c.responseCode / 100 != 2) throw java.io.IOException("HTTP ${c.responseCode}")
            return c.inputStream.bufferedReader().use { it.readText() }
        } finally {
            c.disconnect()
        }
    }
}
