// Where a destination's state lives. A destination — a tab's root, a page —
// is one back stack entry, and what it was showing when it was left belongs
// to that entry, not to whichever composition happens to be drawing it: the
// tab shown, the tab drawn under one going back to it, a page under the page
// going back off it, a page come back after the process was killed. Each of
// those used to be a separate composition finding its scroll by where in the
// tree it was called from, and every place that called it from somewhere
// else drew the list at its top for a frame — Search after an album, an
// album after its composer, Home under the tab back preview.
//
// So every scrolling container of a destination takes its state from here
// (`rememberDestinationListState`, `…GridState`, `…ScrollState`), keyed by a
// name within the destination, and the destination keeps the places in a
// holder on its entry (a `ViewModel`, saved with the entry's own saved
// state): the same answer wherever and however often it is composed, on its
// first frame, and after the process is killed. What is not a destination —
// a sheet, a dialog, an onboarding page — opens at its top every time and
// says so (`rememberTransientListState`, `…ScrollState`). Nothing else in an
// app makes a scroll state; a source scan in harken's tests holds it to that.
//
// And how far on a destination is: its content sees the lifecycle of where
// it is drawn (`LocalLifecycleOwner`) — resumed in front at rest, started
// while it is drawn and not in front (under a back preview, behind a card
// that is up, either end of a move), and created while it is not drawn at
// all. A NavHost already moves its entries so; `TabStack` and
// `LifecycleCapped` say the same of a tab and of what is behind a card.
package dev.arkdb.ui

import android.os.Bundle
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.lifecycle.HasDefaultViewModelProviderFactory
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.viewmodel.CreationExtras
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavGraph
import androidx.navigation.NavGraphBuilder
import androidx.navigation.compose.composable

/**
 * One destination's places: where each of its scrolling containers was, by name, kept on its back
 * stack entry for as long as the entry is on the stack, and saved with it. While a container is
 * drawn its live state is the answer; once it is not, the place it was left at is.
 */
@Stable
public class DestinationState
internal constructor(
    /** What the destination is: its route, as the app named it. */
    public val name: String,
    restored: Map<String, IntArray> = emptyMap(),
) {
    private val kept = HashMap(restored)
    private val live = LinkedHashMap<String, Any>()

    /** How many compositions of this destination there are now: never more than one. */
    public var composed: Int = 0
        internal set

    /**
     * How many times a composition of it has begun: a test's view of "drawn again, from scratch".
     */
    public var entered: Int = 0
        internal set

    /** The lifecycle its content sees, as it was last composed: a test's view of it. */
    public var lifecycle: Lifecycle? = null
        internal set

    /**
     * Whether it was left somewhere other than the top of every container: what a destination whose
     * rows are not there yet asks before drawing a list that would lose that place to its top.
     */
    public val keepsAPlace: Boolean
        get() = kept.any { (k, v) -> k !in live && v.any { it != 0 } }

    /**
     * Every container's place now, by name: a list's or a grid's (index, offset), a column's
     * (value).
     */
    public fun places(): Map<String, List<Int>> =
        (kept.mapValues { it.value.toList() } + live.mapValues { placeOf(it.value).toList() })

    /** The state drawing `key` now, while it is drawn: a test's view of what is on screen. */
    public fun drawn(key: String): Any? = live[key]

    /** Every name drawn now. */
    public val drawnKeys: Set<String>
        get() = live.keys.toSet()

    internal fun list(key: String): LazyListState =
        kept[key].let { LazyListState(it?.getOrNull(0) ?: 0, it?.getOrNull(1) ?: 0) }

    internal fun grid(key: String): LazyGridState =
        kept[key].let { LazyGridState(it?.getOrNull(0) ?: 0, it?.getOrNull(1) ?: 0) }

    internal fun scroll(key: String): ScrollState = ScrollState(kept[key]?.getOrNull(0) ?: 0)

    internal fun attach(key: String, state: Any) {
        val there = live[key]
        check(there == null || there === state) {
            "two containers of $name are both kept as \"$key\": give each its own name"
        }
        live[key] = state
    }

    internal fun detach(key: String, state: Any) {
        if (live[key] !== state) return
        kept[key] = placeOf(state)
        live.remove(key)
    }

    /** Every place, the live ones read now: what is saved with the entry. */
    internal fun save(): Map<String, IntArray> = kept + live.mapValues { placeOf(it.value) }

    private fun placeOf(s: Any): IntArray =
        when (s) {
            is LazyListState -> intArrayOf(s.firstVisibleItemIndex, s.firstVisibleItemScrollOffset)
            is LazyGridState -> intArrayOf(s.firstVisibleItemIndex, s.firstVisibleItemScrollOffset)
            is ScrollState -> intArrayOf(s.value)
            else -> error("not a scroll state: $s")
        }
}

