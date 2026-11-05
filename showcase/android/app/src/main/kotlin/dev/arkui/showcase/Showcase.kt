// The shared components the docs show, on a real device: one activity,
// opened at one shot by name (`--es shot cards`), drawing ark-ui on its own —
// placeholder content, and ark-ui's neutral theme.
//
// The shots are the docs' (`docs/site/src/components.rs`), and the screens
// workflow (.github/workflows/screens.yaml) captures each from an emulator:
// a still a few seconds after it opens, or a recording: the shot opened and
// left to settle, the recording started, then the shot asked again with
// `--ei go <slow>`, which reaches the activity already up (it is singleTop)
// and sets it moving — so what is recorded is the motion, and never the
// launch. `<slow>` is how many times slower than life the workflow has set
// the system's animations (animator_duration_scale, which Compose obeys);
// the shot's own pauses are stretched to match, so the recording, sped back
// up by as much, is life's timing with `<slow>` times the frames. The theme is the system's, which the workflow sets before each
// capture.
package dev.arkui.showcase

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.unit.dp
import dev.arkui.ArkGlyphs
import dev.arkui.ArkTheme
import dev.arkui.Chevron
import dev.arkui.CoverCard
import dev.arkui.Empty
import dev.arkui.ExpandingCard
import dev.arkui.Fetching
import dev.arkui.Glass
import dev.arkui.GlassContainer
import dev.arkui.GlassMorph
import dev.arkui.Heading
import dev.arkui.ListRow
import dev.arkui.LocalArkTheme
import dev.arkui.LocalReselect
import dev.arkui.PageBar
import dev.arkui.Reselect
import dev.arkui.ScrollsToTop
import dev.arkui.SheetRow
import dev.arkui.SignInPhase
import dev.arkui.SignInScreen
import dev.arkui.Space
import dev.arkui.Square
import dev.arkui.TabTitle
import dev.arkui.VolumeSlider
import dev.arkui.glass
import dev.arkui.rememberExpandingCard
import kotlinx.coroutines.delay

class ShowcaseActivity : ComponentActivity() {
    /** Whether a recording's shot has been told to move. */
    private val go = mutableIntStateOf(0)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val shot = intent.getStringExtra("shot") ?: "cards"
        go.intValue = intent.getIntExtra("go", 0)
        setContent { Neutral { Showcase(shot, go.intValue) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.getIntExtra("go", 0).takeIf { it > 0 }?.let { go.intValue = it }
    }
}

/** ark-ui's own look: Material's colours and ark-ui's neutral squares, nothing of any app's. */
@Composable
private fun Neutral(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    CompositionLocalProvider(LocalArkTheme provides ArkTheme(if (dark) ArkTheme.NeutralDark else ArkTheme.NeutralLight)) {
        MaterialTheme(colorScheme = if (dark) darkColorScheme() else lightColorScheme()) {
            Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) { content() }
        }
    }
}

/** What the cards and rows say: places, which mean nothing to any app in particular. */
private val places = listOf("Harbour", "Orchard", "Meadow", "Summit", "Delta", "Lagoon", "Canyon", "Glacier", "Prairie", "Fjord", "Dune", "Grove")

/** A screen's worth of something, under the status bar. */
@Composable
private fun Screen(content: @Composable () -> Unit) = Box(Modifier.fillMaxSize().safeDrawingPadding()) { content() }

