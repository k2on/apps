// The comings and goings an app asks of Compose's own containers — a
// NavHost's pages, an AnimatedContent's steps, an AnimatedVisibility's bar —
// said once, on arkui's springs, so two apps moving a page move it alike and
// neither writes a number of its own.
//
// A page pushed over another comes in a little way from the end it is going
// toward and fades in, and the page under it goes a little the other way; a
// page popped retraces that path, so the back gesture drives it in reverse
// along the way it came. A step through something ordered — a month, a
// sheet's next leaf — does the same between its two ends, the direction
// following the step. A bar on an edge comes in from that edge and leaves
// through it. Each slides by [Springs.SHIFT] of its width, critically damped
// (a tap moved it, nothing threw it), and fades in on the spring for a size
// and out on the quicker [Springs.fade]: exits are quicker than entries.
//
// Peers are not moves. A tab is there at once ([none]), as harken's are: a
// slide or a zoom between tabs implies a depth that is not there, and a tab is
// switched far too often for any motion to read as anything but lag.
//
// With motion reduced, nothing travels: every one of these is a quick
// cross-fade in place.
package dev.arkui

import androidx.compose.animation.ContentTransform
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

public object Transitions {
    /** A page pushed over the one showing, coming in from the end. */
    public fun pushIn(reduced: Boolean): EnterTransition = across(1, reduced)

    /** The page under one pushed, going a little toward the start. */
    public fun pushOut(reduced: Boolean): ExitTransition = away(-1, reduced)

    /** The page under one popped, coming back from the start. */
    public fun popIn(reduced: Boolean): EnterTransition = across(-1, reduced)

    /** A page popped, going back toward the end it came in from. */
    public fun popOut(reduced: Boolean): ExitTransition = away(1, reduced)

    /**
     * One step of something ordered replaced by the next: in from the side it was stepped toward
     * (`forward`, the end; otherwise the start) and the old one out the other side.
     */
    public fun stepped(forward: Boolean, reduced: Boolean): ContentTransform {
        val d = if (forward) 1 else -1
        return across(d, reduced) togetherWith away(-d, reduced)
    }

    /** A bar at the foot coming in from below it. */
    public fun riseIn(reduced: Boolean): EnterTransition =
        if (reduced) fadeIn(Springs.fade())
        else slideInVertically(Springs.slide()) { it } + fadeIn(Springs.size())

    /** A bar at the foot going down through it. */
    public fun sinkOut(reduced: Boolean): ExitTransition =
        if (reduced) fadeOut(Springs.fade())
        else slideOutVertically(Springs.slide()) { it } + fadeOut(Springs.fade())

    /** Something swapped in place, whatever motion is asked: a cross-fade. */
    public fun crossFade(): ContentTransform = fadeIn(Springs.fade()) togetherWith fadeOut(Springs.fade())

    /** A peer for a peer — a tab for a tab — with no motion at all: there on the next frame. */
    public val none: ContentTransform = EnterTransition.None togetherWith ExitTransition.None

    /** In from `d` (1 the end, -1 the start), [Springs.SHIFT] of its width. */
    private fun across(d: Int, reduced: Boolean): EnterTransition =
        if (reduced) fadeIn(Springs.fade())
        else slideInHorizontally(Springs.slide()) { w -> (d * w * Springs.SHIFT).toInt() } + fadeIn(Springs.size())

    /** Out toward `d`, [Springs.SHIFT] of its width, quicker than it came. */
    private fun away(d: Int, reduced: Boolean): ExitTransition =
        if (reduced) fadeOut(Springs.fade())
        else slideOutHorizontally(Springs.slide()) { w -> (d * w * Springs.SHIFT).toInt() } + fadeOut(Springs.fade())
}

/**
 * A lazy list's row as rows come and go around it: one arriving fades in, one leaving fades out
 * and stops taking room at once, and the others close the gap on arkui's spring for a move —
 * together, not after. With motion reduced the others are simply where they now are; the fades
 * stay, since they say what changed.
 */
@Composable
public fun LazyItemScope.reflowing(): Modifier =
    Modifier.animateItem(
        fadeInSpec = Springs.fade(),
        placementSpec = if (reducedMotion()) null else Springs.slide(),
        fadeOutSpec = Springs.fade(),
    )
