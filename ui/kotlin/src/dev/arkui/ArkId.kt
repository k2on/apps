// Each element's walk id (ark_app::shots::id_of: `budget.row:dining-out`, `budget.auto-assign`)
// as its test tag, so what names an element in a screenshot walk names it to anything outside the
// app too (the resource id UI Automator reads, with `testTagsAsResourceId` on the activity's
// root). Written by the generated screens (ui/rust/gen), never by hand, and never said: a test
// tag is not read aloud.
package dev.arkui

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag

/** `content` under the walk id `id`, laid out as it would be alone. */
@Composable
public fun ArkId(id: String, content: @Composable () -> Unit) {
    Box(Modifier.testTag(id), propagateMinConstraints = true) { content() }
}

/**
 * Words as an id's part, as ark_app::shots::slug makes them: lower case, letters and digits, a
 * dash between words, anything else dropped; a tag's `/` kept.
 */
public fun arkSlug(s: String): String {
    val out = StringBuilder()
    for (c in s) {
        when {
            c.isLetterOrDigit() -> out.append(c.lowercase())
            c == '/' -> out.append('/')
            out.isNotEmpty() && !out.endsWith("-") && !out.endsWith("/") -> out.append('-')
        }
    }
    return out.toString().trim('-')
}
