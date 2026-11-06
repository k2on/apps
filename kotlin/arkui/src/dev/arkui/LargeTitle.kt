// A page's title at the weight the iPhone gives a large title: 34 sp and
// bold, tracked tight as large text is (`Typeset`, −2% of its size) on its
// 41 sp leading, so a page reads the same on both phones. [LargeTitle] is the style;
// [LargeTitles] hands it to Material's large top app bar, which draws its
// expanded title in `headlineMedium`, and leaves the collapsed one as it is.
package dev.arkui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/** A top-level page's title: the iPhone's large title, 34 sp bold, tracked for its size. */
public val LargeTitle: TextStyle =
    TextStyle(
        fontSize = 34.sp,
        lineHeight = 41.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = Typeset.tracking(34f),
    )

/** `content` with [LargeTitle] as `headlineMedium`: a `LargeTopAppBar`'s expanded title. */
@Composable
public fun LargeTitles(content: @Composable () -> Unit) {
    val type = MaterialTheme.typography
    MaterialTheme(
        colorScheme = MaterialTheme.colorScheme,
        shapes = MaterialTheme.shapes,
        typography = type.copy(headlineMedium = type.headlineMedium.merge(LargeTitle)),
        content = content,
    )
}
