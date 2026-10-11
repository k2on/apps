// Who a peer is, and how it gets a token to say so (`rust/ark-auth`'s client
// half, `client.rs` and `util.rs`).
//
// The server is the only OpenID Connect client. A peer opens one URL —
// `/auth/login?redirect=R` — and receives one single-use code at `R`, which it
// trades at `/auth/exchange` for a `Login`: the token every `Hello` carries,
// the session its entries are authored under, and who it is. In dev mode, with
// a name given, the server answers the first request with the redirect itself
// and no page: that is the whole of a headless peer's sign-in, and of a phone
// in this repository.
//
// `login` is the desktop's flow, as `ark_auth::client::login_with` is: a
// loopback port as `R`, a browser opened on whatever page the server sends
// (`open`), and the code read off the request the browser makes back. Over
// `HttpURLConnection`, which the JVM and Android both have, and a JSON reader
// the runtime already has: no dependency.
package dev.arkdb.client

import dev.arkdb.Json
import dev.arkdb.Value
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.ServerSocket
import java.net.URL

/** A signed-in person, as the server knows them (`ark_auth::Account`). */
public data class Account(
    val id: String,
    val name: String = "",
    val email: String = "",
    /** The roles the server says this person holds; what the device's `Ctx` carries. */
    val roles: List<String> = emptyList(),
)

/** What a sign-in hands a client and the client keeps (`ark_auth::Login`). */
public data class Login(
    /** Proves the session; sent in every `Hello`, never in a URL. */
    val token: String,
    /** The session's id, which entries authored under it carry. */
    val session: String,
    val user: Account,
    /** When the token stops working, as milliseconds since the epoch. */
    val expiresMs: Long,
) {
    /** The login as `serde_json` writes `ark_auth::Login`, roles left out when there are none. */
    public fun toJson(): String {
        val u = StringBuilder()
        u.append("{\"id\":").append(Json.quoted(user.id))
        u.append(",\"name\":").append(Json.quoted(user.name))
        u.append(",\"email\":").append(Json.quoted(user.email))
        if (user.roles.isNotEmpty())
            u.append(",\"roles\":").append(Json.array(user.roles.map(Json::quoted)))
        u.append("}")
        return "{\"token\":${Json.quoted(token)},\"session\":${Json.quoted(session)}," +
            "\"user\":$u,\"expires_ms\":$expiresMs}"
    }

    public companion object {
        /** A login as the server's `/auth/exchange` answers it, or as [toJson] wrote it. */
        public fun of(v: Value): Login {
            val s = v.asStruct()
            val u = s["user"].asStruct()
            fun text(m: Value.VStruct, k: String): String =
                m[k].let { if (it.isNull()) "" else it.asText() }
            return Login(
                token = s["token"].asText(),
                session = s["session"].asText(),
                user =
                    Account(
                        id = u["id"].asText(),
                        name = text(u, "name"),
                        email = text(u, "email"),
                        roles =
                            u["roles"].let { r ->
                                if (r.isNull()) emptyList() else r.asList().map { it.asText() }
                            },
                    ),
                expiresMs = s["expires_ms"].let { if (it.isNull()) 0 else it.asInt() },
            )
        }

        public fun parse(json: String): Login = of(Json.decode(json))
    }
}

/** What a sign-in could not do. */
public class AuthError(what: String) : Exception(what)

/**
 * How long a request waits on the server (`ark_auth::client::Patience`): the connection, and each
 * read. Ten and twenty seconds by default.
 */
public data class Patience(val connectMs: Long = 10_000, val readMs: Long = 20_000) {
    public companion object {
        public val DEFAULT: Patience = Patience()

        /** The same bound on the connection and on each read. */
        public fun of(ms: Long): Patience = Patience(ms, ms)
    }
}

public object Auth {
    /**
     * The URL a client sends a person to (`ark_auth::login_url`): `redirect` is where the code
     * comes back to; `user` is honoured only by a server in dev mode.
     */
    public fun loginUrl(server: String, redirect: String, user: String?): String {
        val sb = StringBuilder(server.trimEnd('/'))
        sb.append("/auth/login?redirect=").append(percentEncode(redirect))
        if (user != null) sb.append("&user=").append(percentEncode(user))
        return sb.toString()
    }

    /** The socket beside the login, `/sync`, with the scheme to match (`ark_auth::socket_url`). */
    public fun socketUrl(server: String): String {
        val s = server.trimEnd('/')
        val socket =
            when {
                s.startsWith("https://") -> "wss://" + s.removePrefix("https://")
                s.startsWith("http://") -> "ws://" + s.removePrefix("http://")
                s.startsWith("ws://") || s.startsWith("wss://") -> s
                else -> "ws://$s"
            }
        return "$socket/sync"
    }

    /** Everything but the unreserved characters, as `%XX` (`ark_auth::percent_encode`). */
    public fun percentEncode(s: String): String {
        val out = StringBuilder()
        for (b in s.toByteArray(Charsets.UTF_8)) {
            val c = b.toInt() and 0xff
            val ch = c.toChar()
            if (ch in 'A'..'Z' || ch in 'a'..'z' || ch in '0'..'9' || ch in "-_.~") out.append(ch)
            else out.append('%').append(String.format("%02X", c))
        }
        return out.toString()
    }