/** One shot, by the id the docs give it; a recording's moves once `go`, its pauses `go` times life's. */
@Composable
fun Showcase(shot: String, go: Int) {
    when (shot) {
        "cards" -> Screen {
            Column(Modifier.padding(Space.page), verticalArrangement = Arrangement.spacedBy(Space.l)) {
                Heading("Cards")
                for (pair in places.take(6).chunked(2)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Space.l)) {
                        pair.forEachIndexed { i, p -> CoverCard(p, "", p, "A place", 160.dp, onClick = {}, round = i == 1) }
                    }
                }
            }
        }
        "lists" -> Screen {
            Column {
                TabTitle("Lists")
                for (p in places.take(8)) ListRow(onClick = {}, title = p, under = "A place") { Square(p, Modifier.size(Space.thumb)) }
            }
        }
        "headers" -> Screen {
            Column {
                PageBar("A page", onBack = {})
                TabTitle("A tab's title")
                Heading("A heading", action = "See all")
                Text("What a page says under its heading.", modifier = Modifier.padding(horizontal = Space.page))
            }
        }
        "sheet" -> Sheet {
            SheetRow(ArkGlyphs.Chevron, "Open", onClick = {})
            SheetRow(ArkGlyphs.More, "More", onClick = {}, trailing = { Chevron() })
            SheetRow(ArkGlyphs.Volume, "Louder", onClick = {})
            SheetRow(ArkGlyphs.Back, "Remove", onClick = {}, emphasis = true)
        }
        "empty" -> Screen {
            Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.SpaceEvenly) {
                Empty(ArkGlyphs.More, "Nothing here yet", "What a page says while it has nothing to show.", action = "Add one")
                Fetching("example.com", delayMs = 0)
            }
        }
        "art" -> Screen {
            Column(Modifier.padding(Space.page), verticalArrangement = Arrangement.spacedBy(Space.m)) {
                Heading("Squares")
                for (row in places.chunked(3)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Space.m)) { row.forEach { Square(it, Modifier.size(96.dp)) } }
                }
            }
        }
        "sign-in" -> SignInScreen(rememberVectorPainter(ArkGlyphs.More), "example.com", null, SignInPhase.Idle, onSignIn = {}, onChangeServer = {})
        "slider" -> Screen {
            var level by remember { mutableIntStateOf(60) }
            Column(Modifier.fillMaxSize().padding(Space.page), verticalArrangement = Arrangement.Center) {
                VolumeSlider(level, muted = false, onLevel = { level = it }, onDone = {}, onMute = {}, modifier = Modifier.fillMaxWidth())
            }
        }
        // A bar at the foot rising to the whole screen on its own springs, held, and falling back.
        "expanding-card" -> {
            val card = rememberExpandingCard()
            LaunchedEffect(card, go) {
                if (go == 0) return@LaunchedEffect
                delay(400L * go)
                card.open()
                delay(1_500L * go)
                card.close()
            }
            Box(Modifier.fillMaxSize()) {
                Screen { Column { TabTitle("A page"); for (p in places.take(6)) ListRow(onClick = {}, title = p, under = "A place") { Square(p, Modifier.size(Space.thumb)) } } }
                ExpandingCard(
                    card,
                    stand = 24.dp,
                    onOpen = {},
                    collapsed = { m ->
                        Row(m.height(58.dp).padding(horizontal = Space.s), verticalAlignment = Alignment.CenterVertically) {
                            Spacer(Modifier.size(40.dp))
                            Spacer(Modifier.width(Space.m))
                            Text("A card", style = MaterialTheme.typography.titleSmall)
                        }
                    },
                    expanded = { slot, m ->
                        Column(m.fillMaxSize().safeDrawingPadding().padding(Space.xl), verticalArrangement = Arrangement.spacedBy(Space.l)) {
                            Spacer(Modifier.fillMaxWidth().aspectRatio(1f).then(slot))
                            Text("A card", style = MaterialTheme.typography.headlineSmall)
                            Text("Its bar's bounds to the screen's, and back.")
                        }
                    },
                    traveler = { m -> Square("Harbour", m) },
                )
            }
        }
        // A long list scrolled down, then asked to go to its top: what reselecting a tab does.
        "scroll-to-top" -> {
            val reselect = remember { Reselect() }
            val list = rememberLazyListState()
            LaunchedEffect(go) {
                if (go == 0) return@LaunchedEffect
                delay(400L * go)
                list.animateScrollToItem(30)
                delay(800L * go)
                reselect.ask()
            }
            CompositionLocalProvider(LocalReselect provides reselect) {
                Screen {
                    ScrollsToTop(list)
                    LazyColumn(state = list) {
                        items((1..60).map { "${places[it % places.size]} $it" }) { p ->
                            ListRow(onClick = {}, title = p, under = "A place") { Square(p, Modifier.size(Space.thumb)) }
                        }
                    }
                }
            }
        }
        // A glass button growing into its menu, held, and shrinking back into the button; the still is
        // the menu held open. iOS's shot is the system's glass morphing between the two.
        "glass-menu", "glass-menu-open" -> {
            var open by remember { mutableStateOf(false) }
            LaunchedEffect(go) {
                if (go == 0) return@LaunchedEffect
                delay(400L * go)
                open = true
                delay(1_500L * go)
                open = false
            }
            GlassMenu(open, held = if (shot == "glass-menu-open") 1f else null, onDismiss = { open = false })
        }
        // The same button growing into a sheet resting at the foot of the screen, as iOS 26's floating
        // sheets do, and back. iOS's shot is the system's sheet, zoomed out of its button.
        "glass-sheet" -> {
            var open by remember { mutableStateOf(false) }
            LaunchedEffect(go) {
                if (go == 0) return@LaunchedEffect
                delay(400L * go)
                open = true
                delay(1_500L * go)
                open = false
            }
            GlassMenu(open, held = null, onDismiss = { open = false }, sheet = true)
        }
        // Three capsules of glass sliding together until they are one surface, held, and parting.
        "glass-merge" -> {
            var together by remember { mutableStateOf(false) }
            LaunchedEffect(go) {
                if (go == 0) return@LaunchedEffect
                delay(400L * go)
                together = true
                delay(1_500L * go)
                together = false
            }
            GlassMerge(together)
        }
        else -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text("No showcase for $shot") }
    }
}

