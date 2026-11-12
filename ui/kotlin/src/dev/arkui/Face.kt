// Who is signed in, as a face: their initials on the accent, and their
// picture over them once it has arrived. One face for every app — money's
// account face and harken's draw this, the same size in the same place —
// so a person looks the same whichever app they opened. The picture is the
// server's copy of it (`Pictures.url`, ark-auth's `picture_url`), fetched once
// and kept decoded in memory: `/auth/picture` answers anybody, so no login
// goes with it.
package dev.arkui

import android.graphics.BitmapFactory
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
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.net.HttpURLConnection
import java.net.URL
import kotlin.math.max
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** How large the account's face is, at the end of a tab's title, in every app. */
public val FaceSide: Dp = 32.dp

/**
 * Who is signed in, round, [side] across: [picture] (a whole URL, empty for none) once it has
 * loaded, cropped to fill, over the initials of [name] on the accent. A screen reader hears
 * [description] — who is signed in, by default — and nothing else.
 */
@Composable
public fun Face(
    name: String,
    picture: String = "",
    side: Dp = FaceSide,
    description: String = "Signed in as $name",
    modifier: Modifier = Modifier,
) {
    val c = MaterialTheme.colorScheme
    Box(
        modifier.clearAndSetSemantics { contentDescription = description }.size(side).clip(CircleShape).background(c.primary),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            Faces.initials(name).ifEmpty { name.take(1) },
            color = c.onPrimary,
            fontSize = (side.value * 0.4f).sp,
            fontWeight = FontWeight.SemiBold,
        )
        if (picture.isNotEmpty()) {
            val px = with(LocalDensity.current) { side.roundToPx() }
            var bitmap by remember(picture) { mutableStateOf(Faces.cached(picture)) }
            if (bitmap == null)
                LaunchedEffect(picture, px) {
                    // A screenshot waits until it has come, or failed (Ready, ui/docs/shots.md).
                    Ready.waiting("face", true)
                    try {
                        bitmap = Faces.load(picture, px)
                    } finally {
                        Ready.waiting("face", false)
                    }
                }
            bitmap?.let { Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize()) }
        }
    }
}

/** Faces: their letters, and the pictures already fetched. */
public object Faces {
    // A few faces is all an app ever shows: whoever is signed in, and whoever was before. Made
    // when a picture is first asked for.
    private val memory by lazy { LruCache<String, ImageBitmap>(8) }

    /**
     * What a face says without a picture: the first letters of the name's first and last words
     * (`Ada Lovelace` is `AL`, `Ada` is `A`), upper case.
     */
    public fun initials(name: String): String {
        fun first(w: String): Char? = w.firstOrNull { it.isLetterOrDigit() }
        val words = name.split(Regex("\\s+")).filter { first(it) != null }
        val letters =
            when (words.size) {
                0 -> emptyList()
                1 -> listOfNotNull(first(words[0]))
                else -> listOfNotNull(first(words.first()), first(words.last()))
            }
        return letters.joinToString("") { it.uppercase() }
    }

    /** A picture already decoded, without waiting. */
    public fun cached(url: String): ImageBitmap? = memory.get(url)

    /** The picture at [url], about [side] pixels across, fetched off the main thread; none when it cannot be had. */
    public suspend fun load(url: String, side: Int): ImageBitmap? =
        memory.get(url)
            ?: withContext(Dispatchers.IO) {
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

    /** [bytes] as a bitmap no wider or taller than about [side]: sampled down as it is decoded. */
    private fun decoded(bytes: ByteArray, side: Int): ImageBitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0) return null
        var sample = 1
        while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= side) sample *= 2
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample })
            ?.asImageBitmap()
    }
}
