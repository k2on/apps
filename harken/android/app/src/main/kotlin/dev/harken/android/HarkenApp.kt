// harken, once the onboarding has handed it a session: three tabs — Home,
// Search, Library — under one bar, the pages they open pushed over them,
// the mini player above the bar and the full player it grows into over
// everything, and the sheets a song's menu raises. This file wires; what
// each screen draws is the screen's, from plain values.
package dev.harken.android

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import dev.arkdb.Id
import dev.arkdb.android.ArkSession
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** The three tabs. */
enum class Tab(val route: String, val title: String) {
    Home("home", "Home"),
    Search("search", "Search"),
    Library("library", "Library"),
}

/**
 * The frame every tab is drawn in: the page, then the mini player and the tab bar at its foot.
 * The app and the screenshots both draw it, so what is reviewed is what ships.
 */
@Composable
fun Chrome(
    tab: Tab?,
    player: PlayerState,
    controls: Controls,
    onTab: (Tab) -> Unit,
    snackbar: SnackbarHostState? = null,
    page: @Composable () -> Unit,
) {
    Scaffold(
        snackbarHost = { if (snackbar != null) SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
        bottomBar = {
            Column {
                MiniPlayer(player, controls)
                NavigationBar(containerColor = MaterialTheme.colorScheme.surfaceContainer) {
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
        },
    ) { padding -> Box(Modifier.padding(padding).fillMaxSize()) { page() } }
}

private fun NavHostController.tab(t: Tab) =
    navigate(t.route) {
        popUpTo(graph.findStartDestination().id) { saveState = true }
        launchSingleTop = true
        restoreState = true
    }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HarkenApp(session: ArkSession) {
    val context = LocalContext.current.applicationContext
    val library = remember(session) { Library(session) }
    val playback = remember(session) { Playback(context, session.server) }
    val covers = remember(session) { Covers(context, session.server) }
    val device = remember { Device(context) }
    DisposableEffect(session) {
        onDispose {
            library.close()
            playback.close()
        }
    }

    val nav = rememberNavController()
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val tracks by library.tracks.collectAsStateWithLifecycle()
    val playlists by library.playlists.collectAsStateWithLifecycle()
    val albums by library.albums.collectAsStateWithLifecycle()
    val artists by library.artists.collectAsStateWithLifecycle()
    val details by library.details.collectAsStateWithLifecycle()
    val loaded by library.loaded.collectAsStateWithLifecycle()
    val status by library.status.collectAsStateWithLifecycle()
    val notice by library.notice.collectAsStateWithLifecycle()
    val player by playback.state.collectAsStateWithLifecycle()
    var last by remember { mutableStateOf(device.lastPlayed) }
    var libraryTab by rememberSaveable { mutableStateOf(LibraryTab.Playlists) }
    var expanded by rememberSaveable { mutableStateOf(false) }
    var queue by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf<Pair<Track, Page?>?>(null) }
    var adding by remember { mutableStateOf<Track?>(null) }
    var naming by remember { mutableStateOf<Pair<Boolean, Track?>?>(null) }
    var startedFrom by remember { mutableStateOf<Page?>(null) }

    val offline = if (status.link == "waiting" || status.link == "offline") session.server.substringAfter("://") else null
    val playing = player.now?.id
    val albumOf: (Track) -> Album? = { t ->
        details[t.id]?.album?.takeIf { it.isNotEmpty() }?.let { name -> albums.firstOrNull { it.name == name } ?: Album(name, t.creator, "", 0) }
    }

    playback.onStart = { p ->
        val l = LastPlayed(p, startedFrom)
        device.lastPlayed = l
        last = l
    }

    LaunchedEffect(notice) {
        val n = notice ?: return@LaunchedEffect
        snackbar.showSnackbar(n)
        library.dismiss()
    }

    val act =
        object : Do {
            override fun open(page: Page) = nav.navigate(page.route)

            override fun play(queue: List<Track>, at: Int, from: Page?) {
                startedFrom = from
                playback.play(queue, at, albumOf)
            }

            override fun shuffle(queue: List<Track>, from: Page?) {
                startedFrom = from
                playback.play(queue, queue.indices.random(), albumOf, shuffled = true)
            }

            override fun more(track: Track, from: Page?) {
                menu = track to from
            }

            override fun resume(last: LastPlayed) {
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
                nav.tab(Tab.Library)
            }

            override fun settings() = nav.navigate("settings")

            override fun back() {
                nav.popBackStack()
            }
        }

    val controls =
        object : Controls {
            override fun open() {
                expanded = true
            }

            override fun close() {
                expanded = false
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

            override fun more() {
                val now = player.now ?: return
                tracks.firstOrNull { it.id.hex == now.id }?.let { menu = it to null }
            }
        }

    CompositionLocalProvider(LocalCovers provides covers) {
        HarkenTheme {
            val entry by nav.currentBackStackEntryAsState()
            val route = entry?.destination?.route
            val tab = Tab.entries.firstOrNull { it.route == route }
            Box(Modifier.fillMaxSize()) {
                Chrome(tab, player, controls, onTab = { nav.tab(it) }, snackbar) {
                    NavHost(nav, startDestination = Tab.Home.route) {
                        composable(Tab.Home.route) {
                            HomeScreen(HomeData(loaded, last, tracks, playlists, albums, offline), albumOf, act)
                        }
                        composable(Tab.Search.route) {
                            var needle by rememberSaveable { mutableStateOf("") }
                            var results by remember { mutableStateOf<Results?>(null) }
                            var recent by remember { mutableStateOf(device.searches) }
                            LaunchedEffect(needle, tracks, albums, artists) {
                                if (needle.isBlank()) return@LaunchedEffect
                                delay(150)
                                results = Results.of(needle, library.search(needle.trim()), albums, artists)
                            }
                            SearchScreen(
                                needle,
                                results,
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
                        composable(Tab.Library.route) {
                            LibraryScreen(libraryTab, { libraryTab = it }, LibraryData(loaded, playlists, albums, artists, tracks, offline), albumOf, playing, act)
                        }
                        composable("album/{arg}") { back ->
                            val page = Page.parse("album/" + back.arguments?.getString("arg")) as? Page.ForAlbum
                            if (page != null) {
                                val rows by held(library, page)
                                AlbumPage(page.name, albums.firstOrNull { it.name == page.name }, rows, details, playing, act)
                            }
                        }
                        composable("artist/{arg}") { back ->
                            val page = Page.parse("artist/" + back.arguments?.getString("arg")) as? Page.ForArtist
                            if (page != null) {
                                val rows by held(library, page)
                                val theirs = rows.mapNotNull { details[it.id]?.album?.takeIf(String::isNotEmpty) }.distinct()
                                ArtistPage(
                                    page.name,
                                    artists.firstOrNull { it.name == page.name },
                                    rows,
                                    theirs.mapNotNull { n -> albums.firstOrNull { it.name == n } },
                                    albumOf,
                                    playing,
                                    act,
                                )
                            }
                        }
                        composable("playlist/{arg}") { back ->
                            val page = Page.parse("playlist/" + back.arguments?.getString("arg")) as? Page.ForPlaylist
                            if (page != null) {
                                val rows by held(library, page)
                                PlaylistPage(page.id, playlists.firstOrNull { it.id == page.id }, rows, albumOf, playing, act)
                            }
                        }
                        composable("settings") {
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
                                            nav.navigate("diagnostics")
                                        }
                                        taps >= 4 && !unlocked -> scope.launch { snackbar.showSnackbar("${7 - taps} more taps for diagnostics") }
                                    }
                                },
                                onDiagnostics = { nav.navigate("diagnostics") },
                            )
                        }
                        composable("diagnostics") {
                            var d by remember { mutableStateOf<Diagnostics?>(null) }
                            LaunchedEffect(Unit) {
                                while (true) {
                                    d = library.diagnostics()
                                    delay(500)
                                }
                            }
                            DiagnosticsScreen(d, onBack = act::back, onVerify = library::verify)
                        }
                    }
                }
                AnimatedVisibility(expanded, enter = slideInVertically { it }, exit = slideOutVertically { it }) {
                    FullPlayer(player, controls)
                }
            }
            BackHandler(expanded) { expanded = false }

            menu?.let { (track, page) ->
                ModalBottomSheet(onDismissRequest = { menu = null }) {
                    TrackMenu(
                        track,
                        albumOf(track),
                        page,
                        object : MenuActions {
                            override fun playNext() {
                                playback.playNext(track, albumOf(track))
                                menu = null
                            }

                            override fun addTo() {
                                adding = track
                                menu = null
                            }

                            override fun goTo(page: Page) {
                                menu = null
                                expanded = false
                                nav.navigate(page.route)
                            }

                            override fun remove() {
                                (page as? Page.ForPlaylist)?.let { library.removeFromPlaylist(track, it.id) }
                                menu = null
                            }
                        },
                    )
                }
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
        }
    }
}

// A page's list, held while the page is shown.
@Composable
private fun held(library: Library, page: Page): androidx.compose.runtime.State<List<Track>> {
    val flow = remember(page) { library.hold(page) }
    DisposableEffect(page) { onDispose { library.release(page) } }
    return flow.collectAsStateWithLifecycle()
}

/** "0.1 (1)", from the package. */
private fun version(context: android.content.Context): String =
    try {
        val info = context.packageManager.getPackageInfo(context.packageName, 0)
        "${info.versionName} (${androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(info)})"
    } catch (e: Exception) {
        "unknown"
    }
