// What this phone remembers that is nobody else's business: what was
// played last and where it was played from, the last few searches, and
// whether diagnostics have been unlocked. In the app's private
// preferences, never in the log — none of it is worth replaying on
// another device, and none of it should be.
package dev.harken.android

import android.content.Context
import org.json.JSONObject

/** What was playing last, and the page it was started from, so Home can offer it back. */
data class LastPlayed(val track: Playing, val from: Page?)

class Device(context: Context) {
    private val prefs = context.getSharedPreferences("harken-device", Context.MODE_PRIVATE)

    var lastPlayed: LastPlayed?
        get() =
            prefs.getString(LAST, null)?.let {
                try {
                    val j = JSONObject(it)
                    LastPlayed(
                        Playing(j.getString("id"), j.getString("title"), j.getString("artist"), j.optString("album"), j.optString("art")),
                        Page.parse(j.optString("from")),
                    )
                } catch (e: Exception) {
                    null
                }
            }
        set(v) {
            val j =
                v?.let {
                    JSONObject()
                        .put("id", it.track.id)
                        .put("title", it.track.title)
                        .put("artist", it.track.artist)
                        .put("album", it.track.album)
                        .put("art", it.track.art)
                        .put("from", it.from?.route ?: "")
                        .toString()
                }
            prefs.edit().putString(LAST, j).apply()
        }

    /** The last few searches, newest first. */
    var searches: List<String>
        get() = prefs.getString(SEARCHES, null)?.split('\n')?.filter { it.isNotBlank() } ?: emptyList()
        set(v) {
            prefs.edit().putString(SEARCHES, v.joinToString("\n")).apply()
        }

    /** Remember a search, at the top, once. */
    fun searched(needle: String) {
        val n = needle.trim()
        if (n.isEmpty()) return
        searches = (listOf(n) + searches.filter { !it.equals(n, ignoreCase = true) }).take(RECENT)
    }

    /** Whether the version has been tapped seven times. */
    var diagnostics: Boolean
        get() = prefs.getBoolean(DIAGNOSTICS, false)
        set(v) {
            prefs.edit().putBoolean(DIAGNOSTICS, v).apply()
        }

    private companion object {
        const val LAST = "last-played"
        const val SEARCHES = "searches"
        const val DIAGNOSTICS = "diagnostics"
        const val RECENT = 8
    }
}
