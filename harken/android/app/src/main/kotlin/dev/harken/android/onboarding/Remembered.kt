// What the onboarding remembers, so that it is asked once: the server, the
// login, and which permissions have been asked for.
//
// The server and the permissions over a [Shelf], a key/value store of
// strings: the app's private `SharedPreferences` on a phone (`PrefsShelf`), a
// map in a test. The login in the secure store (ark-secure-store: the Android
// keystore), through the bridge's `LoginKeep` (`ark_auth::keep`), kept whole
// as `ark_auth::Login`'s JSON so a restart reuses the token; one that no
// longer parses is forgotten rather than trusted, and one an older build kept
// on the shelf under `ark.login` is moved there once and deleted. Nothing
// here touches the replica, which lives in a directory of its own per server.
package dev.harken.android.onboarding

import dev.arkdb.auth.Remembered
import dev.arkdb.ffi.LoginKeep
import dev.arkdb.ffi.OldShelf
import uniffi.ark_secure_store.SecureStore

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

public class Kept(private val shelf: Shelf, store: SecureStore?) {
    private val login = LoginKeep(store, OldKeys(shelf), LOGIN)

    /** The server's base URL and the login there, as the sign-in machine last asked them written. */
    public var remembered: Remembered
        get() = Remembered(shelf.get(SERVER), login.recall(), false)
        set(r) {
            shelf.put(SERVER, r.server)
            login.keep(r.login)
        }

    /** The permissions already asked for, by [Permission.key], whatever the answer was. */
    public val asked: Set<String>
        get() = shelf.get(ASKED)?.split('\n')?.filter { it.isNotEmpty() }?.toSet() ?: emptySet()

    public fun markAsked(key: String) {
        shelf.put(ASKED, (asked + key).sorted().joinToString("\n"))
    }

    private companion object {
        const val SERVER = "ark.server"
        /** Where an older build kept the login, moved from once. */
        const val LOGIN = "ark.login"
        const val ASKED = "ark.asked"
    }
}

/** A [Shelf] as where a login used to be kept: read once, then removed. */
public class OldKeys(private val shelf: Shelf) : OldShelf {
    override fun get(key: String): String? = shelf.get(key)

    override fun remove(key: String): Unit = shelf.put(key, null)
}
