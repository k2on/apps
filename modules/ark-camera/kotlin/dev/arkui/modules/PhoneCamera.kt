// ark-camera on Android (modules/ark-camera/README.md): the `Camera` interface UniFFI writes, over
// the framework's own camera (Camera2: no library). A watched photo is the back camera full
// screen — a preview, a small YUV stream for the eyes, a full JPEG — every 4:3 and in the sensor's
// orientation: each analysis frame's luminance and a 256-pixel colour square shown to the eyes in
// Rust (`Eyes.look`, on a thread of their own, a frame at a time), what they answer drawn over the
// preview (their outline gliding on `Springs.move`, their words), and the photo taken when they say
// or at the shutter, as the sensor took it: the frames' `turns` stand both upright. A plain photo
// is the same page with no eyes. A photo picked is the system's Photo Picker's (`PickVisualMedia`:
// no permission), stood upright by its EXIF, a JPEG at most 3000 on its long side. Reading a QR
// code needs a decoder Android does not have yet.
//
// The page (and the picker) is drawn by `CameraPage`, which an app puts once at its root.
package dev.arkui.modules

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageFormat
import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.ExifInterface
import android.media.Image
import android.media.ImageReader
import android.net.Uri
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Size
import android.view.Surface
import android.view.TextureView
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.IconButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import dev.arkui.Space
import dev.arkui.Springs
import dev.arkui.pressable
import dev.arkui.reducedMotion
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
import java.util.concurrent.Semaphore
import kotlin.math.abs
import kotlin.math.max
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.ark_camera.Camera
import uniffi.ark_camera.Eyes
import uniffi.ark_camera.Frame
import uniffi.ark_camera.Look
import uniffi.ark_camera.PhotoReply
import uniffi.ark_camera.ScanReply

/** The phone's back camera; its page is [CameraPage]'s. */
class PhoneCamera : Camera {
    /** A photo being taken (or, `pick`, chosen): who watches (none for a plain photo) and where it goes. */
    class Asked(val eyes: Eyes?, val reply: PhotoReply, val pick: Boolean = false)

    private val _asked = MutableStateFlow<Asked?>(null)
    val asked: StateFlow<Asked?> = _asked

    override fun scanQr(reply: ScanReply) = reply.failed("Scanning a code isn't on Android yet")

    override fun takePhoto(reply: PhotoReply) = ask(Asked(null, reply))

    override fun watch(eyes: Eyes, reply: PhotoReply) = ask(Asked(eyes, reply))

    override fun pickPhoto(reply: PhotoReply) = ask(Asked(null, reply, pick = true))

    private fun ask(a: Asked) {
        // One page at a time: one asked while another is up is backed out of.
        if (_asked.value != null) return a.reply.taken(null)
        _asked.value = a
    }

    /** The page answered: its photo, or none. */
    fun answered(jpeg: ByteArray?) {
        _asked.value?.reply?.taken(jpeg)
        _asked.value = null
    }

    fun failed(why: String) {
        _asked.value?.reply?.failed(why)
        _asked.value = null
    }
}

/** The camera's page, or the photo picker, while it is asked for: put once at the app's root. */
@Composable
fun CameraPage(camera: PhoneCamera) {
    val asked by camera.asked.collectAsState()
    val a = asked ?: return
    if (a.pick) return PhotoPicker(camera)
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
    // Upright while it is up.
    DisposableEffect(Unit) {
        val activity = context.activity()
        val was = activity?.requestedOrientation
        activity?.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        onDispose { if (was != null) activity.requestedOrientation = was }
    }
    Dialog(
        onDismissRequest = { camera.answered(null) },
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
            Box(Modifier.fillMaxSize().background(Color.Black)) {
                when {
                    allowed -> Viewfinder(a.eyes, onPhoto = camera::answered, onFailed = camera::failed)
                    refused ->
                        Column(Modifier.align(Alignment.Center).padding(Space.xl), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text("The camera isn't allowed.", color = Color.White)
                            Spacer(Modifier.height(Space.l))
                            Button(onClick = { ask.launch(Manifest.permission.CAMERA) }) { Text("Allow the Camera") }
                            TextButton(onClick = { camera.answered(null) }) { Text("Not Now", color = Color.White) }
                        }
                    else -> {}
                }
                IconButton(
                    onClick = { camera.answered(null) },
                    modifier = Modifier.systemBarsPadding().padding(Space.s).align(Alignment.TopStart)) {
                        Icon(Icons.Default.Close, contentDescription = "Close the Camera", tint = Color.White)
                    }
            }
        }
}

