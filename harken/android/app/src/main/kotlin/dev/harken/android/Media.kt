// Where a track's bytes are, and how a length is said.
//
// A track's `file` is a path under the server's media root, and `/media/`
// serves that same path back, with range requests and no token — so every
// device joins it to its own server, as the desktop's `media_url` does
// (`harken::listening::url`): each segment percent-encoded, because a `#` or
// a `?` in a file name would otherwise end the path mid-name. A whole URL
// is already an answer and passes through. Kept free of Android, and held
// by `MediaTests`.
package dev.harken.android

object Media {
    /** The URL `file` is at on `server`. */
    fun url(server: String, file: String): String {
        if (file.isEmpty() || file.startsWith("http://") || file.startsWith("https://")) return file
        val out = StringBuilder(server.trimEnd('/')).append("/media")
        for (part in file.split('/')) {
            out.append('/')
            for (b in part.toByteArray(Charsets.UTF_8)) {
                val c = b.toInt() and 0xff
                val ch = c.toChar()
                if (ch in 'A'..'Z' || ch in 'a'..'z' || ch in '0'..'9' || ch in "-_.~") out.append(ch)
                else out.append('%').append("%02X".format(c))
            }
        }
        return out.toString()
    }

    /**
     * A track's length as a row shows it: `m:ss`, or `h:mm:ss` past an hour. Spelt by hand rather
     * than by `String.format`, which parses its pattern on every call: every row entering a list
     * asks for one.
     */
    fun clock(ms: Long): String {
        val s = ms.coerceAtLeast(0) / 1000
        val out = StringBuilder(8)
        if (s >= 3600) two(out.append(s / 3600).append(':'), s / 60 % 60) else out.append(s / 60)
        return two(out.append(':'), s % 60).toString()
    }

    private fun two(out: StringBuilder, n: Long): StringBuilder = out.append(if (n < 10) "0" else "").append(n)

    /** A record's length, as a header says it: `48 min`, `2 hr 30 min`. */
    fun spell(ms: Long): String {
        val min = (ms + 30_000) / 60_000
        return when {
            min < 1 -> "under a minute"
            min < 60 -> "$min min"
            min % 60 == 0L -> "${min / 60} hr"
            else -> "${min / 60} hr ${min % 60} min"
        }
    }

    /** `1 track`, `12 tracks`. */
    fun count(n: Int, one: String, many: String = one + "s"): String = "$n ${if (n == 1) one else many}"
}