/** A bottom sheet as an app raises one: a scrim over the page, the sheet at its foot. */
@Composable
private fun Sheet(content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.scrim.copy(alpha = 0.32f))) {
        Spacer(Modifier.weight(1f))
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(Modifier.padding(bottom = Space.xl)) {
                Box(Modifier.fillMaxWidth().padding(vertical = 22.dp), contentAlignment = Alignment.Center) {
                    Box(Modifier.width(32.dp).height(4.dp).background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f), RoundedCornerShape(2.dp)))
                }
                content()
            }
        }
    }
}

// The glass shots are laid out as iOS's are (showcase/ios), number for number, so the two
// recordings can be watched side by side: a ground a step from the page's, for the glass to stand
// on, and the page's title.

/** How near two pieces of glass come before they join: GlassEffectContainer's spacing on iOS. */
private val glassReach = 24.dp

/** What the glass button opens: the same entries on both phones. */
private val sorts = listOf("Name", "Distance", "Most visited", "Recently visited")

/**
 * A page with a glass button at its foot that grows into a menu of [sorts] while `open`; or, with
 * `sheet`, into a sheet of them under a heading, as wide as the screen less its margins.
 */
@Composable
private fun GlassMenu(open: Boolean, held: Float?, onDismiss: () -> Unit, sheet: Boolean = false) {
    GlassContainer(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainer), spacing = glassReach) {
        Screen {
            Column {
                TabTitle("Glass")
                for (p in places.take(5)) ListRow(onClick = {}, title = p, under = "A place") { Square(p, Modifier.size(Space.thumb)) }
            }
        }
        GlassMorph(
            expanded = open,
            onDismiss = onDismiss,
            modifier = Modifier.align(Alignment.BottomCenter).safeDrawingPadding().padding(bottom = Space.xl),
            id = "sort",
            menuWidth = 250.dp,
            menuCorner = if (sheet) Glass.sheetCorner else Glass.menuCorner,
            sheet = sheet,
            held = held,
            collapsed = {
                Box(Modifier.height(44.dp).padding(horizontal = 20.dp), contentAlignment = Alignment.Center) {
                    Text("Sort", style = MaterialTheme.typography.labelLarge)
                }
            },
        ) {
            Column(Modifier.padding(vertical = if (sheet) Space.l else Space.s)) {
                if (sheet) Text("Sort by", style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 20.dp, vertical = Space.s))
                for (s in sorts) {
                    Text(s, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = Space.m))
                }
            }
        }
    }
}

// How far apart the merge's capsules start: far enough that their slide together is seen frame
// by frame, rather than over in the spring's first few (the iPhone's is the same).
private val MERGE_APART = 80.dp

/** Three capsules in one glass container, apart, or touching (`together`) and so one surface. */
@Composable
private fun GlassMerge(together: Boolean) {
    GlassContainer(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainer), spacing = glassReach) {
        Screen { TabTitle("Glass") }
        Row(Modifier.align(Alignment.Center), horizontalArrangement = Arrangement.spacedBy(if (together) 0.dp else MERGE_APART)) {
            for (l in listOf("A", "B", "C")) {
                Box(Modifier.glass(id = l).size(width = 64.dp, height = 44.dp), contentAlignment = Alignment.Center) {
                    Text(l, style = MaterialTheme.typography.titleMedium)
                }
            }
        }
    }
}
