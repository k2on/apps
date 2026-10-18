// harken, once the onboarding has handed it a session: three tabs — Home,
// Search, Library — under one bar, the pages they open growing over them
// (`ark-ui`'s `PageMotion`), the player's bar above the tabs and the full
// player it grows into over everything (PlayerSheet.kt, an `ExpandingCard`),
// and the sheets a song's menu raises — and the account's listening session (Listen.kt), which the
// player draws while this phone is part of it, and the device picker that moves the sound. This file
// wires; what each screen draws is the screen's, from plain values.
package dev.harken.android

import android.content.Context
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBars
import androidx.compose.material3.BasicAlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.rememberNavController
import dev.arkdb.Id
import dev.arkdb.android.ArkSession
import dev.arkdb.ui.LifecycleCapped
import dev.arkdb.ui.LocalCoverLoader
import dev.arkdb.ui.LocalPageMotion
import dev.arkdb.ui.Origin
import dev.arkdb.ui.PageBack
import dev.arkdb.ui.PageMotion
import dev.arkdb.ui.Recompositions
import dev.arkdb.ui.Reselect
import dev.arkdb.ui.TabStack
import dev.arkdb.ui.Space
import dev.arkdb.ui.Tapped
import dev.arkdb.ui.destinationKeepsAPlace
import dev.arkdb.ui.destinations
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/** The three tabs. */
enum class Tab(val route: String, val title: String) {
    Home("home", "Home"),
    Search("search", "Search"),
    Library("library", "Library");

    companion object {
        /** The routes that switch at once, where every other route is a page that grows (`ark-ui`'s `PageMotion`). */
        val routes: Set<String> = entries.map { it.route }.toSet()
    }
}

/**
 * The frame every tab is drawn in: the page, then the player's bar and the tab bar at its foot, and
 * the player over all of it. The app and the screenshots both draw it, so what is reviewed is what
 * ships.
 */
@Composable
fun Chrome(
    tab: Tab?,
    player: PlayerState,
    controls: Controls,
    onTab: (Tab) -> Unit,
    snackbar: SnackbarHostState? = null,
    /** Where in the track the player is: handed down to the mini player's hairline, never read here. */
    position: StateFlow<Long> = still(),
    /** Where another device's scrubber is: handed down to the full player's, never read here. */
    scrubbing: StateFlow<Long?> = nobody(),
    /** How far up the player is: read where the tab bar and the player are drawn, never here. */
    sheet: PlayerSheet = rememberPlayerSheet(),
    page: @Composable () -> Unit,
) {
    Recompositions.hit("Chrome")
    // The tab bar's height, which the player's bar stands on: Material's 80 and the system's bar.
    val tabs = 80.dp + WindowInsets.systemBars.asPaddingValues().calculateBottomPadding()
    Box(Modifier.fillMaxSize()) {
        Scaffold(
            snackbarHost = { if (snackbar != null) SnackbarHost(snackbar) },
            containerColor = MaterialTheme.colorScheme.background,
            bottomBar = {
                Column {
                    // Where the player's bar stands; the player draws it, over everything.
                    if (player.now != null) Spacer(Modifier.height(BAR + Space.xs * 2))
                    TabBar(tab, onTab, sheet)
                }
            },
        ) { padding ->
            Box(Modifier.padding(padding).fillMaxSize()) {
                // Under the player while it is up or on its way: drawn, not in front.
                LifecycleCapped({ if (sheet.p > 0f) Lifecycle.State.STARTED else Lifecycle.State.RESUMED }) { page() }
            }
        }
        PlayerCard(sheet, player, controls, position, tabs, scrubbing)
    }
}