/** The destination being drawn, or none outside one (a screenshot of a screen on its own). */
public val LocalDestination: ProvidableCompositionLocal<DestinationState?> =
    staticCompositionLocalOf {
        null
    }

/**
 * Every destination as it enters composition, while a test listens; null, always, in the app. What
 * the restoration tests read every container's place from.
 */
public object Destinations {
    @Volatile public var sink: ((DestinationState) -> Unit)? = null
}

/**
 * Draw `content` as the destination `owner` is — a back stack entry, which keeps its places, saved
 * with its own state. [destinations] puts every route of a graph in one.
 */
@Composable
public fun Destination(owner: ViewModelStoreOwner, name: String, content: @Composable () -> Unit) {
    val state = remember(owner) { holderOf(owner, name).state }
    val life = LocalLifecycleOwner.current.lifecycle
    SideEffect { state.lifecycle = life }
    DisposableEffect(state) {
        state.composed++
        state.entered++
        Destinations.sink?.invoke(state)
        onDispose { state.composed-- }
    }
    CompositionLocalProvider(LocalDestination provides state, content = content)
}

/**
 * Every route of `all` as a destination of the graph: each drawn in the navigation's motion
 * ([Moving]) and keeping its places ([Destination]). The app's list of routes is the graph, so a
 * route cannot be drawn without being one; [routes] reads a built graph back, for a test to hold
 * the list and the graph to each other.
 */
public fun <D> NavGraphBuilder.destinations(
    all: Iterable<D>,
    motion: PageMotion,
    route: (D) -> String,
    content: @Composable (D, NavBackStackEntry) -> Unit,
) {
    for (d in all) {
        val r = route(d)
        composable(r) { entry ->
            Moving(motion, entry) { Destination(entry, r) { content(d, entry) } }
        }
    }
}

/** Every route a graph can show, its nested graphs' included. */
public fun NavGraph.routes(): Set<String> =
    flatMap { d -> if (d is NavGraph) d.routes() else listOfNotNull(d.route) }.toSet()

/**
 * A vertical or horizontal list's state, kept by the destination it is in as `key`: where it was
 * left, on its first frame, wherever the destination is drawn. Outside a destination, the
 * composition's, as `rememberLazyListState` would be.
 */
@Composable
public fun rememberDestinationListState(key: String): LazyListState {
    val d = LocalDestination.current
    if (d == null) return rememberSaveable(saver = LazyListState.Saver) { LazyListState() }
    val s = remember(d, key) { d.list(key) }
    DisposableEffect(d, key, s) {
        d.attach(key, s)
        onDispose { d.detach(key, s) }
    }
    return s
}

/** …a grid's. */
@Composable
public fun rememberDestinationGridState(key: String): LazyGridState {
    val d = LocalDestination.current
    if (d == null) return rememberSaveable(saver = LazyGridState.Saver) { LazyGridState() }
    val s = remember(d, key) { d.grid(key) }
    DisposableEffect(d, key, s) {
        d.attach(key, s)
        onDispose { d.detach(key, s) }
    }
    return s
}

/** …and a scrolling column's or row's. */
@Composable
public fun rememberDestinationScrollState(key: String): ScrollState {
    val d = LocalDestination.current
    if (d == null) return rememberSaveable(saver = ScrollState.Saver) { ScrollState(0) }
    val s = remember(d, key) { d.scroll(key) }
    DisposableEffect(d, key, s) {
        d.attach(key, s)
        onDispose { d.detach(key, s) }
    }
    return s
}

/**
 * Whether the destination being drawn was left somewhere other than its top
 * ([DestinationState.keepsAPlace]).
 */
@Composable
public fun destinationKeepsAPlace(): Boolean = LocalDestination.current?.keepsAPlace == true

/**
 * A list that is not a destination's — a sheet's, a dialog's — at its top each time it opens. Never
 * a destination's: those are [rememberDestinationListState].
 */
