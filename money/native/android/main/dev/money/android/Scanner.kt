// Scanning a receipt as Apple Notes scans a document: the camera full
// screen, and over it, wherever the paper is, a translucent blue quad that
// glides to its edges as the phone moves; held still and sharp for about a
// second, the picture takes itself — a flash, the page made flat, and it
// shrinks away into the corner — and the photo goes to the book. The
// shutter takes it at once, page or not.
//
// The camera is the framework's own (Camera2): a preview on a TextureView,
// a small YUV stream for the eyes, and a full JPEG for the page, all 4:3
// and in the sensor's orientation. The eyes are DocAligner's corner model
// in money's library (`moneyPageCorners`, money/ffi/src/page.rs), on a
// thread of its own, a frame at a time as fast as it goes; while it has not
// answered yet, and on a library without it, Vision.kt's bright region. The scanner holds the
// phone upright while it is open, so the preview and the page are turned by
// the sensor's orientation alone.
package dev.money.android

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.BitmapRegionDecoder
import android.graphics.ImageFormat
import android.graphics.Matrix
import android.graphics.Rect
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.ImageReader
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.util.Size
import android.view.Surface
import android.view.TextureView
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import dev.arkui.pressable
import dev.arkui.Accent
import dev.arkui.Space
import dev.arkui.Springs
import dev.arkui.reducedMotion
import java.io.ByteArrayOutputStream
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.max
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** The overlay's colour: money's green as the dark side has it, bright enough to read over a camera's picture. */
private val Tint = Accent.Green.colors(dark = true).accent

/** The least sharpness (Vision.sharpness on the analysis frame) a page is taken at. */
private const val SHARP = 60.0

/** A receipt's page, scanned: `onPage` is given the flat page as a JPEG. */
@Composable
fun Scanner(onPage: (ByteArray) -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    var allowed by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    var refused by remember { mutableStateOf(false) }
    val ask =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
            allowed = ok
            refused = !ok
        }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }
    // Upright while it scans.
    DisposableEffect(Unit) {
        val activity = context.activity()
        val was = activity?.requestedOrientation
        activity?.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        onDispose { if (was != null) activity.requestedOrientation = was }
    }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            when {
                allowed -> Viewfinder(onPage, onDismiss)
                refused ->
                    Column(Modifier.align(Alignment.Center).padding(Space.xl), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("money needs the camera to scan a receipt.", color = Color.White)
                        Spacer(Modifier.height(Space.l))
                        Button(onClick = { ask.launch(Manifest.permission.CAMERA) }) { Text("Allow the Camera") }
                        TextButton(onClick = onDismiss) { Text("Not now", color = Color.White) }
                    }
                else -> {}
            }
            IconButton(onClick = onDismiss, modifier = Modifier.systemBarsPadding().padding(Space.s).align(Alignment.TopStart)) {
                Icon(Icons.Default.Close, contentDescription = "Close the scanner", tint = Color.White)
            }
        }
    }
}

/** What the eyes say of a frame, for the screen. */
private data class Seen(val quad: Quad?, val sharp: Boolean, val w: Int, val h: Int)

