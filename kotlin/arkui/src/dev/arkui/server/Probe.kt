// The server step's question, asked: each URL an address could mean, in
// order, until one answers `/healthz`. The first answer decides — an
// `https` server that answers and is the wrong app is not tried again in
// the clear — and when none answers, the sentence names the host and the
// last reason. The request is a parameter, so the order and the sentences
// are held by plain JVM tests; [Probe.http] is `HttpURLConnection`, which
// Android and the JVM both have.
package dev.arkui.server

import java.io.IOException
import java.net.ConnectException
import java.net.HttpURLConnection
import java.net.NoRouteToHostException
import java.net.SocketTimeoutException
import java.net.URI
import java.net.UnknownHostException
import javax.net.ssl.SSLException

/** What a request came back with. */
public data class Answer(val status: Int, val body: String)

/** Where the server step ended. */
public sealed class Found {
    /** The base URL to sign in at and sync with. */
    public data class Server(val url: String, val newer: Boolean) : Found()

    public data class Failed(val why: String) : Found()
}

public object Probe {
    /** Ask `/healthz` of each URL `typed` could mean (`Address`), with `get` making the request. */
    public fun find(typed: String, domain: String, app: String, get: (String) -> Answer): Found {
        val urls =
            when (val a = Address.parse(typed)) {
                is Address.Invalid -> return Found.Failed(a.why)
                is Address.Candidates -> a
            }
        var last: IOException? = null
        for (url in urls.urls) {
            val answer =
                try {
                    get("$url/healthz")
                } catch (e: IOException) {
                    last = e
                    continue
                }
            return when (val h =
                Health.decide(display(url), answer.status, answer.body, domain, app)) {
                is Health.Serving -> Found.Server(url, h.newer)
                is Health.Refused -> Found.Failed(h.why)
            }
        }
        val over = if (urls.urls.size > 1) " over https or http" else ""
        return Found.Failed("Couldn't reach ${display(urls.urls.first())}$over: ${reason(last)}.")
    }

    /** A URL as a person would write it: the host and port. */
    public fun display(url: String): String = url.substringAfter("://")

    /** Why a request failed, in words. */
    public fun reason(e: IOException?): String =
        when (e) {
            is UnknownHostException -> "there is no such name"
            is ConnectException -> "nothing answered there"
            is NoRouteToHostException -> "this phone has no way to it"
            is SocketTimeoutException -> "it took too long to answer"
            is SSLException -> "its certificate isn't one this phone trusts"
            null -> "no answer"
            else -> e.message ?: e.javaClass.simpleName
        }

    /** A GET asking for JSON, ten seconds to connect and ten to answer. */
    public fun http(url: String): Answer {
        val c = URI(url).toURL().openConnection() as HttpURLConnection
        c.connectTimeout = 10_000
        c.readTimeout = 10_000
        c.useCaches = false
        c.instanceFollowRedirects = true
        c.setRequestProperty("Accept", "application/json")
        try {
            val status = c.responseCode
            val stream = if (status / 100 == 2) c.inputStream else c.errorStream
            val body = stream?.use { String(it.readBytes(), Charsets.UTF_8) } ?: ""
            return Answer(status, body)
        } finally {
            c.disconnect()
        }
    }
}
