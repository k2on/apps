// The shared components the docs show, on a real device: one activity,
// opened at one shot by name (`--es shot cards`), drawing ark-ui on its own —
// placeholder content, and ark-ui's neutral theme.
//
// The shots are the docs' (`docs/site/src/components.rs`), and the screens
// workflow (.github/workflows/screens.yaml) captures each from an emulator:
// a still a few seconds after it opens, or a recording: the shot opened and
// left to settle, the recording started, then the shot asked again with
// `--ez go true`, which reaches the activity already up (it is singleTop)
// and sets it moving — so what is recorded is the motion, and never the
// launch. The theme is the system's, which the workflow sets before each
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
import dev.harken.android.ui.ArkGlyphs
import dev.harken.android.ui.ArkTheme
import dev.harken.android.ui.Chevron
import dev.harken.android.ui.CoverCard
import dev.harken.android.ui.Empty
import dev.harken.android.ui.ExpandingCard
import dev.harken.android.ui.Fetching
import dev.harken.android.ui.Heading
import dev.harken.android.ui.ListRow
import dev.harken.android.ui.LocalArkTheme
import dev.harken.android.ui.LocalReselect
import dev.harken.android.ui.PageBar
import dev.harken.android.ui.Reselect
import dev.harken.android.ui.ScrollsToTop
import dev.harken.android.ui.SheetRow
import dev.harken.android.ui.SignInPhase
import dev.harken.android.ui.SignInScreen
import dev.harken.android.ui.Space
import dev.harken.android.ui.Square
import dev.harken.android.ui.TabTitle
import dev.harken.android.ui.VolumeSlider
import dev.harken.android.ui.rememberExpandingCard
import kotlinx.coroutines.delay

class ShowcaseActivity : ComponentActivity() {
    /** Whether a recording's shot has been told to move. */
    private val go = mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val shot = intent.getStringExtra("shot") ?: "cards"
        go.value = intent.getBooleanExtra("go", false)
        setContent { Neutral { Showcase(shot, go.value) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (intent.getBooleanExtra("go", false)) go.value = true
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

/** One shot, by the id the docs give it; a recording's moves once `go`. */
@Composable
fun Showcase(shot: String, go: Boolean) {
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
                if (!go) return@LaunchedEffect
                delay(400)
                card.open()
                delay(1_500)
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
                if (!go) return@LaunchedEffect
                delay(400)
                list.animateScrollToItem(30)
                delay(800)
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
