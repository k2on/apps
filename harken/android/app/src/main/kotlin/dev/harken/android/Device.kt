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
         * What this phone is called in a device picker, as a person would call it ([DeviceName]):
         * the name its owner gave it in the system's settings ("Ana's Pixel") or its Bluetooth name,
         * else its maker and model ("Pixel 7"), else "Android Emulator" on an emulator.
         */
        fun name(context: Context): String {
            val cr = context.contentResolver
            return DeviceName.of(
                given =
                    listOf(
                        runCatching { Settings.Global.getString(cr, Settings.Global.DEVICE_NAME) }.getOrNull(),
                        runCatching { Settings.Secure.getString(cr, "bluetooth_name") }.getOrNull(),
                    ),
                manufacturer = Build.MANUFACTURER,
                model = Build.MODEL,
                build = listOf(Build.FINGERPRINT, Build.PRODUCT, Build.HARDWARE),
            )
        }
    }
}

/**
 * A phone's name as the iPhone's reads ("iPhone 17 Pro"), from what the system says of it. A name
 * somebody gave it comes first, unless it is a build code nobody would call a phone by
 * ("sdk_gphone64_x86_64"); then the maker and the model, the model alone where it already says the
 * maker or is a Pixel; an emulator is "Android Emulator".
 */
object DeviceName {
    private val CODES = listOf("sdk", "generic", "emulator", "gphone")

    fun of(given: List<String?>, manufacturer: String?, model: String?, build: List<String?> = emptyList()): String {
        given.map { it?.trim().orEmpty() }.firstOrNull { it.isNotEmpty() && !raw(it) }?.let { return it }
        val m = model?.trim().orEmpty()
        if (emulator(build + m) || (m.isNotEmpty() && raw(m) && CODES.any { m.lowercase().startsWith(it) })) return "Android Emulator"
        val maker = manufacturer?.trim().orEmpty().let { capitalised(it) }
        val named = tidied(m)
        return when {
            named.isEmpty() -> maker.ifEmpty { "Phone" }
            named.startsWith("Pixel", ignoreCase = true) -> capitalised(named)
            maker.isEmpty() || named.startsWith(maker, ignoreCase = true) -> capitalised(named)
            else -> "$maker $named"
        }
    }

    /** Whether `name` is a build code rather than a name: underscores, an emulator's prefix, or all one lower-case word. */
    fun raw(name: String): Boolean {
        val n = name.trim()
        val low = n.lowercase()
        return '_' in n || CODES.any { low.startsWith(it) } || (' ' !in n && n == low && n.any { it.isLetter() })
    }

    /** Whether the build says it is an emulator's. */
    fun emulator(build: List<String?>): Boolean =
        build.any { b -> b != null && b.lowercase().let { it.contains("emulator") || it.contains("generic") || it.startsWith("sdk") || it.contains("/sdk") || it.contains("sdk_") || it.contains("ranchu") || it.contains("goldfish") } }

    // A build code's underscores as spaces.
    private fun tidied(model: String): String = model.replace('_', ' ').replace(Regex("\\s+"), " ").trim()

    private fun capitalised(s: String): String = s.replaceFirstChar { if (it.isLowerCase()) it.titlecase() else it.toString() }
}
