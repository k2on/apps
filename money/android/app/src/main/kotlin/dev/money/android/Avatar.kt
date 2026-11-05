// Who is signed in, as a face: their initials on the accent, and their
// picture over them once it has arrived, as harken's Avatar draws it. The
// picture is the server's copy of it (`/auth/picture`, ark-auth's
// `picture_url`), fetched once and kept decoded in memory: a login holds the
// provider's address, which a phone could not always load (a dev server's is
// a file on that machine).
package dev.money.android

import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Who is signed in, round, `side` across: `picture` (a whole URL, empty for none) once it has
 * loaded, cropped to fill, over the initials of `name` on the accent. A screen reader hears who
 * is signed in and nothing else.
 */
@Composable
fun Avatar(name: String, picture: String = "", side: Dp = 32.dp) {
    val c = MaterialTheme.colorScheme
    Box(
        Modifier.clearAndSetSemantics { contentDescription = "Signed in as $name" }.size(side).clip(CircleShape).background(c.primary),
        contentAlignment = Alignment.Center,
    ) {
        Text(Faces.initials(name).ifEmpty { name.take(1) }, color = c.onPrimary, fontSize = (side.value * 0.4f).sp, style = MaterialTheme.typography.titleSmall)
        if (picture.isNotEmpty()) {
            val px = with(LocalDensity.current) { side.roundToPx() }
            var bitmap by remember(picture) { mutableStateOf(Faces.cached(picture)) }
            if (bitmap == null) LaunchedEffect(picture, px) { bitmap = Faces.load(picture, px) }
            bitmap?.let { Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize()) }
        }
    }
}

/** Faces: their letters, where their picture is, and the pictures already fetched. */
object Faces {
    // A few faces is all money ever shows: whoever is signed in, and whoever was before. Made
    // when a picture is first asked for, so the JVM's tests of the rest need no Android.
    private val memory by lazy { LruCache<String, ImageBitmap>(8) }

    /**
     * What a face says without a picture: the first letters of the name's first and last words
     * (`Ada Lovelace` is `AL`, `Ada` is `A`), upper case, as harken's `Who.initials` has it.
     */
    fun initials(name: String): String {
        fun first(w: String): Char? = w.firstOrNull { it.isLetterOrDigit() }
        val words = name.split(Regex("\\s+")).filter { first(it) != null }
        val letters = when (words.size) {
            0 -> emptyList()
            1 -> listOfNotNull(first(words[0]))
            else -> listOfNotNull(first(words.first()), first(words.last()))
        }
        return letters.joinToString("") { it.uppercase() }
    }

    /**
     * Where this phone loads `user`'s picture from: `server`'s `/auth/picture`, with the login's
     * `picture` as its version (`v`), so a new picture is a new address — ark-auth's
     * `picture_url`, letter for letter. Empty when there is no picture or no server.
     */
    fun url(server: String, user: String, picture: String): String {
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

    /** A picture already decoded, without waiting. */
    fun cached(url: String): ImageBitmap? = memory.get(url)

    /**
     * The picture at `url`, about `side` pixels across, fetched and decoded off the main thread;
     * none when it cannot be had. `/auth/picture` answers anybody, so no login goes with it.
     */
    suspend fun load(url: String, side: Int): ImageBitmap? =
        memory.get(url) ?: withContext(Dispatchers.IO) {
            runCatching {
                    val http = URL(url).openConnection() as HttpURLConnection
                    http.connectTimeout = 10_000
                    http.readTimeout = 10_000
                    try {
                        if (http.responseCode != 200) null else decoded(http.inputStream.use { it.readBytes() }, side)
                    } finally {
                        http.disconnect()
                    }
                }
                .getOrNull()
                ?.also { memory.put(url, it) }
        }
}