/** The system's Photo Picker, one image; what it picked handed over as [libraryPhoto] makes it. */
@Composable
private fun PhotoPicker(camera: PhoneCamera) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val pick =
        rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
            if (uri == null) return@rememberLauncherForActivityResult camera.answered(null)
            scope.launch {
                val jpeg = withContext(Dispatchers.IO) { runCatching { libraryPhoto(context, uri) }.getOrNull() }
                if (jpeg == null) camera.failed("That photo couldn't be read") else camera.answered(jpeg)
            }
        }
    LaunchedEffect(Unit) { pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }
}

/** A photo from the library as a server reads it: a JPEG at most 3000 on its long side, upright. */
private fun libraryPhoto(context: Context, uri: Uri): ByteArray? {
    val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return null
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth <= 0) return null
    var sample = 1
    while (max(bounds.outWidth, bounds.outHeight) / sample > 3000) sample *= 2
    val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample }) ?: return null
    val orientation =
        runCatching { ExifInterface(bytes.inputStream()).getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL) }
            .getOrDefault(ExifInterface.ORIENTATION_NORMAL)
    val degrees =
        when (orientation) {
            ExifInterface.ORIENTATION_ROTATE_90 -> 90f
            ExifInterface.ORIENTATION_ROTATE_180 -> 180f
            ExifInterface.ORIENTATION_ROTATE_270 -> 270f
            else -> 0f
        }
    val up = if (degrees == 0f) bmp else Bitmap.createBitmap(bmp, 0, 0, bmp.width, bmp.height, Matrix().apply { postRotate(degrees) }, true)
    return ByteArrayOutputStream().use { out ->
        up.compress(Bitmap.CompressFormat.JPEG, 90, out)
        out.toByteArray()
    }
}

@Composable
private fun Viewfinder(eyes: Eyes?, onPhoto: (ByteArray?) -> Unit, onFailed: (String) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var look by remember { mutableStateOf(Look(emptyList(), false, "", false)) }
    var taking by remember { mutableStateOf(false) }
    val flash = remember { Animatable(0f) }
    // With motion reduced the flash is a softer one: a jump in brightness is what reduced motion asks to be spared.
    val reduced = reducedMotion()
    val camera =
        remember {
            FrameCamera(context, eyes, onLook = { l -> look = l }, onFailed = onFailed)
        }
    DisposableEffect(camera) { onDispose { camera.stop() } }

    fun take() {
        if (taking) return
        taking = true
        scope.launch {
            flash.snapTo(if (reduced) 0.3f else 0.9f)
            flash.animateTo(0f, Springs.size())
        }
        camera.capture { jpeg ->
            if (jpeg == null) onFailed("The camera didn't take the picture") else onPhoto(jpeg)
        }
    }

    // The eyes say when.
    LaunchedEffect(look.take) { if (look.take) take() }

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
            Outline(look)
            if (flash.value > 0f) Box(Modifier.fillMaxSize().background(Color.White.copy(alpha = flash.value)))
        }
        val words = if (taking) "Keeping it…" else look.words
        if (words.isNotEmpty()) {
            Surface(
                color = Color.Black.copy(alpha = 0.55f),
                contentColor = Color.White,
                shape = CircleShape,
                modifier = Modifier.align(Alignment.TopCenter).systemBarsPadding().padding(top = Space.xl)) {
                    Text(words, modifier = Modifier.padding(horizontal = Space.l, vertical = Space.s))
                }
        }
        // The shutter: at once, whatever the eyes say.
        Box(
            Modifier.align(Alignment.BottomCenter)
                .systemBarsPadding()
                .padding(bottom = Space.xl)
                .size(76.dp)
                // The whole shutter gives under the finger, its ring with it.
                .pressable(enabled = !taking, onClickLabel = "Take the Picture", onClick = { take() })
                .clip(CircleShape)
                .border(4.dp, Color.White, CircleShape)
                .padding(8.dp)
                .clip(CircleShape)
                .background(if (taking) Color.Gray else Color.White)
                .semantics { contentDescription = "Take the Picture" })
    }
}

