// A page's title at the weight the iPhone gives a large title: 34 sp and
// bold, tracked tight as large text is (`Typeset`, −2% of its size) on its
// 41 sp leading, so a page reads the same on both phones. [LargeTitle] is the style;
// [LargeTitles] hands it to Material's large top app bar, which draws its
// expanded title in `headlineMedium`, and leaves the collapsed one as it is;
// [LargeTitleLine] puts the account's face at the end of that expanded line,
// where [TabTitle] has it.
package dev.arkui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
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

/**
 * A large top app bar's title with the account at the end of its line: for the `title` of a
 * `LargeTopAppBar` under [LargeTitles]. Material draws that title twice, once on the collapsed
 * row in `titleLarge` and once on the expanded line in `headlineMedium`; the face ([account], an
 * [AccountFace]) goes on the expanded line only, centred on the title, and only while [shown] —
 * the bar no more than half collapsed — so a bar rolled up has no face hidden in it to tap. It
 * goes up with the large title, as the iPhone's does when its title scrolls away.
 */
@Composable
public fun LargeTitleLine(shown: Boolean, account: @Composable () -> Unit, title: @Composable () -> Unit) {
    val expanded = LocalTextStyle.current.fontSize == MaterialTheme.typography.headlineMedium.fontSize
    if (!expanded) return title()
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.weight(1f)) { title() }
        if (shown) account()
    }
}
