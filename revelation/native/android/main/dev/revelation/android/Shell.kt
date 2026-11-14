// The chrome around the surface, in Compose: a slim header that always says
// where the camera is, and three places it opens — the picker (the 66 books,
// then a book's chapters), search (a scan of the text, or a typed
// reference), and About (the translation's attribution). Everything that
// goes somewhere goes by the surface's fly-to.
package dev.revelation.android

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

enum class Overlay { NONE, PICKER, SEARCH, ABOUT }

/**
 * The whole screen: the surface edge to edge, the header over its top, and whichever of the picker,
 * search and About is open. [place] is the surface's, hoisted so the header follows the camera.
 */
@Composable
fun RevelationApp(surface: BibleView, bible: Bible?, place: BibleView.Place?, start: Overlay = Overlay.NONE) {
    val palette = LocalPalette.current
    var overlay by remember { mutableStateOf(start) }
    val density = LocalDensity.current
    val navBottom = WindowInsets.navigationBars.getBottom(density)
    SideEffect {
        surface.palette = palette
        surface.bottomInset = navBottom.toFloat()
    }
    val go: (Ref) -> Unit = { ref ->
        overlay = Overlay.NONE
        surface.flyTo(ref)
    }
    BackHandler(enabled = overlay != Overlay.NONE) { overlay = Overlay.NONE }

    Box(Modifier.fillMaxSize().background(Color(palette.ground))) {
        AndroidView(factory = { surface }, modifier = Modifier.fillMaxSize())
        if (bible != null) {
            Header(
                text = place?.header ?: "",
                onPick = { overlay = Overlay.PICKER },
                onSearch = { overlay = Overlay.SEARCH },
                onAbout = { overlay = Overlay.ABOUT },
                modifier = Modifier.align(Alignment.TopCenter).onGloballyPositioned { surface.topInset = it.size.height.toFloat() },
            )
            when (overlay) {
                Overlay.PICKER -> Picker(bible, place?.verse ?: 0, go, onClose = { overlay = Overlay.NONE })
                Overlay.SEARCH -> SearchPanel(bible, go, onClose = { overlay = Overlay.NONE })
                Overlay.ABOUT -> AboutSheet(bible, onClose = { overlay = Overlay.NONE })
                Overlay.NONE -> {}
            }
        }
    }
}

