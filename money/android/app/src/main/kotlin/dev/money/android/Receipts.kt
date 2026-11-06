// A transaction's receipt, in its sheet: whatever paid for it drawn as the
// paper it came on — monospace on white, in light and dark alike — with how
// sure its reading is; the photos taken for it, still being read or failed;
// and the ways to give it one: scan it (Scanner.kt), choose a photo, type it,
// or attach one that arrived on its own (an Amazon order, a Costco
// receipt) and found nothing that paid for it. A receipt can be typed again
// (a reading put right), shown as its photo, or taken off.
package dev.money.android

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import dev.arkdb.ffi.Id
import dev.arkui.pressable
import dev.arkui.Press
import dev.arkui.LocalAmounts
import dev.arkui.Space
import dev.arkui.Tokens
import dev.money.android.gen.FileRef
import dev.money.android.gen.ItemLine
import dev.money.android.gen.PhotoLine
import dev.money.android.gen.ReceiptLine
import dev.money.android.gen.StoreItem
import dev.money.android.gen.WriteReceipt
import java.io.ByteArrayOutputStream
import kotlin.math.abs
import kotlin.math.max
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.ark_format.formatQuantity
import uniffi.ark_format.formatDayLong

/** The paper's colours: a receipt is white whatever the phone's theme, so the light side's. */
private val Paper = Tokens.Light.surface
private val Ink = Tokens.Light.ink
private val Faded = Tokens.Light.quiet

private val Mono = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp, lineHeight = 18.sp, color = Ink)

/** A transaction's receipts and photos, and the ways to give it one. */
@Composable
fun ReceiptSection(ledger: Ledger, txn: Id, receipt: String, amount: Long, date: String) {
    val book = LocalShell.current.book
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val receipts = ledger.receiptsOf(txn, receipt)
    val reading = ledger.photosOf(txn).filter { it.receiptId.isEmpty() }
    var scanning by remember { mutableStateOf(false) }
    var typing by remember { mutableStateOf<ReceiptDraft?>(null) }
    var attaching by remember { mutableStateOf(false) }
    var showing by remember { mutableStateOf<FileRef?>(null) }
    val pick =
        rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
            if (uri != null) {
                scope.launch {
                    val jpeg = withContext(Dispatchers.IO) { runCatching { libraryPhoto(context, uri) }.getOrNull() }
                    if (jpeg == null) book.tell("Couldn't read that photo") else book.addPhoto(jpeg, txn)
                }
            }
        }

    Column(Modifier.fillMaxWidth().padding(vertical = Space.m)) {
        Text("Receipt", style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
        Spacer(Modifier.height(Space.s))
        for (r in receipts) {
            val photo = ledger.photoOf(r.id)
            ReceiptPaper(ledger, r, photo)
            Row(horizontalArrangement = Arrangement.spacedBy(Space.s), modifier = Modifier.padding(vertical = Space.s)) {
                TextButton(onClick = { typing = ReceiptDraft.of(r, ledger.items[r.id].orEmpty(), txn) }) { Text("Edit") }
                if (photo != null) TextButton(onClick = { showing = photo.file }) { Text("Photo") }
                if (r.txnId == txn) TextButton(onClick = { book.pay(r.id, null) }) { Text("Take Off") }
            }
        }
        for (p in reading) PhotoRow(p, onShow = { showing = p.file })
        if (receipts.isEmpty() && reading.isEmpty()) {
            Text(
                "No receipt yet. Scan it, choose a photo of it, or type it.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(Space.s))
        }
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(Space.s)) {
            AssistChip(
                onClick = { scanning = true },
                label = { Text("Scan") },
                leadingIcon = { Icon(Glyphs.Scan, contentDescription = null, Modifier.size(AssistChipDefaults.IconSize)) })
            AssistChip(
                onClick = { pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                label = { Text("Choose Photo") },
                leadingIcon = { Icon(Glyphs.Photo, contentDescription = null, Modifier.size(AssistChipDefaults.IconSize)) })
            AssistChip(
                onClick = { typing = ReceiptDraft.blank(txn, date, amount) },
                label = { Text("Type It") },
                leadingIcon = { Icon(Glyphs.Write, contentDescription = null, Modifier.size(AssistChipDefaults.IconSize)) })
            if (ledger.unmatched.isNotEmpty()) {
                AssistChip(
                    onClick = { attaching = true },
                    label = { Text("Attach One") },
                    leadingIcon = { Icon(Glyphs.Receipt, contentDescription = null, Modifier.size(AssistChipDefaults.IconSize)) })
            }
        }
    }

    if (scanning) {
        Scanner(
            onPage = { jpeg ->
                scanning = false
                book.addPhoto(jpeg, txn)
            },
            onDismiss = { scanning = false })
    }
    typing?.let { d -> ReceiptEditor(d, onDismiss = { typing = null }) }
    if (attaching) {
        AttachDialog(ledger.unmatched, amount, date, onPick = { r ->
            book.pay(r.id, txn)
            attaching = false
        }, onDismiss = { attaching = false })
    }
    showing?.let { f -> PhotoViewer(f, onDismiss = { showing = null }) }
}

/** A receipt as its paper: the place and day, each item and its price, the total; how sure it is read. */
@Composable
fun ReceiptPaper(ledger: Ledger, r: ReceiptLine, photo: PhotoLine?) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    DisposableEffect(r.id) {
        book.holdItems(r.id)
        onDispose { book.releaseItems(r.id) }
    }
    val items = ledger.items[r.id].orEmpty()
    Column {
        when (photo?.state) {
            "check" -> Banner("Check this: what was read doesn't add up to the total", MaterialTheme.colorScheme.errorContainer)
            "settled" -> Banner("Read, with a line or two put right so it adds up", MaterialTheme.colorScheme.secondaryContainer)
            else -> {}
        }
        Surface(
            color = Paper,
            contentColor = Ink,
            shape = RoundedCornerShape(4.dp),
            shadowElevation = 2.dp,
            modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(horizontal = Space.l, vertical = Space.l)) {
                    Text(
                        r.place.ifEmpty { storeName(r.store) }.uppercase(),
                        style = Mono.copy(fontWeight = FontWeight.Bold, fontSize = 15.sp),
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth())
                    Text(r.date, style = Mono.copy(color = Faded), textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth())
                    Rule()
                    if (items.isEmpty() && r.items > 0) Text("…", style = Mono.copy(color = Faded))
                    for (i in items) {
                        PaperLine(i.name, money(a, -i.amount))
                        if (i.quantity != 1000L && i.quantity > 0) {
                            Text("  ${formatQuantity(i.quantity)} ${i.unit} @ ${money(a, i.unitPrice)}", style = Mono.copy(color = Faded))
                        }
                    }
                    Rule()
                    PaperLine("TOTAL", money(a, -r.total), bold = true)
                    if (r.payee.isNotEmpty()) {
                        Spacer(Modifier.height(Space.s))
                        Text("PAID ${r.payee} ${r.account}".trim(), style = Mono.copy(color = Faded, fontSize = 11.sp))
                    }
                }
            }
    }
}

