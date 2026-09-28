// Navigation: three tabs — Library, Playlists, Settings — and the one page
// pushed over them, a playlist. A snackbar carries refusals and rejections;
// the top bar says whether the peer is signed out, linked, alone, not synced
// or turned away.
package dev.harken.android

import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import dev.arkdb.Id
import dev.arkdb.client.Status

object Routes {
    const val LIBRARY = "library"
    const val PLAYLISTS = "playlists"
    const val PLAYLIST = "playlist/{id}"
    const val SETTINGS = "settings"

    fun playlist(id: Id): String = "playlist/${id.hex}"
}

/** One line about the peer, for the top bar. */
fun statusLine(s: Status?, signedIn: Boolean): String {
    val pend = if (s != null && s.pending > 0) ", ${s.pending} not synced" else ""
    return when {
        s == null -> "opening…"
        s.denied != null -> "turned away: ${s.denied}"
        s.serverless -> "alone$pend"
        !signedIn -> "signed out$pend"
        s.linked -> "linked$pend"
        else -> "offline" + (s.retryInMs?.let { ", retry in ${(it + 999) / 1000}s" } ?: "") + pend
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HarkenApp(model: Model) {
    val nav = rememberNavController()
    val snackbar = remember { SnackbarHostState() }
    val notice by model.notice.collectAsStateWithLifecycle()
    val status by model.status.collectAsStateWithLifecycle()
    val settings by model.settings.collectAsStateWithLifecycle()

    LaunchedEffect(notice) {
        val n = notice ?: return@LaunchedEffect
        snackbar.showSnackbar(n)
        model.dismissNotice()
    }

    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Harken") },
                actions = { Text(statusLine(status, settings.user != null), modifier = Modifier.padding(end = 16.dp)) },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
        bottomBar = {
            NavigationBar {
                val tabs = listOf(
                    Triple(Routes.LIBRARY, "Library", Icons.Default.Home),
                    Triple(Routes.PLAYLISTS, "Playlists", Icons.AutoMirrored.Filled.List),
                    Triple(Routes.SETTINGS, "Settings", Icons.Default.Settings),
                )
                for ((r, label, icon) in tabs) {
                    val selected = route == r || (r == Routes.PLAYLISTS && route == Routes.PLAYLIST)
                    NavigationBarItem(
                        selected = selected,
                        onClick = {
                            nav.navigate(r) {
                                popUpTo(nav.graph.findStartDestination().id) { saveState = true }
                                launchSingleTop = true
                                restoreState = true
                            }
                        },
                        icon = { Icon(icon, contentDescription = label) },
                        label = { Text(label) },
                    )
                }
            }
        },
    ) { padding ->
        NavHost(nav, startDestination = Routes.LIBRARY, modifier = Modifier.padding(padding)) {
            composable(Routes.LIBRARY) { LibraryScreen(model) }
            composable(Routes.PLAYLISTS) {
                PlaylistsScreen(model, onOpen = { id ->
                    model.select(id)
                    nav.navigate(Routes.playlist(id))
                })
            }
            composable(Routes.PLAYLIST) { back ->
                val hex = back.arguments?.getString("id")
                val id = hex?.let { runCatching { Id.ofHex(it) }.getOrNull() }
                if (id == null) {
                    Text("no such playlist")
                } else {
                    PlaylistScreen(model, id, onBack = { nav.popBackStack() })
                }
            }
            composable(Routes.SETTINGS) { SettingsScreen(model) }
        }
    }
}
