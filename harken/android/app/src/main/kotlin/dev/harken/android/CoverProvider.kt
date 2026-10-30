// The covers, for a car and the lock screen: a picture something outside
// the app loads by a `content://` URI — Android Auto draws a browse list,
// and the playing track beside a map, in its own process and takes the art
// as a URI, never a bitmap, and an `http` one it will not load. So the
// playing track's cover is one of these too (`Items.of`), at [CarArt.PLAYING]:
// Media3 reads it in this process for the notification's bitmap, and a car
// reads it itself, granted the URI as the queue is set.
//
// A URI names the size the car asked for, the record's name and where its
// cover is (`uri`): the provider takes the cover's original from the
// screens' own cache (Covers.kt, the same files), fetching it once if it is
// not there, shrinks and crops it to a square that size, and keeps the
// result beside it, so the next ask is a file read. A record with no cover,
// or one that cannot be had — offline, a 404 — is the derived square the
// screens draw for it (`ark-ui`'s `squareBitmap`, in the car's dark
// gradients): a car never shows an empty tile where the phone shows a
// colour.
//
// Not exported: a car reads only the URIs it was handed, each granted to
// its package as it is handed over — a browse list's as it is answered, the
// queue's as it is set (PlaybackService) — so nothing else can ask this
// provider to fetch anything at all.
package dev.harken.android

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.graphics.Bitmap
import android.net.Uri
import android.os.ParcelFileDescriptor

import dev.arkui.Art
import dev.arkui.squareBitmap
import java.io.File
import java.io.FileNotFoundException

class CoverProvider : ContentProvider() {
    override fun onCreate(): Boolean = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        if (mode != "r") throw SecurityException("covers are read-only")
        val c = context ?: throw FileNotFoundException("no context")
        val file = CarArt.file(c, uri) ?: throw FileNotFoundException(uri.toString())
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun getType(uri: Uri): String = "image/png"

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = null

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = 0
}

/** The URIs [CoverProvider] answers, and the pictures behind them. */
object CarArt {
    /** The provider's authority, as the manifest declares it. */
    const val AUTHORITY = "dev.harken.android.art"

    /** What a car is given when it does not say what size it draws at. */
    const val SIZE = 320

    /** What the playing track's picture is given at: the lock screen's, a car's now-playing. */
    const val PLAYING = 512

    /** The most a car is given across, whatever it asks: a cover is decoded to [Covers.BOUND] anyway. */
    const val MAX = 768

    /**
     * The picture of `seed` (an album's or an artist's name), `px` across: its cover at `url` — a
     * whole URL, already joined to the server — or, with none, the derived square.
     */
    fun uri(url: String?, seed: String, px: Int = SIZE): Uri =
        Uri.Builder()
            .scheme("content")
            .authority(AUTHORITY)
            .appendPath(px.coerceIn(16, MAX).toString())
            .appendPath(Hex.encode(seed.toByteArray(Charsets.UTF_8)))
            .appendPath(url?.takeIf { it.isNotEmpty() }?.let { Hex.encode(it.toByteArray(Charsets.UTF_8)) } ?: "-")
            .build()

    /** What a URI names: the size, the seed, and the cover's URL if it has one; null for one not ours. */
    data class Named(val px: Int, val seed: String, val url: String?)

    fun parse(uri: Uri): Named? {
        if (uri.authority != AUTHORITY) return null
        val p = uri.pathSegments
        if (p.size != 3) return null
        return try {
            Named(
                p[0].toInt().coerceIn(16, MAX),
                String(Hex.decode(p[1]), Charsets.UTF_8),
                p[2].takeIf { it != "-" }?.let { String(Hex.decode(it), Charsets.UTF_8) },
            )
        } catch (e: Exception) {
            null
        }
    }

    /**
     * The PNG a URI names, made once and kept under the cache: the cover, cropped square and shrunk
     * to the size asked for, or the derived square. Null for a URI that is not one of these.
     */
    fun file(context: Context, uri: Uri): File? {
        val n = parse(uri) ?: return null
        val dir = File(context.cacheDir, "car-art")
        val out = File(dir, "%08x-%d%s.png".format(Art.hash(n.seed + "\u0000" + (n.url ?: "")), n.px, if (n.url == null) "-square" else ""))
        if (out.exists()) return out
        val cover = n.url?.let { url -> Covers.original(Covers.folder(context), url)?.let { Covers.decode(it, n.px) } }
        // A cover that could not be had is not kept: the next ask tries again.
        val bitmap = cover?.let { square(it, n.px) } ?: squareBitmap(n.seed, Palette.DarkArt, n.px)
        dir.mkdirs()
        val tmp = File(dir, out.name + ".tmp")
        tmp.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        val kept = if (cover != null || n.url == null) out else File(dir, out.name + ".once")
        tmp.renameTo(kept)
        return kept
    }

    /** `b`, its middle cropped square and scaled to `px`. */
    private fun square(b: Bitmap, px: Int): Bitmap {
        val side = minOf(b.width, b.height)
        val cropped = Bitmap.createBitmap(b, (b.width - side) / 2, (b.height - side) / 2, side, side)
        return if (side == px) cropped else Bitmap.createScaledBitmap(cropped, px, px, true)
    }
}
