// The player as one thing with two faces: the bar above the tabs and the
// full player are one card, `ark-ui`'s `ExpandingCard`, and how far it has
// come up is one number, `p`, 0 at the bar and 1 covering the screen. What
// is harken's is what the card shows: the mini player as its collapsed face
// (`MiniFace`, without its cover), the full player as its expanded face
// (`FullFace`, leaving a slot for the cover), and the cover itself, which
// the card draws as it travels between the two. A sideways drag on the bar
// skips — left the next track, right the one before — and a tap opens it.
// The tab bar slides away under it (HarkenApp.kt). How it moves, settles and
// answers the back gesture is the card's, and `docs/plan-android.md` says
// why.
package dev.harken.android

import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import dev.arkui.Cover
import dev.arkui.ExpandingCard
import dev.arkui.ExpandingCardState
import dev.arkui.rememberExpandingCard
import kotlinx.coroutines.flow.StateFlow

/** The player's card: how far up it is, and the drag that moves it (`ark-ui`'s). */
typealias PlayerSheet = ExpandingCardState

/** The player's card, at the bar. */
@Composable
fun rememberPlayerSheet(): PlayerSheet = rememberExpandingCard()

/** The bar's height: its row and the hairline under it. */
val BAR = 58.dp

/**
 * The player over the page and the tabs: the dim, and the card with its two faces and the art. `tabs`
 * is the tab bar's height, which the bar stands on.
 */
@Composable
fun PlayerCard(sheet: PlayerSheet, state: PlayerState, controls: Controls, position: StateFlow<Long>, tabs: Dp, scrubbing: StateFlow<Long?> = nobody()) {
    val now = state.now ?: return
    val seed = now.album.ifEmpty { now.title }
    ExpandingCard(
        sheet,
        stand = tabs,
        onOpen = controls::open,
        collapsed = { m -> MiniFace(state, controls, position, art = false, modifier = m) },
        expanded = { slot, m -> FullFace(state, controls, position, slot = slot, modifier = m, scrubbing = scrubbing) },
        traveler = { m -> Cover(seed, now.art, 360.dp, m, corner = 0.dp) },
        onSwipeLeft = controls::next,
        onSwipeRight = controls::previous,
        openLabel = "Open the player",
    )
}
