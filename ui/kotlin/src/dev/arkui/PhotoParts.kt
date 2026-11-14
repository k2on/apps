// A photo by its URL and a slip of paper (ark_app's `photo`, `zoom_photo` and `paper`,
// ui/docs/parts.md): what a receipt is shown as, its photo whole and the paper it was printed on.
// The iPhone's `Photo` and `Paper` (PhotoParts.swift), measure for measure.
package dev.arkui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import android.util.Base64
import android.util.LruCache
import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLDecoder
import kotlin.math.max
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * A photo by its URL — `file:` (a file this device holds), `data:` (its bytes) or `https:` — fitted
 * to its width, [label] said for it; a spinner while the URL is empty (still being fetched) or the
 * photo is being read. Zoomed ([zoom]), a pinch or a double tap makes it larger, a drag pans it while
 * it is, and past its least and most size it rubber-bands ([Springs.band]) and settles back on
 * [Springs.move] once let go.
 */
@Composable
public fun Photo(url: String, label: String, zoom: Boolean) {
    val picture by produceState(Photos.cached(url), url) { value = Photos.load(url) }
    val p = picture
    val shape = RoundedCornerShape(Space.corner)
    Box(
        Modifier.fillMaxWidth().padding(vertical = Space.m).clip(shape).semantics { contentDescription = label },
        contentAlignment = Alignment.Center) {
            when {
                p == null -> CircularProgressIndicator(Modifier.padding(Space.xl))
                zoom -> Zoomed(p)
                else -> Image(p, contentDescription = null, contentScale = ContentScale.FillWidth, modifier = Modifier.fillMaxWidth())
            }
        }
}

/** A photo that pinches, double-taps and pans. */
@Composable
private fun Zoomed(picture: ImageBitmap) {
    val reduced = reducedMotion()
    val spec = if (reduced) Springs.fade() else Springs.move()
    val scope = rememberCoroutineScope()
    val most = MotionTokens.ZOOM_MAX
    // Where it rests, and what the fingers have made of it.
    val scale = remember { Animatable(1f) }
    val x = remember { Animatable(0f) }
    val y = remember { Animatable(0f) }
    var raw by remember { mutableStateOf(1f) }
    var size by remember { mutableStateOf(IntSize.Zero) }
    fun banded(s: Float): Float =
        when {
            s < 1f -> 1f - Springs.band(1f - s, 0.5f)
            s > most -> most + Springs.band(s - most, most / 2f)
            else -> s
        }
    fun reach(s: Float): Offset = Offset(max(0f, (size.width * s - size.width) / 2f), max(0f, (size.height * s - size.height) / 2f))
    fun settle(to: Float = raw.coerceIn(1f, most), at: Offset? = null) {
        raw = to
        val r = reach(to)
        val o = at ?: Offset(x.value, y.value)
        scope.launch { scale.animateTo(to, spec) }
        scope.launch { x.animateTo(o.x.coerceIn(-r.x, r.x), spec) }
        scope.launch { y.animateTo(o.y.coerceIn(-r.y, r.y), spec) }
    }
    val state =
        rememberTransformableState { zoomBy, pan, _ ->
            raw *= zoomBy
            scope.launch { scale.snapTo(banded(raw)) }
            // Panned one to one while it is larger than it fits.
            if (raw > 1f) scope.launch {
                x.snapTo(x.value + pan.x)
                y.snapTo(y.value + pan.y)
            }
        }
    LaunchedEffect(state.isTransformInProgress) { if (!state.isTransformInProgress) settle() }
    Image(
        picture,
        contentDescription = null,
        contentScale = ContentScale.Fit,
        modifier =
            Modifier.fillMaxWidth()
                .heightIn(min = Space.row * 6)
                .clipToBounds()
                .onSizeChanged { size = it }
                .pointerInput(Unit) { detectTapGestures(onDoubleTap = { settle(if (raw > 1f) 1f else MotionTokens.ZOOM_TAP, Offset.Zero) }) }
                .transformable(state, lockRotationOnZoomPan = true)
                .graphicsLayer {
                    scaleX = scale.value
                    scaleY = scale.value
                    translationX = x.value
                    translationY = y.value
                })
}