@Composable
private fun Viewfinder(onPage: (ByteArray) -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var seen by remember { mutableStateOf(Seen(null, false, 4, 3)) }
    var steady by remember { mutableStateOf(false) }
    var taking by remember { mutableStateOf(false) }
    var failed by remember { mutableStateOf<String?>(null) }
    var page by remember { mutableStateOf<ImageBitmap?>(null) }
    val flash = remember { Animatable(0f) }
    val away = remember { Animatable(0f) }
    // With motion reduced the page fades where it is rather than shrinking into the corner, and the
    // flash is a softer one: a jump in brightness is what reduced motion asks to be spared.
    val reduced = reducedMotion()
    val camera =
        remember {
            PageCamera(
                context,
                object : PageCamera.Listener {
                    private val still = Steady()
                    private val main = Handler(Looper.getMainLooper())

                    override fun frame(q: Quad?, sharpness: Double, w: Int, h: Int) {
                        val held = still.tell(q, SystemClock.uptimeMillis(), hypot(w.toFloat(), h.toFloat()))
                        val sharp = sharpness >= SHARP
                        main.post {
                            seen = Seen(q, sharp, w, h)
                            steady = held && sharp
                        }
                    }

                    override fun failed(why: String) {
                        main.post { failed = why }
                    }
                })
        }
    DisposableEffect(camera) { onDispose { camera.stop() } }

    fun take() {
        if (taking) return
        taking = true
        val quad = seen.quad
        val (w, h) = seen.w to seen.h
        camera.capture { jpeg ->
            scope.launch {
                if (jpeg == null) {
                    taking = false
                    failed = "The camera didn't take the picture"
                    return@launch
                }
                // The flash dies away, and the page goes, on arkui's critically damped springs: a
                // shutter is not a throw.
                launch {
                    flash.snapTo(if (reduced) 0.3f else 0.9f)
                    flash.animateTo(0f, Springs.size())
                }
                val flat = withContext(Dispatchers.Default) { runCatching { flatPage(jpeg, quad, w, h, camera.turns) }.getOrNull() }
                if (flat == null) {
                    taking = false
                    failed = "Couldn't make the page flat"
                    return@launch
                }
                page = flat.first
                away.animateTo(1f, if (reduced) Springs.fade() else Springs.move())
                onPage(flat.second)
            }
        }
    }

    // A page held still and sharp takes itself.
    LaunchedEffect(steady) {
        if (steady && !taking) {
            delay(120)
            if (steady) take()
        }
    }

    Box(Modifier.fillMaxSize()) {
        BoxWithConstraints(Modifier.align(Alignment.Center).fillMaxWidth().aspectRatio(3f / 4f)) {
            AndroidView(
                factory = { c ->
                    TextureView(c).apply {
                        surfaceTextureListener =
                            object : TextureView.SurfaceTextureListener {
                                override fun onSurfaceTextureAvailable(t: SurfaceTexture, width: Int, height: Int) = camera.start(t)

                                override fun onSurfaceTextureSizeChanged(t: SurfaceTexture, width: Int, height: Int) {}

                                override fun onSurfaceTextureDestroyed(t: SurfaceTexture): Boolean {
                                    camera.pause()
                                    return true
                                }

                                override fun onSurfaceTextureUpdated(t: SurfaceTexture) {}
                            }
                    }
                },
                modifier = Modifier.fillMaxSize())
            val density = LocalDensity.current
            val vw = with(density) { maxWidth.toPx() }
            Overlay(seen, camera.turns, vw, steady)
            if (flash.value > 0f) Box(Modifier.fillMaxSize().background(Color.White.copy(alpha = flash.value)))
            page?.let { img ->
                // The flat page, shrinking into the corner.
                val t = away.value
                Image(
                    img,
                    contentDescription = null,
                    modifier =
                        Modifier.fillMaxSize().padding(Space.xl).graphicsLayer {
                            if (reduced) {
                                alpha = 1f - t
                                return@graphicsLayer
                            }
                            val s = 1f - 0.8f * t
                            scaleX = s
                            scaleY = s
                            translationX = size.width * 0.4f * t
                            translationY = size.height * 0.45f * t
                            alpha = 1f - 0.3f * t
                        })
            }
        }
        val words =
            when {
                failed != null -> failed!!
                taking -> "Keeping it…"
                seen.quad == null -> "Point the camera at the receipt"
                !seen.sharp -> "Hold steady: it's blurry"
                else -> "Hold still"
            }
        Surface(
            color = Color.Black.copy(alpha = 0.55f),
            contentColor = Color.White,
            shape = CircleShape,
            modifier = Modifier.align(Alignment.TopCenter).systemBarsPadding().padding(top = Space.xl)) {
                Text(words, modifier = Modifier.padding(horizontal = Space.l, vertical = Space.s))
            }
        // The shutter: at once, page or not.
        Box(
            Modifier.align(Alignment.BottomCenter)
                .systemBarsPadding()
                .padding(bottom = Space.xl)
                .size(76.dp)
                // The whole shutter gives under the finger, its ring with it.
                .pressable(enabled = !taking, onClickLabel = "Take the picture", onClick = { take() })
                .clip(CircleShape)
                .border(4.dp, Color.White, CircleShape)
                .padding(8.dp)
                .clip(CircleShape)
                .background(if (taking) Color.Gray else Color.White)
                .semantics { contentDescription = "Take the picture" })
    }
}