@Composable
private fun Banner(text: String, color: Color) {
    Surface(color = color, shape = RoundedCornerShape(8.dp), modifier = Modifier.fillMaxWidth().padding(bottom = Space.s)) {
        Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(Space.m))
    }
}

@Composable
private fun PaperLine(name: String, price: String, bold: Boolean = false) {
    val style = if (bold) Mono.copy(fontWeight = FontWeight.Bold) else Mono
    Row(Modifier.fillMaxWidth()) {
        Text(name.uppercase(), style = style, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        Spacer(Modifier.width(Space.m))
        Text(price, style = style)
    }
}

@Composable
private fun Rule() {
    Text("-".repeat(48), style = Mono.copy(color = Faded), maxLines = 1, overflow = TextOverflow.Clip, modifier = Modifier.padding(vertical = 2.dp))
}

/** Money as a receipt prints it: no sign for what was paid, a minus for what was taken off. */
private fun money(a: dev.arkui.Amounts, paid: Long): String = if (paid < 0) "-" + a.format(-paid) else a.format(paid)

private fun storeName(store: String): String =
    when (store) {
        "amazon" -> "Amazon"
        "target" -> "Target"
        "costco" -> "Costco"
        else -> "Receipt"
    }

/** A photo taken for the transaction and not read into a receipt yet. */
@Composable
private fun PhotoRow(p: PhotoLine, onShow: () -> Unit) {
    val words =
        when (p.state) {
            "failed" -> "Couldn't read it yet: ${p.why}. It's tried again."
            "cancelled" -> "Not read"
            else -> "Reading the photo…"
        }
    ListItem(
        leadingContent = { Thumb(p.file) },
        headlineContent = { Text(if (p.state == "failed") "Photo" else "Photo of the receipt") },
        supportingContent = { Text(words) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.pressable(onClickLabel = "Show", onClick = onShow, press = Press.Highlight),
    )
}

/** A photo's bytes as a picture once this phone has them, fetched when it has not, `side` at most. */
@Composable
private fun rememberPhoto(f: FileRef, side: Int): ImageBitmap? {
    val book = LocalShell.current.book
    val got by
        produceState<ImageBitmap?>(null, f.hash, side) {
            book.state.collect {
                if (value == null) value = withContext(Dispatchers.IO) { book.photo(f)?.let { decoded(it, side) } }
            }
        }
    return got
}

@Composable
private fun Thumb(f: FileRef) {
    val img = rememberPhoto(f, 160)
    Box(Modifier.size(48.dp).background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(6.dp))) {
        if (img != null) Image(img, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
    }
}

/** A receipt's photo, whole, over everything. */
@Composable
fun PhotoViewer(f: FileRef, onDismiss: () -> Unit) {
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            val img = rememberPhoto(f, 2400)
            if (img != null) {
                Image(img, contentDescription = "The receipt's photo", contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
            } else {
                Text("Fetching the photo…", color = Color.White, modifier = Modifier.align(Alignment.Center))
            }
            IconButton(onClick = onDismiss, modifier = Modifier.align(Alignment.TopStart).padding(Space.l)) {
                Icon(Icons.Default.Close, contentDescription = "Close", tint = Color.White)
            }
        }
    }
}

/** The receipts nothing pays for, the nearest in amount and then in days first, to attach one. */
@Composable
private fun AttachDialog(unmatched: List<ReceiptLine>, amount: Long, date: String, onPick: (ReceiptLine) -> Unit, onDismiss: () -> Unit) {
    val a = LocalAmounts.current
    val near = unmatched.sortedWith(compareBy({ abs(it.total - amount) }, { abs((Dates.millis(it.date) ?: 0) - (Dates.millis(date) ?: 0)) })).take(30)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Attach a Receipt") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                for (r in near) {
                    ListItem(
                        headlineContent = { Text(r.place.ifEmpty { storeName(r.store) }) },
                        supportingContent = { Text("${formatDayLong(r.date)} · ${r.items} items") },
                        trailingContent = { Text(a.format(abs(r.total))) },
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        modifier = Modifier.pressable(onClickLabel = "Attach", onClick = { onPick(r) }, press = Press.Highlight),
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

/** One item as the form holds it: its name and price typed, positive for paid. */
data class DraftItem(val name: String = "", val price: String = "", val sku: String = "", val quantity: Long = 1000, val unit: String = "ea")

/** A receipt being typed, or one put right. */
data class ReceiptDraft(
    val id: String,
    val txn: Id,
    val place: String,
    val date: String,
    val items: List<DraftItem>,
    /** What the transaction was, our sign, to say how far the items are from it. */
    val expect: Long?,
) {
    /** What it sends, or why not: every item named, every price an amount. */
    fun write(a: dev.arkui.Amounts): Result<WriteReceipt> {
        val lines = items.filter { it.name.isNotBlank() || it.price.isNotBlank() }
        if (lines.isEmpty()) return Result.failure(IllegalArgumentException("A receipt needs an item"))
        val out = ArrayList<StoreItem>()
        for (l in lines) {
            if (l.name.isBlank()) return Result.failure(IllegalArgumentException("Every item needs a name"))
            val p = a.parse(l.price) ?: return Result.failure(IllegalArgumentException("${l.name}: that isn't an amount"))
            out.add(StoreItem(l.name.trim(), l.sku, l.quantity, l.unit, -p))
        }
        return Result.success(WriteReceipt(id, txn, place.trim(), date, null, out))
    }

    companion object {
        fun blank(txn: Id, date: String, amount: Long) = ReceiptDraft("", txn, "", date, listOf(DraftItem()), amount)

        fun of(r: ReceiptLine, items: List<ItemLine>, txn: Id): ReceiptDraft {
            val a = LibraryAmounts()
            return ReceiptDraft(
                r.id,
                r.txnId ?: txn,
                r.place,
                r.date,
                items.map { DraftItem(it.name, a.editable(-it.amount).ifEmpty { "0" }, it.sku, it.quantity, it.unit) }.ifEmpty { listOf(DraftItem()) },
                r.total)
        }
    }
}

/** Typing a receipt: where, when, and each item and its price; the sum against what the bank took. */
@Composable
fun ReceiptEditor(start: ReceiptDraft, onDismiss: () -> Unit) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    var d by remember(start) { mutableStateOf(start) }
    val items = remember(start) { mutableStateListOf<DraftItem>().apply { addAll(start.items) } }
    var error by remember { mutableStateOf<String?>(null) }
    val sum = items.sumOf { a.parse(it.price) ?: 0 }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
            Column(Modifier.fillMaxSize().imePadding()) {
                Row(Modifier.padding(Space.s), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onDismiss) { Icon(Icons.Default.Close, contentDescription = "Close") }
                    Text(if (start.id.isEmpty()) "Type a receipt" else "Edit the receipt", style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
                    Button(onClick = {
                        d.copy(items = items.toList()).write(a).fold(
                            onSuccess = {
                                // The domain's own sentence first (a day that isn't one), under the form.
                                val why = book.check("receipts.write_receipt", it.json())
                                if (why != null) {
                                    error = why
                                } else {
                                    book.writeReceipt(it)
                                    onDismiss()
                                }
                            },
                            onFailure = { error = it.message })
                    }) { Text("Save") }
                }
                Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = Space.xl)) {
                    OutlinedTextField(
                        value = d.place,
                        onValueChange = { d = d.copy(place = it.replace('\n', ' ')) },
                        label = { Text("Store") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth())
                    OutlinedTextField(
                        value = d.date,
                        onValueChange = { d = d.copy(date = it.trim()) },
                        label = { Text("Day (YYYY-MM-DD)") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth())
                    Spacer(Modifier.height(Space.m))
                    for ((n, it) in items.withIndex()) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            OutlinedTextField(
                                value = it.name,
                                onValueChange = { v -> items[n] = items[n].copy(name = v.replace('\n', ' ')) },
                                label = { Text("Item") },
                                singleLine = true,
                                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                                modifier = Modifier.weight(1f))
                            Spacer(Modifier.width(Space.s))
                            OutlinedTextField(
                                value = it.price,
                                onValueChange = { v -> items[n] = items[n].copy(price = v) },
                                label = { Text("Price") },
                                singleLine = true,
                                isError = a.parse(it.price) == null,
                                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = ImeAction.Next),
                                modifier = Modifier.width(112.dp))
                            IconButton(onClick = { if (items.size > 1) items.removeAt(n) else items[0] = DraftItem() }) {
                                Icon(Icons.Default.Delete, contentDescription = "Remove the item")
                            }
                        }
                    }
                    TextButton(onClick = { items.add(DraftItem()) }) {
                        Icon(Icons.Default.Add, contentDescription = null)
                        Spacer(Modifier.width(Space.s))
                        Text("Item")
                    }
                    Spacer(Modifier.height(Space.m))
                    val expect = start.expect?.let { -it }
                    Text("Total ${a.format(sum)}", style = MaterialTheme.typography.titleMedium)
                    if (expect != null && expect != sum) {
                        Text(
                            "The transaction was ${a.format(expect)}: ${a.format(abs(expect - sum))} ${if (sum < expect) "short" else "over"}. Tax goes in as an item.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(vertical = Space.s)) }
                    Spacer(Modifier.height(Space.xxl))
                }
            }
        }
    }
}