    /** A query value back out of a URL's query (`ark_auth::query_value`). */
    public fun queryValue(query: String, key: String): String? {
        for (pair in query.substringAfter('?').split('&')) {
            val (k, v) = pair.split('=', limit = 2).let { it[0] to it.getOrElse(1) { "" } }
            if (k == key) return percentDecode(v)
        }
        return null
    }

    private fun percentDecode(s: String): String {
        val out = java.io.ByteArrayOutputStream()
        var i = 0
        while (i < s.length) {
            val c = s[i]
            if (c == '%' && i + 2 < s.length) {
                val b = s.substring(i + 1, i + 3).toIntOrNull(16)
                if (b != null && s[i + 1] != '+' && s[i + 1] != '-') {
                    out.write(b)
                    i += 3
                    continue
                }
            }
            if (c == '+') out.write(' '.code)
            else out.write(c.toString().toByteArray(Charsets.UTF_8))
            i++
        }
        return String(out.toByteArray(), Charsets.UTF_8)
    }

    /**
     * Sign in to `server` (`ark_auth::client::login_with`): a dev server with `user` answers at
     * once and no page opens; anything else is a page for a person, handed to [open], and the code
     * is read off the loopback request the server's redirect makes the browser send. Blocking, for
     * as long as the person takes; the server is waited on for [patience] at most.
     */
    public fun login(
        server: String,
        user: String?,
        patience: Patience = Patience.DEFAULT,
        open: (String) -> Unit = {},
    ): Login {
        ServerSocket(0, 1, InetAddress.getLoopbackAddress()).use { listener ->
            val redirect = "http://127.0.0.1:${listener.localPort}/"
            val url = loginUrl(server, redirect, user)
            val c = connection(url, patience)
            c.instanceFollowRedirects = false
            val status =
                try {
                    c.responseCode
                } catch (e: java.io.IOException) {
                    throw AuthError("cannot reach $server: ${e.message ?: e.javaClass.simpleName}")
                }
            val code =
                when (status / 100) {
                    3 -> {
                        val location = c.getHeaderField("Location") ?: ""
                        c.disconnect()
                        if (location.startsWith(redirect))
                            queryValue(location.removePrefix(redirect), "code")
                                ?: throw AuthError("the server sent no code")
                        else {
                            open(location)
                            waitForCode(listener)
                        }
                    }
                    2 -> {
                        c.disconnect()
                        open(url)
                        waitForCode(listener)
                    }
                    else -> throw AuthError("$server answered $status: ${body(c.errorStream)}")
                }
            return exchange(server, code, patience)
        }
    }

    /** The code, for the login it stands for (`ark_auth::client::exchange_with`). */
    public fun exchange(
        server: String,
        code: String,
        patience: Patience = Patience.DEFAULT
    ): Login {
        val c = connection("${server.trimEnd('/')}/auth/exchange", patience)
        c.requestMethod = "POST"
        c.doOutput = true
        c.setRequestProperty("Content-Type", "application/json")
        val sent = "{\"code\":${Json.quoted(code)}}".toByteArray(Charsets.UTF_8)
        try {
            c.outputStream.use { it.write(sent) }
            val status = c.responseCode
            if (status / 100 != 2) throw AuthError(body(c.errorStream))
            val text = body(c.inputStream)
            return try {
                Login.parse(text)
            } catch (e: Exception) {
                throw AuthError("the login did not parse: ${e.message}")
            }
        } catch (e: java.io.IOException) {
            throw AuthError(e.message ?: e.javaClass.simpleName)
        } finally {
            c.disconnect()
        }
    }

    private fun connection(url: String, patience: Patience): HttpURLConnection {
        val c = URL(url).openConnection() as HttpURLConnection
        c.connectTimeout = patience.connectMs.toInt()
        c.readTimeout = patience.readMs.toInt()
        c.useCaches = false
        return c
    }

    private fun body(s: InputStream?): String =
        s?.use { String(it.readBytes(), Charsets.UTF_8) } ?: ""

    // One request on the loopback listener, which the server's redirect makes
    // the browser send: `GET /?code=… HTTP/1.1`.
    private fun waitForCode(listener: ServerSocket): String {
        while (true) {
            listener.accept().use { s ->
                val line = s.getInputStream().bufferedReader(Charsets.UTF_8).readLine() ?: ""
                val path = line.split(' ').getOrElse(1) { "" }
                val code = if ('?' in path) queryValue(path, "code") else null
                val (status, body) =
                    if (code != null) "200 OK" to "signed in — you can close this tab"
                    else "404 Not Found" to "nothing here"
                val bytes = body.toByteArray(Charsets.UTF_8)
                s.getOutputStream().apply {
                    write(
                        ("HTTP/1.1 $status\r\nContent-Type: text/plain; charset=utf-8\r\n" +
                                "Content-Length: ${bytes.size}\r\nConnection: close\r\n\r\n")
                            .toByteArray(Charsets.US_ASCII))
                    write(bytes)
                    flush()
                }
                if (code != null) return code
            }
        }
    }
}