/** The page's quad over the preview, gliding toward where the eyes last saw it. */
@Composable
private fun Overlay(seen: Seen, turns: Int, viewWidth: Float, steady: Boolean) {
    var shown by remember { mutableStateOf<Quad?>(null) }
    var alpha by remember { mutableStateOf(0f) }
    val target =
        seen.quad?.let { q ->
            val r = q.rotated(turns, seen.w, seen.h)
            val rw = if (turns % 2 == 1) seen.h else seen.w
            r.scaled(viewWidth / rw)
        }
    val aim by rememberUpdatedState(target)
    LaunchedEffect(Unit) {
        var last = 0L
        while (true) {
            withFrameNanos { now ->
                val dt = if (last == 0L) 0.016f else ((now - last) / 1e9f).coerceIn(0f, 0.1f)
                last = now
                val k = 1f - kotlin.math.exp(-dt * 14f)
                val t = aim
                if (t != null) {
                    shown = shown?.toward(t, k) ?: t
                    alpha += (1f - alpha) * k
                } else {
                    alpha += (0f - alpha) * k
                    if (alpha < 0.02f) shown = null
                }
            }
        }
    }
    val q = shown ?: return
    Canvas(Modifier.fillMaxSize()) {
        val path =
            Path().apply {
                moveTo(q.tl.x, q.tl.y)
                lineTo(q.tr.x, q.tr.y)
                lineTo(q.br.x, q.br.y)
                lineTo(q.bl.x, q.bl.y)
                close()
            }
        drawPath(path, Tint.copy(alpha = (if (steady) 0.38f else 0.26f) * alpha))
        drawPath(path, Tint.copy(alpha = alpha), style = Stroke(width = 3.dp.toPx(), join = StrokeJoin.Round))
        for (c in q.corners) drawCircle(Tint.copy(alpha = alpha), radius = 5.dp.toPx(), center = Offset(c.x, c.y))
    }
}

/**
 * The JPEG's page made flat and upright: `quad` (in the analysis frame, `w` by `h`, the sensor's way
 * up) scaled to the photo, flattened, and turned `turns` quarters to be upright; the whole photo
 * turned when there is no quad. The page as a picture for the screen, and as a JPEG.
 */
private fun flatPage(jpeg: ByteArray, quad: Quad?, w: Int, h: Int, turns: Int): Pair<ImageBitmap, ByteArray> {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(jpeg, 0, jpeg.size, bounds)
    val (pw, ph) = bounds.outWidth to bounds.outHeight
    val page = quad?.takeIf { abs(pw.toFloat() / ph - w.toFloat() / h) < 0.05f }?.scaled(pw.toFloat() / w)
    var flat: Bitmap = if (page == null) wholePhoto(jpeg, pw, ph) else pageAlone(jpeg, page, pw, ph)
    if (turns % 4 != 0) flat = Bitmap.createBitmap(flat, 0, 0, flat.width, flat.height, Matrix().apply { postRotate(90f * turns) }, true)
    val bytes = ByteArrayOutputStream().use { o ->
        flat.compress(Bitmap.CompressFormat.JPEG, 90, o)
        o.toByteArray()
    }
    return flat.asImageBitmap() to bytes
}

/** The longest side the whole photo is kept at, a page not found in it: a little past the 1920 the server reads at. */
private const val WHOLE = 2600

/** The longest side the rectangle round a page is decoded at before it is halved. */
private const val PAGE = 4000

/** The whole photo, `pw` by `ph`, halved until it is at most [WHOLE] on its longest side. */
private fun wholePhoto(jpeg: ByteArray, pw: Int, ph: Int): Bitmap {
    var sample = 1
    while (max(pw, ph) / sample > WHOLE) sample *= 2
    return BitmapFactory.decodeByteArray(jpeg, 0, jpeg.size, BitmapFactory.Options().apply { inSampleSize = sample })
}

/**
 * The page alone, flat, at the photo's own resolution: only the rectangle round `q` (in the photo's
 * pixels, `pw` by `ph`) is decoded, halved only past [PAGE]. A receipt is a strip of the photo, and
 * halving the whole photo first left one 420 pixels across, its small print read wrong.
 */
