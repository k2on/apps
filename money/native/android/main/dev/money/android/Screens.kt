// What a screen written from Rust needs of money on Android: its page frame. The screens' holders
// over their runtimes are generated (ui/prebuild.py, from app.toml's `[screens]`, over the app
// library's `ArkScreen`), and the runtimes are money's host's.
package dev.money.android

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import dev.arkui.LocalGlyphs
import dev.arkui.LocalRowMark
import dev.arkui.LocalTabFrame

/**
 * What a screen written from Rust needs of this app: its page frame (the top bar; adding a
 * transaction is the shell toolbar's), its marks and its glyphs.
 */
@Composable
fun ScreenFrame(title: String? = null, onBack: (() -> Unit)? = null, content: @Composable () -> Unit) {
    CompositionLocalProvider(
        LocalTabFrame provides { tab, _, inner ->
            // No floating button over the rows: a new transaction is the
            // shell's toolbar's (money/app/src/shell/view.rs), as on every client.
            // A page pushed over a tab (All Accounts) has its own title and the way back.
            Page(title = title ?: tab, onBack = onBack) { pad, _ -> Box(Modifier.padding(top = pad.calculateTopPadding())) { inner() } }
        },
        LocalRowMark provides { kind -> KindTile(kind) },
        LocalGlyphs provides Glyphs::shared,
        content = content,
    )
}