// * Pictures

/** `bytes` as a bitmap no wider or taller than about `side`, upright by its EXIF. */
fun decoded(bytes: ByteArray, side: Int): ImageBitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth <= 0) return null
    var sample = 1
    while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= side) sample *= 2
    val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample }) ?: return null
    return upright(bmp, orientation(bytes)).asImageBitmap()
}

/** A photo from the library as the server reads it: a JPEG at most 3000 on its long side, upright. */
fun libraryPhoto(context: Context, uri: Uri): ByteArray? {
    val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return null
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth <= 0) return null
    var sample = 1
    while (max(bounds.outWidth, bounds.outHeight) / sample > 3000) sample *= 2
    val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample }) ?: return null
    val up = upright(bmp, orientation(bytes))
    return ByteArrayOutputStream().use { out ->
        up.compress(Bitmap.CompressFormat.JPEG, 90, out)
        out.toByteArray()
    }
}

/** The EXIF orientation of JPEG `bytes`, 1 (as it is) when it says none. */
private fun orientation(bytes: ByteArray): Int =
    runCatching {
            android.media.ExifInterface(bytes.inputStream())
                .getAttributeInt(android.media.ExifInterface.TAG_ORIENTATION, android.media.ExifInterface.ORIENTATION_NORMAL)
        }
        .getOrDefault(android.media.ExifInterface.ORIENTATION_NORMAL)

private fun upright(b: Bitmap, orientation: Int): Bitmap {
    val degrees =
        when (orientation) {
            android.media.ExifInterface.ORIENTATION_ROTATE_90 -> 90f
            android.media.ExifInterface.ORIENTATION_ROTATE_180 -> 180f
            android.media.ExifInterface.ORIENTATION_ROTATE_270 -> 270f
            else -> return b
        }
    return Bitmap.createBitmap(b, 0, 0, b.width, b.height, Matrix().apply { postRotate(degrees) }, true)
}