private fun pageAlone(jpeg: ByteArray, q: Quad, pw: Int, ph: Int): Bitmap {
    val xs = q.corners.map { it.x }
    val ys = q.corners.map { it.y }
    val left = floor(xs.min()).toInt().coerceIn(0, pw - 1)
    val top = floor(ys.min()).toInt().coerceIn(0, ph - 1)
    val rect = Rect(left, top, ceil(xs.max()).toInt().coerceIn(left + 1, pw), ceil(ys.max()).toInt().coerceIn(top + 1, ph))
    var sample = 1
    while (max(rect.width(), rect.height()) / sample > PAGE) sample *= 2
    val decoder =
        if (Build.VERSION.SDK_INT >= 31) BitmapRegionDecoder.newInstance(jpeg, 0, jpeg.size)
        else @Suppress("DEPRECATION") checkNotNull(BitmapRegionDecoder.newInstance(jpeg, 0, jpeg.size, false)) { "the photo does not decode" }
    val part = decoder.decodeRegion(rect, BitmapFactory.Options().apply { inSampleSize = sample })
    decoder.recycle()
    val local = q.shifted(-left.toFloat(), -top.toFloat()).scaled(part.width.toFloat() / rect.width())
    val (fw, fh) = local.flatSize()
    val px = IntArray(part.width * part.height)
    part.getPixels(px, 0, part.width, 0, 0, part.width, part.height)
    val out = Warp.flatten(px, part.width, part.height, local, fw, fh)
    return Bitmap.createBitmap(out, fw, fh, Bitmap.Config.ARGB_8888)
}

private tailrec fun Context.activity(): Activity? =
    when (this) {
        is Activity -> this
        is ContextWrapper -> baseContext.activity()
        else -> null
    }

/**
 * The back camera, for pages: a preview to `texture`, frames for the eyes, and a JPEG on asking — all
 * on a thread of its own. `turns` is how many quarters clockwise its frames turn to stand upright.
 */
class PageCamera(private val context: Context, private val listener: Listener) {
    interface Listener {
        /** The page in a frame (`w` by `h`), or none, and how sharp the frame is. */
        fun frame(q: Quad?, sharpness: Double, w: Int, h: Int)

        fun failed(why: String)
    }

    private val thread = HandlerThread("money-scanner").apply { start() }
    private val handler = Handler(thread.looper)
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var analysis: ImageReader? = null
    private var still: ImageReader? = null
    private var preview: Surface? = null
    private var looked = 0L
    @Volatile private var stopped = false
    /** The model's thread, one frame on it at a time, and the page it last saw. */
    private val eyes = java.util.concurrent.Executors.newSingleThreadExecutor()
    private val model = java.util.concurrent.Semaphore(1)
    @Volatile private var seen: Quad? = null
    @Volatile private var answered = false

    /** Quarters clockwise the sensor's frames turn to stand upright, the phone held upright. */
    @Volatile var turns: Int = 1
        private set

    @SuppressLint("MissingPermission")
    fun start(texture: SurfaceTexture) {
        handler.post {
            if (stopped || device != null) return@post
            try {
                val cm = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
                val id =
                    cm.cameraIdList.firstOrNull { cm.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK }
                        ?: return@post listener.failed("This phone has no camera on its back")
                val ch = cm.getCameraCharacteristics(id)
                turns = ((ch.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 90) / 90) % 4
                val map = ch.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return@post listener.failed("The camera says nothing of its sizes")
                fun fourThree(s: Size) = s.width * 3 == s.height * 4
                val jpeg = map.getOutputSizes(ImageFormat.JPEG).filter(::fourThree).maxByOrNull { it.width * it.height } ?: Size(1600, 1200)
                val view = map.getOutputSizes(SurfaceTexture::class.java).filter { fourThree(it) && it.width <= 1920 }.maxByOrNull { it.width } ?: Size(1440, 1080)
                val eyes =
                    map.getOutputSizes(ImageFormat.YUV_420_888).filter(::fourThree).minByOrNull { abs(it.width - 640) } ?: Size(640, 480)
                texture.setDefaultBufferSize(view.width, view.height)
                val p = Surface(texture)
                preview = p
                analysis =
                    ImageReader.newInstance(eyes.width, eyes.height, ImageFormat.YUV_420_888, 2).apply {
                        setOnImageAvailableListener({ r -> look(r) }, handler)
                    }
                still = ImageReader.newInstance(jpeg.width, jpeg.height, ImageFormat.JPEG, 2)
                cm.openCamera(
                    id,
                    object : CameraDevice.StateCallback() {
                        override fun onOpened(d: CameraDevice) {
                            if (stopped) return d.close()
                            device = d
                            open(d)
                        }

                        override fun onDisconnected(d: CameraDevice) {
                            d.close()
                            device = null
                        }

                        override fun onError(d: CameraDevice, error: Int) {
                            d.close()
                            device = null
                            listener.failed("The camera stopped (error $error)")
                        }
                    },
                    handler)
            } catch (e: Exception) {
                listener.failed("The camera didn't open: ${e.message}")
            }
        }
    }