/** The reference under the screen's centre, which opens the picker, and search and About beside it. */
@Composable
fun Header(text: String, onPick: () -> Unit, onSearch: () -> Unit, onAbout: () -> Unit, modifier: Modifier = Modifier) {
    val ground = MaterialTheme.colorScheme.background
    // The ground fades out below the header rather than ending at an edge, so the text scrolls under it.
    Box(
        modifier.fillMaxWidth().background(
            Brush.verticalGradient(0f to ground, 0.7f to ground.copy(alpha = 0.92f), 1f to ground.copy(alpha = 0f)),
        ),
    ) {
        Row(
            Modifier.statusBarsPadding().fillMaxWidth().height(56.dp).padding(start = 8.dp, end = 4.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(
                Modifier.weight(1f).clickable(onClick = onPick).padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(text, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onBackground, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = "Go to", tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 2.dp).size(20.dp))
            }
            IconButton(onClick = onSearch) { Icon(Icons.Filled.Search, contentDescription = "Search", tint = MaterialTheme.colorScheme.onSurfaceVariant) }
            IconButton(onClick = onAbout) { Icon(Icons.Outlined.Info, contentDescription = "About", tint = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}

/** The 66 books, then a book's chapters; a chapter flies there. [verse] is where the camera is, marked. */
@Composable
fun Picker(bible: Bible, verse: Int, onGo: (Ref) -> Unit, onClose: () -> Unit, book: Int? = null) {
    var chosen by remember { mutableStateOf(book) }
    val current = bible.verseBook[verse.coerceIn(0, bible.verseCount - 1)]
    BackHandler(enabled = chosen != null) { chosen = null }
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(Modifier.statusBarsPadding()) {
            Row(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                val b = chosen
                IconButton(onClick = { if (b != null) chosen = null else onClose() }) {
                    Icon(if (b != null) Icons.AutoMirrored.Filled.ArrowBack else Icons.Filled.Close, contentDescription = if (b != null) "Books" else "Close")
                }
                Text(if (b != null) bible.bookTitle(b) else "Books", style = MaterialTheme.typography.titleMedium)
            }
            val b = chosen
            if (b == null) {
                val matthew = bible.bookNames.indexOf("Matthew").takeIf { it >= 0 } ?: 39
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(112.dp),
                    contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 24.dp),
                    modifier = Modifier.navigationBarsPadding(),
                ) {
                    item(span = { GridItemSpan(maxLineSpan) }) { Section("Old Testament") }
                    items((0 until matthew).toList()) { BookCell(bible.bookTitle(it), it == current) { chosen = it } }
                    item(span = { GridItemSpan(maxLineSpan) }) { Section("New Testament") }
                    items((matthew until bible.bookCount).toList()) { BookCell(bible.bookTitle(it), it == current) { chosen = it } }
                }
            } else {
                val here = if (b == current) bible.verseChapterNumber[verse.coerceIn(0, bible.verseCount - 1)] else -1
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(56.dp),
                    contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 24.dp),
                    modifier = Modifier.navigationBarsPadding(),
                ) {
                    items((1..bible.chapters(b)).toList()) { c ->
                        Box(
                            Modifier.height(52.dp).clickable { onGo(Ref(b, c, 1)) },
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(
                                c.toString(),
                                style = MaterialTheme.typography.bodyLarge,
                                fontWeight = if (c == here) FontWeight.Bold else FontWeight.Normal,
                                color = if (c == here) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.85f),
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Section(title: String) {
    Text(
        title.uppercase(),
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = 8.dp, top = 16.dp, bottom = 6.dp),
    )
}

@Composable
private fun BookCell(title: String, current: Boolean, onClick: () -> Unit) {
    Box(Modifier.height(48.dp).clickable(onClick = onClick).padding(horizontal = 8.dp), contentAlignment = Alignment.CenterStart) {
        Text(
            title,
            style = MaterialTheme.typography.labelLarge,
            fontWeight = if (current) FontWeight.Bold else FontWeight.Normal,
            maxLines = 2,
        )
    }
}

/** A field over the text: a typed reference goes straight there; words are found by a scan, each result a fly-to. */
@Composable
fun SearchPanel(bible: Bible, onGo: (Ref) -> Unit, onClose: () -> Unit, initial: String = "", focus: Boolean = true) {
    var query by remember { mutableStateOf(initial) }
    var results by remember { mutableStateOf(if (initial.trim().length >= 2) bible.search(initial) else IntArray(0)) }
    val focusRequester = remember { FocusRequester() }
    LaunchedEffect(query) {
        delay(200)
        val q = query
        results = if (q.trim().length >= 2) withContext(Dispatchers.Default) { bible.search(q) } else IntArray(0)
    }
    if (focus) LaunchedEffect(Unit) { focusRequester.requestFocus() }
    val ref = bible.parseRef(query)
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(Modifier.statusBarsPadding().imePadding()) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onClose) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Close") }
                TextField(
                    value = query,
                    onValueChange = { query = it },
                    singleLine = true,
                    placeholder = { Text("Words, or a reference like John 3:16", maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    textStyle = MaterialTheme.typography.bodyLarge,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions = KeyboardActions(onSearch = { if (ref != null) onGo(ref) }),
                    colors = TextFieldDefaults.colors(
                        focusedContainerColor = Color.Transparent,
                        unfocusedContainerColor = Color.Transparent,
                        focusedIndicatorColor = Color.Transparent,
                        unfocusedIndicatorColor = Color.Transparent,
                    ),
                    modifier = Modifier.weight(1f).focusRequester(focusRequester),
                )
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            LazyColumn(contentPadding = PaddingValues(bottom = 24.dp), modifier = Modifier.navigationBarsPadding()) {
                if (ref != null) {
                    item {
                        val label = if (ref.verse > 1) bible.label(bible.verseIndex(ref)) else "${bible.bookNames[ref.book]} ${ref.chapter}"
                        Text(
                            "Go to $label",
                            style = MaterialTheme.typography.bodyLarge,
                            modifier = Modifier.fillMaxWidth().clickable { onGo(ref) }.padding(horizontal = 20.dp, vertical = 14.dp),
                        )
                    }
                }
                if (results.isNotEmpty()) {
                    item {
                        Text(
                            if (results.size >= 500) "500+ verses" else if (results.size == 1) "1 verse" else "${results.size} verses",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(start = 20.dp, top = 12.dp, bottom = 4.dp),
                        )
                    }
                }
                items(results.toList()) { v ->
                    Column(Modifier.fillMaxWidth().clickable { onGo(bible.ref(v)) }.padding(horizontal = 20.dp, vertical = 10.dp)) {
                        Text(bible.label(v), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text(marked(bible.verseText(v), query.trim()), style = MaterialTheme.typography.bodyMedium, maxLines = 3, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }
    }
}

/** [text] with the first place [needle] occurs in it, ignoring case, in bold. */
private fun marked(text: String, needle: String): AnnotatedString = buildAnnotatedString {
    val at = if (needle.isEmpty()) -1 else text.indexOf(needle, ignoreCase = true)
    if (at < 0) {
        append(text)
        return@buildAnnotatedString
    }
    append(text, 0, at)
    pushStyle(SpanStyle(fontWeight = FontWeight.Bold))
    append(text, at, at + needle.length)
    pop()
    append(text, at + needle.length, text.length)
}

/** What the app is and whose the text is: the translation's own attribution, which it asks to be shown. */
@Composable
fun AboutSheet(bible: Bible, onClose: () -> Unit) {
    Box(
        Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.45f))
            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onClose),
    ) {
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerHigh,
            shape = RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp),
            modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth()
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) {},
        ) {
            Column(Modifier.navigationBarsPadding().padding(start = 24.dp, end = 24.dp, top = 24.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Revelation", style = MaterialTheme.typography.titleLarge)
                Text(
                    "The Bible as one scroll, zoomed like a map. Pinch out until the words become lines, the lines a column, " +
                        "and the column one line; pinch in to read. Double-tap zooms in, a two-finger tap zooms out, " +
                        "and the reference at the top goes anywhere.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(4.dp))
                Text("THE TEXT", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                for (line in bible.attribution) Text(line, style = MaterialTheme.typography.bodyMedium)
                Text(
                    "The app's code is licensed MIT OR Apache-2.0.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
