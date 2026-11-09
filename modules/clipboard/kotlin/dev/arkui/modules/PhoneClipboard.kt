// ark-clipboard on Android (modules/clipboard/README.md): the `Clipboard` interface UniFFI
// writes, over the system's ClipboardManager.
package dev.arkui.modules

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import uniffi.ark_clipboard.Clipboard

/** The system clipboard. */
class PhoneClipboard(private val context: Context) : Clipboard {
    override fun copyText(text: String) {
        val m = context.getSystemService(ClipboardManager::class.java)
        m?.setPrimaryClip(ClipData.newPlainText("Text", text))
    }
}
