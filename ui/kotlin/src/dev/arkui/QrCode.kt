// A QR code, drawn, and "Add a Device", which shows one: what another
// phone's camera reads to sign in as this login. Which modules are dark is
// the sign-in's, made once in Rust (`ark_auth::onboarding::qr_code`, the
// bridge's `deviceLink`); here it is squares on a light ground with the
// four-module quiet zone a scanner needs, always dark on light whatever the
// theme.
package dev.arkui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp

/** The light modules around the code. */
public const val QR_QUIET: Int = 4

/** The code `size` modules a side, `dark` row by row from the top left. */
@Composable
public fun QrCode(size: Int, dark: List<Boolean>, modifier: Modifier = Modifier) {
    Canvas(modifier.aspectRatio(1f).semantics { contentDescription = "QR code" }) {
        val m = this.size.minDimension / (size + 2 * QR_QUIET)
        drawRect(Color.White)
        dark.forEachIndexed { i, d ->
            if (d) {
                val x = (i % size + QR_QUIET) * m
                val y = (i / size + QR_QUIET) * m
                drawRect(Color.Black, Offset(x, y), Size(m, m))
            }
        }
    }
}

/** What "Add a Device" shows: the join link, and it as a QR code; or why it could not be made. */
public data class DeviceLinkShown(
    val link: String?,
    val size: Int,
    val dark: List<Boolean>,
    val error: String?,
)

/**
 * "Add a Device": the join link another device scans or opens — as a QR code, and to copy — made
 * by `make` (the server is asked, as this login), waiting while it is made; or why it could not be.
 */
@Composable
public fun AddDeviceDialog(make: suspend () -> DeviceLinkShown, onDismiss: () -> Unit) {
    var shown by remember { mutableStateOf<DeviceLinkShown?>(null) }
    LaunchedEffect(Unit) { shown = make() }
    val clipboard = LocalClipboardManager.current
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add a Device") },
        text = {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                val s = shown
                when {
                    s == null -> CircularProgressIndicator()
                    s.error != null -> Text(s.error)
                    else -> {
                        QrCode(s.size, s.dark, Modifier.fillMaxWidth())
                        Spacer(Modifier.height(12.dp))
                        Text(
                            "Scan this with the other device's camera, or open the link there. It works once, for ten minutes.",
                            style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        },
        confirmButton = {
            val link = shown?.link
            if (link != null) {
                TextButton(onClick = { clipboard.setText(AnnotatedString(link)) }) { Text("Copy Link") }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Done") } },
    )
}
