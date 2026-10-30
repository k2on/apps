// What a person types into the server field, and the URLs it could mean.
//
// One field takes `host`, `host:port` or a whole URL. A URL with a scheme
// is taken at its word — somebody who typed `http://` meant it. Anything
// else is tried over `https` first, and over `http` as well only where the
// address is one a certificate is unlikely to cover and nobody outside can
// reach: loopback, the private ranges, link-local, the carrier-grade range
// a tailnet uses, and a name with no dot or one under `.local`, `.lan`,
// `.home.arpa` or `.internal`. A public name is never tried in the clear.
//
// Kept free of Android, so that it is held by plain JVM tests.
package dev.harken.android.onboarding

/** What the server field came to: the URLs to try, in order, or why there are none. */
public sealed class Address {
    /** Base URLs with no trailing slash, `https` first. */
    public data class Candidates(val urls: List<String>, val host: String) : Address()

    public data class Invalid(val why: String) : Address()

    public companion object {
        /** The URLs `typed` could mean, in the order to try them. */
        public fun parse(typed: String): Address {
            val text = typed.trim()
            if (text.isEmpty()) return Invalid("Type the address of your server.")
            if (text.any { it.isWhitespace() }) return Invalid("An address has no spaces in it.")
            val scheme = text.substringBefore("://", "")
            val rest = if (text.contains("://")) text.substringAfter("://") else text
            if (text.contains("://") && scheme.lowercase() !in setOf("http", "https"))
                return Invalid("A server is reached over http or https, not $scheme.")
            val authority = rest.substringBefore('/').substringBefore('?').substringBefore('#')
            val path =
                rest.removePrefix(authority).substringBefore('?').substringBefore('#').trimEnd('/')
            if (authority.isEmpty() || authority.contains('@'))
                return Invalid("That isn't an address a server can have: $text")
            val host =
                hostOf(authority)
                    ?: return Invalid("That isn't an address a server can have: $text")
            val port = authority.removePrefix(if (authority.startsWith("[")) "[$host]" else host)
            if (port.isNotEmpty()) {
                val n = port.removePrefix(":").toIntOrNull()
                if (!port.startsWith(":") || n == null || n !in 1..65535)
                    return Invalid("The port has to be a number from 1 to 65535.")
            }
            val base = authority.lowercase() + path
            val urls =
                when {
                    scheme.isNotEmpty() -> listOf(scheme.lowercase() + "://" + base)
                    isLocal(host) -> listOf("https://$base", "http://$base")
                    else -> listOf("https://$base")
                }
            return Candidates(urls, host.lowercase())
        }

        // The host of `host`, `host:port`, `[v6]` or `[v6]:port`, or null when
        // it has characters no host has.
        private fun hostOf(authority: String): String? {
            if (authority.startsWith("[")) {
                val end = authority.indexOf(']')
                if (end < 0) return null
                val v6 = authority.substring(1, end)
                return v6.takeIf {
                    it.isNotEmpty() && it.all { c -> c.isLetterOrDigit() || c == ':' || c == '.' }
                }
            }
            val h = authority.substringBefore(':')
            if (h.isEmpty()) return null
            val ok =
                h.all {
                    it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '-' || it == '.'
                }
            return h.takeIf { ok && !it.startsWith('.') && !it.startsWith('-') }
        }

        /**
         * Whether `host` is one nobody outside this network reaches, where a server is as likely to
         * speak plain http as https.
         */
        public fun isLocal(host: String): Boolean {
            val h = host.lowercase().trimEnd('.')
            if (h == "localhost" || h.endsWith(".localhost")) return true
            if (!h.contains('.') && !h.contains(':')) return true
            if (listOf(".local", ".lan", ".home.arpa", ".internal").any { h.endsWith(it) })
                return true
            if (h.contains(':')) {
                return h == "::1" ||
                    h.startsWith("fc") ||
                    h.startsWith("fd") ||
                    h.startsWith("fe80:")
            }
            val parts = h.split('.').map { it.toIntOrNull() ?: return false }
            if (parts.size != 4 || parts.any { it !in 0..255 }) return false
            val (a, b) = parts
            return a == 127 ||
                a == 10 ||
                (a == 172 && b in 16..31) ||
                (a == 192 && b == 168) ||
                (a == 169 && b == 254) ||
                (a == 100 && b in 64..127)
        }
    }
}