/** The eyes' outline over the preview, each corner gliding on the move spring to where they last saw it. */
@Composable
private fun Outline(look: Look) {
    val seen = look.outline.size == 8
    val shown by animateFloatAsState(if (seen) 1f else 0f, Springs.fade(), label = "outline")
    // The last outline seen, kept while it fades.
    var last by remember { mutableStateOf(List(8) { 0f }) }
    if (seen) last = look.outline
    val c = List(8) { i -> animateFloatAsState(last[i], Springs.move(), label = "corner").value }
    if (shown <= 0f) return
    // The app's accent: the theme's primary.
    val tint = MaterialTheme.colorScheme.primary
    Canvas(Modifier.fillMaxSize()) {
        val pts = List(4) { Offset(c[2 * it] * size.width, c[2 * it + 1] * size.height) }
        val path =
            Path().apply {
                moveTo(pts[0].x, pts[0].y)
                for (p in pts.drop(1)) lineTo(p.x, p.y)
                close()
            }
        drawPath(path, tint.copy(alpha = (if (look.held) 0.38f else 0.26f) * shown))
        drawPath(path, tint.copy(alpha = shown), style = Stroke(width = 3.dp.toPx(), join = StrokeJoin.Round))
        for (p in pts) drawCircle(tint.copy(alpha = shown), radius = 5.dp.toPx(), center = p)
    }
}

private tailrec fun Context.activity(): Activity? =
    when (this) {
        is Activity -> this
        is ContextWrapper -> baseContext.activity()
        else -> null
    }

/**
 * The back camera: a preview to a texture, frames for the eyes, and a JPEG on asking — on a thread
 * of its own, the eyes on another, one frame at a time.
 */