/** The photos a URL names, decoded once (at most [SIDE] on their long side, upright) and kept a while. */
internal object Photos {
    private const val SIDE = 2400
    private val memory = LruCache<String, ImageBitmap>(8)

    fun cached(url: String): ImageBitmap? = if (url.isEmpty()) null else memory.get(url)

    suspend fun load(url: String): ImageBitmap? =
        if (url.isEmpty()) null
        else
            memory.get(url)
                ?: withContext(Dispatchers.IO) { runCatching { bytes(url)?.let(::decoded) }.getOrNull()?.also { memory.put(url, it) } }

    private fun bytes(url: String): ByteArray? =
        when {
            url.startsWith("file://") -> File(URLDecoder.decode(url.removePrefix("file://").replace("+", "%2B"), "UTF-8")).readBytes()
            url.startsWith("data:") -> url.substringAfter(";base64,", "").let { Base64.decode(it, Base64.DEFAULT) }
            url.startsWith("https://") -> {
                val http = URL(url).openConnection() as HttpURLConnection
                http.connectTimeout = 10_000
                http.readTimeout = 10_000
                try {
                    if (http.responseCode != 200) null else http.inputStream.use { it.readBytes() }
                } finally {
                    http.disconnect()
                }
            }
            else -> null
        }

    private fun decoded(bytes: ByteArray): ImageBitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0) return null
        var sample = 1
        while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= SIDE) sample *= 2
        val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample }) ?: return null
        val degrees =
            when (
                runCatching { ExifInterface(bytes.inputStream()).getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL) }
                    .getOrDefault(ExifInterface.ORIENTATION_NORMAL)) {
                ExifInterface.ORIENTATION_ROTATE_90 -> 90f
                ExifInterface.ORIENTATION_ROTATE_180 -> 180f
                ExifInterface.ORIENTATION_ROTATE_270 -> 270f
                else -> 0f
            }
        val up: Bitmap =
            if (degrees == 0f) bmp else Bitmap.createBitmap(bmp, 0, 0, bmp.width, bmp.height, Matrix().apply { postRotate(degrees) }, true)
        return up.asImageBitmap()
    }
}

/** The paper's ink: a receipt is white whatever the phone's theme, so dark on it in both. */
private val PaperInk = Color(0xFF1C1C1C)
private val PaperFaded = PaperInk.copy(alpha = 0.55f)

/**
 * A slip of paper (a receipt as it was printed): monospace, dark on white in light and dark alike;
 * its heading bold and its date centred, a rule, each line's name with its figure against the right
 * and its note under it, a rule, and the total.
 */
@Composable
public fun Paper(heading: String, date: String, names: List<String>, figures: List<String>, notes: List<String>, total: String) {
    val mono = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp, color = PaperInk)
    val shape = RoundedCornerShape(Space.corner)
    Column(
        Modifier.fillMaxWidth()
            .padding(vertical = Space.m)
            .shadow(6.dp, shape)
            .background(Color.White, shape)
            .padding(Space.l)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(Space.xs)) {
            Text(heading, style = mono.copy(fontWeight = FontWeight.Bold, textAlign = TextAlign.Center), modifier = Modifier.fillMaxWidth())
            Text(date, style = mono.copy(textAlign = TextAlign.Center), modifier = Modifier.fillMaxWidth())
            Rule(mono)
            names.forEachIndexed { i, name ->
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Space.m)) {
                    Text(name, style = mono, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                    Text(figures.getOrElse(i) { "" }, style = mono, modifier = Modifier.widthIn(min = Space.xl))
                }
                notes.getOrNull(i)?.takeIf { it.isNotEmpty() }?.let { Text("  $it", style = mono.copy(color = PaperFaded)) }
            }
            Rule(mono)
            Row(Modifier.fillMaxWidth()) {
                Text("TOTAL", style = mono.copy(fontWeight = FontWeight.Bold), modifier = Modifier.weight(1f))
                Text(total, style = mono.copy(fontWeight = FontWeight.Bold))
            }
        }
}

@Composable
private fun Rule(mono: TextStyle) {
    Text("-".repeat(48), style = mono.copy(color = PaperFaded), maxLines = 1, overflow = TextOverflow.Clip)
}
