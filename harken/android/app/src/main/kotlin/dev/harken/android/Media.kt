// Where a track's bytes are. How a length is said is ark_format's
// (`formatClock`, `formatLength`), every client's.
//
// A track's `file` is a path under the server's media root, and `/media/`
// serves that same path back, with range requests and no token — so every
// device joins it to its own server, as the desktop's `media_url` does
// (`harken::listening::url`): each segment percent-encoded, because a `#` or
// a `?` in a file name would otherwise end the path mid-name. A whole URL
// is already an answer and passes through. Kept free of Android.
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

}