private class FrameCamera(
    private val context: Context,
    private val eyes: Eyes?,
    private val onLook: (Look) -> Unit,
    private val onFailed: (String) -> Unit,
) {
    private val thread = HandlerThread("ark-camera").apply { start() }
    private val handler = Handler(thread.looper)
    private val main = Handler(context.mainLooper)
    private val seeing = Executors.newSingleThreadExecutor()
    private val free = Semaphore(1)
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var analysis: ImageReader? = null
    private var still: ImageReader? = null
    private var preview: Surface? = null
    @Volatile private var stopped = false

    /** Quarters clockwise the sensor's frames turn to stand upright, the phone held upright. */
    @Volatile private var turns: Int = 1

    private fun failed(why: String) {
        main.post { onFailed(why) }
    }

    @SuppressLint("MissingPermission")
    fun start(texture: SurfaceTexture) {
        handler.post {
            if (stopped || device != null) return@post
            try {
                val cm = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
                val id =
                    cm.cameraIdList.firstOrNull { cm.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK }
                        ?: return@post failed("This phone has no camera on its back")
                val ch = cm.getCameraCharacteristics(id)
                turns = ((ch.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 90) / 90) % 4
                val map = ch.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return@post failed("The camera says nothing of its sizes")
                fun fourThree(s: Size) = s.width * 3 == s.height * 4
                val jpeg = map.getOutputSizes(ImageFormat.JPEG).filter(::fourThree).maxByOrNull { it.width * it.height } ?: Size(1600, 1200)
                val view = map.getOutputSizes(SurfaceTexture::class.java).filter { fourThree(it) && it.width <= 1920 }.maxByOrNull { it.width } ?: Size(1440, 1080)
                val small = map.getOutputSizes(ImageFormat.YUV_420_888).filter(::fourThree).minByOrNull { abs(it.width - 640) } ?: Size(640, 480)
                texture.setDefaultBufferSize(view.width, view.height)
                preview = Surface(texture)
                if (eyes != null) {
                    analysis =
                        ImageReader.newInstance(small.width, small.height, ImageFormat.YUV_420_888, 2).apply {
                            setOnImageAvailableListener({ r -> look(r) }, handler)
                        }
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
                            failed("The camera stopped (error $error)")
                        }
                    },
                    handler)
            } catch (e: Exception) {
                failed("The camera didn't open: ${e.message}")
            }
        }
    }

    @Suppress("DEPRECATION") // The list of surfaces is what API 26 has.
    private fun open(d: CameraDevice) {
        val p = preview ?: return
        val s = still ?: return
        val a = analysis
        d.createCaptureSession(
            listOfNotNull(p, a?.surface, s.surface),
            object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(cs: CameraCaptureSession) {
                    if (stopped) return cs.close()
                    session = cs
                    val r =
                        d.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply {
                            addTarget(p)
                            a?.let { addTarget(it.surface) }
                            set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
                        }
                    runCatching { cs.setRepeatingRequest(r.build(), null, handler) }.onFailure { failed("The preview didn't start") }
                }

                override fun onConfigureFailed(cs: CameraCaptureSession) = failed("The camera couldn't be set up")
            },
            handler)
    }

    /** The newest frame shown to the eyes, once they are free; a frame that comes while they look is let go. */
    private fun look(r: ImageReader) {
        val image = r.acquireLatestImage() ?: return
        val eyes = eyes
        if (eyes == null || !free.tryAcquire()) return image.close()
        val frame =
            try {
                Frame(image.width.toUInt(), image.height.toUInt(), luma(image), square(image, SQUARE), turns.toUByte(), SystemClock.uptimeMillis())
            } catch (e: Exception) {
                free.release()
                return
            } finally {
                image.close()
            }
        seeing.execute {
            try {
                val l = eyes.look(frame)
                if (!stopped) main.post { onLook(l) }
            } finally {
                free.release()
            }
        }
    }

    /** A full picture, as the sensor has it (not turned): `done` with its JPEG, or none. */
    fun capture(done: (ByteArray?) -> Unit) {
        handler.post {
            val d = device
            val cs = session
            val s = still
            if (d == null || cs == null || s == null) {
                main.post { done(null) }
                return@post
            }
            s.setOnImageAvailableListener(
                { r ->
                    val image = r.acquireNextImage()
                    val bytes =
                        image?.let {
                            val b = it.planes[0].buffer
                            ByteArray(b.remaining()).also { out -> b.get(out) }.also { _ -> it.close() }
                        }
                    r.setOnImageAvailableListener(null, null)
                    main.post { done(bytes) }
                },
                handler)
            val req =
                d.createCaptureRequest(CameraDevice.TEMPLATE_STILL_CAPTURE).apply {
                    addTarget(s.surface)
                    set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
                    set(CaptureRequest.JPEG_ORIENTATION, 0)
                    set(CaptureRequest.JPEG_QUALITY, 95.toByte())
                }
            runCatching { cs.capture(req.build(), null, handler) }.onFailure { main.post { done(null) } }
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
            seeing.shutdown()
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

    private companion object {
        /** The side of the colour square the eyes are given (ark-camera's `SQUARE`). */
        const val SQUARE = 256
    }
}

/** A YUV_420_888 frame's luminance, rows packed. */
private fun luma(image: Image): ByteArray {
    val plane = image.planes[0]
    val (w, h) = image.width to image.height
    val buf = plane.buffer
    val px = ByteArray(w * h)
    for (y in 0 until h) {
        buf.position(y * plane.rowStride)
        buf.get(px, y * w, w)
    }
    return px
}

/**
 * A YUV_420_888 frame shrunk to `side` by `side`, RGB, by the nearest pixel: what a model is given
 * (it squeezes every picture to its square, as it was trained).
 */
private fun square(image: Image, side: Int): ByteArray {
    val (w, h) = image.width to image.height
    val (yp, up, vp) = Triple(image.planes[0], image.planes[1], image.planes[2])
    val (yb, ub, vb) = Triple(yp.buffer, up.buffer, vp.buffer)
    val out = ByteArray(side * side * 3)
    for (j in 0 until side) {
        val sy = j * h / side
        for (i in 0 until side) {
            val sx = i * w / side
            val y = yb.get(sy * yp.rowStride + sx * yp.pixelStride).toInt() and 0xff
            val u = (ub.get((sy / 2) * up.rowStride + (sx / 2) * up.pixelStride).toInt() and 0xff) - 128
            val v = (vb.get((sy / 2) * vp.rowStride + (sx / 2) * vp.pixelStride).toInt() and 0xff) - 128
            val at = (j * side + i) * 3
            out[at] = clamp(y + 1.402f * v)
            out[at + 1] = clamp(y - 0.344f * u - 0.714f * v)
            out[at + 2] = clamp(y + 1.772f * u)
        }
    }
    return out
}

private fun clamp(v: Float): Byte = v.toInt().coerceIn(0, 255).toByte()
