// What this phone remembers that is nobody else's business: the last few
// searches, and whether diagnostics have been unlocked. In the app's private
// preferences, never in the log — neither is worth replaying on another
// device, and neither should be. What was played last is not here any more:
// it is the log's (`record_play`, `last_played`) and the listening
// session's, the same on every device (docs/plan-live.md).
//
// And what this phone is called in somebody else's device picker.
package dev.harken.android

import android.content.Context
import android.os.Build
import android.provider.Settings

class Device(context: Context) {
    private val prefs = context.getSharedPreferences("harken-device", Context.MODE_PRIVATE)

    init {
        // What an older build kept here, now the log's.
        if (prefs.contains(LAST)) prefs.edit().remove(LAST).apply()
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

    companion object {
        private const val LAST = "last-played"
        private const val SEARCHES = "searches"
        private const val DIAGNOSTICS = "diagnostics"
        private const val RECENT = 8

        /**
         * What this phone is called in a device picker: the name its owner gave it in the system's
         * settings ("Ana's Pixel"), else its model, else "Phone". Nothing is guessed beyond what the
         * system says.
         */
        fun name(context: Context): String =
            runCatching { Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME) }.getOrNull()?.takeIf { it.isNotBlank() }
                ?: Build.MODEL?.takeIf { it.isNotBlank() }
                ?: "Phone"
    }
}
