// Where a client loads a signed-in person's picture from: their server's
// `/auth/picture`, with the login's own picture as its version, as ark-auth's
// `picture_url` builds it (rust/ark-auth/src/util.rs), letter for letter. A
// login holds the provider's address, which a phone cannot always load: a dev
// server's is a file on that machine, and a provider's may need the server to
// fetch it. Every app's face is loaded through this.
package dev.arkui

public object Pictures {
    /** `user`'s picture on `server`, versioned by the login's `picture`; empty for none. */
    public fun url(server: String, user: String, picture: String): String {
        if (picture.isEmpty() || server.isEmpty()) return ""
        // FNV-1a: a version, not a secret.
        var h = 0x811c9dc5.toInt()
        for (b in picture.toByteArray(Charsets.UTF_8)) h = (h xor (b.toInt() and 0xff)) * 0x01000193
        return server.trimEnd('/') + "/auth/picture?user=" + encode(user) + "&v=" + "%08x".format(h)
    }

    /** A query value, encoded as ark-auth encodes one: everything but the unreserved characters. */
    private fun encode(s: String): String {
        val out = StringBuilder()
        for (b in s.toByteArray(Charsets.UTF_8)) {
            val c = (b.toInt() and 0xff).toChar()
            if (c in 'A'..'Z' || c in 'a'..'z' || c in '0'..'9' || c in "-_.~") out.append(c) else out.append("%%%02X".format(b.toInt() and 0xff))
        }
        return out.toString()
    }
}