    @Suppress("DEPRECATION") // The list of surfaces is what API 26 has.
    private fun open(d: CameraDevice) {
        val p = preview ?: return
        val a = analysis ?: return
        val s = still ?: return
        d.createCaptureSession(
            listOf(p, a.surface, s.surface),
            object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(cs: CameraCaptureSession) {
                    if (stopped) return cs.close()
                    session = cs
                    val r =
                        d.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply {
                            addTarget(p)
                            addTarget(a.surface)
                            set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
                        }
                    runCatching { cs.setRepeatingRequest(r.build(), null, handler) }.onFailure { listener.failed("The preview didn't start") }
                }

                override fun onConfigureFailed(cs: CameraCaptureSession) = listener.failed("The camera couldn't be set up")
            },
            handler)
    }

    // A frame's luminance looked at, ten times a second at most.
    private fun look(r: ImageReader) {
        val image = r.acquireLatestImage() ?: return
        try {
            val now = SystemClock.uptimeMillis()
            if (now - looked < 90) return
            looked = now
            val plane = image.planes[0]
            val (w, h) = image.width to image.height
            val buf = plane.buffer
            val stride = plane.rowStride
            val px = ByteArray(w * h)
            for (y in 0 until h) {
                buf.position(y * stride)
                buf.get(px, y * w, w)
            }
            val luma = Luma(w, h, px)
            // The model's latest answer, and the next frame given to it once
            // it is free.
            if (model.tryAcquire()) {
                val rgb = square(image, Model.SIDE)
                eyes.execute {
                    try {
                        seen = Model.page(rgb, w, h)
                        answered = true
                    } finally {
                        model.release()
                    }
                }
            }
            val page = if (Model.works && answered) seen else Vision.page(luma)
            listener.frame(page, Vision.sharpness(luma), w, h)
        } catch (e: Exception) {
            // A frame that does not read is skipped.
        } finally {
            image.close()
        }
    }

    /** A full picture, as the sensor has it (not turned): `done` with its JPEG, or none. */
    fun capture(done: (ByteArray?) -> Unit) {
        handler.post {
            val d = device
            val cs = session
            val s = still
            if (d == null || cs == null || s == null) return@post done(null)
            s.setOnImageAvailableListener(
                { r ->
                    val image = r.acquireNextImage()
                    val bytes =
                        image?.let {
                            val b = it.planes[0].buffer
                            ByteArray(b.remaining()).also { out -> b.get(out) }.also { _ -> it.close() }
                        }
                    r.setOnImageAvailableListener(null, null)
                    done(bytes)
                },
                handler)
            val req =
                d.createCaptureRequest(CameraDevice.TEMPLATE_STILL_CAPTURE).apply {
                    addTarget(s.surface)
                    set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
                    set(CaptureRequest.JPEG_ORIENTATION, 0)
                    set(CaptureRequest.JPEG_QUALITY, 95.toByte())
                }
            runCatching { cs.capture(req.build(), null, handler) }.onFailure { done(null) }
        }
    }

    /** The camera let go while the preview is gone (the app behind another); `start` takes it again. */
    fun pause() {
        handler.post { close() }
    }

    fun stop() {
        if (stopped) return
        stopped = true
        handler.post {
            close()
            thread.quitSafely()
            eyes.shutdown()
        }
    }

    private fun close() {
        runCatching { session?.close() }
        runCatching { device?.close() }
        runCatching { analysis?.close() }
        runCatching { still?.close() }
        runCatching { preview?.release() }
        session = null
        device = null
        analysis = null
        still = null
        preview = null
    }
}
