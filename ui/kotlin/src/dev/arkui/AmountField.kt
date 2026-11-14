// Typing an amount: the number pad with its decimal key, the currency's
// symbol before what is typed, and figures that line up. What is typed is
// kept as typed — a half-typed `12.` is not an amount yet — and read with
// [Amounts.parse] when it is used; `error` is the sentence under the field.
package dev.arkui

import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType

/** An amount's field: `typed` so far, the symbol before it, `onDone` from the keyboard's action. */
@Composable
public fun AmountField(
    typed: String,
    onType: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    error: String? = null,
    supporting: String? = null,
    imeAction: ImeAction = ImeAction.Done,
    onDone: () -> Unit = {},
    textStyle: TextStyle = LocalTextStyle.current,
) {
    val a = LocalAmounts.current
    val note = error ?: supporting
    OutlinedTextField(
        value = typed,
        onValueChange = { t -> onType(t.filter { it.isDigit() || it == '.' || it == ',' || it == '-' }) },
        label = { Text(label) },
        prefix = { Text(a.symbol) },
        isError = error != null,
        supportingText = if (note != null) { { Text(note) } } else null,
        singleLine = true,
        textStyle = textStyle.copy(fontFeatureSettings = "tnum"),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = imeAction),
        keyboardActions = KeyboardActions(onDone = { onDone() }, onNext = { onDone() }, onGo = { onDone() }),
        modifier = modifier,
    )
}
