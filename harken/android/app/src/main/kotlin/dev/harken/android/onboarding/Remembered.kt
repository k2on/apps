// What the onboarding remembers, so that it is asked once: the server, the
// login, and which permissions have been asked for.
//
// Over a [Shelf], a key/value store of strings: the app's private
// `SharedPreferences` on a phone (`PrefsShelf`), a map in a test. The login
// is kept whole as `ark_auth::Login`'s JSON (`loginJson`, the bridge's), so
// a restart reuses the token; one that no longer parses is forgotten rather
// than trusted. Nothing here touches the replica, which lives in a
// directory of its own per server.
package dev.harken.android.onboarding

import dev.arkdb.auth.Remembered
import dev.arkdb.auth.loginJson
import dev.arkdb.auth.loginParse

/** A key/value store of strings; `null` removes. */
public interface Shelf {
    public fun get(key: String): String?

    public fun put(key: String, value: String?)
}

/** A [Shelf] in memory, for tests and previews. */
public class MemoryShelf : Shelf {
    public val map: MutableMap<String, String> = LinkedHashMap()

    override fun get(key: String): String? = map[key]

    override fun put(key: String, value: String?) {
        if (value == null) map.remove(key) else map[key] = value
    }
}

public class Kept(private val shelf: Shelf) {
    /** The server's base URL and the login there, as the sign-in machine last asked them written. */
    public var remembered: Remembered
        get() = Remembered(shelf.get(SERVER), shelf.get(LOGIN)?.let { loginParse(it) }, false)
        set(r) {
            shelf.put(SERVER, r.server)
            shelf.put(LOGIN, r.login?.let { loginJson(it) })
        }

    /** The permissions already asked for, by [Permission.key], whatever the answer was. */
    public val asked: Set<String>
        get() = shelf.get(ASKED)?.split('\n')?.filter { it.isNotEmpty() }?.toSet() ?: emptySet()

    public fun markAsked(key: String) {
        shelf.put(ASKED, (asked + key).sorted().joinToString("\n"))
    }

    private companion object {
        const val SERVER = "ark.server"
        const val LOGIN = "ark.login"
        const val ASKED = "ark.asked"
    }
}