@Composable
public fun rememberTransientListState(): LazyListState =
    rememberSaveable(saver = LazyListState.Saver) { LazyListState() }

/** …a column that is not a destination's: an onboarding page's. */
@Composable
public fun rememberTransientScrollState(): ScrollState =
    rememberSaveable(saver = ScrollState.Saver) { ScrollState(0) }

// The holder on the entry: its places for as long as it is on the stack, and saved with it.
private class Held(handle: SavedStateHandle?, name: String) : ViewModel() {
    val state = DestinationState(name, handle?.get<Bundle>(SAVED)?.let(::read).orEmpty())

    init {
        handle?.setSavedStateProvider(SAVED) { write(state.save()) }
    }
}

private const val SAVED = "dev.arkdb.ui.destination"

private fun read(b: Bundle): Map<String, IntArray> =
    b.keySet().mapNotNull { k -> b.getIntArray(k)?.let { k to it } }.toMap()

private fun write(places: Map<String, IntArray>): Bundle =
    Bundle().apply { for ((k, v) in places) putIntArray(k, v) }

private fun holderOf(owner: ViewModelStoreOwner, name: String): Held {
    val extras =
        (owner as? HasDefaultViewModelProviderFactory)?.defaultViewModelCreationExtras
            ?: CreationExtras.Empty
    val factory =
        object : ViewModelProvider.Factory {
            override fun <T : ViewModel> create(modelClass: Class<T>, extras: CreationExtras): T {
                // An owner with no saved state of its own keeps the places for as long as it lives.
                val handle =
                    try {
                        extras.createSavedStateHandle()
                    } catch (e: IllegalArgumentException) {
                        null
                    } catch (e: IllegalStateException) {
                        null
                    }
                @Suppress("UNCHECKED_CAST") return Held(handle, name) as T
            }
        }
    return ViewModelProvider(owner.viewModelStore, factory, extras)[SAVED, Held::class.java]
}

/**
 * A lifecycle no further on than its parent's, nor than `most()`: what is under something drawn
 * over it is [Lifecycle.State.STARTED] — visible, not in front — however resumed the activity is.
 * `most` is read as a snapshot, never in composition, so a card moving every frame composes
 * nothing.
 */
@Composable
public fun LifecycleCapped(most: () -> Lifecycle.State, content: @Composable () -> Unit) {
    val parent = LocalLifecycleOwner.current
    val owner =
        remember(parent) {
            CappedLifecycle(parent.lifecycle, Snapshot.withoutReadObservation(most))
        }
    DisposableEffect(owner) {
        owner.start()
        onDispose { owner.stop() }
    }
    LaunchedEffect(owner) { snapshotFlow(most).collect { owner.cap = it } }
    CompositionLocalProvider(LocalLifecycleOwner provides owner, content = content)
}

/**
 * A lifecycle that follows `parent`'s, held to at most `cap`, and never below created while the
 * parent is not destroyed: one that went back to created comes forward again.
 */
internal class CappedLifecycle(private val parent: Lifecycle, cap: Lifecycle.State) :
    LifecycleOwner, LifecycleEventObserver {
    private val registry = LifecycleRegistry(this)

    override val lifecycle: Lifecycle
        get() = registry

    var cap: Lifecycle.State = cap
        set(v) {
            field = v
            sync()
        }

    private var watching = false

    fun start() {
        if (watching) return
        watching = true
        parent.addObserver(this)
        sync()
    }

    fun stop() {
        if (!watching) return
        watching = false
        parent.removeObserver(this)
        // Not drawn: no further on than created, and back again when it is.
        if (registry.currentState.isAtLeast(Lifecycle.State.CREATED))
            registry.currentState = minOf(registry.currentState, Lifecycle.State.CREATED)
    }

    override fun onStateChanged(source: LifecycleOwner, event: Lifecycle.Event) = sync()

    private fun sync() {
        if (registry.currentState == Lifecycle.State.DESTROYED) return
        val p = parent.currentState
        if (p == Lifecycle.State.INITIALIZED) return
        if (p == Lifecycle.State.DESTROYED) {
            if (registry.currentState != Lifecycle.State.INITIALIZED)
                registry.currentState = Lifecycle.State.DESTROYED
            return
        }
        registry.currentState =
            maxOf(Lifecycle.State.CREATED, minOf(p, if (watching) cap else Lifecycle.State.CREATED))
    }
}