// The three tabs, sliding away under the player as it comes up.
@Composable
private fun TabBar(tab: Tab?, onTab: (Tab) -> Unit, sheet: PlayerSheet) {
    NavigationBar(containerColor = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.graphicsLayer { translationY = size.height * sheet.p }) {
        for (t in Tab.entries) {
            NavigationBarItem(
                selected = t == tab,
                onClick = { onTab(t) },
                icon = {
                    Icon(
                        when (t) {
                            Tab.Home -> Glyphs.Home
                            Tab.Search -> Glyphs.Search
                            Tab.Library -> Glyphs.Library
                        },
                        contentDescription = null,
                    )
                },
                label = { Text(t.title) },
                colors =
                    NavigationBarItemDefaults.colors(
                        selectedIconColor = MaterialTheme.colorScheme.primary,
                        selectedTextColor = MaterialTheme.colorScheme.primary,
                        indicatorColor = MaterialTheme.colorScheme.secondaryContainer,
                    ),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HarkenApp(
    session: ArkSession,
    /** The player's end of it: the media session's, except in a test that drives the position itself. */
    playbackOf: (Context, String) -> Playback = ::Playback,
    /** Where the covers come from: the server's, except in a test that counts them. */
    coversOf: (Context, String) -> Covers = ::Covers,
    /** A tab of the Library to open on instead of Home — every song, say (a test's). */
    start: LibraryTab? = null,
    /** How the pages move, and how far up the player is: the app's own, or a test's to watch. */
    motion: PageMotion = remember { PageMotion(Tab.routes) },
    sheet: PlayerSheet = rememberPlayerSheet(),
    /** Told what a song's menu can do as it opens: a test's way to choose from a sheet it cannot reach. */
    onMenu: (MenuActions) -> Unit = {},
    /** Told each tab's stack once: a test's way to reach a destination, and to read the graphs back. */
    onNavs: (Map<Tab, NavHostController>) -> Unit = {},
) {
    Recompositions.hit("HarkenApp")
    val context = LocalContext.current.applicationContext
    // The library and the listening session live as long as the session does, not as long as
    // this composition (Kept.kt): the media service holds them too, and the activity going is
    // not the music going. The screens' own ends — the player's controller, the covers they
    // decode — are this composition's, and go with it.
    val kept = remember(session) { Kept.of(session, Device.name(context)) }
    val library = kept.library
    val listen = kept.listen
    val playback = remember(session) { playbackOf(context, session.server) }
    val covers = remember(session) { coversOf(context, session.server) }
    val device = remember { Device(context) }
    DisposableEffect(session) {
        playback.follow(listen)
        onDispose { playback.close() }
    }

    // One stack per tab, each its own NavHost (`TabStack`): switching tabs is not a navigation, so
    // what was on a tab is there again, at once, where it was left.
    var tab by rememberSaveable { mutableStateOf(if (start != null) Tab.Library else Tab.Home) }
    val navs = Tab.entries.associateWith { rememberNavController() }
    val reselects = remember { Tab.entries.associateWith { Reselect() } }
    LaunchedEffect(navs) { onNavs(navs) }

    /** The shown tab's stack. */
    fun nav(): NavHostController = navs.getValue(tab)

    fun show(t: Tab) {
        tab = t
    }

    /**
     * A tab tapped: another tab is shown as it was left; the one already shown goes back to its root,
     * with back's own motion, or, at its root, its list to its top.
     */
    fun select(t: Tab) {
        if (t != tab) {
            tab = t
            return
        }
        val n = navs.getValue(t)
        if (n.previousBackStackEntry != null) n.popBackStack(t.route, inclusive = false) else reselects.getValue(t).ask()
    }
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val tracks by library.tracks.collectAsStateWithLifecycle()
    val playlists by library.playlists.collectAsStateWithLifecycle()
    val albums by library.albums.collectAsStateWithLifecycle()
    val artists by library.artists.collectAsStateWithLifecycle()
    val details by library.details.collectAsStateWithLifecycle()
    val loaded by library.loaded.collectAsStateWithLifecycle()
    val complete by library.complete.collectAsStateWithLifecycle()
    val status by library.status.collectAsStateWithLifecycle()
    val notice by library.notice.collectAsStateWithLifecycle()
    val player by playback.state.collectAsStateWithLifecycle()
    val heard by listen.heard.collectAsStateWithLifecycle()
    val played by library.lastPlayed.collectAsStateWithLifecycle()
    var libraryTab by rememberSaveable { mutableStateOf(start ?: LibraryTab.Playlists) }
    var queue by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf<Menu?>(null) }
    var adding by remember { mutableStateOf<Track?>(null) }
    var naming by remember { mutableStateOf<Pair<Boolean, Track?>?>(null) }
    var devices by remember { mutableStateOf(false) }

    val offline = if (status.link == "waiting" || status.link == "offline") session.server.substringAfter("://") else null
    val fetching = if (!status.synced) session.server.substringAfter("://") else null
    val playing = player.now?.id

    // `albumOf`, `act` and `controls` are made once per session, as `library` and `playback` are.
    // Each reads the state above through its delegate when it is called, so it never goes stale —
    // and making a fresh one per composition is what made every screen, every lazy list and every
    // visible row recompose on each of the player's position ticks: an anonymous object is a new
    // value to Compose every time, and every row's `onClick` captured it (`ScrollTests`).
    //
    // The albums by name are made when the albums change, and only then: every row entering a list
    // asks for its album, and a scan of every album per row was a cost that grew with the library.
    val albumsByName by remember(session) { derivedStateOf { albums.associateBy { it.name } } }
    val albumOf: (Track) -> Album? = remember(session) { { t -> Album.ofTrack(t, details, albumsByName) } }
    // What the search last found: the app's, not the tab's, so leaving the tab keeps it.
    val searched = remember(session) { Searched() }
    // What Home says was played last: the session's track, else the log's newest play.
    val last = remember(heard, played, albumsByName, playlists) { lastPlayed(heard, played, playlists, { albumsByName[it]?.art ?: "" }, session.server, listen.name) }

    LaunchedEffect(notice) {
        val n = notice ?: return@LaunchedEffect
        snackbar.showSnackbar(n)
        library.dismiss()
    }

    val act = remember(session) {
        object : Do {
            override fun open(page: Page) {
                // Every song is a tab of the Library, not a page of its own.
                if (page is Page.Songs) library(LibraryTab.Songs) else nav().navigate(page.route)
            }

            override fun play(queue: List<Track>, at: Int, from: Page?) {
                playback.play(queue, at, albumOf, from = from?.place(playlists) ?: "")
            }

            override fun shuffle(queue: List<Track>, from: Page?) {
                playback.play(queue, queue.indices.random(), albumOf, shuffled = true, from = from?.place(playlists) ?: "")
            }

            override fun more(track: Track, from: Page?) {
                // The row says where it is as its menu opens: what a page the menu goes to grows from.
                menu = Menu(track, from, motion.tapped)
            }

            override fun resume(last: LastPlayed) {
                // The session's own track: resumed where the session has it, and the player opened.
                if (last.live) {
                    if (!player.playing) playback.resume()
                    scope.launch { sheet.open() }
                    return
                }
                scope.launch {
                    val list = last.from?.let { library.read(it) } ?: library.tracks.value
                    val at = list.indexOfFirst { it.id.hex == last.track.id }
                    if (at >= 0) play(list, at, last.from) else snackbar.showSnackbar("That song isn't in your library any more.")
                }
            }

            override fun newPlaylist() {
                naming = true to null
            }

            override fun library(tab: LibraryTab) {
                libraryTab = tab
                show(Tab.Library)
            }

            override fun settings() = nav().navigate(Route.Settings.pattern)

            override fun back() {
                nav().popBackStack()
            }
        }
    }

    val controls = remember(session) {
        object : Controls {
            override fun open() {
                scope.launch { sheet.open() }
            }

            override fun close() {
                scope.launch { sheet.close() }
            }

            override fun toggle() = playback.toggle()

            override fun next() = playback.next()

            override fun previous() = playback.previous()

            override fun seek(ms: Long) = playback.seek(ms)

            override fun shuffle() = playback.shuffle()

            override fun repeat() = playback.repeat()

            override fun queue() {
                queue = true
            }

            override fun skipAhead(n: Int) = playback.skipAhead(n)

            override fun devices() {
                devices = true
            }

            override fun scrub(ms: Long) = playback.scrub(ms)

            override fun volume(level: Int) = playback.volume(level)

            override fun volumeDone() = playback.volumeDone()

            override fun mute() = playback.mute()

            override fun more() {
                val now = player.now ?: return
                tracks.firstOrNull { it.id.hex == now.id }?.let { menu = Menu(it, null, null) }
            }
        }
    }

    CompositionLocalProvider(LocalCoverLoader provides covers, LocalPageMotion provides motion) {
        HarkenTheme {
            // Before the player, so that the player's own back gesture registers after it and is asked
            // first; and stood down while the player is up, whatever the order. At the root of Search
            // or of the Library, back goes Home; at Home's, it is the system's.
            PageBack(motion, blocked = { sheet.p > 0f }, toFirst = if (tab != Tab.Home) ({ tab = Tab.Home }) else null)
            Box(Modifier.fillMaxSize()) {
                Chrome(tab, player, controls, onTab = { select(it) }, snackbar, playback.position, playback.scrubbing, sheet) {
                    // Said for every move, so the NavHost's own cross-fade is nowhere: a page grows
                    // or shrinks, drawn by the page (`Moving`). Which it is, is decided here, where
                    // both ends of the move are known. A tab is not a move of any NavHost at all.
                    TabStack(motion, tab, Tab.Home, reselect = { reselects.getValue(it) }) { t ->
                        NavHost(
                            navs.getValue(t),
                            startDestination = t.route,
                            enterTransition = {
                                motion.push(initialState, targetState)
                                EnterTransition.None
                            },
                            exitTransition = {
                                motion.push(initialState, targetState)
                                ExitTransition.None
                            },
                            popEnterTransition = {
                                motion.pop(initialState, targetState)
                                EnterTransition.None
                            },
                            popExitTransition = {
                                motion.pop(initialState, targetState)
                                ExitTransition.None
                            },
                        ) {
                            // Every route this tab's stack can show, each a destination keeping
                            // its own places (`ark-ui`'s `destinations`); what each draws is below.
                            destinations(Route.entries.filter { it.on(t) }, motion, Route::pattern) { r, back ->
                                when (r) {
                                    Route.Home ->
                                        HomeScreen(HomeData(loaded, last, tracks, playlists, albums, offline, fetching), albumOf, act)
                                    Route.Search -> {
                                        var needle by rememberSaveable { mutableStateOf("") }
                                        var recent by remember { mutableStateOf(device.searches) }
                                        // Asked again only when the needle or the library moved: coming back to
                                        // the tab finds the answer it left, on its first frame, so its list has
                                        // the rows to restore its scroll into. With no answer at all — the
                                        // process was killed — it asks at once rather than after the typing beat.
                                        LaunchedEffect(needle, tracks, albums, artists) {
                                            if (needle.isBlank() || searched.answers(needle, tracks, albums, artists)) return@LaunchedEffect
                                            if (searched.results != null) delay(150)
                                            searched.answer(needle, tracks, albums, artists, library.search(needle.trim()))
                                            searched.results?.let { r -> covers.warm(searchArt(r, albumOf)) }
                                        }
                                        // A place to go back to and no answer to show it in: nothing, until there is.
                                        if (needle.isNotBlank() && searched.results == null && destinationKeepsAPlace()) return@destinations
                                        SearchScreen(
                                            needle,
                                            searched.results,
                                            recent,
                                            onNeedle = { needle = it },
                                            onSearched = {
                                                device.searched(it)
                                                recent = device.searches
                                            },
                                            onForget = {
                                                device.searches = emptyList()
                                                recent = emptyList()
                                            },
                                            albumOf,
                                            playing,
                                            offline,
                                            act,
                                        )
                                    }
                                    Route.Library ->
                                        LibraryScreen(libraryTab, { libraryTab = it }, LibraryData(loaded, playlists, albums, artists, tracks, offline, complete), albumOf, playing, act)
                                    Route.Album -> {
                                        val page = Page.parse("album/" + back.arguments?.getString("arg")) as? Page.ForAlbum
                                        if (page != null) {
                                            val rows by held(library, page, back)
                                            Ready(rows) { AlbumPage(page.name, albumsByName[page.name], it.orEmpty(), details, playing, act) }
                                        }
                                    }
                                    Route.Artist -> {
                                        val page = Page.parse("artist/" + back.arguments?.getString("arg")) as? Page.ForArtist
                                        if (page != null) {
                                            val rows by held(library, page, back)
                                            Ready(rows) { read ->
                                                val tracks = read.orEmpty()
                                                val theirs = tracks.mapNotNull { details[it.id]?.album?.takeIf(String::isNotEmpty) }.distinct()
                                                ArtistPage(
                                                    page.name,
                                                    artists.firstOrNull { it.name == page.name },
                                                    tracks,
                                                    theirs.mapNotNull { n -> albums.firstOrNull { it.name == n } },
                                                    albumOf,
                                                    playing,
                                                    act,
                                                )
                                            }
                                        }
                                    }
                                    Route.Playlist -> {
                                        val page = Page.parse("playlist/" + back.arguments?.getString("arg")) as? Page.ForPlaylist
                                        if (page != null) {
                                            val rows by held(library, page, back)
                                            Ready(rows) { r -> PlaylistPage(page.id, playlists.firstOrNull { it.id == page.id }, r.orEmpty(), albumOf, playing, act) }
                                        }
                                    }
                                    Route.Settings -> {
                                        var taps by remember { mutableIntStateOf(0) }
                                        var unlocked by remember { mutableStateOf(device.diagnostics) }
                                        SettingsScreen(
                                            SettingsData(library.name, library.user, library.email, library.server, status.linked, version(context), unlocked),
                                            onBack = act::back,
                                            onSignOut = library::signOut,
                                            onVersion = {
                                                taps++
                                                when {
                                                    taps >= 7 -> {
                                                        taps = 0
                                                        unlocked = true
                                                        device.diagnostics = true
                                                        nav().navigate(Route.Diagnostics.pattern)
                                                    }
                                                    taps >= 4 && !unlocked -> scope.launch { snackbar.showSnackbar("${7 - taps} more taps for diagnostics") }
                                                }
                                            },
                                            onDiagnostics = { nav().navigate(Route.Diagnostics.pattern) },
                                        )
                                    }
                                    Route.Diagnostics -> {
                                        // Held by the entry, as a page's rows are, and read again when the
                                        // session reports a change, not on a clock: come back to, it has its
                                        // numbers on its first frame.
                                        val reading = remember(back) { kept(back, "diagnostics") { Reading(library) } }
                                        val d by reading.diagnostics.collectAsStateWithLifecycle()
                                        val moved by library.moved.collectAsStateWithLifecycle()
                                        LaunchedEffect(moved) { reading.read() }
                                        Ready(d) { DiagnosticsScreen(it, onBack = act::back, onVerify = library::verify, heard = heard) }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            menu?.let { m ->
                val track = m.track
                val page = m.from
                val actions =
                    remember(m) {
                        object : MenuActions {
                            override fun addToQueue() {
                                playback.addToQueue(track, albumOf(track))
                            }

                            override fun playNext() {
                                playback.playNext(track, albumOf(track))
                                menu = null
                            }

                            override fun addTo() {
                                adding = track
                                menu = null
                            }

                            override fun goTo(page: Page) {
                                // The page grows out of the row the menu was about, and the sheet
                                // goes in the same frame, so the two do not overlap.
                                motion.from(m.origin(page))
                                menu = null
                                scope.launch { sheet.close() }
                                nav().navigate(page.route)
                            }

                            override fun remove() {
                                (page as? Page.ForPlaylist)?.let { library.removeFromPlaylist(track, it.id) }
                                menu = null
                            }
                        }
                    }
                LaunchedEffect(actions) { onMenu(actions) }
                ModalBottomSheet(onDismissRequest = { menu = null }) { TrackMenu(track, albumOf(track), page, actions) }
            }
            adding?.let { track ->
                var on by remember(track) { mutableStateOf<Set<Id>?>(null) }
                LaunchedEffect(track, playlists) { on = library.playlistsOf(track) }
                ModalBottomSheet(onDismissRequest = { adding = null }) {
                    AddToPlaylist(
                        track,
                        playlists,
                        on,
                        onToggle = { p ->
                            if (on?.contains(p.id) == true) library.removeFromPlaylist(track, p.id) else library.addToPlaylist(track, p)
                            on = (on ?: emptySet()).let { if (p.id in it) it - p.id else it + p.id }
                        },
                        onNew = {
                            adding = null
                            naming = true to track
                        },
                    )
                }
            }
            naming?.let { (_, then) ->
                var name by remember { mutableStateOf("") }
                var problem by remember { mutableStateOf<String?>(null) }
                LaunchedEffect(name) { problem = library.nameProblem(name) }
                BasicAlertDialog(onDismissRequest = { naming = null }) {
                    NewPlaylistCard(
                        name,
                        problem,
                        adding = then != null,
                        onName = { name = it },
                        onMake = {
                            library.createPlaylist(name, then)
                            naming = null
                        },
                        onCancel = { naming = null },
                    )
                }
            }
            if (queue) {
                ModalBottomSheet(onDismissRequest = { queue = false }) { QueueSheet(player, controls) }
            }
            if (devices) {
                val rows = heard.listening?.let { Picker.rows(it, heard.me) }.orEmpty()
                ModalBottomSheet(onDismissRequest = { devices = false }) {
                    DevicesSheet(
                        rows,
                        Picker.cursor(rows),
                        object : PickerActions {
                            override fun pick(row: DeviceRow) {
                                Picker.pick(row, heard.listening)?.let { (it as? Press.Transfer)?.let { t -> playback.transfer(t.to) } }
                                devices = false
                            }

                            override fun level(row: DeviceRow, level: Int) = playback.level(row.level, level)

                            override fun levelDone() = playback.volumeDone()

                            override fun mute(row: DeviceRow) {
                                Picker.mute(row)?.let(playback::ask)
                            }

                            override fun join(row: DeviceRow) {
                                Picker.join(row)?.let(playback::ask)
                            }
                        },
                    )
                }
            }
        }
    }
}

/**
 * Every destination: the tabs' roots and every page. Each tab's NavHost is built from this list
 * (`ark-ui`'s `destinations`), so nothing is drawn on a stack without being one — keeping its
 * places, seeing its own lifecycle — and the restoration tests walk the same list, so a new route
 * is covered the day it is added.
 */
enum class Route(val pattern: String, val tab: Tab?) {
    Home("home", Tab.Home),
    Search("search", Tab.Search),
    Library("library", Tab.Library),
    Album("album/{arg}", null),
    Artist("artist/{arg}", null),
    Playlist("playlist/{arg}", null),
    Settings("settings", null),
    Diagnostics("diagnostics", null);

    /** Whether `t`'s stack can show it: that tab's own root, or any page. */
    fun on(t: Tab): Boolean = tab == null || tab == t
}

/**
 * `content`, once what it draws is there — or at once, with nothing, when the destination has no
 * place to go back to. A destination come back to with its rows not yet read (after the process was
 * killed) draws nothing for those milliseconds rather than a list too short to hold its place, which
 * would clamp it to the top.
 */
@Composable
private fun <T : Any> Ready(value: T?, content: @Composable (T?) -> Unit) {
    if (value == null && destinationKeepsAPlace()) return
    content(value)
}

/** A holder that lives as long as `entry` is on its stack: made once, by `make`, under `key`. */
@Suppress("UNCHECKED_CAST")
private fun <V : ViewModel> kept(entry: NavBackStackEntry, key: String, make: () -> V): V =
    ViewModelProvider(entry, object : ViewModelProvider.Factory {
        override fun <T : ViewModel> create(modelClass: Class<T>): T = make() as T
    })[key, ViewModel::class.java] as V

/**
 * A page's list, held for as long as its entry is on the back stack — not only while it is drawn —
 * so that coming back to it, from a page it opened or from another tab, finds its rows on the first
 * frame: its list restores its place into rows that are there, rather than into an empty list that
 * clamps it to the top. Null until the page's view has first been read. Let go when the entry is
 * popped.
 */
private class Holding(val library: Library, val page: Page) : ViewModel() {
    val rows = library.hold(page)

    override fun onCleared() = library.release(page)
}

@Composable
private fun held(library: Library, page: Page, entry: NavBackStackEntry): androidx.compose.runtime.State<List<Track>?> {
    val holding = remember(entry, page) { kept(entry, "held:" + page.route) { Holding(library, page) } }
    return holding.rows.collectAsStateWithLifecycle()
}

/** The diagnostics' numbers, held by their entry as a page's rows are. */
private class Reading(val library: Library) : ViewModel() {
    val diagnostics = kotlinx.coroutines.flow.MutableStateFlow<Diagnostics?>(null)

    suspend fun read() {
        diagnostics.value = library.diagnostics()
    }
}

/** What a search last found, and what it was asked of: one per session, so it outlives the tab. */
@Stable
class Searched {
    var results by mutableStateOf<Results?>(null)
        private set

    private var asked: List<Any?> = emptyList()

    /** Whether the results here are already the answer for this needle over these lists. */
    fun answers(needle: String, vararg over: List<*>): Boolean = asked.size == over.size + 1 && asked[0] == needle.trim() && over.indices.all { asked[it + 1] === over[it] }

    fun answer(needle: String, tracks: List<Track>, albums: List<Album>, artists: List<Artist>, songs: List<Track>) {
        results = Results.of(needle, songs, albums, artists)
        asked = listOf(needle.trim(), tracks, albums, artists)
    }
}

/** Every cover a search's results draw, at the size each is drawn: the songs' first, as they come first. */
private fun searchArt(r: Results, albumOf: (Track) -> Album?): List<Pair<String, androidx.compose.ui.unit.Dp>> =
    r.songs.take(SEARCH_SONGS).mapNotNull { t -> albumOf(t)?.art?.let { it to Space.thumb } } + r.albums.map { it.art to SEARCH_ALBUM } + r.artists.map { it.art to SEARCH_ARTIST }

/** A song's menu: the song, the page its row was on, and where the row was when it opened. */
class Menu(val track: Track, val from: Page?, val tapped: Tapped?) {
    /**
     * What a page the menu goes to grows out of: its album out of the row's cover, which is that
     * album's art; anything else — its artist — out of the row itself; and nothing, for a menu that
     * came from no row (the full player's), so that page rises.
     */
    fun origin(to: Page): Origin? = if (to is Page.ForAlbum) tapped?.art ?: tapped?.row else tapped?.row
}

/** "0.1 (1)", from the package. */
private fun version(context: android.content.Context): String =
    try {
        val info = context.packageManager.getPackageInfo(context.packageName, 0)
        "${info.versionName} (${androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(info)})"
    } catch (e: Exception) {
        "unknown"
    }
