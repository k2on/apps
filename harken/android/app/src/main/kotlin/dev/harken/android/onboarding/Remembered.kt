// What the onboarding remembers, so that it is asked once: the server, the
// login, and which permissions have been asked for.
//
// Over a [Shelf], a key/value store of strings: the app's private
// `SharedPreferences` on a phone (`PrefsShelf`), a map in a test. The login
// is kept whole as `ark_auth::Login`'s JSON ([Logins]), so a restart reuses
// the token; one that no longer parses is forgotten rather than trusted. Signing out forgets the login and keeps the server; nothing here
// touches the replica, which lives in a directory of its own per server.
package dev.harken.android.onboarding

import dev.arkdb.ffi.Login
import org.json.JSONArray
import org.json.JSONObject

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

public class Remembered(private val shelf: Shelf) {
    /** The server's base URL, as the server step found it. */
    public var server: String?
        get() = shelf.get(SERVER)
        set(v) = shelf.put(SERVER, v)

    /** The login, or null — including when what was kept no longer parses. */
    public var login: Login?
        get() =
            shelf.get(LOGIN)?.let {
                try {
                    Logins.parse(it)
                } catch (e: Exception) {
                    null
                }
            }
        set(v) = shelf.put(LOGIN, v?.let(Logins::json))

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

/** A login as `ark_auth::Login`'s JSON: the token, the session, and the account. */
public object Logins {
    public fun json(l: Login): String =
        JSONObject()
            .put("token", l.token)
            .put("session", l.session)
            .put(
                "user",
                JSONObject()
                    .put("id", l.user)
                    .put("name", l.name)
                    .put("email", l.email)
                    .put("picture", l.picture)
                    .put("roles", JSONArray(l.roles)))
            .put("expires_ms", l.expiresMs)
            .toString()

    public fun parse(s: String): Login {
        val o = JSONObject(s)
        val u = o.getJSONObject("user")
        val roles = u.optJSONArray("roles")
        return Login(
            token = o.getString("token"),
            session = o.getString("session"),
            user = u.getString("id"),
            name = u.optString("name"),
            email = u.optString("email"),
            roles = List(roles?.length() ?: 0) { roles!!.getString(it) },
            expiresMs = o.optLong("expires_ms"),
            picture = u.optString("picture"),
        )
    }
}
