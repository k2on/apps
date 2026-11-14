// A server from outside — a join link opened, a code scanned — asked about
// before it is used, any Ark app's: the sign-in machine's question
// (`ark_auth::onboarding`'s `Step.Confirm`, its words its own), Join and
// Cancel. A link that cannot be used says why, and can only be cancelled.
package dev.arkui

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.unit.dp

/** The question, the sentence under it, and Join (only when `canJoin`) and Cancel. */
@Composable
public fun ConfirmScreen(
    mark: Painter,
    question: String?,
    sentence: String,
    canJoin: Boolean,
    onJoin: () -> Unit,
    onCancel: () -> Unit,
) {
    OnboardingPage(mark, title = question ?: "Join", sentence = sentence) {
        if (canJoin) {
            Button(onClick = onJoin, modifier = Modifier.fillMaxWidth()) { Text("Join") }
            Spacer(Modifier.height(8.dp))
        }
        TextButton(onClick = onCancel, modifier = Modifier.fillMaxWidth()) { Text("Cancel") }
    }
}
