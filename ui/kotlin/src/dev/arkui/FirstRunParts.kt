// A first run's parts (ark_app::welcome, ui/docs/app.md): what the generated first run of any Ark
// app calls — its page, the server's address, a page-wide button, what it waits on, and a QR code —
// over the onboarding's own page (Onboarding.kt's `OnboardingPage`). Every colour is the theme's.
package dev.arkui

import androidx.compose.foundation.layout.height
import androidx.compose.material3.ProvideTextStyle
import androidx.compose.material3.FilledTonalButton
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/** A first run's page: the app's mark in the theme's primary, a title, a sentence, and what to do. */
@Composable
public fun FirstRunPage(glyph: String, title: String, sentence: String, content: @Composable () -> Unit) {
    // `app`: the app's own mark (icon.svg's, its generated drawable/mark), as the iPhone's page draws
    // it; a glyph by its name otherwise.
    val context = LocalContext.current
    val own = if (glyph == "app") context.resources.getIdentifier("mark", "drawable", context.packageName) else 0
    val painter =
        if (own != 0) painterResource(own)
        else rememberVectorPainter(ArkGlyphs.named(glyph) ?: ArkGlyphs.Chevron)
    OnboardingPage(painter, title = title, sentence = sentence) {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.m)) { content() }
    }
}

/** A server's address: the keyboard for a URL, no corrections, Go to go on; its note under it until there is a problem. */
@Composable
public fun AddressField(
    label: String,
    prompt: String,
    text: String,
    note: String,
    problem: String?,
    enabled: Boolean,
    onChange: (String) -> Unit,
    onSubmit: () -> Unit,
) {
    OutlinedTextField(
        value = text,
        onValueChange = onChange,
        label = { Text(label) },
        placeholder = { Text(prompt) },
        isError = problem != null,
        supportingText = { Text(problem ?: note) },
        enabled = enabled,
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go, autoCorrectEnabled = false),
        keyboardActions = KeyboardActions(onGo = { onSubmit() }),
        modifier = Modifier.fillMaxWidth(),
    )
}

/** A button the width of its page: filled in the primary, or quietly; while busy, a spinner before its title and no press. */
@Composable
public fun WideButton(title: String, prominent: Boolean, enabled: Boolean, busy: Boolean, onTap: () -> Unit) {
    val label: @Composable () -> Unit = {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.Center, modifier = Modifier.clearAndSetSemantics {}) {
            if (busy) {
                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = LocalContentColor.current)
                Spacer(Modifier.width(12.dp))
            }
            Text(title)
        }
    }
    // The iPhone's wide button: as tall (`wide-button-height`), its title the body's size, the
    // other one filled quietly as its glass is, not outlined. Its title is its accessible name, said
    // outright on the button itself, so whatever its label's layout does at a width (dry run
    // 43105302139: a tablet's and an open Fold's Continue had no text in the tree) a screen reader
    // and a driver read it; the label's own text is left out so it is not said twice.
    val wide =
        Modifier.fillMaxWidth().height(Space.button).semantics {
            contentDescription = title
            role = Role.Button
        }
    if (prominent) {
        Button(onClick = onTap, enabled = enabled && !busy, modifier = wide) { ProvideTextStyle(MaterialTheme.typography.bodyLarge) { label() } }
    } else {
        FilledTonalButton(onClick = onTap, enabled = enabled && !busy, modifier = wide) { ProvideTextStyle(MaterialTheme.typography.bodyLarge) { label() } }
    }
}

/** What a page waits on, quietly (a spinner beside it while busy), and what went wrong, in the error colour. */
@Composable
public fun StatusLine(text: String?, busy: Boolean, problem: String?) {
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Space.s)) {
        if (problem != null) {
            Text(
                problem,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
                textAlign = TextAlign.Center,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Assertive })
        }
        if (text != null || busy) {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite }) {
                if (busy) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    if (text != null) Spacer(Modifier.width(Space.s))
                }
                if (text != null) {
                    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                }
            }
        }
    }
}

/** A QR code (`QrCode`), no wider than another phone's camera wants. */
@Composable
public fun CodeImage(size: Long, dark: List<Boolean>) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
        QrCode(size.toInt(), dark, Modifier.widthIn(max = 260.dp))
    }
}
